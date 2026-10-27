# tests\e2e2.ps1 — phase-2 deep e2e: selectors, unicode/drag/keys, capture, error paths,
# config override, sandbox deep, ubb bridge contract, window ops, exit codes.
$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$cua = "$root\bin\cua.exe"
if (-not (Test-Path $cua)) { $cua = "$root\target\debug\cua.exe" }
$testpad = "$root\bin\testpad.exe"
if (-not (Test-Path $testpad)) { $testpad = "$root\target\debug\testpad.exe" }
$cache = Join-Path $env:LOCALAPPDATA "cua\cache"
$fixturesUri = ([System.Uri](Join-Path $root "tests\fixtures")).AbsoluteUri.TrimEnd("/")
New-Item -ItemType Directory -Force -Path $cache | Out-Null
$script:pass = 0; $script:fail = 0; $script:failures = @()

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
function RunCua { & $cua @args 2>$null }
function RunErr { (& $cua @args 2>&1) -join "`n" }
function JsonArg($h) { ($h | ConvertTo-Json -Compress) -replace '"', '\"' }
function CloseBrowser { & $cua browser session.stop 2>$null | Out-Null }

Write-Host "== cua e2e phase-2 (cli: $cua)" -ForegroundColor Cyan

# ---------- 0. clean slate ----------
& $cua sandbox stop 2>$null | Out-Null
taskkill /IM testpad.exe /F 2>$null | Out-Null
Get-ChildItem (Join-Path $env:LOCALAPPDATA "cua\recordings") -Filter *.jsonl -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 800

# ---------- fixture ----------
& $cua launch --exe $testpad | Out-Null
Start-Sleep -Seconds 2
$st = J (RunCua state --window "CuaTestPad")
$edit = $st.nodes | Where-Object { $_.type -eq "Edit" } | Select-Object -First 1
$editIdx = $edit.i
$hwnd = $st.window.hwnd
$pidSel = "pid:$($st.window.pid)"

# ---------- A. selectors & observation ----------
$r = J (RunCua state --window "class:CuaTestPad" --no-shot)
Check "A1 selector class:" ($r.window.hwnd -eq $hwnd) "$($r.window.hwnd) vs $hwnd"
$r = J (RunCua state --window "$hwnd" --no-shot)
Check "A2 selector hwnd decimal" ($r.window.hwnd -eq $hwnd)
$hex = "0x{0:x}" -f $hwnd
$r = J (RunCua state --window $hex --no-shot)
Check "A3 selector hwnd hex" ($r.window.hwnd -eq $hwnd)
$r = J (RunCua state --window $pidSel --no-shot)
Check "A4 selector pid:" ($r.window.hwnd -eq $hwnd)
$r = J (RunCua state --window "CuaTestPad" --no-shot --depth 2)
$r2 = J (RunCua state --window "CuaTestPad" --no-shot --depth 12)
Check "A5 depth limit reduces nodes" ($r.node_count -lt $r2.node_count) "$($r.node_count) vs $($r2.node_count)"
$outPath = "$cache\state-out.png"
$r = J (RunCua state --window "CuaTestPad" --out $outPath)
Check "A6 state --out custom path" ((Test-Path $outPath) -and $r.screenshot.rendered)
& $cua set-value --window "CuaTestPad" --index $editIdx --value "secret-text" | Out-Null
$r = J (RunCua state --window "CuaTestPad" --no-shot --redact)
$ed = $r.nodes | Where-Object { $_.type -eq "Edit" } | Select-Object -First 1
Check "A7 redact masks value" ($ed.value -eq "s…") "$($ed.value)"
$rAll = J (RunCua apps --all)
$rVis = J (RunCua apps)
Check "A8 apps --all >= apps" ($rAll.count -ge $rVis.count) "$($rAll.count) vs $($rVis.count)"

