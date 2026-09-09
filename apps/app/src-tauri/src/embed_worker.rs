//! 嵌入 worker（ADR-0013 多索引）：对每个已加载索引器独立清"ready 但未嵌入"队列。
//! 解码/推理流水化：解码线程读原图 + 全图解码，按 8 张一组经有界通道交给推理端，
//! 两组 IO/CPU（GPU）重叠——源盘读从"读 3s / 推理静默 8s"的断续突发变为匀速补给，
//! 批次墙钟 ≈ 推理时长。自愈式：读不出的资产跳过不阻塞批次；模型未就绪时空转；
//! 错峰保留：导入进行中不嵌（避免与导入预取在源盘/读路径上互相拖垮，实测 10 倍读放大）；
//! 同内容跨工作区直接复制向量，免二次解码推理。
//! 可观测（2026-09 走查裁决）：每张写完发逐张进度事件（done/total 真实队列口径）；
//! 解码/推理/写库失败全部落日志；同一资产连续解码失败进入会话级冷却，
//! 不再允许恒败项占死队首（ORDER BY asset_id）让后续资产永远轮不上。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use mm_core::{DecodedImage, Indexer};
use mm_pipeline::ImportEngine;
use mm_store::Store;
use tauri_specta::Event;

use crate::events::EmbedProgressEvent;

/// 解码→推理分组（与 mm-embed 内部 IMAGE_BATCH 对齐，一次推理正好一组）
const GROUP_SIZE: usize = 8;

/// 同一资产连续解码失败达到该次数后进入会话级冷却（重启自动重试）
const MAX_DECODE_ATTEMPTS: u32 = 3;

/// 每轮取的队列长度（保持既有节奏：零写入即断路交还外层 1.5s 节拍）
const BATCH_SIZE: usize = 32;

pub struct EmbedWorker;

impl EmbedWorker {
    pub fn spawn(
        db_path: PathBuf,
        indexers: Arc<RwLock<Vec<Arc<dyn Indexer>>>>,
        engine: Arc<ImportEngine>,
        app: tauri::AppHandle,
    ) {
        let attempts: Arc<Mutex<HashMap<i64, u32>>> = Arc::new(Mutex::new(HashMap::new()));
        let cooldown: Arc<Mutex<HashSet<i64>>> = Arc::new(Mutex::new(HashSet::new()));
        let failed_total: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
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
                    embed_queue_for(
                        indexer.as_ref(),
                        &db_path,
                        &app,
                        &attempts,
                        &cooldown,
                        &failed_total,
                    );
                }
            })
            .expect("启动嵌入线程失败");
    }
}

/// 解码者 → 推理者的消息：一张已解码图 + 组间隔完成的向量复制计数
struct DecodedItem {
    asset_id: i64,
    folder_id: i64,
    file_name: String,
    image: DecodedImage,
}

struct DecodedGroup {
    items: Vec<DecodedItem>,
    copied: u32,
    /// 本组解码失败的 (asset_id, 文件名, 错误摘要)——推理端统一记冷却/发事件
    failed: Vec<(i64, String, String)>,
}

