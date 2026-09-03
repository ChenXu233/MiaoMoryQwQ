//! 解码模块（ADR-0008 纯 Rust 栈）：JPEG/PNG/WebP 走 `image`，HEIC/HEIF 走 `libheif-rs`。
//! EXIF：拍摄时间与方向（栅格图方向由 `image` 解码器应用；HEIC 由 libheif 解码时应用）。

use std::io::BufReader;
use std::path::Path;

use mm_core::{DecodedImage, ErrorCode};

/// 解码产物
pub struct DecodedPhoto {
    pub image: DecodedImage,
    pub mime: &'static str,
    /// EXIF 拍摄时间（UTC 秒）；无则由调用方回落 mtime
    pub taken_at: Option<i64>,
    /// 存档用最小 EXIF JSON（taken_at / orientation）
    pub exif_json: Option<String>,
}

pub const SUPPORTED_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "heic", "heif"];

pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SUPPORTED_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// 解码单张照片；统一产出 RGB8（`mm-core::DecodedImage`）
pub fn decode_photo(path: &Path) -> Result<DecodedPhoto, ErrorCode> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .ok_or(ErrorCode::DecodeFailed)?;
    match ext.as_str() {
        "jpg" | "jpeg" => decode_raster(path, "image/jpeg"),
        "png" => decode_raster(path, "image/png"),
        "webp" => decode_raster(path, "image/webp"),
        "heic" | "heif" => decode_heif(path),
        _ => Err(ErrorCode::DecodeFailed),
    }
}

fn decode_raster(path: &Path, mime: &'static str) -> Result<DecodedPhoto, ErrorCode> {
    use image::{DynamicImage, ImageDecoder, ImageReader};

    let file = std::fs::File::open(path).map_err(|_| ErrorCode::ReadFailed)?;
    let exif_source = std::io::Cursor::new(std::fs::read(path).map_err(|_| ErrorCode::ReadFailed)?);
    let (exif_meta, taken_at) = read_exif(&mut std::io::BufReader::new(exif_source));

    let reader = ImageReader::new(BufReader::new(file));
    let mut decoder = reader
        .with_guessed_format()
        .map_err(|_| ErrorCode::DecodeFailed)?
        .into_decoder()
        .map_err(|_| ErrorCode::DecodeFailed)?;
    // 解码器读出的 EXIF 方向在 apply_orientation 时落进像素
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut dynamic = DynamicImage::from_decoder(decoder).map_err(|_| ErrorCode::DecodeFailed)?;
    dynamic.apply_orientation(orientation);
    let rgb = dynamic.into_rgb8();
    let (width, height) = (rgb.width(), rgb.height());

    Ok(DecodedPhoto {
        image: DecodedImage {
            width,
            height,
            rgb: rgb.into_raw(),
        },
        mime,
        taken_at,
        exif_json: exif_meta,
    })
}

fn decode_heif(path: &Path) -> Result<DecodedPhoto, ErrorCode> {
    use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};

    let bytes = std::fs::read(path).map_err(|_| ErrorCode::ReadFailed)?;
    let lh = LibHeif::new();
    // libheif 解码时自动应用旋转/裁剪等几何变换（含 iPhone 方向）
    let ctx = HeifContext::read_from_bytes(&bytes).map_err(|_| ErrorCode::DecodeFailed)?;
    let handle = ctx
        .primary_image_handle()
        .map_err(|_| ErrorCode::DecodeFailed)?;

    // HEIC 内嵌 EXIF（TIFF 块，去掉 "Exif\0\0" 前缀后可解析）
    let mut ids = [0u32; 1];
    let exif_count = handle.metadata_block_ids(&mut ids, b"Exif");
    let (taken_at, exif_json) = if exif_count > 0 {
        match handle.metadata(ids[0]) {
            Ok(block) => {
                let tiff = strip_exif_prefix(&block);
                let (meta, taken) = read_exif(&mut std::io::Cursor::new(tiff));
                (taken, meta)
            }
            Err(_) => (None, None),
        }
    } else {
        (None, None)
    };

    let img = lh
        .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgb), None)
        .map_err(|_| ErrorCode::DecodeFailed)?;
    let planes = img.planes();
    let plane = planes.interleaved.ok_or(ErrorCode::DecodeFailed)?;
    let (width, height) = (plane.width, plane.height);
    let stride = plane.stride;
    let row_len = width as usize * 3;
    let mut rgb = Vec::with_capacity(row_len * height as usize);
    for y in 0..height as usize {
        let start = y * stride;
        let end = start + row_len;
        let row = plane.data.get(start..end).ok_or(ErrorCode::DecodeFailed)?;
        rgb.extend_from_slice(row);
    }

    Ok(DecodedPhoto {
        image: DecodedImage { width, height, rgb },
        mime: "image/heic",
        taken_at,
        exif_json,
    })
}

/// 解析 EXIF：返回（最小 JSON、拍摄时间 UTC 秒）
fn read_exif<R: std::io::BufRead + std::io::Seek>(reader: &mut R) -> (Option<String>, Option<i64>) {
    let exif = match exif::Reader::new().read_from_container(reader) {
        Ok(e) => e,
        Err(_) => return (None, None),
    };
    let mut json = serde_json::Map::new();

    let taken_at = exif
        .get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
        .and_then(|f| match &f.value {
            exif::Value::Ascii(vals) => vals.first().map(|b| b.as_slice()),
            _ => None,
        })
        .and_then(|bytes| {
            let trimmed: Vec<u8> = bytes.iter().copied().take_while(|&b| b != 0).collect();
            exif::DateTime::from_ascii(&trimmed)
                .ok()
                .map(|dt| utc_seconds(&dt))
        });

    let orientation = exif
        .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        .filter(|o| (1..=8).contains(o));

    if let Some(t) = taken_at {
        json.insert("taken_at".into(), serde_json::json!(t));
    }
    if let Some(o) = orientation {
        json.insert("orientation".into(), serde_json::json!(o));
    }
    let meta = if json.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(json).to_string())
    };
    (meta, taken_at)
}

/// 去掉 HEIF Exif item 的 "Exif\0\0" 前缀，露出 TIFF 头
fn strip_exif_prefix(block: &[u8]) -> &[u8] {
    if block.starts_with(b"Exif\0\0") {
        &block[6..]
    } else {
        block
    }
}

/// EXIF DateTime（无时区，按 UTC 解释）→ Unix 秒；days_from_civil（Hinnant）
fn utc_seconds(dt: &exif::DateTime) -> i64 {
    let (y, m) = if dt.month <= 2 {
        (i64::from(dt.year) - 1, i64::from(dt.month) + 12)
    } else {
        (i64::from(dt.year), i64::from(dt.month))
    };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + i64::from(dt.hour) * 3600 + i64::from(dt.minute) * 60 + i64::from(dt.second)
}
