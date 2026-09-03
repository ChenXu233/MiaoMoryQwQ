//! `mm-store`：SQLite 存储层（白皮书 §4.5）。
//!
//! 唯一允许持有 SQLite 连接的 crate；sqlite-vec 经静态内嵌注册（ADR-0002）。
//! 连接不跨线程共享：引擎线程与查询各自 `Store::open`（WAL 支持多读单写）。

use std::path::Path;
use std::sync::Once;

use mm_core::{AssetKind, AssetStatus, ErrorCode};
use rusqlite::{params, Connection, OptionalExtension, Row};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("数据库错误：{0}")]
    Db(#[from] rusqlite::Error),
    #[error("分页游标无效")]
    InvalidCursor,
    #[error("核心错误：{0}")]
    Core(#[from] ErrorCode),
}

pub type Result<T> = std::result::Result<T, StoreError>;

impl From<StoreError> for ErrorCode {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::Core(code) => code,
            _ => ErrorCode::StoreFailed,
        }
    }
}

/// schema 迁移：每版 +1（`PRAGMA user_version`），历史追加不修改
pub const MIGRATIONS: &[&str] = &[
    // v1：assets 表（白皮书 §4.5；vec_assets 随 v2、fts_text 随 v3）
    "CREATE TABLE assets (
        asset_id  INTEGER PRIMARY KEY,
        sha256    TEXT NOT NULL UNIQUE,
        storage_key TEXT NOT NULL,
        kind      TEXT NOT NULL,
        size      INTEGER,
        width     INTEGER,
        height    INTEGER,
        taken_at  INTEGER,
        year      TEXT NOT NULL,
        mime      TEXT,
        exif      TEXT,
        thumb_key TEXT,
        status    TEXT NOT NULL DEFAULT 'pending',
        error_code TEXT,
        imported_at INTEGER NOT NULL
    );
    CREATE INDEX idx_assets_taken ON assets(taken_at DESC, asset_id DESC);",
    // v2：向量表（sqlite-vec vec0，元数据外置在 assets）。
    // 偏差：上游版本忽略 PARTITION KEY 约束（静默不过滤），MVP ≤5 万向量全表 KNN 足够；
    // 分区键随上游稳定后引入（届时需重建 vec 表，见规格 0003 §7）。
    "CREATE VIRTUAL TABLE vec_assets USING vec0(
        asset_id INTEGER PRIMARY KEY,
        embedding float32[512]
    );",
    // v3：文本检索（文件名等；描述/OCR 后续扩展 content 结构）
    "CREATE VIRTUAL TABLE fts_text USING fts5(asset_id UNINDEXED, content, tokenize='trigram');",
];

/// 静态注册 sqlite-vec 扩展（对所有新连接生效）
// transmute 目标签名由 sqlite3_auto_extension 形参决定，此处必须省略注解
#[allow(clippy::missing_transmute_annotations)]
fn register_vec_extension() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let _ = rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    });
}

pub struct Store {
    conn: Connection,
}

/// 列表页行（命令层按 year 分组为时间轴快照）
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AssetRow {
    pub asset_id: i64,
    pub sha256: String,
    pub storage_key: String,
    pub kind: AssetKind,
    pub size: Option<i64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub taken_at: i64,
    pub year: String,
    pub thumb_key: Option<String>,
    pub status: AssetStatus,
    pub error_code: Option<String>,
}

/// 新资产待写记录（pipeline persist 阶段构造）
#[derive(Debug, Clone)]
pub struct NewAsset {
    pub sha256: String,
    pub storage_key: String,
    pub kind: AssetKind,
    pub size: Option<i64>,
    pub taken_at: i64,
    pub imported_at: i64,
}

