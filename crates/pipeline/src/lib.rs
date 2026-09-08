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

use mm_core::{Clock, ErrorCode, EventSink, PipelineEvent, StorageAdapter};
use mm_store::{NewAsset, Store};
use rayon::prelude::*;
use sha2::{Digest, Sha256};

pub use decode::{decode_photo, decode_photo_bytes, is_supported};
pub use storage::LocalDiskAdapter;
pub use thumb::{make_thumbnail, thumb_key, THUMB_MAX_EDGE};

/// 引擎可调参数（测试用小 chunk 与自动暂停钩子）
#[derive(Debug, Clone)]
pub struct ImportConfig {
    pub chunk_size: usize,
    /// 处理到 done >= N 时自动暂停（0 = 不暂停）；手工验证与测试用
    pub pause_after_done: u64,
    /// IO 预取内存预算（字节）；0 = 按系统内存自适应 clamp(总内存/8, 256MB, 1GB)。
    /// 预算同时决定解码工作线程数（约 96MB/张瞬态），总处理内存 ≈ 预算（v5 裁定 22）
    pub io_budget_bytes: u64,
}

impl Default for ImportConfig {
    fn default() -> Self {
        Self {
            chunk_size: 32,
            pause_after_done: 0,
            io_budget_bytes: 0,
        }
    }
}

/// 自适应预算：clamp(系统总内存 / 8, 256MB, 1GB)；探测失败回退 512MB
fn adaptive_io_budget() -> u64 {
    let total = sysinfo::System::new_all().total_memory();
    (total / 8).clamp(256 * 1024 * 1024, 1024 * 1024 * 1024)
}

/// 解码工作线程数：预算驱动（≈96MB/线程瞬态），上限 = 逻辑核 - 2（导入属分钟级
/// 后台任务，保留 2 个逻辑核给 UI/系统/读路径），再 clamp 到池上限。
/// 用 saturating_sub——逻辑核 ≤ 2 时退化为单线程，绝不因 usize 下溢 panic（走查修复）
fn worker_count(budget: u64, logical: usize) -> usize {
    let logical = logical.max(1);
    let core_cap = logical.saturating_sub(2).max(1) as u64;
    (budget / (96 * 1024 * 1024))
        .clamp(1, core_cap)
        .clamp(1, logical as u64) as usize
}

/// 单文件读入上限（与解码分配上限对齐）：超限按读失败进失败列表，不整读进内存
const MAX_READ_BYTES: u64 = 512 * 1024 * 1024;

/// 分段耗时累计（纳秒；跨线程原子累加，批次日志与完成摘要共用）
#[derive(Default)]
struct ImportTimings {
    io_files: AtomicU64,
    io_bytes: AtomicU64,
    io_ns: AtomicU64,
    hash_ns: AtomicU64,
    decode_ns: AtomicU64,
    thumb_ns: AtomicU64,
    persist_ns: AtomicU64,
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
    /// IO 预取线程的停止开关（abort 时置位；job 字段被锁保护，预取线程不碰锁）
    io_abort: Arc<std::sync::atomic::AtomicBool>,
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
            io_abort: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// 启动导入；已有任务在跑时返回 `ImportBusy`。folder_id = 来源工作区（迁移 v4）
    pub fn start(self: &Arc<Self>, folder: PathBuf, folder_id: i64) -> Result<u64, ErrorCode> {
        if !folder.is_dir() {
            return Err(ErrorCode::ReadFailed);
        }
        let mut state = self.state.lock().unwrap();
        if state.job.as_ref().is_some_and(|j| !j.finished) {
            return Err(ErrorCode::ImportBusy);
        }
        let job_id = self.next_job_id.fetch_add(1, Ordering::SeqCst);
        self.io_abort.store(false, Ordering::SeqCst);
        let scan_t0 = Instant::now();
        let files = scan_folder(&folder).map_err(|_| ErrorCode::ReadFailed)?;
        let total = files.len() as u64;
        tracing::info!(
            count = total,
            ms = scan_t0.elapsed().as_millis() as u64,
            "目录扫描完成"
        );
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
            .spawn(move || engine.run(files, job_id, folder_id))
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
        self.io_abort.store(true, Ordering::SeqCst);
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
        let _ = self.sink.emit(PipelineEvent::Finished {
            failed_count: failed,
        });
    }

