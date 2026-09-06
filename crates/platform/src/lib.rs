//! `mm-platform`：路径解析 / 配置加载 / 日志初始化（白皮书 §4.1）。
//!
//! 所有目录默认值集中在这一处，其他 crate 一律通过本 crate 取路径，
//! 不自己拼 `~/.appdata` 之类的字符串。
//!
//! 数据布局采用 ADR-0011 四级优先（`resolve_layout`）：
//! env `MIAOMORY_DATA_DIR` > 显式便携（`data\config.toml` / `.portable`） >
//! 既有配置零破坏（含 `data_dir` 覆盖） > 全新可写目录自动便携 / 只读回退经典布局。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const APP_DIR_NAME: &str = "MiaoMory";
pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const DB_FILE_NAME: &str = "index.db";
pub const THUMBS_DIR_NAME: &str = "thumbs";
pub const MODELS_DIR_NAME: &str = "models";
pub const LOGS_DIR_NAME: &str = "logs";
/// 口袋式数据目录名（位于安装目录下，ADR-0011）
pub const DATA_DIR_NAME: &str = "data";
/// 手动强制便携的空标记文件（放在 exe 旁，ADR-0011）
pub const PORTABLE_MARKER_NAME: &str = ".portable";
/// 数据根覆盖的环境变量（开发/测试/高级用户）
pub const DATA_DIR_ENV: &str = "MIAOMORY_DATA_DIR";

/// 数据布局模式（ADR-0011）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataMode {
    /// `MIAOMORY_DATA_DIR` 环境变量指定（开发/测试）
    EnvOverride,
    /// 口袋式：数据根 = `<安装目录>\data\`（或其 config 的 `data_dir` 覆盖）
    Portable,
    /// 经典配置里的 `data_dir` 指向了自定义数据根
    Rooted,
    /// 经典布局：文档工作区 + AppData 模型/配置/日志
    Classic,
}

/// 工作区与应用配置（白皮书 §3.5：工作区 = 一个 TOML 配置）
///
/// `workspace_dir`/`model_dir` 仅在经典布局下生效（向后兼容）；
/// 口袋式/数据根布局由 `data_dir` 统一指定，两个字段被忽略。
/// `hf_endpoint` 为模型分发源，默认 HuggingFace；国内可配 hf-mirror.com 等镜像。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    pub workspace_dir: Option<PathBuf>,
    pub model_dir: Option<PathBuf>,
    pub hf_endpoint: Option<String>,
    /// 数据根覆盖（ADR-0011）：设置"更改数据位置"写入此字段
    pub data_dir: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("无法定位系统目录（Documents/AppData）")]
    SystemDirUnavailable,
    #[error("配置文件解析失败：{0}")]
    ConfigParse(#[from] toml::de::Error),
    #[error("配置序列化失败：{0}")]
    ConfigSerialize(#[from] toml::ser::Error),
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
}

pub fn app_data_dir() -> Result<PathBuf, PlatformError> {
    dirs::data_dir()
        .map(|p| p.join(APP_DIR_NAME))
        .ok_or(PlatformError::SystemDirUnavailable)
}

pub fn default_workspace_dir() -> Result<PathBuf, PlatformError> {
    dirs::document_dir()
        .map(|p| p.join(APP_DIR_NAME))
        .ok_or(PlatformError::SystemDirUnavailable)
}

/// 解析后的全部路径（其余 crate 只消费这个结构）
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPaths {
    pub mode: DataMode,
    /// 经典布局：工作区（index.db + thumbs）；Rooted/Portable：等于 data_root
    pub workspace_dir: PathBuf,
    /// 经典布局：模型目录；其余布局：`<数据根>/models`
    pub model_dir: PathBuf,
    pub hf_endpoint: String,
    /// 数据根（口袋式/Rooted/env 模式）；经典布局为 None
    pub data_root: Option<PathBuf>,
    /// 经典布局的 AppData 应用目录（日志落点）；其余布局为 None
    pub app_dir: Option<PathBuf>,
    /// 生效的配置文件路径（"更改数据位置"写回这里）
    pub config_path: PathBuf,
    /// 配置存在但解析失败时的错误（不阻断启动，回退默认值；setup 初始化日志后应记录）
    pub config_load_error: Option<String>,
}

impl ResolvedPaths {
    pub fn db_path(&self) -> PathBuf {
        self.workspace_dir.join(DB_FILE_NAME)
    }
    pub fn thumbs_dir(&self) -> PathBuf {
        self.workspace_dir.join(THUMBS_DIR_NAME)
    }
    pub fn logs_dir(&self) -> PathBuf {
        match (&self.data_root, &self.app_dir) {
            (Some(root), _) => root.join(LOGS_DIR_NAME),
            (None, Some(app)) => app.join(LOGS_DIR_NAME),
            (None, None) => app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(LOGS_DIR_NAME),
        }
    }
    /// "打开数据文件夹"的目标：数据根（口袋式）或工作区（经典）
    pub fn data_folder(&self) -> PathBuf {
        self.data_root.clone().unwrap_or_else(|| self.workspace_dir.clone())
    }
    pub fn is_portable(&self) -> bool {
        self.data_root.is_some()
    }
}

pub fn config_path() -> Result<PathBuf, PlatformError> {
    Ok(app_data_dir()?.join(CONFIG_FILE_NAME))
}

/// 读取指定路径的配置；文件不存在返回默认值；解析失败返回默认值并携带错误（不阻断启动）
fn load_config_opt(path: &Path) -> (AppConfig, Option<String>) {
    match std::fs::read_to_string(path) {
        Err(_) => (AppConfig::default(), None),
        Ok(raw) => match toml::from_str::<AppConfig>(&raw) {
            Ok(cfg) => (cfg, None),
            Err(e) => (AppConfig::default(), Some(e.to_string())),
        },
    }
}

/// 读取指定路径的配置；文件不存在时返回默认值；解析失败原样报错
pub fn load_config_at(path: &Path) -> Result<AppConfig, PlatformError> {
    let raw = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&raw)?)
}

