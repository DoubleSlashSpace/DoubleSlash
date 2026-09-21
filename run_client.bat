@echo off
setlocal

set "ROOT=%~dp0"
set "DOUBLESLASH_HOME=%ROOT%.clientA"
set "DOUBLESLASH_KEY_DIR=%DOUBLESLASH_HOME%"

:: Prefer a freshly built Qt client (release, then debug), then the packaged dist.
set "WS_RELEASE=%ROOT%rust\doubleslash-client\target\release\doubleslash-client.exe"
set "WS_DEBUG=%ROOT%rust\doubleslash-client\target\debug\doubleslash-client.exe"
set "DIST_BIN=%ROOT%dist\DoubleSlash\DoubleSlash.exe"
set "BINARY="
set "USE_DEV_QT=0"
if exist "%WS_RELEASE%" (
    set "BINARY=%WS_RELEASE%"
    set "USE_DEV_QT=1"
) else if exist "%WS_DEBUG%" (
    set "BINARY=%WS_DEBUG%"
    set "USE_DEV_QT=1"
) else if exist "%DIST_BIN%" (
    set "BINARY=%DIST_BIN%"
    set "USE_DEV_QT=0"
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
    echo   %WS_DEBUG%
    echo   %DIST_BIN%
    echo.
    echo Build with:
    echo   cd rust\doubleslash-client
    echo   cargo build -p doubleslash-client --features "qt-ui,webengine,console"
    echo Or package via build_win64.ps1 so dist\DoubleSlash\DoubleSlash.exe exists.
    exit /b 1
)

if "%USE_DEV_QT%"=="1" (
    if defined QT_DIR (
        set "PATH=%QT_DIR%\bin;%PATH%"
    ) else if exist "C:\Qt\6.8.3\msvc2022_64\bin\NUL" (
        set "PATH=C:\Qt\6.8.3\msvc2022_64\bin;%PATH%"
        set "QT_DIR=C:\Qt\6.8.3\msvc2022_64"
    )
)

if not exist "%DOUBLESLASH_HOME%\NUL" mkdir "%DOUBLESLASH_HOME%"

echo Launching: %BINARY%
"%BINARY%" %*
