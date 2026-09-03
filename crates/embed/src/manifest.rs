//! 模型资产清单与状态检查。
//! 清单内置于二进制（include_str!），资产文件由发布流程上传到分发源（规格 0004）。

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ModelManifest {
    pub version: u32,
    pub model_id: String,
    pub embedding_dim: usize,
    /// 文本编码 context length（Chinese-CLIP ViT-B/16 = 52）
    pub context_length: Option<usize>,
    /// 视觉模型输入是否固定 224×224（决定 center-crop 与批推理）
    pub visual_fixed_square: bool,
    /// 分发路径（相对分发源 base URL）
    pub files: Vec<ModelFile>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ModelFile {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}

/// 内置清单：随应用锁定（ADR-0006 模型随应用版本锁定）
pub const MANIFEST_JSON: &str = include_str!("../assets/model-manifest.json");

pub fn manifest() -> ModelManifest {
    serde_json::from_str(MANIFEST_JSON).expect("内置模型清单损坏")
}

impl ModelManifest {
    /// 检查本地模型目录：哪些文件缺失或校验不符
    pub fn missing_files(&self, model_dir: &std::path::Path) -> Vec<String> {
        use sha2::{Digest, Sha256};
        let mut missing = Vec::new();
        for f in &self.files {
            let path = model_dir.join(&f.name);
            let ok = (|| -> Option<bool> {
                use std::io::Read;
                let mut file = std::fs::File::open(&path).ok()?;
                let mut hasher = Sha256::new();
                let mut buf = [0u8; 64 * 1024];
                loop {
                    match file.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => hasher.update(&buf[..n]),
                        Err(_) => return Some(false),
                    }
                }
                Some(hex::encode(hasher.finalize()) == f.sha256)
            })()
            .unwrap_or(false);
            if !ok {
                missing.push(f.name.clone());
            }
        }
        missing
    }
}