/// 保存配置到指定路径（父目录按需创建）
pub fn save_config_at(path: &Path, config: &AppConfig) -> Result<(), PlatformError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, toml::to_string_pretty(config)?)?;
    Ok(())
}

/// 读取 AppData 配置；文件不存在时返回默认值（不自动创建，首次保存才落盘）
pub fn load_config() -> Result<AppConfig, PlatformError> {
    load_config_at(&config_path()?)
}

/// 保存配置到 AppData（父目录按需创建）
pub fn save_config(config: &AppConfig) -> Result<(), PlatformError> {
    save_config_at(&config_path()?, config)
}

/// 目录可写探测：尝试在其中创建并删除临时文件
pub fn dir_is_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".mm-write-probe-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            drop(std::fs::remove_file(&probe));
            true
        }
        Err(_) => false,
    }
}

/// 实际入口：从运行环境解析布局（exe 目录、环境变量、系统目录）
pub fn resolve_layout() -> Result<ResolvedPaths, PlatformError> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .ok_or(PlatformError::SystemDirUnavailable)?;
    let env_data_dir = std::env::var_os(DATA_DIR_ENV).map(PathBuf::from);
    resolve_layout_at(
        &exe_dir,
        env_data_dir,
        app_data_dir().ok().as_deref(),
        default_workspace_dir().ok().as_deref(),
    )
}

/// 四级优先布局解析（ADR-0011，可注入路径以便测试）
///
/// - `app_dir`：AppData 应用目录（经典配置所在）；None = 系统目录不可用
/// - `doc_dir`：默认工作区目录（文档\MiaoMory）；None 同上
pub fn resolve_layout_at(
    exe_dir: &Path,
    env_data_dir: Option<PathBuf>,
    app_dir: Option<&Path>,
    doc_dir: Option<&Path>,
) -> Result<ResolvedPaths, PlatformError> {
    // ① 环境变量：最高优先，全部数据落指定根
    if let Some(root) = env_data_dir {
        let config_path = root.join(CONFIG_FILE_NAME);
        let (cfg, err) = load_config_opt(&config_path);
        return Ok(rooted(DataMode::EnvOverride, root, &cfg, config_path, err));
    }

    let data_root_default = exe_dir.join(DATA_DIR_NAME);
    let portable_config = data_root_default.join(CONFIG_FILE_NAME);

    // ② 显式便携：data\config.toml 或 .portable 标记
    if portable_config.is_file() || exe_dir.join(PORTABLE_MARKER_NAME).is_file() {
        let (cfg, err) = load_config_opt(&portable_config);
        let root = cfg.data_dir.clone().unwrap_or_else(|| data_root_default.clone());
        return Ok(rooted(DataMode::Portable, root, &cfg, portable_config, err));
    }

    // ③ 既有 AppData 配置：零破坏（data_dir 覆盖转数据根布局）
    if let Some(app) = app_dir {
        let app_config = app.join(CONFIG_FILE_NAME);
        if app_config.is_file() {
            let (cfg, err) = load_config_opt(&app_config);
            if let Some(root) = cfg.data_dir.clone() {
                return Ok(rooted(DataMode::Rooted, root, &cfg, app_config, err));
            }
            return classic(&cfg, app_dir, doc_dir, err);
        }
    }

    // ④ 全新安装：exe 目录可写 → 自动口袋式（写入初始 config 固化判定）
    if dir_is_writable(exe_dir) {
        let cfg = AppConfig::default();
        save_config_at(&portable_config, &cfg)?;
        return Ok(rooted(DataMode::Portable, data_root_default, &cfg, portable_config, None));
    }

    // ⑤ 只读安装目录（如 Program Files）：经典布局
    classic(&AppConfig::default(), app_dir, doc_dir, None)
}

