# 运行时准备（spec 0008 / ADR-0014）：为应用提供 runtime\dml\onnxruntime.dll。
#
# 该 dll 是随附的 onnxruntime 变体，覆盖 CPU 与 DirectML 两种 EP（CUDA 走按需下载）。
# 部署位置（各自幂等，已存在即跳过）：
#   1. apps/app/src-tauri/runtime/dml/   —— tauri bundle 资源（tauri.conf.json resources）
#   2. target/debug/runtime/dml/         —— dev 的 exe 旁（load-dynamic 启动候选）
#   3. target/release/runtime/dml/       —— release 构建的 exe 旁
# dll 来源优先级：
#   1. 既有部署位置互借（任一已有即复制）
#   2. 本地自建产物（pack-runtime.ps1 注释所述 cmake --use_dml 产物，真正 DirectML）
#   3. 微软官方 CPU 变体顶位（无 DirectML；会话层自动降级 CPU，设置页可见原因）
#
# 用法：powershell -ExecutionPolicy Bypass -File scripts/prepare-bundle-runtime.ps1
param(
    [string]$Version = "1.28.0"
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot

$destDirs = @(
    (Join-Path $repo "apps\app\src-tauri\runtime\dml"),
    (Join-Path $repo "target\debug\runtime\dml"),
    (Join-Path $repo "target\release\runtime\dml")
)

# 已就位的目标跳过
$missing = @()
foreach ($d in $destDirs) {
    if (Test-Path (Join-Path $d "onnxruntime.dll")) {
        Write-Host "[dml] $d 已存在，跳过"
    } else {
        $missing += $d
    }
}
if ($missing.Count -eq 0) { exit 0 }

# 找一份源 dll：既有部署互借 > 自建产物 > 官方 CPU 变体
$src = $null
$allDirs = $destDirs + @(
    (Join-Path $repo "target\release\runtime\dml"),
    (Join-Path $env:TEMP "miaomory-runtime-cache\dml")
)
foreach ($cand in $allDirs) {
    $p = Join-Path $cand "onnxruntime.dll"
    if (Test-Path $p) { $src = $p; break }
}
$srcKind = "既有部署"
if (-not $src) {
    foreach ($cand in @(
        (Join-Path $repo "target\release\runtime\dml\onnxruntime.dll"),
        (Join-Path $env:TEMP "miaomory-runtime-cache\dml\onnxruntime.dll")
    )) {
        if (Test-Path $cand) { $src = $cand; $srcKind = "自建产物"; break }
    }
}
if (-not $src) {
    $cache = Join-Path $env:TEMP "miaomory-runtime-cache"
    New-Item -ItemType Directory -Force -Path $cache | Out-Null
    $zip = Join-Path $cache "onnxruntime-win-x64-$Version.zip"
    if (-not (Test-Path $zip)) {
        $url = "https://github.com/microsoft/onnxruntime/releases/download/v$Version/onnxruntime-win-x64-$Version.zip"
        Write-Host "[download] $url"
        Invoke-WebRequest -Uri $url -OutFile $zip
    }
    $out = "$zip.out"
    Expand-Archive -Path $zip -DestinationPath $out -Force
    $src = Join-Path $out "onnxruntime.dll"
    $srcKind = "官方 CPU 变体顶位（DirectML 将降级 CPU，设置页可见原因）"
}

foreach ($d in $missing) {
    New-Item -ItemType Directory -Force -Path $d | Out-Null
    Copy-Item $src (Join-Path $d "onnxruntime.dll") -Force
    Write-Host "[dml] 已部署到 $d（来源：$srcKind）"
}
