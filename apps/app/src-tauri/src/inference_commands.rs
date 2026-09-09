//! P7 命令：推理后端查询/切换、CUDA 运行时下载与导入、模型本地导入。
//! 规格 0008、ADR-0014。

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::events::{ModelsImportedEvent, RuntimeDownloadProgressEvent, RuntimeReadyEvent};
use crate::state::AppState;

/// 运行时下载任务防重入（与模型下载同模式，spec 0004 验收 7）
static RUNTIME_DOWNLOAD_IN_FLIGHT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Type)]
pub struct EpOption {
    pub kind: String,
    /// 检测层面可用（DirectML：Windows；CUDA：运行时就绪）
    pub available: bool,
    /// 检测到独立显卡且变体就绪时提示推荐
    pub recommended: bool,
    pub hint: String,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct InferenceInfo {
    /// 用户选择（config）
    pub current_ep: String,
    /// 本次会话实际装配所用后端（降级后为 cpu）
    pub effective_ep: String,
    /// 非空 = 本次会话发生过降级（所选后端不可用），前端展示原因
    pub degraded_reason: Option<String>,
    /// 非空 = 推理运行时整体不可用（变体缺失/清单损坏）：语义搜索与建索引停用，
    /// 其余功能照常；前端以此区分「降级但可用」与「语义功能停用」
    pub runtime_missing: Option<String>,
    pub options: Vec<EpOption>,
    /// 所选变体的运行时是否就绪（CUDA 下载/导入完成）
    pub runtime_ready: bool,
    /// 检测到独立显卡（NVIDIA/AMD 独显特征名，仅文案提示不作开关）
    pub gpu_detected: bool,
}

fn parse_ep(state: &AppState) -> mm_embed::EpKind {
    state.ep
}

#[tauri::command]
#[specta::specta]
pub fn inference_info(state: State<'_, AppState>) -> InferenceInfo {
    // 清单损坏不再 panic（曾令设置页必崩）：带出原因走 runtime_missing 展示
    let (manifest, manifest_err) = match mm_embed::runtime::runtime_manifest() {
        Ok(m) => (Some(m), None),
        Err(e) => (None, Some(e)),
    };
    let selected = parse_ep(&state);
    let degraded = state.ep_degraded.lock().unwrap().clone();
    let runtime_missing = state.runtime_missing.lock().unwrap().clone().or(manifest_err);
    let gpu_detected = detect_discrete_gpu();
    let cuda_variant = manifest.as_ref().and_then(|m| m.variant("cuda")).cloned();
    let runtime_ready = cuda_variant
        .map(|v| v.is_ready(&state.workspace.runtime_dir("cuda")))
        .unwrap_or(false);

    let options = vec![
        EpOption {
            kind: "cpu".into(),
            available: true,
            recommended: false,
            hint: "零依赖，所有机器可用；建索引最慢".into(),
        },
        EpOption {
            kind: "directml".into(),
            available: cfg!(windows),
            recommended: false,
            hint: "Windows 自带 GPU 加速，无需下载；推荐核显 / AMD 显卡用户".into(),
        },
        EpOption {
            kind: "cuda".into(),
            available: runtime_ready,
            recommended: runtime_ready && gpu_detected,
            hint: "NVIDIA 显卡专用，建索引最快；首次需下载运行时包（约 350MB）".into(),
        },
    ];

    InferenceInfo {
        current_ep: selected.as_str().into(),
        effective_ep: if degraded.is_some() {
            "cpu".into()
        } else {
            selected.as_str().into()
        },
        degraded_reason: degraded,
        runtime_missing,
        options,
        runtime_ready,
        gpu_detected,
    }
}

/// 切换推理后端：写 config.inference_ep，重启生效（spec 0008 §3.2）
#[tauri::command]
#[specta::specta]
pub fn set_inference_ep(state: State<'_, AppState>, ep: String) -> Result<(), String> {
    if mm_embed::EpKind::parse(&ep).is_none() {
        return Err(format!("未知的推理后端：{ep}"));
    }
    let config_path = state.workspace.config_path.clone();
    let mut cfg: mm_platform::AppConfig =
        mm_platform::load_config_at(&config_path).unwrap_or_default();
    cfg.inference_ep = Some(ep.clone());
    mm_platform::save_config_at(&config_path, &cfg).map_err(|e| format!("配置写入失败：{e}"))?;
    tracing::info!(ep = %ep, "推理后端已更改，重启后生效");
    Ok(())
}

/// 下载所选变体的运行时包（后台线程；断点续传 + 校验 + 解压，spec 0008 §3.3）
#[tauri::command]
#[specta::specta]
pub fn download_runtime(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: String,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    if kind != "cuda" && kind != "cpu" {
        return Err(format!("运行时 {kind} 不支持按需下载"));
    }
    if RUNTIME_DOWNLOAD_IN_FLIGHT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(()); // 已有下载任务在跑
    }
    let endpoints = vec![
        "https://github.com/microsoft/onnxruntime/releases/download/v1.28.0".to_string(),
        state.model_endpoints[0].clone(),
    ];
    let runtime_dir = state
        .workspace
        .runtime_dir("cuda")
        .parent()
        .unwrap()
        .to_path_buf();
    let app2 = app.clone();