fn rooted(
    mode: DataMode,
    root: PathBuf,
    cfg: &AppConfig,
    config_path: PathBuf,
    config_load_error: Option<String>,
) -> ResolvedPaths {
    ResolvedPaths {
        mode,
        workspace_dir: root.clone(),
        model_dir: root.join(MODELS_DIR_NAME),
        hf_endpoint: cfg.hf_endpoint.clone().unwrap_or_else(default_hf_endpoint),
        data_root: Some(root),
        app_dir: None,
        config_path,
        config_load_error,
    }
}

fn classic(
    cfg: &AppConfig,
    app_dir: Option<&Path>,
    doc_dir: Option<&Path>,
    config_load_error: Option<String>,
) -> Result<ResolvedPaths, PlatformError> {
    let workspace_dir = match &cfg.workspace_dir {
        Some(p) => p.clone(),
        None => doc_dir.ok_or(PlatformError::SystemDirUnavailable)?.to_path_buf(),
    };
    let model_dir = match &cfg.model_dir {
        Some(p) => p.clone(),
        None => app_dir
            .ok_or(PlatformError::SystemDirUnavailable)?
            .join(MODELS_DIR_NAME),
    };
    let config_path = app_dir
        .ok_or(PlatformError::SystemDirUnavailable)?
        .join(CONFIG_FILE_NAME);
    Ok(ResolvedPaths {
        mode: DataMode::Classic,
        workspace_dir,
        model_dir,
        hf_endpoint: cfg.hf_endpoint.clone().unwrap_or_else(default_hf_endpoint),
        data_root: None,
        app_dir: app_dir.map(|p| p.to_path_buf()),
        config_path,
        config_load_error,
    })
}

fn default_hf_endpoint() -> String {
    // 产品默认分发源：GitHub Release 托管的模型资产（ADR-0010 分发约定）
    "https://github.com/ChenXu233/MiaoMoryQwQ/releases/download/models-v1".to_string()
}

/// 确保工作区目录结构存在（db 与 thumbs 都在工作区内，备份 = checkpoint 后整目录复制）
pub fn ensure_workspace_layout(paths: &ResolvedPaths) -> Result<(), PlatformError> {
    std::fs::create_dir_all(&paths.workspace_dir)?;
    std::fs::create_dir_all(paths.thumbs_dir())?;
    std::fs::create_dir_all(&paths.model_dir)?;
    Ok(())
}

