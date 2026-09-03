# 01 架构与目录

状态：现行
维护者：项目所有者
最后更新：2026-09-04

## 什么

ADR-0005 双 workspace 的落地形态（以仓库实际目录为准）。

## 为什么

依赖单向是本项目"替换存储/模型/索引不动 UI"承诺的物理基础。

## 怎么做

```
apps/app/            Tauri 壳 + Vue UI（唯一可执行物）
  src-tauri/         commands/events 装配层（crate: miaomory-app）
  src/               Vue 组件（components/、design/tokens.css）
crates/core/         mm-core：领域类型 + ErrorCode + 5 端口 trait（零 IO 零 async）
crates/platform/     mm-platform：路径/配置/tracing
crates/store/        mm-store：SQLite + 迁移 + 查询（P1+）
crates/pipeline/     mm-pipeline：scan→hash→decode→thumb→persist（P1+）
crates/embed/        mm-embed：ort 推理 + 模型下载（P2+，唯一碰 ONNX 处）
packages/contracts/  tauri-specta 生成物（生成物入库）
.github/workflows/   ci.yml（质量门+三平台）、release.yml（tag 发布）
```

依赖规则（强制）：`core` 不依赖任何业务 crate；`embed/store/platform` 只被 `pipeline` 与装配层依赖；装配只在 `apps/app/src-tauri`。

端口全项目 5 个：StorageAdapter、VectorIndex、Embedder、EventSink、Clock（`mm-core` 定义）。

IPC：CQRS-lite，写命令返回 job 快照，读查询返回快照；类型一律由 specta 生成（`03-ipc-contracts.md`）。
