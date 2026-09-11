"""层级聚类 demo(2026-09-11 所有者构想:粗聚/细聚 + 非树状垂直轴)

- 细簇:语义空间 k-means k=48(基本层)
- 粗簇:对 48 个**簇心**再聚类 k=10(纲目层)——簇的原型的原型(递归)
- 命名:细簇/粗簇各取最近知识锚点(wiki_anchor 词表)
- 非树状验证:颜色轴(区域颜色矩 k=6)切片——颜色簇成员横跨多少个语义细簇
"""
import json
from pathlib import Path

import cv2
import numpy as np
from PIL import Image
from sklearn.cluster import KMeans

import wiki_anchor as W  # 复用词表/tokenizer/编码

data = np.load("regions.npz", allow_pickle=False)
vecs = data["vecs"].astype(np.float32)
paths = [str(s) for s in data["paths"]]
photo_idx = data["photo_idx"]
boxes = data["boxes"]
n = len(vecs)

# ---- 语义细簇(48)与簇心 ----
fine = KMeans(n_clusters=48, n_init=10, random_state=0).fit(vecs)
fine_centers = fine.cluster_centers_
fine_labels = fine.labels_

# ---- 粗簇:簇心的簇(k=10)----
coarse = KMeans(n_clusters=10, n_init=10, random_state=0).fit(fine_centers)
coarse_of_fine = coarse.labels_
coarse_centers = coarse.cluster_centers_

# ---- 锚点命名 ----
tok = W.load_tokenizer(str(Path(r"E:\git\MiaoMoryQwQ\.devdata\models") / "vocab.txt"))
import onnxruntime as ort
sess = ort.InferenceSession(str(Path(r"E:\git\MiaoMoryQwQ\.devdata\models") / "text.int8.onnx"),
                            providers=["CPUExecutionProvider"])
anchor_vecs = W.encode_texts(sess, tok, W.CONCEPTS)

def name_of(center: np.ndarray) -> tuple[str, float]:
    d = 1.0 - anchor_vecs @ center
    c = int(np.argmin(d))
    return W.CONCEPTS[c], float(1.0 - d[c])

tree = {"coarse_clusters": []}
for c in range(10):
    fine_ids = np.where(coarse_of_fine == c)[0]
    size = int((np.isin(fine_labels, fine_ids)).sum())
    cname, csim = name_of(coarse_centers[c])
    children = []
    for f in fine_ids:
        fname, fsim = name_of(fine_centers[f])
        children.append({"fine": int(f), "name": fname, "sim": round(fsim, 2),
                         "size": int((fine_labels == f).sum())})
    children.sort(key=lambda x: -x["size"])
    tree["coarse_clusters"].append({
        "coarse": c, "anchor": cname, "conf": round(csim, 2),
        "regions": size, "fine_clusters": children,
    })
tree["coarse_clusters"].sort(key=lambda x: -x["regions"])
print(json.dumps([{k: x[k] for k in ("coarse", "anchor", "conf", "regions")} for x in tree["coarse_clusters"]],
                 ensure_ascii=False, indent=1))

# ---- 非树状验证:颜色轴切片 ----
cache: dict[str, np.ndarray] = {}
color_feats = []
for r in range(n):
    p = paths[photo_idx[r]]
    if p not in cache:
        if len(cache) > 32:
            cache.clear()
        raw = np.fromfile(p, dtype=np.uint8)
        cache[p] = cv2.imdecode(raw, cv2.IMREAD_COLOR) if raw.size else None
    img = cache[p]
    x0, y0, x1, y1 = (int(v) for v in boxes[r])
    c = img[y0:y1, x0:x1] if img is not None else np.zeros((2, 2, 3), np.uint8)
    mean, _ = cv2.meanStdDev(c)
    color_feats.append(np.concatenate([mean.flatten()]) / 255.0)
color_feats = np.stack(color_feats)
color_labels = KMeans(n_clusters=6, n_init=4, random_state=0).fit_predict(color_feats)

cross = []
for col in range(6):
    members = np.where(color_labels == col)[0]
    spread = len(np.unique(fine_labels[members]))
    cross.append({"color_cluster": col, "size": int(len(members)),
                  "spans_fine_clusters": spread})
print(json.dumps(cross, ensure_ascii=False, indent=1))

Path("outputs/hierarchy.json").write_text(
    json.dumps({"tree": tree, "color_axis_cross": cross}, ensure_ascii=False, indent=2),
    encoding="utf-8")
print("saved outputs/hierarchy.json")
