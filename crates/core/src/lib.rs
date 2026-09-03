//! `mm-core`：领域类型与端口 trait（ADR-0005）。
//!
//! 铁律：零 IO、零 async、不依赖 tauri / ort / 数据库；只定义"是什么"，
//! 不定义"怎么读写"。所有可变性都封在端口（trait）之后。

use serde::{Deserialize, Serialize};

/// 资产主键（自增，跨 vec_assets / fts_text 对齐）
pub type AssetId = u64;

/// 媒体种类（白皮书 §4.5）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetKind {
    Photo,
    Video,
    Audio,
}

/// 资产生命周期状态（白皮书 §4.5：pending | indexing | ready | failed）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetStatus {
    Pending,
    Indexing,
    Ready,
    Failed,
}

/// 资产元数据（与 `assets` 表对应，白皮书 §4.5）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub asset_id: AssetId,
    /// 内容哈希，跨端去重主键
    pub sha256: String,
    /// 相对路径（本地或未来 S3）
    pub storage_key: String,
    pub kind: AssetKind,
    pub width: u32,
    pub height: u32,
    /// 拍摄时间 UTC 秒；缺失时回落文件 mtime
    pub taken_at: i64,
    /// 分区键（拍摄年份，来自 taken_at）
    pub year: u16,
    pub thumb_key: Option<String>,
    pub status: AssetStatus,
    pub imported_at: i64,
}

/// 解码后的图像缓冲（`libheif`/`image` 统一产出，供缩略图与模型预处理共用）
#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// RGB8，行优先
    pub rgb: Vec<u8>,
}

/// pipeline 进度事件（`EventSink` 载荷；装配层桥接为 tauri 事件）
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum PipelineEvent {
    Progress { total: u64, done: u64, failed: u64 },
    ItemFailed { path: String, code: ErrorCode },
    Paused,
    Resumed,
    Finished { failed_count: u64 },
}

/// 用户可见错误码（ADR-0005 错误分类学：UI 映射为"发生了什么 + 用户能做什么"）
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    #[error("文件解码失败")]
    DecodeFailed,
    #[error("读取文件失败")]
    ReadFailed,
    #[error("写入存储失败")]
    WriteFailed,
    #[error("数据库读写失败")]
    StoreFailed,
    #[error("导入已取消")]
    ImportCancelled,
    #[error("已有导入任务进行中")]
    ImportBusy,
    #[error("模型未就绪")]
    ModelMissing,
    #[error("模型下载失败")]
    ModelDownloadFailed,
    #[error("语义搜索暂不可用")]
    SearchUnavailable,
    #[error("未知错误")]
    Unknown,
}

impl ErrorCode {
    /// 机器码（与 serde snake_case 一致）；入库 error_code 列与日志用
    pub fn slug(&self) -> &'static str {
        match self {
            ErrorCode::DecodeFailed => "decode_failed",
            ErrorCode::ReadFailed => "read_failed",
            ErrorCode::WriteFailed => "write_failed",
            ErrorCode::StoreFailed => "store_failed",
            ErrorCode::ImportCancelled => "import_cancelled",
            ErrorCode::ImportBusy => "import_busy",
            ErrorCode::ModelMissing => "model_missing",
            ErrorCode::ModelDownloadFailed => "model_download_failed",
            ErrorCode::SearchUnavailable => "search_unavailable",
            ErrorCode::Unknown => "unknown",
        }
    }
}

// ---- 端口（ADR-0005：全项目仅 5 个，多一个都是过度设计） ----

/// 存储抽象：MVP 仅 `LocalDiskAdapter` 一个实现；S3 为 LATER（白皮书 §3.5）
pub trait StorageAdapter {
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), ErrorCode>;
    fn get(&self, key: &str) -> Result<Vec<u8>, ErrorCode>;
    fn delete(&self, key: &str) -> Result<(), ErrorCode>;
}

/// 推理抽象：唯一实现为 `crates/embed`（P2，ONNX Runtime）
pub trait Embedder {
    fn embed_images(&self, batch: &[DecodedImage]) -> Result<Vec<Vec<f32>>, ErrorCode>;
}

/// 向量索引抽象：唯一实现为 sqlite-vec（ADR-0002；更新 = 删除 + 重插）
pub trait VectorIndex {
    fn add(&mut self, id: AssetId, vec: &[i8]) -> Result<(), ErrorCode>;
    fn remove(&mut self, id: AssetId) -> Result<(), ErrorCode>;
    fn search(&self, query: &[f32], top_k: usize) -> Result<Vec<(AssetId, f32)>, ErrorCode>;
}

/// 进度事件出口：P1 实现为 tauri 事件桥（引擎线程跨线程调用，需 Send + Sync）
pub trait EventSink: Send + Sync {
    fn emit(&self, event: PipelineEvent) -> Result<(), ErrorCode>;
}

/// 时钟抽象：测试时可注入固定时钟
pub trait Clock: Send + Sync {
    fn now_unix(&self) -> i64;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct CollectingSink(Mutex<Vec<PipelineEvent>>);

    impl EventSink for CollectingSink {
        fn emit(&self, event: PipelineEvent) -> Result<(), ErrorCode> {
            self.0.lock().unwrap().push(event);
            Ok(())
        }
    }

    #[test]
    fn event_sink_collects_events_in_order() {
        let sink = CollectingSink(Mutex::new(vec![]));
        sink.emit(PipelineEvent::Progress { total: 10, done: 1, failed: 0 }).unwrap();
        sink.emit(PipelineEvent::ItemFailed { path: "x".into(), code: ErrorCode::DecodeFailed })
            .unwrap();
        sink.emit(PipelineEvent::Paused).unwrap();
        sink.emit(PipelineEvent::Resumed).unwrap();
        sink.emit(PipelineEvent::Finished { failed_count: 0 }).unwrap();
        assert_eq!(sink.0.lock().unwrap().len(), 5);
        assert!(matches!(sink.0.lock().unwrap()[2], PipelineEvent::Paused));
    }

    struct FixedClock(i64);

    impl Clock for FixedClock {
        fn now_unix(&self) -> i64 {
            self.0
        }
    }

    #[test]
    fn clock_is_injectable() {
        let clock = FixedClock(1_760_000_000);
        assert_eq!(clock.now_unix(), 1_760_000_000);
    }
}
