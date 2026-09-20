<#
.SYNOPSIS
    Build DoubleSlash into a portable Windows distribution (Rust + Qt).

.DESCRIPTION
    Builds doubleslash-client (--features qt-ui,webengine) and doubleslash-installer with
    cargo, runs windeployqt6 to gather Qt runtime DLLs, optionally signs
    all binaries, then produces:

        dist\DoubleSlash\                   — portable folder (copy and run)
        dist\DoubleSlash-x.y.z-win64.7z    — redistributable archive

    Requirements:
      * Rust + cargo on PATH (msvc toolchain, x86_64-pc-windows-msvc)
      * Qt 6.x MSVC install — auto-detected or set QT_DIR
      * signtool.exe in PATH for code signing (optional)
      * 7z.exe for archiving (required — install via winget/choco if absent)

    For a local build you only intend to run, set DOUBLESLASH_DEV_BUILD=1 — it
    skips the 7z archive, which is the slow part. Notices are identical either
    way; the generator finds them under packaging\licenses\ (docs/LICENSING.md).

    The bundle is assembled in dist\.DoubleSlash.staging\ and swapped into
    dist\DoubleSlash\ only once every gate passes, so a failed build leaves the
    previous working bundle intact.

    Environment variables (all optional):
      QT_DIR                  — override Qt MSVC root, e.g. C:\Qt\6.8.3\msvc2022_64
      DOUBLESLASH_DEV_BUILD      — set to "1" to skip the 7z archive
      DOUBLESLASH_DEBUG          — set to "1" to do a debug build instead of release
      DOUBLESLASH_SIGN_THUMBPRINT  — SHA-1 cert thumbprint in Windows store
      DOUBLESLASH_SIGN_PFX         — path to .pfx file
      DOUBLESLASH_SIGN_PASSWORD    — password for .pfx
      DOUBLESLASH_SIGN_TIMESTAMP   — RFC 3161 URL (default: DigiCert)
      DOUBLESLASH_SIGN_AUTO        — set to sign with best-available cert

.USAGE
    .\build_win64.ps1
    $env:QT_DIR="C:\Qt\6.8.3\msvc2022_64"; .\build_win64.ps1

#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ROOT      = $PSScriptRoot
$RUST_DIR  = Join-Path $ROOT "rust"
$CLIENT_DIR = Join-Path $RUST_DIR "doubleslash-client"
$QML_DIR   = Join-Path $CLIENT_DIR "qml"
$DIST      = Join-Path $ROOT "dist"
# The bundle is assembled in $STAGING and only swapped into $FINAL_BUNDLE once
# every gate has passed, so a failed build never destroys a working bundle.
# $BUNDLE points at the staging directory until Publish-Bundle runs.
$FINAL_BUNDLE = Join-Path $DIST "DoubleSlash"
$STAGING      = Join-Path $DIST ".DoubleSlash.staging"
$BUNDLE       = $STAGING

$PROFILE_NAME = if ($env:DOUBLESLASH_DEBUG -eq "1") { "debug" } else { "release" }
[string[]]$CARGO_ARGS = if ($PROFILE_NAME -eq "release") { @("--release") } else { @() }

# Debug console toggle: set DOUBLESLASH_DEBUG_CONSOLE=1 to keep the terminal window
# attached (enables the `console` Cargo feature which removes windows_subsystem = "windows").
$_features = "qt-ui"
if ($env:DOUBLESLASH_DEVICE_ROUTING -eq "1") {
    $_features += ",device-routing"
    Write-Host "    [preview] Simultaneous identity routing enabled"
}
if ($env:DOUBLESLASH_DEBUG_CONSOLE -eq "1") {
    $_features += ",console"
    Write-Host "    [debug] Console window enabled (DOUBLESLASH_DEBUG_CONSOLE=1)"
}
# ── Version ──────────────────────────────────────────────────────────────────
$_cargoToml = Join-Path $RUST_DIR "doubleslash-client\Cargo.toml"
$_vLine = Select-String -Path $_cargoToml -Pattern '^version\s*=\s*"([^"]+)"' |
    Select-Object -First 1
