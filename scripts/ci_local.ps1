<#
.SYNOPSIS
    Run the same checks as .github/workflows/ci.yml before pushing.

.DESCRIPTION
    Mirrors the Linux "Rust tests" job and the Windows "Rust tests (non-Qt)"
    job: version sync, Opus weights, fmt, clippy, release-manifest self-test,
    release-mode tests, and cargo-audit, across all four Cargo workspaces
    (rust/, the client, the Android bridge, and the supernode manager).

    Android extras, skipped when the tools are missing:
      - Gradle `testDebugUnitTest` when an Android SDK is configured
      - `cargo ndk` clippy of the JNI cdylib when NDK + cargo-ndk + the
        aarch64-linux-android target are present
      - `assembleDebug` only with -IncludeAndroidApk

    Linux desktop/client cfg and supernode packaging are not produced from this
    Windows script. On Linux or in WSL, run scripts/ci_local.sh instead.

    Run from the repository root:

        powershell -ExecutionPolicy Bypass -File scripts/ci_local.ps1

    Faster iteration (lint only, no tests/audit):

        powershell -ExecutionPolicy Bypass -File scripts/ci_local.ps1 -SkipTests -SkipAudit

.PARAMETER RustToolchain
    Must match env.RUST_TOOLCHAIN in ci.yml (default: 1.97.1).

.PARAMETER SkipTests
    Skip cargo test, Gradle unit tests, and the optional APK build.

.PARAMETER SkipAudit
    Skip cargo-audit (requires no network / advisory-db fetch).

.PARAMETER SkipOpusFetch
    Skip scripts/fetch_opus_weights.ps1 (safe when all tar_list.txt dnn files exist).

.PARAMETER SkipAndroidNdk
    Skip the optional cargo-ndk clippy even when the Android NDK toolchain is present.

.PARAMETER IncludeAndroidApk
    If the Android SDK is present, also run `gradlew assembleDebug`. Off by default;
    the APK job lives in .github/workflows/android.yml.
#>

[CmdletBinding()]
param(
    [string]$RustToolchain = '1.97.1',
    [switch]$SkipTests,
    [switch]$SkipAudit,
    [switch]$SkipOpusFetch,
    [switch]$SkipAndroidNdk,
    [switch]$IncludeAndroidApk
)

$ErrorActionPreference = 'Stop'

$RepoRoot = Split-Path -Parent $PSScriptRoot
$RustDir = Join-Path $RepoRoot 'rust'
$ClientDir = Join-Path $RustDir 'doubleslash-client'
$AndroidRustDir = Join-Path $RustDir 'doubleslash-android'
$AndroidDir = Join-Path $RepoRoot 'android'
$ManagerDir = Join-Path $RustDir 'doubleslash-supernode-manager'

function Write-Step([string]$Name) {
    Write-Host ""
    Write-Host "==> $Name" -ForegroundColor Cyan
}

function Write-Skip([string]$Name, [string]$Reason) {
    Write-Host ""
    Write-Host "==> $Name" -ForegroundColor Cyan
    Write-Host "    skipped: $Reason" -ForegroundColor Yellow
}

