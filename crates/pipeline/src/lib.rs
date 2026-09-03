//! `mm-pipeline`：摄取流水线 scan → hash → decode → thumb → persist（白皮书 §4.3）。
//!
//! 引擎在独立线程串行持久化（SQLite 单写者），CPU 密集段用 rayon 并行；
//! 暂停/恢复以 chunk 为界；坏文件进入失败列表不阻塞批次（ADR-0010 TDD 覆盖）。

pub mod decode;
pub mod storage;
pub mod thumb;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mm_core::{Clock, DecodedImage, ErrorCode, EventSink, PipelineEvent, StorageAdapter};
use mm_store::{NewAsset, Store};
use rayon::prelude::*;
use sha2::{Digest, Sha256};

pub use decode::{decode_photo, is_supported};
pub use storage::LocalDiskAdapter;
pub use thumb::{make_thumbnail, thumb_key, THUMB_MAX_EDGE};

/// 引擎可调参数（测试用小 chunk 与自动暂停钩子）
#[derive(Debug, Clone)]
pub struct ImportConfig {
    pub chunk_size: usize,
    /// 处理到 done >= N 时自动暂停（0 = 不暂停）；手工验证与测试用
    pub pause_after_done: u64,
}

impl Default for ImportConfig {
    fn default() -> Self {
        Self { chunk_size: 32, pause_after_done: 0 }
    }
}

/// 任务快照（命令层返回给 UI）
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct JobSnapshot {
    pub job_id: u64,
    pub total: u64,
    pub done: u64,
    pub failed: u64,
    pub paused: bool,
    pub finished: bool,
    pub running: bool,
}

struct JobRuntime {
    job_id: u64,
    total: u64,
    done: u64,
    failed: u64,
    paused: bool,
    finished: bool,
    abort: bool,
}

#[derive(Default)]
struct EngineState {
    job: Option<JobRuntime>,
}

/// 导入引擎：同一时刻至多一个任务（`ImportBusy`）
pub struct ImportEngine {
    state: Mutex<EngineState>,
    db_path: PathBuf,
    thumbs_dir: PathBuf,
    sink: Arc<dyn EventSink>,
    clock: Arc<dyn Clock>,
    config: ImportConfig,
    next_job_id: AtomicU64,
}

impl ImportEngine {
    pub fn new(
        db_path: PathBuf,
        thumbs_dir: PathBuf,
        sink: Arc<dyn EventSink>,
        clock: Arc<dyn Clock>,
        config: ImportConfig,
    ) -> Self {
        Self {
            state: Mutex::new(EngineState { job: None }),
            db_path,
            thumbs_dir,
            sink,
            clock,
            config,
            next_job_id: AtomicU64::new(1),
        }
    }

    /// 启动导入；已有任务在跑时返回 `ImportBusy`
    pub fn start(self: &Arc<Self>, folder: PathBuf) -> Result<u64, ErrorCode> {
        if !folder.is_dir() {
            return Err(ErrorCode::ReadFailed);
        }
        let mut state = self.state.lock().unwrap();
        if state.job.as_ref().is_some_and(|j| !j.finished) {
            return Err(ErrorCode::ImportBusy);
        }
        let job_id = self.next_job_id.fetch_add(1, Ordering::SeqCst);
        let files = scan_folder(&folder).map_err(|_| ErrorCode::ReadFailed)?;
        let total = files.len() as u64;
        state.job = Some(JobRuntime {
            job_id,
            total,
            done: 0,
            failed: 0,
            paused: false,
            finished: false,
            abort: false,
        });
        drop(state);

        let engine = Arc::clone(self);
        std::thread::Builder::new()
            .name("mm-import".into())
            .spawn(move || engine.run(files, job_id))
            .map_err(|_| ErrorCode::Unknown)?;
        Ok(job_id)
    }

    pub fn pause(&self) {
        let mut state = self.state.lock().unwrap();
        if let Some(job) = state.job.as_mut() {
            if !job.finished {
                job.paused = true;
            }
        }
    }

    pub fn resume(&self) {
        let mut state = self.state.lock().unwrap();
        if let Some(job) = state.job.as_mut() {
            if job.paused && !job.finished {
                job.paused = false;
                let _ = self.sink.emit(PipelineEvent::Resumed);
            }
        }
    }

    /// 停止任务（用户确认后；幂等恢复 = 重新导入，哈希去重保证无重复）
    pub fn abort(&self) {
        let mut state = self.state.lock().unwrap();
        if let Some(job) = state.job.as_mut() {
            if !job.finished {
                job.abort = true;
                job.paused = false;
            }
        }
    }

