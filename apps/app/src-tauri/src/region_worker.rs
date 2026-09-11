//! 区域提取 worker（ADR-0015）：对「ready 但无区域记录」的照片依次做
//! SAM 区域分割 → CLIP 区域编码 → 时空调制在线 DP-means 原型准入。
//!
//! 调度约束：嵌入优先（任何整图索引有待嵌入时让路，避免 IO/CPU 竞争）；
//! 导入进行中让路（与嵌入 worker 同款错峰）。SAM ONNX 模型缺失时整个
//! worker 空转禁用（区域索引是可选能力，缺失不影响主检索）。
//! 无法解码的照片写 sentinel（region_idx=-1）避免队首死循环。

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mm_core::{DecodedImage, Indexer};
use mm_embed::region::{dp_assign, dp_drift, SamSegmenter};
use mm_pipeline::ImportEngine;
use mm_store::Store;
use tauri_specta::Event;

use crate::events::RegionProgressEvent;

/// 区域簇所属的逻辑索引（独立于 index_meta 的整图索引；ADR-0015 §6）
pub const REGION_INDEX_ID: i64 = 2;
/// 篇章切分：与上一张拍摄间隔超过该天数 → 完全新增原型集（所有者 2×2 矩阵）
const CHAPTER_GAP_DAYS: i64 = 30;
/// 连拍阈值：间隔小于该分钟数 → τ 收紧（相同聚类信号）
const BURST_MINUTES: f64 = 5.0;
const TAU_BASE: f32 = 0.24;
/// 每轮处理的资产数（交还外层节拍，进度事件随批刷新）
const BATCH: u32 = 8;

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub struct RegionWorker;

