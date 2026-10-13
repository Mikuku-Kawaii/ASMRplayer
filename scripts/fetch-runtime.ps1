# Retrieve the known runtime from this project's existing release, verifying both
# the source installer and extracted DLLs. Administrative extraction does not install it.
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$runtime = Join-Path $projectRoot 'src-tauri/binaries'
$hashes = @{
    'mpv-2.dll' = '5080e00cd114f3dfc0eb87f35068fec70afc8bfd6fc29dca4b79c874a42485ee'
    'vulkan-1.dll' = '2baae0a109cb5962437b00a182750308ce9f69f59381df62939c00c21150ee6b'
}
$missing = @($hashes.Keys | Where-Object { -not (Test-Path -LiteralPath (Join-Path $runtime $_)) })
if ($missing.Count) {
    $temp = Join-Path $projectRoot ('.tmp/runtime-' + [guid]::NewGuid())
    New-Item -ItemType Directory -Path $temp -Force | Out-Null
    $installer = Join-Path $temp 'runtime.msi'
    Invoke-WebRequest -Uri 'https://github.com/Mikuku-Kawaii/ASMRplayer/releases/download/v1.0.0/ASMR.Player_1.0.0_x64_en-US.msi' -OutFile $installer
    if ((Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant() -ne 'febce5ba5794bfc45020067969022d85649c01729fe003834d3ce028d15805a7') { throw 'Runtime installer hash mismatch' }
    $extracted = Join-Path $temp 'extracted'
    $process = Start-Process msiexec.exe -ArgumentList @('/a', "`"$installer`"", '/qn', "TARGETDIR=`"$extracted`"") -Wait -PassThru -WindowStyle Hidden
    if ($process.ExitCode -ne 0) { throw "Runtime extraction failed: $($process.ExitCode)" }
    New-Item -ItemType Directory -Path $runtime -Force | Out-Null
    foreach ($name in $missing) {
        $files = @(Get-ChildItem -LiteralPath $extracted -Recurse -File -Filter $name)
        if ($files.Count -ne 1) { throw "Expected one $name in extracted runtime" }
        if ((Get-FileHash -LiteralPath $files[0].FullName -Algorithm SHA256).Hash.ToLowerInvariant() -ne $hashes[$name]) { throw "Runtime hash mismatch: $name" }
        Copy-Item -LiteralPath $files[0].FullName -Destination (Join-Path $runtime $name)
    }
}
foreach ($name in $hashes.Keys) {
    if ((Get-FileHash -LiteralPath (Join-Path $runtime $name) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $hashes[$name]) { throw "Runtime hash mismatch: $name" }
}
Write-Output 'Verified libmpv and Vulkan runtime.'
