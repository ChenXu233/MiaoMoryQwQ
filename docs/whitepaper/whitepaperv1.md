# 多模态个人媒体语义索引引擎：技术白皮书

**版本：v0.2（产品规格与架构基线）**
**日期：2026-09-04**
**状态：现行**

> 本文件沿用 `whitepaperv1.md` 的文件名以保留既有引用；内容版本为 v0.2，取代 v0.1（v0.1 历史内容见 Git 记录与本文件附录 B 修订记录）。

---

## 0. 规范先行与文档导航

本项目处于**规范先行**阶段：先固化文档规范与决策，再启动代码。本文档与下列文档共同构成项目基线：

| 文档 | 路径 | 作用 |
| :--- | :--- | :--- |
| 项目规范（最高事实源） | `AGENT.md` | 文档/ADR/提交/工作流/质量规范 |
| 白皮书（本文） | `docs/whitepaper/whitepaperv1.md` | 产品定义、选型论证、MVP 边界、架构契约 |
| 架构决策记录 | `docs/decisions/` | ADR-0001 ~ 0007，历史不可改写 |
| 功能规格 | `docs/spec/` | 每功能：流程、状态矩阵、验收标准 |
| 体验规格 | `docs/ux/` | 延迟预算、状态矩阵、文案与无障碍标准 |
| 开发者文档 | `docs/developers/` | 环境、架构、工作流（面向开发者） |
| 用户文档 | `docs/users/` | 手册、FAQ、隐私说明（面向使用者） |

**冲突优先级**：`AGENT.md` > ADR > 白皮书 > 功能/体验规格 > 指南。

---

## 1. 产品定位与设计哲学

### 1.1 产品定义

本产品是一个**个人媒体语义索引引擎**，用于对照片、视频、音频进行语义级检索与组织。它不承担"数据湖"角色，而是定位为**胶水/感知引擎**——只负责摄取、分析、索引与检索，不存储原始媒体文件。

### 1.2 核心设计哲学："胶水哲学"

| 设计原则 | 内涵 | 用户价值 |
| :--- | :--- | :--- |
| **数据不锁定** | 索引库为可剥落的标准格式（SQLite/JSONL），不依赖本软件即可读取 | 用户数据永不被绑架，软件可被替代 |
| **存储可替换** | 通过适配器模式支持本地目录、S3 兼容对象存储、PostgreSQL 等多种后端 | 适应不同用户的运维能力与存储偏好 |
| **AI 可插拔** | 模型推理通过 ONNX Runtime 端侧执行，支持 CPU/GPU/NPU 多种加速 | 不绑定单一 AI 能力，可随生态演进 |

### 1.3 体验哲学（v0.2 新增）

1. **感知性能优先于真实性能**：任何超过 500ms 的操作必须有可见反馈；导入可暂停/恢复/错误隔离，绝不出现"假死"。
2. **状态完备**：每条主流程必须设计 空态/加载中/部分成功/错误/离线 五类状态（规格模板见 `docs/spec/`）。
3. **中文母语检索是硬约束**：语义模型必须支持中文查询（ADR-0006），不做英文中心的产品。
4. **隐私默认值**：不上报、不联网（模型下载除外）；GPS 默认不入库；一切敏感能力默认关闭。

### 1.4 目标用户

- **非运维个人用户**（MVP 优先服务）：双击即用，无需 Docker、无需命令行、无需配置
- **轻量级创作者**：本地素材库 + 公司 S3，需要"工作区"隔离不同项目（LATER）
- **技术用户**：可插拔语义索引底座，自主决定连接方式（LATER）

### 1.5 差异化定位

| 对比项 | Immich | PhotoPrism | 本产品 |
| :--- | :--- | :--- | :--- |
| 安装 | Docker/服务器 | Docker/服务器 | 单二进制，双击即用 |
| 启动 | 需服务端 | 需服务端 | < 1 秒 |
| 数据锁定 | 锁死 | 锁死 | 可剥落，哈希去重 |
| 中文语义检索 | 弱 | 弱 | 硬约束（ADR-0006） |
| 移动端 | 原生 App | 不支持 | 架构预留，MVP 后置（ADR-0003） |

---

## 2. MVP 范围与边界（v0.2 新增，依据 ADR-0003）

### 2.1 范围原则

MVP = **桌面优先 + 照片为主 + 中文语义搜索**，按垂直切片交付（ADR-0007）。

### 2.2 范围表

