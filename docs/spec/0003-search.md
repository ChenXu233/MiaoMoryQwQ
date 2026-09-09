# 规格 0003：中文语义搜索

- 状态：已评审（所有者 2026-09-04 授权连续作业，视为批准冻结）
- 负责人：ZCode（AI 代理）
- 关联切片：P2（ADR-0007）
- 关联体验规格：`docs/ux/0003-search.md`
- 关联规格：spec 0008（推理执行提供者配置）
- 关联 ADR：ADR-0002、ADR-0006（基准推迟与临时选型见 ADR-0010）、ADR-0013（可插拔索引）、ADR-0014
- 最后更新：2026-09-08

## 1. 目标与非目标

**目标**：用户输入中文查询词，应用在已导入照片上做图文语义检索，返回按相似度排序的照片；临时模型为 Chinese-CLIP ViT-B/16（ADR-0010，512 维）。

**非目标**：正式模型基准与选型（推迟，ADR-0010）；位置/OCR 检索；跨工作区检索；增量重嵌（模型更换时全量重建，见 §7）。

## 2. 用户故事

作为用户，我想输入"海边的日落"就找到相关照片，而不需要给照片打标签。

## 3. 流程

1. 前置：模型资产已下载到本地模型目录（见规格 0004）。
2. 导入完成后的 ready 资产批量嵌入：视觉编码 → L2 归一化 → 量化 → 写入 `vec_assets`（v2 迁移）。
3. 用户键入查询（防抖 150ms）→ 文本编码 → 归一化 → 量化 → sqlite-vec KNN（top 100，分区过滤可选）→ join `assets` → 返回快照（含相似度得分）。
4. 批量嵌入在导入任务结束后自动进行（错峰补嵌）；也提供"全部重建索引"入口（模型变更/量化策略变更时）。
   - **错峰约定（2026-09-08 实测，待所有者追认）**：嵌入 worker 在导入进行期间必须空转等待（`ImportEngine::running`）。同盘并发双读流（导入预取 + 嵌入重读）在 USB 外置盘上实测产生约 10 倍读放大（30.6MB/s 单流顺序读 vs 2.9MB/s 并发），且嵌入解码与导入解码争抢 CPU。导入完成即自动开始清队列，检索进度经 `EmbedProgressEvent` 上报，UI 呈现"建立索引中"。
5. 模型未就绪：搜索框可用但提交时显示降级提示（状态矩阵"离线"），不阻塞浏览。

## 4. 状态矩阵

| 状态 | 展示什么 | 用户能做什么 | 文案要点 | 错误码 |
| :--- | :--- | :--- | :--- | :--- |
| 空 | 搜索框 + 提示文案 | 输入查询 | "试试：海边的日落、猫咪、生日蛋糕" | - |
| 加载中 | 搜索框内联 spinner（首屏 <150ms 预算内多数直接出结果） | 继续输入 | - | - |
| 成功 | 结果网格（复用时间轴单元格）+ 相似度排序 | 点击进 Lightbox（←/→ 同序） | - | - |
| 部分成功 | 结果 + 顶部提示部分照片尚未索引 | 继续/稍后自动补齐 | "还有 N 张照片正在建立索引" | - |
| 错误 | 错误条 | 重试 | "搜索出了点问题，请重试" | search_unavailable |
| 离线（模型缺失） | 搜索框 + 下载引导入口 | 去下载模型（规格 0004） | "搜索功能需要先下载识别模型（约 200MB，仅下载一次）" | model_missing |
| 权限缺失 | 不适用（模型在应用数据目录） | - | - | - |

## 5. 数据契约

- schema v2：`CREATE VIRTUAL TABLE vec_assets USING vec0(asset_id INTEGER PRIMARY KEY, embedding float32[512]);`
  - **偏差记录**：① sqlite-vec 当前版本不识别 `int8` 列类型且忽略 `PARTITION KEY` 约束（静默不过滤），故存归一化 f32、不做分区；MVP ≤5 万向量全表 KNN 足够，二者随上游稳定后引入（届时重建 vec 表）。`mm-embed::quantize` 模块与单测保留。
  - **偏差记录**：sqlite-vec 当前版本（0.1.10-alpha）不识别 `int8` 列类型，INT8 量化推迟到上游支持；现阶段存归一化 f32（2KB/向量，5 万张 ≈100MB，可接受）。`mm-embed::quantize` 模块与单测保留，届时切换。
- 命令：`search_assets(query: String, top_k: Option<u32>) -> SearchPage { items: Vec<SearchHit>, model_ready: bool, pending_indexing: u64 }`；`SearchHit { summary: AssetSummary, score: f64 }`
- 命令：`reindex_all() -> u64`（返回受影响资产数）；`model_status() -> ModelStatus { ready, files_missing: Vec<String> }`
- 量化：归一化 f32 → INT8（×127 取整）；查询向量同样量化后走 `embedding MATCH` KNN
- 嵌入在导入任务结束后由后台 worker 补嵌（错峰，见 §3.4）；失败不阻塞导入（重试 = 重新导入或重建索引）

## 6. 验收标准（Given / When / Then）

1. Given 3 张已索引照片，When search_assets("测试")，Then 返回按 score 升序（距离）排列且不超过 top_k。
2. Given 查询向量量化后 512 维，When KNN，Then `vec_assets` 返回 (asset_id, distance) 且无解析错误（迁移 SQL 合法性由 store 迁移测试覆盖）。
3. Given 空查询或全空白查询，Then 返回空结果不调用模型。
4. Given 模型文件缺失，When search_assets，Then 返回 model_ready=false 且 items 为空（UI 显示离线态）。
5. Given 导入 10 张新照片，When 导入完成，Then 全部进入待嵌队列并完成嵌入（pending_indexing 归零）。
6. Given 相同查询连发两次，Then 结果顺序一致（确定性）。
7. f32 → INT8 → 反量化余弦相似度 ≥ 0.99（量化纯函数单测，随机向量）。

## 7. 边界与降级

- 模型更换（正式选型后）：全量重建索引（reindex_all），旧 `vec_assets` drop 重建；迁移策略随正式选型 ADR 给出。
- 10 万资产重建：批量 64 张/次，预计分钟级；进度经事件上报（复用导入事件通道，job_id 前缀区分）。
- 检索质量不达标：所有者手工验收触发 ADR-0006 正式基准（ADR-0010 约定）。

## 8. 开放问题

- KNN top_k 默认 100 是否够大（RRF 融合在 P3 依赖语义流候选质量）——P3 实测调整。
