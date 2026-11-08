# tools\bench-perception.ps1 - repeatable recall/latency check for the OCR+detector stack.
# Usage:
#   powershell -File tools\bench-perception.ps1                 # built-in testpad fixture
#   powershell -File tools\bench-perception.ps1 -Manifest set.json -Runs 5
# Manifest format: [{ "image": "shot.png", "expect": ["Save", "Cancel"], "min_recall": 0.8 }]
param(
    [string]$Manifest,
    [int]$Runs = 3,
    [double]$MinRecall = 0.8
)
$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$cua = $env:CUA_TEST_BIN
if (-not $cua -or -not (Test-Path $cua)) { $cua = "$root\target\release\cua.exe" }
if (-not (Test-Path $cua)) { $cua = "$root\bin\cua.exe" }
if (-not (Test-Path $cua)) { throw "no cua.exe - run 'cargo build --release' first" }
if ($env:CUA_MODEL_DIR) { $null = $env:CUA_MODEL_DIR } elseif (Test-Path "$root\.models") { $env:CUA_MODEL_DIR = "$root\.models" }

function J($out) {
    $s = ($out -join "`n")
    $b = $s.IndexOf("["); $c = $s.IndexOf("{")
    if ($b -ge 0 -and ($c -lt 0 -or $b -lt $c)) { $s = $s.Substring($b) }
    elseif ($c -ge 0) { $s = $s.Substring($c) }
    $s | ConvertFrom-Json
}
function RunCua { & $cua @args 2>$null }

$work = Join-Path $env:TEMP ("cua-bench-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
New-Item -ItemType Directory -Force -Path $work | Out-Null
$cases = @()

if ($Manifest) {
    if (-not (Test-Path $Manifest)) { throw "manifest not found: $Manifest" }
    $parsed = Get-Content $Manifest -Raw | ConvertFrom-Json
    # PS 5.1 hands a JSON array back as one object; unwrap it explicitly.
    $cases = if ($parsed -is [System.Array]) { $parsed } else { @($parsed) }
} else {
    $testpad = $env:CUA_TEST_TESTPAD
    if (-not $testpad -or -not (Test-Path $testpad)) { $testpad = "$root\target\release\testpad.exe" }
    if (-not (Test-Path $testpad)) { $testpad = "$root\bin\testpad.exe" }
    if (-not (Test-Path $testpad)) { throw "testpad.exe not found; pass -Manifest instead" }
    & $cua launch --exe $testpad --args "--title CuaBench" | Out-Null
    & $cua wait --window "title:CuaBench" --timeout-ms 8000 | Out-Null
    Start-Sleep -Milliseconds 800
    $shot = Join-Path $work "fixture.png"
    $r = J (RunCua shot --window "title:CuaBench" --out $shot)
    if (-not (Test-Path $shot)) { throw "fixture capture failed: $($r.status)" }
    $cases = @([pscustomobject]@{
        image = $shot
        expect = @("AppendMark", "AutoMark", "alpha", "beta", "gamma", "red", "green", "blue", "OCR TEST 123")
        min_recall = $MinRecall
    })
}

$results = @()
foreach ($case in $cases) {
    $image = $case.image
    if (-not [System.IO.Path]::IsPathRooted($image)) { $image = Join-Path (Split-Path -Parent $Manifest) $image }
    $expect = @($case.expect)
    $min = if ($case.min_recall) { [double]$case.min_recall } else { $MinRecall }
    $times = @()
    $found = @()
    $counts = $null
    for ($i = 0; $i -lt [Math]::Max(1, $Runs); $i++) {
        $out = Join-Path $work ("viz-" + [IO.Path]::GetFileNameWithoutExtension($image) + "-$i.png")
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $v = J (RunCua perception visualize --image $image --detail fast --out $out)
        $sw.Stop()
        $times += $sw.ElapsedMilliseconds
        $found = @($v.elements | ForEach-Object { $_.text } | Where-Object { $_ })
        $counts = $v.counts
    }
    $hits = @($expect | Where-Object { $e = $_; $found | Where-Object { $_.ToLower().Contains($e.ToLower()) } })
    $missing = @($expect | Where-Object { $e = $_; -not ($found | Where-Object { $_.ToLower().Contains($e.ToLower()) }) })
    $recall = if ($expect.Count -eq 0) { 1.0 } else { $hits.Count / $expect.Count }
    $sorted = @($times | Sort-Object)
    $results += [pscustomobject]@{
        image      = $image
        recall     = [Math]::Round($recall, 3)
        expected   = $expect.Count
        missing    = $missing
        elements   = $counts.total
        ocr        = $counts.ocr
        icons      = $counts.icons
        p50_ms     = $sorted[[int][Math]::Floor($sorted.Count / 2)]
        max_ms     = $sorted[-1]
        ok         = ($recall -ge $min)
    }
}

& $cua close --window "title:CuaBench" --confirm 2>$null | Out-Null
$summary = [pscustomobject]@{
    schema   = "cua-perception-bench-v1"
    binary   = $cua
    runs     = $Runs
    ok       = (@($results | Where-Object { -not $_.ok }).Count -eq 0)
    results  = $results
}
$summary | ConvertTo-Json -Depth 6
if (-not $summary.ok) { exit 1 }
exit 0
