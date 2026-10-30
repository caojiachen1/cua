# tests\e2e4.ps1 - phase-4: real Windows applications (no fixtures).
# Covers UIA control parsing per control type/pattern, control-level actions with verified
# side effects, background (message-level) control while another window keeps focus,
# hidden-desktop sandbox with real apps + process cleanup, deny list on real windows,
# and occluded-window capture.
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
New-Item -ItemType Directory -Force -Path $cache | Out-Null
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
function RunErr { (& $cua @args 2>&1) -join "`n" }
# locale-tolerant element lookup: any of the supplied name fragments (zh or en)
function Nodes($st, $type, $names = @()) {
    $list = @($st.nodes | Where-Object { $_.type -eq $type })
    if ($names.Count -gt 0) {
        $list = @($list | Where-Object { $nm = $_.name; @($names | Where-Object { $nm -like "*$_*" }).Count -gt 0 })
    }
    return $list
}
function First($arr) { $a = @($arr); if ($a.Count -gt 0) { return $a[0] } return $null }
function Pids($name) { @(Get-Process -Name $name -ErrorAction SilentlyContinue | ForEach-Object { $_.Id }) }
function Start-App($exe, $procSel, $waitMs = 9000) {
    & $cua launch --exe $exe 2>$null | Out-Null
    & $cua wait --window $procSel --timeout-ms $waitMs 2>$null | Out-Null
    Start-Sleep -Milliseconds 600
}
function ShotOf($sel, $file) { (J (RunCua shot --window $sel --out $file)).state_id }

Write-Host "== cua e2e phase-4 (real apps, cli: $cua)" -ForegroundColor Cyan

# ---------- 0. clean slate + pre-existing process snapshot ----------
& $cua sandbox stop 2>$null | Out-Null
taskkill /IM testpad.exe /F 2>$null | Out-Null
Start-Sleep -Milliseconds 500
$preNotepad = Pids "Notepad"
$preCharmap = Pids "charmap"
$preMsinfo = Pids "msinfo32"
$preSndvol = Pids "SndVol"

# CJK fragments built from code points so the script file stays ASCII
$F_SELECT = @(([string][char]0x9009 + [char]0x62E9), "Select")              # select
$F_ADDTAB = @(([string][char]0x6DFB + [char]0x52A0), "Add")                 # add (new tab)
$F_BOLD = @(([string][char]0x52A0 + [char]0x7C97), "Bold")                  # bold
$F_FILE = @(([string][char]0x6587 + [char]0x4EF6), "File")
$F_ADDR = [string][char]0x5730 + [char]0x5740                  # file (menu)

# ---------- launch real apps ----------
Start-App "charmap.exe" "process:charmap"
Start-App "msinfo32.exe" "process:msinfo32" 15000
Start-App "notepad.exe" "process:notepad"
Start-App "sndvol.exe" "process:sndvol"
$dir = Join-Path $env:TEMP ("cua-e2e4-" + $PID)
New-Item -ItemType Directory -Force -Path $dir | Out-Null
1..3 | ForEach-Object { "explorer probe line $_" | Set-Content -Encoding ASCII -Path (Join-Path $dir ("alpha$_.txt")) }
$leaf = Split-Path $dir -Leaf
$expl = $null
for ($try = 0; $try -lt 2 -and $null -eq $expl; $try++) {
    & $cua open --name $dir | Out-Null
    for ($k = 0; $k -lt 16; $k++) {
        Start-Sleep -Milliseconds 500
        $cand = (J (RunCua apps)).windows | Where-Object {
            $_.process -eq "explorer.exe" -and $_.class -eq "CabinetWClass" -and $_.title -like "*$leaf*"
        } | Sort-Object { [int64]$_.rect[2] * [int64]$_.rect[3] } -Descending | Select-Object -First 1
        if ($null -ne $cand) { $expl = $cand; break }
    }
}
$CM = "process:charmap"; $MS = "process:msinfo32"; $NP = "process:notepad"; $SV = "process:sndvol"
Check "0 real apps launched (charmap/msinfo32/notepad/sndvol/explorer)" (($null -ne (First (Nodes (J (RunCua state --window $CM --no-shot)) "Window"))) -and ($null -ne $expl)) "explorer window: $($expl.hwnd)"

