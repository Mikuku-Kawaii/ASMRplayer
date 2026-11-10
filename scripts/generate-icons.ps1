param()

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$projectRoot = [IO.Path]::GetFullPath("$PSScriptRoot/..")
$scratch = Join-Path $projectRoot '.tmp/brand-icon-generation'
New-Item -ItemType Directory -Force -Path $scratch | Out-Null
function Render-Svg([string]$Source, [int]$Size, [string]$Target) {
    & "$projectRoot/node_modules/.bin/tauri.cmd" icon $Source --png $Size --output $scratch
    if ($LASTEXITCODE -ne 0) { throw "Cannot render $Source" }
    Copy-Item -LiteralPath (Join-Path $scratch ("${Size}x${Size}.png")) -Destination $Target -Force
}
$source = Join-Path $projectRoot 'assets/branding/app-icon.svg'
$masterFile = Join-Path $projectRoot 'assets/branding/app-icon.png'
Render-Svg $source 1024 $masterFile
Render-Svg "$projectRoot/public/brand-mark.svg" 192 "$projectRoot/android/app/src/main/res/drawable-nodpi/brand_mark.png"
# The launcher supplies the final mask. Extend the same illustration without its baked corner clip.
$unmasked = Join-Path $scratch 'unmasked.svg'
[IO.File]::WriteAllText($unmasked, [IO.File]::ReadAllText($source).Replace('clip-path="url(#icon-corners)"',''))
$unmaskedPng = Join-Path $scratch 'unmasked.png'
Render-Svg $unmasked 1024 $unmaskedPng
$adaptiveArt = [Drawing.Bitmap]::FromFile($unmaskedPng)
$art = [Drawing.Bitmap]::FromFile($masterFile)

function New-Canvas([int]$Size) {
    return [Drawing.Bitmap]::new($Size, $Size, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
}
function New-Graphics($Bitmap) {
    $g = [Drawing.Graphics]::FromImage($Bitmap)
    $g.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    return $g
}
function Resize-Art($Image, [int]$Size) {
    $b = New-Canvas $Size
    $g = New-Graphics $b
    $attributes = [Drawing.Imaging.ImageAttributes]::new()
    $attributes.SetWrapMode([Drawing.Drawing2D.WrapMode]::TileFlipXY)
    $g.DrawImage($Image, [Drawing.Rectangle]::new(0, 0, $Size, $Size), 0, 0, $Image.Width, $Image.Height, [Drawing.GraphicsUnit]::Pixel, $attributes)
    $attributes.Dispose(); $g.Dispose()
    return $b
}
function Round-Art($Image, [double]$Radius) {
    $b = New-Canvas $Image.Width
    $g = New-Graphics $b
    $path = [Drawing.Drawing2D.GraphicsPath]::new()
    $d = [single]($Image.Width * $Radius * 2)
    $s = [single]$Image.Width
    $path.AddArc(0, 0, $d, $d, 180, 90)
    $path.AddArc(($s-$d), 0, $d, $d, 270, 90)
    $path.AddArc(($s-$d), ($s-$d), $d, $d, 0, 90)
    $path.AddArc(0, ($s-$d), $d, $d, 90, 90)
    $path.CloseFigure()
    $brush = [Drawing.TextureBrush]::new($Image)
    $g.FillPath($brush, $path)
    $brush.Dispose(); $path.Dispose(); $g.Dispose()
    return $b
}
function Save-Png($Image, [string]$RelativePath) {
    $path = Join-Path $projectRoot $RelativePath
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
    $Image.Save($path, [Drawing.Imaging.ImageFormat]::Png)
}
function Save-Resized($Image, [int]$Size, [string]$Path) {
    $b = Resize-Art $Image $Size
    try { Save-Png $b $Path } finally { $b.Dispose() }
}

try {
    $master = Resize-Art $art 1024
    Save-Resized $master 512 'public/asmr-icon.png'
    Save-Resized $master 512 'src-tauri/icons/icon.png'
    Save-Resized $master 128 'src-tauri/icons/128x128.png'
    Save-Resized $master 32 'src-tauri/icons/32x32.png'

    # Windows ICO directory with PNG frames; preserve alpha at every shell size.
    $sizes = @(16, 24, 32, 48, 64, 128, 256)
    $frames = @()
    foreach ($size in $sizes) {
        $b = Resize-Art $master $size
        $stream = [IO.MemoryStream]::new()
        $b.Save($stream, [Drawing.Imaging.ImageFormat]::Png)
        $frames += ,$stream.ToArray()
        $stream.Dispose(); $b.Dispose()
    }
    $writer = [IO.BinaryWriter]::new([IO.File]::Create("$projectRoot/src-tauri/icons/icon.ico"))
    try {
        $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]$sizes.Count)
        $offset = 6 + 16 * $sizes.Count
        for ($i = 0; $i -lt $sizes.Count; $i++) {
            $dimension = [byte]($sizes[$i] % 256)
            $writer.Write($dimension); $writer.Write($dimension)
            $writer.Write([byte]0); $writer.Write([byte]0)
            $writer.Write([uint16]1); $writer.Write([uint16]32)
            $writer.Write([uint32]$frames[$i].Length); $writer.Write([uint32]$offset)
            $offset += $frames[$i].Length
        }
        foreach ($frame in $frames) { $writer.Write([byte[]]$frame) }
    } finally { $writer.Dispose() }

    $res = 'android/app/src/main/res'
    Save-Resized $master 192 "$res/drawable-nodpi/app_icon.png"
    $round = Round-Art $master 0.5
    $densities = @{mdpi=48; hdpi=72; xhdpi=96; xxhdpi=144; xxxhdpi=192}
    foreach ($density in $densities.Keys) {
        Save-Resized $master $densities[$density] "$res/mipmap-$density/ic_launcher.png"
        Save-Resized $round $densities[$density] "$res/mipmap-$density/ic_launcher_round.png"
    }

    # Adaptive layers are 108dp; the central 72dp holds the original composition.
    # Extend the edge colors into the overscan so launcher masks never expose a seam.
    $adaptive = New-Canvas 432
    $g = New-Graphics $adaptive
    $edgeSampling = [Drawing.Imaging.ImageAttributes]::new()
    $edgeSampling.SetWrapMode([Drawing.Drawing2D.WrapMode]::TileFlipXY)
    $srcX = @(0, 0, ($adaptiveArt.Width-1)); $srcY = @(0, 0, ($adaptiveArt.Height-1))
    $srcW = @(1, $adaptiveArt.Width, 1); $srcH = @(1, $adaptiveArt.Height, 1)
    $positions = @(0, 72, 360); $lengths = @(72, 288, 72)
    for ($y=0; $y -lt 3; $y++) {
        for ($x=0; $x -lt 3; $x++) {
            $g.DrawImage($adaptiveArt, [Drawing.Rectangle]::new($positions[$x], $positions[$y], $lengths[$x], $lengths[$y]),
                [int]$srcX[$x], [int]$srcY[$y], [int]$srcW[$x], [int]$srcH[$y], [Drawing.GraphicsUnit]::Pixel, $edgeSampling)
        }
    }
    $edgeSampling.Dispose(); $g.Dispose()
    Save-Png $adaptive "$res/drawable-nodpi/ic_launcher_artwork.png"
    $adaptive.Dispose(); $round.Dispose(); $master.Dispose()
    Write-Output 'Generated desktop PNG/ICO, Android legacy/adaptive launcher assets and NSFW artwork.'
} finally { $art.Dispose(); $adaptiveArt.Dispose() }
