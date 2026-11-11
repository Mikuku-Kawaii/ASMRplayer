param([switch]$DesktopOnly)

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
function Save-Png($Image, [string]$RelativePath) {
    $path = Join-Path $projectRoot $RelativePath
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
    $Image.Save($path, [Drawing.Imaging.ImageFormat]::Png)
}
function Save-Resized($Image, [int]$Size, [string]$Path) {
    $b = Resize-Art $Image $Size
    try { Save-Png $b $Path } finally { $b.Dispose() }
}

function Write-DesktopIcons($master) {
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
    Copy-Item -LiteralPath "$projectRoot/src-tauri/icons/icon.ico" -Destination "$projectRoot/public/favicon.ico" -Force

    Write-Output 'Generated desktop PNG, ICO and favicon.'
}

$launcherMaster = Join-Path $scratch 'launcher-master.png'
Render-Svg "$projectRoot/assets/branding/launcher-icon.svg" 1024 $launcherMaster
$launcherArt = [Drawing.Bitmap]::FromFile($launcherMaster)
try {
    Write-DesktopIcons $launcherArt
    if (!$DesktopOnly) {
        $res = 'android/app/src/main/res'
        $androidMaster = Join-Path $projectRoot 'assets/branding/app-icon.png'
        Render-Svg "$projectRoot/assets/branding/app-icon.svg" 1024 $androidMaster
        Render-Svg "$projectRoot/public/desktop-brand-mark.svg" 192 "$projectRoot/$res/drawable-nodpi/brand_mark.png"
        $androidArt = [Drawing.Bitmap]::FromFile($androidMaster)
        try { $neutralMaster = Resize-Art $androidArt 1024 } finally { $androidArt.Dispose() }
        try { Save-Resized $neutralMaster 192 "$res/drawable-nodpi/app_icon.png" } finally { $neutralMaster.Dispose() }
        $densities = @{mdpi=48; hdpi=72; xhdpi=96; xxhdpi=144; xxxhdpi=192}
        foreach ($density in $densities.Keys) {
            Save-Resized $launcherArt $densities[$density] "$res/mipmap-$density/ic_launcher.png"
        }
        Write-Output 'Generated Android rounded-square launcher and brand assets.'
    }
} finally { $launcherArt.Dispose() }
