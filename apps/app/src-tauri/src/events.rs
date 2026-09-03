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
