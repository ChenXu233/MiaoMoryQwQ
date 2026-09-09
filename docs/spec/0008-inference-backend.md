# 规格 0008：推理后端与运行时分发

- 状态：已评审（所有者 2026-09-09 逐条裁定：默认 CPU 最小配置、设置内自由选择、应用内一键下载或本地导入；本规格随附冻结）
- 负责人：ZCode（AI 代理）
- 关联切片：P7（推理自由化）
- 关联规格：spec 0003（语义搜索——EP 的消费方）、spec 0004（模型下载——复用其下载器模式）、spec 0006（设置页 IA——本功能的宿主卡）
- 关联 ADR：ADR-0014、ADR-0006（模型硬约束）、ADR-0011（数据布局：runtime 目录随数据根）
- 最后更新：2026-09-09

## 1. 目标与非目标

**目标**：语义索引的推理执行提供者（EP）可由用户在设置中选择——`CPU`（默认，零依赖）/ `DirectML`（Windows 自带组件，零下载）/ `CUDA`（N 卡专用，应用内一键下载运行时包或本地导入 zip）；所选 EP 不可用时自动降级 CPU 且降级原因在设置页可见；模型文件支持从本地目录/zip 导入（按内置清单校验），兑现 ADR-0013 预留的「用户自带模型」边界。

**非目标**：

- EP 热切换——onnxruntime 动态库是进程级一次性加载（变体文件决定可用 EP 集合），切换**重启生效**（与「更改数据位置」同先例）
- TensorRT EP（GB 级运行时 + 按机建引擎，对分钟级后台索引不值，ADR-0014 备选表否决）
- fp16 模型变体分发（开放问题 §8，待模型侧产出并实测）
- 文本塔/视觉塔分塔配置 EP（一律同 EP，简化交互）
- macOS CoreML 可选化（维持编译期 CoreML→CPU 现状）

## 2. 用户故事

- 作为无独显用户，我装好应用什么都不配就能建索引（CPU 默认），不被任何驱动/运行时门槛拦住。
- 作为 N 卡用户，我在设置里看到 CUDA 被标记「推荐」，一键下载运行时包（断点续传），重启后建索引快一个量级。
- 作为离线/高级用户，我自己下载的模型包和运行时 zip 能从本地导入，校验通过即可用，不依赖在线分发。

## 3. 流程

### 3.0 设置页信息架构（宿主，详见 spec 0006）

「推理加速」卡位于设置页**模型与推理**分类下，与「语义模型」卡同组。

### 3.1 EP 解析与装配（每次启动一次）

1. 读 `config.toml` 的 `inference_ep`（缺省 = `cpu`）。
2. 解析动态库路径（Windows）：`<数据根>\runtime\<ep>\onnxruntime.dll`（用户下载/导入的变体）→ 未命中则用应用自带的 `runtime\dml\onnxruntime.dll`（安装包随附 DML 变体，CPU EP 也经由它）。应用在装配任何 session 之前显式初始化 ort 并校验 dylib 可加载。
3. 按 EP 装配 indexer（`ClipEmbedder::load` 透传 `EpKind`）：`cpu` 仅注册 CPU EP；`directml` 注册 DirectML→CPU shadowing；`cuda` 注册 CUDA→DirectML→CPU shadowing。
4. **降级**：所选 EP 初始化/加载失败（驱动过旧、缺运行时、非 N 卡等）→ 自动以 CPU 完成装配，**降级原因**记入 `inference_info` 与日志；**不改写 config**（用户选择保留，修好环境重启即生效）。
5. CUDA 设备枚举只认硬件适配器（NVIDIA/AMD/Intel 显卡），跳过 IddCx 虚拟显示适配器（Oray/Todesk 等），避免选中无算力设备。

### 3.2 设置：切换推理后端

前置：设置页 → 模型与推理 → 推理加速卡。

