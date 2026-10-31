# tests\e2e8.ps1 - regression suite for the review fixes:
# data-dir isolation, truncated reporting, background index typing, message click
# sequence, strategy validation, sandbox background click/scroll/type/alt+f4,
# recording session isolation and truthful replay verification, redacted diff.
$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$cua = $env:CUA_TEST_BIN
if (-not $cua -or -not (Test-Path $cua)) { $cua = "$root\bin\cua.exe" }
if (-not (Test-Path $cua)) { $cua = "$root\target\debug\cua.exe" }
$testpad = $env:CUA_TEST_TESTPAD
if (-not $testpad -or -not (Test-Path $testpad)) { $testpad = "$root\bin\testpad.exe" }
if (-not (Test-Path $testpad)) { $testpad = "$root\target\debug\testpad.exe" }

$ownData = $false
if (-not $env:CUA_DATA_DIR) {
    $env:CUA_DATA_DIR = Join-Path $env:TEMP ("cua-e2e8-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
    $ownData = $true
}
New-Item -ItemType Directory -Force -Path $env:CUA_DATA_DIR | Out-Null
$cache = Join-Path $env:CUA_DATA_DIR "cache"
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
function Node($st, $type, $name = $null) {
    if ($null -eq $name) { return ($st.nodes | Where-Object { $_.type -eq $type } | Select-Object -First 1) }
    return ($st.nodes | Where-Object { $_.type -eq $type -and $_.name -eq $name } | Select-Object -First 1)
}
function Val($sel, $idx) { (J (RunCua get-value --window $sel --index $idx)).value }

Write-Host "== cua e2e phase-8 regressions (cli: $cua)" -ForegroundColor Cyan
Write-Host ("== data dir: {0}" -f $env:CUA_DATA_DIR) -ForegroundColor DarkGray

# ---------- 0. isolation + clean slate ----------
$chk = J (RunCua check)
Check "P0 CUA_DATA_DIR isolates config/cache" (
    $chk.data_dir -and [string]::Equals([string]$chk.data_dir, $env:CUA_DATA_DIR, [StringComparison]::OrdinalIgnoreCase)
) "$($chk.data_dir)"
& $cua sandbox stop 2>$null | Out-Null
& $cua rec stop 2>$null | Out-Null
taskkill /IM testpad.exe /F 2>$null | Out-Null
Start-Sleep -Milliseconds 800

$W = "title:CuaRegPad"
& $cua launch --exe $testpad --args "--title CuaRegPad" | Out-Null
& $cua wait --window $W --timeout-ms 5000 | Out-Null
$st = J (RunCua state --window $W --no-shot)
$edit = Node $st "Edit"
$btn = Node $st "Button" "AppendMark"
$hit = Node $st "Pane" "HitArea"
if (-not $edit -or -not $btn -or -not $hit) {
    Check "P1 fixture exposes Edit/Button/HitArea" $false "edit=$($null -ne $edit) btn=$($null -ne $btn) hit=$($null -ne $hit)"
} else {
    Check "P1 fixture exposes Edit/Button/HitArea" $true "nodes=$($st.node_count)"
}

# ---------- 1. tree truncation is reported ----------
$t0 = J (RunCua state --window $W --depth 0 --no-shot)
Check "P2 depth-0 walk is marked truncated" ($t0.truncated -eq $true -and $t0.node_count -eq 1) "truncated=$($t0.truncated) nodes=$($t0.node_count)"
# The depth-0 observation replaced the element cache; refresh indices before acting.
$st = J (RunCua state --window $W --no-shot)
$edit = Node $st "Edit"
$btn = Node $st "Button" "AppendMark"
$hit = Node $st "Pane" "HitArea"
$chk = Node $st "CheckBox" "AutoMark"
$li = Node $st "ListItem" "beta"
Check "P2b full observation restores the element cache" ($st.node_count -gt 10 -and $null -ne $edit -and $null -ne $btn -and $null -ne $hit) "nodes=$($st.node_count)"
$obsPath = [string]$st.observation
$obs = if (Test-Path $obsPath) { Get-Content $obsPath -Raw -Encoding UTF8 | ConvertFrom-Json } else { $null }
Check "P2d state writes an immutable observation" (
    (Test-Path $obsPath) -and $null -ne $obs -and $obs.id -eq $st.state_id -and @($obs.nodes).Count -eq $st.node_count
) "path=$obsPath nodes=$(if ($obs) { @($obs.nodes).Count } else { 'none' })"
$sh = J (RunCua shot --window $W --out "$cache\obs-guard.png")
& $cua set-value --window $W --index $edit.i --value "AFTER-SHOT" | Out-Null
$v = Val $W $edit.i
Check "P2e shot does not replace the element observation" ($sh.state_id -ne $st.state_id -and $v -eq "AFTER-SHOT") "shot=$($sh.state_id) v=$v"
# Explicit pin: keep using an older immutable snapshot even after a newer, shallow state.
$env:CUA_OBSERVATION_ID = $st.state_id
& $cua state --window $W --depth 0 --no-shot | Out-Null
$rp = J (RunCua set-value --window $W --index $edit.i --value "PINNED")
$vp = Val $W $edit.i
Remove-Item Env:CUA_OBSERVATION_ID -ErrorAction SilentlyContinue
$rl = J (RunCua set-value --window $W --index $edit.i --value "LATEST")
Check "P2f CUA_OBSERVATION_ID pins an immutable snapshot" ($rp.status -eq "ok" -and $vp -eq "PINNED") "$($rp.status) v=$vp"
Check "P2g without the pin the shallow latest cache is used" ($rl.status -eq "failed") "$($rl.status)/$($rl.detail)"
& $cua state --window $W --no-shot | Out-Null

# ---------- 1b. CUA_SESSION isolates observations ----------
$env:CUA_SESSION = "isoA"
& $cua state --window $W --depth 0 --no-shot | Out-Null
$env:CUA_SESSION = "isoB"
$fullB = J (RunCua state --window $W --no-shot)
$env:CUA_SESSION = "isoA"
$rA = J (RunCua set-value --window $W --index $edit.i --value "ISO-A")
$env:CUA_SESSION = "isoB"
$rB = J (RunCua set-value --window $W --index $edit.i --value "ISO-B")
$vB = (J (RunCua get-value --window $W --index $edit.i)).value
Remove-Item Env:CUA_SESSION -ErrorAction SilentlyContinue
Check "P2c sessions keep separate element caches" ($rA.status -eq "failed" -and $rB.status -eq "ok" -and $vB -eq "ISO-B") "A=$($rA.status) B=$($rB.status)/$vB"
# restore a default-session cache for the remaining index actions
& $cua state --window $W --no-shot | Out-Null

# ---------- 2. background typing honours --index ----------
& $cua set-value --window $W --index $edit.i --clear | Out-Null
$r = J (RunCua type --window $W --text "IDX-TYPE" --background --index $edit.i)
$v = Val $W $edit.i
Check "P3 type --background --index reaches the field" ($r.status -eq "ok" -and $v -eq "IDX-TYPE") "$($r.status)/$($r.detail) v=$v"
Check "P3c receipts expose verification/evidence/input_mode" ($r.verification -eq "not_requested" -and $r.evidence -eq "complete" -and $r.input_mode -eq "background") "verification=$($r.verification) evidence=$($r.evidence) mode=$($r.input_mode)"
$r = J (RunCua click --window $W --index $hit.i --background --strategy a11y)
Check "P3d background a11y stays on a11y patterns" (
    $r.status -eq "ok" -and $r.input_mode -eq "background-a11y" -and $r.detail -like "*Legacy*" -and $r.detail -notlike "*message-level*"
) "$($r.status)/$($r.input_mode)/$($r.detail)"
$sA11y = J (RunCua shot --window $W --out "$cache\a11y-coord.png")
$r = J (RunCua click --window $W --x 20 --y 20 --strategy a11y --shot-id $sA11y.state_id)
Check "P3e a11y strategy refuses raw coordinates" (
    $r.status -eq "failed" -and $r.input_mode -eq "a11y" -and $r.detail -like "*coordinates*"
) "$($r.status)/$($r.input_mode)/$($r.detail)"
& $cua set-value --window $W --index $edit.i --clear | Out-Null
$r = J (RunCua type --window $W --text "SHOULD-NOT" --background --index 9999)
$v = Val $W $edit.i
Check "P3b bad --index fails without typing into another control" ($r.status -eq "failed" -and [string]::IsNullOrEmpty($v)) "$($r.status) v=$v"

# ---------- 3. message-level double click uses DOWN/UP/DBLCLK/UP ----------
& $cua set-value --window $W --index $edit.i --clear | Out-Null
$r = J (RunCua click --window $W --index $hit.i --background --double)
Start-Sleep -Milliseconds 300
$v = Val $W $edit.i
Check "P4 background double click emits DBLCLK" ($r.status -eq "ok" -and $v -match "MDOWN" -and $v -match "DBLCLK") "$($r.detail) v=$($v -replace "`n", '/')"

# ---------- 3b. element-level wait ----------
& $cua set-value --window $W --index $edit.i --value "WAIT-OK" | Out-Null
$w1 = J (RunCua wait --window $W --index $edit.i --value "WAIT-OK" --timeout-ms 1200)
Check "P4b wait sees an element value" ($w1.satisfied -eq $true -and $w1.last.value -eq "WAIT-OK") "$($w1 | ConvertTo-Json -Compress)"
$sw = [Diagnostics.Stopwatch]::StartNew()
$w2 = J (RunCua wait --window $W --index $edit.i --value "NOPE" --timeout-ms 300)
$sw.Stop()
Check "P4c wait times out honestly" ($w2.satisfied -eq $false -and $sw.ElapsedMilliseconds -ge 250 -and $sw.ElapsedMilliseconds -lt 3000) "satisfied=$($w2.satisfied) ms=$($sw.ElapsedMilliseconds)"
& $cua toggle --window $W --index $chk.i | Out-Null
$w3 = J (RunCua wait --window $W --index $chk.i --toggle on --timeout-ms 1200)
Check "P4d wait sees a toggle state" ($w3.satisfied -eq $true -and $w3.last.toggle -eq "on") "$($w3 | ConvertTo-Json -Compress)"
$si = J (RunCua scroll-item --window $W --index $li.i --align-top)
Check "P4e ScrollItemPattern scrolls an item into view" ($si.status -eq "ok" -and $si.detail -like "*ScrollItemPattern*") "$($si.status)/$($si.detail)"
# multi-select: the second listbox is LBS_MULTIPLESEL
$mst = J (RunCua state --window $W --no-shot)
$red = Node $mst "ListItem" "red"
$green = Node $mst "ListItem" "green"
$r1 = J (RunCua select-add --window $W --index $red.i)
$r2 = J (RunCua select-add --window $W --index $green.i)
$mst2 = J (RunCua state --window $W --no-shot)
$redOn = (Node $mst2 "ListItem" "red").selected
$greenOn = (Node $mst2 "ListItem" "green").selected
Check "P4f SelectionItem add_to_selection builds a multi-selection" ($r1.status -eq "ok" -and $r2.status -eq "ok" -and $redOn -eq $true -and $greenOn -eq $true) "$($redOn)/$($greenOn)"
$r3 = J (RunCua select-remove --window $W --index $red.i)
$mst3 = J (RunCua state --window $W --no-shot)
$redOff = (Node $mst3 "ListItem" "red").selected
$greenStill = (Node $mst3 "ListItem" "green").selected
Check "P4g select-remove clears only the requested item" ($r3.status -eq "ok" -and $redOff -eq $false -and $greenStill -eq $true) "$($redOff)/$($greenStill)"
# refresh the cache after the state calls above
$st = J (RunCua state --window $W --no-shot)
$edit = Node $st "Edit"
$btn = Node $st "Button" "AppendMark"
$hit = Node $st "Pane" "HitArea"

# ---------- 4. unknown strategy is refused as an input error ----------
$err = RunErr click --window $W --index $btn.i --strategy magic
Check "P5 unknown click strategy rejects the command" ($LASTEXITCODE -eq 1 -and $err -like "*unknown strategy*") "exit=$LASTEXITCODE err=$err"

# ---------- 5. sandbox background paths ----------
$SBW = "title:CuaRegSandbox"
$sbStart = J (RunCua sandbox start --exe $testpad --args "--title CuaRegSandbox")
Check "P6 sandbox start" ($null -ne $sbStart.pid) "$($sbStart | ConvertTo-Json -Compress)"
if ($null -ne $sbStart.pid) {
    $sbState = J (RunCua sandbox state --window $SBW --no-shot)
    $sbEdit = Node $sbState "Edit"
    $sbBtn = Node $sbState "Button" "AppendMark"
    $sbHit = Node $sbState "Pane" "HitArea"
    if (-not $sbEdit -or -not $sbBtn -or -not $sbHit) {
        Check "P6b sandbox fixture controls" $false "state nodes=$($sbState.node_count)"
    } else {
        Check "P6b sandbox fixture controls" $true "nodes=$($sbState.node_count)"

        $r = J (RunCua sandbox click --window $SBW --index $sbBtn.i)
        Start-Sleep -Milliseconds 300
        $v = (J (RunCua sandbox get-value --window $SBW --index $sbEdit.i)).value
        Check "P7 sandbox index click (no real input) invokes the button" ($r.status -eq "ok" -and $v -like "*MARK*") "$($r.detail) v=$v"
        Check "P7b sandbox receipts expose the same contract" ($r.verification -eq "not_requested" -and $r.evidence -eq "complete") "verification=$($r.verification) evidence=$($r.evidence)"

        $r = J (RunCua sandbox click --window $SBW --index $sbHit.i --right)
        Start-Sleep -Milliseconds 300
        $v = (J (RunCua sandbox get-value --window $SBW --index $sbEdit.i)).value
        Check "P8 sandbox right-click falls back to messages" ($r.status -eq "ok" -and $v -like "*RCLICK*") "$($r.detail) v=$v"

        $sbShot = J (RunCua sandbox shot --window $SBW --out "$cache\e2e8-sb.png")
        $apps = J (RunCua sandbox apps)
        $win = $apps.windows | Where-Object { $_.title -eq "CuaRegSandbox" } | Select-Object -First 1
        $cx = [int]($win.rect[0] + $win.rect[2] / 2)
        $cy = [int]($win.rect[1] + $win.rect[3] / 2)
        $r = J (RunCua sandbox scroll --x $cx --y $cy --dy -3)
        Check "P9 sandbox coordinate scroll needs a shot id" ($r.status -eq "refused") "$($r.status)/$($r.detail)"
        $r = J (RunCua sandbox scroll --x $cx --y $cy --dy -3 --shot-id $sbShot.state_id)
        Check "P9b sandbox coordinate scroll with shot id works" ($r.status -eq "ok") "$($r.status)/$($r.detail)"

        $r = J (RunCua sandbox key --window $SBW --keys "alt + f4")
        Check "P10 sandbox alt+f4 spacing still needs --confirm" ($r.status -eq "refused") "$($r.status)/$($r.detail)"

        & $cua sandbox set-value --window $SBW --index $sbEdit.i --clear | Out-Null
        $r = J (RunCua sandbox type --window $SBW --text "SB-IDX" --index $sbEdit.i)
        $v = (J (RunCua sandbox get-value --window $SBW --index $sbEdit.i)).value
        Check "P11 sandbox type honours --index" ($r.status -eq "ok" -and $v -like "*SB-IDX*") "$($r.detail) v=$v"

        $sbBeta = Node $sbState "ListItem" "beta"
        $r = J (RunCua sandbox scroll-item --window $SBW --index $sbBeta.i)
        Check "P11b sandbox ScrollItem works" ($r.status -eq "ok" -and $r.detail -like "*ScrollItemPattern*") "$($r.detail)"
        $sbOcr = J (RunCua sandbox ocr --window $SBW --max-lines 20)
        Check "P11e sandbox OCR reads the hidden window" (
            $sbOcr.text -match "OCR TEST 123" -and $sbOcr.line_count -ge 1
        ) "text=$($sbOcr.text -replace "`n", ' / ')"

        # sandbox actions used to drop --record entirely
        $trace = "sb-regtrace"
        Remove-Item (Join-Path $env:CUA_DATA_DIR "recordings\$trace.jsonl") -Force -ErrorAction SilentlyContinue
        $r = J (RunCua click --sandbox --window $SBW --index $sbBtn.i --record $trace)
        $tracePath = Join-Path $env:CUA_DATA_DIR "recordings\$trace.jsonl"
        $lines = @(if (Test-Path $tracePath) { Get-Content $tracePath })
        Check "P11c sandbox action is recorded" ($r.status -eq "ok" -and $lines.Count -eq 1 -and $lines[0] -like "*--sandbox*") "status=$($r.status) lines=$($lines.Count) match=$($lines.Count -eq 1 -and $lines[0] -like '*--sandbox*')"
        $rp = J (RunCua replay $trace)
        Check "P11d sandbox recording replays" ($rp.ok -eq $true -and @($rp.steps).Count -eq 1) "ok=$($rp.ok) steps=$(@($rp.steps).Count)"

        # ---------- P22: sandbox Table/ItemContainer positive fixture (Explorer) ----------
        # These two commands previously had no positive sandbox coverage; Explorer's
        # item view carries Table + ItemContainer providers and runs on the hidden desktop.
        $tblDir = Join-Path $env:TEMP ("cua-e2e8-tbl-" + [guid]::NewGuid().ToString("N").Substring(0, 6))
        New-Item -ItemType Directory -Force -Path $tblDir | Out-Null
        1..3 | ForEach-Object { Set-Content -Encoding ASCII -Path (Join-Path $tblDir ("alpha$_.txt")) -Value "probe $_" }
        $sbExLaunch = J (RunCua sandbox launch --exe explorer.exe --args $tblDir)
        Start-Sleep -Seconds 4
        $sbExSt = $null
        for ($k = 0; $k -lt 10; $k++) {
            $sbExSt = J (RunCua sandbox state --window process:explorer --no-shot --depth 16 --patterns)
            if (@($sbExSt.nodes | Where-Object { 'Table' -in $_.patterns }).Count -ge 1) { break }
            Start-Sleep -Milliseconds 800
        }
        $sbTblNode = $sbExSt.nodes | Where-Object { 'Table' -in $_.patterns } | Select-Object -First 1
        if ($null -eq $sbTblNode) {
            Skip "P22 sandbox table/find-item" "explorer on hidden desktop exposes no Table provider"
        } else {
            $sbT = J (RunCua table --sandbox --window process:explorer --index $sbTblNode.i --max-headers 4)
            Check "P22a sandbox Table reads real column headers" (
                @($sbT.table.column_headers).Count -ge 3 -and $sbT.table.column_headers[0].name -ne ""
            ) "headers=$(@($sbT.table.column_headers | ForEach-Object { $_.name }) -join ',')"
            $sbF = J (RunCua find-item --sandbox --window process:explorer --index $sbTblNode.i --property name --value alpha2.txt)
            Check "P22b sandbox ItemContainer finds an item by name" ($sbF.element.name -eq "alpha2.txt") "name=$($sbF.element.name)"
        }
    }
    $stop = J (RunCua sandbox stop)
    Check "P12 sandbox stop only kills owned processes" ($stop.killed_processes -ge 2) "$($stop | ConvertTo-Json -Compress)"
} else {
    Skip "P6b-P12 sandbox checks" "sandbox did not start"
}