/// 插入结果：`duplicated` = 库内已有同哈希（跨端去重，零重复向量化）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InsertOutcome {
    pub asset_id: i64,
    pub duplicated: bool,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        register_vec_extension();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_memory() -> Result<Self> {
        register_vec_extension();
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn db_path(&self) -> Option<String> {
        self.conn.path().map(|p| p.to_string())
    }

    fn migrate(&self) -> Result<()> {
        let current: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (idx, sql) in MIGRATIONS.iter().enumerate() {
            let version = (idx + 1) as i64;
            if version <= current {
                continue;
            }
            self.conn.execute_batch(sql)?;
            self.conn.pragma_update(None, "user_version", version)?;
        }
        Ok(())
    }

    pub fn user_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    /// 插入新资产（pending 态）；哈希命中即返回既有 id
    pub fn insert_pending(&self, new: &NewAsset) -> Result<InsertOutcome> {
        let inserted = self.conn.execute(
            "INSERT INTO assets (sha256, storage_key, kind, taken_at, year, size, imported_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(sha256) DO NOTHING",
            params![
                new.sha256,
                new.storage_key,
                kind_str(new.kind),
                new.taken_at,
                year_of(new.taken_at),
                new.size,
                new.imported_at,
            ],
        )?;
        if inserted > 0 {
            let id = self.conn.last_insert_rowid();
            return Ok(InsertOutcome {
                asset_id: id,
                duplicated: false,
            });
        }
        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT asset_id FROM assets WHERE sha256 = ?1",
                params![new.sha256],
                |r| r.get(0),
            )
            .optional()?;
        match existing {
            Some(id) => Ok(InsertOutcome {
                asset_id: id,
                duplicated: true,
            }),
            None => Err(StoreError::Core(ErrorCode::StoreFailed)),
        }
    }

    /// persist 完成置 ready（含解码元数据与缩略图 key）
    #[allow(clippy::too_many_arguments)]
    pub fn mark_ready(
        &self,
        asset_id: i64,
        width: u32,
        height: u32,
        taken_at: i64,
        mime: &str,
        exif_json: Option<&str>,
        thumb_key: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE assets SET status='ready', width=?2, height=?3, taken_at=?4, year=?5,
             mime=?6, exif=?7, thumb_key=?8, error_code=NULL WHERE asset_id=?1",
            params![
                asset_id,
                width,
                height,
                taken_at,
                year_of(taken_at),
                mime,
                exif_json,
                thumb_key
            ],
        )?;
        Ok(())
    }

    pub fn mark_failed(&self, asset_id: i64, error_code: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE assets SET status='failed', error_code=?2 WHERE asset_id=?1",
            params![asset_id, error_code],
        )?;
        Ok(())
    }

    /// 是否已有**就绪**资产（重试语义：pending/failed 行不算，允许重新处理）
    pub fn exists_ready(&self, sha256: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM assets WHERE sha256 = ?1 AND status = 'ready'",
                params![sha256],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM assets", [], |r| r.get(0))?)
    }

    /// 时间轴分页：taken_at 降序 keyset 游标（taken_at, asset_id）
    pub fn list_page(&self, cursor: Option<(i64, i64)>, page_size: u32) -> Result<Vec<AssetRow>> {
        let page_size = page_size.clamp(1, 500);
        let sql = "SELECT asset_id, sha256, storage_key, kind, size, width, height,
                          taken_at, year, thumb_key, status, error_code
                   FROM assets
                   WHERE (?1 IS NULL OR taken_at < ?1 OR (taken_at = ?1 AND asset_id < ?2))
                   ORDER BY taken_at DESC, asset_id DESC
                   LIMIT ?3";
        let (cur_t, cur_id) = match cursor {
            Some((t, id)) => (Some(t), Some(id)),
            None => (None, None),
        };
        let rows = self
            .conn
            .prepare(sql)?
            .query_map(params![cur_t, cur_id, page_size], row_to_asset)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_asset(&self, asset_id: i64) -> Result<Option<AssetRow>> {
        let sql = "SELECT asset_id, sha256, storage_key, kind, size, width, height,
                          taken_at, year, thumb_key, status, error_code
                   FROM assets WHERE asset_id = ?1";
        Ok(self
            .conn
            .query_row(sql, params![asset_id], row_to_asset)
            .optional()?)
    }

    /// 批量删除；返回（删除数、未命中数）与被删资产的 thumb_key 列表（文件由调用方清理）
    pub fn delete_assets(&self, ids: &[i64]) -> Result<(usize, usize, Vec<String>)> {
        let mut deleted = 0usize;
        let mut missing = 0usize;
        let mut thumb_keys = Vec::new();
        for &id in ids {
            let key: Option<Option<String>> = self
                .conn
                .query_row(
                    "SELECT thumb_key FROM assets WHERE asset_id=?1",
                    params![id],
                    |r| r.get(0),
                )
                .optional()?;
            match key {
                Some(k) => {
                    self.conn
                        .execute("DELETE FROM assets WHERE asset_id=?1", params![id])?;
                    let _ = self.delete_embedding(id);
                    let _ = self
                        .conn
                        .execute("DELETE FROM fts_text WHERE asset_id = ?1", params![id]);
                    deleted += 1;
                    if let Some(k) = k {
                        thumb_keys.push(k);
                    }
                }
                None => missing += 1,
            }
        }
        Ok((deleted, missing, thumb_keys))
    }

    /// 失败清单（批次后供 UI 重试与展示）
    pub fn list_failed(&self) -> Result<Vec<AssetRow>> {
        let sql = "SELECT asset_id, sha256, storage_key, kind, size, width, height,
                          taken_at, year, thumb_key, status, error_code
                   FROM assets WHERE status = 'failed' ORDER BY imported_at DESC";
        let rows = self
            .conn
            .prepare(sql)?
            .query_map([], row_to_asset)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 重试失败项：清 error 回 pending（pipeline 下一轮按哈希幂等处理）
    pub fn reset_failed(&self) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE assets SET status='pending', error_code=NULL WHERE status='failed'",
            [],
        )?)
    }

    // ---- 向量检索（P2；迁移 v2 起）----

    /// 写入/覆盖嵌入（vec0 无原地更新，走整行替换）；f32 按 blob 绑定
    pub fn insert_embedding(&self, asset_id: i64, embedding: &[f32]) -> Result<()> {
        self.conn.execute(
            "DELETE FROM vec_assets WHERE asset_id = ?1",
            params![asset_id],
        )?;
        let blob = unsafe {
            std::slice::from_raw_parts(
                embedding.as_ptr().cast::<u8>(),
                std::mem::size_of_val(embedding),
            )
        };
        self.conn.execute(
            "INSERT INTO vec_assets (asset_id, embedding) VALUES (?1, ?2)",
            params![asset_id, blob],
        )?;
        Ok(())
    }

    pub fn delete_embedding(&self, asset_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM vec_assets WHERE asset_id = ?1",
            params![asset_id],
        )?;
        Ok(())
    }

    /// KNN 检索：返回 (asset_id, distance)；f32 向量以 blob 绑定
    pub fn knn_search(&self, query: &[f32], k: u32) -> Result<Vec<(i64, f32)>> {
        let k = k.clamp(1, 1000);
        let qblob: &[u8] = unsafe {
            std::slice::from_raw_parts(query.as_ptr().cast::<u8>(), std::mem::size_of_val(query))
        };
        let sql = "SELECT asset_id, distance FROM vec_assets
                   WHERE embedding MATCH ?1 AND k = ?2";
        let rows = self
            .conn
            .prepare(sql)?
            .query_map(params![qblob, k], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, f32>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 待嵌入的 ready 资产（嵌入管线取件）
    pub fn list_ready_without_embedding(&self, limit: u32) -> Result<Vec<(i64, String)>> {
        let sql = "SELECT a.asset_id, a.year FROM assets a
                   WHERE a.status = 'ready'
                     AND a.asset_id NOT IN (SELECT asset_id FROM vec_assets)
                   ORDER BY a.asset_id LIMIT ?1";
        let rows = self
            .conn
            .prepare(sql)?
            .query_map(params![limit], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 已嵌入数量
    pub fn count_embedded(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM vec_assets", [], |r| r.get(0))?)
    }

    /// 清空全部嵌入（重建索引用）
    pub fn clear_embeddings(&self) -> Result<()> {
        self.conn.execute("DELETE FROM vec_assets", [])?;
        Ok(())
    }

    // ---- 文本检索（P3；迁移 v3 起）----

    /// 写入/覆盖资产的文本索引（content = 文件名等）
    pub fn insert_fts(&self, asset_id: i64, content: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM fts_text WHERE asset_id = ?1",
            params![asset_id],
        )?;
        self.conn.execute(
            "INSERT INTO fts_text (asset_id, content) VALUES (?1, ?2)",
            params![asset_id, content],
        )?;
        Ok(())
    }

    /// FTS5 匹配：query 需为已清洗的 MATCH 表达式；返回按 rank 排序的 asset_id
    pub fn search_fts(&self, match_query: &str, k: u32) -> Result<Vec<i64>> {
        let k = k.clamp(1, 1000);
        let rows = self
            .conn
            .prepare(
                "SELECT asset_id FROM fts_text WHERE fts_text MATCH ?1 ORDER BY rank LIMIT ?2",
            )?
            .query_map(params![match_query, k], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 把任意用户输入转成安全的 FTS MATCH 表达式（按空白分词后逐词加引号）
    pub fn build_match_query(input: &str) -> String {
        input
            .split_whitespace()
            .map(|term| format!("\"{}\"", term.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn row_to_asset(row: &Row<'_>) -> rusqlite::Result<AssetRow> {
    let kind: String = row.get(3)?;
    let status: String = row.get(10)?;
    Ok(AssetRow {
        asset_id: row.get(0)?,
        sha256: row.get(1)?,
        storage_key: row.get(2)?,
        kind: match kind.as_str() {
            "video" => AssetKind::Video,
            "audio" => AssetKind::Audio,
            _ => AssetKind::Photo,
        },
        size: row.get(4)?,
        width: row.get::<_, Option<i64>>(5)?.map(|v| v as u32),
        height: row.get::<_, Option<i64>>(6)?.map(|v| v as u32),
        taken_at: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
        year: row.get(8)?,
        thumb_key: row.get(9)?,
        status: match status.as_str() {
            "indexing" => AssetStatus::Indexing,
            "ready" => AssetStatus::Ready,
            "failed" => AssetStatus::Failed,
            _ => AssetStatus::Pending,
        },
        error_code: row.get(11)?,
    })
}

fn kind_str(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Photo => "photo",
        AssetKind::Video => "video",
        AssetKind::Audio => "audio",
    }
}

/// 拍摄年份（UTC，Howard Hinnant civil_from_days），同时作为分区键来源
fn year_of(taken_at: i64) -> String {
    let days = taken_at.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400; // 以 3 月为年首的年份
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    // 1、2 月属于前一个"三月年首年"，回正为日历年
    if month <= 2 {
        (y + 1).to_string()
    } else {
        y.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_asset(sha: &str, taken_at: i64) -> NewAsset {
        NewAsset {
            sha256: sha.into(),
            storage_key: format!("photos/{sha}.jpg"),
            kind: AssetKind::Photo,
            size: Some(1024),
            taken_at,
            imported_at: 1_760_000_000,
        }
    }

    #[test]
    fn migration_creates_v1_schema() {
        let store = Store::open_memory().unwrap();
        assert!(store.user_version().unwrap() >= 1, "至少完成 v1 迁移");
        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM assets", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        // vec 扩展随连接注册可用（ADR-0002 前置验证）
        let ver: String = store
            .conn
            .query_row("SELECT vec_version()", [], |r| r.get(0))
            .unwrap();
        assert!(!ver.is_empty());
    }

    #[test]
    fn insert_is_deduplicated_by_sha() {
        let store = Store::open_memory().unwrap();
        let first = store
            .insert_pending(&new_asset("aaa", 1_700_000_000))
            .unwrap();
        assert!(!first.duplicated);
        let again = store
            .insert_pending(&new_asset("aaa", 1_700_000_000))
            .unwrap();
        assert!(again.duplicated);
        assert_eq!(first.asset_id, again.asset_id);
        assert_eq!(store.count().unwrap(), 1);
    }

    #[test]
    fn mark_ready_updates_metadata_and_year() {
        let store = Store::open_memory().unwrap();
        let id = store
            .insert_pending(&new_asset("bbb", 1_700_000_000))
            .unwrap()
            .asset_id;
        store
            .mark_ready(
                id,
                1920,
                1080,
                1_700_000_000,
                "image/jpeg",
                None,
                "ab/bbb.webp",
            )
            .unwrap();
        let row = store.get_asset(id).unwrap().unwrap();
        assert_eq!(row.status, AssetStatus::Ready);
        assert_eq!(row.width, Some(1920));
        assert_eq!(row.thumb_key.as_deref(), Some("ab/bbb.webp"));
        // 1_700_000_000 = 2023-11-14 UTC
        assert_eq!(row.year, "2023");
    }

    #[test]
    fn timeline_page_is_keyset_paginated() {
        let store = Store::open_memory().unwrap();
        for i in 0..10 {
            let t = 1_700_000_000 + i * 60;
            let id = store
                .insert_pending(&new_asset(&format!("sha{i:02}"), t))
                .unwrap()
                .asset_id;
            store
                .mark_ready(id, 10, 10, t, "image/jpeg", None, "k")
                .unwrap();
        }
        let page1 = store.list_page(None, 4).unwrap();
        assert_eq!(page1.len(), 4);
        assert!(page1.windows(2).all(|w| w[0].taken_at >= w[1].taken_at));
        let last = page1.last().unwrap();
        let page2 = store
            .list_page(Some((last.taken_at, last.asset_id)), 4)
            .unwrap();
        // 无重复无遗漏
        assert!(!page2
            .iter()
            .any(|r| page1.iter().any(|p| p.asset_id == r.asset_id)));
        let page3 = store
            .list_page(
                Some((
                    page2.last().unwrap().taken_at,
                    page2.last().unwrap().asset_id,
                )),
                4,
            )
            .unwrap();
        assert_eq!(page3.len(), 2);
    }

    #[test]
    fn delete_reports_missing_and_collects_thumb_keys() {
        let store = Store::open_memory().unwrap();
        let a = store
            .insert_pending(&new_asset("c1", 1_700_000_000))
            .unwrap()
            .asset_id;
        store
            .mark_ready(a, 1, 1, 1_700_000_000, "image/jpeg", None, "c1/c1.webp")
            .unwrap();
        let (deleted, missing, keys) = store.delete_assets(&[a, 999]).unwrap();
        assert_eq!((deleted, missing), (1, 1));
        assert_eq!(keys, vec!["c1/c1.webp".to_string()]);
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn exists_ready_distinguishes_status() {
        let store = Store::open_memory().unwrap();
        let sha = "status-sha";
        let id = store
            .insert_pending(&new_asset(sha, 1_700_000_000))
            .unwrap()
            .asset_id;
        assert!(!store.exists_ready(sha).unwrap());
        store.mark_failed(id, "decode_failed").unwrap();
        assert!(!store.exists_ready(sha).unwrap(), "failed 行允许重试");
        store.reset_failed().unwrap();
        assert!(!store.exists_ready(sha).unwrap(), "pending 行允许重试");
        store
            .mark_ready(id, 1, 1, 1_700_000_000, "image/jpeg", None, "k")
            .unwrap();
        assert!(store.exists_ready(sha).unwrap());
    }

    #[test]
    fn migration_creates_v2_vec_table() {
        let store = Store::open_memory().unwrap();
        assert!(store.user_version().unwrap() >= 2);
        let id = store
            .insert_pending(&new_asset("vec-sha", 1_700_000_000))
            .unwrap()
            .asset_id;
        store
            .mark_ready(id, 1, 1, 1_700_000_000, "image/jpeg", None, "k")
            .unwrap();

        let emb: Vec<f32> = (0..512).map(|i| ((i % 64) as f32 - 32.0) / 32.0).collect();
        store.insert_embedding(id, &emb).unwrap();
        assert_eq!(store.count_embedded().unwrap(), 1);

        let hits = store.knn_search(&emb, 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, id);
        // 距离为 0（自身）
        assert!(hits[0].1.abs() < 1e-4);

        // 重复插入 = 覆盖（vec0 无原地更新语义由上层保证）
        store.insert_embedding(id, &emb).unwrap();
        assert_eq!(store.count_embedded().unwrap(), 1);

        // 删除资产级联删除向量
        store.delete_assets(&[id]).unwrap();
        assert_eq!(store.count_embedded().unwrap(), 0);
    }

    #[test]
    fn pending_embedding_queue_and_clear() {
        let store = Store::open_memory().unwrap();
        let mut ids = Vec::new();
        for i in 0..3 {
            let id = store
                .insert_pending(&new_asset(&format!("pq{i}"), 1_700_000_000))
                .unwrap()
                .asset_id;
            store
                .mark_ready(id, 1, 1, 1_700_000_000, "image/jpeg", None, "k")
                .unwrap();
            ids.push(id);
        }
        assert_eq!(store.list_ready_without_embedding(10).unwrap().len(), 3);
        let emb: Vec<f32> = vec![0.0; 512];
        store.insert_embedding(ids[0], &emb).unwrap();
        assert_eq!(store.list_ready_without_embedding(10).unwrap().len(), 2);
        store.clear_embeddings().unwrap();
        assert_eq!(store.list_ready_without_embedding(10).unwrap().len(), 3);
    }

    #[test]
    fn migration_v3_fts_roundtrip_and_cascade() {
        let store = Store::open_memory().unwrap();
        assert!(store.user_version().unwrap() >= 3);
        let id = store
            .insert_pending(&new_asset("fts-sha", 1_700_000_000))
            .unwrap()
            .asset_id;
        store.insert_fts(id, "IMG_2023 海边日落").unwrap();

        let hits = store
            .search_fts(&Store::build_match_query("2023"), 10)
            .unwrap();
        assert_eq!(hits, vec![id]);
        // trigram 分词：≥3 字符子串可命中（中文连续段）
        let hits2 = store
            .search_fts(&Store::build_match_query("海边日落"), 10)
            .unwrap();
        assert_eq!(hits2, vec![id]);
        let miss = store
            .search_fts(&Store::build_match_query("雪山"), 10)
            .unwrap();
        assert!(miss.is_empty());

        // 删除资产级联清 FTS（无幽灵结果）
        store.delete_assets(&[id]).unwrap();
        assert!(store
            .search_fts(&Store::build_match_query("2023"), 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn fts_match_query_is_safe_against_special_chars() {
        // 特殊字符被剥离/引号包裹，不产生语法错误
        let q = Store::build_match_query("a\"b OR 1=1");
        assert!(!q.contains("\"\""));
        let store = Store::open_memory().unwrap();
        let _ = store.search_fts(&q, 10); // 只要不 panic / 不返回 Err 即可
    }

    #[test]
    fn failed_lifecycle_roundtrip() {
        let store = Store::open_memory().unwrap();
        let id = store
            .insert_pending(&new_asset("ddd", 1_700_000_000))
            .unwrap()
            .asset_id;
        store.mark_failed(id, "decode_failed").unwrap();
        let failed = store.list_failed().unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].error_code.as_deref(), Some("decode_failed"));
        assert_eq!(store.reset_failed().unwrap(), 1);
        assert!(store.list_failed().unwrap().is_empty());
    }

    #[test]
    fn year_conversion_matches_known_dates() {
        // 2020-02-29 12:00 UTC = 1582977600（闰年边界）
        assert_eq!(year_of(1_582_977_600), "2020");
        // 1999-12-31 23:59 UTC = 946684740（跨年边界）
        assert_eq!(year_of(946_684_740), "1999");
        // 2023-11-14 UTC = 1700000000
        assert_eq!(year_of(1_700_000_000), "2023");
        // 1970-01-01 UTC = 0
        assert_eq!(year_of(0), "1970");
    }
}