| 类别 | 内容 |
| :--- | :--- |
| **IN（MVP）** | 桌面端（Win/macOS/Linux）单二进制；照片导入（JPEG/PNG/WebP/HEIC，FFmpeg 解码）；SHA-256 增量扫描与去重；缩略图与时间轴/网格浏览（虚拟滚动）+ Lightbox 预览；中文图文语义搜索；元数据过滤（日期/类型）；FTS5 文件名与文本检索；混合检索融合（RRF）；导入进度（可暂停/恢复/错误隔离）；模型下载（断点续传/离线提示）；自动更新器；键盘操作与基础无障碍 |
| **OUT（MVP 不做）** | 移动端交付；S3/PostgreSQL 后端；人脸识别；插件系统/MCP 桥接；音视频语义索引（Whisper 转写）；OCR；多窗口/平板分屏 |
| **LATER（明确后续）** | 上述 OUT 项；多语言 UI（i18n）；位置搜索/地图视图；收藏与人工相册；音视频缩略图与转写；分享与导出 |

### 2.3 明确非目标（治理性约束）

- **不在 CI 建常驻性能 bench**：语义检索性能是 sqlite-vec 的职责，我们做一次性验收；模型推理速度做一次性人工基准（ADR-0004）。
- **不做种子数据生成器**（延后）：测试数据由项目所有者人工提供，无需生成图片（ADR-0004）。
- **不自研向量检索算法**：索引与检索由 sqlite-vec 承担（ADR-0002）。

---

## 3. 技术选型论证

### 3.1 应用框架：Tauri v2（桌面优先）

**选择理由**：

1. **轻量与原生**：使用系统原生 WebView（Windows WebView2 / macOS WKWebView），安装包远小于 Electron 方案，单二进制分发。
2. **一套代码多端演进**：Rust 核心不依赖任何桌面假设，UI 使用响应式 token；移动端（Android/iOS）为 LATER，不锁死路线。
3. **生态成熟**：Tauri v2 稳定，桌面端签名/更新器（tauri-plugin-updater）/WebDriver 测试（tauri-driver）均有成熟方案。

**关键技术约束**：

- UI 适配依赖 CSS 媒体查询与 `process.env.__TAURI_MOBILE__` 编译时标志（为 LATER 移动端预留）
- Rust 核心 `pub fn run()` 需添加 `#[cfg_attr(mobile, tauri::mobile_entry_point)]` 属性（预留）
- 桌面优先意味着 MVP 不承担 iOS 签名/Android 碎片化成本（ADR-0003）

### 3.2 向量索引：sqlite-vec（ADR-0002 确认保留）

**核心判断**：sqlite-vec 在 **50k 向量以下**表现极佳，超过此规模需启用 KNN 索引、分区键和量化等优化策略。索引与检索算法由数据库扩展承担，我们不维护第二套检索算法（ADR-0002 否决了"BLOB + 内存暴力扫描"方案）。

**关键优化路径**（基于官方文档与实测数据）：

| 优化项 | 配置 | 效果 |
| :--- | :--- | :--- |
| KNN 索引 | `k = ?` 原生 KNN 操作符 | 查询延迟从 8490ms 降至约 50ms（提升 190 倍） |
| 分区键 | `year` 字段声明为 partition key | 按年份物理聚簇，查询时自动过滤分区，避免全表扫描 |
| 量化 | `INT8` + 重排序 | 存储成本降低 4 倍，查询速度显著提升 |
| 页面参数 | `page_size=32768`, `mmap_size` | 减少磁盘 IO，提升数据访问速度 |

**分区键设计要点**：

- 分区键值需要有数百个向量，避免过度分片（over-sharding）导致查询变慢
- 最多支持 4 个分区键，通常 1 个足够

**已知限制与对策（v0.2 补充）**：

| 限制 | 对策 |
| :--- | :--- |
| vec0 不支持原地更新（更新 = 删除 + 重插） | 索引重建由 pipeline 幂等化处理；删除走显式 delete 流程 |
| 虚拟表只存向量，元数据必须外置 | 元数据放 `assets` 表，以 `asset_id` 对齐 |
| WAL 模式下"复制单文件备份"不成立 | 备份 = checkpoint + 复制工作区目录；写入用户文档 |
| 上游维护状态需关注 | 列入 §8 风险表；`VectorIndex` trait 保留 escape hatch（usearch / sqlite-vector / PG+pgvector） |

### 3.3 模型推理：ONNX Runtime + ort crate

**选择理由**：

1. **跨平台加速**：`ort` 支持多种执行提供程序（Execution Providers）：