# ---------- A. UIA control parsing on real apps ----------
$stCm = J (RunCua state --window $CM --no-shot --patterns)
Check "A1 charmap control types parsed" (@(Nodes $stCm "Edit").Count -ge 1 -and @(Nodes $stCm "CheckBox").Count -ge 1 -and @(Nodes $stCm "ComboBox").Count -ge 1 -and @(Nodes $stCm "Button").Count -ge 2) "nodes=$($stCm.node_count)"
$cmCombo = First (Nodes $stCm "ComboBox")
$cmChk = First (Nodes $stCm "CheckBox")
Check "A2 charmap patterns: ComboBox(Value+ExpandCollapse) CheckBox(Toggle)" ($cmCombo.patterns -contains "Value" -and $cmCombo.patterns -contains "ExpandCollapse" -and $cmChk.patterns -contains "Toggle") "$($cmCombo.patterns) / $($cmChk.patterns)"
Check "A2b charmap combo reports collapsed state" ($cmCombo.expanded -eq "collapsed") "$($cmCombo.expanded)"
$stMs = J (RunCua state --window $MS --no-shot --patterns)
$msTrees = @(Nodes $stMs "TreeItem")
Check "A3 msinfo32 tree parsed with ExpandCollapse+SelectionItem" ($msTrees.Count -ge 3 -and (($msTrees[0].patterns) -contains "ExpandCollapse") -and (($msTrees[0].patterns) -contains "SelectionItem")) "treeitems=$($msTrees.Count)"
Check "A3b msinfo32 details list parsed" (@(Nodes $stMs "ListItem").Count -ge 1) "listitems=$((@(Nodes $stMs "ListItem").Count))"
$msShotFile = "$cache\e4-msinfo.png"
$msShot = J (RunCua shot --window $MS --out $msShotFile)
Check "A4 msinfo32 screenshot rendered (real dialog content)" ($msShot.rendered -and (Get-Item $msShotFile).Length -gt 20000) "bytes=$((Get-Item $msShotFile).Length)"
$stNp = J (RunCua state --window $NP --no-shot --patterns)
$npDoc = First (Nodes $stNp "Document")
Check "A5 notepad Document exposes Value pattern" ($null -ne $npDoc -and $npDoc.patterns -contains "Value") "$($npDoc.patterns)"
Check "A5b notepad tabs + menus parsed" (@(Nodes $stNp "TabItem").Count -ge 1 -and @(Nodes $stNp "MenuItem").Count -ge 3) "tabs=$((@(Nodes $stNp "TabItem").Count)) menus=$((@(Nodes $stNp "MenuItem").Count))"
$stSv = J (RunCua state --window $SV --no-shot --patterns)
$svSliders = @(Nodes $stSv "Slider" | Where-Object { $null -ne $_.range })
Check "A6 sndvol sliders expose RangeValue 0..100" ($svSliders.Count -ge 1 -and $svSliders[0].range[1] -eq 0 -and $svSliders[0].range[2] -eq 100) "sliders=$($svSliders.Count) range=$($svSliders[0].range)"
$EX = "$($expl.hwnd)"
$stEx = J (RunCua state --window $EX --no-shot --patterns)
Check "A7 explorer items parsed as ListItems" (@(Nodes $stEx "ListItem" | Where-Object { $_.name -like "alpha*.txt" }).Count -ge 3) "listitems=$((@(Nodes $stEx "ListItem").Count))"
$exAddr = First (Nodes $stEx "Edit" @("address", ""))
Check "A7b explorer address bar + view radios parsed" (@(Nodes $stEx "RadioButton").Count -ge 1 -and @(Nodes $stEx "Edit").Count -ge 1 -and (@(Nodes $stEx "ScrollBar" | Where-Object { $null -ne $_.range }).Count -ge 1)) "edits=$((@(Nodes $stEx "Edit").Count)) radios=$((@(Nodes $stEx "RadioButton").Count))"

# Grid / Transform / VirtualizedItem (the patterns that unlock table cells and mouse-free moves)
$exGrid = $stEx.nodes | Where-Object { $_.patterns -contains "Grid" } | Select-Object -First 1
Check "A8 explorer advertises a UIA Grid provider" ($null -ne $exGrid) "nodes=$($stEx.node_count)"
if ($exGrid) {
    $g = J (RunCua grid --window $expl.hwnd --index $exGrid.i --max-cells 12)
    Check "A8b grid dumps a 2D matrix" ($g.grid.rows -gt 0 -and $g.grid.cols -ge 3 -and @($g.grid.cells[0]).Count -eq [int]$g.grid.cols) "rows=$($g.grid.rows) cols=$($g.grid.cols)"
    $c = J (RunCua grid --window $expl.hwnd --index $exGrid.i --row 1 --col 0)
    Check "A8c one cell resolves by (row,col)" ($null -ne $c.grid.rect -and $c.grid.rect[2] -gt 0) "rect=$($c.grid.rect -join ',')"
}
$exTable = $stEx.nodes | Where-Object { $_.patterns -contains "Table" } | Select-Object -First 1
if ($exTable) {
    $tb = J (RunCua table --window $EX --index $exTable.i --max-headers 5)
    $cols = @($tb.table.column_headers)
    Check "A8d table reads column headers" ($cols.Count -ge 3 -and $tb.table.row_or_column_major) "cols=$($cols.Count) major=$($tb.table.row_or_column_major)"
} else {
    Skip "A8d table reads column headers" "no Table provider"
}
$exIC = $stEx.nodes | Where-Object { $_.patterns -contains "ItemContainer" } | Select-Object -First 1
if ($exIC) {
    $firstItem = @(Nodes $stEx "ListItem")[0]
    $fi = J (RunCua find-item --window $EX --index $exIC.i --property name --value $firstItem.name)
    Check "A8e ItemContainer finds an item by name" ($fi.found -eq $true -and $fi.element.name -eq $firstItem.name) "found=$($fi.found)"
} else {
    Skip "A8e ItemContainer finds an item by name" "no ItemContainer provider"
}
$cmWin = $stCm.nodes | Where-Object { $_.patterns -contains "Transform" } | Select-Object -First 1
Check "A9 charmap window exposes TransformPattern" ($null -ne $cmWin) "patterns on window node"
if ($cmWin) {
    $rep = J (RunCua transform --window $CM --index $cmWin.i)
    Check "A9b transform reports capabilities" ($rep.detail -like "*can_move*:true*") "$($rep.detail.Substring(0, [Math]::Min(80, $rep.detail.Length)))"
    $cap0 = J (RunCua cursor)
    $mv = J (RunCua transform --window $CM --index $cmWin.i --move-to "520,300")
    # Read the pointer immediately: the sleep below is only for the window rect,
    # and a user moving the mouse during it must not fail this assertion.
    $cap1 = J (RunCua cursor)
    Start-Sleep -Milliseconds 700
    $rectNow = (J (RunCua apps)).windows | Where-Object { $_.hwnd -eq $stCm.window.hwnd } | ForEach-Object { $_.rect }
    Check "A9c transform moves a window with no cursor and no SetWindowPos" ($mv.status -eq "ok" -and $rectNow[0] -ge 500 -and $cap0.x -eq $cap1.x -and $cap0.y -eq $cap1.y) "x=$($rectNow[0]) cursor $($cap0.x),$($cap0.y)->$($cap1.x),$($cap1.y)"
}
$rz = RunErr realize --window $CM --index $cmChk.i
Check "A10 realize fails cleanly when the pattern is absent" ($rz -like "*VirtualizedItemPattern*") ($rz.Substring(0, [Math]::Min(70, $rz.Length)))

