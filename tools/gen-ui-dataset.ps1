# tools\gen-ui-dataset.ps1 - reproducible synthetic UI screenshots for the
# OCR/detector benchmark. Uses only System.Drawing; never captures the real
# desktop. Writes 6 PNGs (1024x640) plus manifest.json for
# tools\bench-perception.ps1.
#
# Usage:
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\gen-ui-dataset.ps1
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\gen-ui-dataset.ps1 -OutDir D:\tmp\ui-set
param(
    [string]$OutDir
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

if ([string]::IsNullOrWhiteSpace($OutDir)) {
    $OutDir = Join-Path $env:TEMP ("cua-ui-dataset-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
}
$OutDir = [System.IO.Path]::GetFullPath($OutDir)
if (-not (Test-Path $OutDir)) {
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
}

# Font families are resolved once; the first installed candidate wins.
$installed = @{}
(New-Object System.Drawing.Text.InstalledFontCollection).Families |
    ForEach-Object { $installed[$_.Name] = $true }
function Resolve-Family([string[]]$candidates) {
    foreach ($name in $candidates) {
        if ($script:installed.ContainsKey($name)) { return $name }
    }
    return "Arial"
}
$fontEn = Resolve-Family @("Segoe UI", "Microsoft YaHei UI", "Arial")
$fontZh = Resolve-Family @("Microsoft YaHei UI", "Microsoft YaHei", "SimSun", "Segoe UI")

function New-UiFont([string]$family, [int]$px, [bool]$bold) {
    $style = [System.Drawing.FontStyle]::Regular
    if ($bold) { $style = [System.Drawing.FontStyle]::Bold }
    return New-Object System.Drawing.Font($family, [single]$px, $style, [System.Drawing.GraphicsUnit]::Pixel)
}
function New-UiBrush([string]$hex) {
    return New-Object System.Drawing.SolidBrush ([System.Drawing.ColorTranslator]::FromHtml($hex))
}

function New-UiImage {
    param(
        [string]$Path,
        [string]$Theme,   # light | dark
        [string]$Lang,    # en | zh
        [int]$Px          # 11 | 16
    )

    $dark = ($Theme -eq "dark")
    $zh = ($Lang -eq "zh")

    if ($zh) {
        $title = if ($dark) { "设置 — 深色" } else { "设置 — 浅色" }
        $nav = @("常规设置", "外观主题", "高级选项")
        $labels = @("显示名称", "电子邮件地址")
        $buttons = @("保存更改", "恢复默认")
        $list = @("自动保存已开启", "跨设备同步已开启", "显示隐藏文件")
        $status = "准备就绪"
        $family = $fontZh
    } else {
        $title = if ($dark) { "Settings — Dark" } else { "Settings — Light" }
        $nav = @("General", "Appearance", "Advanced")
        $labels = @("Display name", "Email address")
        $buttons = @("Save changes", "Cancel")
        $list = @("Autosave every 5 minutes", "Sync across devices", "Show hidden files")
        $status = "Ready"
        $family = $fontEn
    }

    if ($dark) {
        $cBg = "#17191C"; $cTitleBar = "#23262B"; $cTitleText = "#F3F4F6"
        $cNav = "#1D2024"; $cNavText = "#E5E7EB"; $cSelBg = "#2A3646"; $cAccent = "#60A5FA"
        $cPanel = "#24282E"; $cBorder = "#4B5563"; $cLabel = "#D1D5DB"
        $cRowA = "#1D2024"; $cRowB = "#21252A"; $cRowBorder = "#343A42"; $cText = "#F3F4F6"
        $cBtn2 = "#2B3138"; $cBtnBorder = "#566070"; $cBtn2Text = "#E5E7EB"
        $cStatus = "#23262B"; $cStatusText = "#D1D5DB"
    } else {
        $cBg = "#F5F6F8"; $cTitleBar = "#E9EDF2"; $cTitleText = "#1F2937"
        $cNav = "#FFFFFF"; $cNavText = "#111827"; $cSelBg = "#E3EEFB"; $cAccent = "#2563EB"
        $cPanel = "#FFFFFF"; $cBorder = "#B7C0CC"; $cLabel = "#374151"
        $cRowA = "#FFFFFF"; $cRowB = "#F9FAFB"; $cRowBorder = "#DDE2E8"; $cText = "#111827"
        $cBtn2 = "#FFFFFF"; $cBtnBorder = "#B7C0CC"; $cBtn2Text = "#111827"
        $cStatus = "#E9EDF2"; $cStatusText = "#374151"
    }

    $bmp = New-Object System.Drawing.Bitmap 1024, 640, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::ClearTypeGridFit

    $brBg = New-UiBrush $cBg
    $brTitleBar = New-UiBrush $cTitleBar
    $brNav = New-UiBrush $cNav
    $brSel = New-UiBrush $cSelBg
    $brAccent = New-UiBrush $cAccent
    $brPanel = New-UiBrush $cPanel
    $brRowA = New-UiBrush $cRowA
    $brRowB = New-UiBrush $cRowB
    $brBtn1 = New-UiBrush "#2563EB"
    $brBtn2 = New-UiBrush $cBtn2
    $brStatus = New-UiBrush $cStatus
    $brTitleText = New-UiBrush $cTitleText
    $brNavText = New-UiBrush $cNavText
    $brLabel = New-UiBrush $cLabel
    $brText = New-UiBrush $cText
    $brBtn2Text = New-UiBrush $cBtn2Text
    $brStatusText = New-UiBrush $cStatusText
    $brWhite = New-UiBrush "#FFFFFF"
    $penBorder = New-Object System.Drawing.Pen ([System.Drawing.ColorTranslator]::FromHtml($cBorder)), ([single]1)
    $penRow = New-Object System.Drawing.Pen ([System.Drawing.ColorTranslator]::FromHtml($cRowBorder)), ([single]1)
    $penBtn2 = New-Object System.Drawing.Pen ([System.Drawing.ColorTranslator]::FromHtml($cBtnBorder)), ([single]1)

    # Background, title bar, nav panel, status bar.
    $g.FillRectangle($brBg, 0, 0, 1024, 640)
    $g.FillRectangle($brTitleBar, 0, 0, 1024, 56)
    $g.FillRectangle($brNav, 0, 56, 240, 540)
    $g.FillRectangle($brStatus, 0, 596, 1024, 44)
    $g.DrawLine($penBorder, 240, 56, 240, 596)
    $g.DrawLine($penBorder, 0, 56, 1024, 56)
    $g.DrawLine($penBorder, 0, 596, 1024, 596)

    # Window controls: plain ellipses (no glyphs, so no unlisted text).
    $dot = New-Object System.Drawing.SolidBrush ([System.Drawing.ColorTranslator]::FromHtml($cBorder))
    $g.FillEllipse($dot, 962, 24, 12, 12)
    $g.FillEllipse($dot, 982, 24, 12, 12)
    $g.FillEllipse($dot, 1002, 24, 12, 12)
    $dot.Dispose()

    $fTitle = New-UiFont $family $Px $true
    $fBody = New-UiFont $family $Px $false
    $fmtCenter = New-Object System.Drawing.StringFormat
    $fmtCenter.Alignment = [System.Drawing.StringAlignment]::Center
    $fmtCenter.LineAlignment = [System.Drawing.StringAlignment]::Center

    # Title, vertically centered in the 56 px title bar.
    $ts = $g.MeasureString($title, $fTitle)
    $g.DrawString($title, $fTitle, $brTitleText, [single]24, [single]([Math]::Max(2, (56 - $ts.Height) / 2)))

    # Left navigation: 3 items; the first is selected.
    $navY = @(110, 170, 230)
    for ($i = 0; $i -lt 3; $i++) {
        $y = $navY[$i]
        if ($i -eq 0) {
            $g.FillRectangle($brSel, 8, $y - 8, 224, 36)
            $g.FillRectangle($brAccent, 8, $y - 8, 4, 36)
        }
        $g.DrawString($nav[$i], $fBody, $brNavText, [single]24, [single]($y - 2))
    }

    # Two labeled inputs (labels only; the boxes are empty).
    $g.DrawString($labels[0], $fBody, $brLabel, [single]280, [single]96)
    $g.FillRectangle($brPanel, 280, 122, 420, 40)
    $g.DrawRectangle($penBorder, 280, 122, 420, 40)
    $g.DrawString($labels[1], $fBody, $brLabel, [single]280, [single]190)
    $g.FillRectangle($brPanel, 280, 216, 420, 40)
    $g.DrawRectangle($penBorder, 280, 216, 420, 40)

    # Two buttons with centered text.
    $g.FillRectangle($brBtn1, 280, 286, 160, 44)
    $rect1 = New-Object System.Drawing.RectangleF 280, 286, 160, 44
    $g.DrawString($buttons[0], $fBody, $brWhite, $rect1, $fmtCenter)
    $g.FillRectangle($brBtn2, 456, 286, 160, 44)
    $g.DrawRectangle($penBtn2, 456, 286, 160, 44)
    $rect2 = New-Object System.Drawing.RectangleF 456, 286, 160, 44
    $g.DrawString($buttons[1], $fBody, $brBtn2Text, $rect2, $fmtCenter)

    # Three list rows.
    $rowTop = @(366, 416, 466)
    for ($i = 0; $i -lt 3; $i++) {
        $brush = if (($i % 2) -eq 0) { $brRowA } else { $brRowB }
        $g.FillRectangle($brush, 280, $rowTop[$i], 720, 50)
        $g.DrawLine($penRow, 280, $rowTop[$i] + 50, 1000, $rowTop[$i] + 50)
        $ms = $g.MeasureString($list[$i], $fBody)
        $g.DrawString($list[$i], $fBody, $brText, [single]296, [single]($rowTop[$i] + (50 - $ms.Height) / 2))
    }
    $g.DrawRectangle($penRow, 280, 366, 720, 150)

    # Status bar.
    $ss = $g.MeasureString($status, $fBody)
    $g.DrawString($status, $fBody, $brStatusText, [single]24, [single](596 + (44 - $ss.Height) / 2))

    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)

    $fTitle.Dispose(); $fBody.Dispose(); $fmtCenter.Dispose()
    $brBg.Dispose(); $brTitleBar.Dispose(); $brNav.Dispose(); $brSel.Dispose()
    $brAccent.Dispose(); $brPanel.Dispose(); $brRowA.Dispose(); $brRowB.Dispose()
    $brBtn1.Dispose(); $brBtn2.Dispose(); $brStatus.Dispose(); $brTitleText.Dispose()
    $brNavText.Dispose(); $brLabel.Dispose(); $brText.Dispose(); $brBtn2Text.Dispose()
    $brStatusText.Dispose(); $brWhite.Dispose()
    $penBorder.Dispose(); $penRow.Dispose(); $penBtn2.Dispose()
    $g.Dispose(); $bmp.Dispose()

    return @($title) + $nav + $labels + $buttons + $list + @($status)
}

$cases = @(
    @{ Name = "light-en-11"; Theme = "light"; Lang = "en"; Px = 11 },
    @{ Name = "light-en-16"; Theme = "light"; Lang = "en"; Px = 16 },
    @{ Name = "light-zh-16"; Theme = "light"; Lang = "zh"; Px = 16 },
    @{ Name = "dark-en-11";  Theme = "dark";  Lang = "en"; Px = 11 },
    @{ Name = "dark-zh-11";  Theme = "dark";  Lang = "zh"; Px = 11 },
    @{ Name = "dark-zh-16";  Theme = "dark";  Lang = "zh"; Px = 16 }
)

$manifest = @()
foreach ($case in $cases) {
    $path = Join-Path $OutDir ($case.Name + ".png")
    $expect = @(New-UiImage -Path $path -Theme $case.Theme -Lang $case.Lang -Px $case.Px)
    $manifest += [pscustomobject]@{
        image      = $path
        expect     = $expect
        min_recall = 0.8
    }
}

# UTF-8 with BOM: Windows PowerShell 5.1 Get-Content decodes it correctly,
# which matters because bench-perception.ps1 reads the manifest without
# an explicit -Encoding.
$manifestPath = Join-Path $OutDir "manifest.json"
$json = ConvertTo-Json -InputObject @($manifest) -Depth 5
[System.IO.File]::WriteAllText($manifestPath, $json, (New-Object System.Text.UTF8Encoding($true)))

Write-Host ("manifest: " + $manifestPath)
foreach ($item in $manifest) {
    Write-Host ("{0}: expect={1}" -f [System.IO.Path]::GetFileName($item.image), $item.expect.Count)
}
