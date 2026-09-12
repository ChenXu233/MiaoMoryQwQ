# 05 发布流程（打 tag 即发版）

状态：现行
维护者：项目所有者
最后更新：2026-09-12

## 什么

发布 CI 全流程：版本号注入、发布说明生成、三平台构建与更新器签名、GitHub Release 发布、CHANGELOG.md 维护、更新器 JSON 分发。

## 为什么

- 版本号散布 5 处文件，手改易错且曾返工（MSI/ProductVersion 拒绝字母预发布标识）。
- 更新器依赖签名密钥与固定分发地址，配置不可逆（私钥丢失 = 老用户永远收不到更新）。
- GitHub 的 `/releases/latest` 永不指向 Pre-release，且会被其他 release（如模型资产 models-v1）抢占，alpha 期间原生端点链路不通。

收敛为**「只打 tag，其余全自动」**。

## 怎么做

### 发版（唯一手动步骤）

```bash
# 在已过 CI 的 main commit 上
git tag v0.1.1-alpha.1
git push origin v0.1.1-alpha.1
```

约定：

- tag 格式 `v<主>.<次>.<修订>(-后缀)?`，不合法直接构建失败。
- 版本文件里永远是纯数字 `X.Y.Z`（MSI 约束）；alpha 语义只存在于 tag 后缀与 GitHub Pre-release 标记。
- 每次 tag 必须换新的基础版本号（0.1.0 → 0.1.1 → …），保证更新器版本严格递增。
- 非 `v*` tag（如 models-v2）不触发发布。

### CI 自动做什么（.github/workflows/release.yml）

1. **build**（三平台矩阵）：校验 tag → `scripts/set-version.mjs` 注入版本（仅 runner 工作区）→ git-cliff 生成发布说明（`scripts/cliff.toml`，conventional commits 中文分组）→ tauri-action 构建 + 签名 + 发布 Pre-release（tauri-action 合并各平台更新条目）。
2. **finalize**（全矩阵成功后）：重生成根 `CHANGELOG.md` → 版本回写 5 处文件并提交 main（github-actions[bot]，GITHUB_TOKEN 推送不触发 ci.yml）→ 把合并后的 latest.json 整体替换推到 `updater` 分支。

失败重试：某平台构建失败可单独 re-run 该 leg，tauri-action 会补传到已存在的 Release；finalize 失败直接 re-run。连续推 tag 会排队（concurrency: release）。

### 版本号位置（勿手改，CI 自动同步）

`apps/app/package.json`、`packages/contracts/package.json`、`apps/app/src-tauri/tauri.conf.json`、根 `Cargo.toml [workspace.package]`、`Cargo.lock`（`cargo update --workspace` 同步）。

### 更新器

- 密钥（已生成，2026-09-04）：私钥 `%USERPROFILE%\.tauri\miaomory.key`（**绝不入库**，`.gitignore` 已挡 `*.key`；丢失无法补救）；公钥内嵌 `tauri.conf.json`。
- 分发端点：`https://raw.githubusercontent.com/ChenXu233/MiaoMoryQwQ/updater/latest.json`（自管分支，每次发布由 CI 整体替换；不使用 `releases/latest/download/latest.json`，原因见上）。
- alpha 期间所有发布保持 Pre-release 标记；macOS 产物未签名，安装需右键绕过 Gatekeeper。
- 更新检查 UI 与端到端验证在 P3 完成（ADR-0010：e2e 延后，人工验证）。

### CHANGELOG.md

根目录 `CHANGELOG.md` 由 CI 每次发布全量重写（git-cliff），**请勿手改**；提交规范见 conventional commits，`chore(release)` 回写提交自动排除。
