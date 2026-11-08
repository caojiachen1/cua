# tools\install-models.ps1 - copy the pinned perception assets into the cua
# data directory. The assets themselves are not part of the repository; this
# script only copies locally prepared, hash-locked files.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File tools\install-models.ps1
#   powershell -ExecutionPolicy Bypass -File tools\install-models.ps1 -Source D:\models
param(
    [string]$Source = (Join-Path (Split-Path -Parent $PSScriptRoot) ".models"),
    [switch]$Deep
)
$ErrorActionPreference = "Stop"
if (-not (Test-Path $Source)) { throw "model source not found: $Source" }
$destRoot = Join-Path $env:LOCALAPPDATA "cua\models"
New-Item -ItemType Directory -Force -Path $destRoot | Out-Null

foreach ($item in @("omni-v2", "ppocr-v4", "florence2-caption", "models")) {
    $src = Join-Path $Source $item
    if (-not (Test-Path $src)) { throw "missing asset directory: $src" }
    $dst = Join-Path $destRoot $item
    Write-Host ("== {0} -> {1}" -f $src, $dst) -ForegroundColor Cyan
    robocopy $src $dst /E /NFL /NDL /NJH /NJS /NP | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "robocopy failed for $src (exit $LASTEXITCODE)" }
}

$cua = $env:CUA_TEST_BIN
if (-not $cua) { $cua = Join-Path (Split-Path -Parent $PSScriptRoot) "target\release\cua.exe" }
if (-not (Test-Path $cua)) { $cua = Join-Path (Split-Path -Parent $PSScriptRoot) "bin\cua.exe" }
if (Test-Path $cua) {
    $args = @("perception", "models", "verify")
    if ($Deep) { $args += "--deep" }
    & $cua @args
    exit $LASTEXITCODE
}
Write-Host "cua.exe not built; assets copied, verification skipped" -ForegroundColor Yellow
exit 0
