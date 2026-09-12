//! MiaoMory Tauri 壳：commands/events 的薄装配层（白皮书 §4.1）。
//!
//! 业务逻辑一律下沉到 `crates/*`；本 crate 只做三件事：
//! 声明 IPC 命令（specta 契约的唯一事实源）、注册插件、初始化平台层。

mod commands;
mod data_commands;
mod embed_worker;
mod events;
mod folder_commands;
mod folder_watcher;
mod inference_commands;
mod region_worker;
mod search_commands;
mod state;

use std::path::PathBuf;

use tauri::{Manager, Wry};
use tauri_specta::{collect_commands, collect_events, Builder};

use crate::events::{
    EmbedProgressEvent, FolderStatusChangedEvent, ImportFinishedEvent, ImportItemFailedEvent,
    ImportPausedEvent, ImportProgressEvent, ImportResumedEvent, ModelDownloadProgressEvent,
    ModelReadyEvent, ModelsImportedEvent, RegionProgressEvent, RuntimeDownloadFailedEvent,
    RuntimeDownloadProgressEvent,
    RuntimeReadyEvent,
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
            search_commands::reindex_folder,
            search_commands::reindex_assets,
            data_commands::data_info,
            data_commands::open_data_folder,
            data_commands::set_data_location,
            folder_commands::list_folders,
            folder_commands::recheck_folder,
            folder_commands::relocate_folder,
            folder_commands::delete_folder,
            folder_commands::report_original_missing,
            folder_commands::asset_detail,
            folder_commands::storage_usage,
            folder_commands::embed_status,
            inference_commands::inference_info,
            inference_commands::set_inference_ep,
            inference_commands::download_runtime,
            inference_commands::import_runtime,
            inference_commands::import_models,
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
            RuntimeDownloadProgressEvent,
            RuntimeReadyEvent,
            RuntimeDownloadFailedEvent,
            ModelsImportedEvent,
            RegionProgressEvent,
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
            // 运行时放行的目录不跨重启（scope 每次启动重建）——重启后原图全部 403,
            // 前端误报「源离线」把整个文件夹置 offline（一票否决缺陷的真正根因）。
            // 启动时对所有已注册文件夹重新放行：不按状态过滤，offline 文件夹漏放行
            // 会永远 403、永远回不到 online（重检只翻 DB 状态、不放行 scope，解不了套）。
            {
                if let Ok(store) = mm_store::Store::open(&paths.db_path()) {
                    if let Ok(folders) = store.list_folders() {
                        for f in folders {
                            if f.path.is_empty() {
                                continue;
                            }
                            let path = PathBuf::from(&f.path);
                            let _ =
                                app.asset_protocol_scope().allow_directory(&path, true);
                            // offline/missing 是运行期判定，不把误报持久为真相：
                            // 磁盘实际可达即恢复 online。不在此做 online → missing 的
                            // 降级——启动瞬间网络盘可能尚未挂载，误判 missing 会让
                            // watcher 永久跳过该文件夹；missing 交给主动重检判定。
                            if f.status != "online" && path.is_dir() {
                                let _ = store.set_folder_status(f.folder_id, "online");
                            }
                        }
                    }
                }
            }

            // 推理后端（spec 0008 / ADR-0014）：config.inference_ep → EpKind，
            // 进程最早处选定 onnxruntime 变体（一次性）；所选变体缺失/加载失败
            // 回退自带 DML 变体并记入降级（config 不改写，修复环境重启即生效）
            let (ep, runtime_degraded, runtime_missing) = {
                let cfg_value = paths.inference_ep.as_deref().unwrap_or("cpu");
                let mut parsed = mm_embed::EpKind::parse(cfg_value).unwrap_or_else(|| {
                    tracing::warn!(value = cfg_value, "inference_ep 配置值无效，按 CPU 处理");
                    mm_embed::EpKind::Cpu
                });
                let mut degraded: Option<String> = None;
                let mut missing: Option<String> = None;
                #[cfg(windows)]
                {
                    let mut candidates: Vec<PathBuf> = Vec::new();
                    if parsed == mm_embed::EpKind::Cuda {
                        candidates.push(paths.runtime_dir("cuda").join("onnxruntime.dll"));
                    }
                    if parsed == mm_embed::EpKind::Cpu {
                        // 所选 CPU 且用户下载过 CPU 变体则优先（任何变体都含 CPU EP）
                        candidates.push(paths.runtime_dir("cpu").join("onnxruntime.dll"));
                    }
                    candidates.push(paths.runtime_dir("dml").join("onnxruntime.dll"));
                    candidates.push(paths.runtime_dir("cpu").join("onnxruntime.dll"));
                    if let Some(exe) =
                        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()))
                    {
                        candidates.push(exe.join("runtime").join("dml").join("onnxruntime.dll"));
                    }
                    // 按所选 EP 会重复推入同一目录：去重保持优先级顺序
                    let mut seen = std::collections::HashSet::new();
                    candidates.retain(|c| seen.insert(c.clone()));
                    let mut loaded = false;
                    let mut last_err: Option<String> = None;
                    for cand in &candidates {
                        if !cand.is_file() {
                            continue;
                        }
                        match mm_embed::init_runtime_dylib(cand) {
                            Ok(()) => {
                                loaded = true;
                                break;
                            }
                            Err(e) => last_err = Some(e),
                        }
                    }
                    if !loaded {
                        // 不再 panic：变体全缺 = 安装损坏，但语义功能之外的一切照常可用。
                        // 记录原因走降级路径（索引不装配、搜索无语义流、设置页提示），
                        // ort 未初始化时任何 session 构建都会失败，必须跳过装配。
                        let reason = last_err.unwrap_or_else(|| {
                            "找不到 onnxruntime 变体（自带 runtime\\dml 缺失，安装可能损坏）".into()
                        });
                        missing = Some(reason);
                        parsed = mm_embed::EpKind::Cpu;
                    }
                    if parsed == mm_embed::EpKind::Cuda {
                        if !candidates[0].is_file() {
                            degraded = Some(
                                "CUDA 运行时未下载/导入，本次以自带 DirectML 变体启动".into(),
                            );
                        } else {
                            // providers_cuda.dll 依赖预检（CUDA 13 运行时/驱动缺失时
                            // onnxruntime 注册 CUDA EP 会段错误——上游问题，必须前置拦截）
                            let dir = candidates[0].parent().unwrap().to_path_buf();
                            if !mm_embed::probe_provider_dll(&dir, "onnxruntime_providers_cuda.dll") {
                                degraded = Some(
                                    "CUDA 运行时组件加载失败（需要 NVIDIA 驱动与 CUDA 13 运行时），本次以 CPU 继续".into(),
                                );
                            }
                        }
                    }
                    if degraded.is_some() && parsed == mm_embed::EpKind::Cuda {
                        // 会话层改用纯 CPU 链（当前变体上 DirectML 静默跳过 → 实际 CPU）
                        parsed = mm_embed::EpKind::Cpu;
                    }
                }
                #[cfg(not(windows))]
                mm_embed::init_runtime_dylib(std::path::Path::new("")).ok();
                if let Some(reason) = &degraded {
                    tracing::warn!(reason, "推理运行时降级");
                }
                if let Some(reason) = &missing {
                    tracing::error!(reason, "推理运行时初始化失败");
                }
                (parsed, degraded, missing)
            };

            // 分发源：config.hf_endpoint 覆盖优先，默认 GitHub Release（规格 0004 / ADR-0010）
            let endpoints = vec![paths.hf_endpoint.clone()];
            let model_dir = paths.model_dir.clone();
            app.manage(AppState::build(app.handle(), paths, model_dir, endpoints, ep));
            if let Some(reason) = runtime_degraded {
                *app.state::<AppState>().ep_degraded.lock().unwrap() = Some(reason);
            }

            // 模型已在本地则直接加载（重启后无需再下载）；运行时不可用时整体跳过装配
            {
                let state = app.state::<AppState>();
                if let Some(reason) = runtime_missing {
                    *state.runtime_missing.lock().unwrap() = Some(reason);
                    tracing::error!("推理运行时不可用，跳过索引装配（语义搜索/建索引停用，其余功能照常）");
                } else if let Err(missing) = state.load_indexers() {
                    tracing::info!(?missing, "模型未就绪，语义搜索保持降级");
                }
                embed_worker::EmbedWorker::spawn(
                    state.workspace.db_path(),
                    state.indexers.clone(),
                    state.engine.clone(),
                    app.handle().clone(),
                );
                region_worker::RegionWorker::spawn(
                    state.workspace.db_path(),
                    state.model_dir.clone(),
                    state.indexers.clone(),
                    state.engine.clone(),
                    app.handle().clone(),
                );
            }

            // 文件夹 watcher 自动同步（规格 0001 §3.9）：启动即预检全部已注册文件夹
            folder_watcher::FolderWatcher::spawn(
                app.state::<AppState>().workspace.db_path(),
                app.state::<AppState>().engine.clone(),
            );

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
