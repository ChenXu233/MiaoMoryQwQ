# 开发者文档

- 状态：标准就绪（代码尚未初始化，内容随切片编写）
- 维护者：项目所有者
- 最后更新：2026-09-04

面向开发者的 how-to 文档。每篇回答"什么 / 为什么 / 怎么做"，不贴大段代码代替说明。

## 目录规划

| 文档 | 内容 | 状态 |
| :--- | :--- | :--- |
| `00-setup.md` | 环境准备（Rust/Tauri/Node/FFmpeg） | 待 P0 编写 |
| `01-architecture.md` | 架构与目录（ADR-0005 落地细节） | 待 P0 编写 |
| `02-workflow.md` | 垂直切片工作流与 DoD | 待 P0 编写 |
| `03-ipc-contracts.md` | specta 契约生成与变更流程 | 待 P0 编写 |
| `04-testing.md` | 测试策略与人工数据集约定 | 待 P1 编写 |
| `05-release.md` | 构建/签名/自动更新器 | 待 P0 编写 |

## 环境速览（白皮书附录 A）

Rust 1.80+ · Node.js 18+ · pnpm · Tauri CLI v2 · FFmpeg（缩略图/HEIC）· WebView2（Windows）/ WKWebView（macOS）

## 架构速览（ADR-0005）

- 目录：`apps/app`（壳与 UI）+ `crates/{core,pipeline,embed,store,platform}` + `packages/{contracts,ui}` + `tests/e2e`
- 依赖单向：core 零依赖；embed/store/platform 只被 pipeline 与装配层依赖
- 端口全项目 5 个：StorageAdapter、VectorIndex、Embedder、EventSink、Clock/IdGen
- CQRS-lite：写命令（job id + 事件流）/ 读查询（快照）；specta 生成 TS 类型

## 开发工作流（`AGENT.md` §7）

P0 → P3 切片；每片 DoD：文档先行 → 实现 → e2e → 质量门 →（涉模型/索引时）一次性基准记录。

## 测试策略

- 单元：core（哈希/去重/EXIF 解析）、pipeline 各阶段、store 迁移
- e2e：小规模固定人工数据集（项目所有者提供）+ tauri-driver
- 性能：一次性人工基准，见 `docs/ux/latency-budget.md`；CI 不做常驻 bench（ADR-0004）

## 文档维护责任

开发者文档随实现同步更新；过时文档视同 bug。
