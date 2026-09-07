//! 嵌入 worker：轮询"ready 但未嵌入"的资产，批量解码 → 视觉编码 → 写向量。
//! 独立线程自愈式设计：读不出的资产留在队列外不影响其余批次；模型未就绪时空转。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use mm_core::{DecodedImage, Embedder};
use mm_store::Store;
use tauri_specta::Event;

use crate::events::EmbedProgressEvent;

pub struct EmbedWorker;

impl EmbedWorker {
    pub fn spawn(
        db_path: PathBuf,
        embedder: Arc<std::sync::OnceLock<Arc<mm_embed::ClipEmbedder>>>,
        app: tauri::AppHandle,
    ) {
        std::thread::Builder::new()
            .name("mm-embed".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(1500));
                let Some(embedder) = embedder.get() else {
                    continue; // 模型未就绪：等待下载/加载
                };
                let Ok(store) = Store::open(&db_path) else {
                    continue;
                };
                // 单轮：分批处理直到队列清空
                loop {
                    let pending = match store.list_ready_without_embedding(32) {
                        Ok(p) => p,
                        Err(_) => break,
                    };
                    if pending.is_empty() {
                        break;
                    }

                    let mut decoded: Vec<(i64, String, DecodedImage)> = Vec::new();
                    let mut skipped = 0u32;
                    for (asset_id, year, sha256) in &pending {
                        // 同内容已在其他工作区建过索引：直接复制向量，免二次解码推理
                        if let Ok(Some(src)) = store.find_embedding_source(sha256, *asset_id) {
                            let _ = store.copy_embedding(src, *asset_id);
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
                            Ok(photo) => decoded.push((*asset_id, year.clone(), photo.image)),
                            // 原图不可读：本轮跳过（计入进度），不阻塞其余资产
                            Err(_) => skipped += 1,
                        }
                    }

                    if !decoded.is_empty() {
                        let images: Vec<DecodedImage> =
                            decoded.iter().map(|(_, _, img)| img.clone()).collect();
                        if let Ok(vectors) = Embedder::embed_images(embedder.as_ref(), &images) {
                            for ((asset_id, _year, _), vec) in decoded.iter().zip(vectors) {
                                let _ = store.insert_embedding(*asset_id, &vec);
                            }
                        }
                    }

                    let done = store.count_embedded().unwrap_or(0);
                    let _ = EmbedProgressEvent {
                        done: done as i32,
                        total: ((done + i64::from(skipped)).max(done)) as i32,
                    }
                    .emit(&app);
                }
            })
            .expect("启动嵌入线程失败");
    }
}
