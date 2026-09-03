# 00 环境准备（Setup）

状态：现行
维护者：项目所有者
最后更新：2026-09-04

## 什么

MiaoMory 开发环境所需的全部工具链与原生库。

## 为什么

Windows 上的原生构建（libheif）与 Tauri 打包都有前置条件，缺一项就构建失败；本页是唯一权威清单。

## 怎么做

### 基础工具链

| 工具 | 版本要求 | 安装 |
| :--- | :--- | :--- |
| Rust | 1.80+（当前开发机 1.97.1） | [rustup](https://rustup.rs) |
| Node.js | 22 LTS | 官网或 nvm-windows |
| pnpm | 10.x | `corepack enable` 或独立安装 |
| Git | 任意新版 | 官网 |

### Tauri 桌面前置

- Windows：WebView2 运行时（Win11 自带）
- 首次构建验证：`pnpm --filter miaomory-app tauri dev`

### libheif（HEIC 解码，ADR-0008）

Windows 经 vcpkg 提供原生库：

```powershell
git clone --depth 1 https://github.com/microsoft/vcpkg.git "$HOME\vcpkg"
& "$HOME\vcpkg\bootstrap-vcpkg.bat" -disableMetrics
& "$HOME\vcpkg\vcpkg.exe" install "libheif:x64-windows"
```

`crates/pipeline`（P1）接入 `libheif-rs` 时的环境变量约定：

- `RUSTFLAGS` 不需要；`libheif-sys` 在 Windows 走 vcpkg 探测路径，确保 `VCPKG_ROOT=$HOME\vcpkg`
- 运行时需要 `vcpkg\installed\x64-windows\bin` 在 `PATH`（开发期）；正式发布时把所需 DLL 随安装包捆绑

macOS / Linux：`brew install libheif` 或 `apt install libheif-dev`。

### 验证安装

```bash
cargo check --workspace --all-targets
pnpm install && pnpm --filter miaomory-app typecheck
cargo run -p miaomory-app --bin export_contracts
```

三条全部通过即环境就绪。
