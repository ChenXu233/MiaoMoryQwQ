"""CLIP 偏移检验(2026-09-11 所有者质疑:锚点法是分类不是聚类,聚类是否只是 CLIP 投影?)

用 DINOv2(纯自监督视觉特征,无文本对齐、无词表先验)独立提取区域特征,
对比两个独立特征空间的聚类结构:
- NMI(DINO-k32, CLIP-k32):高 → 自然类是数据固有的(双模型一致涌现);低 → CLIP 偏移主导
- 附频域特征做第三参照
"""
import argparse
import time
import json
from pathlib import Path

import cv2
import numpy as np
import torch
from sklearn.cluster import KMeans
from sklearn.metrics import normalized_mutual_info_score as nmi


def imread_unicode(path: str):
    raw = np.fromfile(path, dtype=np.uint8)
    return cv2.imdecode(raw, cv2.IMREAD_COLOR) if raw.size else None


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--npz", default="regions.npz")
    ap.add_argument("--photos", default=r"F:\相册\2022.10-2024.7\相机")
    ap.add_argument("--model", default="vit_small_patch14_dinov2.lvd142m")
    ap.add_argument("--img-size", type=int, default=224)
    ap.add_argument("--limit-regions", type=int, default=0, help="0=全部")
    ap.add_argument("--k", type=int, default=32)
    ap.add_argument("--out", default="outputs/dino_check.json")
    args = ap.parse_args()

    import timm
    model = timm.create_model(args.model, pretrained=True, num_classes=0, img_size=args.img_size).eval()
    cfg = model.default_cfg
    mean = np.array(cfg["mean"], dtype=np.float32)
    std = np.array(cfg["std"], dtype=np.float32)
    size = args.img_size

    data = np.load(args.npz, allow_pickle=False)
    vecs_clip = data["vecs"].astype(np.float32)
    paths = [str(s) for s in data["paths"]]
    photo_idx = data["photo_idx"]
    boxes = data["boxes"]
    n = len(vecs_clip) if args.limit_regions == 0 else min(args.limit_regions, len(vecs_clip))

    cache: dict[str, np.ndarray] = {}
    dino_feats = []
    t0 = time_start = time.time()
    for r in range(n):
        p = paths[photo_idx[r]]
        if p not in cache:
            if len(cache) > 24:
                cache.clear()
            cache[p] = imread_unicode(p)
        img = cache[p]
        if img is None:
            dino_feats.append(np.zeros(384, np.float32))
            continue
        x0, y0, x1, y1 = (int(v) for v in boxes[r])
        c = img[y0:y1, x0:x1]
        if c.size == 0:
            dino_feats.append(np.zeros(384, np.float32))
            continue
        c = cv2.cvtColor(c, cv2.COLOR_BGR2RGB)
        c = cv2.resize(c, (size, size), interpolation=cv2.INTER_AREA).astype(np.float32) / 255.0
        c = (c - mean) / std
        t = torch.from_numpy(np.transpose(c, (2, 0, 1))).unsqueeze(0)
        with torch.inference_mode():
            f = model(t)
        f = np.asarray(f, dtype=np.float32).reshape(-1)
        dino_feats.append(f / (np.linalg.norm(f) + 1e-9))
        if (r + 1) % 500 == 0:
            print(f"{r+1}/{n} ({time.time() - t0:.0f}s)")
    dino = np.stack(dino_feats)
    print(f"dino feats: {dino.shape}, {time.time() - t0:.0f}s")

    lab_clip = KMeans(n_clusters=args.k, n_init=10, random_state=0).fit_predict(vecs_clip[:n])
    lab_dino = KMeans(n_clusters=args.k, n_init=10, random_state=0).fit_predict(dino)
    out = {
        "n": n, "k": args.k,
        "nmi_clip_vs_dino": round(float(nmi(lab_clip, lab_dino)), 3),
        "note": "高 → 自然类为数据固有(独立双模型一致涌现);低 → CLIP 偏移主导",
    }
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    Path(args.out).write_text(json.dumps(out, indent=2), encoding="utf-8")
    np.savez_compressed("dino.npz", vecs=dino)
    print(json.dumps(out, indent=2))


if __name__ == "__main__":
    main()
