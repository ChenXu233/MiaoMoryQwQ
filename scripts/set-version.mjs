// 发布 CI 版本注入：把 tag 的基础版本号（纯数字 X.Y.Z）写入 5 处版本文件。
// 用法：node scripts/set-version.mjs <X.Y.Z>
// 仓库中的版本文件永远保持纯数字（MSI/ProductVersion 不接受字母预发布标识），
// alpha 语义只存在于 tag（vX.Y.Z-alpha.N）与 GitHub Pre-release 标记。
// CI 中此脚本只改 runner 工作区；回写 main 由 release.yml 的 finalize job 负责。
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const version = process.argv[2];

if (!version || !/^\d+\.\d+\.\d+$/.test(version)) {
  console.error(`用法：node scripts/set-version.mjs <X.Y.Z>（收到：${version ?? '（无）'}）`);
  process.exit(1);
}

const jsonFiles = [
  'apps/app/package.json',
  'packages/contracts/package.json',
  'apps/app/src-tauri/tauri.conf.json',
];

for (const rel of jsonFiles) {
  const file = path.join(root, rel);
  const parsed = JSON.parse(readFileSync(file, 'utf8'));
  if (parsed.version === version) {
    console.log(`跳过（已是目标版本）：${rel}`);
    continue;
  }
  parsed.version = version;
  writeFileSync(file, JSON.stringify(parsed, null, 2) + '\n');
  console.log(`已写入：${rel} → ${version}`);
}

// 根 Cargo.toml：只动 [workspace.package] 段的 version，成员 crate 均 version.workspace = true
const cargoTomlPath = path.join(root, 'Cargo.toml');
const cargoToml = readFileSync(cargoTomlPath, 'utf8');
const sectionRe = /(\[workspace\.package\]\n(?:[^\n]*\n)*?version\s*=\s*")([^"\n]+)(")/;
if (!sectionRe.test(cargoToml)) {
  console.error('未能在根 Cargo.toml 的 [workspace.package] 段定位 version 行');
  process.exit(1);
}
if (cargoToml.match(sectionRe)[2] !== version) {
  writeFileSync(cargoTomlPath, cargoToml.replace(sectionRe, `$1${version}$3`));
  console.log(`已写入：Cargo.toml [workspace.package] → ${version}`);
} else {
  console.log(`跳过（已是目标版本）：Cargo.toml [workspace.package]`);
}

// Cargo.lock 同步工作区成员版本（第三方依赖不动）
execFileSync('cargo', ['update', '--workspace'], { cwd: root, stdio: 'inherit' });
console.log('已同步：Cargo.lock（cargo update --workspace）');
console.log(`版本注入完成：${version}`);
