#Requires -Version 5.1
<#
.SYNOPSIS
    UIA capability health check - classifies how drivable a Windows app is
    through accessibility APIs alone (zero interference with the user's mouse).

.DESCRIPTION
    For every target app this script measures, with the `cua` CLI only:

      1. provider coverage   - does a UIA tree exist at all, and how big is it (node_count)
      2. locatability        - share of nodes carrying a non-empty name, and whether the
                               provider hands out runtime ids (rid). Apps such as QQ Music
                               expose names but NO runtime id, which blocks every pattern
                               action (set-value / toggle / select / expand / set-range).
      3. actionability       - pattern histogram over the whole tree (needs `state --patterns`)
                               plus whether any element is Invoke/Toggle/SelectionItem/
                               Value/RangeValue/ExpandCollapse capable *with* an rid.
      4. safe-write probe    - picks ONE harmless element and really performs a pattern
                               action, then reads the effect back (toggle -> re-read toggle,
                               set-range -> get-range, select -> re-read selected,
                               set-value -> get-value, expand -> tree grows, invoke -> sig
                               changes). The receipt `detail` tells which channel was used,
                               so a silent cursor fallback is detected and reported.
      5. message-level probe - `click --background --shot-id` + `type --background`
                               (PostMessage). Accepted? observable effect?
      6. capture             - `shot` -> rendered true/false (false = black/unusable frame).

    Caveat: "effect read back" is the strong signal, a bare state-sig change is the weak
    one - an app with a clock or a spinner churns its fingerprint on its own.

    Verdict ladder (first match wins, evaluated in this order):

      LAUNCH_FAILED    exe missing/unstartable, or no window/state within the timeout
      NO_PROVIDER      node_count <= 10 and (no names or no actionable pattern at all)
      A11Y_FULL        safe-write probe verified via a UIA/LegacyIAccessible pattern,
                       effect read back, and NO cursor fallback in the receipt
      A11Y_PARTIAL     patterns exist, but the probed action needed a coordinate/cursor
                       fallback, or ran yet the effect could not be read back
      MSG_LEVEL_ONLY   pattern actions useless (no actionable pattern / no rid), but
                       message-level input produced a real effect
      A11Y_RECT_ONLY   names exist but no runtime id and no pattern: only the
                     relocate-and-click-the-rectangle path works (real cursor required)
      READ_ONLY        tree + names exist, pattern actions useless, no message-level effect
      CAPTURE_BLACK    drivability unproven AND the window captures back black
      NEEDS_REAL_INPUT nothing above worked - only synthesized (SendInput) events will

    NOTE ON CHROMIUM / ELECTRON: without `--force-renderer-accessibility` a CEF/Electron
    window exposes only ~6-10 nodes (the native frame chrome) and NO web content, so it
    gets classified NO_PROVIDER even though it is perfectly drivable once the renderer
    accessibility tree is switched on. `cua launch` deliberately strips ELECTRON_* /
    NODE_* environment variables, so the env-var route (ELECTRON_FORCE_RENDERER_ACCESSIBILITY=1)
    is not available - the command line flag is the only way. That flag is therefore the
    default ExtraArgs for cloudmusic/bilibili; override per target with -ExtraArgs.

    SAFETY: the safe-write probe only ever touches one element, chosen from a conservative
    allow-list of control types, and refuses anything whose name looks destructive
    (close/min/max/quit/delete/send/pay/save/install...). It still *does* mutate app state
    (a checkbox flips, a slider moves, a list row selects) and the message-level probe types
    a throwaway token into the app's input target, so run it on apps you are willing to
    poke. Windows this script opened are closed again at the end (-KeepOpen to leave them).
    If the app is ALREADY running the script never starts a second copy: it reuses that
    window, measures it as-is, and leaves it running (and reports that its default
    ExtraArgs, e.g. the accessibility flag, were not applied - relaunch it yourself with
    the flag if you need the a11y tree).

.PARAMETER Apps
    Names from the built-in registry ('testpad','charmap','msinfo32','notepad','qqmusic',
    'cloudmusic','sodamusic','bilibili'), a comma separated string, a string array, or a
    hashtable name -> exe path (or name -> @{exe=..;args=..}) for custom targets.
    Any value that looks like a path is treated as an ad-hoc target.

.PARAMETER ExtraArgs
    hashtable target name -> extra command line args for the launch, e.g.
    -ExtraArgs @{qqmusic='--force-renderer-accessibility'}. Also accepts a bare string
    which then applies to every target that is missing its own entry.

.PARAMETER OutFile
    Markdown report path (default D:\Codebase\cua\tools\uia-report.md).

.PARAMETER Cua
    Path to cua.exe when the automatic lookup (repo bin, then target\debug) is wrong.

.PARAMETER List
    Print the known targets and exit.

.PARAMETER SelfTest
    Run the verdict ladder against synthetic measurements (no app is launched) and print
    which branch each case routes to. Use it after editing Get-Verdict.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File tools\uia-probe.ps1 -SelfTest

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File tools\uia-probe.ps1 -List

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File tools\uia-probe.ps1 -Apps testpad,charmap,msinfo32,notepad

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File tools\uia-probe.ps1 -Apps @{myapp='C:\Tools\myapp.exe'} -ExtraArgs @{myapp='--force-renderer-accessibility'}
#>
[CmdletBinding()]
param(
    [object]$Apps,
    [switch]$List,
    [object]$ExtraArgs,
    [string]$OutFile,
    [string]$Cua,
    [int]$LaunchTimeoutMs = 25000,
    [int]$SettleMs = 2500,
    [switch]$KeepOpen,
    [switch]$SelfTest
)

$ErrorActionPreference = 'Continue'
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { }
$OutputEncoding = [System.Text.Encoding]::UTF8

$script:Root = if ($PSScriptRoot) { Split-Path -Parent $PSScriptRoot } else { Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path) }
# find the CLI: -Cua, then <repo>\bin, then <repo>\target\debug, walking up from this script
$script:CuaExe = $null
$candidates = @()
if ($Cua) { $candidates += $Cua }
$probe = $script:Root
for ($i = 0; $i -lt 4 -and $probe; $i++) {
    $candidates += (Join-Path $probe 'bin\cua.exe')
    $candidates += (Join-Path $probe 'target\debug\cua.exe')
    $probe = Split-Path -Parent $probe
}
foreach ($c in $candidates) { if ($c -and (Test-Path -LiteralPath $c)) { $script:CuaExe = (Resolve-Path -LiteralPath $c).Path; break } }
if (-not $script:CuaExe) { $script:CuaExe = 'D:\Codebase\cua\bin\cua.exe' }
if (-not (Test-Path -LiteralPath $script:CuaExe)) {
    Write-Host ('(!) cua.exe not found (' + $script:CuaExe + ') - pass -Cua <path>') -ForegroundColor Red
}
$script:Scratch = Join-Path $env:TEMP 'cua-uia-probe'
New-Item -ItemType Directory -Force -Path $script:Scratch | Out-Null
# every window handle this run opened; closed again in the finally block
$script:Opened = New-Object System.Collections.ArrayList
if (-not $OutFile) { $OutFile = Join-Path $PSScriptRoot 'uia-report.md' }
$OutFile = (New-Object System.IO.FileInfo($OutFile)).FullName
$outDir = Split-Path -Parent $OutFile
if (-not $outDir -or -not (Test-Path -LiteralPath $outDir)) {
    Write-Host ('(!) report folder missing (' + $outDir + ') - writing to ' + $script:Scratch) -ForegroundColor DarkYellow
    $OutFile = Join-Path $script:Scratch 'uia-report.md'
}

# ---------------------------------------------------------------------------
# target registry
# ---------------------------------------------------------------------------
$KnownTargets = [ordered]@{
    'testpad'    = @{ exe = (Join-Path $script:Root 'bin\testpad.exe'); args = ''; note = 'project fixture: Edit/AppendMark/HitArea/checkbox/listbox/slider' }
    'charmap'    = @{ exe = 'C:\Windows\System32\charmap.exe';          args = ''; note = 'Win32 dialog fixture (#32770)' }
    'msinfo32'   = @{ exe = 'C:\Windows\System32\msinfo32.exe';         args = ''; note = 'Win32 tree + list fixture (big tree)' }
    'notepad'    = @{ exe = 'C:\Windows\System32\notepad.exe';          args = ''; note = 'Win32/XAML fixture (Document + ToggleButtons)' }
    'qqmusic'    = @{ exe = 'D:\Softwares\QQMusic\QQMusic.exe';         args = ''; note = 'TXGuiFoundation: names but NO runtime id' }
    'cloudmusic' = @{ exe = 'D:\Softwares\CloudMusic\cloudmusic.exe';   args = '--force-renderer-accessibility'; note = 'Electron - needs the a11y flag' }
    'sodamusic'  = @{ exe = 'D:\Softwares\Soda Music\3.5.1\SodaMusic.exe'; args = ''; note = 'Qt/QML based player' }
    'bilibili'   = @{ exe = 'D:\Softwares\bilibili\哔哩哔哩.exe';        args = '--force-renderer-accessibility'; note = 'Electron - needs the a11y flag' }
}

