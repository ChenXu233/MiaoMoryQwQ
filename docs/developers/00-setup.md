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

**P0 spike 已验证**（2026-09-04，独立项目 `~/libheif-spike`）：`libheif-rs 3.0` + `libheif-sys 5.3.1` 对接 vcpkg `libheif 1.23.2` 链接成功，`version() = [1,23,2]`，解码插件 1 个（libde265）、编码插件 2 个（x265）。构建/运行环境变量约定（`crates/pipeline` P1 接入时沿用）：

- `VCPKG_ROOT` = `%USERPROFILE%\vcpkg`
- `VCPKGRS_TRIPLET` = `x64-windows`（libheif-sys 默认找 `x64-windows-static-md`，会报"not installed"）
- `VCPKGRS_DYNAMIC` = `1`（使用动态库三元组必须显式声明）
- 运行时 `PATH` 需含 `%USERPROFILE%\vcpkg\installed\x64-windows\bin`（开发期）；正式发布时把所需 DLL 随安装包捆绑

macOS / Linux：`brew install libheif` 或 `apt install libheif-dev`（libheif-rs 走 pkg-config，无需上述环境变量）。

### 验证安装

```bash
cargo check --workspace --all-targets
pnpm install && pnpm --filter miaomory-app typecheck
cargo run -p miaomory-app --bin export_contracts
```

三条全部通过即环境就绪。
