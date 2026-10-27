# tests/e2e7.ps1 - the UIA capability ladder must keep routing correctly.
# Runs the probe's synthetic selftest only: no app is launched, nothing is mutated.
$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$probe = Join-Path $root "tools\uia-probe.ps1"
$script:pass = 0; $script:fail = 0; $script:failures = @()

function Check($name, $cond, $detail = "") {
    if ($cond) { $script:pass++; Write-Host ("  PASS  {0}" -f $name) -ForegroundColor Green }
    else { $script:fail++; $script:failures += $name; Write-Host ("  FAIL  {0}  {1}" -f $name, $detail) -ForegroundColor Red }
}

Write-Host "== cua e2e phase-7: UIA capability ladder" -ForegroundColor Cyan
if (-not (Test-Path $probe)) {
    Check "L0 probe script present" $false "missing $probe"
} else {
    $head = (Get-Content -LiteralPath $probe -TotalCount 3 -Encoding Byte) -join ''
    Check "L0 probe script keeps its UTF-8 BOM (PS 5.1 needs it for CJK literals)" ($head -eq '239187191') "first bytes: $head"
    $out = & powershell -NoProfile -ExecutionPolicy Bypass -File $probe -SelfTest 2>&1
    $code = $LASTEXITCODE
    $text = ($out -join "`n")
    Check "L1 ladder selftest exits clean" ($code -eq 0) "exit=$code"
    Check "L2 every branch reported as routed" ($text -like "*all ladder branches behave as documented*") ($text.Substring([Math]::Max(0, $text.Length - 200)))
    $cases = ([regex]::Matches($text, '(?m)^\s+(ok|MISS)\s')).Count
    Check "L3 at least 12 synthetic cases exercised" ($cases -ge 12) "cases=$cases"
    foreach ($v in @('A11Y_FULL', 'A11Y_PARTIAL', 'A11Y_RECT_ONLY', 'MSG_LEVEL_ONLY', 'READ_ONLY', 'NO_PROVIDER', 'NEEDS_REAL_INPUT', 'CAPTURE_BLACK', 'LAUNCH_FAILED')) {
        Check ("L4 verdict {0} is reachable" -f $v) ($text -like ("*$v*")) ""
    }
}

Write-Host ""
Write-Host ("==== PHASE-7 RESULT: {0} passed, {1} failed" -f $script:pass, $script:fail) -ForegroundColor $(if ($script:fail -eq 0) { "Green" } else { "Red" })
if ($script:fail -gt 0) { $script:failures | ForEach-Object { Write-Host ("  failed: " + $_) -ForegroundColor Red }; exit 1 }
exit 0
