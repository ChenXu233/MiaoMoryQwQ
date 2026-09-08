//! 缩略图生成：长边 384px，JPEG 有损 q80（v5 裁定：缩略图与照片强绑定不逐出，
//! 体积是永久成本 → 有损小图；152px 网格与灯箱离线预览均够用）。
//! 历史兼容：旧 .webp key 的存量缩略图照常显示（懒迁移——只有重新生成才写 .jpg）。

use std::io::Cursor;

use image::ImageBuffer;
use mm_core::{DecodedImage, ErrorCode};

pub const THUMB_MAX_EDGE: u32 = 384;
const JPEG_QUALITY: u8 = 80;

/// 生成缩略图，返回（JPEG 字节、宽、高）。
/// 消费 `DecodedImage`（调用方在缩略图后不再用像素，免 36MB 级整缓冲克隆）；
/// 大倍率缩小走 box 采样（`imageops::thumbnail`：每输出像素平均源矩形），
/// 采样次数远少于通用采样 resize 的大支撑核，且高倍率下抗锯齿更稳。
pub fn make_thumbnail(photo: DecodedImage) -> Result<(Vec<u8>, u32, u32), ErrorCode> {
    let src =
        ImageBuffer::<image::Rgb<u8>, Vec<u8>>::from_raw(photo.width, photo.height, photo.rgb)
            .ok_or(ErrorCode::DecodeFailed)?;

    let (w, h) = fit_within(photo.width, photo.height, THUMB_MAX_EDGE);
    let thumb = image::imageops::thumbnail(&src, w, h);

    let mut out = Vec::new();
    let mut cursor = Cursor::new(&mut out);
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, JPEG_QUALITY);
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

/// 缩略图存储 key：`{sha 前 2 位}/{sha}.jpg`（二级目录避免单目录文件过多）。
/// 存量行可能是 `.webp`（v5b 之前），以 DB thumb_key 为准、不迁移文件。
pub fn thumb_key(sha256: &str) -> String {
    let (head, _) = sha256.split_at(2.min(sha256.len()));
    format!("{head}/{sha256}.jpg")
}

/// 编码格式嗅探（测试与降级判断用）
pub fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.len() > 3 && bytes[0] == 0xFF && bytes[1] == 0xD8
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
    fn thumbnail_is_jpeg_within_384() {
        let photo = gradient(2048, 1024);
        let (bytes, w, h) = make_thumbnail(photo).unwrap();
        assert_eq!((w, h), (384, 192));
        assert!(is_jpeg(&bytes), "JPEG 魔数 FF D8");
    }

    #[test]
    fn small_images_are_not_upscaled() {
        let photo = gradient(100, 50);
        let (bytes, w, h) = make_thumbnail(photo).unwrap();
        assert_eq!((w, h), (100, 50));
        assert!(!bytes.is_empty());
    }

    #[test]
    fn jpeg_is_smuch_smaller_than_lossless_budget() {
        // 2048x1024 渐变：JPEG q80 输出应远小于无损 WebP（体积是永久成本，裁定 8）
        let photo = gradient(1024, 512);
        let (bytes, _, _) = make_thumbnail(photo).unwrap();
        assert!(bytes.len() < 300 * 1024, "实际 {} 字节", bytes.len());
    }

    #[test]
    fn thumb_key_uses_jpg_extension() {
        assert_eq!(thumb_key("abcdef"), "ab/abcdef.jpg");
    }

    #[test]
    fn fit_preserves_ratio() {
        assert_eq!(fit_within(3000, 2000, 384), (384, 256));
        assert_eq!(fit_within(50, 50, 384), (50, 50));
    }
}
