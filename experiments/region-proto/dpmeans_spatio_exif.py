"""时空密度调制在线 DP-means——所有者 2×2 矩阵完整实现(EXIF 时间 + GPS)

U(Δt, Δs) → 准入阈值调制 / 篇章触发:
  连拍   Δt<5min   Δs<200m → τ×0.5  相同聚类信号
  久留   Δt≥2h     Δs<200m → τ×0.9  轻微多 K(同地不同时刻)
  移动   Δt<2h     Δs≥1km  → τ×1.3  更多 K(快速换场景)
  篇章   Δt>14d    Δs≥5km  → 完全新增(独立原型集);无 GPS 对:Δt>60d 篇章
  其余   τ×1.0;GPS 缺失对退化为时间规则
对照:fixed(固定 τ 无调制无篇章)。指标:K / 单例率 / top1 / NMI vs 语义基准。
"""
import argparse
import json
import math
from datetime import datetime
from pathlib import Path

import numpy as np
from PIL import Image
from sklearn.cluster import KMeans
from sklearn.metrics import normalized_mutual_info_score as nmi


def exif_meta(path: str):
    """→ (datetime|None, (lat,lng)|None)"""
    try:
        ex = Image.open(path).getexif()
    except Exception:
        return None, None
    dt = None
    raw_dt = ex.get(306) or ex.get_ifd(0x8769).get(36867)
    if raw_dt:
        try:
            dt = datetime.strptime(str(raw_dt), "%Y:%m:%d %H:%M:%S")
        except ValueError:
            pass
    gps = ex.get_ifd(0x8825)

    def deg(d, ref):
        try:
            v = float(d[0]) + float(d[1]) / 60 + float(d[2]) / 3600
            return -v if str(ref).upper() in ("S", "W") else v
        except Exception:
            return None
    latlng = None
    if gps.get(2) and gps.get(4):
        lat, lng = deg(gps[2], gps.get(1, "N")), deg(gps[4], gps.get(3, "E"))
        if lat is not None and lng is not None:
            latlng = (lat, lng)
    return dt, latlng


def haversine_km(a, b) -> float:
    la1, lo1, la2, lo2 = map(math.radians, (a[0], a[1], b[0], b[1]))
    h = math.sin((la2 - la1) / 2) ** 2 + math.cos(la1) * math.cos(la2) * math.sin((lo2 - lo1) / 2) ** 2
    return 2 * 6371 * math.asin(math.sqrt(h))


def modulation(dt_min: float | None, ds_km: float | None, tau: float):
    """→ (tau_eff, new_chapter)"""
    if dt_min is None:
        return tau, False
    if ds_km is None:  # 无 GPS 对:时间规则
        if dt_min < 5:
            return tau * 0.5, False
        if dt_min > 60 * 24 * 60:
            return tau, True
        return tau, False
    near = ds_km < 0.2
    if near and dt_min < 5:
        return tau * 0.5, False          # 连拍
    if near:
        return tau * 0.9, False          # 长时间×单位空间:轻微多 K
    if ds_km >= 1.0 and dt_min < 120:
        return tau * 1.3, False          # 单位时间×大空间:更多 K
    if dt_min > 14 * 24 * 60 and ds_km >= 5.0:
        return tau, True                 # 长时间×大空间:完全新增
    return tau, False


def run(vecs, photo_idx, metas, tau_base, modulate: bool):
    chapters_p: list[list[np.ndarray]] = [[]]
    chapters_c: list[list[int]] = [[]]
    assign: list[int] = []
    k_curve: list[int] = []
    last = None  # (datetime, latlng)
    for pi in sorted(set(photo_idx.tolist())):
        rows = np.where(photo_idx == pi)[0]
        dt, ll = metas[pi] if pi < len(metas) else (None, None)
        dt_min = ds_km = None
        if last is not None and dt is not None and last[0] is not None:
            dt_min = abs((dt - last[0]).total_seconds()) / 60.0
        if last is not None and ll is not None and last[1] is not None:
            ds_km = haversine_km(last[1], ll)
        tau_eff, new_chapter = (tau_base, False)
        if modulate:
            tau_eff, new_chapter = modulation(dt_min, ds_km, tau_base)
        if new_chapter:
            chapters_p.append([])
            chapters_c.append([])
        ci = len(chapters_p) - 1
        for r in rows:
            v = vecs[r]
            P, C = chapters_p[ci], chapters_c[ci]
            if not P:
                P.append(v.copy())
                C.append(1)
            else:
                d = 1.0 - np.stack(P) @ v
                c = int(np.argmin(d))
                if d[c] > tau_eff:
                    P.append(v.copy())
                    C.append(1)
                else:
                    P[c] = (P[c] * C[c] + v) / (C[c] + 1)
                    P[c] /= np.linalg.norm(P[c]) + 1e-9
                    C[c] += 1
            assign.append(sum(len(x) for x in chapters_c[:ci]) + len(C) - 1)
        if dt is not None:
            last = (dt, ll if ll else (last[1] if last else None))
        k_curve.append(sum(len(x) for x in chapters_c))
    return k_curve, chapters_c, np.array(assign)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--npz", default="regions.npz")
    ap.add_argument("--photos", default=r"F:\相册\2022.10-2024.7\相机")
    ap.add_argument("--tau", type=float, default=0.24)
    ap.add_argument("--k-sem", type=int, default=32)
    ap.add_argument("--out", default="outputs/spatio_exif_report.json")
    args = ap.parse_args()

    data = np.load(args.npz, allow_pickle=False)
    vecs = data["vecs"].astype(np.float32)
    photo_idx = data["photo_idx"]
    paths = [str(s) for s in data["paths"]]
    metas = [exif_meta(p) for p in paths]
    n_gps = sum(1 for _, ll in metas if ll)
    sem = KMeans(n_clusters=args.k_sem, n_init=10, random_state=0).fit_predict(vecs)
    print(f"photos={len(paths)} with_gps={n_gps}")

    def summarize(tag, k_curve, chapters_c, assign):
        sizes = np.array([c for cc in chapters_c for c in cc])
        return {
            "variant": tag, "final_k": int(len(sizes)), "chapters": len(chapters_c),
            "singletons": int((sizes == 1).sum()),
            "singleton_rate": round(float((sizes == 1).mean()), 3),
            "top1_share": round(float(sizes.max() / sizes.sum()), 3),
            "nmi_vs_sem": round(float(nmi(sem, assign)), 3),
        }

    report = {"tau_base": args.tau, "with_gps": n_gps}
    for tag, mod in (("fixed", False), ("spatio", True)):
        kc, chc, asg = run(vecs, photo_idx, metas, args.tau, mod)
        report[tag] = summarize(tag, kc, chc, asg)
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    Path(args.out).write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