    pub fn snapshot(&self) -> JobSnapshot {
        let state = self.state.lock().unwrap();
        match state.job.as_ref() {
            Some(j) => JobSnapshot {
                job_id: j.job_id,
                total: j.total,
                done: j.done,
                failed: j.failed,
                paused: j.paused,
                finished: j.finished,
                running: !j.finished,
            },
            None => JobSnapshot {
                job_id: 0,
                total: 0,
                done: 0,
                failed: 0,
                paused: false,
                finished: true,
                running: false,
            },
        }
    }

    fn finish(&self, _job_id: u64, failed: u64) {
        let mut state = self.state.lock().unwrap();
        if let Some(job) = state.job.as_mut() {
            job.finished = true;
            job.failed = failed;
        }
        let _ = self.sink.emit(PipelineEvent::Finished { failed_count: failed });
    }

    fn run(self: Arc<Self>, files: Vec<PathBuf>, job_id: u64) {
        let started = Instant::now();
        let _ = &started;
        let store = match Store::open(&self.db_path) {
            Ok(s) => s,
            Err(_) => {
                self.finish(job_id, 1);
                return;
            }
        };
        let storage = LocalDiskAdapter::new(self.thumbs_dir.clone());
        let chunk_size = self.config.chunk_size.max(1);
        let mut pause_armed = self.config.pause_after_done > 0;

        for chunk in files.chunks(chunk_size) {
            // ---- 暂停/停止闸口（chunk 之间；30ms 轮询，粒度对 UI 足够）----
            loop {
                let (paused, abort, failed) = {
                    let mut state = self.state.lock().unwrap();
                    let job = state.job.as_mut().expect("job runtime present");
                    (job.paused, job.abort, job.failed)
                };
                if abort {
                    {
                        let mut state = self.state.lock().unwrap();
                        if let Some(job) = state.job.as_mut() {
                            job.finished = true;
                            job.paused = false;
                        }
                    }
                    let _ = self.sink.emit(PipelineEvent::Finished { failed_count: failed });
                    return;
                }
                if !paused {
                    break;
                }
                std::thread::sleep(Duration::from_millis(30));
            }

            // ---- CPU 并行：读文件 → hash → decode → thumb（查库不进并行段）----
            enum Outcome {
                Persist(Prepared),
                Failed(PathBuf, Option<String>, ErrorCode), // sha 在哈希前失败时为 None
            }
            struct Prepared {
                path: PathBuf,
                sha256: String,
                size: u64,
                image: DecodedImage,
                mime: &'static str,
                taken_at: i64,
                exif_json: Option<String>,
                thumb: Vec<u8>,
            }

            let outcomes: Vec<Outcome> = chunk
                .par_iter()
                .map(|path| {
                    let bytes = match std::fs::read(path) {
                        Ok(b) => b,
                        Err(_) => {
                            return Outcome::Failed(path.clone(), None, ErrorCode::ReadFailed)
                        }
                    };
                    let size = bytes.len() as u64;
                    let mut hasher = Sha256::new();
                    hasher.update(&bytes);
                    let sha256 = hex::encode(hasher.finalize());

                    let photo = match decode_photo(path) {
                        Ok(p) => p,
                        Err(code) => {
                            return Outcome::Failed(path.clone(), Some(sha256), code)
                        }
                    };
                    let taken_at =
                        photo.taken_at.unwrap_or_else(|| mtime_secs(path).unwrap_or(0));
                    let (thumb, _, _) = match make_thumbnail(&photo.image) {
                        Ok(t) => t,
                        Err(code) => {
                            return Outcome::Failed(path.clone(), Some(sha256), code)
                        }
                    };
                    Outcome::Persist(Prepared {
                        path: path.clone(),
                        sha256,
                        size,
                        image: photo.image,
                        mime: photo.mime,
                        taken_at,
                        exif_json: photo.exif_json,
                        thumb,
                    })
                })
                .collect();

            // ---- 串行持久化 ----
            for outcome in outcomes {
                let mut state = self.state.lock().unwrap();
                let job = state.job.as_mut().expect("job runtime present");
                match outcome {
                    Outcome::Failed(path, sha256, code) => {
                        job.done += 1;
                        job.failed += 1;
                        mark_item_failed(&store, &path, sha256.as_deref(), code, self.clock.now_unix());
                        let _ = self.sink.emit(PipelineEvent::ItemFailed {
                            path: path.to_string_lossy().into_owned(),
                            code,
                        });
                    }
                    Outcome::Persist(p) => {
                        // 已就绪（同哈希）：幂等跳过，避免重复资产与缩略图
                        match store.exists_ready(&p.sha256) {
                            Ok(true) => {
                                job.done += 1;
                                drop(state);
                                continue;
                            }
                            Ok(false) => {}
                            Err(_) => {
                                job.done += 1;
                                job.failed += 1;
                                drop(state);
                                continue;
                            }
                        }
                        let key = thumb_key(&p.sha256);
                        let new_asset = NewAsset {
                            sha256: p.sha256.clone(),
                            storage_key: p.path.to_string_lossy().into_owned(),
                            kind: mm_core::AssetKind::Photo,
                            size: Some(p.size as i64),
                            taken_at: p.taken_at,
                            imported_at: self.clock.now_unix(),
                        };
                        let result = (|| -> Result<(), ErrorCode> {
                            let out = store.insert_pending(&new_asset).map_err(ErrorCode::from)?;
                            storage.put(&key, &p.thumb)?;
                            store
                                .mark_ready(
                                    out.asset_id,
                                    p.image.width,
                                    p.image.height,
                                    p.taken_at,
                                    p.mime,
                                    p.exif_json.as_deref(),
                                    &key,
                                )
                                .map_err(ErrorCode::from)?;
                            Ok(())
                        })();
                        match result {
                            Ok(_) => job.done += 1,
                            Err(code) => {
                                job.done += 1;
                                job.failed += 1;
                                let _ = self.sink.emit(PipelineEvent::ItemFailed {
                                    path: p.path.to_string_lossy().into_owned(),
                                    code,
                                });
                            }
                        }
                    }
                }
                if self.config.pause_after_done > 0
                    && pause_armed
                    && job.done >= self.config.pause_after_done
                    && !job.paused
                {
                    pause_armed = false; // 只自动暂停一次；之后由用户 resume 正常推进
                    job.paused = true;
                    drop(state);
                    let _ = self.sink.emit(PipelineEvent::Paused);
                    continue;
                }
                let (total, done, failed) = (job.total, job.done, job.failed);
                drop(state);
                let _ = self.sink.emit(PipelineEvent::Progress {
                    total,
                    done,
                    failed,
                });
            }
        }

        let failed = {
            let mut state = self.state.lock().unwrap();
            let job = state.job.as_mut().expect("job runtime present");
            job.finished = true;
            job.failed
        };
        let _ = job_id;
        let _ = self.sink.emit(PipelineEvent::Finished { failed_count: failed });
    }
}

