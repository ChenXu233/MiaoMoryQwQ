"""区域级索引检索效果对比测试(2026-09-12)

同一批中文查询词,两种检索路径各取 top-8,导出缩略图拼图对比:
  A. 整图向量 KNN(旧方案,vec_assets)
  B. 区域向量 KNN(新方案,vec_regions → asset 聚合,区域级 MaxSim)
另输出各查询的距离分布(窄带对比)。
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

QUERIES = ["花", "天空", "日落", "狗", "猫", "食物", "建筑", "人群", "山", "树叶"]


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


def load_clip_vecs(cur):
    """vec_assets 全量(asset_id, embedding)——整图方案"""
    cur.execute("LOAD?") if False else None
    rows = cur.execute("SELECT asset_id, embedding FROM vec_assets").fetchall()
    return {int(r[0]): np.frombuffer(r[1], dtype=np.float32) for r in rows}


def load_region_vecs(cur):
    rows = cur.execute(
        "SELECT v.region_id, v.embedding, r.asset_id, r.cluster_id FROM vec_regions v "
        "JOIN regions r ON r.region_id = v.region_id WHERE r.region_idx >= 0"
    ).fetchall()
    vecs, asset_of = [], []
    metas = []
    for rid, emb, aid, cid in rows:
        vecs.append(np.frombuffer(emb, dtype=np.float32))
        asset_of.append(int(aid))
        metas.append({"region_id": int(rid), "asset_id": int(aid), "cluster": cid})
    return np.stack(vecs), np.array(asset_of), metas


def asset_thumb(cur, workspace_thumbs: Path, asset_id: int) -> np.ndarray | None:
    row = cur.execute("SELECT thumb_key FROM assets WHERE asset_id=?", (asset_id,)).fetchone()
    if not row or not row[0]:
        return None
    p = workspace_thumbs / row[0].replace("/", "\\") if "\\" in row[0] else workspace_thumbs / row[0]
    for cand in (p, Path(str(p).replace("/", "\\"))):
        raw = np.fromfile(cand, dtype=np.uint8)
        if raw.size:
            img = cv2.imdecode(raw, cv2.IMREAD_COLOR)
            if img is not None:
                return cv2.resize(img, (112, 112), interpolation=cv2.INTER_AREA)
    return None


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
    ap.add_argument("--queries", default=",".join(QUERIES))
    ap.add_argument("--k", type=int, default=8)
    ap.add_argument("--out", default="outputs/retrieval_compare")
    args = ap.parse_args()

    con = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
    con.enable_load_extension(True)
    con.load_extension(sqlite_vec.loadable_path())

    tok = load_tokenizer(str(Path(args.models) / "vocab.txt"))
    sess = ort.InferenceSession(str(Path(args.models) / "text.int8.onnx"),
                                providers=["CPUExecutionProvider"])
    qvecs = encode_text(sess, tok, args.queries.split(","))

    thumbs_dir = Path(args.thumbs)
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    report = []
    for qi, q in enumerate(args.queries.split(",")):
        qv = qvecs[qi]

        # A. 整图 KNN
        cur = con.execute(
            "SELECT asset_id, distance FROM vec_assets WHERE embedding MATCH ?1 AND k = ?2",
            (qv.tobytes(), 16),
        ).fetchall()
        a_assets = [int(r[0]) for r in cur]
        a_dists = [float(r[1]) for r in cur]

        # B. 区域 KNN → asset 聚合(MaxSim:每 asset 取最小距离)
        cur = con.execute(
            "SELECT r.asset_id, v.distance, v.region_id FROM vec_regions v "
            "JOIN regions r ON r.region_id = v.region_id WHERE v.embedding MATCH ?1 AND k = ?2",
            (qv.tobytes(), 64),
        ).fetchall()
        best: dict[int, float] = {}
        best_region: dict[int, int] = {}
        for aid, dist, rid in cur:
            aid = int(aid)
            d = float(dist)
            if d < best.get(aid, 1e9):
                best[aid] = d
                best_region[aid] = int(rid)
        b_assets = sorted(best, key=lambda x: best[x])[: 8]
        b_dists = [round(best[a], 3) for a in b_assets]

        tiles_a = [t for t in (asset_thumb(con, thumbs_dir, a) for a in a_assets) if t is not None]
        tiles_b = [t for t in (asset_thumb(con, thumbs_dir, a) for a in b_assets) if t is not None]
        cv2.imwrite(str(out_dir / f"q{qi}_A_full_{q}.jpg"),
                    grid(tiles_a, f"FULL  {q}  d=[{min(a_dists):.2f}..{max(a_dists):.2f}]" if a_dists else f"FULL {q}"),
                    [cv2.IMWRITE_JPEG_QUALITY, 90])
        cv2.imwrite(str(out_dir / f"q{qi}_B_region_{q}.jpg"),
                    grid(tiles_b, f"REGION {q}  d=[{min(b_dists):.2f}..{max(b_dists):.2f}]" if b_dists else f"REGION {q}"),
                    [cv2.IMWRITE_JPEG_QUALITY, 90])
        report.append({
            "query": q,
            "full_top8_assets": a_assets[:8],
            "full_dists": [round(d, 3) for d in a_dists[:8]],
            "region_top8_assets": b_assets,
            "region_dists": b_dists,
        })
        print(json.dumps(report[-1], ensure_ascii=False))

    Path(args.out, "compare_report.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print("grids ->", out_dir)


if __name__ == "__main__":
    main()
