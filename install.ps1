# install.ps1 — one-click install for cua (prebuilt binaries, else build from source)
# Run:  powershell -ExecutionPolicy Bypass -File install.ps1 [-Cuda] [-Autotune] [-ForceBuild]
#
# Default path uses the committed dist\cua-<version>-win64.zip, so installing
# needs no Rust toolchain. -ForceBuild (or a missing/corrupt archive) falls back
# to `cargo build --release`.
param([switch]$Cuda, [switch]$Autotune, [switch]$ForceBuild)
$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
if (-not $root) { $root = Split-Path -Parent $MyInvocation.MyCommand.Path }

$version = (Select-String -Path (Join-Path $root "Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
$zipPath = Join-Path $root "dist\cua-$version-win64.zip"

# A resident perception service or the browser daemon locks the old binary on
# Windows; both are this same exe, so stop them before replacing it.
if (Test-Path "$root\bin\cua.exe") {
    & "$root\bin\cua.exe" perception serve --stop 2>$null | Out-Null
    & "$root\bin\cua.exe" browser stop 2>$null | Out-Null
}

$usePrebuilt = (-not $ForceBuild) -and (Test-Path $zipPath)
if ($usePrebuilt) {
    # Warn when the archive predates the sources it claims to represent — the
    # maintainer forgot to run package.ps1. Still installs (better than nothing),
    # but the mismatch is visible.
    $zipTime = (Get-Item $zipPath).LastWriteTimeUtc
    $newer = Get-ChildItem (Join-Path $root "src") -Recurse -Include *.rs -ErrorAction SilentlyContinue |
        Where-Object { $_.LastWriteTimeUtc -gt $zipTime } | Select-Object -First 1
    if ($newer) {
        Write-Host ("   warning: {0} is newer than the archive; run package.ps1 and commit dist\ to ship it" -f $newer.Name) -ForegroundColor Yellow
    }
    Write-Host "== [1/6] unpack prebuilt binaries ($([math]::Round((Get-Item $zipPath).Length/1MB,1)) MB)" -ForegroundColor Cyan
    Write-Host "   from $zipPath" -ForegroundColor DarkGray
    New-Item -ItemType Directory -Force -Path "$root\bin" | Out-Null
    try {
        Expand-Archive -Path $zipPath -DestinationPath "$root\bin" -Force
        $manifest = Join-Path "$root\bin" "manifest.json"
        if (Test-Path $manifest) {
            $m = Get-Content $manifest -Raw | ConvertFrom-Json
            Write-Host ("   prebuilt {0}, built {1}" -f $m.version, $m.built_at) -ForegroundColor DarkGray
        }
        if (-not (Test-Path "$root\bin\cua.exe")) { throw "archive had no cua.exe" }
    } catch {
        Write-Host "   warning: prebuilt unpack failed ($_); falling back to a source build" -ForegroundColor Yellow
        $usePrebuilt = $false
    }
}
if ($usePrebuilt) {
    Write-Host "== [2/6] (skipped: child-session host is in the archive)" -ForegroundColor DarkGray
    Write-Host "== [3/6] (skipped: binaries already unpacked)" -ForegroundColor DarkGray
}

if (-not $usePrebuilt) {
    Write-Host "== [1/6] cargo build --release" -ForegroundColor Cyan
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
        cargo build --release --bin cua --bin cua-settings --bin testpad ("--features=" + ($features -join ","))
    } else {
        cargo build --release --bin cua --bin cua-settings --bin testpad
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

    Write-Host "== [3/6] copy binaries to bin\" -ForegroundColor Cyan
    New-Item -ItemType Directory -Force -Path "$root\bin" | Out-Null
    Copy-Item "$root\target\release\cua.exe" "$root\bin\cua.exe" -Force
    Copy-Item "$root\target\release\cua-settings.exe" "$root\bin\cua-settings.exe" -Force
    Copy-Item "$root\target\release\testpad.exe" "$root\bin\testpad.exe" -Force
    $sessionHost = Join-Path $root "target\session-host\cua-session-host.exe"
    if (Test-Path $sessionHost) {
        Copy-Item $sessionHost "$root\bin\cua-session-host.exe" -Force
        $sessionHostConfig = Join-Path $root "target\session-host\cua-session-host.exe.config"
        if (Test-Path $sessionHostConfig) { Copy-Item $sessionHostConfig "$root\bin\cua-session-host.exe.config" -Force }
    }
    New-Item -ItemType Directory -Force -Path "$root\bin\vnc" | Out-Null
    if (Test-Path "$root\tools\vnc\tvnserver.exe") {
        Copy-Item "$root\tools\vnc\*" "$root\bin\vnc\" -Force
    }
    # ONNX Runtime DirectML (Florence GPU path) and any runtime DLL copied by ort.
    foreach ($dll in @("DirectML.dll", "onnxruntime.dll")) {
        $src = Join-Path "$root\target\release" $dll
        if (Test-Path $src) { Copy-Item $src (Join-Path "$root\bin" $dll) -Force }
    }
}

Write-Host "== [4/6] install browser extension" -ForegroundColor Cyan
# The bridge daemon is linked into cua.exe; what the browser needs is the
# unpacked extension. `cua browser install` copies it next to the data dir and
# registers the native-messaging host so it pairs without a token. Runs on the
# copy in bin\ so the host manifest points at the binary that stays installed.
$installedCua = Join-Path $root "bin\cua.exe"
if (Test-Path $installedCua) {
    & $installedCua browser install | Out-Host
    if ($LASTEXITCODE -ne 0) {
        Write-Host "   warning: cua browser install failed; run it again after the install" -ForegroundColor Yellow
    }
} else {
    Write-Host "   warning: bin\cua.exe missing; run 'cua browser install' once the install completes" -ForegroundColor Yellow
}

Write-Host "== [5/6] install skill to ~\.agents\skills\cua" -ForegroundColor Cyan
$skillDir = Join-Path $env:USERPROFILE ".agents\skills\cua"
New-Item -ItemType Directory -Force -Path $skillDir | Out-Null
Copy-Item "$root\SKILL.md" "$skillDir\SKILL.md" -Force
# the browser subsystem ships its own agent skill (tool catalog, challenge
# recipes, pitfalls) — INSTALL.md tells agents to copy it once
$browserSkillDir = Join-Path $env:USERPROFILE ".agents\skills\cua-browser"
New-Item -ItemType Directory -Force -Path $browserSkillDir | Out-Null
Copy-Item "$root\browser\SKILL.md" "$browserSkillDir\SKILL.md" -Force
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
Write-Host "  Browser  : cua browser install -> load the extension once, then 'cua browser status'"
Write-Host "  Try      : cua apps | cua state --window Notepad | cua sandbox start --exe notepad"
Write-Host "  Child    : cua session setup (admin once) -> sign out/in -> cua session start"
