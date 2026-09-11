//! P2/P3 命令：模型状态/下载、混合检索（语义流 + 文本流 → RRF 融合）、重建索引。
//! 规格 0003、0004、0005。

use mm_store::Store;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
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
        loaded: state.has_loaded_indexer(),
        files_missing: missing,
    }
}

/// 下载任务在跑的标记：启动自动触发与手动按钮并发时防双重下载（spec 0004 验收 7）
static DOWNLOAD_IN_FLIGHT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 下载模型资产（后台线程；进度经事件上报，完成后加载并广播就绪）。
/// 幂等：已就绪或已有下载任务时直接返回。启动时模型缺失即自动调用（spec 0004 修订）。
#[tauri::command]
#[specta::specta]
pub fn download_models(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    if state.has_loaded_indexer() {
        return Ok(()); // 已就绪，幂等
    }
    if DOWNLOAD_IN_FLIGHT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(()); // 已有下载任务在跑
    }
    let endpoints = state.model_endpoints.clone();
    let model_dir = state.model_dir.clone();
    let app_for_load = app.clone();
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
            tracing::warn!(code = "model_download_failed", "模型下载失败");
            DOWNLOAD_IN_FLIGHT.store(false, Ordering::SeqCst); // 允许手动重试
            return;
        }
        // 索引装配统一走 state.load_indexers（ADR-0013）
        let loaded = {
            let st = app_for_load.state::<AppState>();
            st.load_indexers()
        };
        if let Err(missing) = loaded {
            tracing::warn!(?missing, "下载完成但索引装配失败");
        } else {
            let _ = ModelReadyEvent {}.emit(&app2);
        }
        DOWNLOAD_IN_FLIGHT.store(false, Ordering::SeqCst);
    });
    Ok(())
}