if (-not $_vLine) { Write-Error "Could not parse version from $_cargoToml" }
$VERSION = $_vLine.Matches.Groups[1].Value
Write-Host "==> DoubleSlash v$VERSION  (profile: $PROFILE_NAME)"

# ── Dev build ────────────────────────────────────────────────────────────────
# DOUBLESLASH_DEV_BUILD=1 skips the 7z archive for a local build you only mean
# to run. Notices are identical either way: the generator finds them under
# packaging\licenses\<target>\<product>\ without being told where to look.
$DEV_BUILD = $env:DOUBLESLASH_DEV_BUILD -eq "1"
if ($DEV_BUILD) {
    if ($env:DOUBLESLASH_BUILD_ID) {
        Write-Error "DOUBLESLASH_DEV_BUILD is a local-only escape hatch and cannot be used for CI/release builds."
    }
    Write-Host "    [build] DEV BUILD — no archive will be produced" -ForegroundColor Yellow
}

# ── Locate Qt ─────────────────────────────────────────────────────────────────
$QT_ROOT = $null
if ($env:QT_DIR -and (Test-Path (Join-Path $env:QT_DIR "bin\windeployqt6.exe"))) {
    $QT_ROOT = $env:QT_DIR
}
if (-not $QT_ROOT) {
    foreach ($candidate in @(
        "C:\Qt\6.8.3\msvc2022_64",
        "C:\Qt\6.8.2\msvc2022_64",
        "C:\Qt\6.7.3\msvc2022_64",
        "C:\Qt\6.7.2\msvc2022_64"
    )) {
        if (Test-Path (Join-Path $candidate "bin\windeployqt6.exe")) {
            $QT_ROOT = $candidate
            break
        }
    }
}
if (-not $QT_ROOT) {
    Write-Error @"
Qt 6 MSVC install not found.
Set QT_DIR to the msvc2022_64 root (e.g. C:\Qt\6.8.3\msvc2022_64) or install
Qt 6 via the online installer at https://www.qt.io/download.
"@
}
Write-Host "    Qt root : $QT_ROOT"
$env:PATH = "$QT_ROOT\bin;$env:PATH"
$env:QMAKE = Join-Path $QT_ROOT "bin\qmake6.exe"
Write-Host "    QMAKE   : $env:QMAKE"
$WINDEPLOYQT = Join-Path $QT_ROOT "bin\windeployqt6.exe"

