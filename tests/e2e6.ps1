# tests/e2e6.ps1 - long-horizon scenario: drive the Bilibili desktop client end to end
# (launch -> search -> sort -> open the uploader -> play -> verify motion -> seek -> close)
# while `cua rec` records the whole screen, then encode the video and replay the action trace.
# Skips cleanly when the client is not installed.
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
$recName = "bili-e2e"
$trace = "bili-run"
$script:pass = 0; $script:fail = 0; $script:skip = 0; $script:failures = @()

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
    $script:skip++; Write-Host ("  SKIP  {0}  ({1})" -f $name, $why) -ForegroundColor DarkGray
}
function RunCua { & $cua @args 2>$null }
function Ground($sel, $q) { (J (RunCua grounding --window $sel --query $q)).matches | Select-Object -First 1 }
function Shot($sel, $p) { (J (RunCua shot --window $sel --out $p)) }
function Md5($p) { if (Test-Path $p) { (Get-FileHash -Algorithm MD5 -Path $p).Hash } else { "" } }
function WinTitles($proc) { (J (RunCua apps)).windows | Where-Object { $_.process -eq $proc -and $_.visible } }

Write-Host "== cua e2e phase-6: long-horizon Bilibili run (cli: $cua)" -ForegroundColor Cyan
$exe = "D:\Softwares\bilibili\哔哩哔哩.exe"
if (-not (Test-Path $exe)) {
    Write-Host "  SKIP  Bilibili client not installed at $exe" -ForegroundColor DarkGray
    exit 0
}

# ---------- 0. clean slate + start recording ----------
& $cua rec stop 2>$null | Out-Null
Remove-Item (Join-Path (Join-Path $dataRoot "recordings") ($trace + ".jsonl")) -Force -ErrorAction SilentlyContinue
taskkill /IM "哔哩哔哩.exe" /F 2>$null | Out-Null
Start-Sleep -Seconds 2
$r = J (RunCua rec start --fps 2 --out $recName --max-seconds 600)
Check "R1 screen recording started" ($r.status -eq "ok") "$($r.detail)"

# ---------- 1. launch with renderer accessibility ----------
$r = J (RunCua launch --exe $exe --args "--force-renderer-accessibility" --record $trace)
Check "L1 launch reports a sanitized environment" ($r.status -eq "ok" -and $r.detail -like "*env sanitized*") "$($r.detail)"
$W = $null
for ($k = 0; $k -lt 60; $k++) {
    $W = (WinTitles "哔哩哔哩.exe" | Sort-Object { -($_.rect[2] * $_.rect[3]) } | Select-Object -First 1)
    if ($W) { break }
    Start-Sleep -Milliseconds 500
}
Check "L2 main window appears" ($null -ne $W) "no window"
if (-not $W) { & $cua rec stop 2>$null | Out-Null; exit 1 }
$h = $W.hwnd
Start-Sleep -Seconds 6

# ---------- 2. home page is parseable ----------
$st = J (RunCua state --window $h --no-shot)
Check "L3 DOM is exposed through UIA" ($st.node_count -gt 150) "nodes=$($st.node_count)"
$edit = Ground $h "搜索"
if (-not $edit) { $edit = ($st.nodes | Where-Object { $_.type -eq "Edit" } | Select-Object -First 1) }
Check "L4 search box found by grounding" ($null -ne $edit -and $edit.type -eq "Edit") "$($edit | ConvertTo-Json -Compress)"

# ---------- 3. search ----------
$c = J (RunCua click --window $h --index $edit.i --record $trace)
Check "L5 search box clicked" ($c.status -eq "ok") "$($c.detail)"
& $cua type --window $h --text "BEJ48" --record $trace | Out-Null
Start-Sleep -Seconds 2
& $cua key --window $h --keys enter --record $trace | Out-Null
$tab = $null
for ($k = 0; $k -lt 15; $k++) {
    Start-Sleep -Seconds 1
    $tab = Ground $h "最新发布"
    if ($tab) { break }
}
Check "L6 results page reached (sort bar present)" ($null -ne $tab) "$($tab | ConvertTo-Json -Compress)"

# ---------- 4. sort by newest, verify the tree changed ----------
$st = J (RunCua state --window $h --no-shot)
$c = J (RunCua click --window $h --index $tab.i --record $trace)
Check "L7 newest-sort invoked via accessibility" ($c.status -eq "ok") "$($c.detail)"
Start-Sleep -Seconds 3
$st2 = J (RunCua state --window $h --no-shot --diff)
Check "L8 tree changed after sorting" (@($st2.diff.added).Count + @($st2.diff.removed).Count -gt 0) "nodes=$($st2.node_count)"

