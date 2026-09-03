# 规格 0001：导入与增量扫描

- 状态：已评审（所有者 2026-09-04 授权连续作业，视为批准冻结）
- 负责人：ZCode（AI 代理）
- 关联切片：P1（ADR-0007）
- 关联体验规格：`docs/ux/0001-import.md`
- 关联 ADR：ADR-0003、ADR-0008、ADR-0010
- 最后更新：2026-09-04

## 1. 目标与非目标

**目标**：用户选择一个文件夹后，应用扫描其中全部支持格式的照片，完成去重、解码、缩略图与入库，全程可见进度且可暂停/恢复，坏文件不阻塞批次。

**非目标**：音视频导入（LATER）；S3 远程目录；导入时生成任何语义向量（P2）；监视文件夹自动增量（LATER）；多任务并行导入。

**支持格式**：JPEG / PNG / WebP / HEIC / HEIF（ADR-0008）。扫描忽略：隐藏文件、无法读取的目录、其他扩展名。

## 2. 用户故事

作为个人用户，我想把手机导出的整个相册文件夹一次性拖进来，让应用自动完成一切，以便立刻浏览而无需理解任何概念。

## 3. 流程

1. 前置：工作区目录已就绪（P0 已建 `文档/MiaoMory`）。
2. 用户在空态页点击「选择照片文件夹」，系统目录选择器打开。
3. 选择后创建导入任务：scan（枚举 + 过滤）→ hash（SHA-256）→（命中库内已有哈希则跳过该项，计为"已存在"）→ decode（image/libheif + EXIF 方向校正）→ thumbnail（长边 512px WebP 写入工作区 thumbs/）→ persist（事务写入 assets 表）。
4. 每完成一项发出进度事件（总数、已完成、失败数、剩余时间估算）；事件节流 ≥100ms。
5. 暂停：用户点击暂停，引擎在当前项完成后停止发起新项，发出 paused 事件；恢复：从剩余清单继续。应用重启后用户再次导入同一文件夹即等效恢复（SHA-256 去重保证幂等，不产生重复资产与重复缩略图）。
6. 单项失败（解码错误/读取错误）：该项进入 failed 列表并继续下一项；批次结束时汇总。
7. 批次结束发出 finished 事件（含失败数）；UI 转入浏览态。

## 4. 状态矩阵

| 状态 | 展示什么 | 用户能做什么 | 文案要点 | 错误码 |
| :--- | :--- | :--- | :--- | :--- |
| 空 | 空态引导：标题 + 「选择照片文件夹」按钮 | 选择文件夹开始 | "选择照片文件夹，开始整理你的回忆" | - |
| 加载中 | 进度条 + 已处理/总数 + 剩余时间估算 + 暂停按钮 | 暂停、继续使用浏览页看已完成部分 | "正在导入 128 / 1,024 · 预计还需 2 分钟" | - |
| 成功 | 批次完成提示（短暂）后转入时间轴 | 正常浏览 | "导入完成，共 1,024 张" | - |
| 部分成功 | 完成提示 + 「N 个文件无法导入」入口 | 查看失败列表（路径 + 中文原因）、重试失败项 | "导入完成，3 个文件无法读取" | decode_failed / read_failed |
| 错误 | 文件夹无法读取提示 | 换一个文件夹 | "无法读取这个文件夹，请检查它是否被移动或删除" | read_failed |
| 离线 | 不适用（导入不依赖网络） | - | - | - |
| 权限缺失 | 目录不可读提示 | 授权或换目录 | "没有权限读取这个文件夹" | read_failed |

## 5. 数据契约

- 写命令：`import_folder(folder: String) -> ImportJobSnapshot`、`pause_import()`、`resume_import()`、`retry_failed(job_id)`
- 读查询：`list_failed_items() -> Vec<FailedItem>`
- 事件（tauri-specta Event）：`import-progress {job_id,total,done,failed,eta_seconds}`、`import-paused`、`import-resumed`、`import-finished {failed_count}`、`import-item-failed {path,error_code}`
- schema：迁移 v1（assets 表，白皮书 §4.5；`vec_assets`/`fts_text` 分别推迟至 v2/v3）
- 缩略图：`thumbs/{sha256 前 2 位}/{sha256}.webp`，经 `StorageAdapter`（LocalDiskAdapter）写入

## 6. 验收标准（Given / When / Then）

1. Given 库为空，When 导入含 3 张 JPEG + 1 张 PNG + 1 张 HEIC 的文件夹，Then 库中 5 条 ready 记录且缩略图文件存在。
2. Given 同一文件夹已导入完成，When 再次导入，Then 批次瞬间完成且库中记录数不变（done 计数 = 总数，全部标记"已存在"）。
3. Given 批次中混入损坏的 JPEG，When 导入完成，Then 该文件出现在失败列表（error_code=decode_failed），其余项全部 ready。
4. Given 导入进行到一半，When 暂停后立刻恢复，Then 继续处理剩余项且无重复资产。
5. Given 导入进行中应用被关闭，When 重新启动后再次导入同一文件夹，Then 已完成项被跳过，仅处理剩余项。
6. Given 同一内容文件以不同文件名出现在两个子目录，When 导入，Then 库中仅一条记录。
7. Given 带 EXIF Orientation 的 JPEG，When 导入，Then 缩略图方向与实际画面一致，taken_at 取自 EXIF 拍摄时间。
8. Given 无 EXIF 的文件，When 导入，Then taken_at 回落为文件修改时间。
9. 进度事件间隔 ≥100ms 节流（不淹没 IPC）。

## 7. 边界与降级

- 单文件夹 >5 万张：scan 一次性枚举入内存（文件路径列表量级可接受）；解码在 rayon 池并行。
- 磁盘满：persist/thumb 写入失败 → 该项标记 failed（error_code=write_failed），批次继续。
- 文件在处理中被移动/删除：read_failed 进失败列表。
- 0 个可识别文件：批次完成，UI 提示"这个文件夹里没有支持的照片格式"。

## 8. 开放问题

- 失败项"重试"按钮的粒度（单项 vs 全部）——先做"全部重试失败项"，单项重试 P3 打磨时视需要补。
