@echo off
setlocal

set "ROOT=%~dp0"
set "DOUBLESLASH_HOME=%ROOT%.clientA"
set "DOUBLESLASH_KEY_DIR=%DOUBLESLASH_HOME%"

:: The packaged exe is self-contained: Qt, WebEngine, and dxcompiler sit beside
:: it. A Cargo exe under rust\target is not. This PATH already contains another
:: Qt (PortableApps colmap), and prepending the real Qt from inside parentheses
:: does not work because PATH contains ")", so the loader binds the wrong
:: Qt6Core.dll and then reports Qt6WebEngineCore.dll missing.
:: Cargo still writes to rust\target (target-dir = "../target"). Those builds
:: are only a fallback when dist\DoubleSlash has not been packaged.
set "DIST_BIN=%ROOT%dist\DoubleSlash\DoubleSlash.exe"
set "WS_RELEASE=%ROOT%rust\target\release\doubleslash-client.exe"
set "WS_RELEASE_LOCAL=%ROOT%rust\doubleslash-client\target\release\doubleslash-client.exe"
set "WS_DEBUG=%ROOT%rust\target\debug\doubleslash-client.exe"
set "WS_DEBUG_LOCAL=%ROOT%rust\doubleslash-client\target\debug\doubleslash-client.exe"
set "BINARY="
set "USE_DEV_QT=0"
if exist "%DIST_BIN%" (
    set "BINARY=%DIST_BIN%"
    set "USE_DEV_QT=0"
) else if exist "%WS_RELEASE%" (
    set "BINARY=%WS_RELEASE%"
    set "USE_DEV_QT=1"
) else if exist "%WS_RELEASE_LOCAL%" (
    set "BINARY=%WS_RELEASE_LOCAL%"
    set "USE_DEV_QT=1"
) else if exist "%WS_DEBUG%" (
    set "BINARY=%WS_DEBUG%"
    set "USE_DEV_QT=1"
) else if exist "%WS_DEBUG_LOCAL%" (
    set "BINARY=%WS_DEBUG_LOCAL%"
    set "USE_DEV_QT=1"
)

:: HiDPI display scaling.  DoubleSlash sets QT_SCALE_FACTOR=0.75 automatically
:: at runtime when Windows DPI > 96 (i.e. display scaling > 100%), so Material
:: controls stay desktop-compact on 4K/HiDPI monitors.
:: Override here if you want a different value, e.g.:
::   set QT_SCALE_FACTOR=1.0    -- full OS DPI (largest controls)
::   set QT_SCALE_FACTOR=0.85   -- lighter reduction

if not defined BINARY (
    echo DoubleSlash client binary not found.
    echo Tried:
    echo   %WS_RELEASE%
    echo   %WS_RELEASE_LOCAL%
    echo   %WS_DEBUG%
    echo   %WS_DEBUG_LOCAL%
    echo   %DIST_BIN%
    echo.
    echo Build with:
    echo   cd rust\doubleslash-client
    echo   cargo build --release --features "qt-ui,webengine"
    echo Or package via build_win64.ps1 so dist\DoubleSlash\DoubleSlash.exe exists.
    exit /b 1
)

if "%USE_DEV_QT%"=="0" goto :have_path
if not defined QT_DIR set "QT_DIR=C:\Qt\6.8.3\msvc2022_64"
if exist "%QT_DIR%\bin\Qt6Core.dll" set "PATH=%QT_DIR%\bin;%PATH%"
:have_path

if not exist "%DOUBLESLASH_HOME%\NUL" mkdir "%DOUBLESLASH_HOME%"

echo Launching: %BINARY%
"%BINARY%" %*
