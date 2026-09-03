//! 缩略图生成：长边 512px，WebP 格式（白皮书 §3.6 统一 RGB 产出 → 缩略图）。
//! 注：image 0.25 的 WebP 编码器仅支持无损；有损编码推迟到换绑定时评估（key 不变）。

use std::io::Cursor;

use image::ImageBuffer;
use mm_core::{DecodedImage, ErrorCode};

pub const THUMB_MAX_EDGE: u32 = 512;

/// 生成缩略图，返回（WebP 字节、宽、高）
pub fn make_thumbnail(photo: &DecodedImage) -> Result<(Vec<u8>, u32, u32), ErrorCode> {
    let src = ImageBuffer::<image::Rgb<u8>, Vec<u8>>::from_raw(
        photo.width,
        photo.height,
        photo.rgb.clone(),
    )
    .ok_or(ErrorCode::DecodeFailed)?;

    let (w, h) = fit_within(photo.width, photo.height, THUMB_MAX_EDGE);
    let thumb = image::imageops::resize(&src, w, h, image::imageops::FilterType::Triangle);

    let mut out = Vec::new();
    let mut cursor = Cursor::new(&mut out);
    let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut cursor);
    encoder
        .encode(&thumb.into_raw(), w, h, image::ExtendedColorType::Rgb8)
        .map_err(|_| ErrorCode::WriteFailed)?;
    Ok((out, w, h))
}

/// 等比缩入 max_edge 内（不放大）
fn fit_within(w: u32, h: u32, max_edge: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= max_edge || long == 0 {
        return (w.max(1), h.max(1));
    }
    let scale = f64::from(max_edge) / f64::from(long);
    (
        (f64::from(w) * scale).round() as u32,
        (f64::from(h) * scale).round() as u32,
    )
}

/// 缩略图存储 key：`{sha 前 2 位}/{sha}.webp`（二级目录避免单目录文件过多）
pub fn thumb_key(sha256: &str) -> String {
    let (head, _) = sha256.split_at(2.min(sha256.len()));
    format!("{head}/{sha256}.webp")
}

/// 编码格式嗅探（测试与降级判断用）
pub fn is_webp(bytes: &[u8]) -> bool {
    bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32) -> DecodedImage {
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                rgb.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, 128]);
            }
        }
        DecodedImage {
            width: w,
            height: h,
            rgb,
        }
    }

    #[test]
    fn thumbnail_is_webp_within_512() {
        let photo = gradient(2048, 1024);
        let (bytes, w, h) = make_thumbnail(&photo).unwrap();
        assert_eq!((w, h), (512, 256));
        assert_eq!(&bytes[0..4], b"RIFF"); // WebP 容器头
        assert_eq!(&bytes[8..12], b"WEBP");
        assert!(is_webp(&bytes));
    }

    #[test]
    fn small_images_are_not_upscaled() {
        let photo = gradient(100, 50);
        let (bytes, w, h) = make_thumbnail(&photo).unwrap();
        assert_eq!((w, h), (100, 50));
        assert!(!bytes.is_empty());
    }

    #[test]
    fn thumb_key_shards_by_prefix() {
        assert_eq!(thumb_key("abcdef"), "ab/abcdef.webp");
    }

    #[test]
    fn fit_preserves_ratio() {
        assert_eq!(fit_within(3000, 2000, 512), (512, 341));
        assert_eq!(fit_within(50, 50, 512), (50, 50));
    }
}
