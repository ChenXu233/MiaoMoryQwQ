//! 来源文件夹（工作区）与存储占用命令（ADR-0011/0012 方向，P5 切片 A1）。
//! 离线为一等状态：浏览/搜索永不受阻，只有原图访问降级（缩略图 + 状态条）。

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::events::FolderStatusChangedEvent;
use crate::state::AppState;

fn mode_err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// 文件夹（工作区）快照
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct FolderInfo {
    pub folder_id: i32,
    pub path: String,
    pub label: Option<String>,
    /// online | offline | missing
    pub status: String,
    pub asset_count: i32,
}

#[tauri::command]
#[specta::specta]
pub fn list_folders(state: State<'_, AppState>) -> Result<Vec<FolderInfo>, String> {
    let store = crate::commands::store_at(&state).map_err(mode_err)?;
    Ok(store
        .list_folders()
        .map_err(mode_err)?
        .into_iter()
        .map(|f| FolderInfo {
            folder_id: i32::try_from(f.folder_id).unwrap_or(0),
            path: f.path,
            label: f.label,
            status: f.status,
            asset_count: i32::try_from(f.asset_count).unwrap_or(0),
        })
        .collect())
}

/// 主动重检：路径存在 → online，不存在 → missing；广播状态变化
#[tauri::command]
#[specta::specta]
pub async fn recheck_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    folder_id: i32,
) -> Result<FolderInfo, String> {
    let store = crate::commands::store_at(&state).map_err(mode_err)?;
    let status = store
        .recheck_folder(i64::from(folder_id))
        .map_err(mode_err)?;
    let _ = FolderStatusChangedEvent {
        folder_id,
        status: status.clone(),
    }
    .emit(&app);
    folder_info(&store, i64::from(folder_id))
}

/// 重新指定丢失文件夹的新位置；该工作区资产的 storage_key 按新前缀批量改写
#[tauri::command]
#[specta::specta]
pub async fn relocate_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    folder_id: i32,
    new_path: String,
) -> Result<FolderInfo, String> {
    if !std::path::Path::new(&new_path).is_dir() {
        return Err("这个文件夹不存在或无法访问".into());
    }
    let store = crate::commands::store_at(&state).map_err(mode_err)?;
    store
        .relocate_folder(i64::from(folder_id), &new_path)
        .map_err(mode_err)?;
    let _ = FolderStatusChangedEvent {
        folder_id,
        status: "online".into(),
    }
    .emit(&app);
    folder_info(&store, i64::from(folder_id))
}

/// 前端原图加载失败回调（被动检测）：该文件夹标记 offline 并广播（一次性提示由前端控制）
#[tauri::command]
#[specta::specta]
pub async fn report_original_missing(
    app: AppHandle,
    state: State<'_, AppState>,
    asset_id: i32,
) -> Result<(), String> {
    let store = crate::commands::store_at(&state).map_err(mode_err)?;
    let row = store
        .get_asset(i64::from(asset_id))
        .map_err(mode_err)?
        .ok_or("asset not found")?;
    store
        .set_folder_status(row.folder_id, "offline")
        .map_err(mode_err)?;
    let _ = FolderStatusChangedEvent {
        folder_id: i32::try_from(row.folder_id).unwrap_or(0),
        status: "offline".into(),
    }
    .emit(&app);
    Ok(())
}

/// 灯箱角标数据：原图大小 + 来源文件夹状态（缩略图/原图标识，裁决 9）
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct AssetDetail {
    pub size_bytes: f64,
    pub mime: Option<String>,
    pub folder_id: i32,
    pub folder_label: Option<String>,
    pub folder_path: String,
    /// online | offline | missing
    pub folder_status: String,
}