/// 初始化 tracing：日志落盘到当前布局的 logs 目录（ADR-0011），控制台镜像输出
pub fn init_tracing(paths: &ResolvedPaths) -> Result<(), PlatformError> {
    let logs_dir = paths.logs_dir();
    std::fs::create_dir_all(&logs_dir)?;
    let file_appender = tracing_appender::rolling::daily(logs_dir, "miaomory.log");
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(file_appender)
        .with_ansi(false)
        .try_init()
        .ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// 在临时目录搭一个"假安装环境"：返回 (root, exe_dir)
    fn sandbox(tag: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("mm-layout-{}-{}", tag, std::process::id()));
        let exe_dir = root.join("install");
        std::fs::create_dir_all(&exe_dir).unwrap();
        (root, exe_dir)
    }

    fn app_dir_of(root: &Path) -> PathBuf {
        root.join("appdata").join(APP_DIR_NAME)
    }

    fn doc_dir_of(root: &Path) -> PathBuf {
        root.join("docs").join(APP_DIR_NAME)
    }

    #[test]
    fn env_override_beats_everything_and_roots_all_data() {
        let (root, exe_dir) = sandbox("env");
        let env_dir = root.join("envdata");
        // 即使存在 .portable 标记，env 也必须胜出
        std::fs::write(exe_dir.join(PORTABLE_MARKER_NAME), "").unwrap();

        let r = resolve_layout_at(&exe_dir, Some(env_dir.clone()), Some(&app_dir_of(&root)), Some(&doc_dir_of(&root)))
            .unwrap();

        assert_eq!(r.mode, DataMode::EnvOverride);
        assert_eq!(r.data_root.as_deref(), Some(env_dir.as_path()));
        assert_eq!(r.workspace_dir, env_dir);
        assert_eq!(r.model_dir, env_dir.join(MODELS_DIR_NAME));
        assert_eq!(r.logs_dir(), env_dir.join(LOGS_DIR_NAME));
        assert_eq!(r.config_path, env_dir.join(CONFIG_FILE_NAME));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn portable_marker_forces_install_dir_layout() {
        let (root, exe_dir) = sandbox("marker");
        std::fs::write(exe_dir.join(PORTABLE_MARKER_NAME), "").unwrap();

        let r = resolve_layout_at(&exe_dir, None, Some(&app_dir_of(&root)), Some(&doc_dir_of(&root))).unwrap();

        assert_eq!(r.mode, DataMode::Portable);
        assert_eq!(r.data_root.as_deref(), Some(exe_dir.join(DATA_DIR_NAME).as_path()));
        assert_eq!(r.config_path, exe_dir.join(DATA_DIR_NAME).join(CONFIG_FILE_NAME));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn portable_config_file_also_triggers_portable() {
        let (root, exe_dir) = sandbox("pcfg");
        std::fs::create_dir_all(exe_dir.join(DATA_DIR_NAME)).unwrap();
        std::fs::write(exe_dir.join(DATA_DIR_NAME).join(CONFIG_FILE_NAME), "").unwrap();

        let r = resolve_layout_at(&exe_dir, None, Some(&app_dir_of(&root)), Some(&doc_dir_of(&root))).unwrap();

        assert_eq!(r.mode, DataMode::Portable);
        assert_eq!(r.workspace_dir, exe_dir.join(DATA_DIR_NAME));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn portable_config_data_dir_relocates_root_but_keeps_config_in_place() {
        let (root, exe_dir) = sandbox("prelocate");
        let data_dir = exe_dir.join(DATA_DIR_NAME);
        std::fs::create_dir_all(&data_dir).unwrap();
        let custom = root.join("custom-location");
        std::fs::write(
            data_dir.join(CONFIG_FILE_NAME),
            format!("data_dir = {:?}", custom).replace('\\', "\\\\"),
        )
        .unwrap();

        let r = resolve_layout_at(&exe_dir, None, Some(&app_dir_of(&root)), Some(&doc_dir_of(&root))).unwrap();

        assert_eq!(r.mode, DataMode::Portable);
        assert_eq!(r.workspace_dir, custom);
        assert_eq!(r.model_dir, custom.join(MODELS_DIR_NAME));
        // config 留在原便携目录（标记解析的锚点不迁移）
        assert_eq!(r.config_path, data_dir.join(CONFIG_FILE_NAME));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn fresh_writable_install_dir_auto_portables_and_writes_initial_config() {
        let (root, exe_dir) = sandbox("fresh");
        // app_dir 不存在配置文件（全新安装）

        let r = resolve_layout_at(&exe_dir, None, Some(&app_dir_of(&root)), Some(&doc_dir_of(&root))).unwrap();

        assert_eq!(r.mode, DataMode::Portable);
        assert_eq!(r.workspace_dir, exe_dir.join(DATA_DIR_NAME));
        // 初始 config 已落盘，二次解析结果稳定
        assert!(exe_dir.join(DATA_DIR_NAME).join(CONFIG_FILE_NAME).is_file());
        let r2 = resolve_layout_at(&exe_dir, None, Some(&app_dir_of(&root)), Some(&doc_dir_of(&root))).unwrap();
        assert_eq!(r2.mode, DataMode::Portable);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn readonly_install_dir_falls_back_to_classic() {
        let (root, _) = sandbox("readonly");
        // Windows 上目录只读属性不阻止写入；用"路径被文件占用"模拟不可写探针失败
        let blocker = root.join("not-a-dir");
        std::fs::write(&blocker, "").unwrap();

        let r = resolve_layout_at(&blocker, None, Some(&app_dir_of(&root)), Some(&doc_dir_of(&root))).unwrap();

        assert_eq!(r.mode, DataMode::Classic);
        assert_eq!(r.workspace_dir, doc_dir_of(&root));
        assert_eq!(r.model_dir, app_dir_of(&root).join(MODELS_DIR_NAME));
        assert_eq!(r.data_root, None);
        assert_eq!(r.logs_dir(), app_dir_of(&root).join(LOGS_DIR_NAME));
        assert_eq!(r.config_path, app_dir_of(&root).join(CONFIG_FILE_NAME));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn existing_appdata_config_stays_classic_zero_breakage() {
        let (root, exe_dir) = sandbox("zerobreak");
        let app_dir = app_dir_of(&root);
        std::fs::create_dir_all(&app_dir).unwrap();
        std::fs::write(app_dir.join(CONFIG_FILE_NAME), "hf_endpoint = \"https://hf-mirror.com\"\n").unwrap();

        let r = resolve_layout_at(&exe_dir, None, Some(&app_dir), Some(&doc_dir_of(&root))).unwrap();

        // exe 可写也不自动便携（老用户零破坏）
        assert_eq!(r.mode, DataMode::Classic);
        assert_eq!(r.hf_endpoint, "https://hf-mirror.com");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn classic_config_data_dir_becomes_rooted() {
        let (root, exe_dir) = sandbox("rooted");
        let app_dir = app_dir_of(&root);
        std::fs::create_dir_all(&app_dir).unwrap();
        let custom = root.join("chosen");
        std::fs::write(
            app_dir.join(CONFIG_FILE_NAME),
            format!("data_dir = {:?}", custom).replace('\\', "\\\\"),
        )
        .unwrap();

        let r = resolve_layout_at(&exe_dir, None, Some(&app_dir), Some(&doc_dir_of(&root))).unwrap();

        assert_eq!(r.mode, DataMode::Rooted);
        assert_eq!(r.workspace_dir, custom);
        assert_eq!(r.config_path, app_dir.join(CONFIG_FILE_NAME));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn classic_config_workspace_override_still_honored() {
        let (root, exe_dir) = sandbox("classicws");
        let app_dir = app_dir_of(&root);
        std::fs::create_dir_all(&app_dir).unwrap();
        let ws = root.join("my-workspace");
        std::fs::write(
            app_dir.join(CONFIG_FILE_NAME),
            format!("workspace_dir = {:?}", ws).replace('\\', "\\\\"),
        )
        .unwrap();

        let r = resolve_layout_at(&exe_dir, None, Some(&app_dir), Some(&doc_dir_of(&root))).unwrap();

        assert_eq!(r.mode, DataMode::Classic);
        assert_eq!(r.workspace_dir, ws);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn corrupt_config_falls_back_to_default_with_error_recorded() {
        let (root, exe_dir) = sandbox("corrupt");
        let data_dir = exe_dir.join(DATA_DIR_NAME);
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::write(data_dir.join(CONFIG_FILE_NAME), "不是 TOML {{{{").unwrap();

        let r = resolve_layout_at(&exe_dir, None, Some(&app_dir_of(&root)), Some(&doc_dir_of(&root))).unwrap();

        assert_eq!(r.mode, DataMode::Portable);
        assert!(r.config_load_error.is_some());
        assert_eq!(r.hf_endpoint, "https://github.com/ChenXu233/MiaoMoryQwQ/releases/download/models-v1");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_system_dirs_make_classic_unavailable() {
        let (_, exe_dir) = sandbox("nodirs");
        let r = resolve_layout_at(exe_dir.clone().as_path(), None, None, None);
        // 全新且可写 → 先走自动便携，不受系统目录影响
        assert!(r.is_ok());
        std::fs::remove_dir_all(exe_dir).ok();
    }

    #[test]
    fn readonly_dir_probe_reports_not_writable() {
        // 用一个"是文件不是目录"的路径模拟不可写
        let (root, _) = sandbox("probe");
        let blocker = root.join("file");
        std::fs::write(&blocker, "").unwrap();
        assert!(!dir_is_writable(&blocker));
        assert!(dir_is_writable(&root));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn config_roundtrips_with_data_dir_field() {
        let cfg = AppConfig {
            workspace_dir: Some(PathBuf::from("/ws")),
            model_dir: Some(PathBuf::from("/models")),
            hf_endpoint: Some("https://hf-mirror.com".into()),
            data_dir: Some(PathBuf::from("D:\\索引数据")),
        };
        let raw = toml::to_string_pretty(&cfg).unwrap();
        let parsed: AppConfig = toml::from_str(&raw).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[test]
    fn default_config_resolves_without_panic() {
        // 真实环境解析：不假设测试机一定有 Documents/AppData
        let resolved = resolve_layout();
        assert!(resolved.is_ok() || matches!(resolved, Err(PlatformError::SystemDirUnavailable)));
    }
}
