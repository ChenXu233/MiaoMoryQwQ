# 规格 0006：设置页（信息架构与数据位置）

- 状态：已评审（2026-09-09 所有者裁定设置页二级分类导航，本修订随附；数据位置部分沿用 2026-09-06 冻结稿）
- 负责人：ZCode（AI 代理）
- 关联切片：P4（口袋式数据布局）、P7（IA 修订）
- 关联体验规格：`docs/ux/0006-settings.md`
- 关联 ADR：ADR-0011、ADR-0014（推理加速卡）
- 最后更新：2026-09-09

## 1. 目标与非目标

**目标**：全部用户数据（索引库、缩略图、模型、运行时、日志、配置）按 ADR-0011 四级优先解析存放位置，默认"口袋式"（安装目录 `data\` 即全部数据）；设置入口提供查看当前位置、打开数据文件夹、更改数据位置（写配置重启生效）。设置页采用**二级分类导航**（2026-09-09 所有者裁定：分门别类，避免卡片平铺随功能增长失控）。

**非目标**：

- 应用内"搬迁数据"（自动复制 `index.db` + `thumbs\` 到新位置）——后续切片
- 多工作区管理 UI——LATER（白皮书 §3.5）
- 配置文件的可视化编辑（模型分发源等高级字段仍走 config.toml 手改）
- 三级导航与设置项搜索（卡片数增长到失控再议）

## 2. 用户故事

作为便携用户，我想把整个安装目录拷到 U 盘或另一台机器直接使用；作为普通用户，我想在设置里按类目快速找到要改的东西，而不是在一长列卡片里翻找。

## 3. 流程

### 3.0 设置页信息架构（2026-09-09 修订）

- **结构**：设置页内左侧竖排分类导航 + 右侧内容面板；窗口宽 <720px 时导航收为顶部横排（纯 CSS 切换）。
- **分类**（卡片归属）：

| 分类 | 卡片 |
| :--- | :--- |
| 外观 | 主题、语言（占位） |
| 模型与推理 | 语义模型（状态/下载/进度/本地导入）、推理加速（EP 选择/运行时下载，spec 0008） |
| 数据与存储 | 数据位置、来源文件夹、存储占用、无法导入的文件 |

- **行为**：页内 state 切换（不进 hash 路由）；上次停留分类持久化 `localStorage["mm-settings-section"]`（先例 `mm-rail`）；默认落点「外观」。
- **实现**：`SettingsPage.vue` 为壳，三个 Section 组件（`components/settings/{Appearance,ModelIndex,Data}Section.vue`）；导航样式走 `styles.css` + tokens 变量。

### 3.1 启动布局解析（每次启动一次，单点在 `mm-platform`）

1. 读环境变量 `MIAOMORY_DATA_DIR`：非空 → 根目录 = 该值（数据根布局），结束。
2. `<exe目录>\data\config.toml` 存在，或 `<exe目录>\.portable` 存在 → 显式便携：根目录 = `<exe目录>\data\`；若该 config 内 `data_dir` 有值则覆盖为该值，结束。
3. exe 目录可写探测（创建并删除临时文件）：成功 **且** AppData 无既有配置（`%APPDATA%\MiaoMory\config.toml` 不存在）→ 自动便携：根目录 = `<exe目录>\data\`（ensure 布局后写入初始 config），结束。
4. 回退经典布局：工作区 = `文档\MiaoMory`，模型/配置/日志 = `%APPDATA%\MiaoMory`；若经典 config 的 `data_dir` 有值 → 改用数据根布局（根 = `data_dir`）。
5. 数据根布局统一落位：`<根>\index.db`、`<根>\thumbs\`、`<根>\models\`、`<根>\runtime\`、`<根>\logs\`、`<根>\config.toml`。

### 3.2 设置：更改数据位置

前置：应用已启动，设置页打开。

1. 用户点「更改数据位置…」→ 系统目录选择器。
2. 取消 → 无变更。
3. 选定目录 D → 校验 D 可写（创建删除临时文件）；不可写 → 错误提示，停留在步骤 1。
4. 可写 → 将 `data_dir = "D"` 写入**当前生效的** config 文件（经典模式写 AppData config；便携模式写 `<exe目录>\data\config.toml`；env 模式下更改入口禁用）。
5. UI 提示"重启后生效"，记录待生效状态；本次会话继续用旧位置，不部分搬家。

### 3.3 设置：打开数据文件夹

用系统文件管理器打开当前数据目录（模型目录与索引所在处的共同父目录；经典模式打开工作区目录）。失败（目录已被删）→ 重建后重试一次，再失败则报错。

## 4. 状态矩阵

设置页导航：无数据/加载/错误态（纯前端结构切换）；分类记忆读写失败静默降级为默认落点「外观」。

数据位置卡：

| 状态 | 展示什么 | 用户能做什么 | 文案要点 | 错误码 |
| :--- | :--- | :--- | :--- | :--- |
| 空 | 不适用（卡常驻） | — | — | — |
| 加载中 | 不适用（路径解析在启动完成，同步返回） | — | — | — |
| 成功 | 模式徽标（口袋式/标准）+ 各数据路径 + 「打开数据文件夹」「更改数据位置…」 | 查看/打开/更改 | 「数据全部保存在本地」 | — |
| 部分成功 | 不适用 | — | — | — |
| 错误 | 错误行内提示，原路径保留 | 重选目录 / 关闭 | 「该文件夹无法写入，请选择有写入权限的位置」 | `CONFIG_WRITE_FAILED` |
| 离线 | 与离线无关（全部本地） | 同成功 | — | — |
| 权限缺失 | 同错误状态 | 同错误 | 同错误 | `DATA_DIR_NOT_WRITABLE` |

启动布局解析失败（极端：文档目录与 AppData 均不可用）：应用进入错误空态，提示数据目录不可用，附错误码。

## 5. 数据契约

**配置 schema**（`mm-platform`）：

```toml
# config.toml 新增字段（缺省 None，向后兼容）
data_dir = "D:\\MiaoMoryData"      # 数据根覆盖；设置里"更改数据位置"写入
inference_ep = "cuda"              # 推理后端（spec 0008）：cpu | directml | cuda；缺省 cpu
```

**命令**（specta 契约，Rust 为唯一事实源）：

```rust
fn data_info() -> Result<DataInfo, ErrorCode>;
// DataInfo { mode: DataMode /* env | portable | classic | rooted */,
//            db_path, thumbs_dir, models_dir, logs_dir: String,
//            config_path: String, can_change: bool }
fn open_data_folder() -> Result<(), ErrorCode>;
fn set_data_location(dir: String) -> Result<(), ErrorCode>; // 写 config.data_dir
// 推理后端命令见 spec 0008 §5
```

**事件**：无（数据位置域）。**schema 迁移**：无（`PRAGMA user_version` 不受影响）。

## 6. 验收标准（Given / When / Then）

1. Given 未设 env、无标记、目录可写、无旧配置，When 启动，Then 数据落在 `<exe目录>\data\` 且生成初始 config。
2. Given `%APPDATA%\MiaoMory\config.toml` 已存在（老用户），When 升级后启动，Then 布局与升级前一致（零破坏）。
3. Given exe 在只读目录且无旧配置，When 启动，Then 采用经典布局且首界面提示数据位置。
4. Given `MIAOMORY_DATA_DIR=D:\x`，When 启动，Then 全部数据落在 `D:\x`（优先级最高，覆盖便携判定）。
5. Given `<exe目录>\.portable` 存在，When 启动，Then 即使目录判定可写与否均采用 `<exe目录>\data\`（不可写时启动失败并给出可读错误）。
6. Given 设置里选定可写目录 D，When 确认，Then 当前 config 写入 `data_dir=D` 且 UI 提示重启生效；重启后 `data_info` 指向 D。
7. Given 选定目录不可写，When 确认，Then 报 `DATA_DIR_NOT_WRITABLE`，config 不变。
8. Given NSIS 卸载（spike 已核），When 卸载完成，Then `<安装目录>\data\` 保留。
9. Given config.toml 损坏，When 启动，Then 回退默认布局 + 日志记录解析失败，应用可正常使用。
10. Given 窗口宽 ≥720px，When 打开设置页，Then 左侧竖排分类导航 + 右侧面板；<720px 时导航为顶部横排。
11. Given 用户切到「数据与存储」后重启应用，When 再开设置页，Then 默认落点为「数据与存储」。
12. Given localStorage 被清空/值非法，When 打开设置页，Then 落点「外观」，无报错。

## 7. 边界与降级

- **磁盘满**：写入 config / 初始布局失败按错误状态呈现，错误码 `CONFIG_WRITE_FAILED`。
- **路径含中文/空格/UNC**：一律按 `PathBuf` 传递，TOML 序列化转义；禁止拼接字符串路径。
- **便携目录被整体拷贝到另一台机器**：模型随目录携带可直接用；若缺模型走既有下载引导（spec 0004）；运行时同理（spec 0008）。
- **双实例同时跑**：SQLite WAL 允许多读单写；不在本规格扩展（与现状一致）。
- **env 与标记同时存在**：env 胜出（级别最高）。
- **新分类卡增长规则**：新增设置卡必须归入既有三分类之一；出现第四分类前先修订本规格 IA 表。

## 8. 开放问题

- 「重启生效」能否升级为「立即切换」（应用内搬迁 + 热重载连接）？涉及连接池重建与导入任务互斥，放后续切片。
- macOS/Linux 的便携语义（.app bundle 内不可写，需 `.portable` 放 bundle 旁）待对应平台打包时细化。