# ---------- B. control-level actions with verified effects ----------
$stCm = J (RunCua state --window $CM --no-shot)
$cmEdit = First (Nodes $stCm "Edit")
$cmSel = First (Nodes $stCm "Button" $F_SELECT)
$before = (J (RunCua get-value --window $CM --index $cmEdit.i)).value
$r = J (RunCua click --window $CM --index $cmSel.i)
Start-Sleep -Milliseconds 500
$after = (J (RunCua get-value --window $CM --index $cmEdit.i)).value
Check "B1 charmap Invoke button writes into the edit" ($r.status -eq "ok" -and $after -ne $before) "before='$before' after='$after'"
$cmChk = First (Nodes (J (RunCua state --window $CM --no-shot)) "CheckBox")
$st0 = J (RunCua state --window $CM --no-shot)
$chk0 = First (Nodes $st0 "CheckBox")
$r = J (RunCua toggle --window $CM --index $chk0.i)
Start-Sleep -Milliseconds 800
$st1 = J (RunCua state --window $CM --no-shot --diff)
$chk1 = First (Nodes $st1 "CheckBox")
Check "B2 charmap Toggle changes state and adds controls" ($r.status -eq "ok" -and $chk1.toggle -ne $chk0.toggle -and $st1.diff.added -ge 1) "$($chk0.toggle)->$($chk1.toggle) added=$($st1.diff.added)"
& $cua toggle --window $CM --index $chk1.i | Out-Null
Start-Sleep -Milliseconds 500
$chk2 = First (Nodes (J (RunCua state --window $CM --no-shot)) "CheckBox")
Check "B2b charmap Toggle back" ($chk2.toggle -eq $chk0.toggle) "$($chk2.toggle) vs $($chk0.toggle)"
$stCm = J (RunCua state --window $CM --no-shot)
$combo = First (Nodes $stCm "ComboBox")
$comboVal0 = $combo.value
$liBefore = @(Nodes $stCm "ListItem").Count
$r = J (RunCua expand --window $CM --index $combo.i)
Start-Sleep -Milliseconds 900
$stOpen = J (RunCua state --window $CM --no-shot --diff)
$liOpen = @(Nodes $stOpen "ListItem").Count
Check "B3 charmap ComboBox expand reveals items" ($r.status -eq "ok" -and $liOpen -ge ($liBefore + 5)) "listitems $liBefore -> $liOpen added=$($stOpen.diff.added)"
$openItems = @(Nodes $stOpen "ListItem")
$pickItem = $openItems | Where-Object { $_.name -ne $comboVal0 } | Select-Object -First 1
if ($null -eq $pickItem) { $pickItem = $openItems[0] }
$r = J (RunCua select --window $CM --index $pickItem.i)
Start-Sleep -Milliseconds 700
$stAfter = J (RunCua state --window $CM --no-shot)
$comboAfter = First (Nodes $stAfter "ComboBox")
Check "B3b charmap dropdown select changes combo value" ($r.status -eq "ok" -and $comboAfter.value -ne $comboVal0) "'$comboVal0' -> '$($comboAfter.value)' picked=$($pickItem.name)"
$rc3 = J (RunCua collapse --window $CM --index $comboAfter.i)
$combo2 = $null
for ($k = 0; $k -lt 5; $k++) {
    Start-Sleep -Milliseconds 400
    $combo2 = First (Nodes (J (RunCua state --window $CM --no-shot)) "ComboBox")
    if ($null -ne $combo2 -and $combo2.expanded -eq "collapsed") { break }
}
Check "B3c charmap ComboBox collapse" ($rc3.status -eq "ok" -and $null -ne $combo2 -and $combo2.expanded -eq "collapsed") "receipt=$($rc3.status) state=$($combo2.expanded)"
$stMs = J (RunCua state --window $MS --no-shot)
$collapsedTree = @(Nodes $stMs "TreeItem" | Where-Object { $_.expanded -eq "collapsed" })
$treeBefore = @(Nodes $stMs "TreeItem").Count
if ($collapsedTree.Count -gt 0) {
    $t = $collapsedTree[0]
    $r = J (RunCua expand --window $MS --index $t.i)
    Start-Sleep -Milliseconds 1200
    $stMs2 = J (RunCua state --window $MS --no-shot --diff)
    $treeAfter = @(Nodes $stMs2 "TreeItem").Count
    Check "B4 msinfo32 tree expand reveals child nodes" ($r.status -eq "ok" -and $treeAfter -gt $treeBefore) "treeitems $treeBefore -> $treeAfter"
    $kids = @(Nodes $stMs2 "TreeItem" | Where-Object { $_.i -gt $t.i })
    if ($kids.Count -gt 1) {
        $r = J (RunCua select --window $MS --index $kids[1].i)
        Start-Sleep -Milliseconds 1500
        $stMs3 = J (RunCua state --window $MS --no-shot)
        $selNow = @(Nodes $stMs3 "TreeItem" | Where-Object { $_.selected -eq $true })
        Check "B4b msinfo32 SelectionItem select moves selection" ($r.status -eq "ok" -and $selNow.Count -ge 1 -and @($selNow | Where-Object { $_.i -eq $kids[1].i }).Count -eq 1) "selected=$($selNow.Count)"
    } else {
        Skip "B4b msinfo32 SelectionItem select" "no children"
    }
} else {
    Check "B4 msinfo32 tree expand reveals child nodes" $false "no collapsed TreeItem found"
    Check "B4b msinfo32 SelectionItem select moves selection" $false "skipped"
}
$stNp = J (RunCua state --window $NP --no-shot)
$tabsBefore = @(Nodes $stNp "TabItem").Count
$addBtn = First (Nodes $stNp "Button" $F_ADDTAB)
& $cua state --window $NP --no-shot | Out-Null
$r = J (RunCua click --window $NP --index $addBtn.i)
Start-Sleep -Milliseconds 1500
$stNp2 = J (RunCua state --window $NP --no-shot --diff)
$tabsAfter = @(Nodes $stNp2 "TabItem").Count
Check "B5 notepad Invoke adds a tab (multiset diff counts duplicates)" ($r.status -eq "ok" -and $tabsAfter -eq ($tabsBefore + 1) -and $stNp2.diff.added -ge 1) "tabs $tabsBefore -> $tabsAfter added=$($stNp2.diff.added)"
$npDoc2 = First (Nodes $stNp2 "Document")
$lines = (1..200 | ForEach-Object { "cua line $_" }) -join "`n"
$r = J (RunCua set-value --window $NP --index $npDoc2.i --value $lines)
Start-Sleep -Milliseconds 700
$v = (J (RunCua get-value --window $NP --index $npDoc2.i)).value
Check "B6 notepad Document set-value 200 lines via ValuePattern" ($r.status -eq "ok" -and $v.Length -gt 1500) "len=$($v.Length)"
$tx = J (RunCua text --window $NP --index $npDoc2.i --max-chars 80)
Check "B6b TextPattern reads and bounds the document" ($tx.text -like "*cua line 1*" -and $tx.chars -le 80 -and $tx.truncated -eq $true) "chars=$($tx.chars) truncated=$($tx.truncated)"
$npShot = ShotOf $NP "$cache\e4-np.png"
& $cua activate --window $NP | Out-Null
$stS = J (RunCua state --window $NP --no-shot)
$sb = First (@(Nodes $stS "ScrollBar" | Where-Object { $null -ne $_.range -and $_.range[2] -gt 100 }))
& $cua scroll --dy -20 --window $NP | Out-Null
Start-Sleep -Milliseconds 700
$posDown = (J (RunCua get-range --window $NP --index $sb.i)).range.value
& $cua scroll --dy 40 --window $NP | Out-Null
Start-Sleep -Milliseconds 700
$posUp = (J (RunCua get-range --window $NP --index $sb.i)).range.value
Check "B6b real wheel scroll moves the app scrollbar (RangeValue readback)" ($posDown -gt $posUp) "down=$posDown up=$posUp"
$stNp3 = J (RunCua state --window $NP --no-shot)
$bold = First (Nodes $stNp3 "Button" $F_BOLD)
if ($null -ne $bold) {
    $t0 = $bold.toggle
    $r = J (RunCua toggle --window $NP --index $bold.i)
    $t1 = (First (Nodes (J (RunCua state --window $NP --no-shot)) "Button" $F_BOLD)).toggle
    & $cua toggle --window $NP --index $bold.i | Out-Null
    $t2 = (First (Nodes (J (RunCua state --window $NP --no-shot)) "Button" $F_BOLD)).toggle
    Check "B7 notepad XAML Toggle flips and flips back" ($r.status -eq "ok" -and $t1 -ne $t0 -and $t2 -eq $t0) "$t0->$t1->$t2"
} else {
    Check "B7 notepad XAML Toggle flips and flips back" $false "no Bold button found"
}
$stNp4 = J (RunCua state --window $NP --no-shot)
$fileMenu = First (Nodes $stNp4 "MenuItem" $F_FILE)
if ($null -ne $fileMenu) {
    & $cua state --window $NP --no-shot | Out-Null
    $r = J (RunCua expand --window $NP --index $fileMenu.i)
    Start-Sleep -Milliseconds 1000
    $stM = J (RunCua state --window $NP --no-shot --diff)
    Check "B8 notepad menu ExpandCollapse reveals flyout items" ($r.status -eq "ok" -and $stM.diff.added -ge 5 -and @(Nodes $stM "MenuItem").Count -gt @(Nodes $stNp4 "MenuItem").Count) "added=$($stM.diff.added)"
    $fm2 = First (Nodes $stM "MenuItem" $F_FILE)
    & $cua collapse --window $NP --index $fm2.i | Out-Null
    Start-Sleep -Milliseconds 600
    $back = (First (Nodes (J (RunCua state --window $NP --no-shot)) "MenuItem" $F_FILE)).expanded
    Check "B8b notepad menu collapse back" ($back -eq "collapsed") "$back"
} else {
    Check "B8 notepad menu ExpandCollapse reveals flyout items" $false "no File menu found"
    Check "B8b notepad menu collapse back" $false "skipped"
}
$stSv = J (RunCua state --window $SV --no-shot)
$sv0 = First (@(Nodes $stSv "Slider" | Where-Object { $null -ne $_.range }))
$r = J (RunCua set-range --window $SV --index $sv0.i --value 55)
Start-Sleep -Milliseconds 400
$gv = (J (RunCua get-range --window $SV --index $sv0.i)).range.value
Check "B9 sndvol set-range 55 then read back" ($r.status -eq "ok" -and [Math]::Abs($gv - 55) -le 2) "value=$gv"
$r = J (RunCua set-range --window $SV --index $sv0.i --value 30000)
Check "B9b sndvol out-of-range value refused" ($r.status -eq "failed" -and $r.detail -like "*outside range*") "$($r.detail)"
$stEx = J (RunCua state --window $EX --no-shot)
$items = @(Nodes $stEx "ListItem" | Where-Object { $_.name -like "alpha*.txt" })
$r = J (RunCua select --window $EX --index $items[1].i)
Start-Sleep -Milliseconds 700
$stEx2 = J (RunCua state --window $EX --no-shot)
$it2 = @(Nodes $stEx2 "ListItem" | Where-Object { $_.name -like "alpha*.txt" })
$selRows = @($it2 | Where-Object { $_.selected })
Check "B10 explorer ListItem SelectionItem selects exactly one" ($r.status -eq "ok" -and $selRows.Count -eq 1 -and $selRows[0].name -eq $items[1].name) "selected=$($selRows.Count): $(($selRows | ForEach-Object { $_.name }) -join ',')"
$radios = @(Nodes $stEx2 "RadioButton")
if ($radios.Count -ge 2) {
    $orig = $radios | Where-Object { $_.selected } | Select-Object -First 1
    $other = $radios | Where-Object { -not $_.selected } | Select-Object -First 1
    $r = J (RunCua select --window $EX --index $other.i)
    Start-Sleep -Milliseconds 1200
    $stEx3 = J (RunCua state --window $EX --no-shot)
    $now = @($stEx3.nodes | Where-Object { $_.type -eq "RadioButton" -and $_.selected })
    Check "B11 explorer view-mode radio select changes the app view" ($r.status -eq "ok" -and $now.Count -ge 1 -and $now[0].name -eq $other.name) "now=$($now[0].name)"
    if ($null -ne $orig) { & $cua select --window $EX --index ((@($stEx3.nodes | Where-Object { $_.type -eq "RadioButton" -and $_.name -eq $orig.name }) | Select-Object -First 1).i) | Out-Null }
} else {
    Check "B11 explorer view-mode radio select changes the app view" $false "radio count=$($radios.Count)"
}
$stEx4 = J (RunCua state --window $EX --no-shot --patterns)
$addrPool = @($stEx4.nodes | Where-Object { $_.type -eq "Edit" -and @($_.patterns | Where-Object { $_ -eq "Value" }).Count -gt 0 })
$addr = First (@($addrPool | Where-Object { $_.name -like "*$F_ADDR*" }))
if ($null -eq $addr) { $addr = First $addrPool }
$r = J (RunCua set-value --window $EX --index $addr.i --value "$dir")
Start-Sleep -Milliseconds 400
$av = (J (RunCua get-value --window $EX --index $addr.i)).value
Check "B12 explorer address bar ValuePattern roundtrip" ($r.status -eq "ok" -and $av -like "*cua-e2e4*") "index=$($addr.i) value=$av"
$treeItems = @(Nodes $stEx4 "TreeItem")
if ($treeItems.Count -gt 3) {
    $before = $treeItems.Count
    $expandable = $treeItems | Where-Object { $_.expanded -eq "collapsed" } | Select-Object -First 1
    if ($null -ne $expandable) {
        $r = J (RunCua expand --window $EX --index $expandable.i)
        Start-Sleep -Milliseconds 1200
        $after = @(Nodes (J (RunCua state --window $EX --no-shot)) "TreeItem").Count
        Check "B13 explorer navigation tree expand adds TreeItems" ($r.status -eq "ok" -and $after -gt $before) "tree $before -> $after"
        & $cua collapse --window $EX --index $expandable.i | Out-Null
    } else {
        Skip "B13 explorer nav tree expand" "nothing collapsed"
    }
} else {
    Skip "B13 explorer nav tree expand" "nav pane collapsed"
}