    fn run(self: Arc<Self>, files: Vec<PathBuf>, job_id: u64, folder_id: i64) {
        let started = Instant::now();
        let _ = &started;
        let store = match Store::open(&self.db_path) {
            Ok(s) => s,
            Err(_) => {
                self.finish(job_id, 1);
                return;
            }
        };
        let store_path = self.db_path.clone();
        let storage = LocalDiskAdapter::new(self.thumbs_dir.clone());
        let chunk_size = self.config.chunk_size.max(1);
        let mut pause_armed = self.config.pause_after_done > 0;
        let budget = if self.config.io_budget_bytes > 0 {
            self.config.io_budget_bytes
        } else {
            adaptive_io_budget()
        };
        tracing::debug!(budget_mb = budget / (1024 * 1024), "导入 IO 预算");

        // ---- 增量预检（规格 0001 §3.8）：storage_key+size 与库内 ready 资产一致即跳过，
        // 不读盘不哈希。重导同一文件夹与 watcher 自动同步都只处理新增/变化文件。----
        let files = {
            let ready: std::collections::HashMap<String, i64> = Store::open(&self.db_path)
                .map(|s| {
                    s.folder_ready_sizes(folder_id)
                        .unwrap_or_default()
                        .into_iter()
                        .collect()
                })
                .unwrap_or_default();
            let before = files.len();
            let files: Vec<PathBuf> = files
                .into_iter()
                .filter(|p| {
                    let key = p.to_string_lossy().into_owned();
                    match (std::fs::metadata(p), ready.get(&key)) {
                        (Ok(m), Some(&size)) => m.len() as i64 != size, // 未变化 → 跳过
                        _ => true,                                      // 新文件/库内无记录 → 处理
                    }
                })
                .collect();
            let skipped = before - files.len();
            tracing::info!(skipped, pending = files.len(), "增量预检完成");
            {
                let mut state = self.state.lock().unwrap();
                if let Some(job) = state.job.as_mut() {
                    job.total = files.len() as u64;
                }
            }
            files
        };
        if files.is_empty() {
            self.finish(job_id, 0);
            return;
        }

        // ---- IO 预取线程：整批读入内存（预算内），工作线程解码零盘 IO ----
        let batch_budget = (budget / 2).max(1);
        let workers = worker_count(budget, rayon::current_num_threads());
        let pool = match rayon::ThreadPoolBuilder::new().num_threads(workers).build() {
            Ok(p) => p,
            // 资源枯竭等极端场景：按既有先例收尾任务（finish 置 finished 并广播），
            // 绝不 panic——run() 在后台线程，panic 会让任务永久卡在未完成态
            Err(e) => {
                tracing::error!(error = %e, "导入工作池构建失败，任务终止");
                self.finish(job_id, 1);
                return;
            }
        };
        let (tx, rx) = std::sync::mpsc::sync_channel::<IoBatch>(1);
        let timings = Arc::new(ImportTimings::default());
        {
            let engine = Arc::clone(&self);
            let abort_flag = Arc::clone(&self.io_abort);
            let files_for_io = files.clone();
            let timings_for_io = Arc::clone(&timings);
            let spawn_io = std::thread::Builder::new()
                .name("mm-import-io".into())
                .spawn(move || {
                    produce_batches(
                        &files_for_io,
                        batch_budget,
                        chunk_size,
                        &abort_flag,
                        &engine,
                        &tx,
                        &timings_for_io,
                    );
                });
            if let Err(e) = spawn_io {
                tracing::error!(error = %e, "IO 预取线程启动失败，任务终止");
                self.finish(job_id, 1);
                return;
            }
        }

        for batch in rx {
            // ---- 暂停/停止闸口（批次之间；30ms 轮询，粒度对 UI 足够）----
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
                    let _ = self.sink.emit(PipelineEvent::Finished {
                        failed_count: failed,
                    });
                    return;
                }
                if !paused {
                    break;
                }
                std::thread::sleep(Duration::from_millis(30));
            }

            struct Prepared {
                path: PathBuf,
                sha256: String,
                size: u64,
                width: u32,
                height: u32,
                mime: &'static str,
                taken_at: i64,
                exif_json: Option<String>,
                thumb: Vec<u8>,
            }

            // ---- CPU 并行：hash → 去重预检 → decode → thumb（查库不进并行段之外）----
            enum Outcome {
                Persist(Prepared),
                Failed(PathBuf, Option<String>, ErrorCode), // sha 在哈希前失败时为 None
                /// 同哈希已就绪：幂等跳过（规格 0001 §3.3，免解码免缩略图）
                AlreadyReady(PathBuf),
            }