# ---------- 5. open the uploader and their newest upload ----------
$u = Ground $h "用户"
$c = J (RunCua click --window $h --index $u.i --record $trace)
Start-Sleep -Seconds 3
$acc = $null
for ($k = 0; $k -lt 10; $k++) {
    $st = J (RunCua state --window $h --no-shot)
    $acc = $st.nodes | Where-Object { $_.type -eq "Hyperlink" -and $_.name -eq "BEJ48" } | Select-Object -First 1
    if ($acc) { break }
    Start-Sleep -Seconds 1
}
Check "L9 uploader account found" ($null -ne $acc) "$($acc | ConvertTo-Json -Compress)"
$c = J (RunCua click --window $h --index $acc.i --record $trace)
Check "L10 account page opened" ($c.status -eq "ok") "$($c.detail)"
$cover = $null
for ($k = 0; $k -lt 12; $k++) {
    Start-Sleep -Seconds 2
    $st = J (RunCua state --window $h --no-shot)
    $cover = $st.nodes | Where-Object { $_.type -eq "Image" -and $_.rect[2] -gt 300 -and $_.rect[3] -gt 150 } |
        Sort-Object { $_.rect[1] } | Select-Object -First 1
    if ($cover) { break }
}
Check "L11 newest upload cover located" ($null -ne $cover) "$($cover | ConvertTo-Json -Compress)"
$sh = Shot $h "$cache\p6-space.png"
$click = J (RunCua click --window $h --x ($cover.rect[0] + [int]($cover.rect[2] / 2)) --y ($cover.rect[1] + [int]($cover.rect[3] / 2)) --shot-id $sh.state_id --record $trace)
Check "L12 cover clicked at its centre" ($click.status -eq "ok") "$($click.detail)"

# ---------- 6. the player opens in its own window ----------
$pl = $null
for ($k = 0; $k -lt 20; $k++) {
    Start-Sleep -Seconds 1
    $pl = (WinTitles "哔哩哔哩.exe" | Where-Object { $_.title -and $_.title -notlike "*bilibili*" } |
        Sort-Object { -($_.rect[2] * $_.rect[3]) } | Select-Object -First 1)
    if ($pl) { break }
}
Check "L13 player window opened with the video as title" ($null -ne $pl) "title=$($pl.title)"
if ($pl) {
    $ph = $pl.hwnd
    Start-Sleep -Seconds 4
    $t1 = Md5 (Shot $ph "$cache\p6-f1.png").path
    Start-Sleep -Seconds 3
    $t2 = Md5 (Shot $ph "$cache\p6-f2.png").path
    Check "L14 the video is actually playing (frames differ)" ($t1 -ne "" -and $t1 -ne $t2) "$t1 / $t2"

    # seek: arrow keys move the playhead. Verify the key receipts first, then use
    # a range-owned signal when the player exposes one; otherwise compare short
    # time labels. A player that exposes neither gets an explicit skip, not a
    # random pass/fail.
    function TimeLabels($st) {
        @($st.nodes | Where-Object { $_.name -match "\d{1,2}:\d{2}" -and $_.name.Length -lt 60 } |
            ForEach-Object { $_.name } | Sort-Object -Unique)
    }
    # Put keyboard focus on the video surface first: raising the top-level window
    # is not enough for players that keep focus on a toolbar or the page.
    $st = J (RunCua state --window $ph --no-shot)
    $seekShot = J (RunCua shot --window $ph --out "$cache\p6-seek.png")
    $plr = $pl.rect
    $focus = J (RunCua click --window $ph --x ($plr[0] + [int]($plr[2] / 2)) --y ($plr[1] + [int]($plr[3] / 2)) --shot-id $seekShot.state_id)
    Start-Sleep -Milliseconds 600
    $k1 = J (RunCua key --window $ph --keys "right" --record $trace)
    $k2 = J (RunCua key --window $ph --keys "right" --record $trace)
    Start-Sleep -Seconds 2
    $st2 = J (RunCua state --window $ph --no-shot)
    Check "L15 seek focus + keys were accepted" ($focus.status -eq "ok" -and $k1.status -eq "ok" -and $k2.status -eq "ok") "focus=$($focus.status) k1=$($k1.status) k2=$($k2.status)"
    $rng0 = $st.nodes | Where-Object { $null -ne $_.range -and $_.range[2] -gt 60 } | Select-Object -First 1
    $rng1 = if ($rng0) { $st2.nodes | Where-Object { $_.i -eq $rng0.i } | Select-Object -First 1 } else { $null }
    $before = TimeLabels $st
    $after = TimeLabels $st2
    $moved = $false
    $why = ""
    if ($rng0 -and $rng1 -and $rng0.range[0] -ne $rng1.range[0]) {
        $moved = $true
        $why = "range $($rng0.range[0]) -> $($rng1.range[0])"
    } elseif ($before.Count -gt 0 -and @(Compare-Object $before $after).Count -gt 0) {
        $moved = $true
        $why = ($before -join "|") + " -> " + ($after -join "|")
    } elseif (-not $rng0 -and $before.Count -eq 0) {
        Skip "L15 seeking with the right arrow moves the playhead" "player exposes no seek signal"
    } else {
        $why = "range=$($rng0.range -join ',') labels=" + ($before -join "|") + " -> " + ($after -join "|")
    }
    if ($moved) {
        Check "L15 seeking with the right arrow moves the playhead" $true $why
    } elseif ($rng0 -or $before.Count -gt 0) {
        Check "L15 seeking with the right arrow moves the playhead" $false $why
    }

    # window geometry control
    $r = J (RunCua resize --window $ph --width 1100 --height 720 --x 120 --y 120)
    Start-Sleep -Milliseconds 800
    $w2 = (WinTitles "哔哩哔哩.exe" | Where-Object { $_.hwnd -eq $ph } | Select-Object -First 1)
    Check "L16 resize moves and sizes the player" ($w2.rect[0] -eq 120 -and $w2.rect[2] -eq 1100) "rect=$($w2.rect -join ',')"

    # pause through the accessibility tree, then close
    $cl = J (RunCua close --window $ph --confirm --record $trace)
    Check "L17 player closed" ($cl.status -eq "ok") "$($cl.detail)"
}

