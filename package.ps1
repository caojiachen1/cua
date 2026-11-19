# package.ps1 — build the shipping binaries and pack them into dist\cua-<version>-win64.zip
#
# install.ps1 consumes this archive so a fresh clone installs without a Rust
# toolchain. Run this after any code change and commit the resulting zip:
#
#     powershell -ExecutionPolicy Bypass -File package.ps1
#     git add dist/ && git commit -m "Rebuild prebuilt binaries"
#
# The zip is committed rather than the raw exe because cua.exe is ~108 MB and
# GitHub rejects any single file over 100 MB; the deflated archive is ~31 MB.
param([switch]$SkipBuild, [switch]$Cuda, [switch]$Autotune)
$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
if (-not $root) { $root = Split-Path -Parent $MyInvocation.MyCommand.Path }

$version = (Select-String -Path (Join-Path $root "Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$zipName = "cua-$version-win64.zip"
$dist = Join-Path $root "dist"
$stage = Join-Path $root "target\package-stage"

if (-not $SkipBuild) {
    Write-Host "== [1/4] cargo build --release" -ForegroundColor Cyan
    # A resident service or the browser daemon locks the old binary on Windows.
    if (Test-Path "$root\bin\cua.exe") {
        & "$root\bin\cua.exe" perception serve --stop 2>$null | Out-Null
        & "$root\bin\cua.exe" browser stop 2>$null | Out-Null
    }
    $features = @()
    if ($Cuda) { $features += "cuda" }
    if ($Autotune) { $features += "autotune" }
    Push-Location $root
    $env:GPUI_FXC_PATH = $env:GPUI_FXC_PATH
    if ($features.Count -gt 0) {
        cargo build --release --bin cua --bin cua-settings --bin testpad ("--features=" + ($features -join ","))
    } else {
        cargo build --release --bin cua --bin cua-settings --bin testpad
    }
    if ($LASTEXITCODE -ne 0) { Pop-Location; throw "cargo build failed" }
    Pop-Location

    Write-Host "== [2/4] build child-session host" -ForegroundColor Cyan
    $hostProj = Join-Path $root "tools\session-host\CuaSessionHost.csproj"
    if ((Test-Path $hostProj) -and (Get-Command dotnet -ErrorAction SilentlyContinue)) {
        & dotnet build $hostProj -c Release -o (Join-Path $root "target\session-host") | Out-Host
        if ($LASTEXITCODE -ne 0) {
            Write-Host "   warning: child-session host build failed; cua session will be unavailable" -ForegroundColor Yellow
        }
    } elseif (-not (Get-Command dotnet -ErrorAction SilentlyContinue)) {
        Write-Host "   warning: dotnet SDK not found; cua session will be unavailable" -ForegroundColor Yellow
    }
} else {
    Write-Host "== [1/4] skipping build (-SkipBuild)" -ForegroundColor DarkGray
}

Write-Host "== [3/4] stage files" -ForegroundColor Cyan
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

$required = @("cua.exe", "cua-settings.exe", "testpad.exe")
foreach ($name in $required) {
    $src = Join-Path "$root\target\release" $name
    if (-not (Test-Path $src)) { throw "missing build output: $src" }
    Copy-Item $src (Join-Path $stage $name) -Force
}
# optional companions
$hostExe = Join-Path $root "target\session-host\cua-session-host.exe"
if (Test-Path $hostExe) { Copy-Item $hostExe (Join-Path $stage "cua-session-host.exe") -Force }
$hostCfg = Join-Path $root "target\session-host\cua-session-host.exe.config"
if (Test-Path $hostCfg) { Copy-Item $hostCfg (Join-Path $stage "cua-session-host.exe.config") -Force }
foreach ($dll in @("DirectML.dll", "onnxruntime.dll")) {
    $src = Join-Path "$root\target\release" $dll
    if (Test-Path $src) { Copy-Item $src (Join-Path $stage $dll) -Force }
}
if (Test-Path "$root\tools\vnc\tvnserver.exe") {
    New-Item -ItemType Directory -Force -Path (Join-Path $stage "vnc") | Out-Null
    Copy-Item "$root\tools\vnc\*" (Join-Path $stage "vnc\") -Force
}
# a manifest so install.ps1 can verify what it unpacked and when it was built
@{
    version = $version
    built_at = $stamp
    files = @(Get-ChildItem $stage -Recurse -File | ForEach-Object {
        $_.FullName.Substring($stage.Length + 1)
    })
} | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $stage "manifest.json") -Encoding UTF8

Write-Host "== [4/4] compress" -ForegroundColor Cyan
New-Item -ItemType Directory -Force -Path $dist | Out-Null
$zipPath = Join-Path $dist $zipName
if (Test-Path $zipPath) { Remove-Item $zipPath -Force }
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zipPath -CompressionLevel Optimal
$mb = [math]::Round((Get-Item $zipPath).Length / 1MB, 1)
Write-Host ""
Write-Host ("Packed {0} ({1} MB)" -f $zipPath, $mb) -ForegroundColor Green
Write-Host "Commit it:" -ForegroundColor Green
Write-Host "  git add dist/$zipName && git commit -m 'Rebuild prebuilt binaries'"
