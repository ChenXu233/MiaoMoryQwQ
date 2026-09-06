# ADR-0011：口袋式数据布局（默认随安装目录，可设置迁移）

- 状态：Accepted（项目所有者 2026-09-06 批准）
- 日期：2026-09-06
- 决策者：项目所有者
- 相关：白皮书 §1.2（数据不锁定）、§3.5（工作区）；ADR-0003、ADR-0010

## 背景

所有者提出：默认情况下全部数据（索引库、缩略图、模型、配置、日志）直接放在安装目录下，应用呈"口袋式/便携式"——整个安装文件夹拷走即完整可用；同时提供设置项允许把数据放到指定位置。

现行布局（ADR 之前的设计）：工作区（`index.db` + `thumbs/`）默认在「文档\MiaoMory」，模型/配置/日志在 `%APPDATA%\MiaoMory`。路径已集中在 `mm-platform` 单点解析，改动面可控。

Windows 硬约束：

1. **Program Files 不可写**：MSI 默认 perMachine 安装到 Program Files，普通用户无写权限；UAC 虚拟化会造成静默重定向，绝不能依赖。
2. **NSIS 默认按用户安装**：`setup.exe` 装到 `%LOCALAPPDATA%\Programs\MiaoMory`，该目录**可写**，天然适合口袋式。
3. **卸载/升级的数据安全**：数据放进安装目录后，卸载器与 MSI major upgrade 是否保留数据目录**未验证**，必须 spike 实测后才可默认启用；tauri-plugin-updater 只替换应用文件，数据不受影响。
4. **dev 模式**：`tauri dev` 的"安装目录"是 `target/debug`，`cargo clean` 会连同数据一起清掉，开发环境必须走显式覆盖。

## 决策

我们决定：

1. **路径解析改为四级优先**（仍集中于 `mm-platform` 单点）：
   1. 环境变量 `MIAOMORY_DATA_DIR`（开发/测试/高级用户，最高优先）
   2. **便携模式（显式）**：`<exe目录>\data\config.toml` 存在，或存在 `<exe目录>\.portable` 标记 → 全部数据落在 `<exe目录>\data\`（`index.db`、`thumbs\`、`models\`、`logs\`、`config.toml`）
   3. **便携模式（自动）**：首次运行（无既有 AppData 配置）且 exe 目录可写（实测创建临时文件验证）→ 自动采用 `<exe目录>\data\` 布局
   4. **回退（现状布局）**：目录不可写（Program Files）或检测到既有安装的旧配置 → 维持「文档工作区 + AppData 模型」，并在首界面提示数据实际位置
2. **口袋语义**：便携模式下 `data\` 一个文件夹就是全部用户数据；换机器 = 拷贝安装目录。模型也进 `data\models\`（接受换机需重新下载模型一次，或整目录一起拷）。
3. **设置入口（新增最小设置）**：显示当前数据位置 + 「打开数据文件夹」+ 「更改数据位置」（写入解析到的 config 并提示重启生效）。带数据自动搬迁（复制 `index.db` + `thumbs`）的完整迁移体验放后续切片。
4. **既有安装零破坏**：AppData 已有配置的用户（含 alpha 0.1.0 升级）保持旧布局不变；不主动迁移，提供一次性迁移提示（后续切片）。
5. **安装包策略**：Windows 主推 NSIS 按用户包（可写 → 自动口袋式）；MSI 面向需要 perMachine 的场景，自动落入回退布局。`deleteAppDataOnUninstall` 保持 false。
6. **前置 spike（默认启用自动口袋式的闸门）——已完成（2026-09-06），闸门通过**：核实 tauri-bundler 源码（dev 分支）。NSIS 卸载段（`installer.nsi` §Uninstall）仅 `Delete` 安装时登记的文件并对目录做**非递归** `RMDir`，非空 `data\` 令 `RMDir "$INSTDIR"` 失败即保留；"Delete app data" 复选框默认关闭、勾选后也只递归删 `%APPDATA%/%LOCALAPPDATA%` 的 bundle-id 目录，不碰安装目录。MSI（`main.wxs`）`MajorUpgrade Schedule="afterInstallInitialize"` 仅卸载旧产品登记的组件，`RemoveFolder On="uninstall"` 只删空目录。**结论：自动口袋式对全部可写安装场景默认启用；更新器只替换应用文件，数据不受影响。**

## 后果

### 积极

- 与「数据不锁定/可剥落」哲学完全一致：口袋式是可剥落的极致形态
- 备份语义简化为「拷一个文件夹」，用户心智负担最低
- U 盘/同步盘场景开箱即用，差异化于 Immich/PhotoPrism 的服务器绑定

### 消极 / 需关注

- 卸载器与 MSI 升级可能威胁安装目录内数据（spike 闸门前不默认启用）
- 便携模式下模型随目录走，重装系统若只备份数据不备份模型需重新下载
- 路径解析分支增多，需 TDD 覆盖四级优先的组合用例

## 备选方案

| 方案 | 结论 | 原因 |
| :--- | :--- | :--- |
| 仅 `.portable` 标记文件手动开关 | 否 | 默认仍是用户目录，不满足"默认口袋式"诉求 |
| 一律安装目录，不支持回退 | 否 | Program Files 不可写，MSI 用户直接不可用 |
| 维持现状 + 仅 config.toml 手改 | 否 | 早已支持但不可发现，也无口袋语义 |

## 附：目标布局详图

**口袋模式**（自动或显式；示例为 NSIS 默认位置）：

```
<安装目录>\                              如 %LOCALAPPDATA%\Programs\MiaoMory
├── MiaoMory.exe                ┐ 应用文件：卸载/升级归安装器管，
├── *.dll、resources\           │ 永远不触碰 data\
├── .portable                   ← 可选空标记文件，手动强制便携（自动模式不产生）
└── data\                       ┐
    ├── config.toml             │
    ├── index.db (+wal/-shm)    │
    ├── thumbs\ab\{sha256}.webp │ ← 全部用户数据；换机 = 整目录拷走
    ├── models\                 │   visual.int8.onnx / text.int8.onnx / vocab.txt
    └── logs\miaomory.log.YYYY-MM-DD
```

**回退模式**（Program Files 不可写或检测到既有旧配置；与现行布局一致）：

```
<Program Files>\MiaoMory\             仅应用文件（只读）
<文档>\MiaoMory\                      index.db + thumbs\（工作区）
<AppData\Roaming>\MiaoMory\           config.toml + models\ + logs\
```

dev 模式经 `MIAOMORY_DATA_DIR` 指向仓库内忽略目录（如 `.devdata`），避免数据落入 `target\` 被 `cargo clean` 清除。缩略图分桶、模型文件名、日志滚动规则均为相对结构，实现仅替换解析根，不改存储逻辑。