| 平台 | 执行提供程序 |
| :--- | :--- |
| Windows | DirectML, XNNPACK |
| macOS | CoreML, XNNPACK |
| Linux | OpenVINO, XNNPACK |
| Android / iOS（LATER） | NNAPI / CoreML, XNNPACK |

2. **Rust 原生集成**：通过 `ort` crate 在 Rust 侧直接调用 ONNX Runtime，无需 Python 中间层。
3. **量化支持**：INT8 动态量化可缩小模型体积约 4 倍并加速 CPU 推理。

**v0.2 修正的参考数据**：

| 模型 | 用途 | 量化后大小 | 延迟参考（需一次性基准验证） |
| :--- | :--- | :--- | :--- |
| CLIP ViT-B/32 | 图片-文本语义对齐（英文基线） | ~150MB | 桌面 CPU（XNNPACK，INT8，批量）约 30ms/张；单张冷启动与移动端差异显著 |
| 中文图文模型（待选型） | MVP 实际使用（ADR-0006） | 待基准 | 待基准 |
| Whisper Tiny（LATER） | 音视频转写 | ~40MB | 桌面 INT8 接近实时；移动端更慢，不做未实测承诺 |

**部署策略**：模型不打包进安装包，首次运行自动下载到用户目录（`~/.appdata/models`），保持安装包体积可控。**注意**：ORT 原生库本身占安装包体积（每平台约 10~30MB 压缩体积），"< 50MB"目标需实测确认（§6.1）。

### 3.4 图文模型：中文优先（ADR-0006，v0.2 新增）

**问题**：CLIP ViT-B/32 的文本编码器为英文主导，中文查询命中质量明显下降，不满足 §1.3 体验哲学第 3 条。

**候选池**（排序不代表偏好，由一次性基准决定）：

| 候选 | 特点 |
| :--- | :--- |
| jina-clip-v2 | 多语言，社区活跃，量化友好 |
| BGE-Visualized-M3 | 中文原生，BAAI 出品 |
| Chinese-CLIP | 中文 CLIP 经典方案 |

**选型流程（一次性，结果记录并落 ADR）**：

1. 构造人工中文查询集（项目所有者提供真实照片与查询词）
2. 度量：中文 query 集 top-k 命中率、单 query 文本编码延迟、批量图片吞吐、INT8 量化后体积、许可证
3. 英文 CLIP ViT-B/32 作为基线对比
4. 结果写入 ADR 与 `docs/ux/latency-budget.md`，模型版本随应用版本锁定

### 3.5 存储抽象：可插拔适配器（MVP 仅单实现）

**架构模式**：Trait-Based Storage Abstraction——定义统一接口，不同后端实现适配。

**v0.2 边界**：MVP 只实现 **LocalDiskAdapter + SQLite**；S3/PgVector 为 LATER。trait 边界是"防锁死"的保险，不是当期功能，不为其写任何适配代码。

**工作区（Workspace）概念**：每个工作区是一个配置文件（TOML），指定目录、模型配置。用户可维护本地"生活工作区"等，实现数据边界隔离与可迁移。

### 3.6 媒体解码：FFmpeg（v0.2 新增）

- **缩略图生成**：FFmpeg 统一处理。
- **HEIC 解码**：iPhone 默认格式，Rust `image` crate 不支持；FFmpeg（libheif）负责解码，喂给 CLIP 的 RGB 图也统一由 FFmpeg 产出。
- **复用**：LATER 音视频缩略图与转写复用同一 FFmpeg 管线。

---

## 4. 架构设计

### 4.1 分层架构

```
┌─────────────────────────────────────────────────────┐
│  UI (React + TypeScript, 按 feature 组织)             │
├─────────────────────────────────────────────────────┤
│  契约层 (specta 生成 TS 类型; Rust 命令为唯一事实源)   │
├─────────────────────────────────────────────────────┤
│  Tauri v2 壳 (commands/events 的薄装配层)             │
├─────────────────────────────────────────────────────┤
│  crates/                                            │
│  ├── pipeline   (扫描→哈希→解码→嵌入→持久化, 可暂停)   │
│  ├── embed      (ort 封装 + 模型下载, 唯一碰 ONNX 处)  │
│  ├── store      (SQLite + sqlite-vec + FTS5 实现)     │
│  ├── platform   (路径/配置/日志/错误码)                │
│  └── core       (领域类型 + 端口 trait, 零 IO 零 async)│
├─────────────────────────────────────────────────────┤
│  端口 (trait)                                        │
│  ├── StorageAdapter   (LocalDisk / S3...)            │
│  ├── VectorIndex      (sqlite-vec / 其他...)          │
│  └── Embedder         (中文图文模型 / 其他...)         │
└─────────────────────────────────────────────────────┘
```

