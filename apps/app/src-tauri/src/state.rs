//! 应用状态与引擎事件桥：把 `mm-core::EventSink` 事件转成 tauri/specta 事件。
//!
//! AppState 在 tauri `setup` 中构造（TauriSink 需要 AppHandle）。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use mm_core::{ErrorCode, EventSink, PipelineEvent};
use mm_pipeline::{ImportConfig, ImportEngine};
use mm_platform::ResolvedPaths;
use serde::Serialize;

use crate::events::{
    ImportFinishedEvent, ImportItemFailedEvent, ImportPausedEvent, ImportProgressEvent,
    ImportResumedEvent,
};

pub struct AppState {
    pub engine: Arc<ImportEngine>,
    pub workspace: ResolvedPaths,
    pub sink: Arc<TauriSink>,
}

struct SystemClock;
impl mm_core::Clock for SystemClock {
    fn now_unix(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

impl AppState {
    pub fn build(app: &tauri::AppHandle, workspace: ResolvedPaths) -> Self {
        let sink = TauriSink::new(app.clone());
        let engine = ImportEngine::new(
            workspace.db_path(),
            workspace.thumbs_dir(),
            sink.clone(),
            Arc::new(SystemClock),
            ImportConfig::default(),
        );
        Self {
            engine: Arc::new(engine),
            workspace,
            sink,
        }
    }
}

/// 引擎事件 → specta 事件桥
pub struct TauriSink {
    app: tauri::AppHandle,
    job_id: AtomicU64,
    started_at: Mutex<Option<Instant>>,
}

impl TauriSink {
    pub fn new(app: tauri::AppHandle) -> Arc<Self> {
        Arc::new(Self {
            app,
            job_id: AtomicU64::new(0),
            started_at: Mutex::new(None),
        })
    }

    pub fn begin_job(&self, job_id: u64) {
        self.job_id.store(job_id, Ordering::SeqCst);
        *self.started_at.lock().unwrap() = Some(Instant::now());
    }
}

impl EventSink for TauriSink {
    fn emit(&self, event: PipelineEvent) -> Result<(), ErrorCode> {
        use tauri_specta::Event;
        let job_id = self.job_id.load(Ordering::SeqCst);
        match event {
            PipelineEvent::Progress {
                total,
                done,
                failed,
            } => {
                let eta = self
                    .started_at
                    .lock()
                    .unwrap()
                    .map(|t| t.elapsed())
                    .and_then(|e| mm_pipeline::eta_seconds(e.as_secs(), done, total));
                ImportProgressEvent {
                    job_id: u32::try_from(job_id).unwrap_or(0),
                    total: total as u32,
                    done: done as u32,
                    failed: failed as u32,
                    eta_seconds: eta.map(|v| v as u32),
                }
                .emit(&self.app)
                .ok();
            }
            PipelineEvent::ItemFailed { path, code } => {
                ImportItemFailedEvent {
                    job_id: u32::try_from(job_id).unwrap_or(0),
                    path,
                    error_code: code.slug().to_string(),
                }
                .emit(&self.app)
                .ok();
            }
            PipelineEvent::Paused => {
                ImportPausedEvent {
                    job_id: job_id as u32,
                }
                .emit(&self.app)
                .ok();
            }
            PipelineEvent::Resumed => {
                ImportResumedEvent {
                    job_id: job_id as u32,
                }
                .emit(&self.app)
                .ok();
            }
            PipelineEvent::Finished { failed_count } => {
                ImportFinishedEvent {
                    job_id: job_id as u32,
                    failed_count: failed_count as u32,
                }
                .emit(&self.app)
                .ok();
            }
        }
        Ok(())
    }
}

// ---- 读查询/命令共用的 DTO ----

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct AssetSummary {
    /// IPC 契约用 i32/f64（specta 禁 i64 导出；内部仍是 i64，量级远不触及边界）
    pub asset_id: i32,
    pub thumb_path: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub taken_at: f64,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct YearGroup {
    pub year: u16,
    pub items: Vec<AssetSummary>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct TimelinePage {
    pub groups: Vec<YearGroup>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct FailedItem {
    pub asset_id: i32,
    pub path: String,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct DeleteReport {
    pub deleted: i32,
    pub missing: i32,
}

pub fn thumbs_abs_path(workspace: &ResolvedPaths, key: &str) -> PathBuf {
    workspace
        .thumbs_dir()
        .join(key.replace('/', std::path::MAIN_SEPARATOR_STR))
}
