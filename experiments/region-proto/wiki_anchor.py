"""知识锚点聚类原型(2026-09-11 所有者构想:引入百科知识先验给聚类向导)

- 锚点 = 中文视觉概念词条(原型阶段手工词表;正式版 = 中文 Wikipedia 词条/分类树)
  → Chinese-CLIP text.int8.onnx 编码 → 512 维锚点向量(离线一次)
- 区域向量(已有 regions.npz)→ 最近锚点(1-cos)→ **带名字的簇**
- 评价:每锚点 top-8 区域拼图(锚点名即簇名,命名对不对一眼可判)
- 正式版词表来源:中文 Wikipedia 高频视觉概念词条(标题+首段),按分类树剪枝
"""
import argparse
import json
from pathlib import Path

import cv2
import numpy as np
import onnxruntime as ort
from tokenizers import Tokenizer
from tokenizers.models import WordPiece
from tokenizers.pre_tokenizers import Whitespace

CLS_ID, SEP_ID, PAD_ID = 101, 102, 0
CTX = 52

# 中文视觉概念词表(原型;正式版用 Wikipedia 词条)
CONCEPTS = [
    "天空", "白云", "日出", "日落", "夜晚", "星空", "山", "雪山", "海", "河流",
    "湖泊", "瀑布", "森林", "树林", "竹子", "樱花", "花", "玫瑰花", "草地", "树叶",
    "树影", "稻田", "麦田", "农田", "村庄", "城市", "街道", "马路", "桥梁", "高楼",
    "天际线", "古镇", "寺庙", "塔", "城墙", "台阶", "走廊", "栏杆", "围墙", "大门",
    "窗户", "教室", "黑板", "课桌", "宿舍", "食堂", "图书馆", "操场", "跑道", "篮球场",
    "办公室", "厨房", "卧室", "超市", "地铁站", "火车站", "汽车", "自行车", "电动车", "公交车",
    "火车", "飞机", "船", "人物", "学生", "自拍", "合影", "背影", "女孩", "男孩",
    "人群", "猫", "狗", "鸟", "鱼", "昆虫", "食物", "面条", "米饭", "水果",
    "奶茶", "蛋糕", "烧烤", "烟花", "灯光", "霓虹灯", "影子", "倒影", "雾", "雨",
    "雨后", "秋天", "冬天", "雪", "落叶", "桌面", "电脑", "手机", "书本", "纸",
    "椅子", "床", "墙", "地面", "石头", "沙子", "泥土", "行李箱", "书包", "衣服",
    "红色", "蓝色", "绿色", "黄色", "白色", "黑色", "粉色", "紫色", "灰色", "彩虹",
]


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


def encode_texts(sess: ort.InferenceSession, tok: Tokenizer, texts: list[str]) -> np.ndarray:
    """与 crates/embed/tokenizer.rs 同语义:lowercase → whitespace+wordpiece → CLS/SEP → pad 52"""
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


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--npz", default="regions.npz")
    ap.add_argument("--models", default=r"E:\git\MiaoMoryQwQ\.devdata\models")
    ap.add_argument("--photos", default=r"F:\相册\2022.10-2024.7\相机")
    ap.add_argument("--topk-per-anchor", type=int, default=8)
    ap.add_argument("--tau-report", type=float, default=0.30)
    ap.add_argument("--out", default="outputs")
    args = ap.parse_args()

    data = np.load(args.npz, allow_pickle=False)
    vecs = data["vecs"].astype(np.float32)
    paths = [str(s) for s in data["paths"]]
    photo_idx = data["photo_idx"]
    boxes = data["boxes"]

    tok = load_tokenizer(str(Path(args.models) / "vocab.txt"))
    sess = ort.InferenceSession(str(Path(args.models) / "text.int8.onnx"),
                                providers=["CPUExecutionProvider"])
    anchors = encode_texts(sess, tok, CONCEPTS)
    print(f"anchors: {len(CONCEPTS)}, regions: {len(vecs)}")

    sims = vecs @ anchors.T  # 5142 × A
    assign = np.argmax(sims, axis=1)
    conf = np.max(sims, axis=1)  # 1-cos 相似度
    assigned = int((conf >= 1 - args.tau_report).sum())
    print(f"assigned within τ={args.tau_report}: {assigned}/{len(vecs)} ({assigned/len(vecs):.0%})")

    # 每锚点 top 区域拼图(带锚点名)
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    cache: dict[str, np.ndarray] = {}

    def get_crop(p, x0, y0, x1, y1, size=112):
        if p not in cache:
            if len(cache) > 32:
                cache.clear()
            raw = np.fromfile(p, dtype=np.uint8)
            cache[p] = cv2.imdecode(raw, cv2.IMREAD_COLOR) if raw.size else None
        img = cache[p]
        if img is None:
            return None
        c = img[y0:y1, x0:x1]
        return cv2.resize(c, (size, size), interpolation=cv2.INTER_AREA) if c.size else None

    report = {"anchors": len(CONCEPTS), "regions": len(vecs),
              "assigned_within_tau": assigned, "clusters": []}
    for a, name in enumerate(CONCEPTS):
        idx = np.where(assign == a)[0]
        if len(idx) == 0:
            continue
        idx = idx[np.argsort(-conf[idx])]
        picks = idx[: args.topk_per_anchor]
        tiles = []
        for r in picks:
            p = paths[photo_idx[r]]
            x0, y0, x1, y1 = (int(v) for v in boxes[r])
            tile = get_crop(p, x0, y0, x1, y1)
            if tile is not None:
                tiles.append(tile)
        if not tiles:
            continue
        rows = []
        for r0 in range(0, len(tiles), 4):
            row = tiles[r0:r0 + 4]
            while len(row) < 4:
                row.append(np.full((112, 112, 3), 245, np.uint8))
            rows.append(np.hstack(row))
        grid = np.vstack(rows)
        cv2.putText(grid, f"{name}  n={len(idx)}", (6, 20),
                    cv2.FONT_HERSHEY_SIMPLEX, 0.55, (0, 0, 0), 3, cv2.LINE_AA)
        cv2.putText(grid, f"{name}  n={len(idx)}", (6, 20),
                    cv2.FONT_HERSHEY_SIMPLEX, 0.55, (255, 255, 255), 1, cv2.LINE_AA)
        cv2.imwrite(str(out_dir / f"anchor_{a:03d}_{name}.jpg"), grid,
                    [cv2.IMWRITE_JPEG_QUALITY, 90])
        report["clusters"].append({"anchor": name, "size": int(len(idx)),
                                   "mean_sim": round(float(conf[idx].mean()), 3)})
    sizes = sorted((c["size"] for c in report["clusters"]), reverse=True)
    report["size_top10"] = sizes[:10]
    report["nonempty_anchors"] = len(report["clusters"])
    (out_dir / "anchor_report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2),
                                               encoding="utf-8")
    print(json.dumps({k: report[k] for k in ("nonempty_anchors", "size_top10")}, ensure_ascii=False))


if __name__ == "__main__":
    main()