# ---------- C. background control while another window holds focus ----------
# Guarantee under test: cua never calls SetForegroundWindow on these paths.
# * message-level ops (--background): foreground must be byte-identical before/after
# * UIA ops: the target window has to be a *background* window when the action lands
#   (an app may raise itself as a side effect of its own handler - not cua's doing)
$F_ANCHOR = "title:CuaAnchor"
& $cua launch --exe $testpad --args "--title CuaAnchor" | Out-Null
Start-Sleep -Seconds 2
& $cua activate --window $F_ANCHOR | Out-Null
Start-Sleep -Milliseconds 500

function Is-Background($procName) {
    $a = J (RunCua apps)
    $w = $a.windows | Where-Object { $_.process -eq $procName } |
        Sort-Object { [int64]$_.rect[2] * [int64]$_.rect[3] } -Descending | Select-Object -First 1
    return ($null -ne $w -and -not $w.foreground)
}

$fg0 = (J (RunCua apps)).foreground_hwnd
$anchorFg = ($null -ne (Is-Background "charmap.exe")) -and (Is-Background "charmap.exe") -and (Is-Background "msinfo32.exe")
Check "C0 anchor holds focus, target apps are background" ($fg0 -ne 0 -and $anchorFg) "fg=$fg0"

# C1 message-level click on a real checkbox
$stCm = J (RunCua state --window $CM --no-shot)
$chk = First (Nodes $stCm "CheckBox")
$cx = $chk.rect[0] + [int]($chk.rect[2] / 2); $cy = $chk.rect[1] + [int]($chk.rect[3] / 2)
$shotCm = ShotOf $CM "$cache\e4-cm.png"
$fgA = (J (RunCua apps)).foreground_hwnd
$r = J (RunCua click --window $CM --x $cx --y $cy --shot-id $shotCm --background)
Start-Sleep -Milliseconds 800
$chk2 = (First (Nodes (J (RunCua state --window $CM --no-shot)) "CheckBox")).toggle
$fgB = (J (RunCua apps)).foreground_hwnd
Check "C1 background click drives a real app checkbox" ($r.status -eq "ok" -and $chk2 -ne $chk.toggle) "$($chk.toggle)->$chk2"
Check "C1b background click keeps focus and stays background" ($fgA -eq $fgB -and (Is-Background "charmap.exe")) "$fgA -> $fgB"
# restore with another background click (idempotent round trip)
$shotCm = ShotOf $CM "$cache\e4-cm2.png"
& $cua click --window $CM --x $cx --y $cy --shot-id $shotCm --background | Out-Null
Start-Sleep -Milliseconds 600
$chk3 = (First (Nodes (J (RunCua state --window $CM --no-shot)) "CheckBox")).toggle
Check "C1c background click round-trips the checkbox" ($chk3 -eq $chk.toggle) "$chk3 vs $($chk.toggle)"

