//! MiaoMory Tauri 壳：commands/events 的薄装配层（白皮书 §4.1）。
//!
//! 业务逻辑一律下沉到 `crates/*`；本 crate 只做三件事：
//! 声明 IPC 命令（specta 契约的唯一事实源）、注册插件、初始化平台层。

mod commands;
mod embed_worker;
mod events;
mod search_commands;
mod state;

use tauri::{Manager, Wry};
use tauri_specta::{collect_commands, collect_events, Builder};

use crate::events::{
    EmbedProgressEvent, ImportFinishedEvent, ImportItemFailedEvent, ImportPausedEvent,
    ImportProgressEvent, ImportResumedEvent, ModelDownloadProgressEvent, ModelReadyEvent,
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

    mm_platform::init_tracing().expect("初始化日志失败");
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "MiaoMory 启动");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);

            // 装配：工作区/模型路径 + 导入引擎 + 嵌入模型槽位
            let config = mm_platform::load_config().expect("读取配置失败");
            let paths = config.resolved().expect("解析路径失败");
            mm_platform::ensure_workspace_layout(&paths).expect("创建工作区目录失败");
            app.asset_protocol_scope()
                .allow_directory(paths.workspace_dir.clone(), true)
                .ok();

            // 分发源：config 覆盖优先，否则默认 GitHub Release（规格 0004）
            let endpoints = match config.hf_endpoint {
                Some(ref url) => vec![url.clone()],
                None => vec![
                    "https://github.com/ChenXu233/MiaoMoryQwQ/releases/download/models-v1"
                        .to_string(),
                ],
            };
            let model_dir = paths.model_dir.clone();
            app.manage(AppState::build(app.handle(), paths, model_dir, endpoints));

            // 模型已在本地则直接加载（重启后无需再下载）
            {
                let state = app.state::<AppState>();
                if let Err(missing) = state.load_embedder() {
                    tracing::info!(?missing, "模型未就绪，语义搜索保持降级");
                }
                embed_worker::EmbedWorker::spawn(
                    state.workspace.db_path(),
                    state.embedder.clone(),
                    app.handle().clone(),
                );
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