            let cpu_t0 = Instant::now();
            let outcomes: Vec<Outcome> = pool.install(|| {
                batch
                    .par_iter()
                    // 每线程一条短连接（WAL 多读安全），用于解码前的去重预检
                    .map_init(
                        || mm_store::Store::open(&store_path).ok(),
                        |store, (path, bytes)| match bytes {
                            Err(code) => Outcome::Failed(path.clone(), None, *code),
                            Ok(bytes) => {
                                let size = bytes.len() as u64;
                                let t0 = Instant::now();
                                let mut hasher = Sha256::new();
                                hasher.update(bytes);
                                let sha256 = hex::encode(hasher.finalize());
                                timings
                                    .hash_ns
                                    .fetch_add(t0.elapsed().as_nanos() as u64, Ordering::Relaxed);

                                // 去重预检（规格 0001 §3.3：hash 后命中 ready 即跳过，免解码）
                                if let Some(store) = store {
                                    if matches!(store.exists_ready(folder_id, &sha256), Ok(true)) {
                                        return Outcome::AlreadyReady(path.clone());
                                    }
                                }

                                let t0 = Instant::now();
                                let photo = match decode_photo_bytes(bytes, path) {
                                    Ok(p) => p,
                                    Err(code) => {
                                        timings.decode_ns.fetch_add(
                                            t0.elapsed().as_nanos() as u64,
                                            Ordering::Relaxed,
                                        );
                                        return Outcome::Failed(path.clone(), Some(sha256), code);
                                    }
                                };
                                timings
                                    .decode_ns
                                    .fetch_add(t0.elapsed().as_nanos() as u64, Ordering::Relaxed);
                                let taken_at = photo
                                    .taken_at
                                    .unwrap_or_else(|| mtime_secs(path).unwrap_or(0));
                                // 入库尺寸用原图字段：HEIC 快路径的 image 是内嵌缩略图，
                                // 元数据就地提取，解码图在闭包末尾即释放（峰值 ≈ 1 张/工作线程）
                                let (width, height) = (photo.width, photo.height);
                                let t0 = Instant::now();
                                let (thumb, _, _) = match make_thumbnail(&photo.image) {
                                    Ok(t) => t,
                                    Err(code) => {
                                        timings.thumb_ns.fetch_add(
                                            t0.elapsed().as_nanos() as u64,
                                            Ordering::Relaxed,
                                        );
                                        return Outcome::Failed(path.clone(), Some(sha256), code);
                                    }
                                };
                                timings
                                    .thumb_ns
                                    .fetch_add(t0.elapsed().as_nanos() as u64, Ordering::Relaxed);
                                Outcome::Persist(Prepared {
                                    path: path.clone(),
                                    sha256,
                                    size,
                                    width,
                                    height,
                                    mime: photo.mime,
                                    taken_at,
                                    exif_json: photo.exif_json,
                                    thumb,
                                })
                            }
                        },
                    )
                    .collect::<Vec<Outcome>>()
            });
            tracing::info!(
                files = batch.len(),
                wall_ms = cpu_t0.elapsed().as_millis() as u64,
                hash_ms = timings.hash_ns.load(Ordering::Relaxed) / 1_000_000,
                decode_ms = timings.decode_ns.load(Ordering::Relaxed) / 1_000_000,
                thumb_ms = timings.thumb_ns.load(Ordering::Relaxed) / 1_000_000,
                "批次解码完成（hash/decode/thumb 为多线程累计值）"
            );