### 4.2 Workspace 与依赖规则（ADR-0005）

依赖方向单向：`core` 不依赖任何业务 crate；`embed/store/platform` 只被 `pipeline` 与装配层依赖；装配层在 `apps/app`。详细目录见 `docs/developers/README.md`。

端口清单（全项目仅 5~6 个，多一个都是过度设计）：

```rust
pub trait StorageAdapter { fn put(&self, key: &str, bytes: &[u8]) -> Result<()>; }
pub trait Embedder      { fn embed_images(&self, batch: &[DecodedImage]) -> Result<Vec<Vec<f32>>>; }
pub trait VectorIndex   { fn add(&mut self, id: u64, vec: &[i8]) -> Result<()>;
                          fn search(&self, q: &[f32], top_k: usize) -> Result<Vec<(u64, f32)>>; }
pub trait EventSink     { fn emit(&self, event: PipelineEvent) -> Result<()>; }
pub trait Clock / IdGen  // 可测试性
```

### 4.3 数据流（pipeline）

```
选择文件夹 → Scan（枚举 + 忽略规则）
  → Hash（SHA-256 去重）→ Decode（FFmpeg/EXIF 提取）
  → Embed（ONNX 批量推理）→ Persist（assets + vec_assets + fts_text）
  → EventSink（每阶段发进度事件 → UI 实时更新）
```

每个阶段是小转换，`tokio::mpsc` 串起；坏文件进入 failed 队列（错误隔离），job 状态可持久化（暂停/恢复）。

### 4.4 命令/查询分离与 IPC 契约（CQRS-lite）

- **写命令**：`import_folder`、`delete_assets` 等 → 返回 job id + 进度事件流。
- **读查询**：`search_assets`、`list_timeline` 等 → 返回快照。
- **契约**：Rust 命令为唯一事实源，specta 生成 `packages/contracts` 的 TS 类型；禁止手写 IPC 类型。

### 4.5 数据库设计（v0.2 更新）

```sql
PRAGMA user_version = 1;  -- 迁移机制：每版 schema 变更 +1

CREATE TABLE assets (
    asset_id INTEGER PRIMARY KEY,
    sha256 TEXT NOT NULL UNIQUE,        -- 内容哈希，跨端去重
    storage_key TEXT NOT NULL,          -- 相对路径（本地或未来 S3）
    kind TEXT NOT NULL,                 -- 'photo' | 'video' | 'audio'
    size INTEGER, width INTEGER, height INTEGER,
    taken_at INTEGER,                   -- 拍摄时间 UTC 秒；缺失时回落文件 mtime
    year TEXT NOT NULL,                 -- 分区键（拍摄年份，来自 taken_at）
    mime TEXT, exif TEXT,               -- JSON
    thumb_key TEXT,
    status TEXT NOT NULL DEFAULT 'pending',  -- pending|indexing|ready|failed
    error_code TEXT,
    imported_at INTEGER NOT NULL
);

-- 向量表（sqlite-vec vec0；元数据外置到 assets）
CREATE VIRTUAL TABLE vec_assets USING vec0(
    asset_id INTEGER PRIMARY KEY,
    year TEXT PARTITION KEY,
    embedding INT8[768]                 -- 维度随选定模型调整
);

-- 文本检索（FTS5：文件名、OCR 后文本、描述等）
CREATE VIRTUAL TABLE fts_text USING fts5(asset_id UNINDEXED, content);
```

- **备份语义**：WAL checkpoint 后复制工作区目录（db + 缩略图目录）；写进用户文档。
- **人脸表**：v0.1 的 `face_records` 移至 LATER，不在 MVP schema 中预留实现。

### 4.6 混合检索与融合（v0.2 新增）

- **语义**：sqlite-vec KNN（分区键 year）。
- **文本**：FTS5 匹配文件名/描述/未来 OCR 文本。
- **过滤**：元数据（日期区间、类型）在 SQL 层先过滤。
- **融合**：Reciprocal Rank Fusion（RRF，k≈60）合并语义与文本两条结果流；排序理由在 UI 中可解释（"命中文本/语义相似"）。

### 4.7 关键设计决策表