impl RegionWorker {
    pub fn spawn(
        db_path: PathBuf,
        model_dir: PathBuf,
        indexers: Arc<RwLock<Vec<Arc<dyn Indexer>>>>,
        engine: Arc<ImportEngine>,
        app: tauri::AppHandle,
    ) {
        std::thread::Builder::new()
            .name("mm-region".into())
            .spawn(move || {
                let enc_path = model_dir.join("sam").join("sam_encoder.onnx");
                let dec_path = model_dir.join("sam").join("sam_decoder.onnx");
                if !SamSegmenter::available(&enc_path, &dec_path) {
                    tracing::warn!("SAM ONNX 模型缺失，区域索引禁用（model_dir/sam/）");
                    return;
                }
                let mut last_t: Option<i64> = None; // 上一张拍摄时间（时空调制）
                loop {
                    std::thread::sleep(Duration::from_millis(2000));
                    if engine.snapshot().running {
                        continue;
                    }
                    let Ok(store) = Store::open(&db_path) else {
                        continue;
                    };
                    // 嵌入优先：任何整图索引有待嵌入时让路
                    let pending_emb = {
                        let loaded = indexers.read().unwrap();
                        store
                            .list_active_indexes()
                            .map(|idxs| {
                                idxs.iter().any(|i| {
                                    store
                                        .list_ready_without_embedding(i.index_id, 1)
                                        .map(|v| !v.is_empty())
                                        .unwrap_or(false)
                                })
                            })
                            .unwrap_or(true)
                    };
                    if pending_emb {
                        continue;
                    }
                    let clip = {
                        let loaded = indexers.read().unwrap();
                        loaded
                            .iter()
                            .find(|ix| ix.slug() == "chinese-clip-vit-b16-int8")
                            .cloned()
                    };
                    let Some(clip) = clip else {
                        continue; // 语义模型未装配（区域编码依赖其图像塔）
                    };
                    let Ok(mut segmenter) = SamSegmenter::load(&enc_path, &dec_path) else {
                        continue;
                    };
                    let Ok(pending) = store.list_ready_without_regions(BATCH) else {
                        continue;
                    };
                    if pending.is_empty() {
                        continue;
                    }
                    for (asset_id, storage_key, taken_at) in pending {
                        // ---- 时空调制：篇章 + τ 调制（与上一张拍摄间隔）----
                        let dt_min = match (taken_at, last_t) {
                            (Some(t), Some(prev)) => {
                                (t - prev).abs() as f64 / 60.0
                            }
                            _ => f64::INFINITY,
                        };
                        let (chapter_id, chapter_started) =
                            store.latest_region_chapter().unwrap_or((0, 0));
                        let gap_days = match (taken_at, chapter_started) {
                            (Some(t), s) if s > 0 => (t - s).max(0) / 86400,
                            _ => 0,
                        };
                        let chapter_id = if gap_days > CHAPTER_GAP_DAYS {
                            match store.new_region_chapter(taken_at.unwrap_or(now_secs())) {
                                Ok(c) => c,
                                Err(_) => chapter_id,
                            }
                        } else {
                            chapter_id
                        };
                        let tau_eff = if dt_min < BURST_MINUTES {
                            TAU_BASE * 0.5   // 连拍：相同聚类信号
                        } else if dt_min > 7.0 * 24.0 * 60.0 {
                            TAU_BASE * 1.4   // 长时间跨度：更多 K
                        } else {
                            TAU_BASE
                        };

                        // ---- 分割 + 编码 ----
                        let photo = match mm_pipeline::decode::decode_photo(Path::new(&storage_key)) {
                            Ok(p) => p,
                            Err(e) => {
                                tracing::warn!(asset_id, path = %storage_key, error = ?e, "区域提取解码失败，记 sentinel");
                                let _ = store.insert_region(
                                    asset_id, -1, (0, 0, 0, 0), 0.0, chapter_id, -1,
                                    &[0.0; 512],
                                );
                                if taken_at.is_some() {
                                    last_t = taken_at;
                                }
                                continue;
                            }
                        };
                        let boxes = match segmenter
                            .segment(&photo.image.rgb, photo.image.width, photo.image.height)
                        {
                            Ok(b) => b,
                            Err(e) => {
                                tracing::warn!(asset_id, error = ?e, "区域分割失败，记 sentinel");
                                let _ = store.insert_region(
                                    asset_id, -1, (0, 0, 0, 0), 0.0, chapter_id, -1,
                                    &[0.0; 512],
                                );
                                if taken_at.is_some() {
                                    last_t = taken_at;
                                }
                                continue;
                            }
                        };
                        if boxes.is_empty() {
                            let _ = store.insert_region(
                                asset_id, -1, (0, 0, 0, 0), 0.0, chapter_id, -1, &[0.0; 512],
                            );
                            if taken_at.is_some() {
                                last_t = taken_at;
                            }
                            continue;
                        }
                        let crops: Vec<DecodedImage> = boxes
                            .iter()
                            .map(|b| {
                                let x0 = b.x0.max(0) as u32;
                                let y0 = b.y0.max(0) as u32;
                                let x1 = (b.x1 as u32).min(photo.image.width);
                                let y1 = (b.y1 as u32).min(photo.image.height);
                                let bw = x1.saturating_sub(x0) as usize;
                                let bh = (y1 - y0) as usize;
                                let mut rgb = Vec::with_capacity(bw * bh * 3);
                                for yy in y0 as usize..y0 as usize + bh {
                                    let row = yy * photo.image.width as usize * 3;
                                    rgb.extend_from_slice(
                                        &photo.image.rgb[row + x0 as usize * 3
                                            ..row + x1 as usize * 3],
                                    );
                                }
                                DecodedImage { width: bw as u32, height: bh as u32, rgb }
                            })
                            .collect();
                        let Ok(vecs) = clip.embed_images(&crops) else {
                            tracing::warn!(asset_id, "区域 CLIP 编码失败");
                            if taken_at.is_some() {
                                last_t = taken_at;
                            }
                            continue;
                        };

                        // ---- 在线 DP-means 准入（原型从 DB 装载，时空调制 τ）----
                        let mut protos: Vec<(i64, mm_embed::region::DpProto)> = store
                            .list_region_clusters()
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|&(_, ch, _, _)| ch == chapter_id)
                            .map(|(cid, _, vec, count)| (cid, mm_embed::region::DpProto { vec, count }))
                            .collect();
                        let mut next_id = protos.iter().map(|(c, _)| c).max().copied().unwrap_or(0) + 1;
                        for (idx, b) in boxes.iter().enumerate() {
                            let v = &vecs[idx];
                            let proto_refs: Vec<mm_embed::region::DpProto> =
                                protos.iter().map(|(_, p)| p.clone()).collect();
                            let (cid, count) = match dp_assign(v, &proto_refs, tau_eff) {
                                mm_embed::region::DpOutcome::Matched(i) => {
                                    let (cid, proto) = protos[i].clone();
                                    let drifted = dp_drift(&proto.vec, v, proto.count);
                                    let _ =
                                        store.upsert_region_cluster(cid, chapter_id, &drifted, proto.count + 1);
                                    protos[i].1 = mm_embed::region::DpProto {
                                        vec: drifted,
                                        count: proto.count + 1,
                                    };
                                    (cid, proto.count + 1)
                                }
                                mm_embed::region::DpOutcome::New => {
                                    store.upsert_region_cluster(next_id, chapter_id, v, 1);
                                    protos.push((
                                        next_id,
                                        mm_embed::region::DpProto { vec: v.clone(), count: 1 },
                                    ));
                                    let cid = next_id;
                                    next_id += 1;
                                    (cid, 1)
                                }
                            };
                            let _ = store.insert_region(
                                asset_id,
                                idx as i32,
                                (b.x0, b.y0, b.x1, b.y1),
                                b.area_frac,
                                chapter_id,
                                cid,
                                v,
                            );
                        }
                        if taken_at.is_some() {
                            last_t = taken_at;
                        }
                    }
                    let done = store.count_regions().unwrap_or(0);
                    let total = done + store.count_ready_without_regions().unwrap_or(0);
                    let _ = RegionProgressEvent {
                        done: done as i32,
                        total: total as i32,
                    }
                    .emit(&app);
                }
            })
            .expect("启动区域提取线程失败");
    }
}
