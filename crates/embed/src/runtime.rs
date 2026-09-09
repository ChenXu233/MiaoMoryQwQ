//! 推理运行时分发（spec 0008 / ADR-0014）：onnxruntime 变体 zip 的下载、
//! 校验、解压与本地导入；模型文件的本地导入校验。
//! 复用模型下载器的 `.part` 断点续传 + SHA-256 校验模式（spec 0004）。

use std::io::Read;
use std::path::{Path, PathBuf};

use mm_core::ErrorCode;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::download::{ModelDownloader, ProgressFn};
use crate::manifest::{ModelFile, ModelManifest};

/// 运行时变体清单（内嵌；来源 = 微软官方 GitHub Release zip，ADR-0014）
#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeManifest {
    pub version: u32,
    pub onnxruntime: String,
    pub variants: Vec<RuntimeVariant>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeVariant {
    /// 变体标识：cpu | cuda（dml 为安装包随附/自建产物，不在按需分发清单）
    pub kind: String,
    /// 长度恒 1：一个 zip 条目
    pub files: Vec<ModelFile>,
    /// 解压后必须存在的 dll（存在性校验；zip 整体 sha 由 files 保证）
    pub expected_dlls: Vec<String>,
}

pub const RUNTIME_MANIFEST_JSON: &str = include_str!("../assets/runtime-manifest.json");

pub fn runtime_manifest() -> RuntimeManifest {
    serde_json::from_str(RUNTIME_MANIFEST_JSON).expect("内置运行时清单损坏")
}

impl RuntimeManifest {
    pub fn variant(&self, kind: &str) -> Option<&RuntimeVariant> {
        self.variants.iter().find(|v| v.kind == kind)
    }
}

impl RuntimeVariant {
    pub fn zip_entry(&self) -> &ModelFile {
        // 清单契约：每变体恰一个 zip 条目
        &self.files[0]
    }

    /// 变体是否已在本地就绪（全部期望 dll 存在）
    pub fn is_ready(&self, runtime_dir: &Path) -> bool {
        let dest = runtime_dir.join(&self.kind);
        self.expected_dlls
            .iter()
            .all(|d| dest.join(d).is_file())
    }
}

/// 下载并安装变体（幂等：dll 齐全即跳过；zip 已在则跳过下载仅重解压）。
/// `endpoints` = 分发源（默认微软官方 GitHub Release，可 config 覆盖）。
pub fn ensure_runtime(
    endpoints: &[String],
    variant: &RuntimeVariant,
    runtime_dir: &Path,
    on_progress: ProgressFn<'_>,
) -> Result<(), ErrorCode> {
    if variant.is_ready(runtime_dir) {
        return Ok(());
    }
    let zip_entry = variant.zip_entry();
    let dest = runtime_dir.join(&variant.kind);
    std::fs::create_dir_all(&dest).map_err(|_| ErrorCode::WriteFailed)?;

    // zip 已完整（sha 匹配）则跳过下载
    let zip_path = runtime_dir.join(&zip_entry.name);
    if !file_matches(&zip_path, &zip_entry.sha256) {
        let downloader = ModelDownloader::new(
            endpoints.to_vec(),
            runtime_dir.to_path_buf(),
        );
        downloader.ensure_all(&to_manifest(zip_entry), on_progress)?;
    }
    extract_dlls(&zip_path, &dest, &variant.expected_dlls)
}

/// 从本地 zip 导入变体：sha 命中清单走校验路径；不命中但结构合法（含全部
/// 期望 dll 名单里的 onnxruntime.dll）允许落地，由调用方标注「未校验来源」。
pub fn import_runtime_zip(
    zip_path: &Path,
    variant: &RuntimeVariant,
    runtime_dir: &Path,
) -> Result<bool, ErrorCode> {
    let verified = file_matches(zip_path, &variant.zip_entry().sha256);
    if !verified && !zip_is_plausible(zip_path, &variant.expected_dlls) {
        return Err(ErrorCode::ModelDownloadFailed);
    }
    let dest = runtime_dir.join(&variant.kind);
    std::fs::create_dir_all(&dest).map_err(|_| ErrorCode::WriteFailed)?;
    extract_dlls(zip_path, &dest, &variant.expected_dlls)?;
    Ok(verified)
}

