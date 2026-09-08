//! MiaoMory Tauri 壳：commands/events 的薄装配层（白皮书 §4.1）。
//!
//! 业务逻辑一律下沉到 `crates/*`；本 crate 只做三件事：
//! 声明 IPC 命令（specta 契约的唯一事实源）、注册插件、初始化平台层。

mod commands;
mod data_commands;
mod embed_worker;
mod events;
mod folder_commands;
mod search_commands;
mod state;

use tauri::{Manager, Wry};
use tauri_specta::{collect_commands, collect_events, Builder};

use crate::events::{
    EmbedProgressEvent, FolderStatusChangedEvent, ImportFinishedEvent, ImportItemFailedEvent,
    ImportPausedEvent, ImportProgressEvent, ImportResumedEvent, ModelDownloadProgressEvent,
    ModelReadyEvent,
};
use crate::state::AppState;

/// 示例命令：P0 用于验证 specta 契约链路（生成 → 引用 → CI 一致性检查）
#[tauri::command]
#[specta::specta]
fn greet(name: String) -> String {
    format!("你好，{name}！MiaoMory IPC 契约链路已打通。")
}

/// IPC 契约构建器：唯一事实源，`bin/export_contracts.rs` 与运行时共用
pub fn app_builder() -> Builder<Wry> {
    Builder::<Wry>::new()
        .commands(collect_commands![
            greet,
            commands::import_folder,
            commands::pause_import,
            commands::resume_import,
            commands::stop_import,
            commands::import_snapshot,
            commands::list_timeline,
            commands::get_asset_image,
            commands::delete_assets,
            commands::list_failed_items,
            search_commands::model_status,
            search_commands::download_models,
            search_commands::search_assets,
            search_commands::reindex_all,
            data_commands::data_info,
            data_commands::open_data_folder,
            data_commands::set_data_location,
            folder_commands::list_folders,
            folder_commands::recheck_folder,
            folder_commands::relocate_folder,
            folder_commands::report_original_missing,
            folder_commands::asset_detail,
            folder_commands::storage_usage,
        ])
        .events(collect_events![
            ImportProgressEvent,
            ImportPausedEvent,
            ImportResumedEvent,
            ImportFinishedEvent,
            ImportItemFailedEvent,
            ModelDownloadProgressEvent,
            ModelReadyEvent,
            EmbedProgressEvent,
            FolderStatusChangedEvent,
        ])
}

/// 生成 TS 契约到 `packages/contracts/src/bindings.ts`（铁律 6：禁止手写 IPC 类型）
pub fn export_bindings(builder: &Builder<Wry>) {
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../packages/contracts/src/bindings.ts");
    builder
        .export(specta_typescript::Typescript::default(), out)
        .expect("导出 TS 契约失败");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = app_builder();

    // 仅 debug 构建在启动时导出契约；正式以 bin/export_contracts 为准（CI 一致性检查）
    #[cfg(debug_assertions)]
    export_bindings(&builder);

    // 布局解析（ADR-0011）先于日志初始化：logs 落点由布局决定
    let paths = mm_platform::resolve_layout().expect("解析数据布局失败");
    mm_platform::init_tracing(&paths).expect("初始化日志失败");
    if let Some(err) = &paths.config_load_error {
        tracing::warn!(error = %err, "配置文件解析失败，已回退默认值");
    }
    tracing::info!(version = env!("CARGO_PKG_VERSION"), mode = ?paths.mode, "MiaoMory 启动");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);

            // 装配：工作区/模型路径 + 导入引擎 + 嵌入模型槽位
            mm_platform::ensure_workspace_layout(&paths).expect("创建工作区目录失败");
            app.asset_protocol_scope()
                .allow_directory(paths.workspace_dir.clone(), true)
                .ok();

            // 分发源：config.hf_endpoint 覆盖优先，默认 GitHub Release（规格 0004 / ADR-0010）
            let endpoints = vec![paths.hf_endpoint.clone()];
            let model_dir = paths.model_dir.clone();
            app.manage(AppState::build(app.handle(), paths, model_dir, endpoints));

            // 模型已在本地则直接加载（重启后无需再下载）
            {
                let state = app.state::<AppState>();
                if let Err(missing) = state.load_indexers() {
                    tracing::info!(?missing, "模型未就绪，语义搜索保持降级");
                }
                embed_worker::EmbedWorker::spawn(
                    state.workspace.db_path(),
                    state.indexers.clone(),
                    state.engine.clone(),
                    app.handle().clone(),
                );
            }

            // 开发辅助（仅 debug 构建）：设置 MIAOMORY_DEV_AUTO_IMPORT=<目录> 则启动即导入，
            // 走与 IPC 命令完全相同的装配路径，用于无 UI 的性能走查
            #[cfg(debug_assertions)]
            if let Some(folder) = std::env::var_os("MIAOMORY_DEV_AUTO_IMPORT") {
                let state = app.state::<AppState>();
                match crate::commands::start_import(app.handle(), &state, &folder.to_string_lossy())
                {
                    Ok(_) => tracing::info!(folder = %folder.to_string_lossy(), "dev 自动导入已启动"),
                    Err(e) => {
                        tracing::warn!(folder = %folder.to_string_lossy(), error = %e, "dev 自动导入失败")
                    }
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
