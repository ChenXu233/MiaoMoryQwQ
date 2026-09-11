"""晚交互(patch-MaxSim)vs 单向量余弦(2026-09-11 所有者猜想验证)

DeepSeek-OCR 式多 token 表示 + 快速注意力匹配是否优于单向量余弦?
- DINOv2 ViT-S/14 @224 → 每区域 256 个 patch token(≈ 所有者说的「300 token」量级)
- 两级级联:PCA-64 pooled 签名粗排 top-C → 候选内 patch-MaxSim 精排
  score(q, c) = Σ_i max_j(q_i · c_j)(查询 patch × 候选 patch 的注意力匹配)
- 对照:pooled 余弦直接 top-k(同粗排,不精排)
- 指标:同语义簇 hit@10/hit@50;查询采样 100 个控制算力
"""
import argparse
import json
import time
from pathlib import Path

import cv2
import numpy as np
import torch
from sklearn.cluster import KMeans
from sklearn.decomposition import PCA
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
    ap.add_argument("--queries", type=int, default=100)
    ap.add_argument("--candidates", type=int, default=100)
    ap.add_argument("--k-sem", type=int, default=32)
    ap.add_argument("--out", default="outputs/maxsim_report.json")
    args = ap.parse_args()

    import timm
    model = timm.create_model(args.model, pretrained=True, num_classes=0,
                              img_size=args.img_size, dynamic_img_size=True).eval()
    cfg = model.default_cfg
    mean = np.array(cfg["mean"], dtype=np.float32)
    std = np.array(cfg["std"], dtype=np.float32)

    data = np.load(args.npz, allow_pickle=False)
    vecs = data["vecs"].astype(np.float32)          # pooled CLIP?(不,regions.npz 存 CLIP 512)
    paths = [str(s) for s in data["paths"]]
    photo_idx = data["photo_idx"]
    boxes = data["boxes"]
    n = len(vecs)
    sem = KMeans(n_clusters=args.k_sem, n_init=10, random_state=0).fit_predict(vecs)

    # 语义基准签名 = pooled DINO PCA-64(与 CLIP 解耦,公平对比晚交互增益)
    cache: dict[str, np.ndarray] = {}
    patches_all = []       # f16 patch tokens
    pooled_list = []
    rows_meta = []
    rng = np.random.default_rng(0)
    t0 = time.time()
    for r in range(n):
        p = paths[photo_idx[r]]
        if p not in cache:
            if len(cache) > 24:
                cache.clear()
            cache[p] = imread_unicode(p)
        img = cache[p]
        if img is None:
            continue
        x0, y0, x1, y1 = (int(v) for v in boxes[r])
        c = img[y0:y1, x0:x1]
        if c.size == 0:
            continue
        c = cv2.cvtColor(c, cv2.COLOR_BGR2RGB)
        c = cv2.resize(c, (args.img_size, args.img_size), interpolation=cv2.INTER_AREA).astype(np.float32) / 255.0
        c = (c - mean) / std
        t = torch.from_numpy(np.transpose(c, (2, 0, 1))).unsqueeze(0)
        with torch.inference_mode():
            feats = model.forward_features(t)          # 1 × (1+P) × D
        pf = feats[0, 1:, :].cpu().numpy().astype(np.float16)   # 去 CLS,留 P patches
        pooled = pf.mean(axis=0).astype(np.float32)
        pooled /= np.linalg.norm(pooled) + 1e-9
        patches_all.append(pf)
        pooled_list.append(pooled)
        rows_meta.append(r)
        if (len(pooled_list)) % 500 == 0:
            print(f"{len(pooled_list)}/{n} ({time.time() - t0:.0f}s)")

    patches = np.stack(patches_all)                    # N × P × D f16
    pooled = np.stack(pooled_list)
    N, P, D = patches.shape
    print(f"patch tokens: {patches.shape} f16")

    sig = PCA(n_components=64, random_state=0).fit_transform(pooled.astype(np.float32))

    # 查询/候选采样(不相交优先,允许重叠但排除自身)
    q_idx = rng.choice(N, size=min(args.queries, N), replace=False)
    labels_q = sem[q_idx]

    hits_pooled10, hits_pooled50 = [], []
    hits_maxsim10, hits_maxsim50 = [], []
    t0 = time.time()
    for qi, r in enumerate(q_idx):
        d_sig = np.linalg.norm(sig - sig[qi], axis=1)
        d_sig[r] = 1e9  # 排除自身
        cand = np.argsort(d_sig)[: args.candidates]
        # pooled 余弦直接 top-k(在候选内)
        cos = pooled[cand] @ pooled[qi]
        order_cos = cand[np.argsort(-cos)]
        hits_pooled10.append(float(np.mean(sem[order_cos[:10]] == sem[r])))
        hits_pooled50.append(float(np.mean(sem[order_cos[:50]] == sem[r])))
        # patch-MaxSim 精排(候选内)
        Q = patches[r].astype(np.float32)               # P×D
        scores = np.empty(len(cand), dtype=np.float32)
        for j, c in enumerate(cand):
            Cm = patches[c].astype(np.float32)          # P×D
            mm = Q @ Cm.T                               # P×P
            scores[j] = mm.max(axis=1).sum()            # Σ_i max_j
        order_ms = cand[np.argsort(-scores)]
        hits_maxsim10.append(float(np.mean(sem[order_ms[:10]] == sem[r])))
        hits_maxsim50.append(float(np.mean(sem[order_ms[:50]] == sem[r])))
        if (qi + 1) % 25 == 0:
            print(f"query {qi+1}/{len(q_idx)} ({time.time()-t0:.0f}s)")

    out = {
        "patch_grid": int(P ** 0.5), "queries": len(q_idx), "candidates_per_q": args.candidates,
        "pooled_cosine": {"hit@10": round(float(np.mean(hits_pooled10)), 3),
                          "hit@50": round(float(np.mean(hits_pooled50)), 3)},
        "patch_maxsim": {"hit@10": round(float(np.mean(hits_maxsim10)), 3),
                         "hit@50": round(float(np.mean(hits_maxsim50)), 3)},
        "note": "级联:PCA-64 pooled 签名粗排 top-C → MaxSim 精排;同语义簇命中率",
    }
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    Path(args.out).write_text(json.dumps(out, indent=2), encoding="utf-8")
    print(json.dumps(out, indent=2))


if __name__ == "__main__":
    main()
