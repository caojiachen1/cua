# install.ps1 — one-click install for cua (UBB-style: build + expose + install skill)
# Run:  powershell -ExecutionPolicy Bypass -File install.ps1 [-Cuda] [-Autotune]
param([switch]$Cuda, [switch]$Autotune)
$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
if (-not $root) { $root = Split-Path -Parent $MyInvocation.MyCommand.Path }

Write-Host "== [1/6] cargo build --release" -ForegroundColor Cyan
# A resident perception service locks the old binary on Windows; stop it first.
if (Test-Path "$root\bin\cua.exe") { & "$root\bin\cua.exe" perception serve --stop 2>$null | Out-Null }
# GPU choices come from the agent's install-time question (SKILL.md "Installing
# with GPU acceleration"); nothing is enabled silently. No flags = WGPU only.
$features = @()
if ($Cuda) { $features += "cuda" }
if ($Autotune) { $features += "autotune" }
if ($Cuda) {
    Write-Host ("   CUDA detector: on (autotune: {0})" -f $(if ($Autotune) { "on, ~40 s first call" } else { "off, ~1.5 s start" })) -ForegroundColor DarkGray
    if (-not $env:CUDA_PATH -or -not (Test-Path $env:CUDA_PATH)) {
        Write-Host "   warning: CUDA_PATH is not set; the detector will fall back to WGPU at runtime" -ForegroundColor Yellow
    }
} elseif ($Autotune) {
    Write-Host "   autotune: WGPU kernels only (~2x, ~20 s first call)" -ForegroundColor DarkGray
}
Push-Location $root
if ($features.Count -gt 0) {
    cargo build --release --bin cua --bin testpad ("--features=" + ($features -join ","))
} else {
    cargo build --release --bin cua --bin testpad
}
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "cargo build failed" }
Pop-Location

Write-Host "== [2/6] build child-session host" -ForegroundColor Cyan
$hostProj = Join-Path $root "tools\session-host\CuaSessionHost.csproj"
if (Test-Path $hostProj) {
    if (Get-Command dotnet -ErrorAction SilentlyContinue) {
        & dotnet build $hostProj -c Release -o (Join-Path $root "target\session-host") | Out-Host
        if ($LASTEXITCODE -ne 0) {
            Write-Host "   warning: child-session host build failed; `cua session` will be unavailable" -ForegroundColor Yellow
        }
    } else {
        Write-Host "   warning: dotnet SDK not found; `cua session` will be unavailable" -ForegroundColor Yellow
    }
} else {
    Write-Host "   warning: tools\session-host missing; `cua session` will be unavailable" -ForegroundColor Yellow
}

Write-Host "== [3/6] build ubb runtime" -ForegroundColor Cyan
$ubbCrate = Join-Path $root "ubb\rust-cli\Cargo.toml"
if (Test-Path $ubbCrate) {
    Push-Location (Join-Path $root "ubb\rust-cli")
    & cargo build --release
    $ubbCode = $LASTEXITCODE
    Pop-Location
    if ($ubbCode -ne 0) {
        Write-Host "   warning: ubb build failed; cua browser / cua edge --ubb fall back to a ubb on PATH" -ForegroundColor Yellow
    } else {
        Copy-Item (Join-Path $root "ubb\rust-cli\target\release\ubb.exe") (Join-Path $root "ubb\bin\ubb.exe") -Force
    }
} else {
    Write-Host "   warning: ubb\rust-cli missing; cua browser needs a ubb on PATH" -ForegroundColor Yellow
}

Write-Host "== [4/6] copy binaries to bin\" -ForegroundColor Cyan
New-Item -ItemType Directory -Force -Path "$root\bin" | Out-Null
Copy-Item "$root\target\release\cua.exe" "$root\bin\cua.exe" -Force
Copy-Item "$root\target\release\testpad.exe" "$root\bin\testpad.exe" -Force
$sessionHost = Join-Path $root "target\session-host\cua-session-host.exe"
if (Test-Path $sessionHost) {
    Copy-Item $sessionHost "$root\bin\cua-session-host.exe" -Force
    $sessionHostConfig = Join-Path $root "target\session-host\cua-session-host.exe.config"
    if (Test-Path $sessionHostConfig) { Copy-Item $sessionHostConfig "$root\bin\cua-session-host.exe.config" -Force }
}
# The vendored UBB runtime, when it was built above.
$ubbExe = Join-Path $root "ubb\bin\ubb.exe"
if (Test-Path $ubbExe) { Copy-Item $ubbExe "$root\bin\ubb.exe" -Force }
# ONNX Runtime DirectML (Florence GPU path) and any runtime DLL copied by ort.
foreach ($dll in @("DirectML.dll", "onnxruntime.dll")) {
    $src = Join-Path "$root\target\release" $dll
    if (Test-Path $src) { Copy-Item $src (Join-Path "$root\bin" $dll) -Force }
}

Write-Host "== [5/6] install skill to ~\.agents\skills\cua" -ForegroundColor Cyan
$skillDir = Join-Path $env:USERPROFILE ".agents\skills\cua"
New-Item -ItemType Directory -Force -Path $skillDir | Out-Null
Copy-Item "$root\SKILL.md" "$skillDir\SKILL.md" -Force
# ensure exe on user PATH
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$root\bin*") {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$root\bin", "User")
    Write-Host "   added '$root\bin' to user PATH (reopen shells to take effect)"
}

Write-Host "== [6/6] smoke test" -ForegroundColor Cyan
$ver = & "$root\bin\cua.exe" check | ConvertFrom-Json
if ($ver.uia -ne "ok") { throw "UIA unavailable: $($ver | ConvertTo-Json -Compress)" }
Write-Host ("   cua {0} ok — UIA ok, screen {1}x{2}" -f $ver.version, $ver.screen.primary[0], $ver.screen.primary[1]) -ForegroundColor Green

Write-Host ""
Write-Host "Installed:" -ForegroundColor Green
Write-Host "  CLI      : $root\bin\cua.exe"
Write-Host "  Skill    : $skillDir\SKILL.md"
Write-Host "  Session  : $root\bin\cua-session-host.exe"
Write-Host "  Try      : cua apps | cua state --window Notepad | cua sandbox start --exe notepad"
Write-Host "  Child    : cua session setup (admin once) -> sign out/in -> cua session start"