# ---------- B. real input deep ----------
# B1 unicode + CJK + emoji via SendInput
& $cua activate --window "CuaTestPad" | Out-Null
$shot = J (RunCua shot --window "CuaTestPad" --out "$cache\p2b.png")
& $cua click --x ($edit.rect[0] + 80) --y ($edit.rect[1] + 50) --shot-id $shot.state_id | Out-Null
Start-Sleep -Milliseconds 200
& $cua key --keys "ctrl+a" --window "CuaTestPad" | Out-Null
& $cua type --text "中文abc🎉" --window "CuaTestPad" | Out-Null
Start-Sleep -Milliseconds 300
$st = J (RunCua state --window "CuaTestPad" --no-shot)
$ed = $st.nodes | Where-Object { $_.type -eq "Edit" } | Select-Object -First 1
Check "B1 CJK+emoji typed" ($ed.value -like "*中文abc*") "value=$($ed.value)"
# surrogate pair: 🎉 is 2 UTF-16 units — check the Edit actually got both (shows as the char)
Check "B1b emoji arrived" ($ed.value -like "*abc*🎉*" -or $ed.value.Length -gt 8) "len=$($ed.value.Length)"
# B2 newline
& $cua type --text "line2`nline3" --window "CuaTestPad" | Out-Null
Start-Sleep -Milliseconds 300
$st = J (RunCua state --window "CuaTestPad" --no-shot)
$ed = $st.nodes | Where-Object { $_.type -eq "Edit" } | Select-Object -First 1
Check "B2 newline creates 2nd line" ($ed.value -like "*line2*line3*") "value=$($ed.value)"
# B3 key-combo effect: ctrl+z undoes the last typing (classic Edit handles WM_UNDO)
$st = J (RunCua state --window "CuaTestPad" --no-shot)
$edBefore = ($st.nodes | Where-Object { $_.type -eq "Edit" } | Select-Object -First 1).value
& $cua key --keys "ctrl+z" --window "CuaTestPad" | Out-Null
Start-Sleep -Milliseconds 250
$st = J (RunCua state --window "CuaTestPad" --no-shot)
$edAfter = ($st.nodes | Where-Object { $_.type -eq "Edit" } | Select-Object -First 1).value
Check "B3 ctrl+z undid typing" ($edAfter -ne $edBefore) "before=$edBefore after=$edAfter"
& $cua set-value --window "CuaTestPad" --index $editIdx --value "DRAGTARGET" | Out-Null
# B4 real drag selects text -> clipboard
& $cua activate --window "CuaTestPad" | Out-Null
Start-Sleep -Milliseconds 200
$shot = J (RunCua shot --window "CuaTestPad" --out "$cache\p2drag.png")
$dy = $edit.rect[1] + 20
& $cua click --x ($edit.rect[0] + 80) --y ($dy) --shot-id $shot.state_id | Out-Null
Start-Sleep -Milliseconds 200
& $cua key --keys "home" --window "CuaTestPad" | Out-Null
Start-Sleep -Milliseconds 150
$r = J (RunCua drag --x1 ($edit.rect[0] + 14) --y1 $dy --x2 ($edit.rect[0] + 78) --y2 $dy --shot-id (J (RunCua shot --window "CuaTestPad" --out "$cache\p2drag2.png")).state_id)
Check "B4 drag receipt ok" ($r.status -eq "ok") "$($r.status)/$($r.detail)"
& $cua key --keys "ctrl+c" --window "CuaTestPad" | Out-Null
Start-Sleep -Milliseconds 300
$cl = J (RunCua clipboard get)
Check "B4b drag selected contiguous text" ($cl.text.Length -ge 3 -and "DRAGTARGET".Contains($cl.text)) "clip=$($cl.text)"

# ---------- C. capture deep ----------
$r = J (RunCua shot --screen --out "$cache\screen.png")
$sz = (Get-Item "$cache\screen.png").Length
Check "C1 screen shot: file>10KB + state_id" ($sz -gt 10000 -and $null -ne $r.state_id) "bytes=$sz"
# occluded capture: second instance overlaps first
& $cua launch --exe $testpad | Out-Null
Start-Sleep -Seconds 2
& $cua activate --window "CuaTestPad" | Out-Null   # newest instance (2 windows share title; resolver prefers foreground)
Start-Sleep -Milliseconds 300
$r = J (RunCua shot --window "$hwnd" --out "$cache\occluded.png")
$sz = (Get-Item "$cache\occluded.png").Length
Check "C2 occluded window captured" ($r.rendered -and $sz -gt 3000) "rendered=$($r.rendered) bytes=$sz"
$err = RunErr shot --window "999999999" --out "$cache\nothing.png"
Check "C3 shot nonexistent window errors" ($err -like "*does not exist*") "$err"

