# 05 构建 / 签名 / 自动更新器

状态：现行
维护者：项目所有者
最后更新：2026-09-04

## 什么

每周构建 + 自动更新器（tauri-plugin-updater）的本地与 CI 流程。

## 为什么

更新器依赖签名密钥与固定分发地址，配置分散且不可逆（私钥丢失 = 老用户永远收不到更新），必须集中记录。

## 怎么做

### 密钥（已生成，2026-09-04）

- 私钥：`%USERPROFILE%\.tauri\miaomory.key`（**绝不入库**，`.gitignore` 已挡 `*.key`；丢失无法补救）
- 公钥：已嵌入 `apps/app/src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`
- 更新分发地址：`https://github.com/ChenXu233/MiaoMoryQwQ/releases/latest/download/latest.json`

### 本地构建

```bash
pnpm --filter miaomory-app tauri build
# 产物：target/release/bundle/{nsis,msi}/...
```

### 发布带更新的版本（P3 起生效）

1. CI（或本地）设置环境变量后构建：
   - `TAURI_SIGNING_PRIVATE_KEY_PATH` = 私钥路径
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` = 生成密钥时设置的密码（当前为空）
2. `tauri.conf.json` 的 `bundle.createUpdaterArtifacts` 置 `true`（P3 打开）
3. GitHub Release 上传：安装包 + `.sig` 签名 + `latest.json`（指向上传的安装包地址）

### 当前 P0 状态

- 插件已注册（`tauri_plugin_updater` + `tauri_plugin_process`），能力已放行（`updater:default` / `process:default`）
- CI 三平台构建**不签名**（无密钥依赖）；macOS 产物未签名，安装需右键绕过 Gatekeeper
- 更新检查 UI 与端到端验证在 P3 完成（ADR-0010：e2e 延后，人工验证）
