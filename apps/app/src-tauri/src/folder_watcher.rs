//! 文件夹 watcher（规格 0001 §3.9）：已注册在线文件夹的自动增量同步。
//! 变更去抖 ≥2s → 引擎空闲即对该文件夹跑一次增量导入（增量预检保证只处理新增/变化文件）；
//! 导入进行中的变更持续标记，任务结束后自动补扫。启动时对所有已注册文件夹做一次全量预检。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mm_pipeline::ImportEngine;
use mm_store::Store;
use notify::Watcher as _;

/// 事件去抖：文件落盘稳定该时长后才触发同步
const DEBOUNCE: Duration = Duration::from_secs(2);
/// 轮询节拍
const TICK: Duration = Duration::from_millis(50);
/// 文件夹清单刷新间隔（list_folders 一条 SQL，足够轻）
const RESCAN_EVERY: Duration = Duration::from_secs(2);

pub struct FolderWatcher;

impl FolderWatcher {
    pub fn spawn(db_path: PathBuf, engine: Arc<ImportEngine>) {
        std::thread::Builder::new()
            .name("mm-watch".into())
            .spawn(move || watch_loop(db_path, engine))
            .expect("启动文件夹监听线程失败");
    }
}

/// 在线且非早期迁移占位的文件夹 (folder_id, path)
fn online_folders(db_path: &Path) -> Vec<(i64, PathBuf)> {
    let Ok(store) = Store::open(db_path) else {
        return Vec::new();
    };
    store
        .list_folders()
        .unwrap_or_default()
        .into_iter()
        .filter(|f| f.status == "online" && !f.path.is_empty())
        .map(|f| (f.folder_id, PathBuf::from(&f.path)))
        .collect()
}

/// 事件路径所属文件夹（路径前缀最长匹配）
fn owner_of<'a>(folders: &'a [(i64, PathBuf)], p: &Path) -> Option<&'a (i64, PathBuf)> {
    folders
        .iter()
        .filter(|(_, root)| p.starts_with(root))
        .max_by_key(|(_, root)| root.as_os_str().len())
}

fn watch_loop(db_path: PathBuf, engine: Arc<ImportEngine>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!(error = %e, "文件监听不可用，自动同步停用（手动导入不受影响）");
            loop {
                std::thread::sleep(Duration::from_secs(3600));
            }
        }
    };
    let mut watched: HashSet<PathBuf> = HashSet::new();
    let mut folders: Vec<(i64, PathBuf)> = Vec::new();
    // folder_id -> 最近一次变更时间（去抖起点）
    let mut dirty: HashMap<i64, Instant> = HashMap::new();
    // 启动预检只做一次：若放在清单刷新里，触发后 remove 的键会被 or_insert
    // 重新插入（已到期）→ 每 2s 空跑一次同步的自激振荡（实机走查修复）
    let mut initial_pass = true;
    let mut last_rescan = Instant::now() - RESCAN_EVERY;

    loop {
        // ---- 清单刷新 ----
        if last_rescan.elapsed() >= RESCAN_EVERY {
            last_rescan = Instant::now();
            folders = online_folders(&db_path);
            let current: HashSet<PathBuf> = folders.iter().map(|(_, p)| p.clone()).collect();
            for gone in watched.difference(&current) {
                let _ = watcher.unwatch(gone);
            }
            for fresh in current.difference(&watched) {
                match watcher.watch(fresh, notify::RecursiveMode::Recursive) {
                    Ok(()) => tracing::info!(folder = %fresh.display(), "已监听文件夹变更"),
                    Err(e) => tracing::warn!(folder = %fresh.display(), error = %e, "监听失败"),
                }
            }
            watched = current;
            if initial_pass {
                initial_pass = false;
                // 首轮：所有已注册文件夹做一次启动预检（直接置为已过期，下一拍即触发）
                for (fid, _) in &folders {
                    dirty.entry(*fid).or_insert(Instant::now() - DEBOUNCE);
                }
            }
        }

        // ---- 收事件（只刷新时间戳，节流由去抖完成）----
        while let Ok(event) = rx.recv_timeout(TICK) {
            let Ok(event) = event else { continue };
            for p in event.paths {
                if let Some((fid, _)) = owner_of(&folders, &p) {
                    dirty.insert(*fid, Instant::now());
                }
            }
        }

        // ---- 触发：去抖已过且引擎空闲 ----
        let now = Instant::now();
        let due: Vec<i64> = dirty
            .iter()
            .filter(|(_, t)| now.duration_since(**t) >= DEBOUNCE)
            .map(|(fid, _)| *fid)
            .collect();
        for fid in due {
            if engine.snapshot().running {
                break; // 有任务在跑：保留标记，任务结束后下一轮补扫
            }
            let Some(path) = folders.iter().find(|(f, _)| *f == fid).map(|(_, p)| p) else {
                dirty.remove(&fid);
                continue;
            };
            dirty.remove(&fid);
            match engine.start(path.clone(), fid) {
                Ok(job_id) => {
                    tracing::info!(folder = %path.display(), job_id, "watcher 触发增量同步")
                }
                Err(e) => {
                    tracing::debug!(folder = %path.display(), error = ?e, "watcher 同步启动失败（下轮重试）")
                }
            }
        }
    }
}
