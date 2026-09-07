//! 嵌入 worker（ADR-0013 多索引）：对每个已加载索引器独立清"ready 但未嵌入"队列，
//! 批量解码原图 → 视觉编码 → 写向量。自愈式：读不出的资产跳过不阻塞批次；
//! 模型未就绪时空转。同内容跨工作区直接复制向量，免二次解码推理。

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use mm_core::{DecodedImage, Indexer};
use mm_store::Store;
use tauri_specta::Event;

use crate::events::EmbedProgressEvent;

pub struct EmbedWorker;

impl EmbedWorker {
    pub fn spawn(
        db_path: PathBuf,
        indexers: Arc<RwLock<Vec<Arc<dyn Indexer>>>>,
        app: tauri::AppHandle,
    ) {
        std::thread::Builder::new()
            .name("mm-embed".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(1500));
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

/// 单索引的队列清空（分批：取 32 张 → 解码 → 8 张/批推理 → 写向量）
fn embed_queue_for(indexer: &dyn Indexer, db_path: &Path, app: &tauri::AppHandle) {
    let Ok(store) = Store::open(db_path) else {
        return;
    };
    loop {
        let pending = match store.list_ready_without_embedding(indexer.index_id(), 32) {
            Ok(p) => p,
            Err(_) => break,
        };
        if pending.is_empty() {
            break;
        }

        let mut decoded: Vec<(i64, DecodedImage)> = Vec::new();
        let mut skipped = 0u32;
        for (asset_id, _year, sha256) in &pending {
            // 同内容已在其他工作区建过索引：直接复制向量，免二次解码推理
            if let Ok(Some(src)) =
                store.find_embedding_source(indexer.index_id(), sha256, *asset_id)
            {
                let _ = store.copy_embedding(indexer.index_id(), src, *asset_id);
                continue;
            }
            let path = store
                .get_asset(*asset_id)
                .ok()
                .flatten()
                .map(|row| PathBuf::from(row.storage_key));
            let Some(path) = path else {
                skipped += 1;
                continue;
            };
            match mm_pipeline::decode::decode_photo(&path) {
                Ok(photo) => decoded.push((*asset_id, photo.image)),
                // 原图不可读：本轮跳过（计入进度），不阻塞其余资产
                Err(_) => skipped += 1,
            }
        }

        if !decoded.is_empty() {
            let images: Vec<DecodedImage> = decoded.iter().map(|(_, img)| img.clone()).collect();
            if let Ok(vectors) = indexer.embed_images(&images) {
                for ((asset_id, _), vec) in decoded.iter().zip(vectors) {
                    let _ = store.insert_embedding(indexer.index_id(), *asset_id, &vec);
                }
            }
        }

        let done = store.count_embedded(indexer.index_id()).unwrap_or(0);
        let _ = EmbedProgressEvent {
            done: done as i32,
            total: ((done + i64::from(skipped)).max(done)) as i32,
        }
        .emit(app);
    }
}
