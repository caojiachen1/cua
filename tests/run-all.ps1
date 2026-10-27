# tests\run-all.ps1 - run all cua end-to-end phases sequentially and propagate failures.
$ErrorActionPreference = "Continue"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$suites = @("e2e.ps1", "e2e2.ps1", "e2e3.ps1", "e2e4.ps1", "e2e5.ps1", "e2e6.ps1", "e2e7.ps1")
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
exit 0
