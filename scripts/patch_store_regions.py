"""补丁:store 增加区域级语义索引存储函数(ADR-0015 切片 A1)"""

s = open('crates/store/src/lib.rs', encoding='utf-8').read()

# 1) delete_assets 级联删区域
old_del = '''                Some(k) => {
                    self.conn
                        .execute("DELETE FROM assets WHERE asset_id=?1", params![id])?;
                    let _ = self.delete_asset_everywhere(id);
                    let _ = self
                        .conn
                        .execute("DELETE FROM fts_text WHERE asset_id = ?1", params![id]);'''
new_del = '''                Some(k) => {
                    self.conn
                        .execute("DELETE FROM assets WHERE asset_id=?1", params![id])?;
                    let _ = self.delete_asset_everywhere(id);
                    let _ = self.delete_regions_of_assets(&[id]);
                    let _ = self
                        .conn
                        .execute("DELETE FROM fts_text WHERE asset_id = ?1", params![id]);'''
assert old_del in s, 'delete_assets anchor missing'
s = s.replace(old_del, new_del, 1)

# 2) 区域函数块:追加在 ready_count_of_folder 之后
anchor = '''    /// 工作区 ready 资产总数（进度卡分母）
    pub fn ready_count_of_folder(&self, folder_id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM assets WHERE folder_id = ?1 AND status = 'ready'",
            params![folder_id],
            |r| r.get(0),
        )?)
    }'''
block = anchor + '''

    // ---- 区域级语义索引（ADR-0015）----

    /// 删除资产集合的区域记录与向量（delete_assets 级联 / 重新提取前清理）
    pub fn delete_regions_of_assets(&self, asset_ids: &[i64]) -> Result<()> {
        for &id in asset_ids {
            let _ = self.conn.execute(
                "DELETE FROM vec_regions WHERE region_id IN
                 (SELECT region_id FROM regions WHERE asset_id = ?1)",
                params![id],
            );
            let _ = self
                .conn
                .execute("DELETE FROM regions WHERE asset_id = ?1", params![id]);
        }
        Ok(())
    }

    /// 待提取区域的 ready 照片：无区域记录且文件夹在线；按 taken_at 排序
    /// （时空调制需要时间序，连拍相邻）。返回 (asset_id, storage_key, taken_at)。
    pub fn list_ready_without_regions(&self, limit: u32) -> Result<Vec<(i64, String, Option<i64>)>> {
        let rows = self
            .conn
            .prepare(
                "SELECT a.asset_id, a.storage_key, a.taken_at FROM assets a
                 JOIN folders f ON f.folder_id = a.folder_id
                 WHERE a.status = 'ready' AND f.status = 'online'
                   AND a.kind = 'photo'
                   AND NOT EXISTS (SELECT 1 FROM regions WHERE asset_id = a.asset_id)
                 ORDER BY a.taken_at, a.asset_id LIMIT ?1",
            )?
            .query_map(params![limit], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 待提取区域的照片数（进度卡分母）
    pub fn count_ready_without_regions(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM assets a
             JOIN folders f ON f.folder_id = a.folder_id
             WHERE a.status = 'ready' AND f.status = 'online'
               AND a.kind = 'photo'
               AND NOT EXISTS (SELECT 1 FROM regions WHERE asset_id = a.asset_id)",
            [],
            |r| r.get(0),
        )?)
    }

    /// 已提取区域总数（进度卡分子）
    pub fn count_regions(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM regions", [], |r| r.get(0))?)
    }

    /// 写入一个区域（元数据 + 向量）。返回全局 region_id。
    pub fn insert_region(
        &self,
        asset_id: i64,
        region_idx: i32,
        bbox: (i32, i32, i32, i32),
        area_frac: f32,
        chapter_id: i64,
        cluster_id: i64,
        embedding: &[f32],
    ) -> Result<i64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.conn.execute(
            "INSERT INTO regions (asset_id, region_idx, bbox_x0, bbox_y0, bbox_x1, bbox_y1,
                                  area_frac, cluster_id, chapter_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![asset_id, region_idx, bbox.0, bbox.1, bbox.2, bbox.3,
                    area_frac, cluster_id, chapter_id, now],
        )?;
        let region_id = self.conn.last_insert_rowid();
        let blob: &[u8] = unsafe {
            std::slice::from_raw_parts(embedding.as_ptr().cast::<u8>(), embedding.len() * 4)
        };
        self.conn.execute(
            "INSERT INTO vec_regions (region_id, embedding) VALUES (?1, ?2)",
            params![region_id, blob],
        )?;
        Ok(region_id)
    }

    /// 区域 KNN：返回 (region_id, asset_id, distance)
    pub fn knn_regions(&self, query: &[f32], k: u32) -> Result<Vec<(i64, i64, f32)>> {
        let k = k.clamp(1, 4096);
        let blob: &[u8] = unsafe {
            std::slice::from_raw_parts(query.as_ptr().cast::<u8>(), query.len() * 4)
        };
        let rows = self
            .conn
            .prepare(
                "SELECT v.region_id, r.asset_id, v.distance
                 FROM vec_regions v JOIN regions r ON r.region_id = v.region_id
                 WHERE v.embedding MATCH ?1 AND k = ?2",
            )?
            .query_map(params![blob, k], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get::<_, f32>(2)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 章节列表（chapter_id, started_at）升序
    pub fn list_region_chapters(&self) -> Result<Vec<(i64, i64)>> {
        let rows = self
            .conn
            .prepare("SELECT chapter_id, started_at FROM region_chapters ORDER BY chapter_id")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 新开章节（长时间+大空间：完全新增原型集）
    pub fn new_region_chapter(&self, started_at: i64) -> Result<i64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.conn.execute(
            "INSERT INTO region_chapters (started_at, created_at) VALUES (?1, ?2)",
            params![started_at, now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 当前最新章节
    pub fn latest_region_chapter(&self) -> Result<(i64, i64)> {
        self.conn
            .query_row(
                "SELECT chapter_id, started_at FROM region_chapters ORDER BY chapter_id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.into())
    }

    /// 原型 upsert（在线 DP-means：新建 / 漂移 / 计数）
    pub fn upsert_region_cluster(
        &self,
        cluster_id: i64,
        chapter_id: i64,
        prototype: &[f32],
        member_count: i64,
    ) -> Result<()> {
        let blob: &[u8] = unsafe {
            std::slice::from_raw_parts(prototype.as_ptr().cast::<u8>(), prototype.len() * 4)
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.conn.execute(
            "INSERT INTO region_clusters (index_id, cluster_id, chapter_id, prototype, member_count, created_at)
             VALUES (2, ?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(index_id, cluster_id) DO UPDATE SET
               prototype = excluded.prototype,
               member_count = excluded.member_count",
            params![cluster_id, chapter_id, blob, member_count, now],
        )?;
        Ok(())
    }

    /// 删除原型（衰退合并后）
    pub fn delete_region_cluster(&self, cluster_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM region_clusters WHERE index_id = 2 AND cluster_id = ?1",
            params![cluster_id],
        )?;
        Ok(())
    }'''
assert anchor in s, 'ready_count_of_folder anchor missing'
s = s.replace(anchor, block, 1)
open('crates/store/src/lib.rs', 'w', encoding='utf-8', newline='\n').write(s)
print('store functions added')