# ---------- D. error paths & exit codes ----------
$r = J (RunCua click --window "$hwnd" --index 999)
Check "D1 click bad index: failed" ($r.status -eq "failed") "$($r.status)/$($r.detail)"
& $cua get-value --window "$hwnd" --index ($editIdx + 1) 2>$null | Out-Null
$code2 = $LASTEXITCODE
$err2 = RunErr get-value --window "$hwnd" --index ($editIdx + 1)
Check "D2 get-value on non-value element: exit1 + clear error" ($code2 -eq 1 -and $err2 -like "*ValuePattern*") "code=$code2 err=$($err2.Substring(0, [Math]::Min(70, $err2.Length)))"
$err = RunErr state --window "NoSuchWindowXYZ123"
Check "D3 state unknown window errors" ($err -like "*no window matching*") ($err.Substring(0, [Math]::Min(80, $err.Length)))
$r = J (RunCua grounding --window "CuaTestPad" --query "ZZZ-NO-MATCH")
Check "D4 grounding no match: found=false" ($r.found -eq $false)
$err = RunErr replay "no-such-recording-xyz"
Check "D5 replay missing recording errors" ($err -like "*not found*") ($err.Substring(0, [Math]::Min(80, $err.Length)))
$badBatch = "$cache\bad.json"
'{ not json' | Set-Content -Encoding UTF8 $badBatch
$err = RunErr batch --file $badBatch
Check "D6 batch malformed errors" ($err -like "*error*") ($err.Substring(0, [Math]::Min(60, $err.Length)))
$boe = "$cache\boe.json"
@'
[["state", "--window", "NoSuchWindowXYZ"], ["clipboard", "get"]]
'@ | Set-Content -Encoding UTF8 $boe
$r = J (RunCua batch --file $boe --continue-on-error)
Check "D7 batch continue-on-error runs step2" ($r.steps.Count -eq 2 -and $r.ok -eq $false) "$($r | ConvertTo-Json -Compress)"
& $cua clipboard set --text "x" | Out-Null
& $cua close --window "CuaTestPad" 2>$null | Out-Null   # refused
Check "D8 exit code: refused=2" ($LASTEXITCODE -eq 2) "$LASTEXITCODE"
& $cua clipboard get | Out-Null
Check "D8b exit code: ok=0" ($LASTEXITCODE -eq 0) "$LASTEXITCODE"
& $cua state --window "NoSuchWindowXYZ123" 2>$null | Out-Null
Check "D8c exit code: error=1" ($LASTEXITCODE -eq 1) "$LASTEXITCODE"

# cleanup overlapping fixtures before config/sandbox/browser sections
taskkill /IM testpad.exe /F 2>$null | Out-Null
Start-Sleep -Milliseconds 600

# ---------- E. config override ----------
$cfg = Join-Path $env:LOCALAPPDATA "cua\config.json"
[IO.File]::WriteAllText($cfg, '{"require_shot_id_for_coords": false, "deny_processes": [], "deny_key_modifiers": ["win"], "levels": {}}', (New-Object Text.UTF8Encoding $false))
Start-Sleep -Milliseconds 200
$r = J (RunCua click --x 50 --y 50)
Check "E1 config override disables shot guard" ($r.status -eq "ok") "$($r.status)/$($r.detail)"
Remove-Item $cfg -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 200
$r = J (RunCua click --x 50 --y 50)
Check "E2 default guard restored" ($r.status -eq "refused") "$($r.status)"

# ---------- F. sandbox deep ----------
& $cua sandbox stop 2>$null | Out-Null
$r = J (RunCua sandbox stop)
Check "F1 sandbox stop idempotent" ($r.killed_processes -eq 0) "$($r.killed_processes)"
$r = J (RunCua sandbox start --exe $testpad)
Check "F2 sandbox restart cycle" ($null -ne $r.pid) "$($r | ConvertTo-Json -Compress)"
Start-Sleep -Seconds 2
$r = J (RunCua sandbox state --window "CuaTestPad" --no-shot)
$sbEdit = $r.nodes | Where-Object { $_.type -eq "Edit" } | Select-Object -First 1
Check "F2b sandbox state --no-shot" ($null -eq $r.screenshot -and $r.node_count -gt 3)
$r = J (RunCua sandbox set-value --window "CuaTestPad" --index $sbEdit.i --value "sb-deep")
$r = J (RunCua sandbox scroll --dy 2 --window "CuaTestPad")
Check "F3 sandbox scroll ok" ($r.status -eq "ok") "$($r.detail)"
$sbw = (J (RunCua sandbox apps)).windows | Where-Object { $_.title -like "*CuaTestPad*" -and $_.visible } | Select-Object -First 1
$dx1 = $sbw.rect[0] + 60; $dy1 = $sbw.rect[1] + 120; $dx2 = $dx1 + 200; $dy2 = $dy1 + 60
$r = J (RunCua sandbox drag --x1 $dx1 --y1 $dy1 --x2 $dx2 --y2 $dy2)
Check "F4 sandbox drag ok" ($r.status -eq "ok") "$($r.detail)"
$r = J (RunCua sandbox key --keys "alt+f4")
Check "F5 sandbox alt+f4 refused w/o confirm" ($r.status -eq "refused") "$($r.status)"
$r = J (RunCua sandbox key --keys "alt+f4" --confirm)
Check "F5b sandbox alt+f4 with confirm ok" ($r.status -eq "ok") "$($r.status)"
$r = J (RunCua sandbox click --x 200 --y 200)
Check "F6 sandbox shot guard active" ($r.status -eq "refused") "$($r.status)"
$r = J (RunCua sandbox key --keys "s")
Check "F9 sandbox msg-level key ok" ($r.status -eq "ok" -and $r.detail -like "*message-level*") "$($r.detail)"
# F8 user foreground unaffected during sandbox actions
$fgBefore = ((J (RunCua apps)).windows | Where-Object { $_.foreground } | Select-Object -First 1).pid
& $cua sandbox set-value --window "CuaTestPad" --index $sbEdit.i --value "fg-check" | Out-Null
& $cua sandbox click --window "CuaTestPad" --index ($sbEdit.i + 1) 2>$null | Out-Null
$fgAfter = ((J (RunCua apps)).windows | Where-Object { $_.foreground } | Select-Object -First 1).pid
Check "F8 user foreground unchanged by sandbox" ($fgBefore -eq $fgAfter) "$fgBefore vs $fgAfter"
$r = J (RunCua sandbox stop)
Check "F10 sandbox stop kills app+keeper" ($r.killed_processes -ge 2) "$($r.killed_processes)"