# C2 message-level typing into a real app's edit control
$edit = First (Nodes (J (RunCua state --window $CM --no-shot)) "Edit")
& $cua set-value --window $CM --index $edit.i --value "bg-start" | Out-Null
$fgA = (J (RunCua apps)).foreground_hwnd
$r = J (RunCua type --window $CM --text "BGMSG" --background)
Start-Sleep -Milliseconds 600
$v = (J (RunCua get-value --window $CM --index $edit.i)).value
$fgB = (J (RunCua apps)).foreground_hwnd
Check "C2 background typing lands in a real app edit" ($r.status -eq "ok" -and $v -like "*BGMSG*") "value=$v"
Check "C2b background typing kept the user's focus" ($fgA -eq $fgB -and (Is-Background "charmap.exe")) "$fgA -> $fgB"

# C3 message-level navigation key then typing (caret position honoured)
& $cua set-value --window $CM --index $edit.i --value "AB" | Out-Null
& $cua key --window $CM --keys end --background | Out-Null
& $cua type --window $CM --text "CD" --background | Out-Null
Start-Sleep -Milliseconds 500
$v2 = (J (RunCua get-value --window $CM --index $edit.i)).value
$v2c = ($v2 -replace "[`r`n]", "")
Check "C3 background key(end)+type appends at the caret" ($v2c -eq "ABCD") "value=$v2"

