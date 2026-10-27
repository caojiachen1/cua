# tests\e2e.ps1 — end-to-end suite for cua. Run: powershell -ExecutionPolicy Bypass -File tests\e2e.ps1
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
    $i = $s.IndexOf("{")
    if ($i -gt 0) { $s = $s.Substring($i) }
    $s | ConvertFrom-Json
}
function Check($name, $cond, $detail = "") {
    if ($cond) { $script:pass++; Write-Host ("  PASS  {0}" -f $name) -ForegroundColor Green }
    else { $script:fail++; $script:failures += $name; Write-Host ("  FAIL  {0}  {1}" -f $name, $detail) -ForegroundColor Red }
}
function RunCua { & $cua @args 2>$null }

Write-Host "== cua e2e (cli: $cua)" -ForegroundColor Cyan

# ---------- 0. clean slate ----------


& $cua sandbox stop 2>$null | Out-Null


taskkill /IM testpad.exe /F 2>$null | Out-Null


$rec = Join-Path $env:LOCALAPPDATA "cua\recordings"; if (Test-Path $rec) { Remove-Item (Join-Path $rec "*.jsonl") -Force -ErrorAction SilentlyContinue }
Start-Sleep -Milliseconds 800

# ---------- 1. check ----------
$r = J (RunCua check)
Check "check: uia ok" ($r.uia -eq "ok") "$($r.uia)"
Check "check: receipt schema" ($r.receipt_schema -eq "cua-action-receipt-v1")
Check "check: deny list populated" ($r.safety.deny_processes.Count -ge 10)

# ---------- 2. apps ----------
$r = J (RunCua apps)
Check "apps: count>0" ($r.count -gt 0) "count=$($r.count)"
Check "apps: has shell/desktop window" (($r.windows | Where-Object { $_.desktop_area }).Count -ge 1)

# ---------- 3. launch + state ----------
& $cua close --window "CuaTestPad" --confirm 2>$null | Out-Null
Start-Sleep -Milliseconds 500
$r = J (RunCua launch --exe $testpad)
Check "launch: ok receipt" ($r.status -eq "ok" -and $r.dispatched) "$($r.status)/$($r.detail)"
Start-Sleep -Seconds 2
$st = J (RunCua state --window "CuaTestPad")
Check "state: nodes>3" ($st.node_count -gt 3) "nodes=$($st.node_count)"
Check "state: screenshot rendered" ($st.screenshot.rendered -eq $true) "$($st.screenshot | ConvertTo-Json -Compress)"
$edit = $st.nodes | Where-Object { $_.type -eq "Edit" } | Select-Object -First 1
$btn = $st.nodes | Where-Object { $_.name -eq "AppendMark" } | Select-Object -First 1
Check "state: Edit element present" ($null -ne $edit)
Check "state: AppendMark button present" ($null -ne $btn)
$editIdx = $edit.i; $btnIdx = $btn.i
$stateId1 = $st.state_id

# ---------- 4. state --diff ----------
$st2 = J (RunCua state --window "CuaTestPad" --diff)
Check "state --diff: unchanged" ($st2.diff.changed -eq $false) "$($st2.diff | ConvertTo-Json -Compress)"

# ---------- 5. set-value / get-value roundtrip ----------
$r = J (RunCua set-value --window "CuaTestPad" --index $editIdx --value "e2e-value-123")
Check "set-value: ok via ValuePattern" ($r.status -eq "ok" -and $r.detail -like "*ValuePattern*") "$($r.detail)"
$v = J (RunCua get-value --window "CuaTestPad" --index $editIdx)
Check "get-value: roundtrip" ($v.value -eq "e2e-value-123") "$($v.value)"

# ---------- 6. real keyboard typing (SendInput foreground) ----------
& $cua activate --window "CuaTestPad" | Out-Null
# click into the edit area first so caret is inside
& $cua click --x ($edit.rect[0] + 100) --y ($edit.rect[1] + 60) --shot-id (J (RunCua shot --window "CuaTestPad" --out "$cache\es.png")).state_id | Out-Null
Start-Sleep -Milliseconds 300
& $cua key --keys "ctrl+a" --window "CuaTestPad" | Out-Null
$r = J (RunCua type --text "typed-real-input" --window "CuaTestPad")
Check "type: ok receipt" ($r.status -eq "ok") "$($r.status)/$($r.detail)"
Start-Sleep -Milliseconds 300
$st = J (RunCua state --window "CuaTestPad" --no-shot)
$editNow = $st.nodes | Where-Object { $_.type -eq "Edit" } | Select-Object -First 1
Check "type: text arrived via SendInput" ($editNow.value -like "*typed-real-input*") "value=$($editNow.value)"

