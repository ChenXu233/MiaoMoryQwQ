# ADR-0013：可插拔索引架构（Indexer trait / 多索引并存 / 多流置信度融合）

- 状态：Proposed（所有者 2026-09-08 指令「开始做切片 C」为执行授权；文档状态待所有者追认）
- 日期：2026-09-08
- 决策者：项目所有者（裁定 23/24 发起）
- 相关：白皮书 §1.2（AI 可插拔）、§4.5（schema）；裁定 22/23/24/25（技术底稿 §1）

## 背景

所有者裁定：缩略图只是给人看与省 IO 的设计，**索引永远从原图解码**（裁定 23）；模型未来不一定是 CLIP，因此**索引过程要抽象化**——用户可自带模型、第三方可写插件做新索引方案，数据库同时保存**多套模型建立的多份索引**，检索时综合输出置信度（裁定 24）。

现状：`mm-core::Embedder` 单实例 trait，`vec_assets` 单表硬绑 Chinese-CLIP int8（512 维），`mm-embed::ClipEmbedder` 是唯一实现，嵌入 worker 单队列，检索只查这一个向量表 + FTS。

## 决策

1. **端口trait 升级为 `Indexer`**（`mm-core`，替换 `Embedder` 端口）：

   ```rust
   pub trait Indexer: Send + Sync {
       fn index_id(&self) -> i64;      // 对应 index_meta.index_id
       fn slug(&self) -> &str;         // 稳定标识（如 "chinese-clip-vit-b16-int8"）
       fn display(&self) -> &str;      // UI 展示名
       fn dim(&self) -> u32;           // 向量维度
       fn supports_text(&self) -> bool;
       fn embed_images(&self, batch: &[DecodedImage]) -> Result<Vec<Vec<f32>>, ErrorCode>;
       fn embed_text(&self, query: &str) -> Result<Vec<f32>, ErrorCode>;
   }
   ```

   `ClipEmbedder` 实现 `Indexer`；每套模型一个实例，加载器按 `index_meta` 注册表 ∩ 本地模型文件装配（用户导入新模型 = 注册新 index_meta + 提供模型文件，后续切片提供 UI）。

2. **多索引持久化（迁移 v5）**：

   ```sql
   CREATE TABLE index_meta (
       index_id INTEGER PRIMARY KEY,
       slug TEXT NOT NULL UNIQUE,
       display TEXT NOT NULL,
       model TEXT NOT NULL,
       dim INTEGER NOT NULL,
       vec_table TEXT NOT NULL UNIQUE,   -- 每索引一张 vec0 表（维度可不同）
       status TEXT NOT NULL DEFAULT 'active',  -- active | disabled
       created_at INTEGER NOT NULL
   );
   -- 种子：index_id=1, slug='chinese-clip-vit-b16-int8', vec_table='vec_assets'（复用 v2 表，零数据迁移）
   ```

   新索引注册时运行时 `CREATE VIRTUAL TABLE vec_i{index_id} USING vec0(asset_id INTEGER PRIMARY KEY, embedding float32[{dim}])`。表名存 `index_meta`，store 层拼接前加引号防护（内部数据，非用户输入）。

3. **store 全部向量 API 按 `index_id` 参数化**：insert/delete/copy/find_source/count/knn/待嵌入队列均带 index_id；`delete_assets` 级联删除该资产在**所有** active 索引中的向量。

4. **嵌入 worker 按索引循环**：对每个 active 且已加载的索引独立清队列（某索引模型缺失/加载失败不影响其他索引）。

5. **多流融合**：检索 = 每个支持文本的索引各出一路 KNN 排序 + FTS 文本流 → `rrf_fuse_multi`（RRF 推广到 N 流，k=60 不变）；UI 命中徽标语义不变（语义/文件名），各索引原始分进 tracing（综合置信度的展示细化放后续 UX 切片）。

6. **边界（本切片不做）**：用户导入模型文件的 UI/CLI；插件加载机制（动态库/WASM）；索引版本迁移（模型升级 = 新 index_id + 旧索引 disable）；各索引独立置信度权重调节。

## 后果

### 积极

- 数据模型为「用户自带模型 / 第三方索引插件」铺好路，新增一套索引不再动 schema
- 某索引不可用时搜索优雅降级（其余索引照常出结果）
- 每索引占用可量化（裁定 25 的分项展示有了数据源）

### 消极 / 需关注

- 多索引同资产多份向量，磁盘占用随索引数线性增长（设置页分项可见，清理功能所有者已裁定暂不做）
- 跨表动态 SQL 需严格走 `index_meta`（引号防护 + 只读），不暴露给用户输入
- `Embedder` → `Indexer` 端口改名触及 mm-core 5 端口约定，需同步 `docs/developers/01-architecture.md`

## 备选方案

| 方案 | 结论 | 原因 |
| :--- | :--- | :--- |
| 单 vec 表加 index_id 列 | 否 | vec0 主键即 rowid，无法复合键；上游 PARTITION KEY 又是坏的（spec 0003 §5 偏差） |
| 只做 trait 抽象、不做多表 | 否 | 裁定 24 明确要求「数据库存多套索引」 |
| 全量 ONNX 模型管理器（含下载/注册 UI） | 本切片否 | 范围失控；本切片只落数据模型与执行路径，注册 UI 后续 |
