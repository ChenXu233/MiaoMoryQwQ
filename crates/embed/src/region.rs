//! 区域级语义索引（ADR-0015）：SAM ONNX 分割 + 区域裁剪 + 在线 DP-means 原型空间。
//!
//! 分割：MobileSAM ONNX（encoder 整图一次 1024²；decoder 按批点提示，批内 8×8=64 点，
//! 16 批错位网格 → 全图 1024 采样点）；输出低分辨率 mask（256²），在 256 域算面积/包围盒
//! 后映射回原图（省内存，无需全尺寸 mask）。
//! 聚类：在线 DP-means（距最近原型 1−cos > τ → 诞生新原型；否则加权均值漂移）。
//! 时空调制（连拍收紧 / 换场放宽）与持久化由调用方（region worker）编排。

use std::path::Path;

use image::RgbImage;
use mm_core::ErrorCode;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;

use crate::EpKind;

/// SAM 预处理（Meta 官方约定：0-255 域减均值除标准差，不除 255）
const PIX_MEAN: [f32; 3] = [123.675, 116.28, 103.53];
const PIX_STD: [f32; 3] = [58.395, 57.12, 57.375];
const SAM_INPUT: usize = 1024;
const MASK_SIZE: usize = 256;
const GRID: usize = 8; // 每批 8×8 = 64 点（ONNX 固定批量）
const GROUPS: usize = 16; // 批次数（错位网格 → 全图 1024 采样点）
const MASK_FRAC_MIN: f32 = 0.0008;
const MASK_FRAC_MAX: f32 = 0.85;
const MIN_SIDE_PX: i32 = 24;
const PAD_FRAC: f32 = 0.12;
const MAX_REGIONS: usize = 24;

/// 原图坐标系的区域包围盒
#[derive(Debug, Clone)]
pub struct RegionBox {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
    pub area_frac: f32,
}

pub struct SamSegmenter {
    encoder: Session,
    decoder: Session,
}

impl SamSegmenter {
    pub fn load(encoder_path: &Path, decoder_path: &Path) -> Result<Self, ErrorCode> {
        // 必须走文件路径加载：导出的 ONNX 带 external data（.onnx.data），
        // commit_from_memory 的内存 buffer 无法解析相对路径引用（静默失败的教训）。
        // 分割纯 CPU（轻量且避免占用推理 EP）
        let build = |p: &Path| -> Result<Session, ErrorCode> {
            Session::builder()
                .map_err(|_| ErrorCode::ModelMissing)?
                .with_optimization_level(GraphOptimizationLevel::Level3)
                .map_err(|_| ErrorCode::ModelMissing)?
                .commit_from_file(p)
                .map_err(|_| ErrorCode::ModelMissing)
        };
        let encoder = build(encoder_path)?;
        let decoder = build(decoder_path)?;
        Ok(Self { encoder, decoder })
    }

    pub fn available(encoder_path: &Path, decoder_path: &Path) -> bool {
        encoder_path.is_file() && decoder_path.is_file()
    }

