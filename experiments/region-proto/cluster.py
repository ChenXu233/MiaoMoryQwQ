"""阶段三:全局 k-means 聚类(自然语义类)+ 簇可视化拼图

- 输入 regions.npz(extract.py 产物)
- L2 归一化空间 k-means(欧氏 ≈ 余弦),k 可调
- 输出:outputs/cluster_XX.jpg(每簇最多 12 个区域 crop 的 4×3 网格,
  按与簇中心距离升序 = 最代表该簇的排前)+ cluster_report.json
"""
import argparse
import json
from pathlib import Path

import cv2
import numpy as np
from sklearn.cluster import KMeans

IMG_EXTS = {".jpg", ".jpeg", ".png", ".webp"}


def crop_from_photo(path: str, x0: int, y0: int, x1: int, y1: int, size: int = 112) -> np.ndarray | None:
    img = cv2.imread(path)
    if img is None:
        return None
    c = img[y0:y1, x0:x1]
    if c.size == 0:
        return None
    return cv2.resize(c, (size, size), interpolation=cv2.INTER_AREA)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--npz", default="regions.npz")
    ap.add_argument("--k", type=int, default=32)
    ap.add_argument("--photos", default=r"F:\相册\2022.10-2024.7\相机")
    ap.add_argument("--out", default="outputs")
    ap.add_argument("--per-cluster", type=int, default=12)
    args = ap.parse_args()

    data = np.load(args.npz, allow_pickle=False)
    vecs = data["vecs"].astype(np.float32)
    paths = data["paths"]
    boxes = data["boxes"]
    n = len(vecs)
    print(f"regions: {n}, dim: {vecs.shape[1]}")

    km = KMeans(n_clusters=args.k, n_init=10, random_state=0).fit(vecs)
    labels = km.labels_
    centers = km.cluster_centers_

    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    # 照片读盘缓存(每簇的 crop 可能散布在不同照片)
    cache: dict[str, np.ndarray] = {}

    def get_crop(p: str, x0: int, y0: int, x1: int, y1: int, size: int = 112) -> np.ndarray | None:
        if p not in cache:
            if len(cache) > 32:
                cache.clear()
            cache[p] = cv2.imread(p)
        img = cache[p]
        if img is None:
            return None
        c = img[y0:y1, x0:x1]
        if c.size == 0:
            return None
        return cv2.resize(c, (size, size), interpolation=cv2.INTER_AREA)

    report = {"k": args.k, "regions": n, "clusters": []}
    order = np.argsort(labels)
    for c in range(args.k):
        idx = order[labels[order] == c]
        # 按与簇中心距离升序:最代表的排前
        d = np.linalg.norm(vecs[idx] - centers[c], axis=1)
        idx = idx[np.argsort(d)]
        picks = idx[: args.per_cluster]
        tiles = []
        for r in picks:
            p = str(paths[r])
            x0, y0, x1, y1 = (int(v) for v in boxes[r])
            tile = get_crop(p, x0, y0, x1, y1)
            if tile is not None:
                tiles.append(tile)
        if not tiles:
            continue
        rows = []
        for r in range(0, len(tiles), 4):
            row = tiles[r : r + 4]
            while len(row) < 4:
                row.append(np.full((112, 112, 3), 245, np.uint8))
            rows.append(np.hstack(row))
        grid = np.vstack(rows)
        cv2.putText(grid, f"cluster {c}  n={len(idx)}", (6, 20),
                    cv2.FONT_HERSHEY_SIMPLEX, 0.55, (0, 0, 0), 3, cv2.LINE_AA)
        cv2.putText(grid, f"cluster {c}  n={len(idx)}", (6, 20),
                    cv2.FONT_HERSHEY_SIMPLEX, 0.55, (255, 255, 255), 1, cv2.LINE_AA)
        cv2.imwrite(str(out_dir / f"cluster_{c:02d}.jpg"), grid, [cv2.IMWRITE_JPEG_QUALITY, 90])
        report["clusters"].append({"cluster": c, "size": int(len(idx)),
                                   "sample_photo": str(paths[picks[0]])})

    sizes = np.bincount(labels, minlength=args.k)
    report["size_stats"] = {
        "min": int(sizes.min()), "max": int(sizes.max()),
        "mean": round(float(sizes.mean()), 1),
        "empty": int((sizes == 0).sum()),
    }
    (out_dir / "cluster_report.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(report["size_stats"], indent=2))
    print(f"grids -> {out_dir}/cluster_XX.jpg")


if __name__ == "__main__":
    main()