fn mtime_secs(path: &Path) -> Option<i64> {
    std::fs::metadata(path).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

/// 坏文件入库为 failed 行（失败列表可展示/重试）；哈希不可得时用路径派生占位哈希
fn mark_item_failed(
    store: &Store,
    path: &Path,
    sha256: Option<&str>,
    code: ErrorCode,
    imported_at: i64,
) {
    let sha = sha256
        .map(str::to_string)
        .unwrap_or_else(|| hash_file_sha256(format!("failed:{}", path.display()).as_bytes()));
    let new_asset = NewAsset {
        sha256: sha,
        storage_key: path.to_string_lossy().into_owned(),
        kind: mm_core::AssetKind::Photo,
        size: None,
        taken_at: mtime_secs(path).unwrap_or(0),
        imported_at,
    };
    if let Ok(out) = store.insert_pending(&new_asset) {
        let _ = store.mark_failed(out.asset_id, code.slug());
    }
}

/// 递归扫描：跳过隐藏项与不支持扩展名；按路径排序保证确定性
pub fn scan_folder(folder: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(folder).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if is_supported(path) {
            out.push(path.to_path_buf());
        }
    }
    out.sort();
    Ok(out)
}

/// 单文件 SHA-256（十六进制小写）
pub fn hash_file_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// 剩余时间估算（纯函数）：done>0 时按平均速率外推
pub fn eta_seconds(elapsed_secs: u64, done: u64, total: u64) -> Option<u64> {
    if done == 0 || total <= done {
        return None;
    }
    Some(elapsed_secs * (total - done) / done)
}

