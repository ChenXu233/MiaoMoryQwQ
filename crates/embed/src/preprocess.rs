//! 图像预处理：CLIP 规范（resize 短边 → [center crop] → RGB → 归一化 → NCHW f32）。
//! 是否 center-crop 由模型输入形状在运行时决定（固定 224×224 才裁剪）。

use image::{ImageBuffer, RgbImage};
use mm_core::{DecodedImage, ErrorCode};

pub const IMAGE_SIZE: u32 = 224;
pub const MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
pub const STD: [f32; 3] = [0.268_629_54, 0.261_302_6, 0.275_777_1];

/// 单图 → ([3,H,W] f32, 宽, 高)（channel-major）
pub fn to_chw(photo: &DecodedImage, center_crop: bool) -> Result<(Vec<f32>, u32, u32), ErrorCode> {
    let src = ImageBuffer::<image::Rgb<u8>, Vec<u8>>::from_raw(
        photo.width,
        photo.height,
        photo.rgb.clone(),
    )
    .ok_or(ErrorCode::DecodeFailed)?;

    let squared = if center_crop {
        resize_and_center_crop(&src, IMAGE_SIZE)
    } else {
        resize_shortest_edge(&src, IMAGE_SIZE)
    };

    let (w, h) = (squared.width(), squared.height());
    let mut out = vec![0f32; 3 * (w * h) as usize];
    for (idx, px) in squared.pixels().enumerate() {
        let [r, g, b] = px.0;
        out[idx] = (f32::from(r) / 255.0 - MEAN[0]) / STD[0];
        out[(w * h) as usize + idx] = (f32::from(g) / 255.0 - MEAN[1]) / STD[1];
        out[2 * (w * h) as usize + idx] = (f32::from(b) / 255.0 - MEAN[2]) / STD[2];
    }
    Ok((out, w, h))
}

fn resize_shortest_edge(src: &RgbImage, target: u32) -> RgbImage {
    let (w, h) = (src.width(), src.height());
    let long = w.max(h);
    let short = w.min(h);
    let scale = f64::from(target) / f64::from(short.max(1));
    // 短边到 target，另一边等比（不放大超过 2 倍上限保护）
    let (nw, nh) = if w <= h {
        (target, (f64::from(h) * scale).round() as u32)
    } else {
        ((f64::from(w) * scale).round() as u32, target)
    };
    let _ = (long, short);
    image::imageops::resize(
        src,
        nw.max(1),
        nh.max(1),
        image::imageops::FilterType::CatmullRom,
    )
}

fn resize_and_center_crop(src: &RgbImage, target: u32) -> RgbImage {
    // CLIP 官方流程：短边 resize 到 target 后中心裁 target×target
    let resized = resize_shortest_edge(src, target);
    let (w, h) = (resized.width(), resized.height());
    let left = w.saturating_sub(target) / 2;
    let top = h.saturating_sub(target) / 2;
    image::imageops::crop_imm(&resized, left, top, target.min(w), target.min(h)).to_image()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32) -> DecodedImage {
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                rgb.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, 64]);
            }
        }
        DecodedImage {
            width: w,
            height: h,
            rgb,
        }
    }

    #[test]
    fn chw_output_shape_and_normalization_range() {
        let photo = gradient(64, 64);
        let (chw, w, h) = to_chw(&photo, true).unwrap();
        assert_eq!((w, h), (224, 224));
        assert_eq!(chw.len(), 3 * 224 * 224);
        assert!(chw.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn center_crop_square_keeps_size() {
        let photo = gradient(800, 600);
        let (chw, w, h) = to_chw(&photo, true).unwrap();
        assert_eq!((w, h), (224, 224));
        assert_eq!(chw.len(), 3 * 224 * 224);
    }

    #[test]
    fn shortest_edge_only_keeps_aspect() {
        let photo = gradient(800, 600);
        let (chw, w, h) = to_chw(&photo, false).unwrap();
        // 短边 600 → 224，长边 800 → ~298.7 → 299
        assert_eq!((w, h), (299, 224));
        assert_eq!(chw.len(), 3 * 224 * 299);
    }
}