/// 检索过滤器（日期区间 / 类型）
#[derive(Debug, Clone, Default, Deserialize, Type)]
pub struct SearchFilters {
    pub taken_from: Option<f64>,
    pub taken_to: Option<f64>,
    pub kind: Option<String>,
    /// 搜索范围（v8 裁定：跟随侧栏所选工作区；None=全部）
    pub folder_id: Option<i32>,
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
    let indexers: Vec<Arc<dyn mm_core::Indexer>> = state
        .indexers
        .read()
        .unwrap()
        .iter()
        .filter(|ix| ix.supports_text())
        .cloned()
        .collect();
    if indexers.is_empty() {
        return Ok(empty(false));
    }
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(empty(true));
    }

    let store = store_at(&state)?;
    let k = top_k.unwrap_or(100).clamp(1, 500);
    let filters = filters.unwrap_or_default();

    let passes_filters = |row: &mm_store::AssetRow| -> bool {
        if let Some(fid) = filters.folder_id {
            if row.folder_id != i64::from(fid) {
                return false;
            }
        }
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

    // ---- 语义流（每套索引各一路，ADR-0013）----
    // 范围过滤在取回后做：KNN 多取 3 倍候选，保证过滤后仍有 k 条
    let knn_k = if filters.folder_id.is_some() {
        k.saturating_mul(3)
    } else {
        k
    };
    // 每路保留 (asset_id, cosine)：两侧向量均 L2 归一化，cos = 1 − d²/2，clamp 到 [0,1]
    let mut semantic_streams: Vec<Vec<(u64, f64)>> = Vec::new();
    let mut text_qvec: Option<Vec<f32>> = None; // 区域流复用同一查询向量（Chinese-CLIP 对齐空间）
    for ix in &indexers {
        let Ok(qvec) = ix.embed_text(trimmed) else {
            // 静默吞曾让"语义流故障"与"真无结果"不可区分（搜'花'返回空的无声根因之一）
            tracing::warn!(index_id = ix.index_id(), "查询文本编码失败，跳过该路语义流");
            continue;
        };
        if text_qvec.is_none() {
            text_qvec = Some(qvec.clone());
        }
        // 单索引故障只降级该路语义流，不拖垮整个搜索（与 embed_text 失败 continue 同策）
        let hits = match store.knn_search(ix.index_id(), &qvec, knn_k) {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(index_id = ix.index_id(), error = %e, "KNN 检索失败，跳过该索引");
                continue;
            }
        };
        let pairs: Vec<(u64, f64)> = hits
            .into_iter()
            .filter_map(|(asset_id, dist)| {
                let row = store.get_asset(asset_id).ok().flatten()?;
                if !passes_filters(&row) {
                    return None;
                }
                let d = f64::from(dist);
                let cos = (1.0 - d * d / 2.0).clamp(0.0, 1.0);
                Some((u64::try_from(asset_id).unwrap_or(0), cos))
            })
            .collect();
        semantic_streams.push(pairs);
    }
    let semantic_id_lists: Vec<Vec<u64>> = semantic_streams
        .iter()
        .map(|v| v.iter().map(|p| p.0).collect())
        .collect();

    // ---- 区域流（ADR-0015：区域级 MaxSim）----
    // 查询向量与区域向量同在 Chinese-CLIP 对齐空间；每个资产取其区域最小距离，
    // 等价于「查询 token × 图像区域集合」的 MaxSim。作为额外一路语义流参与 RRF。
    let mut region_stream: Vec<(u64, f64)> = Vec::new();
    if let Some(qv) = &text_qvec {
        let fetch = knn_k.saturating_mul(3);
        if let Ok(region_hits) = store.knn_regions(qv, fetch) {
            use std::collections::HashMap as StdMap;
            let mut best: StdMap<u64, f64> = StdMap::new();
            for (_region_id, asset_id, dist) in region_hits {
                let asset_u = u64::try_from(asset_id).unwrap_or(0);
                let d = f64::from(dist);
                let e = best.entry(asset_u).or_insert(f64::INFINITY);
                if d < *e {
                    *e = d;
                }
            }
            let mut ranked: Vec<(u64, f64)> = best.into_iter().collect();
            ranked.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            region_stream = ranked
                .into_iter()
                .filter(|(id, _)| {
                    store
                        .get_asset(i64::try_from(*id).unwrap_or(0))
                        .ok()
                        .flatten()
                        .map(|row| passes_filters(&row))
                        .unwrap_or(false)
                })
                .collect();
        }
    }
    let region_ids: Vec<u64> = region_stream.iter().map(|p| p.0).collect();

    let semantic_id_lists: Vec<Vec<u64>> = semantic_streams
        .iter()
        .map(|v| v.iter().map(|p| p.0).collect())
        .collect();
    let semantic_refs: Vec<&[u64]> = semantic_id_lists.iter().map(|v| v.as_slice()).collect();
    let similarity_of: std::collections::HashMap<u64, f64> = semantic_streams
        .iter()
        .flat_map(|v| v.iter().copied())
        .collect();

    // ---- 文本流（文件名；FTS 语法已由 build_match_query 清洗）----
    let match_query = Store::build_match_query(trimmed);
    let text_ids: Vec<u64> = if match_query.is_empty() {
        Vec::new()
    } else {
        store
            .search_fts(&match_query, k)
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter_map(|asset_id| store.get_asset(asset_id).ok().flatten())
            .filter(|row| passes_filters(row))
            .map(|row| u64::try_from(row.asset_id).unwrap_or(0))
            .collect()
    };

    // ---- 多流 RRF 融合（k=60，白皮书 §4.6 + ADR-0013）----
    let mut all_semantic_refs: Vec<&[u64]> = semantic_refs.clone();
    all_semantic_refs.push(&region_ids);
    let fused = mm_core::search::rrf_fuse_multi(&all_semantic_refs, &text_ids, 60.0);
    let max_score = fused.first().map(|h| h.score).unwrap_or(1.0).max(1e-9);

    let items = fused
        .into_iter()
        .filter_map(|hit| {
            let row = store
                .get_asset(i64::try_from(hit.asset_id).unwrap_or(0))
                .ok()
                .flatten()?;
            let folder_label = row
                .folder_id
                .try_into()
                .ok()
                .and_then(|fid: i32| store.get_folder(i64::from(fid)).ok().flatten())
                .map(|f| f.label.unwrap_or(f.path))
                .unwrap_or_default();
            let file_name = row
                .storage_key
                .rsplit([char::from_u32(0x5C).unwrap(), '/'])
                .next()
                .unwrap_or("")
                .to_string();
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
                // 纯文本流命中 = 语义索引尚未建立（网格呼吸点语义一致）
                indexed: hit.matched.slug() != "text",
            },
            score: (hit.score / max_score * 1000.0).round() / 1000.0,
            // 语义相似度（余弦 0~1；纯文件名命中为 null）——展示用，不参与排序
            similarity: similarity_of.get(&hit.asset_id).copied(),
                matched: hit.matched.slug().to_string(),
                file_name,
                size_bytes: row.size.map(|s| s as f64).unwrap_or(0.0),
                folder_id: i32::try_from(row.folder_id).unwrap_or(0),
                folder_label,
            })
        })
        .collect();

    let pending: i32 = store
        .list_active_indexes()
        .map(|idxs| {
            idxs.iter()
                .filter_map(|i| {
                    store
                        .list_ready_without_embedding(i.index_id, u32::MAX)
                        .ok()
                })
                .map(|v| v.len() as i32)
                .sum()
        })
        .unwrap_or(0);
    let mut available_years: Vec<String> = Vec::new();
    for row in store.list_page(None, 500, None).unwrap_or_default() {
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
    let mut total = 0i64;
    for idx in store.list_active_indexes().map_err(|e| e.to_string())? {
        total += store
            .count_embedded(idx.index_id)
            .map_err(|e| e.to_string())?;
        store
            .clear_embeddings(idx.index_id)
            .map_err(|e| e.to_string())?;
    }
    Ok(total as i32)
}

/// 重建某工作区的全部向量（所有 active 索引）；worker 轮询自动重嵌，进度经
/// EmbedProgressEvent 逐张可见。返回清掉的向量数。重建期间该工作区语义搜索暂缺。
#[tauri::command]
#[specta::specta]
pub async fn reindex_folder(state: State<'_, AppState>, folder_id: i32) -> Result<i32, String> {
    let store = store_at(&state)?;
    let mut total = 0i64;
    for idx in store.list_active_indexes().map_err(|e| e.to_string())? {
        let n = store
            .clear_embeddings_for_folder(idx.index_id, i64::from(folder_id))
            .map_err(|e| e.to_string())?;
        total += i64::from(n);
    }
    tracing::info!(folder_id, cleared = total, "工作区向量已清空，等待重嵌");
    Ok(i32::try_from(total).unwrap_or(i32::MAX))
}

/// 重新向量化指定资产（灯箱单张/网格批量共用）；worker 轮询自动重嵌。返回清掉的向量数。
#[tauri::command]
#[specta::specta]
pub async fn reindex_assets(
    state: State<'_, AppState>,
    asset_ids: Vec<i32>,
) -> Result<i32, String> {
    if asset_ids.is_empty() {
        return Ok(0);
    }
    let ids: Vec<i64> = asset_ids.iter().map(|v| i64::from(*v)).collect();
    let store = store_at(&state)?;
    let cleared = store
        .clear_embeddings_of_assets(&ids)
        .map_err(|e| e.to_string())?;
    tracing::info!(count = ids.len(), cleared, "资产向量已清空，等待重嵌");
    Ok(i32::try_from(cleared).unwrap_or(i32::MAX))
}
