# 规格 0005：混合检索融合（FTS + 过滤 + RRF）

- 状态：已评审（所有者 2026-09-04 授权连续作业，视为批准冻结）
- 负责人：ZCode（AI 代理）
- 关联切片：P3（ADR-0007）
- 关联体验规格：`docs/ux/0005-hybrid-search.md`
- 关联 ADR：ADR-0002、ADR-0010
- 最后更新：2026-09-04

## 1. 目标与非目标

**目标**：搜索升级为混合检索——语义流（vec_assets KNN）与文本流（FTS5 文件名匹配）经 RRF 融合排序；查询前可按日期区间与照片类型过滤。

**非目标**：OCR/描述文本（LATER，fts content 结构已预留）；跨工作区；同义词展开。

## 2. 用户故事

作为用户，我记得照片文件名里有"IMG_2023"，想用文件名片段也能找到它，同时中文语义搜索照常工作。

## 3. 流程

1. schema v3：`fts_text(asset_id UNINDEXED, content)`；资产 ready 时由 pipeline persist 写入 `content = 文件名（去扩展名）`。
2. 查询处理：SQL 前置过滤（taken_at 区间、kind）→ 语义流 KNN top 100 + 文本流 FTS MATCH top 100 → RRF（k=60）融合 → 返回带 `matched` 理由的结果。
3. 排序理由：`semantic` / `text` / `both`，UI 以小徽标展示（可解释性，白皮书 §4.6）。
4. 纯文件名类查询（语义模型对无意义串不敏感）由文本流兜底；两路任一失败不影响另一路。

## 4. 状态矩阵

| 状态 | 展示什么 | 用户能做什么 | 文案要点 | 错误码 |
| :--- | :--- | :--- | :--- | :--- |
| 其余状态 | 复用规格 0003 | - | - | - |
| 部分成功（单流失败） | 另一流结果 + 降级提示 | 继续/重试 | "部分搜索通道暂不可用，结果可能不全" | search_unavailable |

## 5. 数据契约

- schema v3：`CREATE VIRTUAL TABLE fts_text USING fts5(asset_id UNINDEXED, content);`
- pipeline：persist 成功后写 fts_text（文件名去扩展名）；delete 级联删除
- `search_assets` 增加 `filters: Option<SearchFilters { taken_from: Option<i64>, taken_to: Option<i64>, kind: Option<String> }>` 与结果的 `matched` 字段
- RRF 纯函数：`rrf_fuse(semantic: Vec<AssetId>, text: Vec<AssetId>, k=60) -> Vec<(AssetId, score, matched)>`

## 6. 验收标准（Given / When / Then）

1. Given 语义流 [a,b,c]、文本流 [b,d]，k=60，When 融合，Then b 第一（双流命中，matched=both），a、c、d 依 RRF 分数排序。
2. Given 任一列表为空，When 融合，Then 返回另一流的原序（matched 标记正确）。
3. Given 同一 id 在同流出现多次，When 融合，Then 去重取最优排名。
4. Given 过滤器 taken_from > 某资产 taken_at，When 搜索，Then 该资产不出现在结果。
5. Given 文件名含 "IMG_2023"，When 查询 "2023"，Then 文本流命中（FTS token 化含数字段）。
6. Given 资产被删除，When 搜索，Then 其 fts 行同步消失（无幽灵结果）。

## 7. 边界与降级

- FTS 查询语法错误（特殊字符）：包裹引号后重试，仍失败则仅走语义流。
- >5 万向量：RRF 候选集恒定（两流各 100），融合开销可忽略。

## 8. 开放问题

- **已决（v3 落地）**：FTS5 采用 `trigram` 分词器支持子串匹配（数字/拉丁/≥3 字符中文段）；<3 字符中文子串不命中，语义流兜底。