# C4 UIA invoke against a background XAML app
& $cua activate --window $F_ANCHOR | Out-Null
Start-Sleep -Milliseconds 400
$npWasBg = Is-Background "Notepad.exe"
$stNp = J (RunCua state --window $NP --no-shot)
$npAdd = First (Nodes $stNp "Button" $F_ADDTAB)
$tabsB = @(Nodes $stNp "TabItem").Count
$r = J (RunCua click --window $NP --index $npAdd.i)
Start-Sleep -Milliseconds 1500
$tabsA = @(Nodes (J (RunCua state --window $NP --no-shot)) "TabItem").Count
Check "C4 UIA Invoke works on a background XAML app" ($npWasBg -and $r.status -eq "ok" -and $tabsA -eq ($tabsB + 1)) "bg=$npWasBg tabs $tabsB -> $tabsA"

# C5 UIA invoke against a background classic app
& $cua activate --window $F_ANCHOR | Out-Null
Start-Sleep -Milliseconds 600
$cmWasBg = Is-Background "charmap.exe"
& $cua set-value --window $CM --index $edit.i --clear | Out-Null
$stCm3 = J (RunCua state --window $CM --no-shot)
$cmSelBtn = First (Nodes $stCm3 "Button" $F_SELECT)
$r = J (RunCua click --window $CM --index $cmSelBtn.i)
Start-Sleep -Milliseconds 600
$v3 = (J (RunCua get-value --window $CM --index $edit.i)).value
Check "C5 classic app Invoke works while backgrounded" ($cmWasBg -and $r.status -eq "ok" -and $v3.Length -gt 0) "bg=$cmWasBg value=$v"

# C6 focus control: activate works again on demand
& $cua activate --window $F_ANCHOR | Out-Null
Start-Sleep -Milliseconds 400
$fgZ = (J (RunCua apps)).foreground_hwnd
$anchorWin = (J (RunCua state --window $F_ANCHOR --no-shot)).window.hwnd
Check "C6 activate restores focus to the anchor" ($fgZ -eq $anchorWin) "$fgZ vs $anchorWin"

