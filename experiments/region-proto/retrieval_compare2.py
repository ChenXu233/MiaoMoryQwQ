"""检索质量视觉对比(区域向量[regions.npz] vs 整图向量[vec_assets])

同一批中文查询词,两种索引各取 top-8 缩略图拼图:
  A. 整图向量 KNN(vec_assets,DB,每 asset 一向量)
  B. 区域向量 KNN(regions.npz,每 asset 多区域,区域级 MaxSim 聚合)
"""
import argparse
import json
import sqlite3
from pathlib import Path

import cv2
import numpy as np
import onnxruntime as ort
import sqlite_vec
from tokenizers import Tokenizer
from tokenizers.models import WordPiece
from tokenizers.pre_tokenizers import Whitespace

CLS_ID, SEP_ID, PAD_ID = 101, 102, 0
CTX = 52
QUERIES = ["花", "天空", "日落", "狗", "人群", "山", "树叶", "食物"]


def load_tokenizer(vocab_path: str) -> Tokenizer:
    vocab = {}
    with open(vocab_path, encoding="utf-8") as f:
        for i, line in enumerate(f):
            tok = line.rstrip("\n")
            if tok:
                vocab[tok] = i
    tok = Tokenizer(WordPiece(vocab, unk_token="[UNK]"))
    tok.pre_tokenizer = Whitespace()
    return tok


def encode_text(sess, tok, texts):
    ids_batch, att_batch = [], []
    for t in texts:
        enc = tok.encode(t.lower(), add_special_tokens=False)
        ids = [CLS_ID] + enc.ids[: CTX - 2] + [SEP_ID]
        att = [1] * len(ids)
        pad = CTX - len(ids)
        ids_batch.append(ids + [PAD_ID] * pad)
        att_batch.append(att + [0] * pad)
    tt = np.zeros((len(texts), CTX), dtype=np.int64)
    out = sess.run(None, {
        sess.get_inputs()[0].name: np.array(ids_batch, dtype=np.int64),
        sess.get_inputs()[1].name: np.array(att_batch, dtype=np.int64),
        sess.get_inputs()[2].name: tt,
    })[0]
    v = np.asarray(out, dtype=np.float32).reshape(len(texts), -1)
    return v / (np.linalg.norm(v, axis=1, keepdims=True) + 1e-9)


def thumb_by_key(cur, thumbs: Path, key: str) -> np.ndarray | None:
    row = cur.execute("SELECT thumb_key FROM assets WHERE storage_key=?", (key,)).fetchone()
    if not row or not row[0]:
        return None
    p = thumbs / row[0].replace("/", "\\")
    raw = np.fromfile(p, dtype=np.uint8)
    img = cv2.imdecode(raw, cv2.IMREAD_COLOR) if raw.size else None
    if img is None:
        return None
    return cv2.resize(img, (112, 112), interpolation=cv2.INTER_AREA)