# ---------- G. ubb bridge: cli resolution + argument passthrough ----------
# A stub CLI (node .mjs) proves the contract without touching the user's browser.
$stub = Join-Path $cache "ubb-stub.mjs"
Set-Content -Path $stub -Value 'console.log(JSON.stringify({ got: process.argv.slice(2) }))' -Encoding Ascii
$prevUbb = $env:CUA_UBB
$env:CUA_UBB = $stub
$r = J (RunCua browser status)
Check "G1 ubb cli resolved from `$env:CUA_UBB" ($r.status -eq "ok" -and $r.backend -eq "ubb" -and $r.cli -like "*ubb-stub.mjs") "$($r.cli)"
Check "G2 tool name forwarded" ($r.result.got[0] -eq "status") ($r.result.got -join "|")
$params = (@{ url = "http://x/y" } | ConvertTo-Json -Compress)
$wire = JsonArg @{ url = "http://x/y" }
$r = J (RunCua browser tab.create $wire)
Check "G3 json arg forwarded verbatim" ($r.result.got[0] -eq "tab.create" -and $r.result.got[1] -eq $params) ($r.result.got -join "|")
$stubFail = Join-Path $cache "ubb-stub-fail.mjs"
Set-Content -Path $stubFail -Value 'console.log(JSON.stringify({ code: "TOOL_FAILED", message: "no such tool" })); process.exit(1)' -Encoding Ascii
$env:CUA_UBB = $stubFail
$err = RunErr browser no.such.tool "{}"
Check "G4 cli failure maps to receipt failed" ($LASTEXITCODE -eq 1 -and $err -like "*failed*" -and $err -like "*no such tool*") "exit=$LASTEXITCODE"
$env:CUA_UBB = Join-Path $cache "missing-ubb.mjs"
$prevPath = $env:PATH
$env:PATH = "$env:SystemRoot\System32"
$err = RunErr browser status
$env:PATH = $prevPath
Check "G5 missing ubb cli reports not found" ($LASTEXITCODE -eq 1 -and $err -like "*not found*") ($err.Substring(0, [Math]::Min(70, $err.Length)))
if ($prevUbb) { $env:CUA_UBB = $prevUbb } else { Remove-Item Env:CUA_UBB -ErrorAction SilentlyContinue }

# ---------- H. window ops deep ----------
& $cua launch --exe $testpad | Out-Null
Start-Sleep -Seconds 2
$r = J (RunCua maximize --window "CuaTestPad")
Check "H1 maximize receipt ok" ($r.status -eq "ok") "$($r.detail)"
Start-Sleep -Milliseconds 400
$r = J (RunCua apps)
$w = $r.windows | Where-Object { $_.class -eq "CuaTestPad" -and $_.visible } | Select-Object -First 1
Check "H2 maximize flag visible in apps" ($w.maximized -eq $true) "$($w.maximized)"
$r = J (RunCua open --name cmd)
Check "H3 open denied binary refused" ($r.status -eq "refused" -and $r.detail -like "*deny list*") "$($r.detail)"
$r = J (RunCua close --window "CuaTestPad" --confirm)
Check "H4 close graceful" ($r.status -eq "ok" -and $r.detail -like "*gracefully*") "$($r.detail)"
Start-Sleep -Milliseconds 600
$r = J (RunCua apps)
Check "H5 fixture closed (no testpad)" ((($r.windows | Where-Object { $_.process -eq "testpad.exe" }).Count) -eq 0) "still open!"

# ---------- result ----------
Write-Host ""
Write-Host ("==== PHASE-2 RESULT: {0} passed, {1} failed" -f $script:pass, $script:fail) -ForegroundColor $(if ($script:fail -eq 0) { "Green" } else { "Red" })
if ($script:fail -gt 0) { $script:failures | ForEach-Object { Write-Host ("  failed: " + $_) -ForegroundColor Red }; exit 1 }
exit 0
