param([Parameter(Mandatory = $true)][ValidateRange(1, 2100000000)][int]$VersionCode)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
foreach ($name in @('ANDROID_HOME', 'JAVA_HOME', 'ANDROID_KEYSTORE_FILE', 'ANDROID_KEYSTORE_PASSWORD', 'ANDROID_KEY_ALIAS', 'ANDROID_KEY_PASSWORD')) {
    if (-not [Environment]::GetEnvironmentVariable($name)) { throw "Set $name before preparing a signed Android release." }
}
if (-not $env:TAURI_SIGNING_PRIVATE_KEY -and -not $env:TAURI_SIGNING_PRIVATE_KEY_PATH) {
    throw 'Set TAURI_SIGNING_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY_PATH to the existing updater signing key.'
}
Push-Location $root
try {
    $version = (Get-Content package.json -Raw | ConvertFrom-Json).version
    if ($version -notmatch '^\d+\.\d+\.\d+$') { throw 'Use a stable shared release version.' }
    $buildTools = Get-ChildItem -LiteralPath (Join-Path $env:ANDROID_HOME 'build-tools') -Directory |
        Where-Object { $_.Name -match '^\d+\.\d+\.\d+$' } | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
    if (-not $buildTools) { throw 'Android build tools are missing.' }
    & ./android/gradlew.bat -p android ":app:assembleRelease" "-PandroidVersionCode=$VersionCode" --console=plain --no-daemon
    if ($LASTEXITCODE) { throw 'Android release build failed.' }
    $apk = Join-Path $root 'android/app/build/outputs/apk/release/app-release.apk'
    & (Join-Path $buildTools.FullName 'apksigner.bat') verify --verbose $apk
    if ($LASTEXITCODE) { throw 'APK certificate verification failed.' }
    $badging = (& (Join-Path $buildTools.FullName 'aapt.exe') dump badging $apk) -join "`n"
    if ($LASTEXITCODE -or $badging -match 'application-debuggable') { throw 'Invalid or debuggable release APK.' }
    $package = [regex]::Match($badging, "package: name='([^']+)' versionCode='(\d+)' versionName='([^']+)'")
    $minimum = [regex]::Match($badging, "sdkVersion:'(\d+)'")
    if (-not $package.Success -or -not $minimum.Success -or $package.Groups[3].Value -ne $version -or [long]$package.Groups[2].Value -ne $VersionCode) {
        throw 'APK version metadata does not match the release.'
    }
    & pnpm tauri signer sign --app-version $version $apk
    if ($LASTEXITCODE) { throw 'Updater artifact signing failed.' }
    $metadataPath = Join-Path (Split-Path $apk) 'verified-metadata.json'
    @{ packageName = $package.Groups[1].Value; version = $version; versionCode = $VersionCode; minSdk = [int]$minimum.Groups[1].Value } |
        ConvertTo-Json | Set-Content -LiteralPath $metadataPath -Encoding utf8NoBOM
    & node scripts/write-android-update.mjs $apk $metadataPath
    if ($LASTEXITCODE) { throw 'Shared update manifest generation failed.' }
} finally { Pop-Location }
