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
    pub model_dir: PathBuf,
    /// 分发源（依次尝试）：默认 GitHub Releases，可经 config.toml 整体替换
    pub model_endpoints: Vec<String>,
    /// 已加载的索引器集合（ADR-0013；下载完成后由装配层填充，可多套并存）
    pub indexers: Arc<std::sync::RwLock<Vec<Arc<dyn mm_core::Indexer>>>>,
    /// 用户选择的推理后端（config.toml inference_ep，缺省 CPU；spec 0008）
    pub ep: mm_embed::EpKind,
    /// 最近一次装配的 EP 降级原因（所选 EP 失败回落 CPU 时非空，inference_info 消费）
    pub ep_degraded: std::sync::Mutex<Option<String>>,
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
    pub fn build(
        app: &tauri::AppHandle,
        workspace: ResolvedPaths,
        model_dir: PathBuf,
        model_endpoints: Vec<String>,
        ep: mm_embed::EpKind,
    ) -> Self {
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
            model_dir,
            model_endpoints,
            indexers: Arc::new(std::sync::RwLock::new(Vec::new())),
            ep,
            ep_degraded: std::sync::Mutex::new(None),
        }
    }

    /// 尝试加载索引模型（ADR-0013）：按 index_meta 注册表逐个装配，
    /// 文件缺失的索引跳过（其队列等待下载完成后再装配）。返回缺失文件（空 = 全就绪）。
    pub fn load_indexers(&self) -> Result<(), Vec<String>> {
        let manifest = mm_embed::manifest::manifest();
        let missing = manifest.missing_files(&self.model_dir);
        if !missing.is_empty() {
            return Err(missing);
        }
        let store = mm_store::Store::open(&self.workspace.db_path())
            .map_err(|_| vec!["store_failed".to_string()])?;
        let mut loaded = self.indexers.write().unwrap();
        for meta in store
            .list_active_indexes()
            .map_err(|_| vec!["store_failed".to_string()])?
        {
            // 当前内置清单只覆盖 CLIP 索引；其余索引待其模型文件就绪后由未来注册流程装配
            if meta.slug != "chinese-clip-vit-b16-int8"
                || loaded.iter().any(|ix| ix.index_id() == meta.index_id)
            {
                continue;
            }
            match mm_embed::ClipEmbedder::load(&self.model_dir, &manifest, meta.index_id, self.ep) {
                Ok((ix, degraded)) => {
                    if let Some(reason) = degraded {
                        tracing::warn!(index_id = meta.index_id, slug = %meta.slug, reason, "推理后端降级");
                        *self.ep_degraded.lock().unwrap() = Some(reason);
                    }
                    // 成功也留痕：实机出现过「已就绪→未就绪」静默翻转且日志无据可查
                    tracing::info!(index_id = meta.index_id, slug = %meta.slug, ep = self.ep.as_str(), "索引装配成功");
                    loaded.push(Arc::new(ix));
                }
                Err(_) => return Err(vec!["load_failed".to_string()]),
            }
        }
        Ok(())
    }

    /// 是否至少加载了一套索引
    pub fn has_loaded_indexer(&self) -> bool {
        !self.indexers.read().unwrap().is_empty()
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
    /// 语义索引是否已建立（全部 active 索引均有向量；false=网格显示建索引呼吸点）
    pub indexed: bool,
}

/// 给一组资产行打索引状态标记：全部 active 索引都有向量才算已索引
pub fn mark_indexed(store: &mm_store::Store, summaries: &mut [AssetSummary], asset_ids: &[i64]) {
    let mut shared: Option<std::collections::HashSet<i64>> = None;
    for idx in store.list_active_indexes().unwrap_or_default() {
        let set: std::collections::HashSet<i64> = store
            .embedded_ids(idx.index_id, asset_ids)
            .unwrap_or_default()
            .into_iter()
            .collect();
        shared = Some(match shared {
            None => set,
            Some(prev) => prev.intersection(&set).copied().collect(),
        });
    }
    let indexed = shared.unwrap_or_default();
    for s in summaries.iter_mut() {
        s.indexed = indexed.contains(&(s.asset_id as i64));
    }
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
pub struct SearchHit {
    pub summary: AssetSummary,
    /// RRF 归一分（0~1），越大越相关
    pub score: f64,
    /// 命中来源：both / semantic / text（排序可解释，白皮书 §4.6）
    pub matched: String,
    /// 结果元数据（UI 对齐 v6：文件名 / 大小 / 所属工作区）
    pub file_name: String,
    pub size_bytes: f64,
    pub folder_id: i32,
    pub folder_label: String,
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
