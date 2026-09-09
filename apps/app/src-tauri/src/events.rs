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

/// 向量化进度（每张写完发一次）：进度卡按工作区分组逐张推进
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct EmbedProgressEvent {
    /// 该索引全库已嵌入资产数（绝对值）
    pub done: i32,
    /// 该索引全库应嵌总数（done + 队列剩余，仅在线工作区；冷却中的恒败项不计）
    pub total: i32,
    /// 本次会话累计解码失败数（源文件不可读等；重启清零）
    pub failed: i32,
    /// 刚完成这张的归属工作区（-1 = 与本事件无关的内部更新）
    pub folder_id: i32,
    /// 刚完成这张的文件名（进度卡逐张滚动展示）
    pub file_name: String,
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
