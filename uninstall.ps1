# uninstall.ps1 — remove the cua skill + PATH entry (keeps source & binaries)
$ErrorActionPreference = "SilentlyContinue"
$skillDir = Join-Path $env:USERPROFILE ".agents\skills\cua"
Remove-Item -Recurse -Force $skillDir
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -like "*$root\bin*") {
    $new = ($userPath -split ";" | Where-Object { $_ -ne "$root\bin" }) -join ";"
    [Environment]::SetEnvironmentVariable("Path", $new, "User")
}
Write-Host "cua skill removed; binaries remain in $root\bin (delete manually if unwanted)"
