# 开发者文档

- 状态：现行
- 维护者：项目所有者
- 最后更新：2026-09-04

面向开发者的 how-to 文档。每篇回答"什么 / 为什么 / 怎么做"，不贴大段代码代替说明。

## 目录规划

| 文档 | 内容 | 状态 |
| :--- | :--- | :--- |
| `00-setup.md` | 环境准备（Rust/Tauri/Node/vcpkg+libheif） | 现行 |
| `01-architecture.md` | 架构与目录（ADR-0005 落地细节） | 现行 |
| `02-workflow.md` | 垂直切片工作流与 DoD | 现行 |
| `03-ipc-contracts.md` | specta 契约生成与变更流程 | 现行 |
| `04-testing.md` | 测试策略与手工验证清单约定 | 现行 |
| `05-release.md` | 构建/签名/自动更新器 | 现行（更新器端到端待 P3） |
| `06-import-and-indexing.md` | 导入与索引技术解耦现状（线程模型/隐私边界/已知缺陷 D1~D8） | 现行（含待裁决缺陷清单） |

## 环境速览（白皮书附录 A）

Rust 1.80+ · Node.js 22 LTS · pnpm 10 · Tauri CLI v2 · vcpkg + libheif（HEIC，ADR-0008）· WebView2（Windows）/ WKWebView（macOS）

## 架构速览（ADR-0005）

- 目录：`apps/app`（壳与 UI）+ `crates/{core,pipeline,embed,store,platform}` + `packages/contracts`（`packages/ui` 按需后拆）+ `tests/e2e`（延后，ADR-0010）
- 依赖单向：core 零依赖；embed/store/platform 只被 pipeline 与装配层依赖
- 端口全项目 5 个：StorageAdapter、VectorIndex、Embedder、EventSink、Clock/IdGen
- CQRS-lite：写命令（job id + 事件流）/ 读查询（快照）；specta 生成 TS 类型（流程见 `03-ipc-contracts.md`）

## 开发工作流（`AGENT.md` §7，ADR-0010 修订）

P0 → P3 切片；每片 DoD：文档先行 → TDD 实现 → 质量门（fmt/clippy/tsc/契约一致）→ 项目所有者手工验证清单 →（涉模型/索引时）一次性实测记录。e2e 自动化延后。

## 测试策略

- 单元（TDD）：core（哈希/去重/EXIF 解析）、pipeline 各阶段、store 迁移
- 手工验证：UI 与检索质量由项目所有者按切片清单执行（本人照片集 + 中文查询词）；e2e 延后（ADR-0010）
- 性能：一次性人工实测记录，见 `docs/ux/latency-budget.md`；CI 不做常驻 bench（ADR-0004）

## 文档维护责任

开发者文档随实现同步更新；过时文档视同 bug。