# ---------- 7. coordinate click: guard + a11y ----------
$r = J (RunCua click --x 10 --y 10)
Check "click coords: refused without shot_id" ($r.status -eq "refused") "$($r.status)"
$shot = J (RunCua shot --window "CuaTestPad" --out "$cache\g.png")
$r = J (RunCua click --x ($edit.rect[0] + 80) --y ($edit.rect[1] + 40) --shot-id $shot.state_id)
Check "click coords: ok with fresh shot_id" ($r.status -eq "ok") "$($r.status)/$($r.detail)"
$r = J (RunCua click --window "CuaTestPad" --index $btnIdx)
Check "click a11y index: invoked" ($r.status -eq "ok") "$($r.status)/$($r.detail)"
$st = J (RunCua state --window "CuaTestPad" --no-shot)
$editNow = $st.nodes | Where-Object { $_.type -eq "Edit" } | Select-Object -First 1
Check "a11y click: button handler ran (MARK)" ($editNow.value -like "*MARK:*") "value=$($editNow.value)"

# ---------- 8. grounding ----------
$r = J (RunCua grounding --window "CuaTestPad" --query "AppendMark")
Check "grounding: found" ($r.found -and $r.matches.Count -ge 1) "$($r | ConvertTo-Json -Compress)"

# ---------- 9. clipboard ----------
& $cua clipboard set --text "cua-e2e-clipboard" | Out-Null
$v = J (RunCua clipboard get)
Check "clipboard roundtrip" ($v.text -eq "cua-e2e-clipboard") "$($v.text)"

# ---------- 10. safety policy ----------
$r = J (RunCua close --window "CuaTestPad")
Check "close: refused without --confirm" ($r.status -eq "refused" -and $r.detail -like "*Always-Confirm*") "$($r.detail)"
$r = J (RunCua launch --exe cmd.exe)
Check "launch cmd: refused (deny list)" ($r.status -eq "refused") "$($r.status)"
$out = (& $cua key --keys "win+r" 2>&1) -join "`n"
Check "win key: refused" ($out -like "*Windows-logo key*") "$out"

# ---------- 11. batch ----------
$batchFile = "$cache\batch.json"
@'
[["type", "--text", "B", "--window", "CuaTestPad"], ["key", "--keys", "shift", "--window", "CuaTestPad"]]
'@ | Set-Content -Encoding UTF8 $batchFile
$r = J (RunCua batch --file $batchFile)
Check "batch: all steps ok" ($r.ok -eq $true -and $r.steps.Count -eq 2) ($r.steps | ConvertTo-Json -Compress)

# ---------- 12. record + replay + verify ----------
& $cua state --window "CuaTestPad" --no-shot | Out-Null
$r = J (RunCua set-value --window "CuaTestPad" --index $editIdx --value "replay-target" --record e2e-rec)
Check "record: recorded" ($r.detail -like "*e2e-rec*") "$($r.detail)"
$r = J (RunCua replay e2e-rec --verify)
Check "replay: ok + verified" ($r.ok -and $r.verified -eq $true) "ok=$($r.ok) verified=$($r.verified)"

# ---------- 13. window ops ----------
$r = J (RunCua minimize --window "CuaTestPad")
Check "minimize: ok" ($r.status -eq "ok") "$($r.detail)"
& $cua activate --window "CuaTestPad" | Out-Null
$r = J (RunCua activate --window "CuaTestPad")
Check "activate: ok" ($r.status -eq "ok") "$($r.detail)"

# ---------- 14. scroll (receipt-level) ----------
$r = J (RunCua scroll --dy 2 --window "CuaTestPad")
Check "scroll: ok" ($r.status -eq "ok") "$($r.status)/$($r.detail)"

# ---------- 15. receipt schema on every action ----------
$rec = J (RunCua set-value --window "CuaTestPad" --index $editIdx --value "schema-check")
Check "receipt schema field" ($rec.schema -eq "cua-action-receipt-v1" -and $rec.tool -eq "cua" -and $null -ne $rec.ts -and $null -ne $rec.elapsed_ms)

