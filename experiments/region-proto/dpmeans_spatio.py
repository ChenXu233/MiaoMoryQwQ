"""时空密度调制的在线 DP-means(2026-09-11 所有者 2×2 矩阵的时间轴验证)

U(时间,空间)密度函数 → 准入阈值调制 + 篇章切分:
  连拍(Δt<5min)   → τ×0.5  相同聚类信号
  常速(5min~7d)    → τ×1.0
  换场(Δt>7d)      → τ×1.4  更多 K
  篇章(Δt>30d)     → 完全新增:独立原型集,K = Σ章
对照:fixed(固定 τ,无调制无篇章)。评估:K / 单例率 / top1 占比 / NMI vs 语义基准。
"""
import argparse
import json
import re
from datetime import datetime
from pathlib import Path

import numpy as np
from sklearn.cluster import KMeans
from sklearn.metrics import normalized_mutual_info_score as nmi

FMT = re.compile(r"(20\d{2})(\d{2})(\d{2})_(\d{2})(\d{2})(\d{2})")


def photo_time(path: str) -> datetime | None:
    m = FMT.search(Path(path).name)
    if not m:
        return None
    y, mo, d, h, mi, s = (int(g) for g in m.groups())
    try:
        return datetime(y, mo, d, h, mi, s)
    except ValueError:
        return None


def run(vecs: np.ndarray, photo_idx: np.ndarray, times: list, tau_base: float,
        chapter_gap_days: float, modulate: bool):
    chapters_p: list[list[np.ndarray]] = [[]]   # 每章独立原型集
    chapters_c: list[list[int]] = [[]]
    assign: list[int] = []
    k_curve: list[int] = []
    last_t: datetime | None = None

    for pi in sorted(set(photo_idx.tolist())):
        rows = np.where(photo_idx == pi)[0]
        t = times[pi] if pi < len(times) else None
        gap_days = None
        if t is not None and last_t is not None:
            gap_days = abs((t - last_t).total_seconds()) / 86400.0
        if gap_days is not None and gap_days > chapter_gap_days:
            chapters_p.append([])   # 长时间+大空间:完全新增(新篇章)
            chapters_c.append([])
        ci = len(chapters_p) - 1
        dt_min = gap_days * 1440.0 if gap_days is not None else None

        for r in rows:
            v = vecs[r]
            P, C = chapters_p[ci], chapters_c[ci]
            if not P:
                P.append(v.copy())
                C.append(1)
            else:
                d = 1.0 - np.stack(P) @ v
                c = int(np.argmin(d))
                tau = tau_base
                if modulate and dt_min is not None:
                    if dt_min < 5:            # 单位时间×单位空间:连拍
                        tau *= 0.5
                    elif dt_min > 7 * 1440:   # 长时间跨度:换场/新阶段
                        tau *= 1.4
                if d[c] > tau:
                    P.append(v.copy())
                    C.append(1)
                else:
                    P[c] = (P[c] * C[c] + v) / (C[c] + 1)
                    P[c] /= np.linalg.norm(P[c]) + 1e-9
                    C[c] += 1
            assign.append(sum(len(x) for x in chapters_c[:ci]) + len(C) - 1)
        if t is not None:
            last_t = t
        k_curve.append(sum(len(x) for x in chapters_c))
    return k_curve, chapters_c, np.array(assign)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--npz", default="regions.npz")
    ap.add_argument("--tau", type=float, default=0.24)
    ap.add_argument("--chapter-gap-days", type=float, default=30.0)
    ap.add_argument("--k-sem", type=int, default=32)
    ap.add_argument("--out", default="outputs/spatio_report.json")
    args = ap.parse_args()

    data = np.load(args.npz, allow_pickle=False)
    vecs = data["vecs"].astype(np.float32)
    photo_idx = data["photo_idx"]
    times = [photo_time(str(s)) for s in data["paths"]]
    sem = KMeans(n_clusters=args.k_sem, n_init=10, random_state=0).fit_predict(vecs)

    def summarize(tag, k_curve, chapters_c, assign):
        sizes = np.array([c for cc in chapters_c for c in cc])
        return {
            "variant": tag,
            "final_k": int(len(sizes)),
            "chapters": len(chapters_c),
            "singletons": int((sizes == 1).sum()),
            "singleton_rate": round(float((sizes == 1).mean()), 3),
            "top1_share": round(float(sizes.max() / sizes.sum()), 3),
            "nmi_vs_sem": round(float(nmi(sem, assign)), 3),
            "k_curve_tail": [int(k_curve[i]) for i in range(0, len(k_curve), max(1, len(k_curve) // 8))],
        }

    report = {"tau_base": args.tau, "chapter_gap_days": args.chapter_gap_days}
    for tag, gap, mod in (("fixed", 1e9, False), ("spatio", args.chapter_gap_days, True)):
        kc, chc, asg = run(vecs, photo_idx, times, args.tau, gap, mod)
        report[tag] = summarize(tag, kc, chc, asg)

    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    Path(args.out).write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