# element names that must never be the target of a live pattern action
$script:UnsafeNamePattern = '关闭|退出|最小化|最大化|恢复|还原|系统|移动|大小|调整|删除|移除|发送|提交|支付|购买|付款|充值|安装|卸载|升级|保存|另存|清空|清除|全部|关注|点赞|滚动条|翻页|上一行|下一行|位置|垂直|水平|帮助|网络|打开链接|Close|Quit|Exit|Minimize|Maximize|Restore|Resize|Move|Delete|Remove|Send|Submit|Pay|Buy|Install|Uninstall|Upgrade|Save|As ?Save|Clear|Follow|Scroll|Page|Thumb|Grip|Help'
# names that clearly mark OS-drawn frame chrome (never an app feature)
$script:ChromeTypes = @('Button', 'MenuItem', 'MenuBar', 'ScrollBar', 'Thumb', 'TitleBar', 'Window', 'Pane')
$script:CorePatterns = @('Invoke', 'Toggle', 'SelectionItem', 'Value', 'RangeValue', 'ExpandCollapse')

# ---------------------------------------------------------------------------
# low level helpers
# ---------------------------------------------------------------------------

function Invoke-Cua {
    <# Runs `cua` with an argv array, returns @{raw; json; argv; ok}. Empty-string args
       are dropped (PowerShell 5.1 would not forward them to a native exe anyway). #>
    param([string[]]$Argv, [switch]$StderrToo)
    $clean = New-Object System.Collections.Generic.List[string]
    foreach ($a in $Argv) {
        if ($null -eq $a) { continue }
        $s = [string]$a
        if ($s.Length -eq 0) { continue }
        $clean.Add($s)
    }
    $shown = ($clean -join ' ')
    if (-not (Test-Path $script:CuaExe)) {
        return [pscustomobject]@{ raw = ''; json = $null; argv = $shown; ok = $false }
    }
    $out = if ($StderrToo) { & $script:CuaExe @clean 2>&1 } else { & $script:CuaExe @clean 2>$null }
    $raw = ($out | Out-String) -replace "`r`n", "`n"
    $json = ConvertFrom-CuaJson $raw
    [pscustomobject]@{
        raw  = $raw.Trim()
        json = $json
        argv = $shown
        ok   = ($null -ne $json -and $json.status -ne 'failed' -and $json.status -ne 'refused')
    }
}

function ConvertFrom-CuaJson {
    <# BOM tolerant: cua writes UTF-8 (sometimes with a BOM) and progress noise may precede it. #>
    param([string]$Raw)
    if ([string]::IsNullOrWhiteSpace($Raw)) { return $null }
    $s = $Raw.TrimStart([char]0xFEFF, [char]0xFFFE, ' ', "`n", "`t")
    $i = $s.IndexOf('{')
    if ($i -lt 0) { return $null }
    $s = $s.Substring($i)
    try { return (ConvertFrom-Json -InputObject $s) } catch { return $null }
}

function Get-CuaState {
    param([string]$Hwnd, [switch]$Patterns)
    $argv = @('state', '--window', $Hwnd, '--no-shot')
    if ($Patterns) { $argv += '--patterns' }
    $r = Invoke-Cua $argv
    if (-not $r.json) { return $null }
    $nodes = @($r.json.nodes)
    [pscustomobject]@{
        raw = $r.raw; json = $r.json; argv = $r.argv
        node_count = [int]$r.json.node_count
        nodes = $nodes
        sig = [string]$r.json.sig
        window = $r.json.window
    }
}

function Get-Apps {
    $r = Invoke-Cua @('apps')
    if (-not $r.json) { return @() }
    return @($r.json.windows)
}

function Get-NodeName($n) { if ($null -eq $n.name) { return '' } return ([string]$n.name).Trim() }
function Get-NodeClass($n) { if ($null -eq $n.class) { return '' } return ([string]$n.class).Trim() }
function Test-HasRid($n) { ($null -ne $n.rid) -and (@($n.rid).Count -gt 0) }
function Test-HasPattern($n, [string]$Pattern) {
    if ($null -eq $n.patterns) { return $false }
    foreach ($p in @($n.patterns)) { if ([string]$p -eq $Pattern) { return $true } }
    return $false
}
function Get-NodeRect($n) {
    if ($null -eq $n.rect) { return $null }
    $a = @($n.rect)
    if ($a.Count -lt 4) { return $null }
    return @{ x = [int]$a[0]; y = [int]$a[1]; w = [int]$a[2]; h = [int]$a[3] }
}
function Test-NodeUsable($n) {
    if ($n.disabled -eq $true) { return $false }
    if ($n.offscreen -eq $true) { return $false }
    $rc = Get-NodeRect $n
    if (-not $rc) { return $false }
    return ($rc.w -gt 0 -and $rc.h -gt 0)
}
function Test-ChromeNode($n) {
    # OS-drawn frame buttons (close / min / max / system menu / scrollbar parts) have no
    # window class of their own - those must never be the probe victim.
    if ((Get-NodeClass $n).Length -gt 0) { return $false }
    $t = [string]$n.type
    foreach ($c in $script:ChromeTypes) { if ($t -eq $c) { return $true } }
    return $false
}
function Test-UnsafeNode($n) {
    $name = Get-NodeName $n
    if ($name.Length -eq 0) { return $false }
    return ($name -match $script:UnsafeNamePattern)
}

function Find-Window {
    <# Prefer the pid we launched, else the process image name, else the exe path leaf. #>
    param([int]$PidHint, [string]$ProcessLeaf, [string]$WindowTitle)
    $wins = Get-Apps
    $cand = @($wins | Where-Object {
        ($PidHint -gt 0 -and [int]$_.pid -eq $PidHint) -or
        ($ProcessLeaf.Length -gt 0 -and [string]$_.process -imatch ('^' + [regex]::Escape($ProcessLeaf) + '$'))
    })
    # a real app window: visible, not minimised, has a usable area
    $real = @($cand | Where-Object { $_.visible -and -not $_.minimized -and [int]$_.rect[2] -gt 60 -and [int]$_.rect[3] -gt 60 })
    if ($real.Count -eq 0) { $real = @($cand) }
    if ($real.Count -eq 0 -and $WindowTitle) {
        $real = @($wins | Where-Object { ("" + $_.title) -like ('*' + $WindowTitle + '*') })
    }
    if ($real.Count -eq 0) { return $null }
    $fg = @($real | Where-Object { $_.foreground })
    if ($fg.Count -gt 0) { return $fg[0] }
    return ($real | Sort-Object -Property @{ Expression = { [int]$_.rect[2] * [int]$_.rect[3] } } -Descending | Select-Object -First 1)
}

function Add-Evidence($List, [string]$Label, $Res) {
    $List.Add([pscustomobject]@{ label = $Label; argv = ('cua ' + $Res.argv); raw = $Res.raw }) | Out-Null
}

# ---------------------------------------------------------------------------
# metric builders
# ---------------------------------------------------------------------------

function Get-TreeMetrics($State) {
    if (-not $State) {
        return [pscustomobject]@{ node_count = 0; named = 0; named_pct = 0; rid_nodes = 0; rid_pct = 0;
            rect_nodes = 0; rect_pct = 0; hist = @{}; hist_text = '(no tree)'; actionable = 0; pattern_total = 0 }
    }
    $nodes = @($State.nodes)
    $named = 0; $withRid = 0; $actionable = 0; $withRect = 0
    $hist = [ordered]@{}
    $total = 0
    foreach ($n in $nodes) {
        if ((Get-NodeName $n).Length -gt 0) { $named++ }
        if (Test-HasRid $n) { $withRid++ }
        $rc = @($n.rect)
        if ($rc.Count -eq 4 -and [int]$rc[2] -gt 0 -and [int]$rc[3] -gt 0) { $withRect++ }
        if ($null -ne $n.patterns) {
            foreach ($p in @($n.patterns)) {
                $key = [string]$p
                $total++
                if ($hist.Contains($key)) { $hist[$key] = $hist[$key] + 1 } else { $hist[$key] = 1 }
            }
        }
        foreach ($cp in $script:CorePatterns) {
            if ((Test-HasPattern $n $cp) -and (Test-HasRid $n) -and (Test-NodeUsable $n)) { $actionable++; break }
        }
    }
    $count = $nodes.Count
    $parts = @()
    foreach ($e in ($hist.GetEnumerator() | Sort-Object -Property Value -Descending)) { $parts += ('{0}:{1}' -f $e.Key, $e.Value) }
    [pscustomobject]@{
        node_count      = $count
        named           = $named
        named_pct       = if ($count) { [math]::Round(100.0 * $named / $count, 1) } else { 0 }
        rid_nodes       = $withRid
        rid_pct         = if ($count) { [math]::Round(100.0 * $withRid / $count, 1) } else { 0 }
        rect_nodes      = $withRect
        rect_pct        = if ($count) { [math]::Round(100.0 * $withRect / $count, 1) } else { 0 }
        hist            = $hist
        hist_text       = if ($parts.Count) { ($parts -join ' ') } else { '(none)' }
        actionable      = $actionable
        pattern_total   = $total
    }
}

