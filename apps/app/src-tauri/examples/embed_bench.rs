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
    let mut total_decode_ms = 0u64;
    let mut total_infer_ms = 0u64;
    let mut total_imgs = 0u64;
    loop {
        let pending = store
            .list_ready_without_embedding(index_id, 32)
            .expect("查询队列");
        if pending.is_empty() {
            break;
        }
        let t_read = Instant::now();
        let mut decoded: Vec<(i64, mm_core::DecodedImage)> = Vec::new();
        let mut bytes_read = 0u64;
        for (asset_id, _, sha256) in &pending {
            if let Ok(Some(src)) = store.find_embedding_source(index_id, sha256, *asset_id) {
                let _ = store.copy_embedding(index_id, src, *asset_id);
                continue;
            }
            let path = store
                .get_asset(*asset_id)
                .ok()
                .flatten()
                .map(|row| std::path::PathBuf::from(row.storage_key));
            let Some(path) = path else { continue };
            let f0 = Instant::now();
            match mm_pipeline::decode::decode_photo(&path) {
                Ok(photo) => {
                    bytes_read += std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    decoded.push((*asset_id, photo.image));
                    let _ = f0;
                }
                Err(_) => continue,
            }
        }
        let read_ms = t_read.elapsed().as_millis() as u64;
        total_read_bytes += bytes_read;
        total_decode_ms += read_ms;

        let t_infer = Instant::now();
        let mut written = 0usize;
        if !decoded.is_empty() {
            let images: Vec<mm_core::DecodedImage> =
                decoded.iter().map(|(_, img)| img.clone()).collect();
            if let Ok(vectors) = embedder.embed_images(&images) {
                for ((asset_id, _), vec) in decoded.iter().zip(vectors) {
                    if store.insert_embedding(index_id, *asset_id, &vec).is_ok() {
                        written += 1;
                    }
                }
            }
        }
        let infer_ms = t_infer.elapsed().as_millis() as u64;
        total_infer_ms += infer_ms;
        total_imgs += decoded.len() as u64;
        batch_no += 1;
        println!(
            "批 {}: {} 张 | 读取+全图解码 {} ms ({} MB, {} MB/s) | 推理 {} ms",
            batch_no,
            decoded.len(),
            read_ms,
            bytes_read / (1024 * 1024),
            if read_ms > 0 {
                bytes_read / 1024 / read_ms.max(1) * 1000 / 1024
            } else {
                0
            },
            infer_ms
        );
        if written == 0 {
            println!("毒丸断路触发");
            break;
        }
    }
    let wall = t_all.elapsed().as_secs_f64();
    println!(
        "嵌入阶段完成: {} 张 / {} 批, 墙钟 {:.1}s | 读取+解码累计 {:.1}s | 推理累计 {:.1}s | F: 平均读速率 {:.2} MB/s",
        total_imgs,
        batch_no,
        wall,
        total_decode_ms as f64 / 1000.0,
        total_infer_ms as f64 / 1000.0,
        total_read_bytes as f64 / 1024.0 / 1024.0 / wall.max(0.001)
    );
}