| 决策点 | 选择 | 理由 |
| :--- | :--- | :--- |
| 向量索引 | sqlite-vec 保留（ADR-0002） | 检索算法由数据库承担，不自研第二套 |
| MVP 平台 | 桌面优先（ADR-0003） | 交付成本可控，架构不锁死移动端 |
| 语义模型 | 中文优先，候选池待一次性基准（ADR-0006） | 中文检索是硬约束 |
| 索引存储 | 单一 SQLite + 缩略图目录 | 数据可移植；备份 = checkpoint + 复制目录 |
| 并行处理 | Rust rayon + tokio | 多核 CPU，异步 IO 不阻塞主线程 |
| 哈希去重 | SHA-256 为主键 | 跨端移动零重复向量化 |
| IPC 契约 | specta 生成 TS 类型 | 类型漂移归零 |
| 性能验证 | 一次性人工基准，非常驻 CI（ADR-0004） | 语义检索是数据库职责；模型速度一次性检查 |

---

## 5. 体验规格（v0.2 新增，摘要）

完整标准见 `docs/ux/README.md` 与 `docs/ux/latency-budget.md`。功能规格模板见 `docs/spec/`。

### 5.1 主链路清单

| 链路 | 关键体验要求 |
| :--- | :--- |
| 首次启动 | 空态引导：选择文件夹即开始导入；无需任何配置 |
| 导入 | 进度 = 绝对数量 + 剩余时间估算；可暂停/恢复；坏文件隔离不阻塞 |
| 语义搜索 | 键入即搜（防抖）；首屏 < 150ms（P50）；结果流式补全；排序可解释 |
| 浏览/预览 | 虚拟滚动 60fps；Lightbox 键盘导航（←/→/Esc） |
| 模型下载 | 断点续传 + 失败重试 + 离线提示；下载完成前搜索给出明确降级说明 |

### 5.2 状态矩阵原则

每条主流程 × 七状态：**空 / 加载中 / 成功 / 部分成功 / 错误 / 离线 / 权限缺失**；每状态写明：展示什么、用户能做什么、文案、错误码。模板见 `docs/spec/_template.md`。

### 5.3 延迟预算（摘要）

| 交互 | 目标 |
| :--- | :--- |
| 应用冷启动（桌面，不含首次模型下载） | < 1s |
| 搜索框输入反馈 | < 100ms |
| 语义搜索首屏（≤5 万向量，桌面） | P50 < 150ms，P95 < 500ms |
| 缩略图可见即出 | < 250ms |
| 导入吞吐（桌面 CPU 批量） | > 50 张/s |

完整预算与一次性验收方法见 `docs/ux/latency-budget.md`（ADR-0004：性能验收一次性人工执行，不进常驻 CI）。

### 5.4 键盘与无障碍最低线

焦点可见、全键盘可达（搜索/导航/预览）、Esc 可关闭一切浮层、对比度达标、文案不裸技术词。

### 5.5 隐私默认值

不上报、不联网（模型下载除外）、GPS 默认不入库、缩略图仅存本地工作区；隐私说明进用户文档 `docs/users/`。

---

## 6. 性能目标与验证策略（v0.2 重写，依据 ADR-0004）

### 6.1 性能目标

| 指标 | 目标值 | 验证方式 |
| :--- | :--- | :--- |
| 冷启动 | < 1 秒 | 人工实测（桌面三平台） |
| 语义搜索 | P50 < 150ms / P95 < 500ms（≤5 万向量） | 一次性人工验收（sqlite-vec 职责） |
| 索引速度 | > 50 张/s（桌面 CPU 批量） | 一次性人工基准 |
| 模型推理（中文图文模型） | 待选型基准后写入 | 一次性人工基准，记录于 ADR-0006 |
| 空闲内存 | < 200MB | 人工实测 |
| 安装包体积 | < 50MB（含 ORT，待实测，可能上浮） | 打包后测量 |

### 6.2 验证策略（关键约束）

1. **一次性人工基准**：模型推理速度、sqlite-vec 目标规模验收，均一次性执行，结果记录进 `docs/ux/latency-budget.md`；**模型/ORT/sqlite-vec/Tauri 大版本升级时触发重验**。
2. **CI 常驻内容**：格式/lint/clippy/tsc/单元测试/契约生成一致性/e2e 体验流程（小规模固定数据集，由项目所有者人工提供）。
3. **明确不做**：常驻万级合成照片 bench（语义检索性能是数据库的职责）；合成种子数据生成器（延后）。

