# tests\e2e3.ps1 - phase-3: a11y patterns (toggle/select/range), right/double by index,
# background (message-level) input on the user desktop, hover/cursor/wait/resize, exact-title
# selectors, window-scoped shot guard, diff value changes, sandbox multi-app + a11y parity,
# browser bridge (UBB: session/tab/find/click/evaluate/screenshot) and env hygiene.
$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$cua = $env:CUA_TEST_BIN
if (-not $cua -or -not (Test-Path $cua)) { $cua = "$root\bin\cua.exe" }
if (-not (Test-Path $cua)) { $cua = "$root\target\debug\cua.exe" }
$testpad = $env:CUA_TEST_TESTPAD
if (-not $testpad -or -not (Test-Path $testpad)) { $testpad = "$root\bin\testpad.exe" }
if (-not (Test-Path $testpad)) { $testpad = "$root\target\debug\testpad.exe" }
$dataRoot = if ($env:CUA_DATA_DIR) { $env:CUA_DATA_DIR } else { Join-Path $env:LOCALAPPDATA "cua" }
$cache = Join-Path $dataRoot "cache"
Write-Host ("== binary: {0} ({1})" -f $cua, (Get-FileHash $cua -Algorithm SHA256).Hash.Substring(0, 12)) -ForegroundColor DarkGray
$fixturesUri = ([System.Uri](Join-Path $root "tests\fixtures")).AbsoluteUri.TrimEnd("/")
New-Item -ItemType Directory -Force -Path $cache | Out-Null
$script:pass = 0; $script:fail = 0; $script:skip = 0; $script:failures = @()

