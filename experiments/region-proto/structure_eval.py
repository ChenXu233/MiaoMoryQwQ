"""验证所有者的分层假设:「结构空间聚类 → 映射语义」是否保语义纯度(2026-09-11)

- 结构特征(便宜,近零成本):区域 crop → 32×32 灰度 → FFT 幅度谱(log1p,中心化)
  → 8×8 低频系数 = 64 维;另附 4×4 均值亮度/颜色矩,共 ~80 维
- 评估:结构空间 k-means(k=64)的簇标签 vs 语义空间(CLIP 512 维)k-means(k=32)
  标签的 **NMI / 纯度**;对照:随机标签(≈0)与 CLIP 空间自身 k=64 vs k=32(上限)
- 判读:NMI 越高 → 结构簇语义纯度越高 → 「先便宜聚类、语义只标代表」越可行
"""
import argparse
import json
from pathlib import Path

import cv2
import numpy as np
from sklearn.cluster import KMeans
from sklearn.metrics import normalized_mutual_info_score as nmi


def imread_unicode(path: str) -> np.ndarray | None:
    raw = np.fromfile(path, dtype=np.uint8)
    return cv2.imdecode(raw, cv2.IMREAD_COLOR) if raw.size else None


def struct_feature(img_bgr: np.ndarray, box: tuple[int, int, int, int]) -> np.ndarray:
    x0, y0, x1, y1 = box
    c = img_bgr[y0:y1, x0:x1]
    if c.size == 0:
        return None
    g = cv2.cvtColor(c, cv2.COLOR_BGR2GRAY)
    g = cv2.resize(g, (32, 32), interpolation=cv2.INTER_AREA).astype(np.float32)
    # FFT 幅度谱(log 压缩,中心化,取 8×8 低频块)
    f = np.fft.fftshift(np.fft.fft2(g))
    mag = np.log1p(np.abs(f))[12:20, 12:20].flatten()
    mag /= mag.sum() + 1e-6
    # 空间低频:4×4 均值池化(亮度布局)
    low = cv2.resize(g, (4, 4), interpolation=cv2.INTER_AREA).flatten() / 255.0
    # 颜色矩:3 通道 mean/std
    mean, std = cv2.meanStdDev(c)
    color = np.concatenate([mean.flatten(), std.flatten()]) / 255.0
    return np.concatenate([mag * 8.0, low * 0.5, color])  # 频域为主,布局/颜色为辅


def purity(labels_true: np.ndarray, labels_pred: np.ndarray, k_true: int) -> float:
    """每个预测簇取其多数真标签的占比(按最大簇归一平均)"""
    total = 0
    for c in np.unique(labels_pred):
        members = labels_true[labels_pred == c]
        total += np.bincount(members, minlength=k_true).max()
    return total / len(labels_true)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--npz", default="regions.npz")
    ap.add_argument("--photos", default=r"F:\相册\2022.10-2024.7\相机")
    ap.add_argument("--k-struct", type=int, default=64)
    ap.add_argument("--k-sem", type=int, default=32)
    ap.add_argument("--out", default="outputs/structure_eval.json")
    args = ap.parse_args()

    data = np.load(args.npz, allow_pickle=False)
    paths = [str(s) for s in data["paths"]]
    photo_idx = data["photo_idx"]
    boxes = data["boxes"]
    sem_vecs = data["vecs"].astype(np.float32)

    cache: dict[str, np.ndarray] = {}
    feats: list[np.ndarray] = []
    for r in range(len(sem_vecs)):
        p = paths[photo_idx[r]]
        if p not in cache:
            if len(cache) > 32:
                cache.clear()
            cache[p] = imread_unicode(p)
        img = cache[p]
        f = None if img is None else struct_feature(img, tuple(int(v) for v in boxes[r]))
        feats.append(f if f is not None else np.zeros(80, np.float32))
    X = np.stack(feats).astype(np.float32)
    print(f"structure features: {X.shape}")

    sem_labels = KMeans(n_clusters=args.k_sem, n_init=10, random_state=0).fit_predict(sem_vecs)
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)

    result: dict = {"k_struct": args.k_struct, "k_sem": args.k_sem}
    for k in (16, 32, 64, 128):
        st_labels = KMeans(n_clusters=k, n_init=10, random_state=0).fit_predict(X)
        result[f"nmi_struct_k{k}"] = round(float(nmi(sem_labels, st_labels)), 3)
        result[f"purity_struct_k{k}"] = round(purity(sem_labels, st_labels, args.k_sem), 3)
    # 对照:语义空间自身细分(上限参考)与随机
    sem64 = KMeans(n_clusters=64, n_init=10, random_state=0).fit_predict(sem_vecs)
    result["nmi_clip_self_k64"] = round(float(nmi(sem_labels, sem64)), 3)
    result["purity_clip_self_k64"] = round(purity(sem_labels, sem64, args.k_sem), 3)
    result["nmi_random_k64"] = round(float(nmi(sem_labels, np.random.randint(0, 64, len(sem_labels)))), 3)
    result["note"] = "purity = 结构簇内语义标签众数占比;越高 → 「便宜聚类+语义代表」可行"

    Path(args.out).write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
