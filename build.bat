@echo off
setlocal EnableDelayedExpansion

cd /d "%~dp0"

if exist "%USERPROFILE%\.cargo\bin" (
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
)
if exist "%APPDATA%\npm" (
    set "PATH=%APPDATA%\npm;%PATH%"
)

echo ========================================================
echo   ASMR Player - Windows Release Packaging Runner
echo ========================================================
echo [WORKDIR] %CD%

if not exist "package.json" (
    echo [ERROR] package.json not found in %CD%
    if not "%~1"=="--no-pause" pause
    exit /b 1
)
if not exist "src-tauri\Cargo.toml" (
    echo [ERROR] src-tauri\Cargo.toml not found in %CD%
    if not "%~1"=="--no-pause" pause
    exit /b 1
)
if not exist "src-tauri\tauri.conf.json" (
    echo [ERROR] src-tauri\tauri.conf.json not found in %CD%
    if not "%~1"=="--no-pause" pause
    exit /b 1
)

where node >nul 2>nul
if errorlevel 1 (
    echo [ERROR] Node.js ^(node^) not found. Please install Node.js 22 LTS or newer.
    if not "%~1"=="--no-pause" pause
    exit /b 1
)
for /f "delims=" %%v in ('node -v') do echo [OK] Node.js: %%v

where pnpm >nul 2>nul
if errorlevel 1 (
    echo [ERROR] pnpm not found. Run: npm install -g pnpm
    if not "%~1"=="--no-pause" pause
    exit /b 1
)
for /f "delims=" %%v in ('pnpm -v') do echo [OK] pnpm: v%%v

where rustc >nul 2>nul
if errorlevel 1 (
    echo [ERROR] rustc not found. Install Rust stable ^(x86_64-pc-windows-msvc^) from https://rustup.rs
    if not "%~1"=="--no-pause" pause
    exit /b 1
)
for /f "delims=" %%v in ('rustc -V') do echo [OK] Rustc: %%v

where cargo >nul 2>nul
if errorlevel 1 (
    echo [ERROR] cargo not found. Check %%USERPROFILE%%\.cargo\bin
    if not "%~1"=="--no-pause" pause
    exit /b 1
)
for /f "delims=" %%v in ('cargo -V') do echo [OK] Cargo: %%v

if not exist "src-tauri\binaries\mpv-2.dll" (
    echo [ERROR] libmpv not found at src-tauri\binaries\mpv-2.dll
    echo         See src-tauri\binaries\README.md for runtime setup.
    if not "%~1"=="--no-pause" pause
    exit /b 1
)
if not exist "src-tauri\binaries\vulkan-1.dll" (
    echo [ERROR] Vulkan loader not found at src-tauri\binaries\vulkan-1.dll
    echo         See src-tauri\binaries\README.md for runtime setup.
    if not "%~1"=="--no-pause" pause
    exit /b 1
)
echo [OK] libmpv runtime: mpv-2.dll and vulkan-1.dll

set "NEED_INSTALL=0"
if not exist "node_modules" (
    set "NEED_INSTALL=1"
)
if not exist "node_modules\.modules.yaml" (
    set "NEED_INSTALL=1"
)

if "!NEED_INSTALL!"=="1" (
    echo [INFO] Installing frontend dependencies ^(pnpm install --frozen-lockfile^)...
    call pnpm install --frozen-lockfile
    if errorlevel 1 (
        echo [ERROR] pnpm install failed. Check network or lockfile state.
        if not "%~1"=="--no-pause" pause
        exit /b 1
    )
) else (
    echo [OK] Frontend dependencies ^(node_modules^) ready
)

if "%~1"=="--check-only" (
    echo [DONE] Release build environment self-check passed ^(--check-only^).
    exit /b 0
)

if "%TAURI_SIGNING_PRIVATE_KEY%"=="" (
    if exist ".release-signing\updater.key" (
        set "TAURI_SIGNING_PRIVATE_KEY=%CD%\.release-signing\updater.key"
    ) else (
        echo [ERROR] Set TAURI_SIGNING_PRIVATE_KEY before building signed updates.
        exit /b 1
    )
)

REM Prevent "os error 5 (Access Denied)" if asmr-player.exe is currently running
if exist "src-tauri\target\release\asmr-player.old.exe" (
    del /f /q "src-tauri\target\release\asmr-player.old.exe" >nul 2>nul
)
if exist "src-tauri\target\release\asmr-player.exe" (
    move /y "src-tauri\target\release\asmr-player.exe" "src-tauri\target\release\asmr-player.old.exe" >nul 2>nul
)

echo.
echo [STEP 1/2] Running frontend checks ^(typecheck ^& lint^)...
call pnpm typecheck
if errorlevel 1 (
    echo [ERROR] TypeScript check failed.
    if not "%~1"=="--no-pause" pause
    exit /b 1
)
call pnpm lint
if errorlevel 1 (
    echo [ERROR] ESLint check failed.
    if not "%~1"=="--no-pause" pause
    exit /b 1
)

echo.
echo [STEP 2/2] Building Tauri release executable and installers ^(pnpm tauri build^)...
call pnpm tauri build
set "EXIT_CODE=%ERRORLEVEL%"
if not "!EXIT_CODE!"=="0" (
    echo.
    echo [ERROR] pnpm tauri build failed with code: !EXIT_CODE!
    if not "%~1"=="--no-pause" pause
    exit /b !EXIT_CODE!
)

if exist "src-tauri\target\release\asmr-player.old.exe" (
    del /f /q "src-tauri\target\release\asmr-player.old.exe" >nul 2>nul
)

echo.
echo ========================================================
echo   [SUCCESS] Release Packaging Completed!
echo ========================================================

node scripts\write-update-manifest.mjs
if errorlevel 1 exit /b 1
echo   Standalone EXE : src-tauri\target\release\asmr-player.exe
echo   NSIS Installer : src-tauri\target\release\bundle\nsis\
echo   MSI Installer  : src-tauri\target\release\bundle\msi\
echo ========================================================

if not "%~1"=="--no-pause" pause
endlocal