### 6.3 规模演进策略

| 数据量 | 索引方案 | 预期性能 |
| :--- | :--- | :--- |
| < 50k 向量 | sqlite-vec 精确扫描 | 良好（无需优化） |
| 50k - 500k | sqlite-vec + KNN 索引 + 分区键 | 优秀（<100ms） |
| > 500k | 升级到 PostgreSQL + HNSW（适配器切换） | 需切换 |

**架构预留**：`VectorIndex` trait 保持，初期实现为 sqlite-vec；escape hatch 候选：usearch、sqlite-vector（社区延续版）、PG+pgvector。

---

## 7. 移动端规划（LATER，ADR-0003）

MVP 不交付移动端，但架构不锁死：

- 平台要求参考：Android minSdk 24（Android 7+），iOS/iPadOS 9+
- UI 预留：CSS 媒体查询、`process.env.__TAURI_MOBILE__` 编译时标志、`env(safe-area-inset-*)`
- Rust 入口预留 `#[cfg_attr(mobile, tauri::mobile_entry_point)]`
- 现实成本（启动移动端前必须评估）：iOS 需 Mac + Xcode + 付费开发者账号；Android WebView 碎片化；ONNX 移动端推理性能需重验（§3.3 修正说明）

---

## 8. 风险分析与应对（v0.2 更新）

| 风险 | 等级 | 应对策略 |
| :--- | :--- | :--- |
| sqlite-vec 上游维护放缓 | 中 | 版本锁定 + 每次发版复查上游动态；`VectorIndex` trait 保留 escape hatch（usearch / sqlite-vector） |
| 中文图文模型质量不达标 | 中 | ADR-0006 一次性基准提前验证；候选池三个方案；必要时回退"英文模型 + 查询翻译层" |
| HEIC 解码失败（个别文件） | 中 | FFmpeg 解码 + libheif 兜底；失败文件进入 failed 队列并给出可读原因 |
| ORT 体积挤占安装包预算 | 中 | 打包实测；必要时精简 EP feature；模型已外置 |
| 首次模型下载体验差 | 中 | 断点续传 + 离线提示 + 下载完成前降级说明（§5.1） |
| 冷启动随库增大退化 | 低 | 懒加载缩略图、mmap、避免启动时全量读库 |
| 移动端推迟的市场影响 | 低 | 桌面先行验证核心价值；架构预留降低后续成本 |
| Android WebView 碎片化（LATER） | 低 | 启动时检测 WebView 版本，过低提示更新或降级 UI |

---

## 9. 结论

v0.2 将白皮书从"可行性论证"升级为"产品规格 + 架构契约 + 交付基线"：MVP 收敛为**桌面 + 照片 + 中文语义搜索**（ADR-0003），向量检索交给 sqlite-vec（ADR-0002），中文模型选型以一次性基准落定（ADR-0006），性能验收一次性人工执行、CI 只做确定性检查（ADR-0004），交付按垂直切片推进（ADR-0007）。

产品差异化在于：**架构哲学（胶水/感知引擎）、用户体验（双击即用/秒级启动/中文优先/状态完备）、数据可移植性（哈希去重/可剥落存储）**三重组合。

---

## 附录 A：环境配置要求

### Rust
- 版本：1.75+（推荐 1.80+）
- 工具链：`rustup` + `cargo`

### Tauri v2
- CLI：`cargo install tauri-cli --version "^2"`
- 桌面端：WebView2（Windows）/ WKWebView（macOS）

### ONNX Runtime
- `ort` crate 版本 1.14+；执行提供程序按需启用 Cargo features

### 前端
- Node.js 18+；React 18+ + TypeScript；Tailwind CSS；依赖克制清单见 `AGENT.md`

### 媒体
- FFmpeg（缩略图/HEIC 解码）

---

## 附录 B：修订记录

| 版本 | 日期 | 变更 |
| :--- | :--- | :--- |
| v0.1 | 2026-09-04 | 初版：技术可行性论证（Tauri v2 / sqlite-vec / ort 选型） |
| v0.2 | 2026-09-04 | 新增 MVP 边界（ADR-0003）、中文模型硬约束（ADR-0006）、体验规格（§5）；重写验证策略（§6，一次性基准，非常驻 CI）；确认保留 sqlite-vec（ADR-0002）；修正 CLIP/Whisper 延迟参考、安装包体积、WAL 备份语义；新增 FFmpeg/HEIC、FTS5、RRF、CQRS-lite、specta 契约；移动端移至 LATER |
