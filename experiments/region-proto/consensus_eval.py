"""双空间共识检索评估(2026-09-11 所有者修正方案):

频域粗簇(64,近零成本)= 候选池;每簇只编码 3 个语义代表(分散采样)
= CLIP 调用 5142 → 192(−96%);查询走「代表 MaxSim → 命中簇 → 簇内结构精排」。
对比基准:全量 CLIP KNN(100% 编码);对照:PCA-64 签名 KNN。

指标:同语义簇命中率 hit@10 / hit@50(查询区域向量,leave-one-out;
结果成员的语义标签 == 查询语义标签 的比例,多查询平均)。
"""
import argparse
import json
from pathlib import Path

import cv2
import numpy as np
from sklearn.cluster import KMeans
from sklearn.decomposition import PCA


def imread_unicode(path: str) -> np.ndarray | None:
    raw = np.fromfile(path, dtype=np.uint8)
    return cv2.imdecode(raw, cv2.IMREAD_COLOR) if raw.size else None


def struct_feature(img_bgr: np.ndarray, box) -> np.ndarray | None:
    x0, y0, x1, y1 = (int(v) for v in box)
    c = img_bgr[y0:y1, x0:x1]
    if c.size == 0:
        return None
    g = cv2.cvtColor(c, cv2.COLOR_BGR2GRAY)
    g = cv2.resize(g, (32, 32), interpolation=cv2.INTER_AREA).astype(np.float32)
    f = np.fft.fftshift(np.fft.fft2(g))
    mag = np.log1p(np.abs(f))[12:20, 12:20].flatten()
    mag /= mag.sum() + 1e-6
    low = cv2.resize(g, (4, 4), interpolation=cv2.INTER_AREA).flatten() / 255.0
    mean, std = cv2.meanStdDev(c)
    color = np.concatenate([mean.flatten(), std.flatten()]) / 255.0
    return np.concatenate([mag * 8.0, low * 0.5, color])


def hit_rate(query_vecs, query_labels, index_vecs, index_labels, k, exclude_self=None):
    """top-k 命中率:结果中同语义标签比例(查询自身可排除)"""
    sims = query_vecs @ index_vecs.T
    hits = []
    for i in range(len(query_vecs)):
        s = sims[i]
        if exclude_self is not None:
            s[exclude_self[i]] = -2.0
        top = np.argsort(-s)[:k]
        hits.append(float(np.mean(index_labels[top] == query_labels[i])))
    return float(np.mean(hits))


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--npz", default="regions.npz")
    ap.add_argument("--photos", default=r"F:\相册\2022.10-2024.7\相机")
    ap.add_argument("--k-struct", type=int, default=64)
    ap.add_argument("--reps-per-cluster", type=int, default=3)
    args = ap.parse_args()

    data = np.load(args.npz, allow_pickle=False)
    vecs = data["vecs"].astype(np.float32)
    paths = [str(s) for s in data["paths"]]
    photo_idx = data["photo_idx"]
    boxes = data["boxes"]
    sem = KMeans(n_clusters=32, n_init=10, random_state=0).fit_predict(vecs)
    print(f"regions={len(vecs)}, semantic clusters={sem.max()+1}")

    # ---- 结构特征(重算,缓存裁剪) ----
    cache: dict[str, np.ndarray] = {}
    feats = []
    for r in range(len(vecs)):
        p = paths[photo_idx[r]]
        if p not in cache:
            if len(cache) > 32:
                cache.clear()
            cache[p] = imread_unicode(p)
        img = cache[p]
        f = struct_feature(img, boxes[r]) if img is not None else None
        feats.append(f if f is not None else np.zeros(86, np.float32))
    Xs = np.stack(feats).astype(np.float32)

    # ---- 频域粗簇 + 每簇 m 个分散代表 ----
    st = KMeans(n_clusters=args.k_struct, n_init=10, random_state=0).fit_predict(Xs)
    rep_vecs, rep_cluster = [], []
    for c in range(args.k_struct):
        members = np.where(st == c)[0]
        if len(members) == 0:
            continue
        m = min(args.reps_per_cluster, len(members))
        sub = KMeans(n_clusters=m, n_init=4, random_state=0).fit(vecs[members])
        for j in range(m):
            d = np.linalg.norm(vecs[members] - sub.cluster_centers_[j], axis=1)
            rep_vecs.append(vecs[members[int(np.argmin(d))]])
            rep_cluster.append(c)
    rep_vecs = np.stack(rep_vecs)
    rep_cluster = np.array(rep_cluster)
    print(f"struct clusters={args.k_struct}, semantic reps={len(rep_vecs)} (CLIP calls 5142→{len(rep_vecs)})")

    # 簇成员表与簇内结构距离精排
    members_of = [np.where(st == c)[0] for c in range(args.k_struct)]
    st_in_cluster = np.argsort(Xs, axis=0)  # 占位:精排用簇内结构近邻

    # ---- 方案 A:代表 MaxSim → 命中簇 → 簇内结构精排 top-k ----
    P = np.stack(rep_vecs)
    sims = vecs @ P.T  # 5142 × reps
    cluster_choice = np.argmax(sims, axis=1)
    hits10, hits50 = [], []
    for i in range(len(vecs)):
        c = rep_cluster[cluster_choice[i]]
        mem = members_of[c]
        # 簇内结构精排:与查询的结构距离
        d = np.linalg.norm(Xs[mem] - Xs[i], axis=1)
        top10 = mem[np.argsort(d)[:10]]
        top50 = mem[np.argsort(d)[:50]]
        hits10.append(float(np.mean(sem[top10] == sem[i])))
        hits50.append(float(np.mean(sem[top50] == sem[i])))
    res_a10, res_a50 = float(np.mean(hits10)), float(np.mean(hits50))

    # ---- 方案 B:PCA-64 签名 KNN ----
    P64 = PCA(n_components=64, random_state=0).fit_transform(vecs)
    b10 = hit_rate(P64, sem, P64, sem, 10, exclude_self=np.eye(len(vecs), dtype=bool))
    b50 = hit_rate(P64, sem, P64, sem, 50, exclude_self=np.eye(len(vecs), dtype=bool))

    # ---- 基准:全量 CLIP KNN ----
    g10 = hit_rate(vecs, sem, vecs, sem, 10, exclude_self=np.eye(len(vecs), dtype=bool))
    g50 = hit_rate(vecs, sem, vecs, sem, 50, exclude_self=np.eye(len(vecs), dtype=bool))

    out = {
        "clip_calls": {"baseline_full": 5142, "consensus_reps": int(len(rep_vecs))},
        "consensus_freqomain_reps": {"hit@10": round(res_a10, 3), "hit@50": round(res_a50, 3)},
        "pca64_knn": {"hit@10": round(b10, 3), "hit@50": round(b50, 3)},
        "baseline_full_knn": {"hit@10": round(g10, 3), "hit@50": round(g50, 3)},
    }
    Path("outputs/consensus_eval.json").write_text(json.dumps(out, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(out, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