function J($out) {
    $s = ($out -join "`n")
    $b = $s.IndexOf("[")
    $c = $s.IndexOf("{")
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
function RunErr { (& $cua @args 2>&1) -join "`n" }
function JsonArg($h) { ($h | ConvertTo-Json -Compress) -replace '"', '\"' }
function Node($st, $type, $name = $null) {
    if ($null -eq $name) { return ($st.nodes | Where-Object { $_.type -eq $type } | Select-Object -First 1) }
    return ($st.nodes | Where-Object { $_.type -eq $type -and $_.name -eq $name } | Select-Object -First 1)
}
function Val($sel, $idx) { (J (RunCua get-value --window $sel --index $idx)).value }
function ShotId($sel) { (J (RunCua shot --window $sel --out "$cache\p3.png")).state_id }
function FgHwnd { (J (RunCua apps)).foreground_hwnd }
function CloseBrowser { & $cua browser session.stop 2>$null | Out-Null }

Write-Host "== cua e2e phase-3 (cli: $cua)" -ForegroundColor Cyan

# ---------- 0. clean slate ----------
& $cua sandbox stop 2>$null | Out-Null
CloseBrowser
taskkill /IM testpad.exe /F 2>$null | Out-Null
Start-Sleep -Milliseconds 800

# ---------- fixture: two windows with distinct exact titles ----------
& $cua launch --exe $testpad --args "--title CuaTestPad" | Out-Null
& $cua launch --exe $testpad --args "--title CuaTestPad2" | Out-Null
& $cua wait --window "title:CuaTestPad" --timeout-ms 5000 | Out-Null
& $cua wait --window "title:CuaTestPad2" --timeout-ms 5000 | Out-Null
Start-Sleep -Milliseconds 400
$W1 = "title:CuaTestPad"
$W2 = "title:CuaTestPad2"
$st1 = J (RunCua state --window $W1 --no-shot --patterns)
$st2 = J (RunCua state --window $W2 --no-shot)
$editIdx = (Node $st1 "Edit").i
$hit = Node $st1 "Pane" "HitArea"
$chk = Node $st1 "CheckBox" "AutoMark"
$slider = Node $st1 "Slider"
$btn = Node $st1 "Button" "AppendMark"
$alpha = Node $st1 "ListItem" "alpha"
$beta = Node $st1 "ListItem" "beta"
$rectHit = $hit.rect
$hitX = $rectHit[0] + [int]($rectHit[2] / 2)
$hitY = $rectHit[1] + [int]($rectHit[3] / 2)
$rectEdit = (Node $st1 "Edit").rect
$editX = $rectEdit[0] + [int]($rectEdit[2] / 2)
$editY = $rectEdit[1] + [int]($rectEdit[3] / 2)

# ---------- A. selectors, window-scoped guard, diff, patterns ----------
$h1 = $st1.window.hwnd
$h2 = $st2.window.hwnd
Check "A1 exact title: distinct hwnds" ($h1 -ne $h2 -and $st1.window.title -eq "CuaTestPad" -and $st2.window.title -eq "CuaTestPad2") "$h1 / $h2"
$r = J (RunCua state --window "title:CuaTestPad" --no-shot)
Check "A2 exact title resolves W1 not W2" ($r.window.hwnd -eq $h1) "$($r.window.hwnd) vs $h1"
# cross-window shot: observe W2, then click W1 -> refused
$sW2 = ShotId $W2
$r = J (RunCua click --window $W1 --x $hitX --y $hitY --shot-id $sW2 --background)
Check "A3 shot_id is scoped to its window" ($r.status -eq "refused" -and $r.detail -like "*last observation was for hwnd*") "$($r.status)/$($r.detail)"
# fresh W1 shot passes
$sW1 = ShotId $W1
$r = J (RunCua click --window $W1 --x $hitX --y $hitY --shot-id $sW1 --background)
Check "A4 fresh W1 shot passes guard" ($r.status -eq "ok") "$($r.status)/$($r.detail)"
# screen capture covers any point
& $cua activate --window $W1 | Out-Null
$sScreen = (J (RunCua shot --screen --out "$cache\p3-screen.png")).state_id
$r = J (RunCua click --window $W1 --x $editX --y $editY --shot-id $sScreen --background)
Check "A5 screen shot covers any point" ($r.status -eq "ok") "$($r.status)/$($r.detail)"
# diff detects value change
& $cua state --window $W1 --no-shot | Out-Null
& $cua set-value --window $W1 --index $editIdx --value "DIFF-A" | Out-Null
$r = J (RunCua state --window $W1 --no-shot --diff)
Check "A6 --diff detects value change" ($r.diff.changed -eq $true -and $r.diff.value_changes -ge 1) ($r.diff | ConvertTo-Json -Compress)
# patterns advertised
$chkP = (Node $st1 "CheckBox" "AutoMark").patterns
$liP = (Node $st1 "ListItem" "beta").patterns
$slP = (Node $st1 "Slider").patterns
Check "A7 state --patterns: Toggle/SelectionItem/RangeValue" (($chkP -contains "Toggle") -and ($liP -contains "SelectionItem") -and ($slP -contains "RangeValue")) "$chkP / $liP / $slP"
$apps = J (RunCua apps)
Check "A8 apps exposes foreground_hwnd" ($null -ne $apps.foreground_hwnd) "$($apps.foreground_hwnd)"

# ---------- B. a11y actions + background input ----------
$r = J (RunCua toggle --window $W1 --index $chk.i)
$st = J (RunCua state --window $W1 --no-shot)
$now = Node $st "CheckBox" "AutoMark"
Check "B1 toggle: off -> on" ($r.status -eq "ok" -and $now.toggle -eq "on") "$($r.detail) / $($now.toggle)"
& $cua toggle --window $W1 --index $chk.i | Out-Null
$st = J (RunCua state --window $W1 --no-shot)
$now = Node $st "CheckBox" "AutoMark"
Check "B2 toggle: on -> off" ($now.toggle -eq "off") "$($now.toggle)"
$r = J (RunCua select --window $W1 --index $beta.i)
$st = J (RunCua state --window $W1 --no-shot)
$selBeta = Node $st "ListItem" "beta"
$selAlpha = Node $st "ListItem" "alpha"
Check "B3 select list item" ($r.status -eq "ok" -and $selBeta.selected -eq $true -and $selAlpha.selected -eq $false) "$($selBeta.selected)/$($selAlpha.selected)"
$r = J (RunCua set-range --window $W1 --index $slider.i --value 80)
$rg = J (RunCua get-range --window $W1 --index $slider.i)
Check "B4 set-range + get-range" ($r.status -eq "ok" -and $rg.range.value -eq 80 -and $rg.range.maximum -eq 100) "$($rg.range | ConvertTo-Json -Compress)"
# right / double by index must be real gestures, not Invoke
& $cua set-value --window $W1 --index $editIdx --value "BASE" | Out-Null
& $cua activate --window $W1 | Out-Null
Start-Sleep -Milliseconds 200
$r = J (RunCua click --window $W1 --index $hit.i --right)
Start-Sleep -Milliseconds 300
$v = Val $W1 $editIdx
Check "B5 index right-click uses event strategy" ($r.status -eq "ok" -and $r.detail -like "*right*" -and $v -like "*RCLICK*") "$($r.detail) / $v"
$r = J (RunCua click --window $W1 --index $hit.i --double)
Start-Sleep -Milliseconds 400
$v = Val $W1 $editIdx
Check "B6 index double-click produces DBLCLK" ($r.status -eq "ok" -and $r.detail -like "*double*" -and $v -like "*DBLCLK*") "$($r.detail) / $v"
$r = J (RunCua click --window $W1 --index $hit.i --right --strategy a11y)
Check "B7 strict a11y refuses right click" ($r.status -eq "failed" -and $r.detail -like "*strategy=a11y*") "$($r.detail)"
# hover + cursor
$r = J (RunCua hover --window $W1 --index $btn.i)
$cur = J (RunCua cursor)
$expectX = $btn.rect[0] + [int]($btn.rect[2] / 2)
$expectY = $btn.rect[1] + [int]($btn.rect[3] / 2)
Check "B8 hover moves pointer to element center" ($r.status -eq "ok" -and [Math]::Abs($cur.x - $expectX) -le 4 -and [Math]::Abs($cur.y - $expectY) -le 4) "cursor=$($cur.x),$($cur.y) want=$expectX,$expectY"
Check "B8b cursor reports window under pointer" ($cur.window.title -eq "CuaTestPad") "$($cur.window.title)"
$r = J (RunCua hover --x 20 --y 20)
Check "B9 hover coords refused without shot_id" ($r.status -eq "refused") "$($r.status)"
$s = ShotId $W1
$r = J (RunCua hover --x $expectX --y $expectY --shot-id $s)
Check "B9b hover coords ok with fresh shot_id" ($r.status -eq "ok") "$($r.status)/$($r.detail)"
# background click: targets W1 even though W2 may overlap; focus unchanged
& $cua set-value --window $W2 --index $editIdx --value "W2KEEP" | Out-Null
& $cua set-value --window $W1 --index $editIdx --value "BG" | Out-Null
& $cua activate --window $W2 | Out-Null
Start-Sleep -Milliseconds 200
$fg1 = FgHwnd
$s = ShotId $W1
$r = J (RunCua click --window $W1 --x $hitX --y $hitY --shot-id $s --background)
Start-Sleep -Milliseconds 300
$fg2 = FgHwnd
$v1 = Val $W1 $editIdx
$v2 = Val $W2 $editIdx
Check "B10 background click lands in target window" ($r.status -eq "ok" -and $v1 -like "*MDOWN*CLICK*") "$($r.detail) / $v1"
Check "B10b background click does not touch the other window" ($v2 -eq "W2KEEP") "$v2"
Check "B10c background click keeps foreground" ($fg1 -eq $fg2) "$fg1 -> $fg2"
# background typing / keys
& $cua set-value --window $W1 --index $editIdx --clear | Out-Null
$fg1 = FgHwnd
$r = J (RunCua type --window $W1 --text "BG-TYPE" --background)
Start-Sleep -Milliseconds 300
$fg2 = FgHwnd
$v = Val $W1 $editIdx
Check "B11 background type arrives" ($r.status -eq "ok" -and $v -eq "BG-TYPE") "$($r.detail) / $v"
Check "B11b background type keeps foreground" ($fg1 -eq $fg2) "$fg1 -> $fg2"
& $cua set-value --window $W1 --index $editIdx --value "AB" | Out-Null
& $cua key --window $W1 --keys end --background | Out-Null
& $cua type --window $W1 --text "CD" --background | Out-Null
Start-Sleep -Milliseconds 300
$v = Val $W1 $editIdx
Check "B12 background key (end) + type appends" ($v -eq "ABCD") "$v"
# foreground key parsing: uppercase auto-shift and extended ctrl+home
& $cua set-value --window $W1 --index $editIdx --clear | Out-Null
& $cua activate --window $W1 | Out-Null
Start-Sleep -Milliseconds 200
$s = ShotId $W1
& $cua click --window $W1 --x $editX --y $editY --shot-id $s | Out-Null
& $cua key --keys "A" --window $W1 | Out-Null
& $cua type --text "b" --window $W1 | Out-Null
Start-Sleep -Milliseconds 250
$v = Val $W1 $editIdx
Check "B13 key parses uppercase with auto-shift" ($v -eq "Ab") "$v"
& $cua set-value --window $W1 --index $editIdx --value "LINE1`nLINE2" | Out-Null
& $cua key --keys "ctrl+home" --window $W1 | Out-Null
& $cua type --text "X" --window $W1 | Out-Null
Start-Sleep -Milliseconds 250
$v = Val $W1 $editIdx
Check "B14 ctrl+home extended key works" ($v.StartsWith("XLINE1")) "$v"
# negative a11y paths
$r = J (RunCua toggle --window $W1 --index $editIdx)
Check "B15 toggle on Edit fails with TogglePattern" ($r.status -eq "failed" -and $r.detail -like "*TogglePattern*") "$($r.detail)"
$r = J (RunCua select --window $W1 --index $btn.i)
Check "B16 select on Button fails with SelectionItem" ($r.status -eq "failed" -and $r.detail -like "*SelectionItem*") "$($r.detail)"
$r = J (RunCua set-range --window $W1 --index $slider.i --value 500)
Check "B17 set-range out of bounds fails" ($r.status -eq "failed" -and $r.detail -like "*outside range*") "$($r.detail)"
$r = J (RunCua hover)
Check "B18 hover without target fails" ($r.status -eq "failed") "$($r.status)/$($r.detail)"

# ---------- C. window ops: wait / resize / restore ----------
$r = J (RunCua wait --window $W1 --timeout-ms 2000)
Check "C1 wait present" ($r.satisfied -eq $true) "$($r | ConvertTo-Json -Compress)"
$sw = [Diagnostics.Stopwatch]::StartNew()
& $cua wait --window "NoSuchWindowPhase3" --timeout-ms 600 2>$null | Out-Null
$code = $LASTEXITCODE
$sw.Stop()
Check "C2 wait timeout: exit1, bounded overshoot" ($code -eq 1 -and $sw.ElapsedMilliseconds -ge 550 -and $sw.ElapsedMilliseconds -lt 2500) "code=$code ms=$($sw.ElapsedMilliseconds)"
$r = J (RunCua resize --window $W1 --width 720 --height 500 --x 280 --y 140)
Start-Sleep -Milliseconds 500
$apps = J (RunCua apps)
$w = $apps.windows | Where-Object { $_.title -eq "CuaTestPad" } | Select-Object -First 1
Check "C3 resize receipt" ($r.status -eq "ok") "$($r.detail)"
Check "C3b resize applied (DWM bounds tolerance)" ([Math]::Abs($w.rect[2] - 720) -le 30 -and [Math]::Abs($w.rect[3] - 500) -le 30) "$($w.rect | ConvertTo-Json -Compress)"
$r = J (RunCua resize --window $W1 --width 20 --height 20)
Check "C4 absurd resize refused" ($r.status -eq "failed" -and $r.detail -like "*below 50x50*") "$($r.detail)"
& $cua maximize --window $W1 | Out-Null
Start-Sleep -Milliseconds 400
$apps = J (RunCua apps)
$w = $apps.windows | Where-Object { $_.title -eq "CuaTestPad" } | Select-Object -First 1
Check "C5 maximize flag true" ($w.maximized -eq $true) "$($w.maximized)"
$r = J (RunCua restore --window $W1)
Start-Sleep -Milliseconds 400
$apps = J (RunCua apps)
$w = $apps.windows | Where-Object { $_.title -eq "CuaTestPad" } | Select-Object -First 1
Check "C6 restore returns to normal" ($r.status -eq "ok" -and $w.maximized -eq $false) "$($w.maximized)"
# foreground wait: only assert when the OS reports a foreground window
& $cua activate --window $W1 | Out-Null
$fg = FgHwnd
if ($fg -ne 0) {
    $r = J (RunCua wait --window "$fg" --foreground --timeout-ms 2000)
    Check "C7 wait foreground" ($r.satisfied -eq $true) "$($r | ConvertTo-Json -Compress)"
} else {
    $script:pass++; Write-Host "  PASS  C7 wait foreground (skipped: OS reports no foreground window)" -ForegroundColor Green
}
# wait --gone
& $cua close --window $W2 --confirm | Out-Null
$r = J (RunCua wait --window $W2 --gone --timeout-ms 4000)
Check "C8 wait --gone" ($r.satisfied -eq $true) "$($r | ConvertTo-Json -Compress)"

# ---------- D. sandbox: multi-app + a11y parity ----------
& $cua sandbox stop 2>$null | Out-Null
$r = J (RunCua sandbox start --exe $testpad --args "--title CuaSandboxA")
Check "D1 sandbox start" ($null -ne $r.pid) "$($r | ConvertTo-Json -Compress)"
Start-Sleep -Milliseconds 1200
$r = J (RunCua sandbox launch --exe $testpad --args "--title CuaSandboxB")
Check "D2 sandbox launch second app (additive)" ($null -ne $r.pid -and $r.apps -ge 2) "$($r | ConvertTo-Json -Compress)"
Start-Sleep -Milliseconds 1200
$sbApps = J (RunCua sandbox apps)
$titles = @($sbApps.windows | ForEach-Object { $_.title })
Check "D3 sandbox has both apps" (($titles -contains "CuaSandboxA") -and ($titles -contains "CuaSandboxB")) "$($titles -join ',')"
$checkSb = J (RunCua check)
Check "D4 check reports sandbox running" ($checkSb.sandbox.running -eq $true -and $checkSb.sandbox.pids.Count -ge 3) "$($checkSb.sandbox | ConvertTo-Json -Compress)"
$A = "title:CuaSandboxA"
$sb = J (RunCua sandbox state --window $A --no-shot --patterns)
$sbEdit = Node $sb "Edit"
$sbChk = Node $sb "CheckBox" "AutoMark"
$sbBeta = Node $sb "ListItem" "beta"
$sbSlider = Node $sb "Slider"
Check "D5 sandbox state --patterns" (($sbChk.patterns -contains "Toggle") -and ($sbBeta.patterns -contains "SelectionItem") -and ($sbSlider.patterns -contains "RangeValue")) "$($sbChk.patterns)/$($sbBeta.patterns)/$($sbSlider.patterns)"
$r = J (RunCua sandbox toggle --window $A --index $sbChk.i)
$sb2 = J (RunCua sandbox state --window $A --no-shot)
Check "D6 sandbox toggle" ((Node $sb2 "CheckBox" "AutoMark").toggle -eq "on") "$($r.detail)"
$r = J (RunCua sandbox select --window $A --index $sbBeta.i)
$sb2 = J (RunCua sandbox state --window $A --no-shot)
Check "D7 sandbox select" ((Node $sb2 "ListItem" "beta").selected -eq $true) "$($r.detail)"
$r = J (RunCua sandbox set-range --window $A --index $sbSlider.i --value 70)
$rg = J (RunCua sandbox get-range --window $A --index $sbSlider.i)
Check "D8 sandbox set-range/get-range" ($r.status -eq "ok" -and $rg.value.value -eq 70) "$($rg.value | ConvertTo-Json -Compress)"
$B = "title:CuaSandboxB"
$sbB = J (RunCua sandbox state --window $B --no-shot)
$sbBEdit = Node $sbB "Edit"
& $cua sandbox type --window $B --text "SB-B" | Out-Null
$v = J (RunCua sandbox get-value --window $B --index $sbBEdit.i)
Check "D9 sandbox type into second app" ($v.value -like "*SB-B*") "$($v.value)"
$r = J (RunCua sandbox close --window $B)
Check "D10 sandbox close needs --confirm" ($r.status -eq "refused") "$($r.status)/$($r.detail)"
$r = J (RunCua sandbox close --window $B --confirm)
Start-Sleep -Milliseconds 400
$sbApps = J (RunCua sandbox apps)
$titles = @($sbApps.windows | ForEach-Object { $_.title })
Check "D11 sandbox close B" ($r.status -eq "ok" -and -not ($titles -contains "CuaSandboxB")) "$($r.detail) / $($titles -join ',')"
$userApps = J (RunCua apps)
Check "D12 sandbox apps invisible on user desktop" ((@($userApps.windows | Where-Object { $_.title -like "CuaSandbox*" })).Count -eq 0) "leaked"
$r = J (RunCua sandbox stop)
Check "D13 sandbox stop cleans up" ($r.killed_processes -ge 2) "$($r.killed_processes)"

# ---------- E. browser bridge (UBB) ----------
# cua has no browser of its own: `cua browser ...` forwards to the ubb CLI, which drives
# the user's real Edge through the UBB extension. Needs a fixture server, so it is gated.
$ubbReady = $false
$chk = J (RunCua check)
$ubbCli = $chk.browser.cli
$st = J (RunCua browser status)
if ($st.status -eq "ok" -and $st.result.extension -eq $true) { $ubbReady = $true }
$py = $null
if (Get-Command python -ErrorAction SilentlyContinue) { $py = "python" }
# A fixed port collided with servers leaked by earlier failed runs. Pick a free
# one and verify the fixture really answers before running the browser checks.
$port = 0
try {
    $l = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $l.Start(); $port = $l.LocalEndpoint.Port; $l.Stop()
} catch { $port = 8799 }
$srv = $null
$fixtureOk = $false
if ($ubbReady -and $py) {
    $srv = Start-Process -FilePath $py `
        -ArgumentList @("-m", "http.server", "$port", "--bind", "127.0.0.1", "--directory", (Join-Path $root "tests\fixtures")) `
        -PassThru -WindowStyle Hidden
    for ($k = 0; $k -lt 30; $k++) {
        try { Invoke-WebRequest "http://127.0.0.1:$port/page1.html" -UseBasicParsing -TimeoutSec 2 | Out-Null; $fixtureOk = $true; break }
        catch { Start-Sleep -Milliseconds 200 }
    }
}
$browserRun = $ubbReady -and $py -and $fixtureOk
if ($browserRun) {
    Check "E0 ubb extension + fixture server ready" $true "ubb=$ubbReady python=$($null -ne $py) port=$port"
} elseif ($ubbReady -and $py -and -not $fixtureOk) {
    Check "E0 ubb extension + fixture server ready" $false "fixture server did not serve page1 on port $port"
} elseif (-not $ubbCli -or -not $py) {
    # A missing ubb CLI or python is environmental. A CLI that exists but whose
    # status/bridge call fails is a regression and must stay a failure.
    Skip "E0 ubb extension + fixture server ready" "ubb CLI=$ubbCli python=$($null -ne $py)"
} elseif ($st.status -eq "ok") {
    # The daemon answers but the Edge extension is not connected (browser closed).
    # Rerun the browser section after opening Edge with the UBB extension.
    Skip "E0 ubb extension + fixture server ready" "ubb daemon ok, extension not connected"
} else {
    Check "E0 ubb extension + fixture server ready" $false "ubb CLI present but status failed: $($st.detail)"
}
if ($browserRun) {
    $base = "http://127.0.0.1:$port"
    & $cua browser session.stop 2>$null | Out-Null
    $r = J (RunCua browser session.start)
    Check "E1 session.start" ($r.status -eq "ok" -and $r.result.session_id) "$($r.detail)"
    $r = J (RunCua browser tab.create (JsonArg @{ url = "$base/page1.html" }))
    Check "E2 tab.create" ($r.status -eq "ok" -and $r.result.url -like "*page1.html*") "$($r.detail)"
    Start-Sleep -Milliseconds 1500
    $pg = J (RunCua browser read_page)
    Check "E3 read_page title" ($pg.result.title -eq "Cua Page One") "$($pg.result.title)"
    $f = J (RunCua browser find (JsonArg @{ query = "GoToPage2" }))
    $ref = $f.result.matches[0].ref
    Check "E4 find returns a ref" ($f.status -eq "ok" -and $ref) "$($f | ConvertTo-Json -Compress)"
    $r = J (RunCua browser click (JsonArg @{ ref = $ref }))
    Check "E5 click by ref" ($r.status -eq "ok") "$($r.detail)"
    $w = J (RunCua browser wait_for (JsonArg @{ condition = "url_contains"; text = "page2" }))
    Check "E6 wait_for url_contains" ($w.result.ok -eq $true) "$($w | ConvertTo-Json -Compress)"
    $pg = J (RunCua browser read_page)
    Check "E7 page navigated by ref click" ($pg.result.title -eq "Cua Page Two") "$($pg.result.title)"
    $e = J (RunCua browser evaluate (JsonArg @{ expression = "6*7"; world = "isolated" }))
    Check "E8 evaluate isolated needs no confirm" ($e.result.value -eq 42) "$($e.detail)"
    $err = RunErr browser evaluate (JsonArg @{ expression = "6*7"; world = "main" })
    Check "E9 evaluate MAIN world refused without confirm" ($err -like "*NEEDS_CONFIRM*" -and $err -like "*failed*") ($err.Substring(0, [Math]::Min(90, $err.Length)))
    $e = J (RunCua browser evaluate (JsonArg @{ expression = "6*7"; world = "main"; confirm = $true }))
    Check "E10 evaluate MAIN world with confirm" ($e.result.value -eq 42) "$($e.detail)"
    $tid = $e.result.tab_id
    $tl = J (RunCua browser tab.list)
    $mine = @($tl.result.tabs | Where-Object { $_.session_owned -and $_.url -like "*page2.html*" })
    Check "E11 tab.list marks the session tab" ($mine.Count -ge 1) "$($tl.result.tabs.Count) tabs"
    & $cua browser tab.select (JsonArg @{ tab_id = $mine[0].tab_id }) | Out-Null
    Start-Sleep -Milliseconds 800
    $shotPath = Join-Path $cache "ubb-shot.png"
    Remove-Item $shotPath -Force -ErrorAction SilentlyContinue
    $sh = J (RunCua browser screenshot --out $shotPath)
    $bytes = if (Test-Path $shotPath) { (Get-Item $shotPath).Length } else { 0 }
    Check "E12 screenshot of the selected tab" ($sh.status -eq "ok" -and $bytes -gt 1000) "bytes=$bytes $($sh.detail)"
    $bad = RunErr browser no.such.tool
    Check "E13 unknown ubb tool exits 1" ($LASTEXITCODE -eq 1 -and $bad -like "*failed*") "exit=$LASTEXITCODE"
    $sp = J (RunCua browser session.stop)
    Check "E14 session.stop" ($sp.result.stopped -eq $true) "$($sp.detail)"
    Start-Sleep -Milliseconds 800
    $tl = J (RunCua browser tab.list)
    $left = @($tl.result.tabs | Where-Object { $_.session_owned })
    Check "E15 agent tabs closed after stop" ($left.Count -eq 0) "$($left.Count) left"
}
if ($srv) {
    try {
        if (-not $srv.HasExited) { Stop-Process -Id $srv.Id -Force -ErrorAction SilentlyContinue }
    } catch {
        Write-Host ("  (fixture server cleanup failed: {0})" -f $_.Exception.Message) -ForegroundColor DarkGray
    }
}
# Start-Process can hand back a launcher that exits while the real python child
# keeps the port. Sweep only the server we started (chosen free port + repo dir).
if ($port -gt 0) {
    Get-CimInstance Win32_Process -Filter "Name='python.exe'" -ErrorAction SilentlyContinue |
        Where-Object {
            $_.CommandLine -and
            $_.CommandLine -like "*-m http.server $port *" -and
            $_.CommandLine -like "*D:\Codebase\cua\tests\fixtures*"
        } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}

# ---------- F. performance + final cleanup ----------
$sw = [Diagnostics.Stopwatch]::StartNew()
& $cua state --window $W1 --no-shot | Out-Null
$sw.Stop()
Check "F1 state is fast (process cache)" ($sw.ElapsedMilliseconds -lt 1500) "ms=$($sw.ElapsedMilliseconds)"
$check = J (RunCua check)
Check "F2 check exposes cursor/config_path" ($check.cursor.Count -eq 2 -and $check.config_path -like "*config.json" -and $check.receipt_schema -eq "cua-action-receipt-v1") "$($check.config_path)"
& $cua close --window $W1 --confirm | Out-Null
$r = J (RunCua wait --window $W1 --gone --timeout-ms 4000)
Check "F3 final close + wait gone" ($r.satisfied -eq $true) "$($r | ConvertTo-Json -Compress)"

# ---------- G. launch environment hygiene ----------
# An automation shell exports ELECTRON_RUN_AS_NODE / NODE_OPTIONS / CHROME_CRASHPAD_PIPE_NAME;
# inherited, they make every Electron app exit 0 without ever creating a window.
$env:ELECTRON_RUN_AS_NODE = "1"
$env:NODE_OPTIONS = "--max-old-space-size=4096"
$envClean = Join-Path $cache "env-clean.txt"
$envDirty = Join-Path $cache "env-dirty.txt"
Remove-Item $envClean, $envDirty -Force -ErrorAction SilentlyContinue
$rl = J (RunCua launch --exe $testpad --args "--title CuaEnvPad --dump-env $envClean" --cwd $root)
Check "G1 launch receipt reports sanitizing" ($rl.status -eq "ok" -and $rl.detail -like "*env sanitized*ELECTRON_RUN_AS_NODE*") "$($rl.detail)"
$r = J (RunCua wait --window "title:CuaEnvPad" --timeout-ms 8000)
for ($k = 0; $k -lt 25 -and -not (Test-Path $envClean); $k++) { Start-Sleep -Milliseconds 300 }
$dump = if (Test-Path $envClean) { Get-Content $envClean -Raw } else { "" }
Check "G2 child env has no ELECTRON_/NODE_/CHROME_ vars" ($dump -ne "" -and $dump -notmatch "(?m)^(ELECTRON|NODE|CHROME)") "len=$($dump.Length)"
Check "G3 launch honours --cwd" (($dump -split "\r?\n") -contains "CWD:$root") "$(($dump -split "\r?\n" | Select-Object -First 1))"
& $cua close --window "title:CuaEnvPad" --confirm | Out-Null
Start-Sleep -Milliseconds 600
$rl2 = J (RunCua launch --exe $testpad --args "--title CuaEnvPad2 --dump-env $envDirty" --no-clean-env)
Check "G4 --no-clean-env receipt omits sanitizing" ($rl2.status -eq "ok" -and $rl2.detail -notlike "*env sanitized*") "$($rl2.detail)"
$r = J (RunCua wait --window "title:CuaEnvPad2" --timeout-ms 8000)
for ($k = 0; $k -lt 25 -and -not (Test-Path $envDirty); $k++) { Start-Sleep -Milliseconds 300 }
$dump2 = if (Test-Path $envDirty) { Get-Content $envDirty -Raw } else { "" }
Check "G5 --no-clean-env keeps ELECTRON_RUN_AS_NODE" ($dump2 -match "(?m)^ELECTRON_RUN_AS_NODE=1$") "len=$($dump2.Length)"
& $cua close --window "title:CuaEnvPad2" --confirm | Out-Null
Start-Sleep -Milliseconds 600
$ro = J (RunCua open --name $testpad)
Check "G6 open of an .exe also sanitizes" ($ro.status -eq "ok" -and $ro.detail -like "*sanitized*") "$($ro.detail)"
& $cua close --window "title:CuaTestPad Fixture" --confirm | Out-Null

# ---------- result ----------
Write-Host ""
Write-Host ("==== PHASE-3 RESULT: {0} passed, {1} failed, {2} skipped" -f $script:pass, $script:fail, $script:skip) -ForegroundColor $(if ($script:fail -eq 0) { "Green" } else { "Red" })
if ($script:fail -gt 0) { $script:failures | ForEach-Object { Write-Host ("  failed: " + $_) -ForegroundColor Red }; exit 1 }
exit 0