#[tauri::command]
#[specta::specta]
pub fn asset_detail(state: State<'_, AppState>, asset_id: i32) -> Result<AssetDetail, String> {
    let store = crate::commands::store_at(&state).map_err(mode_err)?;
    let row = store
        .get_asset(i64::from(asset_id))
        .map_err(mode_err)?
        .ok_or("asset not found")?;
    let folder = store
        .get_folder(row.folder_id)
        .map_err(mode_err)?
        .ok_or("folder not found")?;
    Ok(AssetDetail {
        size_bytes: row.size.map(|s| s as f64).unwrap_or(0.0),
        mime: row.mime,
        folder_id: i32::try_from(row.folder_id).unwrap_or(0),
        folder_label: folder.label,
        folder_path: folder.path,
        folder_status: folder.status,
    })
}

/// 存储占用分析（裁决 25：只展示，不做清理）
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct StorageUsage {
    pub db_bytes: f64,
    pub wal_bytes: f64,
    pub thumbs_bytes: f64,
    pub models_bytes: f64,
    pub assets_count: i32,
    pub embedded_count: i32,
    /// 各索引分项（裁定 25：占用分析到每一套索引）
    pub per_index: Vec<IndexUsage>,
}

/// 单套索引的占用
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct IndexUsage {
    pub index_id: i32,
    pub slug: String,
    pub display: String,
    /// active | disabled
    pub status: String,
    pub count: i32,
    /// 近似占用 = count × dim × 4（f32）
    pub approx_bytes: f64,
}

#[tauri::command]
#[specta::specta]
pub fn storage_usage(state: State<'_, AppState>) -> Result<StorageUsage, String> {
    let store = crate::commands::store_at(&state).map_err(mode_err)?;
    let db_path = state.workspace.db_path();
    let dir_size = |p: &std::path::Path| -> f64 {
        fn walk(p: &std::path::Path, acc: &mut u64) {
            let Ok(entries) = std::fs::read_dir(p) else {
                return;
            };
            for e in entries.flatten() {
                let Ok(ft) = e.file_type() else { continue };
                if ft.is_dir() {
                    walk(&e.path(), acc);
                } else if let Ok(meta) = e.metadata() {
                    *acc += meta.len();
                }
            }
        }
        let mut acc = 0u64;
        walk(p, &mut acc);
        acc as f64
    };
    let wal_bytes = {
        let mut wal = db_path.clone().into_os_string();
        wal.push("-wal");
        std::fs::metadata(std::path::PathBuf::from(&wal))
            .map(|m| m.len() as f64)
            .unwrap_or(0.0)
    };
    let per_index: Vec<IndexUsage> = store
        .list_indexes()
        .map_err(mode_err)?
        .into_iter()
        .map(|idx| {
            let count = store.count_embedded(idx.index_id).unwrap_or(0);
            IndexUsage {
                index_id: i32::try_from(idx.index_id).unwrap_or(0),
                slug: idx.slug,
                display: idx.display,
                status: idx.status,
                count: i32::try_from(count).unwrap_or(0),
                approx_bytes: (count * idx.dim * 4) as f64,
            }
        })
        .collect();
    Ok(StorageUsage {
        db_bytes: std::fs::metadata(&db_path)
            .map(|m| m.len() as f64)
            .unwrap_or(0.0),
        wal_bytes,
        thumbs_bytes: dir_size(&state.workspace.thumbs_dir()),
        models_bytes: dir_size(&state.model_dir),
        assets_count: i32::try_from(store.count().map_err(mode_err)?).unwrap_or(0),
        embedded_count: per_index
            .iter()
            .filter(|i| i.status == "active")
            .map(|i| i.count)
            .sum::<i32>(),
        per_index,
    })
}

fn folder_info(store: &mm_store::Store, folder_id: i64) -> Result<FolderInfo, String> {
    let f = store
        .get_folder(folder_id)
        .map_err(mode_err)?
        .ok_or("folder not found")?;
    Ok(FolderInfo {
        folder_id: i32::try_from(f.folder_id).unwrap_or(0),
        path: f.path,
        label: f.label,
        status: f.status,
        asset_count: i32::try_from(f.asset_count).unwrap_or(0),
    })
}