# ---------- 6. recordings get distinct session directories ----------
$r1 = J (RunCua rec start --window $W --fps 2 --out regrec --max-seconds 30)
Check "P13 recording starts" ($r1.status -eq "ok") "$($r1.detail)"
$rst = J (RunCua rec status)
Check "P13b status reports a live capture worker" ($rst.recording -eq $true -and $rst.worker_alive -eq $true) "$($rst | ConvertTo-Json -Compress)"
Start-Sleep -Milliseconds 1500
$s1 = J (RunCua rec stop)
Check "P14 first recording stops with frames" ($s1.status -eq "ok" -and $s1.target.frames -ge 1) "$($s1.detail)"
$r2 = J (RunCua rec start --window $W --fps 2 --out regrec --max-seconds 30)
Start-Sleep -Milliseconds 900
$s2 = J (RunCua rec stop)
Check "P14b same display name uses a fresh frame directory" (
    $s2.status -eq "ok" -and $s1.target.dir -ne $s2.target.dir -and (Test-Path $s2.target.dir)
) "$($s1.target.dir) vs $($s2.target.dir)"
$again = J (RunCua rec stop)
Check "P14c second stop reports no active recording" ($again.status -eq "failed") "$($again.status)/$($again.detail)"
$rst2 = J (RunCua rec status)
Check "P14d status reports no worker after stop" ($rst2.recording -eq $false -and $rst2.worker_alive -eq $false) "$($rst2 | ConvertTo-Json -Compress)"
$rc = J (RunCua rec start --window $W --fps 1 --out regcancel --max-seconds 30)
Start-Sleep -Milliseconds 1200
$cn = J (RunCua rec cancel)
$rst3 = J (RunCua rec status)
Check "P14e rec cancel stops a live worker and clears state" (
    $rc.status -eq "ok" -and $cn.target.worker_was_alive -eq $true -and $cn.target.killed -eq $true -and $rst3.recording -eq $false
) "cancel=$($cn.target.worker_was_alive)/$($cn.target.killed) status=$($rst3.recording)/$($cn.detail)"
$stopAfterCancel = J (RunCua rec stop)
Check "P14f stop after cancel reports no recording" ($stopAfterCancel.status -eq "failed") "$($stopAfterCancel.status)/$($stopAfterCancel.detail)"

