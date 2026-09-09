# 推理运行时打包脚本（spec 0008 / ADR-0014）
#
# 作用：
#   1. 从微软官方 GitHub Release 下载 onnxruntime 运行时 zip（CPU / CUDA13 变体）
#   2. 计算 SHA-256 与精确字节数
#   3. 回填 crates/embed/assets/runtime-manifest.json（sha256 与 size）
#
# 用法（在仓库根目录）：
#   powershell -ExecutionPolicy Bypass -File scripts/pack-runtime.ps1              # 全部变体
#   powershell -ExecutionPolicy Bypass -File scripts/pack-runtime.ps1 -Variants cpu # 仅 CPU
#
# 说明：
#   - DML 变体无官方动态库分发（微软 DirectML nuget 冻结在 1.24，API 低于 ort 1.28
#     要求；pyke CDN 为加密私有格式），需从源码自建后手工放入安装包 runtime\dml\：
#       cmake --build 构建 onnxruntime v1.28.0 --use_dml --build_dll --config Release
#   - 运行时主源为微软官方 GitHub Release；如需国内镜像，可将本脚本下载的 zip
#     上传到项目 GitHub Release（runtime-v1），并把 URL 加进 download_runtime 的
#     endpoints 列表。
param(
    [string[]]$Variants = @("cpu", "cuda"),
    [string]$Version = "1.28.0"
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
$manifestPath = Join-Path $repo "crates\embed\assets\runtime-manifest.json"

$urls = @{
    "cpu"  = "https://github.com/microsoft/onnxruntime/releases/download/v$Version/onnxruntime-win-x64-$Version.zip"
    "cuda" = "https://github.com/microsoft/onnxruntime/releases/download/v$Version/onnxruntime-win-x64-gpu_cuda13-$Version.zip"
}

$cache = Join-Path $env:TEMP "miaomory-runtime-cache"
New-Item -ItemType Directory -Force -Path $cache | Out-Null

foreach ($kind in $Variants) {
    if (-not $urls.ContainsKey($kind)) {
        Write-Error "未知变体：$kind（支持 cpu|cuda）"
    }
    $url = $urls[$kind]
    $name = [IO.Path]::GetFileName($url)
    $zip = Join-Path $cache $name

    if (Test-Path $zip) {
        Write-Host "[cache] $name 已在本地缓存"
    } else {
        Write-Host "[download] $url"
        Invoke-WebRequest -Uri $url -OutFile $zip
    }

    $bytes = (Get-Item $zip).Length
    $sha = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
    Write-Host "[hash] $kind  sha256=$sha  size=$bytes"

    # 回填 manifest（按 kind 定位条目，回填 sha256 与 size）
    $json = Get-Content $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
    foreach ($v in $json.variants) {
        if ($v.kind -eq $kind) {
            $v.files[0].sha256 = $sha
            $v.files[0].size = $bytes
        }
    }
    $json | ConvertTo-Json -Depth 10 | Set-Content $manifestPath -Encoding UTF8
    Write-Host "[manifest] 已回填 $kind 条目"
}

Write-Host ""
Write-Host "完成。runtime-manifest.json 已更新；建议人工核对 JSON 格式后提交。"