# ---------------------------------------------------------------------------
# safe-write probe
# ---------------------------------------------------------------------------

# ordered plan: cheapest / most reversible pattern action first
$ProbePlan = @(
    [pscustomobject]@{ kind = 'Toggle'; cmd = 'toggle'; pattern = 'Toggle'; types = @('CheckBox', 'RadioButton', 'Button'); need_name = $true }
    [pscustomobject]@{ kind = 'RangeValue'; cmd = 'set-range'; pattern = 'RangeValue'; types = @('Slider', 'Spinner'); need_name = $false }
    [pscustomobject]@{ kind = 'SelectionItem'; cmd = 'select'; pattern = 'SelectionItem'; types = @('ListItem', 'TreeItem', 'TabItem'); need_name = $false }
    [pscustomobject]@{ kind = 'Value'; cmd = 'set-value'; pattern = 'Value'; types = @('Edit', 'Document', 'ComboBox', 'Text'); need_name = $false }
    [pscustomobject]@{ kind = 'ExpandCollapse'; cmd = 'expand'; pattern = 'ExpandCollapse'; types = @('ComboBox', 'TreeItem', 'MenuItem', 'Button', 'Pane', 'Group'); need_name = $true }
    [pscustomobject]@{ kind = 'Invoke'; cmd = 'click'; pattern = 'Invoke'; types = @('Button', 'Hyperlink', 'MenuItem', 'ListItem', 'Edit', 'Pane'); need_name = $true }
)

function Select-ProbeTarget {
    param($Nodes, [bool]$AllowChrome)
    foreach ($plan in $ProbePlan) {
        foreach ($n in $Nodes) {
            if (-not (Test-HasPattern $n $plan.pattern)) { continue }
            if (-not (Test-HasRid $n)) { continue }
            if (-not (Test-NodeUsable $n)) { continue }
            if (Test-UnsafeNode $n) { continue }
            if ($AllowChrome -eq $false -and (Test-ChromeNode $n)) { continue }
            $t = [string]$n.type
            $typeOk = $false
            foreach ($tt in $plan.types) { if ($t -eq $tt) { $typeOk = $true } }
            if (-not $typeOk) { continue }
            if ($plan.need_name -and (Get-NodeName $n).Length -eq 0) { continue }
            # sliders / scroll bars driven by RangeValue must not be scroll bars
            if ($plan.kind -eq 'RangeValue' -and $t -eq 'ScrollBar') { continue }
            # an already-selected row / already-expanded branch would "verify" without doing anything
            if ($plan.kind -eq 'SelectionItem' -and $n.selected -eq $true) { continue }
            if ($plan.kind -eq 'ExpandCollapse' -and [string]$n.expanded -eq 'expanded') { continue }
            if ($plan.kind -eq 'ExpandCollapse' -and $t -eq 'MenuBar') { continue }
            return [pscustomobject]@{ plan = $plan; node = $n }
        }
    }
    return $null
}

function Classify-Channel([string]$Detail) {
    if ([string]::IsNullOrEmpty($Detail)) { return 'unknown' }
    if ($Detail -match 'via UIA') { return 'uia-pattern' }
    if ($Detail -match 'via LegacyIAccessible') { return 'msaa-default-action' }
    if ($Detail -match 'event strategy') { return 'cursor-fallback' }
    if ($Detail -match 'message-level') { return 'message-level' }
    if ($Detail -match 'no runtime id') { return 'cursor-by-rect' }
    if ($Detail -match 'hovered') { return 'cursor-fallback' }
    return 'unspecified'
}