    /// RGB 原图 → 自然区域包围盒列表（≤`MAX_REGIONS`，按 score 降序）
    pub fn segment(
        &mut self,
        rgb: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<RegionBox>, ErrorCode> {
        // 预处理：1024² + SAM 归一化（RGB）
        let img = RgbImage::from_raw(width, height, rgb.to_vec())
            .ok_or(ErrorCode::DecodeFailed)?;
        let resized =
            image::imageops::resize(&img, SAM_INPUT as u32, SAM_INPUT as u32,
                                    image::imageops::FilterType::Triangle);
        let mut input = vec![0f32; SAM_INPUT * SAM_INPUT * 3];
        for (i, px) in resized.pixels().enumerate() {
            let [r, g, b] = px.0;
            input[i * 3] = (r as f32 - PIX_MEAN[0]) / PIX_STD[0];
            input[i * 3 + 1] = (g as f32 - PIX_MEAN[1]) / PIX_STD[1];
            input[i * 3 + 2] = (b as f32 - PIX_MEAN[2]) / PIX_STD[2];
        }
        let tensor = Tensor::from_array((
            [1i64, 3i64, SAM_INPUT as i64, SAM_INPUT as i64],
            input,
        ))
        .map_err(|_| ErrorCode::DecodeFailed)?;
        let embedding = {
            let outs = self
                .encoder
                .run(ort::inputs! {"images" => tensor})
                .map_err(|_| ErrorCode::SearchUnavailable)?;
            let (_, data) = outs["embeddings"]
                .try_extract_tensor::<f32>()
                .map_err(|_| ErrorCode::SearchUnavailable)?;
            data.to_vec()
        };

        // 16 批错位网格点 → 每批 4 个候选 mask（256² logits）
        let mut candidates: Vec<(Vec<u8>, f32)> = Vec::new(); // (0/1 位图, score)
        for g in 0..GROUPS {
            let offset = ((g as f32 * 37.7) % 96.0) as f32;
            let mut pts = Vec::with_capacity(GRID * GRID * 2);
            let mut labels = Vec::with_capacity(GRID * GRID);
            for j in 0..GRID {
                for i in 0..GRID {
                    let x = (64.0 + offset + i as f32 * (896.0 / (GRID - 1) as f32))
                        .clamp(16.0, 1008.0);
                    let y = (64.0 + offset + j as f32 * (896.0 / (GRID - 1) as f32))
                        .clamp(16.0, 1008.0);
                    pts.push(x);
                    pts.push(y);
                    labels.push(1.0f32);
                }
            }
            let t_emb = Tensor::from_array(([embedding.len() as i64], embedding.clone()))
                .map_err(|_| ErrorCode::SearchUnavailable)?;
            let t_pc = Tensor::from_array((
                [1i64, (GRID * GRID) as i64, 2i64],
                pts,
            ))
            .map_err(|_| ErrorCode::SearchUnavailable)?;
            let t_pl =
                Tensor::from_array(([1i64, (GRID * GRID) as i64], labels))
                    .map_err(|_| ErrorCode::SearchUnavailable)?;
            let t_mi = Tensor::from_array(([1i64, 1i64, MASK_SIZE as i64, MASK_SIZE as i64],
                                           vec![0f32; MASK_SIZE * MASK_SIZE]))
                .map_err(|_| ErrorCode::SearchUnavailable)?;
            let t_hm = Tensor::from_array(([1i64], vec![0f32])).map_err(|_| ErrorCode::SearchUnavailable)?;
            let t_os = Tensor::from_array(([2i64], vec![MASK_SIZE as f32, MASK_SIZE as f32]))
                .map_err(|_| ErrorCode::SearchUnavailable)?;
            let outs = self
                .decoder
                .run(ort::inputs! {
                    "image_embeddings" => t_emb,
                    "point_coords" => t_pc,
                    "point_labels" => t_pl,
                    "mask_input" => t_mi,
                    "has_mask_input" => t_hm,
                    "orig_im_size" => t_os,
                })
                .map_err(|_| ErrorCode::SearchUnavailable)?;
            let (_, mdata) = outs["masks"]
                .try_extract_tensor::<f32>()
                .map_err(|_| ErrorCode::SearchUnavailable)?;
            let (_, sdata) = outs["scores"]
                .try_extract_tensor::<f32>()
                .map_err(|_| ErrorCode::SearchUnavailable)?;
            let n_masks = mdata.len() / (MASK_SIZE * MASK_SIZE);
            for m in 0..n_masks {
                let lo = m * MASK_SIZE * MASK_SIZE;
                let hi = lo + MASK_SIZE * MASK_SIZE;
                let slice = &mdata[lo..hi];
                let bits: Vec<u8> = slice.iter().map(|&v| u8::from(v > 0.0)).collect();
                candidates.push((bits, sdata.get(m).copied().unwrap_or(0.0)));
            }
        }

        // 后处理：面积窗 → bbox → 原图映射 → padding → 最小边 → bbox IoU 去重 → 上限
        let mut boxes: Vec<RegionBox> = Vec::new();
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (bits, _score) in &candidates {
            if boxes.len() >= MAX_REGIONS {
                break;
            }
            let count = bits.iter().filter(|&&b| b == 1).count() as f32;
            let frac = count / (MASK_SIZE * MASK_SIZE) as f32;
            if !(MASK_FRAC_MIN..=MASK_FRAC_MAX).contains(&frac) {
                continue;
            }
            let (mut bx0, mut by0, mut bx1, mut by1) = (i32::MAX, i32::MAX, 0i32, 0i32);
            for (i, &b) in bits.iter().enumerate() {
                if b == 1 {
                    let y = i as i32 / MASK_SIZE as i32;
                    let x = i as i32 % MASK_SIZE as i32;
                    bx0 = bx0.min(x);
                    by0 = by0.min(y);
                    bx1 = bx1.max(x);
                    by1 = by1.max(y);
                }
            }
            // 256 域 → 原图
            let sx = width as f32 / MASK_SIZE as f32;
            let sy = height as f32 / MASK_SIZE as f32;
            let (mut rx0, mut ry0, mut rx1, mut ry1) = (
                (bx0 as f32 * sx) as i32,
                (by0 as f32 * sy) as i32,
                ((bx1 + 1) as f32 * sx) as i32,
                ((by1 + 1) as f32 * sy) as i32,
            );
            let pw = ((rx1 - rx0) as f32 * PAD_FRAC) as i32;
            let ph = ((ry1 - ry0) as f32 * PAD_FRAC) as i32;
            rx0 = (rx0 - pw).max(0);
            ry0 = (ry0 - ph).max(0);
            rx1 = (rx1 + pw).min(width as i32);
            ry1 = (ry1 + ph).min(height as i32);
            if rx1 - rx0 < MIN_SIDE_PX || ry1 - ry0 < MIN_SIDE_PX {
                continue;
            }
            // bbox IoU 去重（跨批重复分割的包围盒几乎重合）
            let dup = boxes.iter().any(|b| {
                let ix0 = rx0.max(b.x0);
                let iy0 = ry0.max(b.y0);
                let ix1 = rx1.min(b.x1);
                let iy1 = ry1.min(b.y1);
                let inter = ((ix1 - ix0).max(0) * (iy1 - iy0).max(0)) as f32;
                let a1 = ((rx1 - rx0) * (ry1 - ry0)) as f32;
                let a2 = ((b.x1 - b.x0) * (b.y1 - b.y0)) as f32;
                inter / (a1 + a2 - inter) > 0.85
            });
            if dup {
                continue;
            }
            boxes.push(RegionBox { x0: rx0, y0: ry0, x1: rx1, y1: ry1, area_frac: frac });
        }
        Ok(boxes)
    }
}

// ---- 在线 DP-means（纯函数；持久化由调用方编排）----

/// 原型（归一化向量 + 成员计数）
#[derive(Debug, Clone)]
pub struct DpProto {
    pub vec: Vec<f32>,
    pub count: i64,
}

pub enum DpOutcome {
    /// 归入现有原型（下标）
    Matched(usize),
    /// 诞生新原型
    New,
}

/// 距最近原型（1−cos）> τ → New；否则 Matched
pub fn dp_assign(v: &[f32], protos: &[DpProto], tau: f32) -> DpOutcome {
    if protos.is_empty() {
        return DpOutcome::New;
    }
    let mut best = (f32::MAX, 0usize);
    for (i, p) in protos.iter().enumerate() {
        let dot: f32 = v.iter().zip(&p.vec).map(|(a, b)| a * b).sum();
        let d = 1.0 - dot;
        if d < best.0 {
            best = (d, i);
        }
    }
    if best.0 > tau {
        DpOutcome::New
    } else {
        DpOutcome::Matched(best.1)
    }
}

/// 原型漂移：加权均值后归一化（count = 归入前成员数）
pub fn dp_drift(proto: &[f32], v: &[f32], count: i64) -> Vec<f32> {
    let n = count as f32;
    let mut out: Vec<f32> = proto
        .iter()
        .zip(v)
        .map(|(a, b)| (a * n + b) / (n + 1.0))
        .collect();
    let norm = out.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-9 {
        for x in out.iter_mut() {
            *x /= norm;
        }
    }
    out
}