/// 单索引的队列清空（每轮取 32 张：解码线程流式产 8 张组 → 推理 → 写向量）
fn embed_queue_for(
    indexer: &dyn Indexer,
    db_path: &Path,
    app: &tauri::AppHandle,
    attempts: &Mutex<HashMap<i64, u32>>,
    cooldown: &Mutex<HashSet<i64>>,
    failed_total: &Mutex<u32>,
) {
    let Ok(store) = Store::open(db_path) else {
        return;
    };
    let index_id = indexer.index_id();
    let mut did_work = false;
    loop {
        let pending = match store.list_ready_without_embedding(index_id, u32::MAX) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(index_id, error = ?e, "嵌入队列查询失败");
                break;
            }
        };
        // 冷却中的恒败项本轮跳过（重启重试）；全量取后再过滤，避免冷却项占死队首
        let pending: Vec<_> = {
            let cd = cooldown.lock().unwrap();
            pending
                .into_iter()
                .filter(|(id, _, _)| !cd.contains(id))
                .collect()
        };
        if pending.is_empty() {
            // 队列清空且本轮处理过内容：发一次校准事件（done==total），进度卡据此收尾
            if did_work {
                let done = store.count_embedded(index_id).unwrap_or(0);
                let _ = EmbedProgressEvent {
                    done: done as i32,
                    total: done as i32,
                    failed: *failed_total.lock().unwrap() as i32,
                    folder_id: -1,
                    file_name: String::new(),
                }
                .emit(app);
            }
            break;
        }
        let pending: Vec<_> = pending.into_iter().take(BATCH_SIZE).collect();
        did_work = true;

        // 批前基线：total = 已嵌 + 剩余未冷却队列（写入/复制使 done 增、pending 减，total 不变）
        let queue_total = {
            let cd = cooldown.lock().unwrap();
            let pending_n = store
                .list_ready_without_embedding(index_id, u32::MAX)
                .map(|v| v.iter().filter(|(id, _, _)| !cd.contains(id)).count())
                .unwrap_or(0);
            store.count_embedded(index_id).unwrap_or(0) as usize + pending_n
        };

        // ---- 解码者线程：读原图 + 全图解码（裁定 23：索引永远解码原图）----
        // 独立 SQLite 连接（rusqlite Connection 非 Sync，一线程一连接是既有惯例）；
        // 打开失败仅失去"同内容免推理"优化，照常解码
        let (tx, rx) = std::sync::mpsc::sync_channel::<DecodedGroup>(1);
        let decode_store = Store::open(db_path).ok();
        let decoder = std::thread::Builder::new()
            .name("mm-embed-decode".into())
            .spawn(move || {
                let mut group: Vec<DecodedItem> = Vec::new();
                let mut copied = 0u32;
                let mut failed: Vec<(i64, String, String)> = Vec::new();
                for (asset_id, _year, sha256) in &pending {
                    // 同内容已在其他工作区建过索引：直接复制向量，免二次解码推理
                    if let Some(s) = &decode_store {
                        if let Ok(Some(src)) =
                            s.find_embedding_source(index_id, sha256, *asset_id)
                        {
                            if matches!(s.copy_embedding(index_id, src, *asset_id), Ok(true)) {
                                copied += 1;
                            }
                            continue;
                        }
                    }
                    let row = decode_store
                        .as_ref()
                        .and_then(|s| s.get_asset(*asset_id).ok().flatten());
                    let Some(row) = row else {
                        failed.push((*asset_id, format!("#{asset_id}"), "资产记录缺失".into()));
                        continue;
                    };
                    let path = PathBuf::from(&row.storage_key);
                    let file_name = path
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_else(|| row.storage_key.clone());
                    let folder_id = row.folder_id;
                    match mm_pipeline::decode::decode_photo(&path) {
                        Ok(photo) => group.push(DecodedItem {
                            asset_id: *asset_id,
                            folder_id,
                            file_name,
                            image: photo.image,
                        }),
                        // 原图不可读：记入失败明细（推理端统一记冷却/发事件），不阻塞其余资产
                        Err(e) => {
                            tracing::warn!(asset_id, path = %row.storage_key, error = ?e, "解码失败");
                            failed.push((*asset_id, file_name, format!("{e:?}")));
                            continue;
                        }
                    }
                    if group.len() >= GROUP_SIZE
                        && tx
                            .send(DecodedGroup {
                                items: std::mem::take(&mut group),
                                copied: std::mem::take(&mut copied),
                                failed: std::mem::take(&mut failed),
                            })
                            .is_err()
                    {
                        return; // 推理端已退出
                    }
                }
                if !group.is_empty() || copied > 0 || !failed.is_empty() {
                    let _ = tx.send(DecodedGroup {
                        items: group,
                        copied,
                        failed,
                    });
                }
                // tx drop：rx 迭代结束
            });
        let Ok(decoder) = decoder else {
            break;
        };

        // ---- 推理端（本线程）：一组一推理 → 逐张写向量 → 逐张进度事件 ----
        let mut written = 0usize;
        for group in rx {
            // 失败明细：记尝试次数（满次进冷却）、累计失败、发事件让前端可见
            for (asset_id, file_name, err) in &group.failed {
                let n = {
                    let mut a = attempts.lock().unwrap();
                    let n = a.entry(*asset_id).or_insert(0);
                    *n += 1;
                    *n
                };
                if n >= MAX_DECODE_ATTEMPTS {
                    cooldown.lock().unwrap().insert(*asset_id);
                    tracing::warn!(asset_id, file = %file_name, attempts = n, last_error = %err, "连续解码失败，本轮会话冷却（重启自动重试）");
                }
                let failed = {
                    let mut f = failed_total.lock().unwrap();
                    *f += 1;
                    *f
                };
                let _ = EmbedProgressEvent {
                    done: store.count_embedded(index_id).unwrap_or(0) as i32,
                    total: queue_total as i32,
                    failed: failed as i32,
                    folder_id: -1,
                    file_name: file_name.clone(),
                }
                .emit(app);
            }
            written += group.copied as usize;
            if group.copied > 0 {
                // 复制的向量不逐张发事件（无解码明细），补一条让 done/total 推进
                let _ = EmbedProgressEvent {
                    done: store.count_embedded(index_id).unwrap_or(0) as i32,
                    total: queue_total as i32,
                    failed: *failed_total.lock().unwrap() as i32,
                    folder_id: -1,
                    file_name: String::new(),
                }
                .emit(app);
            }
            if !group.items.is_empty() {
                let images: Vec<DecodedImage> =
                    group.items.iter().map(|it| it.image.clone()).collect();
                match indexer.embed_images(&images) {
                    Ok(vectors) => {
                        for (item, vec) in group.items.iter().zip(vectors) {
                            match store.insert_embedding(index_id, item.asset_id, &vec) {
                                Ok(()) => {
                                    written += 1;
                                    let _ = EmbedProgressEvent {
                                        done: store.count_embedded(index_id).unwrap_or(0) as i32,
                                        total: queue_total as i32,
                                        failed: *failed_total.lock().unwrap() as i32,
                                        folder_id: i32::try_from(item.folder_id).unwrap_or(-1),
                                        file_name: item.file_name.clone(),
                                    }
                                    .emit(app);
                                }
                                Err(e) => {
                                    tracing::warn!(asset_id = item.asset_id, error = ?e, "向量写入失败");
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(index_id, error = ?e, "批量推理失败，本组跳过");
                    }
                }
            }
        }
        let _ = decoder.join();

        // 毒丸断路：本轮零写入说明队列有"毒丸"（原图缺失等恒败项），再转下去只会
        // 无 sleep 烧核 + 事件洪泛；交还外层 1.5s 节奏自愈重试（原文件恢复后续上）。
        // 恒败项由冷却机制在 MAX_DECODE_ATTEMPTS 轮内自然出队。
        if written == 0 {
            tracing::warn!(index_id, "本轮嵌入零写入，断路等待下一拍（队首可能有恒败项）");
            break;
        }
    }
}