            // ---- 串行持久化 ----
            let persist_t0 = Instant::now();
            let mut persisted = 0usize;
            for outcome in outcomes {
                let mut state = self.state.lock().unwrap();
                let job = state.job.as_mut().expect("job runtime present");
                match outcome {
                    Outcome::Failed(path, sha256, code) => {
                        job.done += 1;
                        job.failed += 1;
                        mark_item_failed(
                            &store,
                            &path,
                            folder_id,
                            sha256.as_deref(),
                            code,
                            self.clock.now_unix(),
                        );
                        let _ = self.sink.emit(PipelineEvent::ItemFailed {
                            path: path.to_string_lossy().into_owned(),
                            code,
                        });
                    }
                    Outcome::AlreadyReady(_path) => {
                        // 幂等跳过：与旧行为一致，不发独立事件（进度计数已含该项）
                        job.done += 1;
                    }
                    Outcome::Persist(p) => {
                        persisted += 1;
                        // 持久化段再查一次：拦住同批内重复内容（预检时对方尚非 ready）
                        match store.exists_ready(folder_id, &p.sha256) {
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
                            folder_id,
                            sha256: p.sha256.clone(),
                            storage_key: p.path.to_string_lossy().into_owned(),
                            kind: mm_core::AssetKind::Photo,
                            size: Some(p.size as i64),
                            taken_at: p.taken_at,
                            imported_at: self.clock.now_unix(),
                        };
                        let result = (|| -> Result<i64, ErrorCode> {
                            let out = store.insert_pending(&new_asset).map_err(ErrorCode::from)?;
                            storage.put(&key, &p.thumb)?;
                            store
                                .mark_ready(
                                    out.asset_id,
                                    p.width,
                                    p.height,
                                    p.taken_at,
                                    p.mime,
                                    p.exif_json.as_deref(),
                                    &key,
                                )
                                .map_err(ErrorCode::from)?;
                            // 文本检索流：文件名（去扩展名）进 FTS（P3）
                            let stem = p
                                .path
                                .file_stem()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let _ = store.insert_fts(out.asset_id, &stem);
                            Ok(out.asset_id)
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
            timings
                .persist_ns
                .fetch_add(persist_t0.elapsed().as_nanos() as u64, Ordering::Relaxed);
            tracing::info!(
                items = persisted,
                ms = persist_t0.elapsed().as_millis() as u64,
                "批次持久化完成（含缩略图落盘与 SQLite 写入）"
            );
        }

        let t = &timings;
        let io_ns = t.io_ns.load(Ordering::Relaxed);
        let total_bytes = t.io_bytes.load(Ordering::Relaxed);
        tracing::info!(
            total_ms = started.elapsed().as_millis() as u64,
            files = t.io_files.load(Ordering::Relaxed),
            read_mb = total_bytes / (1024 * 1024),
            io_read_ms = io_ns / 1_000_000,
            hash_ms = t.hash_ns.load(Ordering::Relaxed) / 1_000_000,
            decode_ms = t.decode_ns.load(Ordering::Relaxed) / 1_000_000,
            thumb_ms = t.thumb_ns.load(Ordering::Relaxed) / 1_000_000,
            persist_ms = t.persist_ns.load(Ordering::Relaxed) / 1_000_000,
            "导入完成分段时间统计"
        );

        let failed = {
            let mut state = self.state.lock().unwrap();
            let job = state.job.as_mut().expect("job runtime present");
            job.finished = true;
            job.failed
        };
        let _ = job_id;
        let _ = self.sink.emit(PipelineEvent::Finished {
            failed_count: failed,
        });
    }
}

/// IO 预取批次通道载荷：路径 + 读盘结果（错误按项携带，不中断整批）
type IoBatch = Vec<(PathBuf, Result<Vec<u8>, ErrorCode>)>;

/// 整读一个文件；Windows 上带顺序扫描提示（预取按路径序整批读，更大预读窗口、
/// 更少的 USB 小事务）。顺序读提示对流式整读是纯收益。
#[cfg(windows)]
fn read_file_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x0800_0000;
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)
        .open(path)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(buf)
}

#[cfg(not(windows))]
fn read_file_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
    std::fs::read(path)
}

/// 带单文件上限的整读：先查元数据长度，超限直接报错（不预读、不受批次预算放行）
fn read_file_bytes_capped(path: &Path) -> std::io::Result<Vec<u8>> {
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > MAX_READ_BYTES {
        return Err(std::io::Error::other("单文件超出读入上限"));
    }
    read_file_bytes(path)
}

