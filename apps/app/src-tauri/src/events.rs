//! IPC 事件 DTO（tauri-specta Event）：引擎事件 → 前端订阅的桥。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct ImportProgressEvent {
    pub job_id: u32,
    pub total: u32,
    pub done: u32,
    pub failed: u32,
    pub eta_seconds: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct ImportPausedEvent {
    pub job_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct ImportResumedEvent {
    pub job_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct ImportFinishedEvent {
    pub job_id: u32,
    pub failed_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct ImportItemFailedEvent {
    pub job_id: u32,
    pub path: String,
    pub error_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct ModelDownloadProgressEvent {
    pub file: String,
    pub received: i32,
    pub total: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct ModelReadyEvent {}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct EmbedProgressEvent {
    pub done: i32,
    pub total: i32,
}

/// 来源文件夹状态变化（online/offline/missing）→ 侧栏状态点与灯箱状态条刷新
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct FolderStatusChangedEvent {
    pub folder_id: i32,
    /// online | offline | missing
    pub status: String,
}

/// 运行时包下载进度（spec 0008 §3.3；复用模型下载进度模式）
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct RuntimeDownloadProgressEvent {
    pub received: i32,
    pub total: i32,
}

/// 运行时包就绪（下载/导入完成并通过校验）
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct RuntimeReadyEvent {}

/// 模型本地导入完成且装配成功
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct ModelsImportedEvent {}