function Invoke-SafeWriteProbe {
    param([string]$Hwnd, $TreeState, [bool]$AllowChrome, $Evidence)

    $sel = Select-ProbeTarget -Nodes $TreeState.nodes -AllowChrome $AllowChrome
    if (-not $sel) {
        return [pscustomobject]@{ attempted = $false; result = 'NO_CANDIDATE'; channel = 'n/a';
            kind = ''; node_desc = ''; detail = 'no element passed the safety allow-list (pattern+rid+enabled+on-screen)';
            effect = $false; restored = ''; before = ''; after = '' }
    }
    $plan = $sel.plan; $node = $sel.node
    $idx = [int]$node.i
    $name = Get-NodeName $node
    $desc = '{0} #{1} "{2}" [{3}]' -f $node.type, $idx, $name, (Get-NodeClass $node)
    $token = 'cua{0}' -f (Get-Random -Minimum 1000 -Maximum 9999)

    $before = Get-CuaState $Hwnd           # refresh the element cache right before acting
    if ($before) { Add-Evidence $Evidence 'state (probe baseline)' $before | Out-Null }
    $beforeSig = if ($before) { $before.sig } else { $TreeState.sig }
    $baseCount = if ($before) { $before.node_count } else { $TreeState.node_count }
    $baseNodes = if ($before) { @($before.nodes) } else { @($TreeState.nodes) }
    $baseNode = Get-LiveNode $baseNodes $node
    if (-not $baseNode) { $baseNode = $node }

    $argv = @($plan.cmd, '--window', $Hwnd, '--index', [string]$idx)
    $origRange = $null; $newRange = $null; $origValue = ''
    if ($null -ne $baseNode.value) { $origValue = [string]$baseNode.value }
    switch ($plan.kind) {
        'Value' { $argv += @('--value', $token) }
        'RangeValue' {
            $rng = @($baseNode.range)
            if ($rng.Count -ge 3) {
                $cur = [double]$rng[0]; $min = [double]$rng[1]; $max = [double]$rng[2]
                $origRange = $cur
                $cand1 = $min + ($max - $min) * 0.5
                $cand2 = $min + ($max - $min) * 0.75
                $newRange = if ([math]::Abs($cur - $cand1) -gt 0.5) { $cand1 } else { $cand2 }
                if ([math]::Abs($newRange - $cur) -lt 0.5) { $newRange = $null }
            }
            if ($null -eq $newRange) {
                return [pscustomobject]@{ attempted = $false; result = 'NO_CANDIDATE'; channel = 'n/a';
                    kind = $plan.kind; node_desc = $desc; detail = 'slider already sits on every reachable probe value';
                    effect = $false; restored = ''; before = ''; after = '' }
            }
            $argv += @('--value', ('{0:0.###}' -f $newRange))
        }
    }

    $act = Invoke-Cua $argv
    Add-Evidence $Evidence ("safe-write action: " + $plan.cmd) $act | Out-Null
    $detail = if ($act.json) { [string]$act.json.detail } else { '(no receipt: ' + $act.raw + ')' }
    $channel = Classify-Channel $detail
    $statusOk = ($act.json -and [string]$act.json.status -eq 'ok')

    Start-Sleep -Milliseconds 400
    $after = Get-CuaState $Hwnd -Patterns:$false
    if ($after) { Add-Evidence $Evidence 'state (read-back)' $after | Out-Null }
    $afterSig = if ($after) { $after.sig } else { '' }
    $afterNodes = if ($after) { @($after.nodes) } else { @() }
    $live = Get-LiveNode $afterNodes $baseNode
    $liveIdx = if ($live) { [int]$live.i } else { $idx }

    $effect = $false; $effectNote = ''
    switch ($plan.kind) {
        'Toggle' {
            $b = [string]$baseNode.toggle; $a = if ($live) { [string]$live.toggle } else { '' }
            $effect = ($b -ne $a -and $a -match 'on|off')
            $effectNote = 'toggle "{0}" -> "{1}"' -f $b, $a
            # some MSAA-backed checkboxes echo the PRE-toggle state in the receipt
            if ($detail -match 'now\s+(on|off|indeterminate)') {
                $claimed = $Matches[1]
                if ($claimed -ne $a) {
                    $effectNote += (' [receipt claims "{0}" but the tree reads "{1}" - provider echoes the pre-toggle state, read-back wins]' -f $claimed, $a)
                }
            }
        }
        'RangeValue' {
            $g = Invoke-Cua @('get-range', '--window', $Hwnd, '--index', [string]$liveIdx)
            Add-Evidence $Evidence 'get-range read-back' $g | Out-Null
            $v = $null
            if ($g.json -and $null -ne $g.json.range) { $v = [double]$g.json.range.value }
            $effect = ($null -ne $v -and [math]::Abs($v - $newRange) -lt 1.0)
            $effectNote = 'range {0} -> {1} (asked {2})' -f $origRange, $v, $newRange
        }
        'SelectionItem' {
            $effect = ($live -and $live.selected -eq $true)
            $effectNote = 'selected -> {0}' -f $(if ($live) { $live.selected } else { 'node vanished' })
        }
        'Value' {
            $g = Invoke-Cua @('get-value', '--window', $Hwnd, '--index', [string]$liveIdx)
            Add-Evidence $Evidence 'get-value read-back' $g | Out-Null
            $v = if ($g.json) { [string]$g.json.value } else { '' }
            $effect = ($v -like ('*' + $token + '*'))
            $effectNote = 'value now "' + $v.Substring(0, [Math]::Min(48, $v.Length)) + '"'
        }
        'ExpandCollapse' {
            $grown = ($after -and $after.node_count -gt $baseCount)
            $exp = ($live -and [string]$live.expanded -eq 'expanded')
            $effect = ($grown -or $exp)
            $effectNote = 'node_count {0} -> {1}, expanded={2}' -f $baseCount, $(if ($after) { $after.node_count } else { '?' }), $(if ($live) { $live.expanded } else { '?' })
        }
        'Invoke' {
            $effect = ($afterSig.Length -gt 0 -and $afterSig -ne $beforeSig)
            $effectNote = 'sig {0} -> {1}' -f $beforeSig, $afterSig
        }
    }

    # best-effort restore so repeated runs stay comparable
    $restored = ''
    switch ($plan.kind) {
        'Toggle' {
            $t = Invoke-Cua @('toggle', '--window', $Hwnd, '--index', [string]$liveIdx)
            $restored = if ($t.ok) { 'toggled back' } else { 'NOT restored' }
        }
        'RangeValue' {
            if ($null -ne $origRange) {
                $t = Invoke-Cua @('set-range', '--window', $Hwnd, '--index', [string]$liveIdx, '--value', ('{0:0.###}' -f $origRange))
                $restored = if ($t.ok) { 'range reset to ' + $origRange } else { 'NOT restored' }
            }
        }
        'Value' {
            if ($origValue) {
                $t = Invoke-Cua @('set-value', '--window', $Hwnd, '--index', [string]$liveIdx, '--value', $origValue)
                $restored = if ($t.ok) { 'old value restored' } else { 'NOT restored' }
            } else {
                $t = Invoke-Cua @('set-value', '--window', $Hwnd, '--index', [string]$liveIdx, '--clear')
                $restored = if ($t.ok) { 'field cleared' } else { 'probe text left in place' }
            }
        }
        'ExpandCollapse' {
            $t = Invoke-Cua @('collapse', '--window', $Hwnd, '--index', [string]$liveIdx)
            $restored = if ($t.ok) { 'collapsed again' } else { 'menu may stay open - Escape sent' }
            if (-not $t.ok) { Invoke-Cua @('key', '--keys', 'escape', '--window', $Hwnd) | Out-Null }
        }
        'SelectionItem' { $restored = 'selection left as is (harmless)' }
        'Invoke' { $restored = 'n/a (append-only action)' }
    }

    $result = 'FAIL'
    if ($statusOk -and $effect -and ($channel -eq 'uia-pattern' -or $channel -eq 'msaa-default-action')) { $result = 'PASS_CLEAN' }
    elseif ($statusOk -and $channel -eq 'uia-pattern') { $result = 'PASS_UNVERIFIED' }
    elseif ($statusOk -and ($channel -eq 'cursor-fallback' -or $channel -eq 'cursor-by-rect')) { $result = 'FALLBACK' }

    [pscustomobject]@{
        attempted  = $true
        result     = $result
        channel    = $channel
        kind       = $plan.kind
        node_desc  = $desc
        detail     = $detail
        effect     = $effect
        effectNote = $effectNote
        restored   = $restored
        before     = $beforeSig
        after      = $afterSig
    }
}

function Get-LiveNode($Nodes, $Ref) {
    <# Re-locate a node across a re-observation: runtime id first, then name+type, then index. #>
    if (-not $Nodes -or $Nodes.Count -eq 0) { return $null }
    if ($null -ne $Ref.rid) {
        $ridKey = (@($Ref.rid) -join ',')
        foreach ($n in $Nodes) { if ((@($n.rid) -join ',') -eq $ridKey) { return $n } }
    }
    $nm = Get-NodeName $Ref
    if ($nm.Length -gt 0) {
        foreach ($n in $Nodes) { if ((Get-NodeName $n) -eq $nm -and [string]$n.type -eq [string]$Ref.type) { return $n } }
        foreach ($n in $Nodes) { if ((Get-NodeName $n) -eq $nm) { return $n } }
    }
    foreach ($n in $Nodes) { if ([int]$n.i -eq [int]$Ref.i) { return $n } }
    return $null
}

# ---------------------------------------------------------------------------
# message-level probe
# ---------------------------------------------------------------------------

function Invoke-MessageProbe {
    param([string]$Hwnd, $TreeState, $Evidence)

    $token = 'MSG{0}' -f (Get-Random -Minimum 1000 -Maximum 9999)
    $before = Get-CuaState $Hwnd
    if (-not $before) {
        return [pscustomobject]@{ attempted = $false; accepted = $false; effect = $false; summary = 'no state to compare against';
            type_detail = ''; click_detail = ''; click = 'skipped'; type = 'skipped' }
    }
    $baseSig = $before.sig

    # a fresh shot id is mandatory for coordinate clicks (cua staleness guard)
    $png = Join-Path $script:Scratch ('bg-' + $Hwnd + '.png')
    $shot = Invoke-Cua @('shot', '--window', $Hwnd, '--out', $png)
    Add-Evidence $Evidence 'shot (for shot-id)' $shot | Out-Null
    $sid = if ($shot.json) { [string]$shot.json.state_id } else { '' }

    $point = Select-MessagePoint -TreeState $TreeState -Before $before
    $clickDetail = '(not attempted)'; $clickOk = $false; $clickEffect = $false
    if ($sid.Length -gt 0 -and $point) {
        $c = Invoke-Cua @('click', '--window', $Hwnd, '--x', [string]$point.x, '--y', [string]$point.y, '--background', '--shot-id', $sid)
        Add-Evidence $Evidence 'message-level click' $c | Out-Null
        $clickDetail = if ($c.json) { [string]$c.json.detail } else { $c.raw }
        $clickOk = ($c.json -and [string]$c.json.status -eq 'ok')
        Start-Sleep -Milliseconds 300
        $a1 = Get-CuaState $Hwnd
        if ($a1) { $clickEffect = ($a1.sig -ne $baseSig) }
        $baseSig = if ($a1) { $a1.sig } else { $baseSig }
    }

    $t = Invoke-Cua @('type', '--window', $Hwnd, '--text', $token, '--background')
    Add-Evidence $Evidence 'message-level type' $t | Out-Null
    $typeDetail = if ($t.json) { [string]$t.json.detail } else { $t.raw }
    $typeOk = ($t.json -and [string]$t.json.status -eq 'ok')
    Start-Sleep -Milliseconds 400
    $a2 = Get-CuaState $Hwnd -Patterns
    $typeEffect = $false
    if ($a2) {
        $typeEffect = ($a2.sig -ne $baseSig)
        foreach ($n in $a2.nodes) {
            foreach ($field in @('name', 'value')) {
                $v = [string]$n.$field
                if ($v -like ('*' + $token + '*')) { $typeEffect = $true }
            }
        }
    }

    $accepted = ($clickOk -or $typeOk)
    $effect = ($clickEffect -or $typeEffect)
    $summary = 'click={0} (effect {1}); type={2} (effect {3})' -f $(if ($clickOk) { 'ok' } else { 'no' }), $(if ($clickEffect) { 'yes' } else { 'no' }), $(if ($typeOk) { 'ok' } else { 'no' }), $(if ($typeEffect) { 'yes' } else { 'no' })
    [pscustomobject]@{
        attempted   = $true
        accepted    = $accepted
        effect      = $effect
        summary     = $summary
        type_detail = $typeDetail
        click_detail = $clickDetail
        type        = [pscustomobject]@{ ok = $typeOk; effect = $typeEffect }
        click       = [pscustomobject]@{ ok = $clickOk; effect = $clickEffect; point = $point }
    }
}

