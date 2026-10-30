# tests\run-all.ps1 - run all cua end-to-end phases sequentially and propagate failures.
$ErrorActionPreference = "Continue"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$repo = Split-Path -Parent $here

# Bind every suite to one binary and one disposable data root. Without this a
# suite can silently test an old bin\cua.exe and touch the user's config/cache.
if (-not $env:CUA_TEST_BIN) {
    $cands = @("$repo\target\release\cua.exe", "$repo\target\debug\cua.exe", "$repo\bin\cua.exe") |
        Where-Object { Test-Path $_ }
    if (-not $cands) { throw "no cua.exe found - run 'cargo build --release' first" }
    $env:CUA_TEST_BIN = @($cands | Sort-Object { (Get-Item $_).LastWriteTime } -Descending)[0]
}
if (-not $env:CUA_TEST_TESTPAD) {
    $cands = @("$repo\target\release\testpad.exe", "$repo\target\debug\testpad.exe", "$repo\bin\testpad.exe") |
        Where-Object { Test-Path $_ }
    if ($cands) {
        $env:CUA_TEST_TESTPAD = @($cands | Sort-Object { (Get-Item $_).LastWriteTime } -Descending)[0]
    }
}
if (-not $env:CUA_DATA_DIR) {
    $env:CUA_DATA_DIR = Join-Path $env:TEMP ("cua-e2e-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
}
New-Item -ItemType Directory -Force -Path $env:CUA_DATA_DIR | Out-Null
Write-Host ("== cli    : {0}" -f $env:CUA_TEST_BIN) -ForegroundColor Cyan
Write-Host ("== sha256 : {0}" -f (Get-FileHash $env:CUA_TEST_BIN -Algorithm SHA256).Hash) -ForegroundColor Cyan
Write-Host ("== data   : {0}" -f $env:CUA_DATA_DIR) -ForegroundColor Cyan

$suites = @("e2e.ps1", "e2e2.ps1", "e2e3.ps1", "e2e4.ps1", "e2e5.ps1", "e2e6.ps1", "e2e7.ps1", "e2e8.ps1")
$failed = @()
foreach ($s in $suites) {
    Write-Host ""
    Write-Host "########## $s ##########" -ForegroundColor Cyan
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $here $s)
    if ($LASTEXITCODE -ne 0) { $failed += $s }
}
Write-Host ""
if ($failed.Count -gt 0) {
    Write-Host ("FAILED SUITES: " + ($failed -join ", ")) -ForegroundColor Red
    exit 1
}
Write-Host "ALL SUITES PASSED" -ForegroundColor Green
if ($env:CUA_E2E_CLEAN -eq "1") {
    Remove-Item -Recurse -Force $env:CUA_DATA_DIR -ErrorAction SilentlyContinue
}
exit 0
