# Generate placeholder BoogieBox icons for Tauri packaging.
# Uses System.Drawing to produce proper DIB-format ICO (RC.EXE compatible).
param(
    [string]$OutDir = "$PSScriptRoot\icons"
)

Add-Type -AssemblyName System.Drawing

if (-not (Test-Path $OutDir)) {
    New-Item -ItemType Directory -Path $OutDir | Out-Null
}

function New-BoogieBoxBitmap([int]$Size) {
    $bmp = New-Object System.Drawing.Bitmap($Size, $Size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
    $g.Clear([System.Drawing.Color]::FromArgb(255, 30, 28, 60))
    $rect = [System.Drawing.Rectangle]::new(0, 0, $Size, $Size)
    $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        $rect,
        [System.Drawing.Color]::FromArgb(255, 30, 28, 60),
        [System.Drawing.Color]::FromArgb(255, 60, 20, 90),
        [System.Drawing.Drawing2D.LinearGradientMode]::ForwardDiagonal
    )
    $g.FillRectangle($brush, 0, 0, $Size, $Size)
    $brush.Dispose()
    $fontSize = [Math]::Max(8, [int]($Size * 0.55))
    $font = New-Object System.Drawing.Font("Arial", $fontSize, [System.Drawing.FontStyle]::Bold)
    $textBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::White)
    $sf = New-Object System.Drawing.StringFormat
    $sf.Alignment = [System.Drawing.StringAlignment]::Center
    $sf.LineAlignment = [System.Drawing.StringAlignment]::Center
    $rectF = [System.Drawing.RectangleF]::new(0, 0, $Size, $Size)
    $g.DrawString("B", $font, $textBrush, $rectF, $sf)
    $font.Dispose(); $textBrush.Dispose(); $sf.Dispose(); $g.Dispose()
    return $bmp
}

Write-Host "Generating BoogieBox placeholder icons in $OutDir ..."

# PNG assets
foreach ($spec in @(@{S=32;N="32x32.png"}, @{S=128;N="128x128.png"}, @{S=256;N="128x128@2x.png"})) {
    $bmp = New-BoogieBoxBitmap $spec.S
    $bmp.Save("$OutDir\$($spec.N)", [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Host "  created $OutDir\$($spec.N)"
}

# ICO: 16, 32, 48 px — stored as DIB (no PNG-in-ICO) so RC.EXE accepts it
$icoSizes = [int[]]@(16, 32, 48)
$count = $icoSizes.Length

# Build each DIB block into its own MemoryStream
$dibStreams = New-Object 'System.Collections.Generic.List[System.IO.MemoryStream]'
foreach ($s in $icoSizes) {
    $bmp = New-BoogieBoxBitmap $s
    $ms  = New-Object System.IO.MemoryStream
    $bw  = New-Object System.IO.BinaryWriter($ms)

    # BITMAPINFOHEADER (40 bytes)
    $bw.Write([uint32]40)
    $bw.Write([int32]$s)
    $bw.Write([int32]($s * 2))   # biHeight doubled = XOR + AND mask
    $bw.Write([uint16]1)          # biPlanes
    $bw.Write([uint16]32)         # biBitCount
    $bw.Write([uint32]0)          # BI_RGB
    $bw.Write([uint32]0)          # biSizeImage
    $bw.Write([int32]0)           # biXPelsPerMeter
    $bw.Write([int32]0)           # biYPelsPerMeter
    $bw.Write([uint32]0)          # biClrUsed
    $bw.Write([uint32]0)          # biClrImportant

    # XOR mask: 32bpp BGRA, bottom-to-top row order
    for ($y = $s - 1; $y -ge 0; $y--) {
        for ($x = 0; $x -lt $s; $x++) {
            $p = $bmp.GetPixel($x, $y)
            $bw.Write([byte]$p.B)
            $bw.Write([byte]$p.G)
            $bw.Write([byte]$p.R)
            $bw.Write([byte]255)   # force fully opaque
        }
    }

    # AND mask: 1bpp, rows padded to DWORD; all-zero = fully opaque
    $andRow  = [int]([Math]::Ceiling($s / 32.0)) * 4
    $andMask = New-Object byte[] ($andRow * $s)
    $bw.Write($andMask)

    $bw.Flush()
    $bmp.Dispose()
    [void]$dibStreams.Add($ms)
}

# Assemble the ICO file
$icoMs = New-Object System.IO.MemoryStream
$icoW  = New-Object System.IO.BinaryWriter($icoMs)

# ICONDIR (6 bytes)
$icoW.Write([uint16]0)       # reserved
$icoW.Write([uint16]1)       # type = ICO
$icoW.Write([uint16]$count)

$dataOffset = [long](6 + 16 * $count)   # 6 + 16 bytes * entry count

# ICONDIRENTRY (16 bytes each)
for ($i = 0; $i -lt $count; $i++) {
    $s      = $icoSizes[$i]
    $dibLen = [long]$dibStreams[$i].Length
    $icoW.Write([byte]$s)
    $icoW.Write([byte]$s)
    $icoW.Write([byte]0)       # colorCount (0 = use biBitCount)
    $icoW.Write([byte]0)       # reserved
    $icoW.Write([uint16]1)     # planes
    $icoW.Write([uint16]32)    # bitCount
    $icoW.Write([uint32]$dibLen)
    $icoW.Write([uint32]$dataOffset)
    $dataOffset += $dibLen
}

# Image data blocks
foreach ($ds in $dibStreams) {
    $icoW.Write($ds.ToArray())
    $ds.Dispose()
}

$icoW.Flush()
$icoPath = "$OutDir\icon.ico"
[System.IO.File]::WriteAllBytes($icoPath, $icoMs.ToArray())
$icoMs.Dispose()
Write-Host "  created $icoPath"
Write-Host "icon.ico created successfully"