function Select-MessagePoint {
    param($TreeState, $Before)
    # prefer a text-ish surface so PostMessage input has a chance of landing somewhere harmless
    $pool = @()
    if ($Before) { $pool = @($Before.nodes) }
    foreach ($want in @('Edit', 'Document', 'Text', 'Pane')) {
        foreach ($n in $pool) {
            if ([string]$n.type -ne $want) { continue }
            if ($n.disabled -eq $true -or $n.offscreen -eq $true) { continue }
            $rc = Get-NodeRect $n
            if (-not $rc -or $rc.w -lt 10 -or $rc.h -lt 10) { continue }
            if ((Get-NodeClass $n).Length -eq 0) { continue }
            if (Get-NodeName $n -match $script:UnsafeNamePattern) { continue }
            return @{ x = ($rc.x + [int]($rc.w / 2)); y = ($rc.y + [int]($rc.h / 2)); via = ([string]$n.type + ' ' + (Get-NodeName $n)) }
        }
    }
    if ($Before -and $Before.window) {
        $wr = @($Before.window.rect)
        if ($wr.Count -ge 4) { return @{ x = ([int]$wr[0] + [int]($wr[2] / 2)); y = ([int]$wr[1] + [int]($wr[3] / 2)); via = 'window centre' } }
    }
    return $null
}

function Invoke-CaptureProbe {
    param([string]$Hwnd, $Evidence)
    $png = Join-Path $script:Scratch ('cap-' + $Hwnd + '.png')
    $s = Invoke-Cua @('shot', '--window', $Hwnd, '--out', $png)
    Add-Evidence $Evidence 'shot (capture health)' $s | Out-Null
    [pscustomobject]@{
        rendered = [bool]($s.json -and $s.json.rendered -eq $true)
        width    = if ($s.json) { [int]$s.json.width } else { 0 }
        height   = if ($s.json) { [int]$s.json.height } else { 0 }
        detail   = if ($s.json) { 'rendered=' + $s.json.rendered + ' ' + $s.json.width + 'x' + $s.json.height } else { $s.raw }
        path     = $png
    }
}

# ---------------------------------------------------------------------------
# verdict ladder
# ---------------------------------------------------------------------------

function Get-Verdict {
    param($m, $probe, $msg, $cap, [bool]$NoWindow)

    # 1. could not even get a window / a tree
    if ($NoWindow) { return 'LAUNCH_FAILED' }
    # 2. no usable accessibility provider behind the window
    if ($m.node_count -eq 0) { return 'NO_PROVIDER' }
    if ($m.node_count -le 10 -and ($m.named -eq 0 -or $m.actionable -eq 0)) { return 'NO_PROVIDER' }

    # 3. a pattern action landed, the effect was read back, no cursor was involved
    if ($probe.attempted -and $probe.result -eq 'PASS_CLEAN') { return 'A11Y_FULL' }

    $patternsUseless = ($m.actionable -eq 0)
    if (-not $patternsUseless) {
        # 4. patterns are real, but the probe needed a fallback or could not be confirmed
        if ($probe.attempted -and ($probe.result -eq 'PASS_UNVERIFIED' -or $probe.result -eq 'FALLBACK')) { return 'A11Y_PARTIAL' }
        if ($probe.result -eq 'NO_CANDIDATE') { return 'A11Y_PARTIAL' }
        # pattern actions were advertised yet nothing landed: judge by the message channel
        if ($msg.accepted -and $msg.effect) { return 'MSG_LEVEL_ONLY' }
        if ($cap -and -not $cap.rendered) { return 'CAPTURE_BLACK' }
        return 'NEEDS_REAL_INPUT'
    }
    # 5. no usable pattern at all (or no runtime id to point one at)
    if ($msg.accepted -and $msg.effect) { return 'MSG_LEVEL_ONLY' }
    # 5b. name-locatable but pattern-less: cua still drives it by relocating the element and
    #     clicking its rectangle - semantic targeting, but a real cursor (e.g. QQ Music)
    if ($m.actionable -eq 0 -and $m.named -gt 0 -and $m.rect_pct -gt 0) {
        if ($cap -and -not $cap.rendered) { return 'CAPTURE_BLACK' }
        return 'A11Y_RECT_ONLY'
    }
    # 6. readable tree with names, but neither channel moves it
    if ($m.named -gt 0) {
        if ($cap -and -not $cap.rendered) { return 'CAPTURE_BLACK' }
        return 'READ_ONLY'
    }
    # 7. last resort: nothing observable through a11y or messages, and the pixels are black too
    if ($cap -and -not $cap.rendered) { return 'CAPTURE_BLACK' }
    return 'NEEDS_REAL_INPUT'
}

# ---------------------------------------------------------------------------
# target resolution
# ---------------------------------------------------------------------------

function Resolve-Targets {
    param([object]$Apps)
    $picked = [ordered]@{}
    $addKnown = {
        param([string]$name)
        if ($KnownTargets.Contains($name)) { $picked[$name] = $KnownTargets[$name] }
    }
    if ($null -eq $Apps) {
        foreach ($k in $KnownTargets.Keys) { $picked[$k] = $KnownTargets[$k] }
        return $picked
    }
    if ($Apps -is [hashtable] -or $Apps -is [System.Collections.Specialized.OrderedDictionary]) {
        foreach ($k in $Apps.Keys) {
            $v = $Apps[$k]
            if ($v -is [hashtable]) { $picked[[string]$k] = $v }
            else { $picked[[string]$k] = @{ exe = [string]$v; args = ''; note = 'user supplied' } }
        }
        return $picked
    }
    $items = @($Apps)
    if ($Apps -is [string]) { $items = @($Apps -split ',') }
    foreach ($raw in $items) {
        $it = ([string]$raw).Trim()
        if ($it.Length -eq 0) { continue }
        if ($it -match '\\' -or $it -match '\.exe$') {
            $leaf = [System.IO.Path]::GetFileNameWithoutExtension($it)
            $picked[$leaf] = @{ exe = $it; args = ''; note = 'ad-hoc path' }
            continue
        }
        & $addKnown $it.ToLower()
    }
    return $picked
}

function Merge-ExtraArgs {
    param($Targets, [object]$ExtraArgs)
    if ($null -eq $ExtraArgs) { return }
    if ($ExtraArgs -is [hashtable]) {
        foreach ($k in $ExtraArgs.Keys) {
            $name = ([string]$k).ToLower()
            if ($Targets.Contains($name)) { $Targets[$name]['args'] = [string]$ExtraArgs[$k] }
        }
        return
    }
    $shared = [string]$ExtraArgs
    foreach ($k in @($Targets.Keys)) {
        if ([string]$Targets[$k]['args'] -eq '--force-renderer-accessibility' -or [string]$Targets[$k]['args'].Length -eq 0) {
            $Targets[$k]['args'] = $shared
        }
    }
}

# ---------------------------------------------------------------------------
# per target run
# ---------------------------------------------------------------------------

function Test-ExePath {
    param([string]$Exe)
    if (Test-Path -LiteralPath $Exe) { return $Exe }
    # CJK file names survive a mangled console: fall back to the directory glob
    try {
        $dir = Split-Path -Parent $Exe
        if (Test-Path -LiteralPath $dir) {
            $leaf = [System.IO.Path]::GetFileName($Exe)
            $skeleton = $leaf -replace '[^\x00-\x7F]', ''
            # CJK names survive a mangled console with the same *shape*: require an identical
            # length as well as the same ASCII skeleton, or '哔哩哔哩.exe' also matches the
            # uninstaller in that folder. Never fall back onto an uninstaller/updater.
            $hit = @(Get-ChildItem -LiteralPath $dir -Filter '*.exe' -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -ne $leaf } |
                Where-Object { $_.Name -notmatch '(?i)unins|uninstall|updater|setup|crash|repair|卸载|更新|修复' } |
                Where-Object {
                    $_.Name.Length -eq $leaf.Length -and
                    ((($_.Name -replace '[^\x00-\x7F]', '') -eq $skeleton) -or ($_.Name -eq $leaf))
                } |
                Select-Object -First 1)
            if ($hit.Count -gt 0) { return $hit[0].FullName }
        }
    } catch { }
    return $null
}

function New-StubProbe([string]$result, [string]$why) {
    [pscustomobject]@{ attempted = $false; result = $result; channel = 'n/a'; kind = ''; node_desc = '';
        detail = $why; effect = $false; effectNote = ''; restored = ''; before = ''; after = '' }
}
function New-StubMsg([string]$why) {
    [pscustomobject]@{ attempted = $false; accepted = $false; effect = $false; summary = $why;
        type_detail = ''; click_detail = ''; type = $null; click = $null }
}