# ---------- 7. replay does not claim verification without evidence ----------
$launchRec = J (RunCua launch --exe $testpad --args "--title CuaRegPad2" --record regtrace-launch)
Check "P15 launch recorded without a tree fingerprint" ($launchRec.status -eq "ok") "$($launchRec.detail)"
$rp = J (RunCua replay regtrace-launch --verify)
Check "P16 replay is not 'verified' when no step has evidence" (
    $rp.verified -eq $false -and $rp.unverified_steps -ge 1 -and $rp.coverage -like "0/*"
) "verified=$($rp.verified) verified_steps=$($rp.verified_steps) unverified=$($rp.unverified_steps)"

$recSet = J (RunCua set-value --window $W --index $edit.i --value "VERIFY-OK" --record regtrace-set)
Check "P16b set-value recorded with a fingerprint" ($recSet.status -eq "ok") "$($recSet.status)/$($recSet.detail)"
$rp = J (RunCua replay regtrace-set --verify)
Check "P17 replay verifies a step that has a fingerprint" (
    $rp.verified -eq $true -and $rp.verified_steps -ge 1 -and $rp.unverified_steps -eq 0
) "verified=$($rp.verified) steps=$($rp.verified_steps)/$($rp.unverified_steps)"

# ---------- 8. redacted diff cannot echo unredacted cached names ----------
& $cua state --window $W --no-shot | Out-Null
$raw = (RunCua state --window $W --no-shot --redact --diff) -join "`n"
Check "P18 redacted diff hides previously cached names" (
    $raw -notmatch "AppendMark" -and $raw -notmatch "AutoMark"
) "leaked an unredacted element name"
# Redaction is for the output; the action cache must keep real names so a
# no-runtime-id element can still be re-located after `state --redact`.
$redState = J (RunCua state --window $W --redact --no-shot)
$redNodes = ($redState.nodes | ConvertTo-Json -Compress)
$cachePath = Join-Path $env:CUA_DATA_DIR ("cache\elements-" + $redState.window.hwnd + ".json")
$cacheRaw = if (Test-Path $cachePath) { Get-Content $cachePath -Raw } else { "" }
Check "P18f redacted output does not poison the action cache" (
    $redNodes -notmatch "AppendMark" -and $cacheRaw -match "AppendMark"
) "cache=$($cacheRaw.Length) chars"
$ocr = J (RunCua ocr --window $W --max-lines 50)
Check "P18g local OCR reads the fixture text" (
    $ocr.text -match "OCR TEST 123" -and $ocr.line_count -ge 1 -and @($ocr.lines).Count -ge 1
) "text=$($ocr.text -replace "`n", ' / ')"
$ocrShot = J (RunCua shot --window $W --out "$cache\ocr-src.png")
$ocrFile = J (RunCua ocr --file "$cache\ocr-src.png" --max-lines 50)
# The static OCR label can be missed by an individual PrintWindow frame; the
# purpose here is the --file path, so assert a stable control label instead.
Check "P18h local OCR works on a saved image" ($ocrFile.text -match "AppendMark") "source=$($ocrFile.source.file) text=$($ocrFile.text -replace "`n", ' / ')"

