param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
$previousKey = $env:TAURI_SIGNING_PRIVATE_KEY
try {
    if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
        $localKey = Join-Path $projectRoot '.release-signing/updater.key'
        if (-not (Test-Path -LiteralPath $localKey)) { throw 'Set TAURI_SIGNING_PRIVATE_KEY or restore .release-signing/updater.key from your private backup.' }
        $env:TAURI_SIGNING_PRIVATE_KEY = $localKey
    }
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    if (-not $SkipBuild) {
        & pnpm typecheck
        if ($LASTEXITCODE) { throw 'TypeScript check failed' }
        & pnpm lint
        if ($LASTEXITCODE) { throw 'Lint failed' }
        # The public source export omits local test suites and fixtures.
        if (Test-Path 'tests/fixtures') {
            & pnpm test
            if ($LASTEXITCODE) { throw 'Frontend tests failed' }
            & cargo test --locked --manifest-path src-tauri/Cargo.toml
            if ($LASTEXITCODE) { throw 'Rust tests failed' }
        } else {
            Write-Output 'Public source export: local regression suites are not included.'
        }
        & pnpm tauri build
        if ($LASTEXITCODE) { throw 'Signed Tauri build failed' }
    }
    & node scripts/write-update-manifest.mjs
    if ($LASTEXITCODE) { throw 'Update metadata generation failed' }
    $version = (Get-Content package.json -Raw | ConvertFrom-Json).version
    Compress-Archive -Path licenses -DestinationPath "output/release/v$version/THIRD_PARTY_LICENSES.zip" -Force
} finally {
    $env:TAURI_SIGNING_PRIVATE_KEY = $previousKey
    Pop-Location
}