# --- fixtures for -SelfTest (verifies the ladder without touching any app) ---
function New-FakeMetrics([int]$NodeCount, [int]$Named, [int]$Actionable, [double]$RectPct = 100) {
    [pscustomobject]@{ node_count = $NodeCount; named = $Named; named_pct = 0; rid_nodes = 0; rid_pct = 0;
        rect_nodes = 0; rect_pct = $RectPct; hist = @{}; hist_text = 'selftest'; actionable = $Actionable; pattern_total = 0 }
}
function New-FakeProbe([string]$Result, [string]$Channel, [bool]$Effect) {
    [pscustomobject]@{ attempted = $true; result = $Result; channel = $Channel; kind = 'Toggle';
        node_desc = 'selftest'; detail = 'selftest'; effect = $Effect; effectNote = ''; restored = ''; before = ''; after = '' }
}
function New-FakeMsg([bool]$Accepted, [bool]$Effect) {
    [pscustomobject]@{ attempted = $true; accepted = $Accepted; effect = $Effect; summary = 'selftest';
        type_detail = ''; click_detail = ''; type = $null; click = $null }
}
function New-FakeCapture([bool]$Rendered) {
    [pscustomobject]@{ rendered = $Rendered; width = 800; height = 600; detail = 'selftest'; path = '' }
}

function Show-LadderSelfTest {
    $cases = @(
        @{ why = 'no window at all';                                        m = (New-FakeMetrics 0 0 0);        p = (New-StubProbe 'NOT_RUN' 'x');     g = (New-StubMsg 'x');              c = $null;                      nw = $true;  want = 'LAUNCH_FAILED' }
        @{ why = 'tiny unnamed tree (no provider)';                         m = (New-FakeMetrics 8 0 0);         p = (New-StubProbe 'NO_CANDIDATE' 'x'); g = (New-StubMsg 'x');             c = (New-FakeCapture $true);      nw = $false; want = 'NO_PROVIDER' }
        @{ why = '6 chrome nodes, names, no usable pattern (Electron flag off)'; m = (New-FakeMetrics 6 6 0);     p = (New-StubProbe 'NO_CANDIDATE' 'x'); g = (New-StubMsg $false $false);  c = (New-FakeCapture $true);      nw = $false; want = 'NO_PROVIDER' }
        @{ why = 'pattern action verified, no fallback';                    m = (New-FakeMetrics 22 18 15);      p = (New-FakeProbe 'PASS_CLEAN' 'uia-pattern' $true); g = (New-FakeMsg $true $true); c = (New-FakeCapture $true);  nw = $false; want = 'A11Y_FULL' }
        @{ why = 'action landed through a cursor fallback';                 m = (New-FakeMetrics 22 18 15);      p = (New-FakeProbe 'FALLBACK' 'cursor-fallback' $true); g = (New-FakeMsg $true $true); c = (New-FakeCapture $true); nw = $false; want = 'A11Y_PARTIAL' }
        @{ why = 'pattern action ok but effect not readable back';          m = (New-FakeMetrics 22 18 15);      p = (New-FakeProbe 'PASS_UNVERIFIED' 'uia-pattern' $false); g = (New-FakeMsg $true $true); c = (New-FakeCapture $true); nw = $false; want = 'A11Y_PARTIAL' }
        @{ why = 'patterns present but nothing safe to poke';               m = (New-FakeMetrics 40 30 12);      p = (New-StubProbe 'NO_CANDIDATE' 'x'); g = (New-FakeMsg $true $true);   c = (New-FakeCapture $true);      nw = $false; want = 'A11Y_PARTIAL' }
        @{ why = 'no runtime id (QQ Music), message input works';           m = (New-FakeMetrics 300 260 0);     p = (New-StubProbe 'NO_CANDIDATE' 'no rid'); g = (New-FakeMsg $true $true); c = (New-FakeCapture $true);     nw = $false; want = 'MSG_LEVEL_ONLY' }
        @{ why = 'named but no runtime id: rectangle click is the only path';     m = (New-FakeMetrics 150 130 0);     p = (New-StubProbe 'NO_CANDIDATE' 'no rid'); g = (New-FakeMsg $false $false); c = (New-FakeCapture $true);  nw = $false; want = 'A11Y_RECT_ONLY' }
        @{ why = 'names only, no clickable rect, both channels dead';         m = (New-FakeMetrics 300 260 0 0);     p = (New-StubProbe 'NO_CANDIDATE' 'no rid'); g = (New-FakeMsg $true $false); c = (New-FakeCapture $true);    nw = $false; want = 'READ_ONLY' }
        @{ why = 'patterns advertised, every action failed, input works';   m = (New-FakeMetrics 50 40 20);      p = (New-FakeProbe 'FAIL' 'unspecified' $false); g = (New-FakeMsg $true $true); c = (New-FakeCapture $true);    nw = $false; want = 'MSG_LEVEL_ONLY' }
        @{ why = 'patterns advertised, nothing works, capture fine';        m = (New-FakeMetrics 50 40 20);      p = (New-FakeProbe 'FAIL' 'unspecified' $false); g = (New-FakeMsg $false $false); c = (New-FakeCapture $true);   nw = $false; want = 'NEEDS_REAL_INPUT' }
        @{ why = 'nothing works and the capture is black';                  m = (New-FakeMetrics 50 40 20);      p = (New-FakeProbe 'FAIL' 'unspecified' $false); g = (New-FakeMsg $false $false); c = (New-FakeCapture $false);  nw = $false; want = 'CAPTURE_BLACK' }
        @{ why = 'unnamed tree over 10 nodes, dead everywhere, black';      m = (New-FakeMetrics 30 0 0);        p = (New-StubProbe 'NO_TREE' 'x');     g = (New-FakeMsg $false $false);  c = (New-FakeCapture $false);     nw = $false; want = 'CAPTURE_BLACK' }
    )
    $bad = 0
    Write-Host '== verdict ladder self test' -ForegroundColor Cyan
    foreach ($case in $cases) {
        $got = Get-Verdict -m $case.m -probe $case.p -msg $case.g -cap $case.c -NoWindow $case.nw
        $ok = ($got -eq $case.want)
        if (-not $ok) { $bad++ }
        Write-Host ('  {0} {1,-16} want={2,-17} got={3,-17} {4}' -f $(if ($ok) { 'ok  ' } else { 'MISS' }), $case.want, $case.want, $got, $case.why) -ForegroundColor $(if ($ok) { 'Green' } else { 'Red' })
    }
    Write-Host ('  ' + $(if ($bad -eq 0) { 'all ladder branches behave as documented' } else { $bad.ToString() + ' branch(es) mis-routed - fix Get-Verdict' })) -ForegroundColor $(if ($bad -eq 0) { 'Green' } else { 'Red' })
    return $bad
}

