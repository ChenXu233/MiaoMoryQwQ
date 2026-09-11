"""在线 DP-means 回放实验:验证簇心数 K(N) ~ U·log(N) 假设(2026-09-11 所有者提出)

- 按照片时间序(文件名序 ≈ 拍摄序)逐张回放区域向量
- 在线 DP-means:距最近原型(1-cos)> τ → 诞生新原型;否则加权均值漂移(1/count)
- τ 扫描多档,输出 K(N) 曲线 + 最终簇大小分布 → outputs/kgrowth_*.png/json
"""
import argparse
import json
import math
from pathlib import Path

import numpy as np


def run_dpmeans(vecs: np.ndarray, photo_idx_arr: np.ndarray, order: np.ndarray, tau: float):
    """在线 DP-means 回放。返回 K(N) 轨迹与最终原型/计数。

    vecs 已 L2 归一;距离 = 1 - dot(即 1-cos,∈[0,2])。
    merge_weight:成员数低于该值且"年龄"足够大的原型在重排时衰退——在线阶段不做,保持纯增量语义。
    """
    protos: list[np.ndarray] = []  # 原型向量(保持归一)
    counts: list[int] = []
    k_curve: list[int] = []  # 每张照片处理完后的 K
    assignments: list[int] = []

    for photo in order:
        rows = np.where(photo_idx_arr == photo)[0]
        for r in rows:
            v = vecs[r]
            if not protos:
                protos.append(v.copy())
                counts.append(1)
                assignments.append(0)
                continue
            P = np.stack(protos)
            d = 1.0 - P @ v
            c = int(np.argmin(d))
            if d[c] > tau:
                protos.append(v.copy())
                counts.append(1)
                assignments.append(len(protos) - 1)
            else:
                n = counts[c]
                # 加权均值漂移(1/(n+1)),再归一
                protos[c] = (protos[c] * n + v) / (n + 1)
                protos[c] /= np.linalg.norm(protos[c]) + 1e-9
                counts[c] += 1
                assignments.append(c)
        k_curve.append(len(protos))
    return np.array(k_curve), protos, counts, np.array(assignments)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--npz", default="regions.npz")
    ap.add_argument("--taus", default="0.18,0.22,0.26,0.30,0.35")
    ap.add_argument("--out", default="outputs")
    args = ap.parse_args()

    data = np.load(args.npz, allow_pickle=False)
    vecs = data["vecs"].astype(np.float32)
    photo_idx_arr = data["photo_idx"]
    n_photos = int(photo_idx_arr.max()) + 1
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    # 时间序 = 照片下标升序(文件名序 ≈ 拍摄序)
    order = np.arange(n_photos)
    report = {"n_regions": int(len(vecs)), "n_photos": n_photos, "runs": []}
    logn = np.log(np.arange(1, n_photos + 1) + math.e)

    for tau in (float(t) for t in args.taus.split(",")):
        k_curve, protos, counts, _ = run_dpmeans(vecs, photo_idx_arr, order, tau)
        sizes = np.sort(np.array(counts))[::-1]
        # 拟合 K(N) ~ U·log N(最小二乘,跳过前 10 张冷启动)
        xs, ys = logn[10:], k_curve[10:]
        U = float((xs @ ys) / (xs @ xs))
        resid = float(np.sqrt(np.mean((ys - U * xs) ** 2)))
        tag = f"tau{tau:.2f}"
        np.save(out_dir / f"kgrowth_{tag}.npy", k_curve)
        # 曲线图
        try:
            import matplotlib
            matplotlib.use("Agg")
            import matplotlib.pyplot as plt
            fig, ax = plt.subplots(figsize=(7, 4.2), dpi=120)
            ax.plot(k_curve, label=f"online DP-means τ={tau} (K={k_curve[-1]})")
            ax.plot(U * logn, "--", label=f"U·log N fit (U={U:.1f}, rmse={resid:.1f})")
            ax.set_xlabel("photos seen (N)")
            ax.set_ylabel("clusters K")
            ax.set_title(f"K(N) growth — τ={tau}")
            ax.legend()
            fig.tight_layout()
            fig.savefig(out_dir / f"kgrowth_{tag}.png")
            plt.close(fig)
        except ImportError:
            pass
        run = {
            "tau": tau, "final_k": int(k_curve[-1]), "U_fit": round(U, 2),
            "logn_rmse": round(resid, 2),
            "size_top10": sizes[:10].tolist(),
            "singletons": int((sizes == 1).sum()),
        }
        report["runs"].append(run)
        print(json.dumps(run, ensure_ascii=False))

    (out_dir / "kgrowth_report.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