function Invoke-Step([string]$Name, [scriptblock]$Body) {
    Write-Step $Name
    Push-Location $RepoRoot
    try {
        & $Body
        if ($LASTEXITCODE -and $LASTEXITCODE -ne 0) {
            throw "Step failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
}

function Invoke-Cargo([string]$WorkingDir, [string[]]$CargoArgs) {
    Push-Location $WorkingDir
    try {
        & cargo @CargoArgs
        if ($LASTEXITCODE -ne 0) {
            throw "cargo $($CargoArgs -join ' ') failed (exit $LASTEXITCODE)"
        }
    }
    finally {
        Pop-Location
    }
}

function Get-AndroidSdkDir {
    $localProps = Join-Path $AndroidDir 'local.properties'
    if (Test-Path $localProps) {
        foreach ($line in Get-Content $localProps) {
            if ($line -match '^\s*sdk\.dir\s*=\s*(.+)\s*$') {
                $dir = $Matches[1].Trim() -replace '/', '\'
                if (Test-Path $dir) { return $dir }
            }
        }
    }
    foreach ($envName in @('ANDROID_HOME', 'ANDROID_SDK_ROOT')) {
        $candidate = [Environment]::GetEnvironmentVariable($envName)
        if ($candidate -and (Test-Path $candidate)) { return $candidate }
    }
    return $null
}

function Get-AndroidNdkVersion {
    $gradle = Join-Path $AndroidDir 'app\build.gradle.kts'
    $text = Get-Content $gradle -Raw
    if ($text -match 'ndkVersion\s*=\s*"([^"]+)"') {
        return $Matches[1]
    }
    return $null
}

function Get-AndroidNdkApi {
    $props = Join-Path $AndroidDir 'gradle.properties'
    $text = Get-Content $props -Raw
    if ($text -match '(?m)^doubleslash\.ndkApi=(.+)$') {
        return $Matches[1].Trim()
    }
    return '26'
}

function Test-CargoNdk {
    $cmd = Get-Command cargo-ndk -ErrorAction SilentlyContinue
    return [bool]$cmd
}

function Test-RustTarget([string]$Triple) {
    $installed = & rustup target list --installed 2>$null
    return [bool]($installed | Where-Object { $_ -eq $Triple })
}

Write-Host "DoubleSlash local CI (toolchain $RustToolchain)" -ForegroundColor Green
Write-Host "Repo: $RepoRoot"

Invoke-Step 'Ensure git submodules (recursive)' {
    git submodule update --init --recursive
}

Invoke-Step "Install Rust $RustToolchain (rustfmt + clippy)" {
    rustup toolchain install $RustToolchain --component rustfmt --component clippy
    $env:RUSTUP_TOOLCHAIN = $RustToolchain
}

Invoke-Step 'Verify version metadata stays in sync' {
    & (Join-Path $PSScriptRoot 'check_version_sync.ps1')
}

if (-not $SkipOpusFetch) {
    Invoke-Step 'Fetch Opus DNN model weights' {
        & (Join-Path $PSScriptRoot 'fetch_opus_weights.ps1')
    }
}

Invoke-Step 'cargo fmt --check (rust/ workspace)' {
    Invoke-Cargo $RustDir @('fmt', '--all', '--', '--check')
}

Invoke-Step 'cargo fmt --check (client workspace)' {
    Invoke-Cargo $ClientDir @('fmt', '--all', '--', '--check')
}

Invoke-Step 'cargo fmt --check (android workspace)' {
    Invoke-Cargo $AndroidRustDir @('fmt', '--all', '--', '--check')
}

Invoke-Step 'cargo fmt --check (supernode-manager workspace)' {
    Invoke-Cargo $ManagerDir @('fmt', '--all', '--', '--check')
}

Invoke-Step 'cargo clippy (rust/ workspace, -D warnings)' {
    Invoke-Cargo $RustDir @('clippy', '--all', '--all-targets', '--', '-D', 'warnings')
}

Invoke-Step 'Release manifest signer self-test' {
    Invoke-Cargo $RustDir @(
        'run', '-p', 'doubleslash-installer', '--bin', 'sign-release-manifest', '--', '--self-test'
    )
}

if (-not $SkipTests) {
    Invoke-Step 'cargo test --all --release (rust/ workspace)' {
        Invoke-Cargo $RustDir @('test', '--all', '--release')
    }

    Invoke-Step 'cargo test (client workspace, headless)' {
        Invoke-Cargo $ClientDir @('test')
    }

    Invoke-Step 'cargo test (android workspace, host)' {
        Invoke-Cargo $AndroidRustDir @('test')
    }

    Invoke-Step 'cargo test (supernode-manager workspace)' {
        Invoke-Cargo $ManagerDir @('test')
    }
}

Invoke-Step 'cargo clippy (client workspace, headless, -D warnings)' {
    Invoke-Cargo $ClientDir @(
        'clippy', '-p', 'doubleslash-client', '--no-default-features', '--all-targets',
        '--', '-D', 'warnings'
    )
}

Invoke-Step 'cargo clippy (android workspace, host, -D warnings)' {
    Invoke-Cargo $AndroidRustDir @('clippy', '--all-targets', '--', '-D', 'warnings')
}

Invoke-Step 'cargo clippy (supernode-manager workspace, -D warnings)' {
    Invoke-Cargo $ManagerDir @('clippy', '--all-targets', '--', '-D', 'warnings')
}

# The macOS capture module is cfg-gated, so a lint inside it is invisible to
# every other platform's clippy run. This compiles it here (clippy type-checks
# without linking, so no Mac is needed) to catch those before CI does.
Invoke-Step 'cargo clippy (macOS capture module, cross-linted, -D warnings)' {
    Invoke-Cargo $ClientDir @(
        'clippy', '-p', 'doubleslash-client', '--no-default-features',
        '--features', 'lint-macos', '--', '-D', 'warnings'
    )
}

$sdkDir = Get-AndroidSdkDir
if ($sdkDir) {
    if (-not $SkipTests) {
        Invoke-Step 'Gradle testDebugUnitTest (Android SDK present)' {
            Push-Location $AndroidDir
            try {
                $env:ANDROID_HOME = $sdkDir
                $env:ANDROID_SDK_ROOT = $sdkDir
                & .\gradlew.bat testDebugUnitTest --console=plain --no-daemon
                if ($LASTEXITCODE -ne 0) {
                    throw "gradlew testDebugUnitTest failed (exit $LASTEXITCODE)"
                }
            }
            finally {
                Pop-Location
            }
        }
    }

    if ($IncludeAndroidApk -and -not $SkipTests) {
        Invoke-Step 'Gradle assembleDebug (-IncludeAndroidApk)' {
            Push-Location $AndroidDir
            try {
                $env:ANDROID_HOME = $sdkDir
                $env:ANDROID_SDK_ROOT = $sdkDir
                & .\gradlew.bat assembleDebug --console=plain --no-daemon
                if ($LASTEXITCODE -ne 0) {
                    throw "gradlew assembleDebug failed (exit $LASTEXITCODE)"
                }
            }
            finally {
                Pop-Location
            }
        }
    }
    elseif ($IncludeAndroidApk -and $SkipTests) {
        Write-Skip 'Gradle assembleDebug' '-SkipTests is set'
    }
}
else {
    Write-Skip 'Gradle Android unit tests' 'no Android SDK (local.properties sdk.dir, ANDROID_HOME, or ANDROID_SDK_ROOT)'
}

$ndkVersion = Get-AndroidNdkVersion
$ndkApi = Get-AndroidNdkApi
$ndkHome = $null
if ($sdkDir -and $ndkVersion) {
    $candidate = Join-Path $sdkDir "ndk\$ndkVersion"
    if (Test-Path $candidate) { $ndkHome = $candidate }
}

if ($SkipAndroidNdk) {
    Write-Skip 'cargo ndk clippy (aarch64-linux-android)' '-SkipAndroidNdk is set'
}
elseif (-not $ndkHome) {
    Write-Skip 'cargo ndk clippy (aarch64-linux-android)' "pinned NDK $ndkVersion not installed under the Android SDK"
}
elseif (-not (Test-CargoNdk)) {
    Write-Skip 'cargo ndk clippy (aarch64-linux-android)' 'cargo-ndk is not on PATH'
}
elseif (-not (Test-RustTarget 'aarch64-linux-android')) {
    Write-Skip 'cargo ndk clippy (aarch64-linux-android)' 'rustup target aarch64-linux-android is not installed'
}
else {
    Invoke-Step 'cargo ndk clippy (arm64-v8a JNI cdylib, -D warnings)' {
        $cmakeBin = Join-Path $sdkDir "cmake\3.31.6\bin"
        $savedPath = $env:PATH
        $savedAndroidHome = $env:ANDROID_HOME
        $savedNdkHome = $env:ANDROID_NDK_HOME
        $savedTargetDir = $env:CARGO_TARGET_DIR
        Push-Location $AndroidRustDir
        try {
            if (Test-Path $cmakeBin) {
                $env:PATH = "$cmakeBin;$env:PATH"
            }
            $env:ANDROID_HOME = $sdkDir
            $env:ANDROID_NDK_HOME = $ndkHome
            $env:CARGO_TARGET_DIR = Join-Path $RustDir 'target-android'
            & cargo ndk -t arm64-v8a --platform $ndkApi clippy --lib -- -D warnings
            if ($LASTEXITCODE -ne 0) {
                throw "cargo ndk clippy failed (exit $LASTEXITCODE)"
            }
        }
        finally {
            $env:PATH = $savedPath
            $env:ANDROID_HOME = $savedAndroidHome
            $env:ANDROID_NDK_HOME = $savedNdkHome
            $env:CARGO_TARGET_DIR = $savedTargetDir
            Pop-Location
        }
    }
}

if (-not $SkipAudit) {
    if (-not (Get-Command cargo-audit -ErrorAction SilentlyContinue)) {
        Write-Step 'Installing cargo-audit (not on PATH)'
        # Keep in sync with CARGO_AUDIT_VERSION in .github/workflows/ci.yml —
        # latest cargo-audit can require a newer rustc than $RustToolchain.
        cargo install cargo-audit --version 0.22.1 --locked
    }

    Invoke-Step 'cargo audit (rust/ workspace)' {
        Push-Location $RustDir
        try {
            cargo audit --file Cargo.lock
            if ($LASTEXITCODE -ne 0) { throw "cargo audit failed (exit $LASTEXITCODE)" }
        }
        finally {
            Pop-Location
        }
    }

    Invoke-Step 'cargo audit (client workspace)' {
        Push-Location $ClientDir
        try {
            cargo audit --file Cargo.lock
            if ($LASTEXITCODE -ne 0) { throw "cargo audit failed (exit $LASTEXITCODE)" }
        }
        finally {
            Pop-Location
        }
    }

    Invoke-Step 'cargo audit (android workspace)' {
        Push-Location $AndroidRustDir
        try {
            cargo audit --file Cargo.lock
            if ($LASTEXITCODE -ne 0) { throw "cargo audit failed (exit $LASTEXITCODE)" }
        }
        finally {
            Pop-Location
        }
    }

    Invoke-Step 'cargo audit (supernode-manager workspace)' {
        Push-Location $ManagerDir
        try {
            cargo audit --file Cargo.lock
            if ($LASTEXITCODE -ne 0) { throw "cargo audit failed (exit $LASTEXITCODE)" }
        }
        finally {
            Pop-Location
        }
    }
}

if (Get-Command wsl -ErrorAction SilentlyContinue) {
    Write-Host ""
    Write-Host "Linux native cfg (v4l/ALSA) and supernode packaging were not run from this Windows script." -ForegroundColor Yellow
    Write-Host "    WSL is available: bash scripts/ci_local.sh" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "Local CI passed." -ForegroundColor Green