function Run-Target {
    param([string]$Name, $Def, $Evidence)

    $t = [ordered]@{
        name = $Name; exe = $Def.exe; extra = [string]$Def.args; note = [string]$Def.note
        exists = $false; launched = $false; reused = $false; hwnd = ''
        title = ''; process = ''; pid = 0
        metrics = Get-TreeMetrics $null
        probe = (New-StubProbe 'NOT_RUN' 'target never reached the probe stage')
        msg = (New-StubMsg 'not reached')
        capture = $null; verdict = 'LAUNCH_FAILED'; detail_lines = @(); closed = ''
    }
    $exe = Test-ExePath $Def.exe
    if (-not $exe) {
        $t['detail_lines'] = @('exe not found: ' + $Def.exe)
        Write-Host ('  SKIP  ' + $Name + ': not installed (' + $Def.exe + ')') -ForegroundColor DarkYellow
        return [pscustomobject]$t
    }
    $t['exe'] = $exe
    $t['exists'] = $true
    $leaf = [System.IO.Path]::GetFileName($exe)

    # already running? reuse it (never disturb a session the user is working in)
    $win = Find-Window -PidHint 0 -ProcessLeaf $leaf -WindowTitle ''
    if ($win) {
        $t['reused'] = $true
        if ($t['extra'].Length -gt 0) {
            $t['detail_lines'] = @('reused running instance - ExtraArgs "' + $t['extra'] + '" NOT applied; relaunch with the a11y flag for a fair verdict')
        }
    } else {
        $argv = @('launch', '--exe', $exe)
        if ($t['extra'].Length -gt 0) { $argv += @('--args', $t['extra']) }
        $r = Invoke-Cua $argv
        Add-Evidence $Evidence 'launch' $r | Out-Null
        if (-not $r.json -or [string]$r.json.status -ne 'ok') {
            $t['detail_lines'] = @('launch refused/failed: ' + $(if ($r.json) { $r.json.detail } else { $r.raw }))
            Write-Host ('  FAIL  ' + $Name + ': launch refused') -ForegroundColor Red
            return [pscustomobject]$t
        }
        $t['launched'] = $true
        $launchedPid = 0
        if ([string]$r.json.detail -match 'pid\s+(\d+)') { $launchedPid = [int]$Matches[1] }
        $t['pid'] = $launchedPid
        $deadline = (Get-Date).AddMilliseconds($LaunchTimeoutMs)
        while ((Get-Date) -lt $deadline) {
            $win = Find-Window -PidHint $launchedPid -ProcessLeaf $leaf -WindowTitle ''
            if ($win) { break }
            Start-Sleep -Milliseconds 600
        }
        if ($win) { Start-Sleep -Milliseconds $SettleMs }
    }
    if (-not $win) {
        $t['detail_lines'] = @('no window for ' + $leaf + ' within ' + $LaunchTimeoutMs + 'ms')
        if ($t['pid']) {
            # the process is up but windowless (splash / single-instance handoff) - do not leak it
            $c = Invoke-Cua @('close', '--window', ('pid:' + $t['pid']), '--confirm')
            $t['detail_lines'] += ('windowless cleanup: ' + $(if ($c.json) { $c.json.detail } else { 'nothing to close' }))
        }
        Write-Host ('  FAIL  ' + $Name + ': window never appeared') -ForegroundColor Red
        return [pscustomobject]$t
    }
    $hwnd = [string]$win.hwnd
    $t['hwnd'] = $hwnd; $t['title'] = [string]$win.title; $t['process'] = [string]$win.process
    if ($t['launched']) { $script:Opened.Add($hwnd) | Out-Null }
    if (-not $t['pid']) { $t['pid'] = [int]$win.pid }
    Write-Host ('  window hwnd=' + $hwnd + ' "' + $t['title'] + '" (reused=' + $t['reused'] + ')') -ForegroundColor DarkGray

    # 1-3: provider / locatability / actionability (retry: slow apps fill their tree late)
    $tree = Get-CuaState $hwnd -Patterns
    for ($k = 0; $k -lt 3; $k++) {
        if ($tree -and $tree.node_count -gt 0) { break }
        Start-Sleep -Milliseconds 1500
        $tree = Get-CuaState $hwnd -Patterns
    }
    if ($tree) { Add-Evidence $Evidence 'state --window ... --no-shot --patterns (provider tree)' $tree | Out-Null }
    $t['metrics'] = Get-TreeMetrics $tree
    if ($tree) {
        $t['detail_lines'] += ('node sample (i / type / name / class / patterns / rid):')
        $take = [Math]::Min(14, @($tree.nodes).Count)
        for ($i = 0; $i -lt $take; $i++) {
            $n = @($tree.nodes)[$i]
            $t['detail_lines'] += ('  ' + ('{0,-3} {1,-11} "{2}" [{3}] {4} {5}' -f $n.i, $n.type, (Get-NodeName $n), (Get-NodeClass $n), ((@($n.patterns) -join '+')), $(if (Test-HasRid $n) { 'rid' } else { 'NO-RID' })))
        }
    }

    # 4: safe-write probe (strict safety first, relaxed second)
    if ($tree) {
        $probe = Invoke-SafeWriteProbe -Hwnd $hwnd -TreeState $tree -AllowChrome $false -Evidence $Evidence
        if (-not $probe.attempted -or $probe.result -eq 'NO_CANDIDATE') {
            $p2 = Invoke-SafeWriteProbe -Hwnd $hwnd -TreeState $tree -AllowChrome $true -Evidence $Evidence
            if ($p2.attempted) { $probe = $p2; $t['detail_lines'] += 'probe candidate selected with the chrome filter relaxed' }
        }
        if ($probe.attempted) {
            $t['detail_lines'] += ('safe-write probe: ' + $probe.kind + ' on ' + $probe.node_desc + ' -> channel=' + $probe.channel + ' effect=' + $probe.effect + ' (' + $probe.effectNote + ') restored=' + $probe.restored)
        } else {
            $t['detail_lines'] += ('safe-write probe: none (' + $probe.detail + ')')
        }
    } else {
        $probe = [pscustomobject]@{ attempted = $false; result = 'NO_TREE'; channel = 'n/a'; kind = ''; node_desc = ''; detail = 'no readable UIA tree'; effect = $false; effectNote = ''; restored = ''; before = ''; after = '' }
    }
    $t['probe'] = $probe

    # 5: message-level probe
    $t['msg'] = Invoke-MessageProbe -Hwnd $hwnd -TreeState $tree -Evidence $Evidence

    # 6: capture
    $t['capture'] = Invoke-CaptureProbe -Hwnd $hwnd -Evidence $Evidence

    $t['verdict'] = Get-Verdict -m $t['metrics'] -probe $probe -msg $t['msg'] -cap $t['capture'] -NoWindow:$false

    # cleanup: only windows THIS run opened (never a window the user was already using)
    if ($KeepOpen) {
        $t['closed'] = 'left open (-KeepOpen)'
    } elseif ($t['launched']) {
        $c = Invoke-Cua @('close', '--window', $hwnd, '--confirm')
        Add-Evidence $Evidence 'close' $c | Out-Null
        $t['closed'] = $(if ($c.json) { [string]$c.json.detail } else { 'close unanswered' })
        if ($c.json -and [string]$c.json.status -ne 'ok') {
            $t['detail_lines'] += ('close failed: ' + $c.json.detail)
        } else {
            $script:Opened.Remove($hwnd) | Out-Null
        }
    } else {
        $t['closed'] = 'reused window left running (not ours to close)'
    }
    return [pscustomobject]$t
}

# ---------------------------------------------------------------------------
# output
# ---------------------------------------------------------------------------

function Format-SummaryRow($r) {
    $safe = if ($r.probe.attempted) { ('{0} {1} {2} via {3}' -f $r.probe.kind, $r.probe.result, $(if ($r.probe.effect) { 'effect' } else { 'no-effect' }), $r.probe.channel) } else { ('{0}: {1}' -f $r.probe.result, $r.probe.detail) }
    [pscustomobject]@{
        app        = $r.name
        verdict    = $r.verdict
        nodes      = $r.metrics.node_count
        named_pct  = $r.metrics.named_pct
        rid_pct    = $r.metrics.rid_pct
        patterns   = $r.metrics.hist_text
        safe_write = $safe
        msg_level  = $(if ($r.msg.attempted) { ('accepted={0} effect={1}' -f $r.msg.accepted, $r.msg.effect) } else { 'not attempted' })
        capture    = $(if ($r.capture) { ('rendered={0}' -f $r.capture.rendered) } else { 'n/a' })
    }
}

function Write-ConsoleTable($Rows) {
    $Rows | Format-Table -AutoSize app, verdict, nodes, named_pct, rid_pct, safe_write, msg_level, capture | Out-String -Width 4096 | Write-Host
    Write-Host ''
    foreach ($r in $Rows) {
        $color = 'Green'
        switch -Regex ($r.verdict) {
            'A11Y_PARTIAL|MSG_LEVEL_ONLY|READ_ONLY' { $color = 'Yellow' }
            'NO_PROVIDER|NEEDS_REAL_INPUT|LAUNCH_FAILED|CAPTURE_BLACK' { $color = 'Red' }
        }
        Write-Host ('  {0,-12} {1,-16} {2}' -f $r.app, $r.verdict, $r.patterns) -ForegroundColor $color
    }
}

function Esc([string]$s) {
    if ($null -eq $s) { return '' }
    return ($s -replace '\|', '\|' -replace "`r`n", ' ' -replace "`n", ' ')
}

