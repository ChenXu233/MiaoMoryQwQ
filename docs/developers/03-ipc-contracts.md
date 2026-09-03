# 03 IPC 契约生成与变更流程

状态：现行
维护者：项目所有者
最后更新：2026-09-04

## 什么

Rust 命令（唯一事实源）→ tauri-specta → `packages/contracts/src/bindings.ts` 的自动生成与一致性检查机制（铁律 6）。

## 为什么

手写 IPC 类型必然漂移；契约生成让"Rust 改签名 → 前端编译报错"变成日常保障。

## 怎么做

### 加一个命令

1. 在 `apps/app/src-tauri/src/lib.rs` 写命令并加 `#[tauri::command]` + `#[specta::specta]`
2. 把命令加进 `app_builder()` 的 `collect_commands![...]`
3. 重新生成契约：
   ```bash
   cargo run -p miaomory-app --bin export_contracts
   ```
4. 前端从 `@miaomory/contracts` 导入使用：
   ```ts
   import { commands } from "@miaomory/contracts";
   const msg = await commands.greet("MiaoMory");
   ```

### 本地与 CI 一致性检查

```bash
cargo run -p miaomory-app --bin export_contracts
git diff --exit-code -- packages/contracts
```

CI 质量门内置同一步骤（`.github/workflows/ci.yml`），生成物有漂移即红。

### 约束

- `packages/contracts/src/bindings.ts` **生成物入库**、禁止手改（文件头有生成标记）
- 事件（`tauri_specta::Event`）自 P1 起：定义在 `crates/core`，装配层收集，前端经 bindings 监听
- 新命令的 DTO 类型若来自 `mm-core`，该类型需派生 `specta::Type`（P1 起引入）