# ---------- D. hidden desktop with real apps ----------
# close the user-desktop msinfo32 first: it raises itself when its scan finishes,
# which would make the foreground assertion below meaningless
$msPidUser = (J (RunCua state --window $MS --no-shot)).window.pid
if ($null -ne $msPidUser) { taskkill /PID $msPidUser /T /F 2>$null | Out-Null }
Start-Sleep -Milliseconds 800
& $cua activate --window $F_ANCHOR | Out-Null
Start-Sleep -Milliseconds 400
$fgSb = (J (RunCua apps)).foreground_hwnd
$preMsSb = Pids "msinfo32"
$r = J (RunCua sandbox start --exe msinfo32.exe)
Check "D1 sandbox start with a real app" ($null -ne $r.pid) "$($r | ConvertTo-Json -Compress)"
Start-Sleep -Seconds 7
$sbSt = J (RunCua sandbox state --window $MS --no-shot --patterns)
$sbTrees = @(Nodes $sbSt "TreeItem")
Check "D2 hidden-desktop UIA tree of a real app" ($sbSt.node_count -ge 60 -and $sbTrees.Count -ge 3) "nodes=$($sbSt.node_count) trees=$($sbTrees.Count)"
$sbShot = J (RunCua sandbox shot --window $MS --out "$cache\e4-sb.png")
Check "D2b hidden-desktop screenshot renders real content" ($sbShot.rendered -and (Get-Item "$cache\e4-sb.png").Length -gt 20000) "bytes=$((Get-Item "$cache\e4-sb.png").Length)"
$collapsed = @($sbTrees | Where-Object { $_.expanded -eq "collapsed" })
if ($collapsed.Count -gt 0) {
    $expBefore = @($sbTrees | Where-Object { $_.expanded -eq "expanded" }).Count
    $r = J (RunCua sandbox expand --window $MS --index $collapsed[0].i)
    Start-Sleep -Milliseconds 1500
    $expAfter = @(Nodes (J (RunCua sandbox state --window $MS --no-shot)) "TreeItem" | Where-Object { $_.expanded -eq "expanded" }).Count
    Check "D3 sandbox expand works on hidden desktop" ($r.status -eq "ok" -and $expAfter -gt $expBefore) "expanded trees $expBefore -> $expAfter"
} else {
    Check "D3 sandbox expand works on hidden desktop" $false "no collapsed node"
}
$sbSt2 = J (RunCua sandbox state --window $MS --no-shot)
$sbChk = First (Nodes $sbSt2 "CheckBox")
if ($null -ne $sbChk) {
    $r = J (RunCua sandbox toggle --window $MS --index $sbChk.i)
    $now = (First (Nodes (J (RunCua sandbox state --window $MS --no-shot)) "CheckBox")).toggle
    Check "D4 sandbox toggle works on hidden desktop" ($r.status -eq "ok" -and $now -ne $sbChk.toggle) "$($sbChk.toggle)->$now"
} else {
    Check "D4 sandbox toggle works on hidden desktop" $false "no checkbox"
}
$r = J (RunCua sandbox scroll --window $MS --dy -3)
Check "D5 sandbox message-level scroll ok" ($r.status -eq "ok") "$($r.detail)"
$userWins = (J (RunCua apps)).windows | Where-Object { $_.process -eq "msinfo32.exe" }
$userOnlySandbox = @($userWins | Where-Object { $preMsSb -notcontains $_.pid }).Count
Check "D6 sandboxed app invisible on the user desktop" ($userOnlySandbox -eq 0) "leaked=$userOnlySandbox"
# non-interference: these gestures would move the real pointer if they were SendInput-based,
# so the user's cursor and focus must be bit-identical afterwards
# sandbox state carries identity but not geometry; take the rect from apps.
$sbWin = (J (RunCua sandbox apps)).windows |
    Where-Object { $_.process -eq "msinfo32.exe" -and $_.visible -and $_.rect[2] -gt 200 -and $_.rect[3] -gt 200 } |
    Sort-Object { -($_.rect[2] * $_.rect[3]) } |
    Select-Object -First 1