# ---------- 8b. grounding publishes the tree its indices belong to ----------
$shallow = J (RunCua state --window $W --depth 1 --no-shot)
$g = J (RunCua grounding --window $W --query "alpha")
$gm = @($g.matches)[0]
Check "P18b grounding caches a deeper tree than the shallow state" (
    $null -ne $gm -and $g.node_count -gt $shallow.node_count -and $gm.i -ge $shallow.node_count
) "nodes=$($shallow.node_count)->$($g.node_count) index=$($gm.i)"
$r = J (RunCua click --window $W --index $gm.i --background)
Start-Sleep -Milliseconds 300
$after = J (RunCua state --window $W --no-shot)
$alpha = Node $after "ListItem" "alpha"
Check "P18c grounding index drives the next action" ($r.status -eq "ok" -and $alpha.selected -eq $true) "$($r.status)/$($r.detail) selected=$($alpha.selected)"

# ---------- 8c. batch data binding (field lookup only, no evaluation) ----------
$cur = J (RunCua state --window $W --no-shot)
$curEdit = Node $cur "Edit"
$batchPath = Join-Path $env:CUA_DATA_DIR "batch-bind.json"
$ph = '${st.nodes[' + $curEdit.i + '].i}'
$batchObj = @{ steps = @(
    @{ argv = @("state", "--window", $W, "--no-shot"); as = "st" },
    @{ argv = @("set-value", "--window", $W, "--index", $ph, "--value", "BATCH-OK") }
) }
$batchObj | ConvertTo-Json -Depth 6 | Set-Content -Path $batchPath -Encoding UTF8
$br = J (RunCua batch --file $batchPath)
$bx = J (RunCua get-value --window $W --index $curEdit.i)
Check "P18d batch binds a prior JSON result" ($br.ok -eq $true -and @($br.steps).Count -eq 2 -and $br.steps[1].captured -eq $null -and $bx.value -eq "BATCH-OK") "ok=$($br.ok) v=$($bx.value)"
$badBatchPath = Join-Path $env:CUA_DATA_DIR "batch-bad.json"
$badObj = @{ steps = @(
    @{ argv = @("state", "--window", $W, "--no-shot"); as = "st" },
    @{ argv = @("set-value", "--window", $W, "--index", '${st.nodes[999].i}', "--value", "X") }
) }
$badObj | ConvertTo-Json -Depth 6 | Set-Content -Path $badBatchPath -Encoding UTF8
$bad = J (RunCua batch --file $badBatchPath)
Check "P18e unknown batch path fails instead of substituting" ($bad.ok -eq $false -and $bad.steps[1].exit -eq 1 -and $bad.steps[1].error -like "*out of range*") "ok=$($bad.ok) err=$($bad.steps[1].error)"

