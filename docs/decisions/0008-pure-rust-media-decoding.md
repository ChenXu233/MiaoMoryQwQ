# ADR-0008：媒体解码纯 Rust 化（image + libheif-rs）

- 状态：Proposed
- 日期：2026-09-04
- 决策者：项目所有者（待批）
- 相关：白皮书 §3.6、附录 A；ADR-0003

## 背景

白皮书 §3.6 原定用 FFmpeg 统一处理缩略图生成与 HEIC 解码，但未定集成方式。MVP 范围（ADR-0003）只处理照片：JPEG/PNG/WebP 解码纯 Rust 即可覆盖，FFmpeg 实际唯一作用是 HEIC。FFmpeg 动态库捆绑将使每平台安装包增加约 20~80MB，直接威胁"<50MB"目标，并引入 LGPL/GPL 构建选择与三平台打包脚本负担。

## 决策

我们决定：

1. MVP 媒体解码采用**纯 Rust 栈**：`image` crate（JPEG/PNG/WebP）+ `libheif-rs`（HEIC/HEIF，捆绑 libheif 原生库）+ `kamadak-exif`（EXIF 提取与方向校正）。
2. FFmpeg 整体**推迟到 LATER**（音视频缩略图与转写阶段复用），同步修订白皮书 §3.6 与附录 A。
3. libheif 的 Windows 构建采用 vcpkg 提供原生库，构建步骤记入 `docs/developers/00-setup.md`；P0 末以 spike 验证可行性。
4. 解码统一产出 RGB 缓冲，供缩略图与（P2 起）模型预处理共用。

## 后果

### 积极

- 安装包体积最小，无 GPL/LGPL 许可证评估负担
- 依赖克制：三个职责单一的 crate 替代一整套 FFmpeg 运行时
- 与"单二进制、双击即用"的产品定位一致

### 消极 / 需关注

- libheif 在 Windows 需 vcpkg 构建原生库，是 P1 最大构建不确定点（P0 spike 前置验证）
- 未来音视频阶段仍需引入 FFmpeg，届时需新 ADR

## 备选方案

| 方案 | 结论 | 原因 |
| :--- | :--- | :--- |
| 按白皮书捆绑 FFmpeg 动态库 | 否 | 体积 +20~80MB 威胁 <50MB 目标；LGPL/GPL 构建选择与三平台打包脚本负担 |
| ffmpeg-sidecar 运行时下载 | 否 | 首启必须联网、多一步失败面；与"双击即用"体验冲突 |
| 仅支持 JPEG/PNG/WebP，不做 HEIC | 否 | iPhone 默认格式，缺失伤害核心用户群 |
