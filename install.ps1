# install.ps1 — one-click install for cua (UBB-style: build + expose + install skill)
# Run:  powershell -ExecutionPolicy Bypass -File install.ps1
$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
if (-not $root) { $root = Split-Path -Parent $MyInvocation.MyCommand.Path }

Write-Host "== [1/4] cargo build --release" -ForegroundColor Cyan
Push-Location $root
cargo build --release --bin cua --bin testpad
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "cargo build failed" }
Pop-Location

Write-Host "== [2/4] copy binaries to bin\" -ForegroundColor Cyan
New-Item -ItemType Directory -Force -Path "$root\bin" | Out-Null
Copy-Item "$root\target\release\cua.exe" "$root\bin\cua.exe" -Force
Copy-Item "$root\target\release\testpad.exe" "$root\bin\testpad.exe" -Force

Write-Host "== [3/4] install skill to ~\.agents\skills\cua" -ForegroundColor Cyan
$skillDir = Join-Path $env:USERPROFILE ".agents\skills\cua"
New-Item -ItemType Directory -Force -Path $skillDir | Out-Null
Copy-Item "$root\SKILL.md" "$skillDir\SKILL.md" -Force
# ensure exe on user PATH
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$root\bin*") {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$root\bin", "User")
    Write-Host "   added '$root\bin' to user PATH (reopen shells to take effect)"
}

Write-Host "== [4/4] smoke test" -ForegroundColor Cyan
$ver = & "$root\bin\cua.exe" check | ConvertFrom-Json
if ($ver.uia -ne "ok") { throw "UIA unavailable: $($ver | ConvertTo-Json -Compress)" }
Write-Host ("   cua {0} ok — UIA ok, screen {1}x{2}" -f $ver.version, $ver.screen.primary[0], $ver.screen.primary[1]) -ForegroundColor Green

Write-Host ""
Write-Host "Installed:" -ForegroundColor Green
Write-Host "  CLI      : $root\bin\cua.exe"
Write-Host "  Skill    : $skillDir\SKILL.md"
Write-Host "  Try      : cua apps | cua state --window Notepad | cua sandbox start --exe notepad"