# ---------- 9. physical input must land on the declared window ----------
$WA = "title:CuaHitA"
$WB = "title:CuaHitB"
& $cua launch --exe $testpad --args "--title CuaHitA" | Out-Null
& $cua launch --exe $testpad --args "--title CuaHitB" | Out-Null
& $cua wait --window $WA --timeout-ms 5000 | Out-Null
& $cua wait --window $WB --timeout-ms 5000 | Out-Null
$ra = J (RunCua resize --window $WA --x 120 --y 120 --width 660 --height 520)
$rb = J (RunCua resize --window $WB --x 120 --y 120 --width 660 --height 520)
$appsHit = J (RunCua apps)
$waInfo = $appsHit.windows | Where-Object { $_.title -eq "CuaHitA" } | Select-Object -First 1
$wbInfo = $appsHit.windows | Where-Object { $_.title -eq "CuaHitB" } | Select-Object -First 1
$fg = $null
for ($k = 0; $k -lt 4; $k++) {
    & $cua activate --window $WB | Out-Null
    Start-Sleep -Milliseconds 200
    $fg = (J (RunCua apps)).foreground_hwnd
    if ($null -ne $wbInfo -and $fg -eq $wbInfo.hwnd) { break }
}
Start-Sleep -Milliseconds 200
$stA = J (RunCua state --window $WA --no-shot)
$hitA = Node $stA "Pane" "HitArea"
$editA = Node $stA "Edit"
$hx = [int]($hitA.rect[0] + $hitA.rect[2] / 2)
$hy = [int]($hitA.rect[1] + $hitA.rect[3] / 2)
$covered = $false
if ($null -ne $wbInfo) {
    $br = $wbInfo.rect
    $covered = $hx -ge $br[0] -and $hy -ge $br[1] -and $hx -lt $br[0] + $br[2] -and $hy -lt $br[1] + $br[3]
}
$setup = $ra.status -eq "ok" -and $rb.status -eq "ok" -and $null -ne $wbInfo -and $fg -eq $wbInfo.hwnd -and $covered
Check "P19 setup: B is foreground and covers A's hit area" $setup "resize=$($ra.status)/$($rb.status) fg=$fg b=$($wbInfo.hwnd) point=$hx,$hy bRect=$($wbInfo.rect -join ',')"
& $cua set-value --window $WA --index $editA.i --clear | Out-Null
if ($setup) {
    $r = J (RunCua click --window $WA --index $hitA.i --strategy event)
    Check "P19 covered physical click is refused" ($r.status -eq "failed" -and $r.detail -like "*not the target*") "$($r.status)/$($r.detail)"
} else {
    Check "P19 covered physical click is refused" $false "setup failed"
}
$shA = J (RunCua shot --window $WA --out "$cache\hitA.png")
$hx = [int]($hitA.rect[0] + $hitA.rect[2] / 2)
$hy = [int]($hitA.rect[1] + $hitA.rect[3] / 2)
$r = J (RunCua click --window $WA --x $hx --y $hy --background --shot-id $shA.state_id)
Start-Sleep -Milliseconds 300
$vA = Val $WA $editA.i
Check "P20 background click bypasses occlusion safely" ($r.status -eq "ok" -and $vA -like "*MDOWN*") "$($r.detail) v=$vA"

