"""阶段一/二:MobileSAM 区域分离 + Chinese-CLIP 区域编码 → regions.npz

- 分割:MobileSAM(vit_t,CPU 可跑)automatic mask generator,
  过滤面积 0.3%~40%,每图最多 24 个区域(按面积降序)
- 编码:区域 bbox(+12% padding)裁剪 → resize 224×224 → Chinese-CLIP
  预处理(/255,CLIP mean/std,NCHW)→ visual.int8.onnx → 512 维 L2 归一
- 输出:regions.npz{vecs, paths, photo_idx, boxes, area_frac} + extract_report.json
"""
import argparse
import json
import os
import time
from pathlib import Path

import cv2
import numpy as np
import onnxruntime as ort
import torch
from mobile_sam import sam_model_registry, SamAutomaticMaskGenerator

CLIP_MEAN = np.array([0.48145466, 0.4578275, 0.40821073], dtype=np.float32)
CLIP_STD = np.array([0.26862954, 0.26130258, 0.27577711], dtype=np.float32)

IMG_EXTS = {".jpg", ".jpeg", ".png", ".webp"}


def imread_unicode(path: str) -> np.ndarray | None:
    """cv2.imread 走 ANSI 路径,中文路径(相册/相机)会静默返回 None——用 fromfile+imdecode"""
    data = np.fromfile(path, dtype=np.uint8)
    return cv2.imdecode(data, cv2.IMREAD_COLOR) if data.size else None


def list_photos(root: Path, limit: int) -> list[Path]:
    photos = sorted(p for p in root.rglob("*") if p.suffix.lower() in IMG_EXTS)
    return photos[:limit] if limit > 0 else photos


def crop_regions(img_bgr: np.ndarray, masks: list[dict], max_per_img: int, scale: float = 1.0) -> list[tuple[np.ndarray, tuple[int, int, int, int]]]:
    """mask(bbox)→ 映射回原分辨率 → +12% padding 裁剪,按面积降序,最多 max_per_img 个"""
    h, w = img_bgr.shape[:2]
    out = []
    for m in sorted(masks, key=lambda m: m["area"], reverse=True):
        frac = m["area"] / (m["segmentation"].shape[0] * m["segmentation"].shape[1])
        # 天空/地面等大语义区域可占 60%+,小碎片(远处人物)0.1% 也有语义,都保留
        if frac < 0.0008 or frac > 0.85:
            continue
        # COCO bbox = 左上角 + 宽高(不是两点)
        bx, by, bw, bh = (int(round(v / scale)) for v in m["bbox"])
        pw, ph = int(bw * 0.12), int(bh * 0.12)
        x0, y0 = max(0, bx - pw), max(0, by - ph)
        x1, y1 = min(w, bx + bw + pw), min(h, by + bh + ph)
        if x1 - x0 < 24 or y1 - y0 < 24:
            continue
        out.append((cv2.cvtColor(img_bgr[y0:y1, x0:x1], cv2.COLOR_BGR2RGB), (x0, y0, x1, y1)))
        if len(out) >= max_per_img:
            break
    return out


def clip_encode(sess: ort.InferenceSession, crops: list[np.ndarray]) -> np.ndarray:
    """RGB crops → N×512 f32(L2 归一);与 crates/embed/preprocess 同均值方差"""
    batch = np.stack(
        [cv2.resize(c, (224, 224), interpolation=cv2.INTER_LINEAR) for c in crops]
    ).astype(np.float32) / 255.0
    batch = (batch - CLIP_MEAN) / CLIP_STD
    batch = np.transpose(batch, (0, 3, 1, 2)).copy()
    inp = sess.get_inputs()[0].name
    out = sess.run(None, {inp: batch})[0]
    v = np.asarray(out, dtype=np.float32).reshape(len(crops), -1)
    return v / (np.linalg.norm(v, axis=1, keepdims=True) + 1e-9)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--photos", default=r"F:\相册\2022.10-2024.7\相机")
    ap.add_argument("--limit", type=int, default=229, help="0 = 全部")
    ap.add_argument("--weights", default="mobile_sam.pt")
    ap.add_argument("--model-type", default="vit_t")
    ap.add_argument("--clip-onnx", default=r"E:\git\MiaoMoryQwQ\.devdata\models\visual.int8.onnx")
    ap.add_argument("--out", default="regions.npz")
    args = ap.parse_args()

    photos = list_photos(Path(args.photos), args.limit)
    print(f"photos: {len(photos)}")

    torch.set_num_threads(max(1, (os.cpu_count() or 8) - 1))

    sam = sam_model_registry[args.model_type](checkpoint=args.weights)
    sam.to("cpu").eval()
    generator = SamAutomaticMaskGenerator(
        sam,
        points_per_side=12,
        pred_iou_thresh=0.86,
        stability_score_thresh=0.90,
        min_mask_region_area=100,
    )

    clip_sess = ort.InferenceSession(args.clip_onnx, providers=["CPUExecutionProvider"])

    # 分割在降采样图上跑(后处理开销 ∝ 像素数),bbox 按比例映射回原图裁剪
    SEG_MAX_SIDE = 1600

    all_vecs: list[np.ndarray] = []
    all_paths: list[str] = []
    all_photo_idx: list[int] = []
    all_boxes: list[tuple[int, int, int, int]] = []

    t0 = time.time()
    for i, p in enumerate(photos):
        img = imread_unicode(str(p))
        if img is None:
            print(f"[skip] unreadable: {p}")
            continue
        h, w = img.shape[:2]
        scale = min(1.0, SEG_MAX_SIDE / max(h, w))
        seg_img = cv2.resize(img, (int(w * scale), int(h * scale)), interpolation=cv2.INTER_AREA) if scale < 1.0 else img
        with torch.inference_mode():
            masks = generator.generate(seg_img)
        regions = crop_regions(img, masks, 40, scale)
        if not regions:
            continue
        crops = [c for c, _ in regions]
        vecs = clip_encode(clip_sess, crops)
        all_vecs.append(vecs)
        all_paths.append(str(p))
        all_photo_idx.extend([i] * len(vecs))
        all_boxes.extend(b for _, b in regions)
        print(f"[{i+1}/{len(photos)}] {p.name}: {len(masks)} masks -> {len(crops)} regions  ({time.time() - t0:.0f}s)")

    vecs = np.concatenate(all_vecs, axis=0)
    np.savez_compressed(
        args.out,
        vecs=vecs,
        paths=np.array(all_paths),
        photo_idx=np.array(all_photo_idx, dtype=np.int32),
        boxes=np.array(all_boxes, dtype=np.int32),
    )
    report = {
        "photos": len(all_paths),
        "regions": int(vecs.shape[0]),
        "dim": int(vecs.shape[1]),
        "seconds": round(time.time() - t0, 1),
        "avg_regions_per_photo": round(vecs.shape[0] / max(1, len(all_paths)), 1),
    }
    Path("extract_report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