def grid(tiles, title):
    rows = []
    for r0 in range(0, len(tiles), 4):
        row = tiles[r0:r0 + 4]
        while len(row) < 4:
            row.append(np.full((112, 112, 3), 245, np.uint8))
        rows.append(np.hstack(row))
    g = np.vstack(rows)
    cv2.putText(g, title, (6, 20), cv2.FONT_HERSHEY_SIMPLEX, 0.55, (0, 0, 0), 3, cv2.LINE_AA)
    cv2.putText(g, title, (6, 20), cv2.FONT_HERSHEY_SIMPLEX, 0.55, (255, 255, 255), 1, cv2.LINE_AA)
    return g


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default=r"E:\git\MiaoMoryQwQ\.devdata\index.db")
    ap.add_argument("--models", default=r"E:\git\MiaoMoryQwQ\.devdata\models")
    ap.add_argument("--thumbs", default=r"E:\git\MiaoMoryQwQ\.devdata\thumbs")
    ap.add_argument("--npz", default="regions.npz")
    ap.add_argument("--queries", default=",".join(QUERIES))
    ap.add_argument("--out", default="outputs/quality_compare")
    args = ap.parse_args()

    con = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
    con.enable_load_extension(True)
    con.load_extension(sqlite_vec.loadable_path())
    key2aid = {k: int(a) for a, k in
               con.execute("SELECT asset_id, storage_key FROM assets").fetchall()}
    key2thumb = {k: t for k, t in
                 con.execute("SELECT storage_key, thumb_key FROM assets WHERE thumb_key IS NOT NULL")}

    tok = load_tokenizer(str(Path(args.models) / "vocab.txt"))
    sess = ort.InferenceSession(str(Path(args.models) / "text.int8.onnx"),
                                providers=["CPUExecutionProvider"])
    queries = args.queries.split(",")
    qvecs = encode_text(sess, tok, queries)
    thumbs = Path(args.thumbs)
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    # ---- 区域索引(regions.npz:每区域一行,key=storage_key)----
    data = np.load(args.npz, allow_pickle=False)
    rvecs = data["vecs"].astype(np.float32)
    r_photos = [str(s) for s in data["paths"]]      # 照片级路径数组
    r_photo_idx = data["photo_idx"]                  # 区域 → 照片下标
    # 区域行 → 所属照片路径 → asset_id
    r_aids = [key2aid.get(r_photos[pi], -1) for pi in r_photo_idx]
    r_region_keys = [r_photos[pi] for pi in r_photo_idx]
    print(f"region vecs: {rvecs.shape}")

    for qi, q in enumerate(queries):
        qv = qvecs[qi]
        # A. 整图 KNN(排除旧测试夹具 folder 1/2/3:只保留有 GPS 语义的主库照片不必要,直接全库)
        cur = con.execute(
            "SELECT asset_id, distance FROM vec_assets WHERE embedding MATCH ?1 AND k = ?2",
            (qv.tobytes(), 8),
        ).fetchall()
        a_assets = [int(r[0]) for r in cur]
        # B. 区域 KNN → asset 聚合(MaxSim)
        sims = rvecs @ qv
        order = np.argsort(-sims)
        best: dict[int, float] = {}
        best_key: dict[int, str] = {}
        for r in order:
            aid = r_aids[r]
            if aid < 0:
                continue
            if aid not in best:
                best[aid] = float(sims[r])
                best_key[aid] = r_region_keys[r]
            if len(best) >= 8:
                break
        b_assets = sorted(best, key=lambda x: -best[x])

        tiles_a = [t for t in (thumb_by_key_by_aid(cur, thumbs, a) for a in a_assets) if t is not None] \
            if False else [t for t in (thumb_by_aid(con, thumbs, a) for a in a_assets) if t is not None]
        tiles_b = [t for t in (thumb_by_aid(con, thumbs, a) for a in b_assets) if t is not None]

        da = [round(float(d), 3) for _, d in
              con.execute("SELECT asset_id, distance FROM vec_assets WHERE embedding MATCH ?1 AND k = ?2",
                          (qv.tobytes(), 8))]
        cv2.imwrite(str(out_dir / f"q{qi}_A_full_{q}.jpg"),
                    grid(tiles_a, f"FULL {q}"), [cv2.IMWRITE_JPEG_QUALITY, 90])
        cv2.imwrite(str(out_dir / f"q{qi}_B_region_{q}.jpg"),
                    grid(tiles_b, f"REGION {q}"), [cv2.IMWRITE_JPEG_QUALITY, 90])
        print(f"{q}: full_assets={a_assets[:8]}")
        print(f"{q}: region_assets={b_assets[:8]}")


def thumb_by_aid(con, thumbs: Path, aid: int):
    row = con.execute("SELECT thumb_key FROM assets WHERE asset_id=?", (aid,)).fetchone()
    if not row or not row[0]:
        return None
    p = thumbs / row[0].replace("/", "\\")
    raw = np.fromfile(p, dtype=np.uint8)
    img = cv2.imdecode(raw, cv2.IMREAD_COLOR) if raw.size else None
    if img is None:
        return None
    return cv2.resize(img, (112, 112), interpolation=cv2.INTER_AREA)


if __name__ == "__main__":
    main()
