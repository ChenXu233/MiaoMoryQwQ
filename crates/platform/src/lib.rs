//! `mm-platform`：路径解析 / 配置加载 / 日志初始化（白皮书 §4.1）。
//!
//! 所有目录默认值集中在这一处，其他 crate 一律通过本 crate 取路径，
//! 不自己拼 `~/.appdata` 之类的字符串。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const APP_DIR_NAME: &str = "MiaoMory";
pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const DB_FILE_NAME: &str = "index.db";
pub const THUMBS_DIR_NAME: &str = "thumbs";
pub const MODELS_DIR_NAME: &str = "models";
pub const LOGS_DIR_NAME: &str = "logs";

/// 工作区与应用配置（白皮书 §3.5：工作区 = 一个 TOML 配置）
///
/// `None` 表示使用默认值；`workspace_dir` 缺省为 `文档/MiaoMory`，
/// 模型目录缺省为 `AppData/MiaoMory/models`（模型不进工作区，换工作区不重下模型）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    pub workspace_dir: Option<PathBuf>,
    pub model_dir: Option<PathBuf>,
    /// 模型分发源，默认 HuggingFace；国内可配 hf-mirror.com 等镜像（ADR-0010 分发约定）
    pub hf_endpoint: Option<String>,
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

impl AppConfig {
    /// 生效配置：显式值优先，否则取默认值
    pub fn resolved(&self) -> Result<ResolvedPaths, PlatformError> {
        Ok(ResolvedPaths {
            workspace_dir: match &self.workspace_dir {
                Some(p) => p.clone(),
                None => default_workspace_dir()?,
            },
            model_dir: match &self.model_dir {
                Some(p) => p.clone(),
                None => app_data_dir()?.join(MODELS_DIR_NAME),
            },
            hf_endpoint: self
                .hf_endpoint
                .clone()
                .unwrap_or_else(|| "https://huggingface.co".to_string()),
        })
    }
}

/// 解析后的全部路径（其余 crate 只消费这个结构）
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPaths {
    pub workspace_dir: PathBuf,
    pub model_dir: PathBuf,
    pub hf_endpoint: String,
}

impl ResolvedPaths {
    pub fn db_path(&self) -> PathBuf {
        self.workspace_dir.join(DB_FILE_NAME)
    }
    pub fn thumbs_dir(&self) -> PathBuf {
        self.workspace_dir.join(THUMBS_DIR_NAME)
    }
    pub fn logs_dir(&self) -> PathBuf {
        app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(LOGS_DIR_NAME)
    }
}

pub fn config_path() -> Result<PathBuf, PlatformError> {
    Ok(app_data_dir()?.join(CONFIG_FILE_NAME))
}

/// 读取配置；文件不存在时返回默认值（不自动创建，首次保存才落盘）
pub fn load_config() -> Result<AppConfig, PlatformError> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let raw = std::fs::read_to_string(&path)?;
    Ok(toml::from_str(&raw)?)
}

/// 保存配置（父目录按需创建）
pub fn save_config(config: &AppConfig) -> Result<(), PlatformError> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, toml::to_string_pretty(config)?)?;
    Ok(())
}

/// 确保工作区目录结构存在（db 与 thumbs 都在工作区内，备份 = checkpoint 后整目录复制）
pub fn ensure_workspace_layout(paths: &ResolvedPaths) -> Result<(), PlatformError> {
    std::fs::create_dir_all(&paths.workspace_dir)?;
    std::fs::create_dir_all(paths.thumbs_dir())?;
    std::fs::create_dir_all(&paths.model_dir)?;
    Ok(())
}

/// 初始化 tracing：日志落盘到 AppData/MiaoMory/logs（AGENT.md §7），控制台镜像输出
pub fn init_tracing() -> Result<(), PlatformError> {
    let logs_dir: PathBuf = app_data_dir()?.join(LOGS_DIR_NAME);
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

/// 目录占位类型检查：确保 `ResolvedPaths` 路径函数与常量命名一致
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn cfg_with_tmp(tmp: &Path) -> AppConfig {
        AppConfig {
            workspace_dir: Some(tmp.join("ws")),
            model_dir: Some(tmp.join("models")),
            hf_endpoint: Some("https://hf-mirror.com".into()),
        }
    }

    #[test]
    fn resolved_paths_use_config_over_defaults() {
        let tmp = std::env::temp_dir().join("mm-platform-test");
        let resolved = cfg_with_tmp(&tmp).resolved().unwrap();
        assert_eq!(resolved.db_path(), tmp.join("ws").join(DB_FILE_NAME));
        assert_eq!(resolved.thumbs_dir(), tmp.join("ws").join(THUMBS_DIR_NAME));
        assert_eq!(resolved.model_dir, tmp.join("models"));
        assert_eq!(resolved.hf_endpoint, "https://hf-mirror.com");
    }

    #[test]
    fn config_roundtrips_through_toml() {
        let cfg = cfg_with_tmp(Path::new("/tmp"));
        let raw = toml::to_string_pretty(&cfg).unwrap();
        let parsed: AppConfig = toml::from_str(&raw).unwrap();
        assert_eq!(cfg, parsed);
    }

    #[test]
    fn default_config_resolves_without_panic() {
        // 不假设测试机一定有 Documents 目录，只要求"要么解析成功，要么给出可读错误"
        let resolved = AppConfig::default().resolved();
        assert!(resolved.is_ok() || matches!(resolved, Err(PlatformError::SystemDirUnavailable)));
    }

    #[test]
    fn ensure_workspace_layout_creates_dirs() {
        let tmp = std::env::temp_dir().join(format!("mm-ws-{}", std::process::id()));
        let cfg = cfg_with_tmp(&tmp);
        let resolved = cfg.resolved().unwrap();
        ensure_workspace_layout(&resolved).unwrap();
        assert!(resolved.db_path().parent().unwrap().is_dir());
        assert!(resolved.thumbs_dir().is_dir());
        assert!(resolved.model_dir.is_dir());
        std::fs::remove_dir_all(&tmp).ok();
    }
}