function ConvertTo-Markdown($Results, [string]$Path) {
    $sb = New-Object System.Text.StringBuilder
    [void]$sb.AppendLine('# UIA capability health check')
    [void]$sb.AppendLine('')
    [void]$sb.AppendLine('- generated: ' + (Get-Date).ToString('yyyy-MM-dd HH:mm:ss'))
    [void]$sb.AppendLine('- cli: ' + $script:CuaExe)
    [void]$sb.AppendLine('- scratch shots: ' + $script:Scratch)
    [void]$sb.AppendLine('- verdict ladder: LAUNCH_FAILED > NO_PROVIDER > A11Y_FULL > A11Y_PARTIAL > MSG_LEVEL_ONLY > READ_ONLY > CAPTURE_BLACK > NEEDS_REAL_INPUT')
    [void]$sb.AppendLine('- A11Y_FULL means a UIA/LegacyIAccessible pattern action landed, the effect was read back, and the receipt showed NO cursor fallback - i.e. controllable without touching the user''s mouse.')
    [void]$sb.AppendLine('- Chromium/Electron targets need `--force-renderer-accessibility`, otherwise the web content tree is absent and the app is (correctly) reported NO_PROVIDER.')
    [void]$sb.AppendLine('- Effect verification reads the UIA tree (toggle state, value, selection, node_count, sig). A sig change is evidence, not proof: churning UIs (clocks, spinners, animated art) move the fingerprint by themselves, so the safe-write read-back is the strong signal and the message-level "effect" column is the weak one.')
    [void]$sb.AppendLine('- Windows this run opened were closed again; a window that was already running is reused and left alone.')
    [void]$sb.AppendLine('')
    [void]$sb.AppendLine('## Summary')
    [void]$sb.AppendLine('')
    [void]$sb.AppendLine('| app | verdict | node_count | named% | rid% | pattern histogram | safe-write probe | message-level | capture |')
    [void]$sb.AppendLine('|---|---|---|---|---|---|---|---|---|')
    foreach ($r in $Results) {
        $p = Format-SummaryRow $r
        $safe = if ($r.probe.attempted) { ('{0} - {1}' -f $p.safe_write, $r.probe.node_desc) } else { $p.safe_write }
        $msg = if ($r.msg.attempted) { $r.msg.summary } else { 'n/a' }
        $cap = if ($r.capture) { ('rendered={0} {1}x{2}' -f $r.capture.rendered, $r.capture.width, $r.capture.height) } else { 'n/a' }
        [void]$sb.AppendLine(('| {0} | **{1}** | {2} | {3} | {4} | {5} | {6} | {7} | {8} |' -f `
            (Esc $r.name), (Esc $r.verdict), $p.nodes, $p.named_pct, $p.rid_pct, (Esc $p.patterns), (Esc $safe), (Esc $msg), (Esc $cap)))
    }
    [void]$sb.AppendLine('')
    [void]$sb.AppendLine('## Per-app detail')
    foreach ($r in $Results) {
        [void]$sb.AppendLine('')
        [void]$sb.AppendLine('### ' + $r.name + ' - ' + $r.verdict)
        [void]$sb.AppendLine('')
        [void]$sb.AppendLine('- exe: ``' + $r.exe + '`` (installed: ' + $r.exists + ', extra args: ' + $(if ($r.extra) { $r.extra } else { 'none' }) + ')')
        [void]$sb.AppendLine('- window: hwnd=' + $r.hwnd + ' ``' + $r.title + '`` process=' + $r.process + ' pid=' + $r.pid + ' reused=' + $r.reused)
        [void]$sb.AppendLine('- provider: node_count=' + $r.metrics.node_count)
        [void]$sb.AppendLine('- locatability: named ' + $r.metrics.named + '/' + $r.metrics.node_count + ' (' + $r.metrics.named_pct + '%), runtime ids ' + $r.metrics.rid_nodes + '/' + $r.metrics.node_count + ' (' + $r.metrics.rid_pct + '%), clickable rects ' + $r.metrics.rect_nodes + '/' + $r.metrics.node_count + ' (' + $r.metrics.rect_pct + '%)')
        [void]$sb.AppendLine('- actionability: ' + $r.metrics.actionable + ' usable pattern elements (pattern+rid+enabled+on-screen), histogram: ' + $r.metrics.hist_text)
        $pr = $r.probe
        if ($pr.attempted) {
            [void]$sb.AppendLine('- safe-write probe: **' + $pr.result + '** ' + $pr.kind + ' on ' + $pr.node_desc)
            [void]$sb.AppendLine('  - channel: ' + $pr.channel + ' | effect observed: ' + $pr.effect + ' (' + $pr.effectNote + ') | restore: ' + $pr.restored)
            [void]$sb.AppendLine('  - receipt detail: ``' + ($pr.detail -replace '`', '') + '``')
            [void]$sb.AppendLine('  - state sig: ' + $pr.before + ' -> ' + $pr.after)
        } else {
            [void]$sb.AppendLine('- safe-write probe: not attempted (' + $pr.result + ': ' + $pr.detail + ')')
        }
        $m = $r.msg
        if ($m.attempted) {
            [void]$sb.AppendLine('- message-level: ' + $m.summary)
            [void]$sb.AppendLine('  - click: ``' + ($m.click_detail -replace '`', '') + '``')
            [void]$sb.AppendLine('  - type: ``' + ($m.type_detail -replace '`', '') + '``')
        } else {
            [void]$sb.AppendLine('- message-level: not attempted (' + $m.summary + ')')
        }
        if ($r.capture) {
            [void]$sb.AppendLine('- capture: ' + $r.capture.detail + ' -> ' + $r.capture.path)
        }
        [void]$sb.AppendLine('- cleanup: ' + $(if ($r.closed) { $r.closed } else { 'n/a' }))
        if (@($r.detail_lines).Count -gt 0) {
            [void]$sb.AppendLine('')
            [void]$sb.AppendLine('```text')
            foreach ($line in $r.detail_lines) { [void]$sb.AppendLine([string]$line) }
            [void]$sb.AppendLine('```')
        }
    }
    [void]$sb.AppendLine('')
    [void]$sb.AppendLine('## Raw receipts')
    [void]$sb.AppendLine('')
    foreach ($group in $EvidenceByTarget.Keys) {
        [void]$sb.AppendLine('### ' + $group)
        [void]$sb.AppendLine('')
        foreach ($e in $EvidenceByTarget[$group]) {
            [void]$sb.AppendLine('**' + $e.label + '**')
            [void]$sb.AppendLine('')
            [void]$sb.AppendLine('    $ ' + $e.argv)
            [void]$sb.AppendLine('')
            $body = [string]$e.raw
            $max = if ($e.label -like 'state*') { 1200 } else { 2400 }
            if ($body.Length -gt $max) { $body = $body.Substring(0, $max) + "`n... (truncated)" }
            if ($body.Trim().Length -eq 0) { $body = '(no output)' }
            [void]$sb.AppendLine('```json')
            [void]$sb.AppendLine($body)
            [void]$sb.AppendLine('```')
            [void]$sb.AppendLine('')
        }
    }
    Set-Content -LiteralPath $Path -Value $sb.ToString() -Encoding UTF8
}

# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------

if ($SelfTest) {
    $bad = Show-LadderSelfTest
    if ($bad -gt 0) { exit 1 }
    exit 0
}

if ($List) {
    Write-Host ('known targets (cua: ' + $script:CuaExe + ')') -ForegroundColor Cyan
    foreach ($k in $KnownTargets.Keys) {
        $d = $KnownTargets[$k]
        $mark = if (Test-ExePath $d.exe) { 'present' } else { 'MISSING' }
        Write-Host ('  {0,-11} {1,-9} args={2,-32} {3}' -f $k, $mark, $(if ($d.args) { $d.args } else { '-' }), $d.exe) -ForegroundColor $(if ($mark -eq 'present') { 'Gray' } else { 'DarkYellow' })
        Write-Host ('              ' + $d.note) -ForegroundColor DarkGray
    }
    exit 0
}

Write-Host ('== cua UIA capability health check (' + $script:CuaExe + ')') -ForegroundColor Cyan
$Targets = Resolve-Targets $Apps
Merge-ExtraArgs -Targets $Targets -ExtraArgs $ExtraArgs
if ($Targets.Count -eq 0) {
    Write-Host 'nothing selected - see -List' -ForegroundColor Red
    exit 1
}

$EvidenceByTarget = [ordered]@{}
$Results = @()
try {
    foreach ($name in $Targets.Keys) {
        $ev = New-Object System.Collections.ArrayList
        $EvidenceByTarget[$name] = $ev
        Write-Host ('-- ' + $name + ' (' + $Targets[$name].exe + ')') -ForegroundColor White
        $res = Run-Target -Name $name -Def $Targets[$name] -Evidence $ev
        $Results += $res
        Write-Host ('   verdict: ' + $res.verdict) -ForegroundColor Cyan
    }
}
finally {
    # close anything this run opened - and never a window it merely reused
    foreach ($hwnd in @($script:Opened)) {
        if (([string]$hwnd).Length -eq 0) { continue }
        Write-Host ('final cleanup: closing hwnd ' + $hwnd) -ForegroundColor DarkYellow
        Invoke-Cua @('close', '--window', ([string]$hwnd), '--confirm') | Out-Null
    }
    $script:Opened.Clear() | Out-Null
}

$rows = @($Results | ForEach-Object { Format-SummaryRow $_ })
Write-Host ''
Write-Host '== summary' -ForegroundColor Cyan
Write-ConsoleTable $rows

ConvertTo-Markdown -Results $Results -Path $OutFile
Write-Host ''
Write-Host ('report written: ' + $OutFile) -ForegroundColor Green
exit 0
