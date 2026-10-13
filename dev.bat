@echo off
setlocal EnableDelayedExpansion

cd /d "%~dp0"

if exist "%USERPROFILE%\.cargo\bin" (
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
)
if exist "%APPDATA%\npm" (
    set "PATH=%APPDATA%\npm;%PATH%"
)

set "RUST_BACKTRACE=1"
if "%RUST_LOG%"=="" (
    set "RUST_LOG=info,asmr_player_lib=debug,asmr_player=debug"
)

echo ========================================================
echo   ASMR Player - Windows Dev Environment Check and Runner
echo ========================================================
echo [WORKDIR] %CD%

if not exist "package.json" (
    echo [ERROR] package.json not found in %CD%
    if not "%~1"=="--check-only" pause
    exit /b 1
)
if not exist "src-tauri\Cargo.toml" (
    echo [ERROR] src-tauri\Cargo.toml not found in %CD%
    if not "%~1"=="--check-only" pause
    exit /b 1
)
if not exist "src-tauri\tauri.conf.json" (
    echo [ERROR] src-tauri\tauri.conf.json not found in %CD%
    if not "%~1"=="--check-only" pause
    exit /b 1
)

where node >nul 2>nul
if errorlevel 1 (
    echo [ERROR] Node.js ^(node^) not found. Please install Node.js 22 LTS or newer.
    if not "%~1"=="--check-only" pause
    exit /b 1
)
for /f "delims=" %%v in ('node -v') do echo [OK] Node.js: %%v

where pnpm >nul 2>nul
if errorlevel 1 (
    echo [ERROR] pnpm not found. Run: npm install -g pnpm
    if not "%~1"=="--check-only" pause
    exit /b 1
)
for /f "delims=" %%v in ('pnpm -v') do echo [OK] pnpm: v%%v

where rustc >nul 2>nul
if errorlevel 1 (
    echo [ERROR] rustc not found. Install Rust stable ^(x86_64-pc-windows-msvc^) from https://rustup.rs
    if not "%~1"=="--check-only" pause
    exit /b 1
)
for /f "delims=" %%v in ('rustc -V') do echo [OK] Rustc: %%v

where cargo >nul 2>nul
if errorlevel 1 (
    echo [ERROR] cargo not found. Check %%USERPROFILE%%\.cargo\bin
    if not "%~1"=="--check-only" pause
    exit /b 1
)
for /f "delims=" %%v in ('cargo -V') do echo [OK] Cargo: %%v

if exist "src-tauri\binaries\mpv-2.dll" (
    echo [OK] libmpv dynamic library: src-tauri\binaries\mpv-2.dll
) else if exist "src-tauri\binaries\libmpv-2.dll" (
    echo [OK] libmpv dynamic library: src-tauri\binaries\libmpv-2.dll
) else (
    echo [ERROR] libmpv dynamic library not found at src-tauri\binaries\mpv-2.dll.
    echo         See src-tauri\binaries\README.md for setup instructions.
    if not "%~1"=="--check-only" pause
    exit /b 1
)

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
        if not "%~1"=="--check-only" pause
        exit /b 1
    )
) else (
    echo [OK] Frontend dependencies ^(node_modules^) ready
)

echo [OK] RUST_BACKTRACE=%RUST_BACKTRACE%
echo [OK] RUST_LOG=%RUST_LOG%

if "%~1"=="--check-only" (
    echo [DONE] Environment self-check passed ^(--check-only^).
    exit /b 0
)

echo [RUN] Starting pnpm tauri dev ...
call pnpm tauri dev
set "EXIT_CODE=%ERRORLEVEL%"
if not "!EXIT_CODE!"=="0" (
    echo.
    echo [ERROR] pnpm tauri dev exited with code: !EXIT_CODE!
    pause
    exit /b !EXIT_CODE!
)

endlocal
