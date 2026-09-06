//! 数据位置与口袋式布局命令（ADR-0011 / 规格 0006）。
//!
//! 全部本地操作，无事件；错误文案面向用户（行动导向），错误码进 tracing。

use std::path::PathBuf;

use mm_platform::{dir_is_writable, load_config_at, save_config_at, AppConfig, DataMode};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::state::AppState;

fn mode_str(mode: DataMode) -> &'static str {
    match mode {
        DataMode::EnvOverride => "env",
        DataMode::Portable => "portable",
        DataMode::Rooted => "rooted",
        DataMode::Classic => "classic",
    }
}

/// 当前数据布局快照（设置对话框数据区）
#[derive(Debug, Serialize, specta::Type)]
pub struct DataInfo {
    /// env | portable | rooted | classic
    pub mode: String,
    /// 数据随应用目录携带（口袋式，含 env/rooted 指定的数据根）
    pub portable: bool,
    pub db_path: String,
    pub thumbs_dir: String,
    pub models_dir: String,
    pub logs_dir: String,
    /// 「打开数据文件夹」的目标目录
    pub data_folder: String,
    pub config_path: String,
    /// 是否允许在设置里更改位置（env 模式禁用）
    pub can_change: bool,
}

#[tauri::command]
#[specta::specta]
pub fn data_info(state: State<'_, AppState>) -> DataInfo {
    let w = &state.workspace;
    DataInfo {
        mode: mode_str(w.mode).to_string(),
        portable: w.is_portable(),
        db_path: w.db_path().to_string_lossy().into_owned(),
        thumbs_dir: w.thumbs_dir().to_string_lossy().into_owned(),
        models_dir: w.model_dir.to_string_lossy().into_owned(),
        logs_dir: w.logs_dir().to_string_lossy().into_owned(),
        data_folder: w.data_folder().to_string_lossy().into_owned(),
        config_path: w.config_path.to_string_lossy().into_owned(),
        can_change: w.mode != DataMode::EnvOverride,
    }
}

/// 用系统文件管理器打开数据目录；目录丢失时重建后重试一次（规格 0006 §3.3）
#[tauri::command]
#[specta::specta]
pub fn open_data_folder(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let dir = state.workspace.data_folder();
    if !dir.is_dir() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("数据目录无法访问：{e}"))?;
    }
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| format!("打开文件夹失败：{e}"))
}

/// 更改数据位置：校验可写与嵌套关系后写入 config.data_dir，重启生效（规格 0006 §3.2）
#[tauri::command]
#[specta::specta]
pub fn set_data_location(state: State<'_, AppState>, dir: String) -> Result<(), String> {
    if state.workspace.mode == DataMode::EnvOverride {
        return Err("开发模式（环境变量指定）下不能在应用内更改数据位置".to_string());
    }
    let target = PathBuf::from(&dir);
    if target == state.workspace.workspace_dir {
        return Err("新位置与当前位置相同".to_string());
    }
    // 互为祖先会形成嵌套搬迁/解析歧义，一律拒绝
    if state.workspace.workspace_dir.starts_with(&target)
        || target.starts_with(&state.workspace.workspace_dir)
    {
        return Err("新位置不能在当前数据目录内部，当前数据目录也不能在新位置内部".to_string());
    }
    if !dir_is_writable(&target) {
        tracing::warn!(code = "DATA_DIR_NOT_WRITABLE", %dir, "数据位置不可写");
        return Err("这个文件夹无法写入，请选择你有写入权限的位置".to_string());
    }
    let config_path = state.workspace.config_path.clone();
    let mut cfg: AppConfig = load_config_at(&config_path).unwrap_or_default();
    cfg.data_dir = Some(target);
    save_config_at(&config_path, &cfg).map_err(|e| {
        tracing::warn!(code = "CONFIG_WRITE_FAILED", error = %e, "写入配置失败");
        format!("配置写入失败：{e}")
    })?;
    tracing::info!(dir = %dir, "数据位置已更改，重启后生效");
    Ok(())
}