# ---------- 7. stop recording and check the artifact ----------
$st = J (RunCua rec status)
Check "R2 frames accumulated during the run" ($st.frames -gt 30) "frames=$($st.frames)"
$rs = J (RunCua rec stop)
Check "R3 recording encoded" ($rs.status -eq "ok" -and ($rs.target.encoded -in @("mp4", "html", "frames"))) "$($rs.detail)"
$dir = $rs.target.dir
$vid = if ($rs.target.encoded -eq "mp4") { $rs.target.video } else { Join-Path $dir "player.html" }
Check "R4 artifact exists and is not trivial" ((Test-Path $vid) -and ((Get-Item $vid).Length -gt 50KB)) "$vid"

# ---------- 8. the action trace itself ----------
$tp = (Join-Path (Join-Path $dataRoot "recordings") ($trace + ".jsonl"))
$lines = @()
if (Test-Path $tp) { $lines = @(Get-Content $tp | Where-Object { $_.Trim() }) }
$cmds = $lines | ForEach-Object { ($_ | ConvertFrom-Json).cmd } | Sort-Object -Unique
Check "R5 every action was recorded to one trace" ($lines.Count -ge 10) "steps=$($lines.Count) at $tp"
Check "R6 trace covers launch/click/type/key/close" (@(Compare-Object $cmds @("click","close","key","launch","type") -SyncWindow 0).Count -eq 0) "cmds=$($cmds -join ',')"
$rp = J (RunCua replay $trace)
Check "R7 replay runs the trace and reports per-step status honestly" (@($rp.steps).Count -ge 1 -and $rp.steps[0].ok -eq $true -and $rp.ok -eq $false) "steps=$(@($rp.steps).Count) ok=$($rp.ok)"

# ---------- 9. result ----------
Write-Host ""
Write-Host ("  recording artifact: {0}" -f $vid) -ForegroundColor Cyan
Write-Host ("==== PHASE-6 RESULT: {0} passed, {1} failed, {2} skipped" -f $script:pass, $script:fail, $script:skip) -ForegroundColor $(if ($script:fail -eq 0) { "Green" } else { "Red" })
if ($script:fail -gt 0) { $script:failures | ForEach-Object { Write-Host ("  failed: " + $_) -ForegroundColor Red }; exit 1 }
exit 0
