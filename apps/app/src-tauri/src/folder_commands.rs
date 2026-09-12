//! 来源文件夹（工作区）与存储占用命令（ADR-0011/0012 方向，P5 切片 A1）。
//! 离线为一等状态：浏览/搜索永不受阻，只有原图访问降级（缩略图 + 状态条）。

use mm_core::StorageAdapter as _;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
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
    /// 待建索引的 ready 资产数（>0 时侧栏显示「建索引中 N」，规格 0007 §1）
    pub pending_index: i32,
}

/// 主索引（第一套 active 索引）的待嵌计数；无 active 索引返回 0
fn pending_index_of(store: &mm_store::Store, folder_id: i64) -> i32 {
    store
        .list_active_indexes()
        .ok()
        .and_then(|idxs| idxs.first().map(|i| i.index_id))
        .and_then(|iid| store.pending_index_count(iid, Some(folder_id)).ok())
        .and_then(|n| i32::try_from(n).ok())
        .unwrap_or(0)
}

#[tauri::command]
#[specta::specta]
pub fn list_folders(state: State<'_, AppState>) -> Result<Vec<FolderInfo>, String> {
    let store = crate::commands::store_at(&state).map_err(mode_err)?;
    Ok(store
        .list_folders()
        .map_err(mode_err)?
        .into_iter()
        .map(|f| {
            let fid = f.folder_id;
            FolderInfo {
                folder_id: i32::try_from(fid).unwrap_or(0),
                path: f.path,
                label: f.label,
                status: f.status,
                asset_count: i32::try_from(f.asset_count).unwrap_or(0),
                pending_index: pending_index_of(&store, fid),
            }
        })
        .collect())
}

/// 主动重检：路径存在 → online，不存在 → missing；广播状态变化。
/// 恢复 online 时补放行 asset scope：scope 每次启动重建，只翻 DB 状态不放行的话，
/// 原图继续 403 又被一票否决打回 offline——重检永远解不了套。
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
    if status == "online" {
        if let Some(folder) = store.get_folder(i64::from(folder_id)).map_err(mode_err)? {
            if !folder.path.is_empty() {
                let _ = app
                    .asset_protocol_scope()
                    .allow_directory(std::path::PathBuf::from(&folder.path), true);
            }
        }
    }
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
    // 新位置同样要放行 asset scope，否则原图 403 又被打回 offline
    let _ = app
        .asset_protocol_scope()
        .allow_directory(std::path::PathBuf::from(&new_path), true);
    let _ = FolderStatusChangedEvent {
        folder_id,
        status: "online".into(),
    }
    .emit(&app);
    folder_info(&store, i64::from(folder_id))
}

/// 前端原图加载失败回调（被动检测）：后端先核实该文件是否真的不在磁盘上。
/// 文件还在 = scope 丢失/瞬时错误（如 403），不打 offline——一票否决把整个
/// 文件夹误标 offline 的根因；改为补放行来源目录并把误标的 offline 拉回 online。
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
    if !std::path::Path::new(&row.storage_key).exists() {
        store
            .set_folder_status(row.folder_id, "offline")
            .map_err(mode_err)?;
        let _ = FolderStatusChangedEvent {
            folder_id: i32::try_from(row.folder_id).unwrap_or(0),
            status: "offline".into(),
        }
        .emit(&app);
        return Ok(());
    }
    let folder = store.get_folder(row.folder_id).map_err(mode_err)?;
    if let Some(folder) = folder {
        if !folder.path.is_empty() {
            let _ = app
                .asset_protocol_scope()
                .allow_directory(std::path::PathBuf::from(&folder.path), true);
        }
        if folder.status != "online" {
            store
                .set_folder_status(row.folder_id, "online")
                .map_err(mode_err)?;
            let _ = FolderStatusChangedEvent {
                folder_id: i32::try_from(row.folder_id).unwrap_or(0),
                status: "online".into(),
            }
            .emit(&app);
        }
    }
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

/// 删除工作区的结果报告
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct FolderDeleteReport {
    /// 随文件夹删除的资产（记录）数
    pub deleted: i32,
}

/// 删除来源文件夹（工作区）：移除其全部记录、各索引向量、区域与缩略图，
/// 磁盘原文件不受影响。导入/同步进行中拒绝；维护守卫与引擎任务启动互斥，
/// 消除「检查与执行之间任务被启动」的竞态（watcher 侧 start 失败会去抖重试）。
#[tauri::command]
#[specta::specta]
pub async fn delete_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    folder_id: i32,
) -> Result<FolderDeleteReport, String> {
    let store = crate::commands::store_at(&state).map_err(mode_err)?;
    let fid = i64::from(folder_id);
    // begin_maintenance 与 start 共用引擎状态锁，原子完成「无运行任务 + 拒新任务」；
    // 守卫 drop 自动退出维护态（含 panic 路径），后段无 await 不放大 Send 约束
    let _maintenance = state
        .engine
        .begin_maintenance()
        .map_err(|_| "有导入或同步正在进行，等它结束再删除".to_string())?;
    let folder = store
        .get_folder(fid)
        .map_err(mode_err)?
        .ok_or("folder not found")?;
    let (deleted, thumb_keys) = store.delete_folder(fid).map_err(mode_err)?;
    let storage = mm_pipeline::LocalDiskAdapter::new(state.workspace.thumbs_dir());
    for key in &thumb_keys {
        let _ = storage.delete(key);
    }
    tracing::info!(
        folder_id,
        path = %folder.path,
        deleted,
        thumbs = thumb_keys.len(),
        "已删除来源工作区"
    );
    let _ = FolderStatusChangedEvent {
        folder_id,
        status: "deleted".into(),
    }
    .emit(&app);
    Ok(FolderDeleteReport {
        deleted: i32::try_from(deleted).unwrap_or(0),
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
        pending_index: pending_index_of(store, folder_id),
    })
}

/// 单工作区的向量化进度（进度卡按工作区分组展示）
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct FolderEmbedStatus {
    pub folder_id: i32,
    pub label: Option<String>,
    /// online | offline | missing（非 online 的待嵌项不会推进，前端标注「源离线」）
    pub status: String,
    /// ready 资产总数（分母）
    pub total: i32,
    /// 已有向量的 ready 资产数（分子）
    pub done: i32,
}

/// 各工作区向量化进度快照（EmbedCard 冷启动兜底；运行中以 EmbedProgressEvent 为准）。
/// 口径 = 第一套 active 索引（与侧栏 pending_index 徽标一致）。
#[tauri::command]
#[specta::specta]
pub fn embed_status(state: State<'_, AppState>) -> Result<Vec<FolderEmbedStatus>, String> {
    let store = crate::commands::store_at(&state).map_err(mode_err)?;
    let index_id = store
        .list_active_indexes()
        .map_err(mode_err)?
        .first()
        .map(|i| i.index_id);
    let mut out = Vec::new();
    for f in store.list_folders().map_err(mode_err)? {
        let (total, done) = match index_id {
            Some(iid) => (
                store.ready_count_of_folder(f.folder_id).unwrap_or(0),
                store
                    .embedded_count_of_folder(iid, f.folder_id)
                    .unwrap_or(0),
            ),
            None => (0, 0),
        };
        out.push(FolderEmbedStatus {
            folder_id: i32::try_from(f.folder_id).unwrap_or(0),
            label: f.label,
            status: f.status,
            total: i32::try_from(total).unwrap_or(i32::MAX),
            done: i32::try_from(done).unwrap_or(i32::MAX),
        });
    }
    Ok(out)
}
