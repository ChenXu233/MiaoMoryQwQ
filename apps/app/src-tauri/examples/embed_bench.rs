//! 嵌入阶段实测（排查"导入后 F: 盘持续低速随机读"）：复刻 embed_worker 的
//! 每批行为（32 张逐张读原图+全图解码 → 8 张/批推理 → 写向量），分段计时。
//! 用法：cargo run -p miaomory-app --example embed_bench -- <源目录> <工作区目录> <模型目录>

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use mm_core::{Clock, ErrorCode, EventSink, PipelineEvent};
use mm_embed::manifest;
use mm_pipeline::{scan_folder, ImportConfig, ImportEngine};
use mm_store::Store;

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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("用法: embed_bench <源目录> <工作区目录> <模型目录>");
        std::process::exit(1);
    }
    let source = Path::new(&args[1]).to_path_buf();
    let ws = Path::new(&args[2]).to_path_buf();
    let model_dir = Path::new(&args[3]).to_path_buf();
    let db_path = ws.join("index.db");
    std::fs::create_dir_all(&ws).expect("创建工作区");

    tracing_subscriber::fmt().with_target(false).init();

    // ---- 准备：无库则先导入（folder_id=1 + CLIP 索引注册）----
    let store = Store::open(&db_path).expect("打开库");
    let (folder_id, _) = store
        .get_or_create_folder(&source.to_string_lossy(), Some("相机"), 1)
        .expect("注册 folder");
    let (index_id, _) = store
        .register_index(
            "chinese-clip-vit-b16-int8",
            "中文 CLIP",
            "chinese-clip",
            512,
            1,
        )
        .expect("注册索引");
    drop(store);

    if scan_folder(&source).unwrap().len() > 0 {
        let engine = Arc::new(ImportEngine::new(
            db_path.clone(),
            ws.join("thumbs"),
            Arc::new(NoopSink),
            Arc::new(SystemClock),
            ImportConfig::default(),
        ));
        let n = engine.snapshot();
        if n.total == 0 {
            engine.start(source.clone(), folder_id).expect("启动导入");
            while !engine.snapshot().finished {
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
            println!("预备导入完成: {:?}", engine.snapshot());
        }
    }

    // ---- 复刻 embed_worker 循环 ----
    let manifest = manifest::manifest();
    let t_load = Instant::now();
    let embedder = mm_embed::ClipEmbedder::load(&model_dir, &manifest, index_id).expect("加载模型");
    println!("模型加载 {:?}", t_load.elapsed());

    let store = Store::open(&db_path).unwrap();
    let t_all = Instant::now();
    let mut batch_no = 0u32;
    let mut total_read_bytes = 0u64;
    let mut total_infer_ms = 0u64;
    let mut total_imgs = 0u64;
    // 流水化（与新 embed_worker 一致）：解码线程产 8 张组 → 有界通道 → 本线程推理
    loop {
        let pending = store
            .list_ready_without_embedding(index_id, 32)
            .expect("查询队列");
        if pending.is_empty() {
            break;
        }
        let (tx, rx) = std::sync::mpsc::sync_channel::<(Vec<(i64, mm_core::DecodedImage)>, u64)>(1);
        let decode_path = db_path.clone();
        let pending_clone = pending.clone();
        let decoder = std::thread::Builder::new()
            .name("bench-decode".into())
            .spawn(move || {
                let dstore = Store::open(&decode_path).unwrap();
                let mut group: Vec<(i64, mm_core::DecodedImage)> = Vec::new();
                let mut bytes_read = 0u64;
                for (asset_id, _, _) in &pending_clone {
                    let path = dstore
                        .get_asset(*asset_id)
                        .ok()
                        .flatten()
                        .map(|row| std::path::PathBuf::from(row.storage_key));
                    let Some(path) = path else { continue };
                    match mm_pipeline::decode::decode_photo(&path) {
                        Ok(photo) => {
                            bytes_read += std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                            group.push((*asset_id, photo.image));
                        }
                        Err(_) => continue,
                    }
                    if group.len() >= 8 {
                        if tx
                            .send((std::mem::take(&mut group), std::mem::take(&mut bytes_read)))
                            .is_err()
                        {
                            return;
                        }
                    }
                }
                if !group.is_empty() {
                    let _ = tx.send((group, bytes_read));
                }
            })
            .unwrap();

        for (group, bytes_read) in rx {
            total_read_bytes += bytes_read;
            total_imgs += group.len() as u64;
            batch_no += 1;
            let t_infer = Instant::now();
            let images: Vec<mm_core::DecodedImage> =
                group.iter().map(|(_, img)| img.clone()).collect();
            let vectors = embedder.embed_images(&images).expect("推理失败");
            for ((asset_id, _), vec) in group.iter().zip(vectors) {
                let _ = store.insert_embedding(index_id, *asset_id, &vec);
            }
            let infer_ms = t_infer.elapsed().as_millis() as u64;
            total_infer_ms += infer_ms;
            println!(
                "组 {}: {} 张 | 读 {} MB（流水化，与推理重叠）| 推理 {} ms",
                batch_no,
                group.len(),
                bytes_read / (1024 * 1024),
                infer_ms
            );
        }
        let _ = decoder.join();
    }
    let wall = t_all.elapsed().as_secs_f64();
    println!(
        "嵌入阶段完成: {} 张 / {} 组, 墙钟 {:.1}s | 推理累计 {:.1}s | F: 平均读速率 {:.2} MB/s（读匀速摊满全程）",
        total_imgs,
        batch_no,
        wall,
        total_infer_ms as f64 / 1000.0,
        total_read_bytes as f64 / 1024.0 / 1024.0 / wall.max(0.001)
    );
}
