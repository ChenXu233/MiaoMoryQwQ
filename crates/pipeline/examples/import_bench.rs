//! 导入 IO 实测工具（排查 F: 盘导入慢）：
//! - bare   模式：按 scan_folder 顺序裸读 N 个文件，输出每文件耗时分布（对照基准）
//! - import 模式：真实 ImportEngine 导入到指定工作区（建议放 NVMe），分段日志
//!
//! 用法：cargo run -p mm-pipeline --example import_bench -- <mode> <source> <workspace> [max_files]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use mm_core::{Clock, ErrorCode, EventSink, PipelineEvent};
use mm_pipeline::{scan_folder, ImportConfig, ImportEngine};

struct NoopSink;
impl EventSink for NoopSink {
    fn emit(&self, _e: PipelineEvent) -> Result<(), ErrorCode> {
        Ok(())
    }
}

struct SystemClock;
impl Clock for SystemClock {
    fn now_unix(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

/// 与 pipeline 内部实现一致（SEQUENTIAL_SCAN + 整读）的裸读
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

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = (((sorted.len() as f64) - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("用法: import_bench <bare|import> <source> <workspace> [max_files]");
        std::process::exit(1);
    }
    let mode = args[1].as_str();
    let source = PathBuf::from(&args[2]);
    let workspace = PathBuf::from(&args[3]);
    let max_files: Option<usize> = args.get(4).and_then(|s| s.parse().ok());

    tracing_subscriber::fmt().with_target(false).init();

    let scan_t0 = Instant::now();
    let mut files = scan_folder(&source).expect("scan");
    println!("scan: {} 个文件, 耗时 {:?}", files.len(), scan_t0.elapsed());
    if let Some(n) = max_files {
        files.truncate(n);
        println!("截取前 {} 个文件实测", files.len());
    }

    match mode {
        "bare" => {
            let mut per_file_ms: Vec<u64> = Vec::new();
            let mut total_bytes = 0u64;
            let t0 = Instant::now();
            for (i, p) in files.iter().enumerate() {
                let f0 = Instant::now();
                match read_file_bytes(p) {
                    Ok(b) => {
                        let ms = f0.elapsed().as_millis() as u64;
                        per_file_ms.push(ms);
                        total_bytes += b.len() as u64;
                        if ms > 100 {
                            println!(
                                "[{}/{}] 慢读 {} ms: {} ({} MB)",
                                i + 1,
                                files.len(),
                                ms,
                                p.display(),
                                b.len() / (1024 * 1024)
                            );
                        }
                    }
                    Err(e) => println!("[{}/{}] 读失败 {}: {}", i + 1, files.len(), p.display(), e),
                }
            }
            let wall = t0.elapsed();
            per_file_ms.sort();
            let n = per_file_ms.len();
            println!(
                "bare 读完成: {} 文件 {} MB, 墙钟 {:?}, 吞吐 {:.1} MB/s",
                n,
                total_bytes / (1024 * 1024),
                wall,
                total_bytes as f64 / 1024.0 / 1024.0 / wall.as_secs_f64().max(1e-9)
            );
            if n > 0 {
                println!(
                    "每文件 ms: avg={} p50={} p95={} max={}",
                    per_file_ms.iter().sum::<u64>() / n as u64,
                    percentile(&per_file_ms, 0.50),
                    percentile(&per_file_ms, 0.95),
                    per_file_ms[n - 1]
                );
            }
        }
        "import" => {
            let ws = workspace.join("ws");
            std::fs::create_dir_all(&ws).expect("创建工作区");
            let engine = Arc::new(ImportEngine::new(
                ws.join("index.db"),
                ws.join("thumbs"),
                Arc::new(NoopSink),
                Arc::new(SystemClock),
                ImportConfig::default(),
            ));
            let t0 = Instant::now();
            let job = engine.start(source.clone(), 1).expect("启动导入");
            // 预检在 start 后异步进行：轮询直到 total 不再是 0（scan 已知数量）
            let mut last = engine.snapshot();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(200));
                let s = engine.snapshot();
                if s.total != last.total || s.done != last.done {
                    println!(
                        "进度: done {}/{} failed {} (累计 {:?})",
                        s.done,
                        s.total,
                        s.failed,
                        t0.elapsed()
                    );
                }
                last = s;
                if s.finished {
                    break;
                }
            }
            let s = engine.snapshot();
            println!(
                "导入结束: done {} failed {} 总耗时 {:?}",
                s.done,
                s.failed,
                t0.elapsed()
            );
            let _ = job;
        }
        other => {
            eprintln!("未知模式: {other}");
            std::process::exit(1);
        }
    }
}
