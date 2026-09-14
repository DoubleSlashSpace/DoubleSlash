@echo off
setlocal

set "ROOT=%~dp0"
set "DOUBLESLASH_HOME=%ROOT%.clientA"
set "DOUBLESLASH_KEY_DIR=%DOUBLESLASH_HOME%"
set "BINARY=%ROOT%dist\DoubleSlash\DoubleSlash.exe"

:: HiDPI display scaling.  DoubleSlash sets QT_SCALE_FACTOR=0.75 automatically
:: at runtime when Windows DPI > 96 (i.e. display scaling > 100%), so Material
:: controls stay desktop-compact on 4K/HiDPI monitors.
:: Override here if you want a different value, e.g.:
::   set QT_SCALE_FACTOR=1.0    -- full OS DPI (largest controls)
::   set QT_SCALE_FACTOR=0.85   -- lighter reduction

if not exist "%BINARY%" (
    echo DoubleSlash client binary not found at:
    echo   %BINARY%
    echo.
    echo Build or package it first so dist\DoubleSlash\DoubleSlash.exe exists.
    exit /b 1
)

if not exist "%DOUBLESLASH_HOME%\NUL" mkdir "%DOUBLESLASH_HOME%"

"%BINARY%" %*
