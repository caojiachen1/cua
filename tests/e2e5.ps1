# tests/e2e5.ps1 - phase-5: real third-party desktop apps (QQ Music, NetEase Cloud Music,
# Soda Music). Every section is gated on the executable existing, so this suite is a clean
# no-op on machines without them. It proves: launch with a sanitized environment, a11y
# parsing of shipping apps, pattern actions where the app exposes them, and the rectangle
# fallback where it exposes no runtime id at all.
$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$cua = $env:CUA_TEST_BIN
if (-not $cua -or -not (Test-Path $cua)) { $cua = "$root\bin\cua.exe" }
if (-not (Test-Path $cua)) { $cua = "$root\target\release\cua.exe" }
if (-not (Test-Path $cua)) { $cua = "$root\target\debug\cua.exe" }
$dataRoot = if ($env:CUA_DATA_DIR) { $env:CUA_DATA_DIR } else { Join-Path $env:LOCALAPPDATA "cua" }
$cache = Join-Path $dataRoot "cache"
Write-Host ("== binary: {0} ({1})" -f $cua, (Get-FileHash $cua -Algorithm SHA256).Hash.Substring(0, 12)) -ForegroundColor DarkGray
New-Item -ItemType Directory -Force -Path $cache | Out-Null
$script:pass = 0; $script:fail = 0; $script:skips = 0; $script:failures = @()

function J($out) {
    $s = ($out -join "`n")
    $b = $s.IndexOf("["); $c = $s.IndexOf("{")
    if ($b -ge 0 -and ($c -lt 0 -or $b -lt $c)) { $s = $s.Substring($b) }
    elseif ($c -ge 0) { $s = $s.Substring($c) }
    $s | ConvertFrom-Json
}
function Check($name, $cond, $detail = "") {
    if ($cond) { $script:pass++; Write-Host ("  PASS  {0}" -f $name) -ForegroundColor Green }
    else { $script:fail++; $script:failures += $name; Write-Host ("  FAIL  {0}  {1}" -f $name, $detail) -ForegroundColor Red }
}
function Skip($name, $why) {
    $script:skips++; Write-Host ("  SKIP  {0}  ({1})" -f $name, $why) -ForegroundColor DarkGray
}
function RunCua { & $cua @args 2>$null }
function Node($st, $type, $name) {
    $st.nodes | Where-Object { $_.type -eq $type -and $_.name -eq $name } | Select-Object -First 1
}
function TitleOf($hwnd) {
    (J (RunCua apps)).windows | Where-Object { $_.hwnd -eq $hwnd } | ForEach-Object { $_.title }
}
function HwndOf($procName) {
    $w = (J (RunCua apps)).windows | Where-Object { $_.process -eq $procName -and $_.visible } |
        Sort-Object { -($_.rect[2] * $_.rect[3]) } | Select-Object -First 1
    if ($w) { $w.hwnd }
}
function WaitGone($procName, $timeoutSec = 30) {
    for ($k = 0; $k -lt ($timeoutSec * 2); $k++) {
        if (-not (HwndOf $procName)) { return $true }
        Start-Sleep -Milliseconds 500
    }
    $false
}
function WaitHwnd($procName, $timeoutSec = 40) {
    for ($k = 0; $k -lt ($timeoutSec * 2); $k++) {
        $h = HwndOf $procName
        if ($h) { return $h }
        Start-Sleep -Milliseconds 500
    }
    $null
}

Write-Host "== cua e2e phase-5 (cli: $cua)" -ForegroundColor Cyan

