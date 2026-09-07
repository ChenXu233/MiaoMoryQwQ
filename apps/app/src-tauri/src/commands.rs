//! IPC 命令（CQRS-lite，白皮书 §4.4）：写命令返回任务快照，读查询返回快照。
//! 全部经 specta 生成 TS 契约；禁止手写前端类型。

use std::path::PathBuf;

use mm_core::ErrorCode;
use mm_core::StorageAdapter as _;
use mm_store::Store;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::events::ImportProgressEvent;
use crate::state::{
    thumbs_abs_path, AppState, AssetSummary, DeleteReport, FailedItem, TimelinePage, YearGroup,
};

pub fn store_at(state: &AppState) -> Result<Store, ErrorCode> {
    Store::open(&state.workspace.db_path()).map_err(ErrorCode::from)
}

fn err_code(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// 时间轴分页查询：按拍摄时间降序，前端按年分组展示
#[tauri::command]
#[specta::specta]
pub fn list_timeline(
    state: State<'_, AppState>,
    cursor: Option<String>,
    page_size: Option<u32>,
    folder_id: Option<i32>,
) -> Result<TimelinePage, String> {
    let store = store_at(&state).map_err(err_code)?;
    let parsed = cursor.and_then(|c| {
        let mut it = c.splitn(2, ':');
        let t = it.next()?.parse::<i64>().ok()?;
        let id = it.next()?.parse::<i64>().ok()?;
        Some((t, id))
    });
    let rows = store
        .list_page(parsed, page_size.unwrap_or(200), folder_id.map(i64::from))
        .map_err(err_code)?;
    let mut groups: Vec<YearGroup> = Vec::new();
    for r in &rows {
        let year: u16 = r.year.parse().unwrap_or(1970);
        match groups.last_mut() {
            Some(g) if g.year == year => g.items.push(AssetSummary {
                asset_id: r.asset_id as i32,
                thumb_path: r.thumb_key.as_ref().map(|k| {
                    thumbs_abs_path(&state.workspace, k)
                        .to_string_lossy()
                        .into_owned()
                }),
                width: r.width,
                height: r.height,
                taken_at: r.taken_at as f64,
            }),
            _ => groups.push(YearGroup {
                year,
                items: vec![AssetSummary {
                    asset_id: r.asset_id as i32,
                    thumb_path: r.thumb_key.as_ref().map(|k| {
                        thumbs_abs_path(&state.workspace, k)
                            .to_string_lossy()
                            .into_owned()
                    }),
                    width: r.width,
                    height: r.height,
                    taken_at: r.taken_at as f64,
                }],
            }),
        }
    }
    let next_cursor = if rows.len() >= page_size.unwrap_or(200) as usize {
        rows.last()
            .map(|r| format!("{}:{}", r.taken_at, r.asset_id))
    } else {
        None
    };
    Ok(TimelinePage {
        groups,
        next_cursor,
    })
}

/// 原图绝对路径（asset protocol 读取；scope 在导入时放行）
#[tauri::command]
#[specta::specta]
pub fn get_asset_image(state: State<'_, AppState>, asset_id: i32) -> Result<String, String> {
    let store = store_at(&state).map_err(err_code)?;
    store
        .get_asset(i64::from(asset_id))
        .map_err(err_code)?
        .map(|r| r.storage_key)
        .ok_or_else(|| "not_found".to_string())
}

/// 批量删除记录与缩略图（永不触碰原文件）
#[tauri::command]
#[specta::specta]
pub fn delete_assets(
    state: State<'_, AppState>,
    asset_ids: Vec<i32>,
) -> Result<DeleteReport, String> {
    let store = store_at(&state).map_err(err_code)?;
    let ids64: Vec<i64> = asset_ids.iter().map(|&i| i as i64).collect();
    let (deleted, missing, thumb_keys) = store.delete_assets(&ids64).map_err(err_code)?;
    let storage = mm_pipeline::LocalDiskAdapter::new(state.workspace.thumbs_dir());
    for key in &thumb_keys {
        let _ = storage.delete(key);
    }
    Ok(DeleteReport {
        deleted: deleted as i32,
        missing: missing as i32,
    })
}

/// 失败清单
#[tauri::command]
#[specta::specta]
pub fn list_failed_items(state: State<'_, AppState>) -> Result<Vec<FailedItem>, String> {
    let store = store_at(&state).map_err(err_code)?;
    Ok(store
        .list_failed()
        .map_err(err_code)?
        .into_iter()
        .map(|r| FailedItem {
            asset_id: i32::try_from(r.asset_id).unwrap_or(0),
            path: r.storage_key,
            error_code: r.error_code,
        })
        .collect())
}

/// 启动导入：选择文件夹 → 引擎后台跑（重试 = 对同一文件夹再导一次，哈希幂等）
#[tauri::command]
#[specta::specta]
pub async fn import_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: String,
) -> Result<i32, String> {
    use tauri::Manager;
    let path = PathBuf::from(&folder);
    if !path.is_dir() {
        return Err("read_failed".into());
    }
    // asset protocol 放行该目录（Lightbox 读原图）
    let _ = app.asset_protocol_scope().allow_directory(&path, true);

    // 重试语义：清掉 failed 行回 pending，随本次导入重新处理
    if let Ok(store) = store_at(&state) {
        let _ = store.reset_failed();
    }

    // 来源工作区：同路径幂等复用同一 folder（迁移 v4）
    let (folder_id, _) = {
        let store = store_at(&state).map_err(err_code)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        store
            .get_or_create_folder(
                &folder,
                path.file_name().map(|s| s.to_string_lossy()).as_deref(),
                now,
            )
            .map_err(err_code)?
    };

    let job_id = state
        .engine
        .start(path, folder_id)
        .map_err(|c| c.slug().to_string())?;
    state.sink.begin_job(job_id);
    // 立即同步一条初始进度（枚举完成），避免 UI 等 100ms 节流
    let snap = state.engine.snapshot();
    ImportProgressEvent {
        job_id: job_id as u32,
        total: u32::try_from(snap.total).unwrap_or(0),
        done: 0,
        failed: 0,
        eta_seconds: None,
    }
    .emit(&app)
    .ok();
    Ok(i32::try_from(job_id).unwrap_or(0))
}

#[tauri::command]
#[specta::specta]
pub fn pause_import(state: State<'_, AppState>) {
    state.engine.pause();
}

#[tauri::command]
#[specta::specta]
pub fn resume_import(state: State<'_, AppState>) {
    state.engine.resume();
}

/// 停止当前导入（确认后）；已处理部分保留，续传 = 重新导入同一文件夹
#[tauri::command]
#[specta::specta]
pub fn stop_import(state: State<'_, AppState>) {
    state.engine.abort();
}

/// 当前任务快照（前端轮询兜底，事件丢失时恢复用）
#[tauri::command]
#[specta::specta]
pub fn import_snapshot(state: State<'_, AppState>) -> Result<ImportProgressEvent, String> {
    let s = state.engine.snapshot();
    Ok(ImportProgressEvent {
        job_id: s.job_id as u32,
        total: s.total as u32,
        done: s.done as u32,
        failed: s.failed as u32,
        eta_seconds: None,
    })
}