# ---------- P21: per-action --observation-id binding ----------
# B2 leftover: index actions must accept an explicit immutable observation id,
# and a wrong id must fail loudly instead of silently re-observing.
$stP21 = J (RunCua state --window $WA --no-shot)
$obsId = $stP21.state_id
$editP21 = Node $stP21 "Edit"
if ($null -eq $editP21 -or $null -eq $obsId) {
    Skip "P21 observation binding" "no state/testpad window"
} else {
    $gv = J (RunCua get-value --window $WA --index $editP21.i --observation-id $obsId)
    Check "P21a explicit --observation-id resolves the pinned snapshot" ($null -ne $gv.value) "value=$($gv.value)"
    $errOut = RunErr get-value --window $WA --index $editP21.i --observation-id "cua-e2e8-missing-obs"
    Check "P21b wrong --observation-id fails loudly (no silent re-observe)" ($errOut -like "*not found or not of this window*") "$errOut"
    $gv2 = J (RunCua get-value --window $WA --index $editP21.i)
    Check "P21c default (no flag) still resolves the latest observation" ($null -ne $gv2.value) "value=$($gv2.value)"
}

# ---------- cleanup + result ----------
& $cua sandbox stop 2>$null | Out-Null
& $cua rec stop 2>$null | Out-Null
taskkill /IM testpad.exe /F 2>$null | Out-Null
Write-Host ""
Write-Host ("==== PHASE-8 RESULT: {0} passed, {1} failed, {2} skipped" -f $script:pass, $script:fail, $script:skip) -ForegroundColor $(if ($script:fail -eq 0) { "Green" } else { "Red" })
if ($script:fail -gt 0) {
    $script:failures | ForEach-Object { Write-Host ("  failed: " + $_) -ForegroundColor Red }
    exit 1
}
if ($ownData -and $env:CUA_E2E_CLEAN -eq "1") { Remove-Item -Recurse -Force $env:CUA_DATA_DIR -ErrorAction SilentlyContinue }
exit 0