/// 模型本地导入（spec 0008 §3.5）：按内置清单从目录/zip 校验并落入 `model_dir`。
pub struct ImportReport {
    pub imported: u32,
    pub skipped: u32,
    pub mismatched: Vec<(String, String)>,
}

pub fn import_models(
    source: &Path,
    manifest: &ModelManifest,
    model_dir: &Path,
) -> Result<ImportReport, ErrorCode> {
    let tmp = tempfile_dir();
    let dir: PathBuf = if source.is_dir() {
        source.to_path_buf()
    } else {
        extract_zip_to(source, &tmp).map_err(|_| ErrorCode::WriteFailed)?;
        tmp.clone()
    };

    let mut report = ImportReport {
        imported: 0,
        skipped: 0,
        mismatched: Vec::new(),
    };
    for f in &manifest.files {
        let src = dir.join(&f.name);
        if !src.is_file() {
            report
                .mismatched
                .push((f.name.clone(), "缺失".into()));
            continue;
        }
        if !file_matches(&src, &f.sha256) {
            report
                .mismatched
                .push((f.name.clone(), "SHA-256 校验不符".into()));
            continue;
        }
        let dest = model_dir.join(&f.name);
        if file_matches(&dest, &f.sha256) {
            report.skipped += 1; // 已存在同内容
            continue;
        }
        std::fs::copy(&src, &dest).map_err(|_| ErrorCode::WriteFailed)?;
        report.imported += 1;
    }
    if dir != source {
        let _ = std::fs::remove_dir_all(&dir);
    }
    Ok(report)
}

// ---- 内部 ----

fn to_manifest(zip_entry: &ModelFile) -> ModelManifest {
    // 复用 ensure_all 的下载/校验流程：单条目清单
    serde_json::from_value(serde_json::json!({
        "version": 1,
        "model_id": "runtime",
        "embedding_dim": 1,
        "context_length": null,
        "visual_fixed_square": true,
        "files": [{
            "name": zip_entry.name,
            "sha256": zip_entry.sha256,
            "size": zip_entry.size,
        }],
    }))
    .expect("runtime 单条目清单构造失败")
}

fn extract_dlls(zip_path: &Path, dest: &Path, expected: &[String]) -> Result<(), ErrorCode> {
    let file = std::fs::File::open(zip_path).map_err(|_| ErrorCode::ReadFailed)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| ErrorCode::ReadFailed)?;
    for name in expected {
        let idx = (0..archive.len()).find(|&i| {
            archive
                .by_index(i)
                .ok()
                .is_some_and(|e| {
                    e.is_file()
                        && Path::new(e.name())
                            .file_name()
                            .is_some_and(|f| f.to_string_lossy() == *name)
                })
        });
        let Some(idx) = idx else {
            return Err(ErrorCode::ReadFailed);
        };
        let mut reader = archive.by_index(idx).map_err(|_| ErrorCode::ReadFailed)?;
        let mut out = Vec::with_capacity(reader.size() as usize);
        reader
            .read_to_end(&mut out)
            .map_err(|_| ErrorCode::WriteFailed)?;
        std::fs::write(dest.join(name), out).map_err(|_| ErrorCode::WriteFailed)?;
    }
    Ok(())
}

/// 结构合法性粗检（未校验来源导入的最低门槛）：zip 可读且含 onnxruntime.dll
fn zip_is_plausible(zip_path: &Path, expected: &[String]) -> bool {
    if !expected.iter().any(|d| d == "onnxruntime.dll") {
        return false;
    }
    let file = match std::fs::File::open(zip_path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return false;
    };
    (0..archive.len()).any(|i| {
        archive
            .by_index(i)
            .ok()
            .is_some_and(|e| Path::new(e.name()).file_name().is_some_and(|f| f.to_string_lossy() == "onnxruntime.dll"))
    })
}

fn extract_zip_to(zip_path: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if !entry.is_file() {
            continue;
        }
        let name = entry
            .enclosed_name()
            .ok_or("zip 内路径非法")?
            .to_path_buf();
        let out = dest.join(name);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        std::fs::write(out, buf).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn tempfile_dir() -> PathBuf {
    std::env::temp_dir().join(format!(
        "miaomory-import-{}",
        std::process::id()
    ))
}

fn file_matches(path: &Path, expected_sha256: &str) -> bool {
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(_) => return false,
        }
    }
    if expected_sha256.is_empty() {
        return true;
    }
    hex::encode(hasher.finalize()) == expected_sha256
}