# ---------- A. QQ Music (Tencent TXGuiFoundation: named UIA nodes, no runtime ids) ----------
$qqExe = "D:\Softwares\QQMusic\QQMusic.exe"
if (-not (Test-Path $qqExe)) { $qqExe = "C:\Program Files (x86)\Tencent\QQMusic\QQMusic.exe" }
if (-not (Test-Path $qqExe)) {
    Skip "A. QQ Music" "not installed"
    Skip "A. QQ Music (rest)" "not installed"
} else {
    taskkill /IM QQMusic.exe /F 2>$null | Out-Null
    WaitGone "QQMusic.exe" | Out-Null
    $r = J (RunCua launch --exe $qqExe)
    Check "A1 launch with a sanitized environment" ($r.status -eq "ok" -and $r.detail -like "*env sanitized*") "$($r.detail)"
    $W = WaitHwnd "QQMusic.exe"
    Check "A2 main window appears" ($null -ne $W) "hwnd=$W"
    if ($W) {
        Start-Sleep -Seconds 6
        $st = J (RunCua state --window $W --no-shot)
        Check "A3 UIA tree is populated" ($st.node_count -gt 80) "nodes=$($st.node_count)"
        $btn = $st.nodes | Where-Object { $_.type -eq "Button" -and ($_.name -eq "播放" -or $_.name -eq "暂停") } | Select-Object -First 1
        Check "A4 play/pause button found by name" ($null -ne $btn) "nodes=$($st.node_count)"
        Check "A5 provider exposes no runtime id" (@($btn.rid).Count -eq 0) "rid=$($btn.rid -join ',')"
        $before = $btn.name
        $c = J (RunCua click --window $W --index $btn.i)
        Check "A6 click --index falls back to the cached rect" ($c.status -eq "ok" -and $c.detail -like "*no runtime id*") "$($c.detail)"
        Start-Sleep -Seconds 2
        $st2 = J (RunCua state --window $W --no-shot)
        $after = (Node $st2 "Button" $before)
        $flipped = $st2.nodes | Where-Object { $_.type -eq "Button" -and $_.name -ne $before -and (@('播放','暂停') -contains $_.name) } | Select-Object -First 1
        Check "A7 playback state toggles and reads back" ($null -ne $flipped -and $null -eq $after) "before=$before"
        Check "A7b element was relocated by name, not by stale index" ($c.detail -like "*re-located*") "$($c.detail)"
        # Message-level input reaches TXGuiFoundation too, and needs no foreground window.
        $st3 = J (RunCua state --window $W --no-shot)
        $btn3 = $st3.nodes | Where-Object { $_.type -eq "Button" -and @("播放","暂停") -contains $_.name } | Select-Object -First 1
        $state3 = $btn3.name
        $c = J (RunCua click --window $W --index $btn3.i --background)
        Check "A8a background click reports message-level input" ($c.status -eq "ok" -and $c.detail -like "*message-level*") "$($c.detail)"
        Start-Sleep -Seconds 2
        $st4 = J (RunCua state --window $W --no-shot)
        $btn4 = $st4.nodes | Where-Object { $_.type -eq "Button" -and @("播放","暂停") -contains $_.name } | Select-Object -First 1
        Check "A8 background click toggles playback" ($btn4.name -ne $state3) "$state3 -> $($btn4.name)"
        $sh = J (RunCua shot --window $W --out "$cache\p5-qq.png")
        Check "A9 window capture renders" ($sh.rendered -eq $true) "rendered=$($sh.rendered)"
        $cl = J (RunCua close --window $W --confirm)
        Check "A10 close" ($cl.status -eq "ok") "$($cl.detail)"
    }
}

