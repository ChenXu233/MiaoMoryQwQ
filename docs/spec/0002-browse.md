# 规格 0002：浏览与预览

- 状态：已评审（所有者 2026-09-04 授权连续作业，视为批准冻结）
- 负责人：ZCode（AI 代理）
- 关联切片：P1（ADR-0007）
- 关联体验规格：`docs/ux/0002-browse.md`
- 关联 ADR：ADR-0003、ADR-0009
- 最后更新：2026-09-04

## 1. 目标与非目标

**目标**：以时间轴（按年分组）网格浏览全部已导入照片，虚拟滚动流畅，可进入 Lightbox 全屏预览并用键盘操作。

**非目标**：收藏/人工相册（LATER）；地图/位置视图（LATER）；多选批量操作（仅支持删除，P3 打磨细化）；视频预览。

## 2. 用户故事

作为用户，我想按年份快速浏览我所有的照片，点开任意一张全屏查看，以便找到并确认某段回忆。

## 3. 流程

1. 库非空时进入浏览态：时间轴列表，按 `taken_at` 降序、年份分组标题。
2. 网格缩略图懒加载（滚动到可见才请求），点击缩略图打开 Lightbox。
3. Lightbox：显示原图（大图按需读取原文件），←/→ 在**全库时间序**中切换，Esc 关闭，Space 切换"适应窗口/1:1"。
4. 删除：Lightbox 内按 Delete 弹确认 → 确认后删除资产记录与缩略图（原文件永不触碰），列表移除该图。
5. 滚动加载：keyset 分页（`taken_at, asset_id` 游标），滚动近底部自动拉取下一页。

## 4. 状态矩阵

| 状态 | 展示什么 | 用户能做什么 | 文案要点 | 错误码 |
| :--- | :--- | :--- | :--- | :--- |
| 空 | 空态引导（同规格 0001） | 去导入 | "还没有照片，先导入一个文件夹吧" | - |
| 加载中 | 骨架占位块 | 继续滚动 | - | - |
| 成功 | 时间轴网格 | 滚动/点击/键盘 | - | - |
| 部分成功 | 缩略图缺失项显示占位图标，不阻塞 | 点击查看原图（原图可读则正常） | "缩略图丢失，点击查看原图会重新生成" | write_failed |
| 错误 | 分页加载失败条（可重试） | 点击重试 | "加载更多失败，点击重试" | store_failed |
| 离线 | 不适用 | - | - | - |
| 权限缺失 | 原图不可读提示（Lightbox 内） | 关闭或删除该条 | "原文件无法读取，可能已被移动" | read_failed |

## 5. 数据契约

- 读查询：`list_timeline(cursor: Option<String>, page_size: u32) -> TimelinePage { groups: Vec<YearGroup { year: u16, items: Vec<AssetSummary }> }, next_cursor: Option<String> }`；`AssetSummary { asset_id, thumb_path, width, height, taken_at }`
- `get_asset_image(asset_id) -> String`（原图绝对路径，经 asset protocol 读取）
- 写命令：`delete_assets(ids: Vec<i64>) -> DeleteReport { deleted, missing }`
- 缩略图 URL 由前端 `convertFileSrc` 生成，asset protocol scope 限定工作区 thumbs/ 与工作区根

## 6. 验收标准（Given / When / Then）

1. Given 库中有跨 3 个年份的 1,000 条资产，When 首页加载，Then 首屏仅渲染可见区缩略图（虚拟滚动），年份分组降序正确。
2. Given 滚动近底部，When 触发分页，Then 以 keyset 游标取下一页，无重复无遗漏。
3. Given Lightbox 打开，When 按 → 三次后按 Esc，Then 关闭后网格焦点回到原缩略图。
4. Given 在 Lightbox 删除当前图，When 确认，Then 库记录与缩略图被删除、原文件保留、列表移除该图。
5. Given 某资产缩略图文件被手动删除，When 浏览，Then 该项显示占位且可打开 Lightbox 看原图。
6. Given `delete_assets` 传入不存在的 id，Then 报告 missing 计数而不报错。

## 7. 边界与降级

- >10 万资产：虚拟滚动 + 分页保证常驻内存与 DOM 数量有界；年份分组标题随滚动吸顶。
- 原图文件被移动：Lightbox 报 read_failed 文案；缩略图仍在时可继续浏览。
- 极小窗口：网格列数自适应（4 → 2 列）。

## 8. 开放问题

- 时间轴"月份"二级分组是否必要——首版只做年份，回归再议。