    std::thread::spawn(move || {
        let manifest = match mm_embed::runtime::runtime_manifest() {
            Ok(m) => m,
            Err(e) => {
                tracing::error!(reason = %e, "运行时清单解析失败，下载任务终止");
                RUNTIME_DOWNLOAD_IN_FLIGHT.store(false, Ordering::SeqCst);
                return;
            }
        };
        let Some(variant) = manifest.variant(&kind).cloned() else {
            RUNTIME_DOWNLOAD_IN_FLIGHT.store(false, Ordering::SeqCst);
            return;
        };
        let progress = |_name: &str, received: u64, total: u64| {
            let _ = RuntimeDownloadProgressEvent {
                received: i32::try_from(received).unwrap_or(i32::MAX),
                total: i32::try_from(total).unwrap_or(i32::MAX),
            }
            .emit(&app2);
        };
        let result =
            mm_embed::runtime::ensure_runtime(&endpoints, &variant, &runtime_dir, &progress);
        match result {
            Ok(()) => {
                tracing::info!(kind = %kind, "运行时就绪");
                let _ = RuntimeReadyEvent {}.emit(&app2);
            }
            Err(e) => {
                tracing::warn!(code = ?e, kind = %kind, "运行时下载失败");
            }
        }
        RUNTIME_DOWNLOAD_IN_FLIGHT.store(false, Ordering::SeqCst);
    });
    Ok(())
}

/// 从本地 zip 导入运行时（spec 0008 §3.4）；返回是否通过清单校验（false = 未校验来源）
#[tauri::command]
#[specta::specta]
pub fn import_runtime(state: State<'_, AppState>, path: String) -> Result<bool, String> {
    let manifest =
        mm_embed::runtime::runtime_manifest().map_err(|e| format!("运行时清单损坏：{e}"))?;
    // 按 zip 名猜测变体（含 "cuda" 走 cuda，否则 cpu）
    let lower = path.to_ascii_lowercase();
    let kind = if lower.contains("cuda") {
        "cuda"
    } else {
        "cpu"
    };
    let Some(variant) = manifest.variant(kind).cloned() else {
        return Err(format!("运行时 {kind} 没有分发清单"));
    };
    let verified = mm_embed::runtime::import_runtime_zip(
        std::path::Path::new(&path),
        &variant,
        state.workspace.runtime_dir("cuda").parent().unwrap(),
    )
    .map_err(|e| e.to_string())?;
    tracing::info!(kind, verified, "运行时本地导入完成");
    Ok(verified)
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ModelFileIssue {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ImportReport {
    pub imported: i32,
    pub skipped: i32,
    pub mismatched: Vec<ModelFileIssue>,
}

/// 从本地目录/zip 导入模型（spec 0008 §3.5）；全部匹配才落文件，否则返回明细
#[tauri::command]
#[specta::specta]
pub fn import_models(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<ImportReport, String> {
    let manifest = mm_embed::manifest::manifest();
    let report =
        mm_embed::runtime::import_models(std::path::Path::new(&path), &manifest, &state.model_dir)
            .map_err(|e| e.to_string())?;
    let out = ImportReport {
        imported: i32::try_from(report.imported).unwrap_or(i32::MAX),
        skipped: i32::try_from(report.skipped).unwrap_or(i32::MAX),
        mismatched: report
            .mismatched
            .into_iter()
            .map(|(name, reason)| ModelFileIssue { name, reason })
            .collect(),
    };
    // 有新文件落地才触发重装配
    if out.imported > 0 {
        let st = app.state::<AppState>();
        if let Err(missing) = st.load_indexers() {
            tracing::warn!(?missing, "模型导入后装配失败");
        } else {
            let _ = ModelsImportedEvent {}.emit(&app);
        }
    }
    Ok(out)
}

/// 独立显卡特征检测（WMI 名单法；仅用于推荐文案，不作功能开关——spec 0008 §3.6）。
/// PowerShell 启动约几百 ms：OnceLock 缓存，进程内只查一次。
fn detect_discrete_gpu() -> bool {
    static GPU: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *GPU.get_or_init(|| {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let output = std::process::Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    "(Get-CimInstance Win32_VideoController).Name -join '`n'",
                ])
                .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
                .output();
            let Ok(out) = output else { return false };
            let names = String::from_utf8_lossy(&out.stdout).to_lowercase();
            names.contains("geforce")
                || names.contains("rtx")
                || names.contains("gtx")
                || names.contains("radeon rx")
                || names.contains("arc")
        }
        #[cfg(not(windows))]
        {
            false
        }
    })
}