# ---------- B. NetEase Cloud Music (CEF: needs --force-renderer-accessibility) ----------
$neExe = "D:\Softwares\CloudMusic\cloudmusic.exe"
if (-not (Test-Path $neExe)) { $neExe = "C:\Program Files (x86)\NetEase\CloudMusic\cloudmusic.exe" }
if (-not (Test-Path $neExe)) {
    Skip "B. NetEase Cloud Music" "not installed"
    Skip "B. NetEase Cloud Music (rest)" "not installed"
} else {
    taskkill /IM cloudmusic.exe /F 2>$null | Out-Null
    WaitGone "cloudmusic.exe" | Out-Null
    $r = J (RunCua launch --exe $neExe --args "--force-renderer-accessibility")
    Check "B1 launch with renderer accessibility" ($r.status -eq "ok") "$($r.detail)"
    $W = WaitHwnd "cloudmusic.exe" 60
    Check "B2 main window appears" ($null -ne $W) "hwnd=$W"
    if ($W) {
        for ($k = 0; $k -lt 12; $k++) {
            $st = J (RunCua state --window $W --no-shot --patterns)
            if ($st.node_count -gt 300) { break }
            Start-Sleep -Seconds 2
        }
        Check "B3 DOM is exposed through UIA" ($st.node_count -gt 300) "nodes=$($st.node_count)"
        $next = $st.nodes | Where-Object { $_.type -eq "Button" -and $_.name -eq "next" } | Select-Object -First 1
        Check "B4 'next' button carries InvokePattern" ($null -ne $next -and @($next.patterns) -contains "Invoke") "$($next.patterns -join ',')"
        $t1 = TitleOf $W
        $c = J (RunCua click --window $W --index $next.i)
        Check "B5 invoked via UIA InvokePattern" ($c.status -eq "ok" -and $c.detail -like "*InvokePattern*") "$($c.detail)"
        Start-Sleep -Seconds 5
        Check "B6 the playing track changed" ((TitleOf $W) -ne $t1) "'$t1' -> '$(TitleOf $W)'"
        $edit = (J (RunCua state --window $W --no-shot --patterns)).nodes |
            Where-Object { $_.type -eq "Edit" } | Select-Object -First 1
        $v = J (RunCua set-value --window $W --index $edit.i --value "BEJ48")
        $back = J (RunCua get-value --window $W --index $edit.i)
        Check "B7 search box ValuePattern round-trip" ($v.status -eq "ok" -and $back.value -eq "BEJ48") "$($back.value)"
        & $cua key --window $W --keys enter | Out-Null
        Start-Sleep -Seconds 6
        $st = J (RunCua state --window $W --no-shot)
        $hit = $st.nodes | Where-Object { $_.name -like "*BEJ48*" } | Select-Object -First 1
        Check "B8 search results render in the UIA tree" ($null -ne $hit) "nodes=$($st.node_count)"
        $play = $st.nodes | Where-Object { $_.type -eq "Button" -and $_.name -like "play*" } | Select-Object -First 1
        $c = J (RunCua click --window $W --index $play.i)
        Start-Sleep -Seconds 5
        Check "B9 results play button works" ($c.status -eq "ok" -and (TitleOf $W) -ne "") "title=$(TitleOf $W)"
        $cl = J (RunCua close --window $W --confirm)
        Check "B10 close" ($cl.status -eq "ok") "$($cl.detail)"
    }
}

# ---------- C. Soda Music (Chromium shell, a11y off and hostile to the switch) ----------
$sodaExe = "D:\Softwares\Soda Music\3.5.1\SodaMusic.exe"
if (-not (Test-Path $sodaExe)) {
    Skip "C. Soda Music" "not installed"
    Skip "C. Soda Music (rest)" "not installed"
} else {
    taskkill /IM SodaMusic.exe /F 2>$null | Out-Null
    WaitGone "SodaMusic.exe" | Out-Null
    $r = J (RunCua launch --exe $sodaExe)
    Check "C1 launch with a path containing spaces" ($r.status -eq "ok") "$($r.detail)"
    $W = WaitHwnd "SodaMusic.exe" 60
    Check "C2 main window appears" ($null -ne $W) "hwnd=$W"
    if ($W) {
        # Soda Music's Chromium accessibility switches itself on while a UIA client walks the
        # tree and off again when it goes idle, so the same window reads 9 nodes and 86 nodes
        # minutes apart. Poll for a populated capture instead of assuming either shape.
        $best = 0
        for ($k = 0; $k -lt 8; $k++) {
            $st = J (RunCua state --window $W --no-shot)
            if ($st.node_count -gt $best) { $best = $st.node_count }
            if ($best -gt 40) { break }
            Start-Sleep -Seconds 2
        }
        Check "C3 DOM becomes parseable while observed" ($best -gt 40) "best=$best"
        $sh = J (RunCua shot --window $W --out "$cache\p5-soda.png")
        Check "C4 window capture renders" ($sh.rendered -eq $true) "rendered=$($sh.rendered)"
        $cl = J (RunCua close --window $W --confirm)
        Check "C5 close gracefully" ($cl.status -eq "ok") "$($cl.detail)"
    }
}

# ---------- result ----------
Write-Host ""
Write-Host ("==== PHASE-5 RESULT: {0} passed, {1} failed, {2} skipped" -f $script:pass, $script:fail, $script:skips) -ForegroundColor $(if ($script:fail -eq 0) { "Green" } else { "Red" })
if ($script:fail -gt 0) { $script:failures | ForEach-Object { Write-Host ("  failed: " + $_) -ForegroundColor Red }; exit 1 }
exit 0