# ---------- 16. sandbox (hidden desktop) ----------
& $cua sandbox stop 2>$null | Out-Null
$r = J (RunCua sandbox start --exe $testpad)
Check "sandbox start: ok" ($null -ne $r.pid) "$($r | ConvertTo-Json -Compress)"
Start-Sleep -Seconds 2
$r = J (RunCua sandbox apps)
Check "sandbox apps: fixture running" (($r.windows | Where-Object { $_.process -eq "testpad.exe" }).Count -ge 1) "count=$($r.count)"
$sb = J (RunCua sandbox state --window "CuaTestPad")
Check "sandbox state: UIA works on hidden desktop" ($sb.node_count -gt 3) "nodes=$($sb.node_count)"
Check "sandbox state: screenshot rendered" ($sb.screenshot.rendered -eq $true)
$sbEdit = $sb.nodes | Where-Object { $_.type -eq "Edit" } | Select-Object -First 1
$sbBtn = $sb.nodes | Where-Object { $_.name -eq "AppendMark" } | Select-Object -First 1
$r = J (RunCua sandbox set-value --window "CuaTestPad" --index $sbEdit.i --value "hidden-line")
Check "sandbox set-value: ok" ($r.status -eq "ok") "$($r.detail)"
$r = J (RunCua sandbox type --window "CuaTestPad" --text "MSGTYPED")
Check "sandbox type: msg-level ok" ($r.status -eq "ok" -and $r.detail -like "*message-level*") "$($r.detail)"
$r = J (RunCua sandbox click --window "CuaTestPad" --index $sbBtn.i)
Check "sandbox a11y click: ok" ($r.status -eq "ok") "$($r.detail)"
$shot = J (RunCua sandbox shot --window "CuaTestPad" --out "$cache\sb-e2e.png")
$r = J (RunCua sandbox click --x ($sbBtn.rect[0] + [int]($sbBtn.rect[2]/2)) --y ($sbBtn.rect[1] + [int]($sbBtn.rect[3]/2)) --shot-id $shot.state_id)
Check "sandbox msg click: ok" ($r.status -eq "ok") "$($r.detail)"
$v = J (RunCua sandbox get-value --window "CuaTestPad" --index $sbEdit.i)
Check "sandbox value: set+type+MARK all landed" ($v.value -like "*hidden-line*" -and $v.value -like "*MSGTYPED*" -and $v.value -like "*MARK:*") "$($v.value)"
$r = J (RunCua apps)
# close the user-desktop fixture so only the (invisible) sandbox copy could exist
$cl = J (RunCua close --window "CuaTestPad" --confirm)
Start-Sleep -Milliseconds 1200
$gone = $false
for ($k = 0; $k -lt 12; $k++) {
    Start-Sleep -Milliseconds 500
    $r = J (RunCua apps)
    if ((($r.windows | Where-Object { $_.process -eq "testpad.exe" }).Count) -eq 0) { $gone = $true; break }
}
Check "sandbox isolation: fixture invisible on user desktop" $gone ("leaked: " + (($r.windows | Where-Object { $_.process -eq "testpad.exe" } | ForEach-Object { "pid=$($_.pid)" }) -join ","))
& $cua sandbox stop | Out-Null

# ---------- 17. browser bridge (UBB) contract ----------
$help = (& $cua browser help 2>&1) -join "`n"
Check "browser help: documents the ubb bridge" ($help -like "*ubb*" -and $help -like "*session.start*") ($help.Substring(0, [Math]::Min(60, $help.Length)))
$ck = J (RunCua check)
Check "check: browser backend is ubb" ($ck.browser.backend -eq "ubb") ($ck.browser | ConvertTo-Json -Compress)
$err = RunErr browser shot --out "$cache
ope.png"
Check "old CDP subcommand is gone" ($err -like "*failed*" -or $err -like "*not found*") ($err.Substring(0, [Math]::Min(70, $err.Length)))

# ---------- result ----------
Write-Host ""
Write-Host ("==== RESULT: {0} passed, {1} failed" -f $script:pass, $script:fail) -ForegroundColor $(if ($script:fail -eq 0) { "Green" } else { "Red" })
if ($script:fail -gt 0) { $script:failures | ForEach-Object { Write-Host ("  failed: " + $_) -ForegroundColor Red }; exit 1 }
exit 0