$rct = if ($sbWin) { $sbWin.rect } else { $null }
if (-not $rct) {
    Check "D6b sandbox coordinate click + drag need no real pointer" $false "no msinfo32 rect on the hidden desktop"
}
$cx = if ($rct) { $rct[0] + [int]($rct[2] / 2) } else { $null }
$cy = if ($rct) { $rct[1] + [int]($rct[3] / 2) } else { $null }
if ($rct) {
    $cur0 = J (RunCua cursor)
    $sbShot2 = J (RunCua sandbox shot --window $MS --out "$cache\e4-sb2.png")
    $cli = J (RunCua sandbox click --window $MS --x $cx --y $cy --shot-id $sbShot2.state_id)
    $dr = J (RunCua sandbox drag --x1 ($cx - 30) --y1 ($cy - 20) --x2 ($cx + 30) --y2 ($cy + 20) --shot-id $sbShot2.state_id)
    $cur1 = J (RunCua cursor)
    $fgNow = (J (RunCua apps)).foreground_hwnd
    Check "D6b sandbox coordinate click + drag need no real pointer" ($cli.status -eq "ok" -and $dr.status -eq "ok") "click=$($cli.status) drag=$($dr.status)"
    Check "D6c the user's cursor never moved" ($cur0.x -eq $cur1.x -and $cur0.y -eq $cur1.y) "$($cur0.x),$($cur0.y) -> $($cur1.x),$($cur1.y)"
    Check "D6d the user's foreground window never changed" ($fgNow -eq $fgSb) "$fgSb -> $fgNow"
}
$fgMid = (J (RunCua apps)).foreground_hwnd
& $cua sandbox state --window $MS --no-shot | Out-Null
$fg6 = (J (RunCua apps)).foreground_hwnd
Check "D6b user foreground untouched by sandbox work" ($fgMid -eq $fg6 -and $fgSb -eq $fg6) "start=$fgSb mid=$fgMid end=$fg6"
$r = J (RunCua sandbox launch --exe charmap.exe)
Check "D7 sandbox launch second real app" ($null -ne $r.pid) "$($r | ConvertTo-Json -Compress)"
Start-Sleep -Seconds 3
$sbApps = J (RunCua sandbox apps)
$procs = @($sbApps.windows | ForEach-Object { $_.process } | Sort-Object -Unique)
Check "D8 both real apps run on the hidden desktop" (($procs -contains "charmap.exe") -and ($procs -contains "msinfo32.exe")) "$($procs -join ',')"
$sbCm = J (RunCua sandbox state --window "process:charmap" --no-shot)
$sbCmEdit = First (Nodes $sbCm "Edit")
$sbCmSel = First (Nodes $sbCm "Button" $F_SELECT)
$r = J (RunCua sandbox click --window "process:charmap" --index $sbCmSel.i)
Start-Sleep -Milliseconds 600
$sbVal = (J (RunCua sandbox get-value --window "process:charmap" --index $sbCmEdit.i)).value
Check "D9 sandbox Invoke works on second app" ($r.status -eq "ok" -and $sbVal -ne $sbCmEdit.value) "'$($sbCmEdit.value)' -> '$sbVal'"
$r = J (RunCua sandbox close --window "process:charmap")
Check "D10 sandbox close needs --confirm" ($r.status -eq "refused") "$($r.status)"
$r = J (RunCua sandbox close --window "process:charmap" --confirm)
Check "D11 sandbox close real app gracefully" ($r.status -eq "ok") "$($r.detail)"
$r = J (RunCua sandbox stop)
Start-Sleep -Seconds 2
$postMs = Pids "msinfo32"
$leftover = @($postMs | Where-Object { $preMsSb -notcontains $_ })
Check "D12 sandbox stop kills the app tree (no orphans)" ($r.killed_processes -ge 2 -and $leftover.Count -eq 0) "killed=$($r.killed_processes) leftover=$($leftover -join ',')"
$r = J (RunCua sandbox stop)
Check "D13 sandbox stop is idempotent" ($r.killed_processes -eq 0) "$($r.killed_processes)"

# ---------- E. safety policy against real windows ----------
$r = J (RunCua launch --exe regedit.exe)
Check "E1 launching a denied real binary is refused" ($r.status -eq "refused") "$($r.status)"
$ss = (J (RunCua apps)).windows | Where-Object { $_.process -eq "SystemSettings.exe" } | Select-Object -First 1
if ($null -ne $ss) {
    $stSs = J (RunCua state --window "$($ss.hwnd)" --no-shot --depth 2)
    $idx = 0
    $r = J (RunCua click --window "$($ss.hwnd)" --index $idx)
    Check "E2 automation of a real Settings window refused (no-negotiables)" ($r.status -eq "refused" -and $r.detail -like "*deny list*") "$($r.status)/$($r.detail)"
} else {
    Skip "E2 Settings window refusal" "no Settings window open"
}
$out = RunErr key --keys "win+r"
Check "E3 Windows-logo key refused" ($out -like "*Windows-logo key*") ($out.Substring(0, [Math]::Min(70, $out.Length)))
$r = J (RunCua open --name "ms-settings:display")
Check "E4 ms-settings URI refused" ($r.status -eq "refused") "$($r.status)"

# ---------- F. occluded capture of real apps ----------
& $cua activate --window "title:CuaAnchor" | Out-Null
Start-Sleep -Milliseconds 500
$occFile = "$cache\e4-occluded.png"
$occ = J (RunCua shot --window $CM --out $occFile)
Check "F1 occluded real dialog captured via PrintWindow" ($occ.rendered -and (Get-Item $occFile).Length -gt 20000) "bytes=$((Get-Item $occFile).Length)"
$stOcc = J (RunCua state --window $NP --out "$cache\e4-np-bg.png")
Check "F2 background window state+screenshot works" ($stOcc.screenshot.rendered -and $stOcc.node_count -gt 10) "nodes=$($stOcc.node_count) rendered=$($stOcc.screenshot.rendered)"

# ---------- cleanup ----------
& $cua sandbox stop 2>$null | Out-Null
if ($null -ne $expl) { & $cua close --window $EX --confirm 2>$null | Out-Null }
foreach ($p in @((Pids "Notepad") | Where-Object { $preNotepad -notcontains $_ })) { taskkill /PID $p /T /F 2>$null | Out-Null }
foreach ($p in @((Pids "charmap") | Where-Object { $preCharmap -notcontains $_ })) { taskkill /PID $p /T /F 2>$null | Out-Null }
foreach ($p in @((Pids "msinfo32") | Where-Object { $preMsinfo -notcontains $_ })) { taskkill /PID $p /T /F 2>$null | Out-Null }
foreach ($p in @((Pids "SndVol") | Where-Object { $preSndvol -notcontains $_ })) { taskkill /PID $p /T /F 2>$null | Out-Null }
taskkill /IM testpad.exe /F 2>$null | Out-Null
Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue

# ---------- result ----------
Write-Host ""
Write-Host ("==== PHASE-4 RESULT: {0} passed, {1} failed, {2} skipped" -f $script:pass, $script:fail, $script:skip) -ForegroundColor $(if ($script:fail -eq 0) { "Green" } else { "Red" })
if ($script:fail -gt 0) { $script:failures | ForEach-Object { Write-Host ("  failed: " + $_) -ForegroundColor Red }; exit 1 }
exit 0