function Resolve-VcInstallDir {
    if ($env:VCINSTALLDIR -and (Test-Path $env:VCINSTALLDIR)) {
        return $env:VCINSTALLDIR
    }

    $vswhereCandidates = @(
        (Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"),
        (Join-Path $env:ProgramFiles "Microsoft Visual Studio\Installer\vswhere.exe")
    )
    foreach ($vswhere in $vswhereCandidates) {
        if (-not (Test-Path $vswhere)) { continue }
        $installationPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
        if ($LASTEXITCODE -eq 0 -and $installationPath) {
            $vcDir = Join-Path $installationPath "VC"
            if (Test-Path $vcDir) { return $vcDir }
        }
    }

    return $null
}

# PowerShell Move-Item enumerates a directory's children, so a running client
# that still has icudtl.dat (or another Qt file) mapped aborts halfway and
# leaves dist\DoubleSlash\ gutted. Directory.Move is a same-volume rename:
# it either succeeds as one directory rename or fails without copying files.
function Get-BundleLockers {
    param([Parameter(Mandatory = $true)][string]$BundleDir)
    if (-not (Test-Path -LiteralPath $BundleDir)) { return @() }
    $prefix = [System.IO.Path]::GetFullPath($BundleDir).TrimEnd('\') + '\'
    return @(Get-CimInstance Win32_Process | Where-Object {
        $_.ExecutablePath -and $_.ExecutablePath.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)
    } | ForEach-Object {
        [pscustomobject]@{ Name = $_.Name; Id = $_.ProcessId; Path = $_.ExecutablePath }
    })
}

function Move-BundleDirectory {
    param(
        [Parameter(Mandatory = $true)][string]$From,
        [Parameter(Mandatory = $true)][string]$To
    )
    if (-not (Test-Path -LiteralPath $From)) {
        throw "Move-BundleDirectory source missing: $From"
    }
    if (Test-Path -LiteralPath $To) {
        throw "Move-BundleDirectory destination already exists: $To"
    }
    try {
        [System.IO.Directory]::Move($From, $To)
    } catch {
        $fromPrefix = $From.TrimEnd('\') + '\'
        $lockers = @(Get-CimInstance Win32_Process | Where-Object {
            $_.ExecutablePath -and $_.ExecutablePath.StartsWith($fromPrefix, [System.StringComparison]::OrdinalIgnoreCase)
        } | ForEach-Object { "{0} ({1})" -f $_.Name, $_.ProcessId })
        $hint = if ($lockers.Count -gt 0) {
            " Processes using the bundle: $($lockers -join ', ')."
        } else {
            ""
        }
        throw "Failed to rename '$From' -> '$To': $_$hint Close DoubleSlash and retry."
    }
}

function Get-RetiredBundlePath {
    $base = Join-Path $DIST ".DoubleSlash.previous"
    if (-not (Test-Path -LiteralPath $base)) { return $base }
    try {
        Remove-Item -LiteralPath $base -Recurse -Force -ErrorAction Stop
    } catch {
        # A prior swap left this tree in use by a still-running client.
    }
    if (-not (Test-Path -LiteralPath $base)) { return $base }
    $stamp = Get-Date -Format 'yyyyMMddHHmmss'
    return Join-Path $DIST ".DoubleSlash.previous.$stamp"
}

$_earlyLockers = @(Get-BundleLockers $FINAL_BUNDLE)
if ($_earlyLockers.Count -gt 0) {
    $names = ($_earlyLockers | ForEach-Object { "{0} ({1})" -f $_.Name, $_.Id }) -join ', '
    Write-Host "    [warn] dist\DoubleSlash is in use by: $names" -ForegroundColor Yellow
    Write-Host "           Live folder will not be replaced until those processes exit." -ForegroundColor Yellow
}

# Include the Qt WebEngine (Chromium) scheme handler for the in-app node portal
# (doubleslash:// custom scheme). Auto-detected from the Qt install. Override with
# DOUBLESLASH_NO_WEBENGINE=1 to force-disable (e.g. Qt WebEngine not installed).
$_weProbe = Join-Path $QT_ROOT "include\QtWebEngineCore\QWebEngineProfile.h"
if ($env:DOUBLESLASH_NO_WEBENGINE -ne "1" -and (Test-Path $_weProbe)) {
    $_features += ",webengine"
    Write-Host "    [web] Qt WebEngine portal enabled"
} elseif ($env:DOUBLESLASH_NO_WEBENGINE -eq "1") {
    Write-Host "    [web] Qt WebEngine portal disabled (DOUBLESLASH_NO_WEBENGINE=1)"
} else {
    Write-Host "    [web] Qt WebEngine NOT found — portal disabled" -ForegroundColor Yellow
    Write-Host "         Install via Qt Maintenance Tool: Qt 6.x > Additional Libraries > Qt WebEngine" -ForegroundColor Yellow
    if ($env:DOUBLESLASH_BUILD_ID) {
        Write-Error @"
CI/release builds require Qt WebEngine for the in-app supernode portal.
Install the module, e.g.:
  pwsh scripts/install_qt_windows.ps1
"@
    }
}

# ── Build doubleslash-client (Qt UI) ─────────────────────────────────────────────
Write-Host "`n==> Building doubleslash-client ($PROFILE_NAME)..."
# doubleslash-client is its own workspace root (rust/doubleslash-client/) so that the
# Windows-local cxx-qt patch does not affect server-side builds on Linux.
# Wrap in try/catch to absorb the spurious NativeCommandError PS 7+ raises
# when any native process writes to stderr, even on success.
Push-Location $CLIENT_DIR
$_prevPref = $ErrorActionPreference; $ErrorActionPreference = "Continue"
& cargo build @CARGO_ARGS --features $_features
$_clientExit = $LASTEXITCODE
$ErrorActionPreference = $_prevPref
Pop-Location
if ($_clientExit -ne 0) { Write-Error "cargo build doubleslash-client failed (exit $_clientExit)" }

function Get-ClientSourceHash {
    $outGlob = Join-Path $RUST_DIR "target\$PROFILE_NAME\build\doubleslash-client-*\output"
    $outFile = Get-ChildItem -Path $outGlob -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $outFile) { return $null }
    foreach ($line in Get-Content $outFile.FullName) {
        if ($line -match 'cargo:rustc-env=DOUBLESLASH_SOURCE_HASH=(.+)') {
            return $Matches[1]
        }
    }
    return $null
}

# Optional second pass: bake DOUBLESLASH_RELEASE_PROOF for official CI/local release builds.
if ($env:DOUBLESLASH_RELEASE_SIGN_KEY -and (Test-Path $env:DOUBLESLASH_RELEASE_SIGN_KEY)) {
    $sourceHash = Get-ClientSourceHash
    $buildId = if ($env:DOUBLESLASH_BUILD_ID) {
        $env:DOUBLESLASH_BUILD_ID
    } else {
        $tag = (& git -C $ROOT describe --tags --exact-match HEAD 2>$null)
        $sha = (& git -C $ROOT rev-parse --short=12 HEAD 2>$null)
        if ($tag) { "release-$($tag -replace '^v','')-$sha" } else { $sha }
    }
    Write-Host "`n==> Signing release build claim (build_id=$buildId)..."
    $claimArgs = @(
        "run", "-p", "doubleslash-installer", "--manifest-path", (Join-Path $RUST_DIR "Cargo.toml"),
        "--bin", "sign-release-manifest", "--",
        "--sign-build-claim",
        "--private-key", $env:DOUBLESLASH_RELEASE_SIGN_KEY,
        "--build-id", $buildId,
        "--claim-version", $VERSION
    )
    if ($sourceHash) { $claimArgs += @("--source-hash", $sourceHash) }
    Push-Location $ROOT
    $_prevPrefClaim = $ErrorActionPreference; $ErrorActionPreference = "Continue"
    $proof = & cargo @claimArgs 2>$null
    $_claimExit = $LASTEXITCODE
    $ErrorActionPreference = $_prevPrefClaim
    Pop-Location
    if ($_claimExit -ne 0 -or -not $proof) {
        Write-Error "Failed to sign release build claim (exit $_claimExit)"
    }
    $env:DOUBLESLASH_RELEASE_PROOF = $proof.Trim()
    Write-Host "    Rebuilding doubleslash-client with DOUBLESLASH_RELEASE_PROOF..."
    Push-Location $CLIENT_DIR
    $_prevPref4 = $ErrorActionPreference; $ErrorActionPreference = "Continue"
    & cargo build @CARGO_ARGS --features $_features
    $_clientExit2 = $LASTEXITCODE
    $ErrorActionPreference = $_prevPref4
    Pop-Location
    if ($_clientExit2 -ne 0) { Write-Error "cargo rebuild with release proof failed (exit $_clientExit2)" }
}

$CLIENT_EXE = Join-Path $RUST_DIR "target\$PROFILE_NAME\doubleslash-client.exe"
if (-not (Test-Path $CLIENT_EXE)) {
    Write-Error "doubleslash-client.exe not found at $CLIENT_EXE"
}

# ── Build doubleslash-installer ──────────────────────────────────────────────────
Write-Host "`n==> Building doubleslash-installer ($PROFILE_NAME)..."
Push-Location $RUST_DIR
$_prevPref2 = $ErrorActionPreference; $ErrorActionPreference = "Continue"
& cargo build @CARGO_ARGS -p doubleslash-installer
$_installerExit = $LASTEXITCODE
$ErrorActionPreference = $_prevPref2
Pop-Location
if ($_installerExit -ne 0) { Write-Error "cargo build doubleslash-installer failed (exit $_installerExit)" }

$INSTALLER_EXE = Join-Path $RUST_DIR "target\$PROFILE_NAME\doubleslash-installer.exe"
if (-not (Test-Path $INSTALLER_EXE)) {
    Write-Error "doubleslash-installer.exe not found at $INSTALLER_EXE"
}

# ── Prepare staging folder ────────────────────────────────────────────────────
# Only the staging directory is cleaned here. dist\DoubleSlash\ is left alone
# until Publish-Bundle swaps the finished staging tree into place.
Write-Host "`n==> Preparing $(Split-Path $STAGING -Leaf)\..."
$resolvedRoot = [System.IO.Path]::GetFullPath($ROOT).TrimEnd('\') + '\'
$resolvedBundle = [System.IO.Path]::GetFullPath($BUNDLE)
if (-not $resolvedBundle.StartsWith($resolvedRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
    Write-Error "Refusing to clean bundle path outside the workspace: $resolvedBundle"
}
if (Test-Path $BUNDLE) {
    Remove-Item -LiteralPath $resolvedBundle -Recurse -Force
    if (Test-Path $BUNDLE) {
        Write-Error "Failed to remove $BUNDLE -- close any running DoubleSlash processes and retry."
    }
}
New-Item -ItemType Directory -Path $BUNDLE | Out-Null

$BUNDLE_EXE       = Join-Path $BUNDLE "DoubleSlash.exe"
$BUNDLE_INSTALLER = Join-Path $BUNDLE "doubleslash-installer.exe"
Copy-Item $CLIENT_EXE    $BUNDLE_EXE
Copy-Item $INSTALLER_EXE $BUNDLE_INSTALLER
Write-Host "    Copied binaries"

# ── Chromium third-party credits ─────────────────────────────────────────────
# Qt compiles Chromium into Qt WebEngine Core but does not ship its credits
# resource, so the notices for the components inside that snapshot have to be
# generated from the qtwebengine-chromium revision this Qt pins. That is about
# 23,000 lines of licence text, so it is generated into packaging/licenses/
# rather than committed, and cached: only a Qt version change re-fetches it.
# This has to run before the notices below, which copy the supplement in.
if ($_features -like "*webengine*") {
    $_qtVersion   = (& (Join-Path $QT_ROOT "bin\qmake6.exe") -query QT_VERSION | Out-String).Trim()
    $_creditsFile = Join-Path $ROOT "packaging\licenses\x86_64-pc-windows-msvc\client\chromium-third-party-credits.txt"
    $_cached = (Test-Path -LiteralPath $_creditsFile) -and
        (Select-String -LiteralPath $_creditsFile -SimpleMatch "qtwebengine v$_qtVersion," -Quiet)
    if ($_cached) {
        Write-Host "    Chromium credits cached for Qt $_qtVersion"
    } else {
        Write-Host "    Generating Chromium third-party credits for Qt $_qtVersion (fetches ~20 MB)..."
        & python (Join-Path $ROOT "scripts\licenses\chromium_credits.py") `
            --qt-tag "v$_qtVersion" --output $_creditsFile
        if ($LASTEXITCODE -ne 0) {
            Write-Error @"
Chromium credits generation failed.

Qt WebEngine bundles Chromium, so the package owes its third-party notices and
cannot be shipped without them. This step needs Python and network access to
github.com. Build with DOUBLESLASH_NO_WEBENGINE=1 to drop Qt WebEngine instead.
See docs/LICENSING.md.
"@
        }
    }
}

$licenseArgs = @(
    (Join-Path $ROOT "scripts\generate_licenses.mjs"),
    "--product", "client", "--target", "x86_64-pc-windows-msvc",
    "--features", $_features, "--output", (Join-Path $BUNDLE "licenses\client")
)
& node @licenseArgs
if ($LASTEXITCODE -ne 0) { Write-Error "Client license generation failed; see docs/LICENSING.md" }
& node (Join-Path $ROOT "scripts\generate_licenses.mjs") --product installer --target x86_64-pc-windows-msvc --output (Join-Path $BUNDLE "licenses\installer")
if ($LASTEXITCODE -ne 0) { Write-Error "Installer license generation failed" }
Copy-Item (Join-Path $BUNDLE "licenses\installer\rust-licenses.html") (Join-Path $DIST "doubleslash-installer-win64-licenses.html")
Copy-Item (Join-Path $ROOT "LICENSE") (Join-Path $BUNDLE "LICENSE.txt")

# ── windeployqt6 ─────────────────────────────────────────────────────────────
Write-Host "`n==> Running windeployqt6..."
$vcInstallDir = Resolve-VcInstallDir
$compilerRuntimeArg = if ($vcInstallDir) {
    $env:VCINSTALLDIR = $vcInstallDir
    Write-Host "    VC runtime deployment enabled: $vcInstallDir"
    "--compiler-runtime"
} else {
    Write-Host "    VC runtime deployment skipped (Visual C++ tools not found)"
    "--no-compiler-runtime"
}
$_prevPref3 = $ErrorActionPreference; $ErrorActionPreference = "Continue"
$windeployArgs = @(
    "--qmldir", $QML_DIR,
    "--no-translations",
    "--skip-plugin-types", "position",
    $compilerRuntimeArg,
    $BUNDLE_EXE
)
& $WINDEPLOYQT @windeployArgs
$_wdqtExit = $LASTEXITCODE
$ErrorActionPreference = $_prevPref3
if ($_wdqtExit -ne 0) { Write-Error "windeployqt6 failed (exit code $_wdqtExit)" }
Write-Host "    Qt runtime deployed"

if ($_features -like "*webengine*") {
    $qtWebEngineResources = Join-Path $QT_ROOT "resources"
    $bundleWebEngineResources = Join-Path $BUNDLE "resources"
    if (Test-Path (Join-Path $qtWebEngineResources "qtwebengine_resources.pak")) {
        Copy-Item $qtWebEngineResources $bundleWebEngineResources -Recurse -Force
        Write-Host "    Qt WebEngine resources deployed"
    } else {
        Write-Warning "Qt WebEngine resources not found under $qtWebEngineResources"
    }
    $qtLocales = Join-Path $QT_ROOT "translations\qtwebengine_locales"
    $bundleLocales = Join-Path $BUNDLE "locales"
    if (Test-Path $qtLocales) {
        Copy-Item $qtLocales $bundleLocales -Recurse -Force
        Write-Host "    Qt WebEngine locales deployed"
    } else {
        # aqt installs Chromium locale packs under resources/ on some Qt builds.
        $altLocales = Join-Path $QT_ROOT "resources\locales"
        if (Test-Path $altLocales) {
            Copy-Item $altLocales $bundleLocales -Recurse -Force
            Write-Host "    Qt WebEngine locales deployed (from resources/locales)"
        }
    }
}

# ── Code-sign binaries (optional) ─────────────────────────────────────────────
#
#   DOUBLESLASH_SIGN_THUMBPRINT  -- SHA-1 thumbprint of a cert in the Windows Store
#   DOUBLESLASH_SIGN_PFX         -- path to a .pfx file (OV cert, local builds)
#   DOUBLESLASH_SIGN_PASSWORD    -- password for the .pfx file
#   DOUBLESLASH_SIGN_TIMESTAMP   -- RFC 3161 URL (default: DigiCert)
#
# If none are set the signing step is skipped (development builds).

$_signtool     = Get-Command "signtool" -ErrorAction SilentlyContinue
$_signThumb    = $env:DOUBLESLASH_SIGN_THUMBPRINT
$_signPfx      = $env:DOUBLESLASH_SIGN_PFX
$_signPassword = $env:DOUBLESLASH_SIGN_PASSWORD
$_timestampUrl = if ($env:DOUBLESLASH_SIGN_TIMESTAMP) { $env:DOUBLESLASH_SIGN_TIMESTAMP } `
                 else { "http://timestamp.digicert.com" }

function Invoke-SignBinary ([string]$Path) {
    $signArgs = @("sign", "/fd", "SHA256", "/tr", $_timestampUrl, "/td", "SHA256")
    if ($_signThumb) {
        $signArgs += @("/sha1", $_signThumb)
    } elseif ($_signPfx) {
        $signArgs += @("/f", $_signPfx)
        if ($_signPassword) { $signArgs += @("/p", $_signPassword) }
    } else {
        $signArgs += "/a"
    }
    $signArgs += $Path
    & $_signtool.Source @signArgs
    if ($LASTEXITCODE -ne 0) { Write-Error "signtool failed for $Path" }
}

$_doSign = $_signtool -and ($_signThumb -or $_signPfx -or $env:DOUBLESLASH_SIGN_AUTO)
if ($_doSign) {
    Write-Host "`n==> Code-signing binaries..."
    foreach ($bin in @($BUNDLE_EXE, $BUNDLE_INSTALLER)) {
        Write-Host "    Signing: $(Split-Path $bin -Leaf)"
        Invoke-SignBinary $bin
    }
    Write-Host "    Code signing complete."
} elseif (-not $_signtool) {
    Write-Host "`n    [sign] signtool.exe not found -- install Windows SDK to enable code signing"
} else {
    Write-Host "`n    [sign] Skipped -- set DOUBLESLASH_SIGN_THUMBPRINT or DOUBLESLASH_SIGN_PFX to sign"
}

# ── Publish staging -> dist\DoubleSlash\ ──────────────────────────────────────
# Every gate has passed by this point, so the previous bundle can be replaced.
# The old tree is moved aside first and only deleted once the new one is in
# place, so an interrupted swap leaves a recoverable directory behind.
Write-Host "`n==> Publishing to dist\DoubleSlash\..."
$_lockers = @(Get-BundleLockers $FINAL_BUNDLE)
$_liveSwap = $true
if ($_lockers.Count -gt 0) {
    $_liveSwap = $false
    $names = ($_lockers | ForEach-Object { "{0} ({1})" -f $_.Name, $_.Id }) -join ', '
    Write-Host "    Skipping live folder swap; in use by: $names" -ForegroundColor Yellow
    Write-Host "    Archive will be built from staging. Close those processes and re-run to update dist\DoubleSlash\." -ForegroundColor Yellow
} else {
    $_retired = Get-RetiredBundlePath
    if (Test-Path -LiteralPath $FINAL_BUNDLE) {
        Move-BundleDirectory $FINAL_BUNDLE $_retired
    }
    try {
        Move-BundleDirectory $STAGING $FINAL_BUNDLE
    } catch {
        if ((Test-Path -LiteralPath $_retired) -and -not (Test-Path -LiteralPath $FINAL_BUNDLE)) {
            try { [System.IO.Directory]::Move($_retired, $FINAL_BUNDLE) } catch { }
        }
        Write-Error "Failed to publish bundle: $_"
    }
    if (Test-Path -LiteralPath $_retired) {
        try {
            Remove-Item -LiteralPath $_retired -Recurse -Force -ErrorAction Stop
        } catch {
            Write-Host "    Left $_retired in place (still in use by a running DoubleSlash process)"
        }
    }
    $BUNDLE = $FINAL_BUNDLE
    Write-Host "    Published"
}

$BUNDLE_EXE       = Join-Path $BUNDLE "DoubleSlash.exe"
$BUNDLE_INSTALLER = Join-Path $BUNDLE "doubleslash-installer.exe"

# ── Copy installer to dist\ root (run-alongside-archive entry point) ──────────
$DIST_INSTALLER = Join-Path $DIST "doubleslash-installer.exe"
Copy-Item $INSTALLER_EXE $DIST_INSTALLER -Force
Write-Host "`n    Copied doubleslash-installer.exe to dist\ (detect-archive entry point)"

# ── Create .7z archive ────────────────────────────────────────────────────────
# The installer downloads this 7z from GitHub Releases for updates.
# It contains the full self-contained portable `DoubleSlash/` folder (exe + Qt runtime + QML + resources).
$archiveName = "DoubleSlash-${VERSION}-win64.7z"
$archivePath = Join-Path $DIST $archiveName

if ($DEV_BUILD) {
    Write-Host "`n==> Skipping 7z archive (dev build)"
} else {

$sevenZip = Get-Command "7z" -ErrorAction SilentlyContinue
if (-not $sevenZip) {
    # Common locations on GitHub runners and dev machines
    $candidates = @(
        "C:\Program Files\7-Zip\7z.exe",
        "C:\Program Files (x86)\7-Zip\7z.exe",
        "${env:ProgramFiles}\7-Zip\7z.exe"
    )
    foreach ($c in $candidates) {
        if (Test-Path $c) {
            $sevenZip = @{ Source = $c }
            break
        }
    }
}
if (-not $sevenZip) {
    Write-Error @"
7z.exe not found. Install 7-Zip before building release archives:
  winget install --id 7zip.7zip
  choco install 7zip -y
Release archives must be non-solid (-ms=off) so the installer's embedded sevenz-rust decoder can extract every file.
"@
}
Write-Host "`n==> Creating 7z archive with 7-Zip (non-solid, installer-compatible)..."
# `7z a` adds to an archive that already exists: same-path files are replaced,
# but anything the bundle no longer contains is kept. Without this delete a
# release archive accumulates every file any previous build ever put in it - a
# dropped DLL, a renamed executable, a notice set that was deliberately
# removed - and `verify_artifact.mjs` cannot catch it, because it proves that
# the promised notices are present, never that nothing else is.
Remove-Item -LiteralPath $archivePath -Force -ErrorAction SilentlyContinue
& $sevenZip.Source a -t7z -mx=9 -ms=off $archivePath "$BUNDLE\*" | Out-Null
if ($LASTEXITCODE -ne 0) { Write-Error "7z failed to create archive" }
& node (Join-Path $ROOT 'scripts/licenses/verify_artifact.mjs') $archivePath client installer
if ($LASTEXITCODE -ne 0) { throw 'Final Windows archive license validation failed' }

if (Test-Path $archivePath) {
    Write-Host "    Archive ready: $archivePath"
}

}

# ── Report results ────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "==> Build successful!" -ForegroundColor Green

$dirSize = [math]::Round(
    (Get-ChildItem $BUNDLE -Recurse | Measure-Object -Property Length -Sum).Sum / 1MB, 0)
Write-Host "    Launcher  : $BUNDLE_EXE"
Write-Host "    Installer : $BUNDLE_INSTALLER"
Write-Host "    Folder    : $BUNDLE\"
Write-Host "    Size      : $dirSize MB"
if (-not $_liveSwap) {
    Write-Host "    Live folder not updated (DoubleSlash still running). Staging left at $STAGING" -ForegroundColor Yellow
}

# Guarded on -not $DEV_BUILD, not on Test-Path: a dev build must not report or
# re-checksum an archive left behind by an earlier release build.
if (-not $DEV_BUILD -and (Test-Path $archivePath)) {
    $archiveSize = [math]::Round((Get-Item $archivePath).Length / 1MB, 1)
    $sha     = (Get-FileHash $archivePath -Algorithm SHA256).Hash.ToLower()
    $shaFile = Join-Path $DIST "$archiveName.sha256"
    "$sha  $archiveName" | Set-Content $shaFile -NoNewline
    Write-Host "    Archive   : $archivePath ($archiveSize MB)"
    Write-Host "    SHA-256   : $sha"
}

if ($DEV_BUILD) {
    Write-Host ""
    Write-Host "    *** DEV BUILD - no archive was produced ***" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "    DoubleSlash v${VERSION}" -ForegroundColor Cyan