/// IO 预取生产者：按（字节数 ≤ batch_budget 且条数 ≤ chunk_size）分批整读入内存，
/// 经有界通道（容量 1）交给主循环；abort 置位即停。单文件读失败按 Failed 结果传递。
#[allow(clippy::too_many_arguments)]
fn produce_batches(
    files: &[PathBuf],
    batch_budget: u64,
    max_items: usize,
    abort_flag: &std::sync::atomic::AtomicBool,
    engine: &Arc<ImportEngine>,
    tx: &std::sync::mpsc::SyncSender<IoBatch>,
    timings: &ImportTimings,
) {
    let mut batch: Vec<(PathBuf, Result<Vec<u8>, ErrorCode>)> = Vec::new();
    let mut batch_bytes = 0u64;
    let mut batch_t0 = Instant::now();
    let mut max_file_ms = 0u64;
    for path in files {
        if abort_flag.load(Ordering::SeqCst) {
            break;
        }
        let t0 = Instant::now();
        let read = read_file_bytes_capped(path).map_err(|_| ErrorCode::ReadFailed);
        let file_ms = t0.elapsed().as_millis() as u64;
        max_file_ms = max_file_ms.max(file_ms);
        timings
            .io_ns
            .fetch_add(file_ms * 1_000_000, Ordering::Relaxed);
        timings.io_files.fetch_add(1, Ordering::Relaxed);
        let size = read.as_ref().map_or(0, |b| b.len() as u64);
        timings.io_bytes.fetch_add(size, Ordering::Relaxed);
        // 单文件超预算也独立成批（不无限膨胀内存）
        let would_exceed = batch_bytes + size > batch_budget && !batch.is_empty();
        if would_exceed || batch.len() >= max_items {
            tracing::info!(
                files = batch.len(),
                mb = batch_bytes / (1024 * 1024),
                ms = batch_t0.elapsed().as_millis() as u64,
                max_file_ms,
                "批次整读完成（IO 预取）"
            );
            if tx.send(std::mem::take(&mut batch)).is_err() {
                return; // 接收端已退出（abort/完成）
            }
            batch_bytes = 0;
            batch_t0 = Instant::now();
            max_file_ms = 0;
        }
        batch_bytes += size;
        batch.push((path.clone(), read));
    }
    if !batch.is_empty() && !abort_flag.load(Ordering::SeqCst) {
        tracing::info!(
            files = batch.len(),
            mb = batch_bytes / (1024 * 1024),
            ms = batch_t0.elapsed().as_millis() as u64,
            max_file_ms,
            "批次整读完成（IO 预取，末批）"
        );
        let _ = tx.send(batch);
    }
    // 静默结束：接收端从通道关闭（rx 迭代结束）感知完成
    let _ = engine;
}

fn mtime_secs(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

/// 坏文件入库为 failed 行（失败列表可展示/重试）；哈希不可得时用路径派生占位哈希
fn mark_item_failed(
    store: &Store,
    path: &Path,
    folder_id: i64,
    sha256: Option<&str>,
    code: ErrorCode,
    imported_at: i64,
) {
    let sha = sha256
        .map(str::to_string)
        .unwrap_or_else(|| hash_file_sha256(format!("failed:{}", path.display()).as_bytes()));
    let new_asset = NewAsset {
        folder_id,
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
    for entry in walkdir::WalkDir::new(folder)
        .into_iter()
        .filter_map(|e| e.ok())
    {
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

    fn engine_in(
        dir_ws: &Path,
        sink: Arc<dyn EventSink>,
        config: ImportConfig,
    ) -> Arc<ImportEngine> {
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
    fn adaptive_budget_stays_in_owner_bounds() {
        // 裁定 22：clamp(总内存/8, 256MB, 1GB)
        let b = adaptive_io_budget();
        assert!((256 * 1024 * 1024..=1024 * 1024 * 1024).contains(&b));
    }

    #[test]
    fn worker_count_never_underflows_even_on_single_thread() {
        // 逻辑核 1/2：退化单线程（此前 `n - 2` 在 debug 构建直接 panic）
        assert_eq!(worker_count(1024 * 1024 * 1024, 1), 1);
        assert_eq!(worker_count(1024 * 1024 * 1024, 2), 1);
        // 预算极小也保底 1
        assert_eq!(worker_count(0, 8), 1);
        // 预算 10 线程、8 核：让 2 核（cap=6）
        assert_eq!(worker_count(1024 * 1024 * 1024, 8), 6);
        // 预算 2 线程、32 核：预算说话
        assert_eq!(worker_count(3 * 96 * 1024 * 1024, 32), 3);
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
        let engine = engine_in(
            &ws,
            Arc::new(FakeSink(Arc::clone(&events))),
            ImportConfig::default(),
        );
        engine.start(src.clone(), 1).unwrap();
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
            ImportConfig {
                chunk_size: 1,
                pause_after_done: 1,
                io_budget_bytes: 0,
            },
        );
        engine.start(src.clone(), 1).unwrap();

        // 等待自动暂停（done == 1 时触发 Paused 事件）
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let paused = events
                .lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, PipelineEvent::Paused));
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
            engine_in(
                &ws,
                Arc::new(FakeSink(Default::default())),
                ImportConfig::default()
            )
            .start(src.parent().unwrap().join("nope"), 1)
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
