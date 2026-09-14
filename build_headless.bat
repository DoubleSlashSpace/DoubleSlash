@echo off
setlocal EnableExtensions

:: ---------------------------------------------------------------------------
:: Build the headless doubleslash-client (no Qt UI) for integration testing.
::
:: Output:
::   rust\target-headless\debug\doubleslash-client.exe     (default)
::   rust\target-headless\release\doubleslash-client.exe   (release mode)
::
:: Usage:
::   build_headless.bat              :: debug + console
::   build_headless.bat release      :: release + console
::   build_headless.bat test         :: cargo test (headless / no qt-ui)
::
:: Then:
::   run_clientA_headless.bat
::   scripts\test_ollama_auto_reply.ps1 -Profile .clientA
::   scripts\test_ollama_api.ps1
:: ---------------------------------------------------------------------------

set "ROOT=%~dp0"
set "CLIENT_DIR=%ROOT%rust\doubleslash-client"
set "CARGO_TARGET_DIR=%ROOT%rust\target-headless"
set "MODE=%~1"
if "%MODE%"=="" set "MODE=debug"

if not exist "%CLIENT_DIR%\Cargo.toml" (
    echo ERROR: client crate not found at %CLIENT_DIR%
    exit /b 1
)

echo === DoubleSlash headless client build ===
echo Crate:    %CLIENT_DIR%
echo Target:   %CARGO_TARGET_DIR%
echo Mode:     %MODE%
echo Features: console  ^(no qt-ui^)
echo.

pushd "%CLIENT_DIR%"
if /I "%MODE%"=="test" (
    cargo test -p doubleslash-client --features console
    if errorlevel 1 (
        popd
        echo.
        echo Tests FAILED.
        exit /b 1
    )
    popd
    echo.
    echo Headless unit tests finished OK.
    exit /b 0
)

if /I "%MODE%"=="release" (
    cargo build -p doubleslash-client --features console --release
    if errorlevel 1 (
        popd
        echo.
        echo Build FAILED.
        exit /b 1
    )
    set "BIN=%CARGO_TARGET_DIR%\release\doubleslash-client.exe"
) else if /I "%MODE%"=="debug" (
    cargo build -p doubleslash-client --features console
    if errorlevel 1 (
        popd
        echo.
        echo Build FAILED.
        exit /b 1
    )
    set "BIN=%CARGO_TARGET_DIR%\debug\doubleslash-client.exe"
) else (
    popd
    echo Unknown mode: %MODE%
    echo Use: debug ^| release ^| test
    exit /b 1
)
popd

if not exist "%BIN%" (
    echo ERROR: expected binary missing: %BIN%
    exit /b 1
)

echo.
echo Build OK.
echo Binary: %BIN%
for %%I in ("%BIN%") do echo Size:   %%~zI bytes   %%~tI
echo.
echo Next:
echo   run_clientA_headless.bat
echo   scripts\test_ollama_auto_reply.ps1 -Profile .clientA
exit /b 0
