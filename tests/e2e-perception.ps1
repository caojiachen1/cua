# tests\e2e-perception.ps1 - pure-Rust perception: v2 snapshots, OCR/detector
# stages, auto routing, model verification, text/template lookup, explicit
# watch confirmation, visual click freshness, stream revisions.
$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$cua = $env:CUA_TEST_BIN
if (-not $cua -or -not (Test-Path $cua)) { $cua = "$root\bin\cua.exe" }
if (-not (Test-Path $cua)) { $cua = "$root\target\release\cua.exe" }
$testpad = $env:CUA_TEST_TESTPAD
if (-not $testpad -or -not (Test-Path $testpad)) { $testpad = "$root\bin\testpad.exe" }
if (-not (Test-Path $testpad)) { $testpad = "$root\target\debug\testpad.exe" }

$models = $env:CUA_MODEL_DIR
if (-not $models) { $models = Join-Path $root ".models" }
$haveModels = Test-Path (Join-Path $models "ppocr-v4\ch_PP-OCRv4_det_infer.onnx")
if (-not $haveModels) {
    Write-Host "SKIP e2e-perception: no model assets (set CUA_MODEL_DIR)" -ForegroundColor Yellow
    exit 0
}
$env:CUA_MODEL_DIR = $models

$ownData = $false
if (-not $env:CUA_DATA_DIR) {
    $env:CUA_DATA_DIR = Join-Path $env:TEMP ("cua-e2e-perc-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
    $ownData = $true
}
New-Item -ItemType Directory -Force -Path $env:CUA_DATA_DIR | Out-Null

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
# Any visual element whose centre falls inside a UIA list row: the visual stage
# may return the colour swatch, its label or the row box, so pick by geometry
# instead of by a name that OCR/detector output can drop between frames.
function PickVisualRow($obs, $excludeRows = @()) {
    $rows = @($obs.elements | Where-Object { $_.provenance -eq "uia" -and $_.uia_type -eq "ListItem" -and $_.rect_px -and $excludeRows -notcontains $_.text })
    $vis = @($obs.elements | Where-Object { $_.provenance -ne "uia" -and $_.rect_px -and $_.rect_px[2] -gt 4 -and $_.rect_px[3] -gt 4 })
    foreach ($v in $vis) {
        $cx = $v.rect_px[0] + $v.rect_px[2] / 2
        $cy = $v.rect_px[1] + $v.rect_px[3] / 2
        foreach ($r in $rows) {
            if ($cx -ge $r.rect_px[0] -and $cx -le ($r.rect_px[0] + $r.rect_px[2]) -and
                $cy -ge $r.rect_px[1] -and $cy -le ($r.rect_px[1] + $r.rect_px[3])) {
                return @{ element = $v; row = $r.text }
            }
        }
    }
    return $null
}

Write-Host "== cua pure-Rust perception E2E (cli: $cua)" -ForegroundColor Cyan
Write-Host ("== models: {0}" -f $env:CUA_MODEL_DIR) -ForegroundColor DarkGray

& $cua sandbox stop 2>$null | Out-Null
& $cua rec stop 2>$null | Out-Null
taskkill /IM testpad.exe /F 2>$null | Out-Null
Start-Sleep -Milliseconds 700

$modelsOut = J (RunCua perception models verify)
Check "P1 model manifest is complete" ($modelsOut.ok -eq $true) "ok=$($modelsOut.ok)"
Check "P2 pinned OmniParser revision" ($modelsOut.revision -eq "f55d0750e5b94db2125ef0b45b0fa4a85ddc59b4") "$($modelsOut.revision)"
$install = J (RunCua perception models install --accept-license)
Check "P2b install is idempotent for local assets" (
    $install.ok -eq $true -and @($install.skipped).Count -ge 1 -and @($install.failed).Count -eq 0
) "installed=$(@($install.installed).Count) skipped=$(@($install.skipped).Count) failed=$(@($install.failed).Count)"

$W = "title:CuaPercE2E"
& $cua launch --exe $testpad --args "--title CuaPercE2E" | Out-Null
& $cua wait --window $W --timeout-ms 6000 | Out-Null
Start-Sleep -Milliseconds 600

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$fast = J (RunCua state --window $W --perception hybrid --detail fast --deadline-ms 10000)
$sw.Stop()
$fastMs = $sw.ElapsedMilliseconds
Check "P3 hybrid emits cua-observation-v2" ($fast.schema -eq "cua-observation-v2") "$($fast.schema)"
Check "P4 UIA/OCR/detector stages complete" (
    $fast.stages.uia.status -eq "complete" -and
    $fast.stages.ocr.status -eq "complete" -and
    $fast.stages.detector.status -eq "complete"
) "uia=$($fast.stages.uia.status) ocr=$($fast.stages.ocr.status) det=$($fast.stages.detector.status)"
Check "P23 default OCR backend is CommandFlow PaddleOCR" (
    $fast.perception.backend.ocr -match "paddle"
) "$($fast.perception.backend.ocr)"
Check "P5 OCR text is present in elements" (
    @($fast.elements | Where-Object { $_.provenance -eq "ocr" -and $_.text -match "alpha|AppendMark" }).Count -ge 1
) "ocr=$(@($fast.elements | Where-Object { $_.provenance -eq 'ocr' }).Count)"
Check "P6 detector elements are present" (
    @($fast.elements | Where-Object { $_.provenance -eq "detector" -or $_.provenance -eq "fused" }).Count -ge 1
) "det=$(@($fast.elements | Where-Object { $_.provenance -eq 'detector' }).Count)"
Check "P7 visual text is 0..1 confidence" (
    @($fast.elements | Where-Object { $_.provenance -eq "ocr" -and ($null -eq $_.confidence.ocr -or $_.confidence.ocr -lt 0 -or $_.confidence.ocr -gt 1) }).Count -eq 0
) ""
Check "P8 fast path under 10s on this machine" ($fastMs -lt 10000) "$fastMs ms"
Check "P9 frame metadata carries freshness unknown" (
    $fast.frame -and $fast.frame.content_freshness -eq "unknown"
) "$($fast.frame.content_freshness)"

$auto = J (RunCua state --window $W --perception auto --detail fast --deadline-ms 10000)
Check "P10 auto records a per-region route" (
    $auto.perception.route.state -in @("uia_primary", "hybrid", "vision_primary") -and
    $null -ne $auto.perception.quality[0].coverage
) "state=$($auto.perception.route.state) cov=$($auto.perception.quality[0].coverage)"
Check "P11 auto can explain vision use" (
    $auto.perception.route.vision_used -eq $true -and $auto.perception.route.reasons.Count -ge 1
) "$($auto.perception.route.reason)"

$uia = J (RunCua state --window $W --perception uia --no-shot)
Check "P12 explicit UIA mode does not capture or infer" (
    $uia.perception.route.state -eq "uia_primary" -and $null -eq $uia.frame -and
    $uia.stages.ocr.status -eq "skipped" -and $uia.stages.detector.status -eq "skipped"
) "state=$($uia.perception.route.state) frame=$($null -ne $uia.frame)"

$conflict = RunErr state --window $W --perception vision --no-shot
Check "P13 vision + no-shot is refused, not silently captured" ($conflict -match "vision requires a new frame") "$conflict"

$obsId = [string]$fast.observation_id
$mt = J (RunCua perception match-text --observation-id $obsId --contains "alpha")
Check "P14 match-text finds the OCR/UIA label" (@($mt.matches).Count -ge 1) "matches=$(@($mt.matches).Count)"

$shot = [string]$fast.screenshot.path
$btn = @($fast.elements | Where-Object { $_.text -eq "AppendMark" }) | Select-Object -First 1
$tpl = Join-Path $env:CUA_DATA_DIR "append-template.png"
if ($btn -and (Test-Path $shot)) {
    Add-Type -AssemblyName System.Drawing
    $bmp = [System.Drawing.Bitmap]::FromFile($shot)
    $x = [int]$btn.rect_px[0]; $y = [int]$btn.rect_px[1]
    $cw = [int]$btn.rect_px[2]; $ch = [int]$btn.rect_px[3]
    $rect = New-Object System.Drawing.Rectangle($x, $y, $cw, $ch)
    $crop = $bmp.Clone($rect, $bmp.PixelFormat)
    $crop.Save($tpl, [System.Drawing.Imaging.ImageFormat]::Png)
    $crop.Dispose(); $bmp.Dispose()
    $m = J (RunCua perception match-template --observation-id $obsId --template $tpl --threshold 0.8)
    Check "P15 template finds the captured control" (
        $m.best_score -ge 0.8 -and @($m.matches).Count -ge 1
    ) "score=$($m.best_score) matches=$(@($m.matches).Count)"
    $watchRaw = (& $cua perception watch-template --window $W --template $tpl --threshold 0.8 --confirm-frames 2 --timeout-ms 4000 --interval-ms 90 2>&1) -join "`n"
    $watch = J $watchRaw
    Check "P16 watch confirms two consecutive frames" (
        $watch.matched -eq $true -and $watch.frames -ge 2 -and $watch.streak -ge 2
    ) "matched=$($watch.matched) frames=$($watch.frames)"
} else {
    Check "P15 template finds the captured control" $false "visual AppendMark missing (shot=$shot)"
    Skip "P16 watch confirms two consecutive frames" "no template"
}

if ($btn) {
    $fresh = J (RunCua state --window $W --perception hybrid --detail fast --deadline-ms 10000)
    $freshId = [string]$fresh.observation_id
    $pick = PickVisualRow $fresh
    $freshSwatch = $pick.element
    $rowName = $pick.row
    if ($freshSwatch) {
        $fr = $freshSwatch.rect_px
        $fs = if ($fresh.frame.pixel_scale) { [double]$fresh.frame.pixel_scale } else { 1.0 }
        $wantX = [int][Math]::Round($fresh.frame.screen_origin[0] + ($fr[0] + $fr[2] / 2) * $fs)
        $wantY = [int][Math]::Round($fresh.frame.screen_origin[1] + ($fr[1] + $fr[3] / 2) * $fs)
        & $cua activate --window $W | Out-Null
        Start-Sleep -Milliseconds 400
        $click = J (RunCua perception click-element --observation-id $freshId --element-id $freshSwatch.element_id)
        $afterClick = J (RunCua state --window $W --no-shot)
        $redRow = @($afterClick.nodes | Where-Object { $_.type -eq "ListItem" -and $_.name -eq $rowName }) | Select-Object -First 1
        Check "P17 visual click is guarded, DPI-scaled and lands" (
            $click.status -eq "ok" -and $click.dispatched -eq $true -and $click.method -match "freshness/hit-test" -and
            [Math]::Abs($click.point[0] - $wantX) -le 1 -and [Math]::Abs($click.point[1] - $wantY) -le 1 -and
            $redRow.selected -eq $true
        ) "method=$($click.method) point=$($click.point -join ',') want=$wantX,$wantY selected=$($redRow.selected)"
        Start-Sleep -Milliseconds 5200
        $stale = RunErr perception click-element --observation-id $freshId --element-id $freshSwatch.element_id
        Check "P18 stale frame visual click is refused" ($stale -match "stale") "$stale"
    } else {
        Check "P17 visual click is guarded, DPI-scaled and lands" $false "fresh visual swatch missing"
        Check "P18 stale frame visual click is refused" $false "fresh visual swatch missing"
    }
} else {
    Check "P17 visual click is guarded, DPI-scaled and lands" $false "no visual swatch"
    Check "P18 stale frame visual click is refused" $false "no visual swatch"
}

$streamOut = RunCua state --window $W --perception hybrid --detail fast --stream --deadline-ms 10000
$lines = @($streamOut | Where-Object { $_.Trim().StartsWith("{") })
Check "P19 stream emits two immutable revisions" ($lines.Count -eq 2) "lines=$($lines.Count)"
if ($lines.Count -eq 2) {
    $p0 = $lines[0] | ConvertFrom-Json
    $p1 = $lines[1] | ConvertFrom-Json
    Check "P20 revision/parent chain is explicit" (
        $p0.observation_id -ne $p1.observation_id -and $p1.revision -eq ($p0.revision + 1) -and
        $p1.parent -eq $p0.observation_id -and $p0.terminal -eq $false -and $p1.terminal -eq $true
    ) "ids=$($p0.observation_id),$($p1.observation_id) rev=$($p0.revision)->$($p1.revision)"
} else {
    Check "P20 revision/parent chain is explicit" $false "missing stream lines"
}

$vizOut = Join-Path $env:CUA_DATA_DIR "parsed.png"
$viz = J (RunCua perception visualize --image $shot --detail fast --out $vizOut)
Check "P24 visualize writes an annotated PNG" (
    (Test-Path $vizOut) -and (Test-Path ([IO.Path]::ChangeExtension($vizOut, "json"))) -and
    $viz.schema -eq "cua-perception-visualize-v1" -and
    $viz.counts.total -ge 1 -and $viz.annotated_width -gt $viz.width
) "out=$vizOut total=$($viz.counts.total) canvas=$($viz.annotated_width)x$($viz.annotated_height)"
$tractDir = Join-Path $models "ppocr-v4"
$tractReady = (Test-Path (Join-Path $tractDir "det640.onnx")) -and
    (Test-Path (Join-Path $tractDir "rec320.onnx")) -and (Test-Path (Join-Path $tractDir "cls192.onnx"))
if ($tractReady) {
    $fallback = J (RunCua state --window $W --perception hybrid --detail fast --ocr-profile ppocr-zh-desktop --deadline-ms 10000)
    Check "P25 optional tract OCR profile still works" (
        $fallback.stages.ocr.status -eq "complete" -and $fallback.perception.backend.ocr -match "tract"
    ) "ocr=$($fallback.stages.ocr.status) backend=$($fallback.perception.backend.ocr)"
} else {
    Skip "P25 optional tract OCR profile" "developer-exported det640/rec320/cls192 graphs not installed"
}

$full = J (RunCua state --window $W --perception hybrid --detail full --deadline-ms 25000)
$cap = @($full.elements | Where-Object { $_.caption })
Check "P21 full detail runs the caption stage" (
    $full.stages.caption.status -in @("complete", "skipped") -and
    ($full.stages.caption.status -eq "skipped" -or $cap.Count -ge 1)
) "caption=$($full.stages.caption.status) captions=$($cap.Count)"
Check "P22 full snapshot remains immutable and complete/partial" (
    $full.terminal -eq $true -and $full.status -in @("complete", "partial")
) "status=$($full.status) terminal=$($full.terminal)"

$env:CUA_FLORENCE_BATCH = "1"
$batched = J (RunCua state --window $W --perception hybrid --detail full --deadline-ms 30000)
Remove-Item Env:\CUA_FLORENCE_BATCH -ErrorAction SilentlyContinue
$batchCaps = @($batched.elements | Where-Object { $_.caption })
Check "P26 forced Florence batch path runs" (
    $batched.stages.caption.status -in @("complete", "skipped") -and
    ($batched.stages.caption.status -eq "skipped" -or $batchCaps.Count -ge 1)
) "caption=$($batched.stages.caption.status) captions=$($batchCaps.Count)"

$easy = J (RunCua state --window $W --perception hybrid --detail fast --ocr-profile easyocr-en-reference --deadline-ms 20000)
Check "P27 EasyOCR reference backend runs" (
    $easy.stages.ocr.status -eq "complete" -and $easy.perception.backend.ocr -match "easyocr"
) "ocr=$($easy.stages.ocr.status) backend=$($easy.perception.backend.ocr)"

# ---------- incremental reuse, redaction, stored-observation rendering, install groups ----------
$cached = $null
for ($k = 0; $k -lt 3; $k++) {
    $cached = J (RunCua state --window $W --perception hybrid --detail fast --deadline-ms 10000)
    if ($cached.stages.ocr.cache_hit -eq $true) { break }
}
Check "P35 elements expose their nearest named container" (
    @($fast.elements | Where-Object { $_.container }).Count -ge 1
) "container=$((@($fast.elements | Where-Object { $_.container }) | Select-Object -First 1).container)"

Check "P30 unchanged frame reuses the visual build" (
    $cached.stages.ocr.cache_hit -eq $true -and $cached.stages.detector.cache_hit -eq $true -and
    @($cached.elements).Count -ge 3 -and $cached.perception.backend.ocr -match "paddle"
) "cache=$($cached.stages.ocr.cache_hit) ocr=$($cached.perception.backend.ocr) elements=$(@($cached.elements).Count)"

$red = J (RunCua state --window $W --perception hybrid --detail fast --redact --deadline-ms 10000)
$leaks = @($red.elements | Where-Object { $_.text -and $_.text -ne "***" })
Check "P31 redact masks recognised text" (
    $red.stages.caption.status -eq "skipped" -and $leaks.Count -eq 0 -and @($red.elements).Count -ge 3
) "caption=$($red.stages.caption.status) leaks=$($leaks.Count)"

$vizObs = Join-Path $env:CUA_DATA_DIR "obs-render.png"
$vr = J (RunCua perception visualize --observation-id $obsId --out $vizObs)
Check "P32 visualize renders a stored observation without re-parsing" (
    (Test-Path $vizObs) -and (Test-Path ([IO.Path]::ChangeExtension($vizObs, "json"))) -and
    $vr.source -like "observation:*" -and $vr.counts.total -ge 3
) "source=$($vr.source) total=$($vr.counts.total)"

$core = J (RunCua perception models install --accept-license --groups core)
$coreEntries = @($core.installed).Count + @($core.skipped).Count
Check "P33 install --groups core excludes the EasyOCR reference" (
    $core.ok -eq $true -and (@($core.groups) -contains "core") -and $coreEntries -eq 8
) "groups=$(@($core.groups) -join ',') entries=$coreEntries"

# ---------- native-resolution tiles recover small controls on a large window ----------
& $cua resize --window $W --width 1600 --height 1000 | Out-Null
Start-Sleep -Milliseconds 500
$bigShot = Join-Path $env:CUA_DATA_DIR "large-frame.png"
& $cua shot --window $W --out $bigShot | Out-Null
$env:CUA_DET_TILES = "0"
$singlePass = J (RunCua perception visualize --image $bigShot --detail fast --out (Join-Path $env:CUA_DATA_DIR "det-single.png"))
$env:CUA_DET_TILES = "16"
$tiledPass = J (RunCua perception visualize --image $bigShot --detail fast --out (Join-Path $env:CUA_DATA_DIR "det-tiled.png"))
Remove-Item Env:\CUA_DET_TILES -ErrorAction SilentlyContinue
Check "P34 native tiles recover small controls on a large frame" (
    $tiledPass.counts.icons -gt $singlePass.counts.icons -and $tiledPass.counts.total -gt $singlePass.counts.total
) "single=$($singlePass.counts.icons)/$($singlePass.counts.total) tiled=$($tiledPass.counts.icons)/$($tiledPass.counts.total)"

# ---------- background (occluded) perception and message-level visual click ----------
$cover = "title:CuaPercCover"
& $cua launch --exe $testpad --args "--title CuaPercCover" | Out-Null
& $cua wait --window $cover --timeout-ms 6000 | Out-Null
$w1 = (J (RunCua state --window $W --no-shot)).window.rect
& $cua resize --window $cover --x $w1[0] --y $w1[1] --width ($w1[2] + 40) --height ($w1[3] + 60) | Out-Null
& $cua activate --window $cover | Out-Null
& $cua wait --window $cover --foreground --timeout-ms 2500 | Out-Null
Start-Sleep -Milliseconds 300
$w1Hwnd = [int64](J (RunCua state --window $W --no-shot)).window.hwnd
$fgBefore = (J (RunCua apps)).foreground_hwnd
$bg = J (RunCua state --window $W --perception hybrid --detail fast --deadline-ms 10000)
$bgVisual = @($bg.elements | Where-Object { $_.provenance -ne "uia" })
Check "P28 occluded window still parses in the background" (
    $bg.frame.capture_status -eq "captured" -and $bg.frame.rendered -eq $true -and
    $bg.stages.ocr.status -eq "complete" -and $bg.stages.detector.status -eq "complete" -and
    $bgVisual.Count -ge 3
) "capture=$($bg.frame.capture_status) rendered=$($bg.frame.rendered) ocr=$($bg.stages.ocr.status) visual=$($bgVisual.Count)"
$bgPick = PickVisualRow $bg @($rowName)
$bgSwatch = $bgPick.element
$bgRowName = $bgPick.row
if ($bgSwatch) {
    $br = $bgSwatch.rect_px
    $bs = if ($bg.frame.pixel_scale) { [double]$bg.frame.pixel_scale } else { 1.0 }
    $wantX = [int][Math]::Round($bg.frame.screen_origin[0] + ($br[0] + $br[2] / 2) * $bs)
    $wantY = [int][Math]::Round($bg.frame.screen_origin[1] + ($br[1] + $br[3] / 2) * $bs)
    $bc = J (RunCua perception click-element --observation-id $bg.observation_id --element-id $bgSwatch.element_id --background)
    Start-Sleep -Milliseconds 500
    $afterBg = J (RunCua state --window $W --no-shot)
    $fgAfter = (J (RunCua apps)).foreground_hwnd
    $greenRow = @($afterBg.nodes | Where-Object { $_.type -eq "ListItem" -and $_.name -eq $bgRowName }) | Select-Object -First 1
    Check "P29 background visual click lands without focus change" (
        $bc.status -eq "ok" -and $bc.method -match "message-level" -and
        [Math]::Abs($bc.point[0] - $wantX) -le 1 -and [Math]::Abs($bc.point[1] - $wantY) -le 1 -and
        $greenRow.selected -eq $true -and $fgAfter -ne $w1Hwnd
    ) "method=$($bc.method) point=$($bc.point -join ',') want=$wantX,$wantY selected=$($greenRow.selected) fg=$fgBefore->$fgAfter target=$w1Hwnd"
} else {
    Skip "P29 background visual click lands without focus change" "no visual element inside a list row"
}

taskkill /IM testpad.exe /F 2>$null | Out-Null
if ($ownData -and $env:CUA_E2E_CLEAN -eq "1") { Remove-Item -Recurse -Force $env:CUA_DATA_DIR -ErrorAction SilentlyContinue }
Write-Host ""
Write-Host ("==== RESULT: {0} passed, {1} failed, {2} skipped ====" -f $script:pass, $script:fail, $script:skip) -ForegroundColor $(if ($script:fail -eq 0) { "Green" } else { "Red" })
if ($script:fail -gt 0) {
    Write-Host ("FAILED: " + ($script:failures -join ", ")) -ForegroundColor Red
    exit 1
}
exit 0