/// 本地磁盘存储适配器（MVP 唯一 `StorageAdapter` 实现，白皮书 §3.5）
pub use storage::LocalDiskAdapter as _LocalDiskAdapterReexport;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct FakeSink(Arc<Mutex<Vec<PipelineEvent>>>);

    impl EventSink for FakeSink {
        fn emit(&self, event: PipelineEvent) -> Result<(), ErrorCode> {
            self.0.lock().unwrap().push(event);
            Ok(())
        }
    }

    struct FixedClock(i64);
    impl Clock for FixedClock {
        fn now_unix(&self) -> i64 {
            self.0
        }
    }

    fn temp_workspace(tag: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join(format!("src-{tag}"));
        let ws = dir.path().join(format!("ws-{tag}"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&ws).unwrap();
        (dir, src, ws)
    }

    fn write_test_png(path: &Path, color: [u8; 3]) {
        let img = image::RgbImage::from_fn(8, 8, |_, _| image::Rgb(color));
        image::DynamicImage::ImageRgb8(img)
            .save_with_format(path, image::ImageFormat::Png)
            .unwrap();
    }

    fn engine_in(dir_ws: &Path, sink: Arc<dyn EventSink>, config: ImportConfig) -> Arc<ImportEngine> {
        Arc::new(ImportEngine::new(
            dir_ws.join("index.db"),
            dir_ws.join("thumbs"),
            sink,
            Arc::new(FixedClock(1_760_000_000)),
            config,
        ))
    }

    fn wait_finished(engine: &ImportEngine, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if engine.snapshot().finished {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn scan_filters_extensions_and_hidden() {
        let (dir, src, _) = temp_workspace("scan");
        let src = src.clone();
        write_test_png(&src.join("a.png"), [1, 2, 3]);
        std::fs::write(src.join("b.txt"), "not an image").unwrap();
        std::fs::write(src.join(".hidden.jpg"), b"x").unwrap();
        let sub = src.join("sub");
        std::fs::create_dir(&sub).unwrap();
        write_test_png(&sub.join("c.JPEG"), [4, 5, 6]);
        let files = scan_folder(&src).unwrap();
        assert_eq!(files.len(), 2);
        drop(dir);
    }

    #[test]
    fn hash_matches_known_sha256() {
        assert_eq!(
            hash_file_sha256(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn eta_extrapolates_and_guards() {
        // 已用 10s 完成 5 项（2s/项），剩 20 项 → 40s
        assert_eq!(eta_seconds(10, 5, 25), Some(40));
        assert_eq!(eta_seconds(10, 0, 25), None);
        assert_eq!(eta_seconds(10, 25, 25), None);
    }

    #[test]
    fn engine_dedups_and_isolates_bad_files() {
        let (dir, src, ws) = temp_workspace("full");
        write_test_png(&src.join("a.png"), [10, 20, 30]);
        write_test_png(&src.join("dup_of_a.png"), [10, 20, 30]); // 同内容 → 去重
        write_test_png(&src.join("b.png"), [40, 50, 60]);
        std::fs::write(src.join("broken.jpg"), b"definitely not a jpeg").unwrap();

        let events = Arc::new(Mutex::new(Vec::new()));
        let engine = engine_in(&ws, Arc::new(FakeSink(Arc::clone(&events))), ImportConfig::default());
        engine.start(src.clone()).unwrap();
        assert!(wait_finished(&engine, Duration::from_secs(15)));

        let store = Store::open(&ws.join("index.db")).unwrap();
        assert_eq!(store.count().unwrap(), 3); // a、dup_of_a（一条）+ b + broken(failed)
        assert_eq!(store.list_failed().unwrap().len(), 1);
        // 缩略图落盘：2 张 ready
        let thumbs = count_files_recursive(&ws.join("thumbs"));
        assert_eq!(thumbs, 2);

        let finished = events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, PipelineEvent::Finished { failed_count: 1 }));
        assert!(finished, "应有 1 个失败项的完成事件");
        drop(dir);
    }

    #[test]
    fn engine_pauses_and_resumes_midway() {
        let (dir, src, ws) = temp_workspace("pause");
        for i in 0..3u8 {
            write_test_png(&src.join(format!("p{i}.png")), [i, i, i]);
        }
        let events = Arc::new(Mutex::new(Vec::new()));
        let engine = engine_in(
            &ws,
            Arc::new(FakeSink(Arc::clone(&events))),
            ImportConfig { chunk_size: 1, pause_after_done: 1 },
        );
        engine.start(src.clone()).unwrap();

        // 等待自动暂停（done == 1 时触发 Paused 事件）
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let paused = events.lock().unwrap().iter().any(|e| matches!(e, PipelineEvent::Paused));
            if paused && engine.snapshot().paused {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let snap = engine.snapshot();
        assert_eq!(snap.done, 1);
        assert!(snap.paused && !snap.finished);

        engine.resume();
        assert!(wait_finished(&engine, Duration::from_secs(10)));
        let snap = engine.snapshot();
        assert_eq!((snap.done, snap.failed), (3, 0));
        assert!(events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, PipelineEvent::Resumed)));
        drop(dir);
    }

    #[test]
    fn start_rejects_when_busy_and_missing_dir() {
        let (dir, src, ws) = temp_workspace("busy");
        assert_eq!(
            engine_in(&ws, Arc::new(FakeSink(Default::default())), ImportConfig::default())
                .start(src.parent().unwrap().join("nope"))
                .unwrap_err(),
            ErrorCode::ReadFailed
        );
        drop(dir);
    }

    fn count_files_recursive(root: &Path) -> usize {
        walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .count()
    }
}
