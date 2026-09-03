# ADR-0005：Workspace 架构与依赖规则

- 状态：Accepted
- 日期：2026-09-04
- 决策者：项目所有者
- 相关：白皮书 §4

## 背景

项目要求"高度抽象的架构式开发 + 代码简洁"。需要统一目录结构与依赖规则，让抽象只建在会变的地方（存储后端、推理引擎、向量索引），其余直接写。

## 决策

我们决定采用 Cargo + pnpm 双 workspace（代码启动时落地）：

```
apps/app/            Tauri 壳 + React UI（唯一可执行物，按 feature 组织）
crates/core/         领域类型 + 端口 trait：零 IO、零 async、不依赖 tauri/ort
crates/pipeline/     ingest 流水线：scan → hash → decode → embed → persist
crates/embed/        唯一允许碰 ONNX 的 crate（ort + 模型下载 + 量化）
crates/store/        SQLite + sqlite-vec + FTS5 的唯一实现
crates/platform/     路径 / 配置 / 日志 / 用户可见错误码
packages/contracts/  specta 生成的 TS 类型（Rust 为唯一事实源）
packages/ui/         设计 token + 基础组件（需要复用时再拆）
tests/e2e/           Playwright + tauri-driver
```

配套规则：

1. **单向依赖**：`core` 不依赖任何业务 crate；`embed/store/platform` 只被 `pipeline` 与装配层依赖；装配在 `apps/app`。
2. **端口清单全项目 5 个**：StorageAdapter、VectorIndex、Embedder、EventSink、Clock/IdGen；多一个都是过度设计。
3. **CQRS-lite**：写命令（返回 job id + 进度事件流）与读查询（返回快照）分离。
4. **IPC 契约**：specta 生成 TS 类型，禁止手写 IPC 类型。
5. **错误分类学**：core 定义 `ErrorCode` 枚举，UI 映射为"发生了什么 + 用户能做什么"的中文文案。
6. UI 依赖克制：react-query（命令缓存）+ tanstack-virtual（虚拟滚动）+ 极薄组件层；动效默认 CSS transition。

## 后果

### 积极

- 替换存储/模型/索引不触及 UI 与流水线
- core 可脱离 Tauri 单测；类型契约消灭 IPC 漂移

### 消极 / 需关注

- workspace 初始文件较多；缓解：只创建现有 crate，不预建空壳

## 备选方案

| 方案 | 结论 | 原因 |
| :--- | :--- | :--- |
| 单包演进式 | 否 | 与"架构式开发"目标不符，后期拆分成本高 |
| 多进程/微服务 | 否 | 桌面单机应用不需要，徒增复杂度 |
