# ADR-0009：前端栈切换为 Vue 3 + TypeScript

- 状态：Proposed
- 日期：2026-09-04
- 决策者：项目所有者（待批）
- 相关：白皮书 §3.1、§4.1；ADR-0005；`AGENT.md` §1

## 背景

白皮书 §4.1 与 `AGENT.md` §1 原定 React + TypeScript。项目所有者基于性能与偏好裁定改用 Vue 3 + TypeScript，并明确前端设计要求：**极简设计 + 颜色 token**，不引入重组件库。此为前端框架级依赖变更，按铁律 2 落 ADR。

## 决策

我们决定：

1. UI 框架采用 **Vue 3（`<script setup>` 组合式 API）+ TypeScript + Vite**，取代 React 表述（白皮书 §4.1 架构图、`AGENT.md` §1 技术栈行同步修订）。
2. ADR-0005 中前端依赖克制条目的 React 生态对应物替换为 Vue 生态等价物：`@tanstack/vue-query`（命令缓存）+ `@tanstack/vue-virtual`（虚拟滚动）；动效默认 CSS transition。ADR-0005 原文不改写，以此 ADR 为准。
3. **设计 token**：以 CSS 自定义属性定义语义色板（`--color-bg` / `--color-fg` / `--color-accent` 等），置于 `apps/app/src/design/tokens.css`；暗色模式预留 `data-theme` 切换。
4. **不引入组件库**；基础组件（Button/Input/EmptyState 等）自建并保持极简。组件复用需求出现时再评估是否拆 `packages/ui`（ADR-0005 原则不变）。
5. IPC 契约不变：tauri-specta 生成 TS 类型，禁止手写 IPC 类型（铁律 6）。

## 后果

### 积极

- 满足所有者对运行时性能与极简设计的要求
- Vue 单文件组件 + 组合式 API 与"极薄组件层"目标契合
- 依赖克制原则原样保留，仅替换生态对应物

### 消极 / 需关注

- tauri-specta 社区示例以 React 为主，Vue 集成需 P0 早期验证（风险已列入 MVP 计划 §9）
- 移动端（LATER）如启动，需确认 Vue 生态对 Tauri mobile 的适配经验

## 备选方案

| 方案 | 结论 | 原因 |
| :--- | :--- | :--- |
| 维持 React | 否 | 项目所有者明确裁定更换；性能与维护偏好 |
| Svelte / SolidJS | 否 | 生态成熟度与类型工具链不如 Vue 3 稳健，团队熟悉度低 |
| Vue + 重组件库（Element Plus 等） | 否 | 与"极简 + 颜色 token"设计要求冲突 |
