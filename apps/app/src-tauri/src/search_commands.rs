//! P2/P3 命令：模型状态/下载、混合检索（语义流 + 文本流 → RRF 融合）、重建索引。
//! 规格 0003、0004、0005。

use mm_core::search::rrf_fuse;
use mm_store::Store;
use serde::{Deserialize, Serialize};
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

/// 检索过滤器（日期区间 / 类型）
#[derive(Debug, Clone, Default, Deserialize, Type)]
pub struct SearchFilters {
    pub taken_from: Option<f64>,
    pub taken_to: Option<f64>,
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct SearchPage {
    pub items: Vec<SearchHit>,
    pub model_ready: bool,
    pub pending_indexing: i32,
    pub available_years: Vec<String>,
}

/// 混合检索（规格 0003/0005）：语义流 + 文本流 → RRF（k=60）融合，过滤前置
#[tauri::command]
#[specta::specta]
pub async fn search_assets(
    state: State<'_, AppState>,
    query: String,
    top_k: Option<u32>,
    filters: Option<SearchFilters>,
) -> Result<SearchPage, String> {
    let empty = |model_ready: bool| SearchPage {
        items: Vec::new(),
        model_ready,
        pending_indexing: 0,
        available_years: Vec::new(),
    };
    let Some(embedder) = state.embedder.get().cloned() else {
        return Ok(empty(false));
    };
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(empty(true));
    }

    let store = store_at(&state)?;
    let k = top_k.unwrap_or(100).clamp(1, 500);
    let filters = filters.unwrap_or_default();

    let passes_filters = |row: &mm_store::AssetRow| -> bool {
        if let Some(from) = filters.taken_from {
            if (row.taken_at as f64) < from {
                return false;
            }
        }
        if let Some(to) = filters.taken_to {
            if (row.taken_at as f64) > to {
                return false;
            }
        }
        if let Some(ref kind) = filters.kind {
            if !kind.is_empty() {
                let matches_kind = match row.kind {
                    mm_core::AssetKind::Photo => kind == "photo",
                    mm_core::AssetKind::Video => kind == "video",
                    mm_core::AssetKind::Audio => kind == "audio",
                };
                if !matches_kind {
                    return false;
                }
            }
        }
        true
    };

    // ---- 语义流 ----
    let qvec = embedder
        .embed_text(trimmed)
        .map_err(|c| c.slug().to_string())?;
    let semantic_ids: Vec<i64> = store
        .knn_search(&qvec, k)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter_map(|(asset_id, _)| store.get_asset(asset_id).ok().flatten())
        .filter(|row| passes_filters(row))
        .map(|row| row.asset_id)
        .collect();

    // ---- 文本流（文件名；FTS 语法已由 build_match_query 清洗）----
    let match_query = Store::build_match_query(trimmed);
    let text_ids: Vec<i64> = if match_query.is_empty() {
        Vec::new()
    } else {
        store
            .search_fts(&match_query, k)
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter_map(|asset_id| store.get_asset(asset_id).ok().flatten())
            .filter(|row| passes_filters(row))
            .map(|row| row.asset_id)
            .collect()
    };

    // ---- RRF 融合（k=60，白皮书 §4.6）----
    let sem_ids: Vec<u64> = semantic_ids.iter().map(|i| *i as u64).collect();
    let txt_ids: Vec<u64> = text_ids.iter().map(|i| *i as u64).collect();
    let fused = rrf_fuse(&sem_ids, &txt_ids, 60.0);
    let max_score = fused.first().map(|h| h.score).unwrap_or(1.0).max(1e-9);

    let items = fused
        .into_iter()
        .filter_map(|hit| {
            let row = store
                .get_asset(i64::try_from(hit.asset_id).unwrap_or(0))
                .ok()
                .flatten()?;
            Some(SearchHit {
                summary: AssetSummary {
                    asset_id: i32::try_from(hit.asset_id).unwrap_or(0),
                    thumb_path: row.thumb_key.map(|key| {
                        crate::state::thumbs_abs_path(&state.workspace, &key)
                            .to_string_lossy()
                            .into_owned()
                    }),
                    width: row.width,
                    height: row.height,
                    taken_at: row.taken_at as f64,
                },
                score: (hit.score / max_score * 1000.0).round() / 1000.0,
                matched: hit.matched.slug().to_string(),
            })
        })
        .collect();

    let pending = store
        .list_ready_without_embedding(u32::MAX)
        .map(|v| v.len() as i32)
        .unwrap_or(0);
    let mut available_years: Vec<String> = Vec::new();
    for row in store.list_page(None, 500).unwrap_or_default() {
        if !available_years.contains(&row.year) {
            available_years.push(row.year.clone());
        }
    }

    Ok(SearchPage {
        items,
        model_ready: true,
        pending_indexing: pending,
        available_years,
    })
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
