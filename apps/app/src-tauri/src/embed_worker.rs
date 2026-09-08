//! 嵌入 worker（ADR-0013 多索引）：对每个已加载索引器独立清"ready 但未嵌入"队列。
//! 解码/推理流水化：解码线程读原图 + 全图解码，按 8 张一组经有界通道交给推理端，
//! 两组 IO/CPU（GPU）重叠——源盘读从"读 3s / 推理静默 8s"的断续突发变为匀速补给，
//! 批次墙钟 ≈ 推理时长。自愈式：读不出的资产跳过不阻塞批次；模型未就绪时空转；
//! 错峰保留：导入进行中不嵌（避免与导入预取在源盘/读路径上互相拖垮，实测 10 倍读放大）；
//! 同内容跨工作区直接复制向量，免二次解码推理。

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use mm_core::{DecodedImage, Indexer};
use mm_pipeline::ImportEngine;
use mm_store::Store;
use tauri_specta::Event;

use crate::events::EmbedProgressEvent;

/// 解码→推理分组（与 mm-embed 内部 IMAGE_BATCH 对齐，一次推理正好一组）
const GROUP_SIZE: usize = 8;

pub struct EmbedWorker;

impl EmbedWorker {
    pub fn spawn(
        db_path: PathBuf,
        indexers: Arc<RwLock<Vec<Arc<dyn Indexer>>>>,
        engine: Arc<ImportEngine>,
        app: tauri::AppHandle,
    ) {
        std::thread::Builder::new()
            .name("mm-embed".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(1500));
                // 错峰：导入进行中不嵌（避免与导入预取在源盘/读路径上互相拖垮，实测 10 倍读放大）
                if engine.snapshot().running {
                    continue;
                }
                // 每个已加载索引独立清自己的队列（ADR-0013）
                let loaded: Vec<Arc<dyn Indexer>> = indexers.read().unwrap().clone();
                if loaded.is_empty() {
                    continue; // 模型未就绪：等待下载/加载
                }
                for indexer in &loaded {
                    embed_queue_for(indexer.as_ref(), &db_path, &app);
                }
            })
            .expect("启动嵌入线程失败");
    }
}

/// 解码者 → 推理者的消息：一组已解码图 + 组间隔完成的向量复制/跳过计数（增量）
struct DecodedGroup {
    items: Vec<(i64, DecodedImage)>,
    copied: u32,
    skipped: u32,
}

/// 单索引的队列清空（每轮取 32 张：解码线程流式产 8 张组 → 推理 → 写向量）
fn embed_queue_for(indexer: &dyn Indexer, db_path: &Path, app: &tauri::AppHandle) {
    let Ok(store) = Store::open(db_path) else {
        return;
    };
    let index_id = indexer.index_id();
    loop {
        let pending = match store.list_ready_without_embedding(index_id, 32) {
            Ok(p) => p,
            Err(_) => break,
        };
        if pending.is_empty() {
            break;
        }

        // ---- 解码者线程：读原图 + 全图解码（裁定 23：索引永远解码原图）----
        // 独立 SQLite 连接（rusqlite Connection 非 Sync，一线程一连接是既有惯例）；
        // 打开失败仅失去"同内容免推理"优化，照常解码
        let (tx, rx) = std::sync::mpsc::sync_channel::<DecodedGroup>(1);
        let decode_store = Store::open(db_path).ok();
        let decoder = std::thread::Builder::new()
            .name("mm-embed-decode".into())
            .spawn(move || {
                let mut group: Vec<(i64, DecodedImage)> = Vec::new();
                let mut copied = 0u32;
                let mut skipped = 0u32;
                for (asset_id, _year, sha256) in &pending {
                    // 同内容已在其他工作区建过索引：直接复制向量，免二次解码推理
                    if let Some(s) = &decode_store {
                        if let Ok(Some(src)) = s.find_embedding_source(index_id, sha256, *asset_id)
                        {
                            if matches!(s.copy_embedding(index_id, src, *asset_id), Ok(true)) {
                                copied += 1;
                            }
                            continue;
                        }
                    }
                    let path = decode_store
                        .as_ref()
                        .and_then(|s| s.get_asset(*asset_id).ok().flatten())
                        .map(|row| PathBuf::from(row.storage_key));
                    let Some(path) = path else {
                        skipped += 1;
                        continue;
                    };
                    match mm_pipeline::decode::decode_photo(&path) {
                        Ok(photo) => group.push((*asset_id, photo.image)),
                        // 原图不可读：跳过（计入进度），不阻塞其余资产
                        Err(_) => {
                            skipped += 1;
                            continue;
                        }
                    }
                    if group.len() >= GROUP_SIZE {
                        if tx
                            .send(DecodedGroup {
                                items: std::mem::take(&mut group),
                                copied: std::mem::take(&mut copied),
                                skipped: std::mem::take(&mut skipped),
                            })
                            .is_err()
                        {
                            return; // 推理端已退出
                        }
                    }
                }
                if !group.is_empty() || copied > 0 || skipped > 0 {
                    let _ = tx.send(DecodedGroup {
                        items: group,
                        copied,
                        skipped,
                    });
                }
                // tx drop：rx 迭代结束
            });
        let Ok(decoder) = decoder else {
            break;
        };

        // ---- 推理端（本线程）：一组一推理 → 写向量 → 进度事件 ----
        let mut written = 0usize;
        let mut skipped_total = 0u32;
        for group in rx {
            skipped_total += group.skipped;
            written += group.copied as usize;
            if !group.items.is_empty() {
                let images: Vec<DecodedImage> =
                    group.items.iter().map(|(_, img)| img.clone()).collect();
                if let Ok(vectors) = indexer.embed_images(&images) {
                    for ((asset_id, _), vec) in group.items.iter().zip(vectors) {
                        if store.insert_embedding(index_id, *asset_id, &vec).is_ok() {
                            written += 1;
                        }
                    }
                }
            }
            let done = store.count_embedded(index_id).unwrap_or(0);
            let _ = EmbedProgressEvent {
                done: done as i32,
                total: ((done + i64::from(skipped_total)).max(done)) as i32,
            }
            .emit(app);
        }
        let _ = decoder.join();

        // 毒丸断路：本轮零写入说明队列有"毒丸"（原图缺失等恒败项），再转下去只会
        // 无 sleep 烧核 + 事件洪泛；交还外层 1.5s 节奏自愈重试（原文件恢复后续上）
        if written == 0 {
            break;
        }
    }
}
