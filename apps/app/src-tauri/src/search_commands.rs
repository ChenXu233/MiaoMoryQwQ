//! P2 命令：模型状态/下载、语义搜索、重建索引（规格 0003、0004）。

use mm_store::Store;
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::events::{ModelDownloadProgressEvent, ModelReadyEvent};
use crate::state::{AppState, AssetSummary, SearchHit};

fn store_at(state: &AppState) -> Result<Store, String> {
    Store::open(&state.workspace.db_path()).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ModelStatus {
    pub ready: bool,
    pub loaded: bool,
    pub files_missing: Vec<String>,
}

#[tauri::command]
#[specta::specta]
pub fn model_status(state: State<'_, AppState>) -> ModelStatus {
    let manifest = mm_embed::manifest::manifest();
    let missing = manifest.missing_files(&state.model_dir);
    ModelStatus {
        ready: missing.is_empty(),
        loaded: state.embedder.get().is_some(),
        files_missing: missing,
    }
}

/// 下载模型资产（后台线程；进度经事件上报，完成后加载并广播就绪）
#[tauri::command]
#[specta::specta]
pub fn download_models(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state.embedder.get().is_some() {
        return Ok(()); // 已就绪，幂等
    }
    let endpoints = state.model_endpoints.clone();
    let model_dir = state.model_dir.clone();
    let embedder_slot = state.embedder.clone();
    let app2 = app.clone();

    std::thread::spawn(move || {
        let manifest = mm_embed::manifest::manifest();
        let downloader = mm_embed::download::ModelDownloader::new(endpoints, model_dir.clone());
        let progress = |file: &str, received: u64, total: u64| {
            let _ = ModelDownloadProgressEvent {
                file: file.to_string(),
                received: i32::try_from(received).unwrap_or(i32::MAX),
                total: i32::try_from(total).unwrap_or(i32::MAX),
            }
            .emit(&app2);
        };
        if downloader.ensure_all(&manifest, &progress).is_err() {
            tracing::warn!("模型下载失败");
            return;
        }
        match mm_embed::ClipEmbedder::load(&model_dir, &manifest) {
            Ok(embedder) => {
                let _ = embedder_slot.set(std::sync::Arc::new(embedder));
                let _ = ModelReadyEvent {}.emit(&app2);
            }
            Err(_) => tracing::warn!("模型加载失败"),
        }
    });
    Ok(())
}

/// 中文语义搜索（规格 0003）：文本编码 → KNN → join assets
#[tauri::command]
#[specta::specta]
pub async fn search_assets(
    state: State<'_, AppState>,
    query: String,
    top_k: Option<u32>,
) -> Result<SearchPage, String> {
    let empty = || SearchPage {
        items: Vec::new(),
        model_ready: false,
        pending_indexing: 0,
    };
    if query.trim().is_empty() {
        return Ok(empty());
    }
    let Some(embedder) = state.embedder.get().cloned() else {
        return Ok(empty());
    };

    let qvec = embedder
        .embed_text(&query)
        .map_err(|c| c.slug().to_string())?;
    let store = store_at(&state)?;
    let k = top_k.unwrap_or(100);
    let hits = store.knn_search(&qvec, k).map_err(|e| e.to_string())?;
    let pending = store
        .list_ready_without_embedding(u32::MAX)
        .map(|v| v.len() as i32)
        .unwrap_or(0);

    let items = hits
        .into_iter()
        .filter_map(|(asset_id, distance)| {
            let row = store.get_asset(asset_id).ok()??;
            let score = (1.0 - distance / 4.0).clamp(0.0, 1.0); // L2² ∈ [0,4] → 归一相似度
            Some(SearchHit {
                summary: AssetSummary {
                    asset_id: i32::try_from(row.asset_id).ok()?,
                    thumb_path: row.thumb_key.map(|key| {
                        crate::state::thumbs_abs_path(&state.workspace, &key)
                            .to_string_lossy()
                            .into_owned()
                    }),
                    width: row.width,
                    height: row.height,
                    taken_at: row.taken_at as f64,
                },
                score: score as f64,
            })
        })
        .collect();

    Ok(SearchPage {
        items,
        model_ready: true,
        pending_indexing: pending,
    })
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct SearchPage {
    pub items: Vec<SearchHit>,
    pub model_ready: bool,
    pub pending_indexing: i32,
}

/// 重建全部向量索引（模型变更/量化策略变更时）；worker 轮询发现空队列后全量重嵌
#[tauri::command]
#[specta::specta]
pub async fn reindex_all(state: State<'_, AppState>) -> Result<i32, String> {
    let store = store_at(&state)?;
    let count = store.count_embedded().map_err(|e| e.to_string())?;
    store.clear_embeddings().map_err(|e| e.to_string())?;
    Ok(count as i32)
}