1. 分段按钮三选（macOS 显示 CoreML 占位）：每项附一句话代价说明（CPU=零依赖最慢；DirectML=Windows 自带零下载；CUDA=下载一次运行时最快，检测到 N 卡时标「推荐」）。
2. 选择即写入 `config.toml`（复用 spec 0006 的 config 读改写路径），UI 提示「重启应用后生效」。
3. CUDA 未就绪时选择 CUDA：允许写入，卡内提示「还需下载运行时包」，展示下载入口。

### 3.3 CUDA 运行时一键下载

1. 内置运行时清单（与模型清单同格式：zip 文件名 + SHA-256 + 字节数 + 解压后文件清单）。
2. 复用模型下载器（spec 0004 模式）：主源 + 可配置分发源、`.part` 断点续传、SHA-256 校验、防重入。
3. 校验通过 → 解压到 `<数据根>\runtime\cuda\` → 逐文件比对清单 → 就绪态。
4. 失败：保留 `.part` 与错误明细，可重试；zip 校验失败删除重下（最多 3 次）。

### 3.4 运行时本地导入

选本地 zip → SHA-256 对清单校验 → 通过则走 3.3 第 3 步落地；不匹配返回文件名与原因明细。允许导入清单外 zip（高级场景：自带运行时）——跳过 sha 但仍校验解压结构与关键 dll 存在，就绪态标注「未校验来源」。

### 3.5 模型本地导入（ADR-0013 边界兑现）

1. 「语义模型」卡「从本地导入…」→ 选择目录或 zip。
2. 按内置模型清单逐文件校验（流式 SHA-256）；zip 先解压到临时目录再校验。
3. 全部匹配 → 落入 `model_dir`（已存在的同内容文件跳过）→ 触发 indexer 重装配 → `models-imported` 事件。
4. 缺失/不匹配 → 返回明细列表（文件名 + 原因），不落任何文件。

### 3.6 GPU 检测

启动时一次 + 设置页打开时刷新：WMI `Win32_VideoController` 适配器名匹配独立显卡特征（GeForce/RTX/GTX/Radeon RX），仅用于「推荐」标记与文案，**不作为功能开关**（检测结果永远允许手选，误判由 3.1 降级兜底）。

## 4. 状态矩阵

**推理加速卡**：

| 状态 | 展示什么 | 用户能做什么 | 文案要点 | 错误码 |
| :--- | :--- | :--- | :--- | :--- |
| 空 | 不适用（卡常驻） | — | — | — |
| 加载中 | 不适用（EP 解析在启动完成） | — | — | — |
| 成功 | 当前生效 EP + 三选项分段钮（推荐徽标） | 切换（写 config + 重启生效提示） | 「当前生效：DirectML（重启应用后切换到 CUDA）」 | — |
| 部分成功 | 降级横幅：所选 EP 与实际生效 EP 不一致 + 原因 | 按原因处理（下载运行时/升级驱动）或改选 | 「CUDA 启动失败（缺运行时包），已用 CPU 继续索引」 | `EP_DEGRADED`（展示用） |
| 错误 | 下载/导入错误明细 + 重试 | 重试 / 本地导入 | 「运行时包校验失败，已保留已下载部分」 | `RUNTIME_DOWNLOAD_FAILED` |
| 离线 | 同错误（网络不可达） | 本地导入 / 稍后重试 | 「网络不可用，可从本地导入运行时 zip」 | `RUNTIME_DOWNLOAD_FAILED` |
| 权限缺失 | 写入失败提示 | 以正常权限重装/改数据位置 | 「runtime 目录无法写入」 | `WRITE_FAILED` |

**模型本地导入**：成功 = 「已导入 N 个文件，索引已恢复」；部分成功 = 明细列出缺失/不匹配文件；错误 = zip 损坏（`IMPORT_INVALID_ARCHIVE`）；权限缺失同上表。

## 5. 数据契约

**配置**（`mm-platform::AppConfig` 新增，缺省 None = cpu）：

```toml
inference_ep = "cuda"   # cpu | directml | cuda
```

**EP 枚举**（`mm-embed::EpKind`，mm-core 不依赖 ort）：

```rust
pub enum EpKind { Cpu, DirectML, Cuda }
// FromStr/Display；#[cfg] 平台可用性：DirectML 仅 windows，Cuda 仅 windows（首期）
```

**命令**（specta 契约，i64 禁出，量级小用 i32/u32）：

```rust
fn inference_info() -> Result<InferenceInfo, ErrorCode>;
// InferenceInfo { current_ep: String, effective_ep: String, degraded_reason: Option<String>,
//                 options: Vec<EpOption { kind: String, available: bool, recommended: bool, hint: String }>,
//                 runtime_ready: bool, gpu_detected: bool }
fn set_inference_ep(ep: String) -> Result<(), ErrorCode>;        // 写 config，重启生效
fn download_runtime() -> Result<(), ErrorCode>;                   // 幂等防重入
fn import_runtime(path: String) -> Result<(), ErrorCode>;         // zip 校验解压
fn import_models(path: String) -> Result<ImportReport, ErrorCode>;
// ImportReport { imported: u32, skipped: u32, mismatched: Vec<ModelFileIssue { name, reason }> }
```

**事件**：`RuntimeDownloadProgressEvent { received, total }`、`RuntimeReadyEvent {}`、`ModelsImportedEvent {}`。

**目录**：`<数据根>\runtime\{dml,cuda}\`（ADR-0011 数据根布局新增成员，随口袋目录携带）。

## 6. 验收标准（Given / When / Then）

1. Given 全新安装（无 config），When 启动并 `inference_info`，Then `current_ep=cpu` 且索引正常（慢），无任何下载强制项。
2. Given 选 DirectML，When 确认，Then config 写入且 UI 提示重启生效；重启后 `effective_ep=directml`。
3. Given 选 CUDA 但 runtime 目录为空，When 启动，Then 降级 CPU、`degraded_reason` 非空、设置卡显示降级横幅，config 仍为 cuda。
4. Given 下载中断（进程被杀），When 再次 download_runtime，Then 从 `.part` 断点继续。
5. Given 运行时 zip 被篡改，When 下载/导入，Then sha 校验失败重下（下载）或明细报错（导入），不落地坏文件。
6. Given 模型 zip 缺一个文件，When import_models，Then 返回明细（缺失文件名），model_dir 不变。
7. Given 模型 zip 完整，When import_models，Then 文件落入 model_dir、重装配成功、事件广播。
8. Given 无 N 卡机器（或仅虚拟显卡），When 打开设置，Then CUDA 项不标推荐（检测为假），但可手选并由 3.1 兜底降级。
9. Given 下载进行中再点下载，Then 幂等返回（防重入）。

## 7. 边界与降级

- **虚拟显示适配器**（Oray/Todesk IddCx）：设备枚举跳过（3.1 第 5 条）；实测 2026-09-09 DML 默认命中真实显卡（RTX 4070 Laptop）。
- **驱动过旧**：CUDA session 初始化失败 → 降级 CPU + `degraded_reason`（文案含"更新 NVIDIA 驱动后重启可启用"）。
- **磁盘满**：解压失败保留 zip 与 `.part`，报 `WRITE_FAILED`，提示清理后重试。
- **运行时包与 onnxruntime 版本**：清单随应用版本锁定（同模型策略 ADR-0006）；CUDA 包内 cuDNN/cuBLAS 版本由 onnxruntime 官方包决定，清单记录整体 sha。
- **便携拷贝**：runtime 目录随数据根携带；目标机器无 N 卡时按 3.1 降级，不报错。
- **实测基线**（2026-09-09，233 张 JPEG@4070 Laptop，release 构建）：CPU EP 与 DML+int8 的对照、批 4/8/16 曲线已在 ADR-0014 记录；CUDA 行待路径落地后回填。

## 8. 开放问题

- fp16 模型变体（非量化）进模型清单：需模型侧产出 fp16 onnx 并实测 DML 提速（预期消除 int8 QDQ 算子回退），产出后评估分发体积。
- CUDA 运行时包体积与切分（整包 vs 按驱动档位分包）待首包发布时定。
- CoreML 可选化（mac）与 Linux EP（.isNull 探测后议）。
