<#
.SYNOPSIS
    Build the x64 DoubleSlash MSIX for Microsoft Store submission.
.DESCRIPTION
    Uses the portable build's Qt deployment and license generation, then packages
    the client without its standalone installer. MakeAppx validates the manifest;
    the final MSIX is extracted and checked for distribution notices. The output
    is unsigned: Microsoft signs it after Store certification. This script does
    not install the package, change certificate trust, or submit it to the Store.
.PARAMETER SkipBuild
    Reuse dist/DoubleSlash after building the current sources with build_win64.ps1.
    Intended for packaging iteration, not producing a release from stale binaries.
.PARAMETER Version
    Product version, major.minor.build.0. Defaults to scripts/app_version.ps1.
    The last component must be 0. That is the Microsoft Store package identity.
#>
[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [string]$Version,
    [string]$MakeAppxPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repoRoot = $PSScriptRoot
$distRoot = Join-Path $repoRoot 'dist'
$bundle = Join-Path $distRoot 'DoubleSlash'
$outputRoot = Join-Path $distRoot 'msix'
$template = Join-Path $repoRoot 'packaging/msix/AppxManifest.xml.in'
if (-not $Version) {
    $Version = (& (Join-Path $repoRoot 'scripts\app_version.ps1')).Trim()
}
if ($Version -notmatch '^\d+\.\d+\.\d+\.0$') {
    throw "Product version must be major.minor.build.0 (got $Version)."
}
if (@($Version.Split('.') | Where-Object { [int]$_ -gt 65535 }).Count -gt 0 -or [int]($Version.Split('.')[0]) -lt 1) {
    throw 'Store version components must be 0-65535 and the major component must be positive.'
}

if (-not $MakeAppxPath) {
    $sdkBin = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/bin'
    $sdk = Get-ChildItem -LiteralPath $sdkBin -Directory |
        Where-Object { $_.Name -match '^10\.0\.\d+\.0$' } |
        Sort-Object { [version]$_.Name } -Descending |
        Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'x64/makeappx.exe') } |
        Select-Object -First 1
    if (-not $sdk) { throw 'Install the Windows 10/11 SDK with MakeAppx.exe.' }
    $MakeAppxPath = Join-Path $sdk.FullName 'x64/makeappx.exe'
}
if (-not (Test-Path -LiteralPath $MakeAppxPath -PathType Leaf)) { throw 'MakeAppx.exe was not found.' }

if (-not $SkipBuild) {
    $previousDevBuild = $env:DOUBLESLASH_DEV_BUILD
    try {
        # Skip the unrelated portable archive; Qt deployment and notice checks run.
        $env:DOUBLESLASH_DEV_BUILD = '1'
        & (Join-Path $repoRoot 'build_win64.ps1')
        $builtExe = Join-Path $repoRoot 'rust/target/release/doubleslash-client.exe'
        if ($env:DOUBLESLASH_DEBUG -eq '1') { throw 'Store packages require a release build.' }
        $builtHash = (Get-FileHash -LiteralPath $builtExe -Algorithm SHA256).Hash
        # build_win64 retains fresh staging when a running client locks dist.
        $freshBundle = @($bundle, (Join-Path $distRoot '.DoubleSlash.staging')) |
            Where-Object {
                $exe = Join-Path $_ 'DoubleSlash.exe'
                (Test-Path -LiteralPath $exe) -and ((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -eq $builtHash)
            } | Select-Object -First 1
        if (-not $freshBundle) { throw 'The deployed bundle does not contain the client just built.' }
        $bundle = $freshBundle
    } finally {
        $env:DOUBLESLASH_DEV_BUILD = $previousDevBuild
    }
}

$clientExe = Join-Path $bundle 'DoubleSlash.exe'
if (-not (Test-Path -LiteralPath $clientExe)) { throw 'Build dist/DoubleSlash first.' }
$fileVersion = (Get-Item -LiteralPath $clientExe).VersionInfo.ProductVersion
if ($fileVersion.Trim() -ne $Version) {
    throw "Bundle version '$fileVersion' is not $Version. Rebuild it."
}
foreach ($required in @('Qt6WebEngineCore.dll', 'QtWebEngineProcess.exe', 'resources/qtwebengine_resources.pak', 'licenses/client/inventory.json')) {
    if (-not (Test-Path -LiteralPath (Join-Path $bundle $required))) { throw "Incomplete bundle: $required" }
}

New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
$stage = Join-Path $outputRoot ('.staging-' + [guid]::NewGuid().ToString('N'))
$priWork = "$stage.pri"
New-Item -ItemType Directory -Path $stage | Out-Null
try {
    # Preserve the deployed layout. Exclude the standalone updater, its notices,
    # and the redistributable installer: an MSIX must not run setup programs.
    foreach ($entry in Get-ChildItem -LiteralPath $bundle) {
        if ($entry.Name -in @('doubleslash-installer.exe', 'vc_redist.x64.exe', 'vc_redist.x86.exe')) { continue }
        if ($entry.Name -eq 'licenses') {
            $licenses = Join-Path $stage 'licenses'
            New-Item -ItemType Directory -Path $licenses | Out-Null
            Copy-Item -LiteralPath (Join-Path $entry.FullName 'client') -Destination $licenses -Recurse
        } else {
            Copy-Item -LiteralPath $entry.FullName -Destination $stage -Recurse
        }
    }

    # App-local VC runtime DLLs replace the portable bundle's setup executable.
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $vsInstall = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($LASTEXITCODE -ne 0 -or -not $vsInstall) { throw 'Visual C++ x64 runtime tools were not found.' }
    $redistRoot = Join-Path $vsInstall 'VC/Redist/MSVC'
    $crt = Get-ChildItem -LiteralPath $redistRoot -Directory |
        Where-Object { $_.Name -match '^\d+\.\d+\.\d+$' } |
        Sort-Object { [version]$_.Name } -Descending |
        ForEach-Object { Get-ChildItem -Path (Join-Path $_.FullName 'x64/Microsoft.VC*.CRT') -Directory } |
        Select-Object -First 1
    if (-not $crt) { throw 'Visual C++ x64 CRT redistributable DLLs were not found.' }
    Get-ChildItem -LiteralPath $crt.FullName -Filter '*.dll' | Copy-Item -Destination $stage

    $assets = Join-Path $stage 'Assets'
    New-Item -ItemType Directory -Path $assets | Out-Null
    Add-Type -AssemblyName System.Drawing
    # The project's ICO stores PNG frames. Decode the largest frame directly:
    # .NET Framework Icon.ToBitmap cannot decode its PNG-compressed 256px frame.
    $iconBytes = [System.IO.File]::ReadAllBytes((Join-Path $repoRoot 'assets/doubleslash.ico'))
    $imageCount = [BitConverter]::ToUInt16($iconBytes, 4)
    $frames = for ($index = 0; $index -lt $imageCount; $index++) {
        $entryOffset = 6 + 16 * $index
        $width = [int]$iconBytes[$entryOffset]
        if ($width -eq 0) { $width = 256 }
        [pscustomobject]@{
            Width = $width
            Length = [int][BitConverter]::ToUInt32($iconBytes, $entryOffset + 8)
            Offset = [int][BitConverter]::ToUInt32($iconBytes, $entryOffset + 12)
        }
    }
    $frame = $frames | Sort-Object Width -Descending | Select-Object -First 1
    if (-not $frame -or $frame.Offset + $frame.Length -gt $iconBytes.Length) { throw 'Invalid project icon.' }
    $iconStream = [System.IO.MemoryStream]::new($iconBytes, $frame.Offset, $frame.Length, $false)
    $source = [System.Drawing.Image]::FromStream($iconStream)
    try {
        foreach ($logo in @(@('StoreLogo', 50), @('Square44x44Logo', 44), @('Square150x150Logo', 150))) {
            $size = [int]$logo[1]
            $bitmap = New-Object System.Drawing.Bitmap($size, $size)
            $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
            try {
                $graphics.Clear([System.Drawing.Color]::Transparent)
                $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                $graphics.DrawImage($source, 0, 0, $size, $size)
                $bitmap.Save((Join-Path $assets ($logo[0] + '.png')), [System.Drawing.Imaging.ImageFormat]::Png)
            } finally { $graphics.Dispose(); $bitmap.Dispose() }
        }
    } finally { $source.Dispose(); $iconStream.Dispose() }

    # The taskbar and Start list draw the 44px logo from target-size variants.
    # Without an unplated one, Windows sets the icon on a plate of the tile
    # colour, and BackgroundColor "transparent" makes that the accent colour.
    # Light mode looks for lightunplated before falling back to the plate.
    # Copy the ICO's own frames at those sizes rather than resampling.
    foreach ($targetSize in @(16, 24, 32, 48, 256)) {
        $exact = $frames | Where-Object { $_.Width -eq $targetSize } | Select-Object -First 1
        if (-not $exact -or $exact.Offset + $exact.Length -gt $iconBytes.Length -or $iconBytes[$exact.Offset] -ne 0x89) {
            throw "The project icon has no PNG ${targetSize}px frame."
        }
        $pngBytes = [byte[]]::new($exact.Length)
        [Array]::Copy($iconBytes, $exact.Offset, $pngBytes, 0, $exact.Length)
        foreach ($altForm in @('unplated', 'lightunplated')) {
            [System.IO.File]::WriteAllBytes((Join-Path $assets "Square44x44Logo.targetsize-${targetSize}_altform-$altForm.png"), $pngBytes)
        }
    }

    $manifest = (Get-Content -LiteralPath $template -Raw).Replace('@VERSION@', $Version)
    [System.IO.File]::WriteAllText((Join-Path $stage 'AppxManifest.xml'), $manifest, (New-Object System.Text.UTF8Encoding($false)))

    # Qualified asset names resolve only through resources.pri. Index the
    # Assets folder alone: the folder indexer would otherwise treat the whole
    # Qt bundle as resources. Paths stay relative to the package root.
    $makePriPath = Join-Path (Split-Path -Parent $MakeAppxPath) 'makepri.exe'
    if (-not (Test-Path -LiteralPath $makePriPath -PathType Leaf)) { throw "MakePri.exe was not found beside $MakeAppxPath." }
    $priRoot = Join-Path $priWork 'root'
    New-Item -ItemType Directory -Path $priRoot -Force | Out-Null
    Copy-Item -LiteralPath $assets -Destination $priRoot -Recurse
    $priConfig = Join-Path $priWork 'priconfig.xml'
    & $makePriPath createconfig /cf $priConfig /dq en-US /pv 10.0.0 /o
    if ($LASTEXITCODE -ne 0) { throw "MakePri createconfig failed (exit $LASTEXITCODE)." }
    & $makePriPath new /pr $priRoot /cf $priConfig /mn (Join-Path $stage 'AppxManifest.xml') /of (Join-Path $stage 'resources.pri') /o
    if ($LASTEXITCODE -ne 0) { throw "MakePri failed (exit $LASTEXITCODE)." }
    $package = Join-Path $outputRoot "DoubleSlash-$Version-x64.msix"
    $candidate = Join-Path $outputRoot ('.candidate-' + [guid]::NewGuid().ToString('N') + '.msix')
    & $MakeAppxPath pack /d $stage /p $candidate /o
    if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed (exit $LASTEXITCODE)." }
    & node (Join-Path $repoRoot 'scripts/licenses/verify_artifact.mjs') $candidate client
    if ($LASTEXITCODE -ne 0) { throw 'Final MSIX license verification failed.' }
    Move-Item -LiteralPath $candidate -Destination $package -Force
    $checksum = (Get-FileHash -LiteralPath $package -Algorithm SHA256).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText("$package.sha256", "$checksum  $([System.IO.Path]::GetFileName($package))`n")
    Write-Host "Store submission package: $package"
    Write-Host 'Unsigned for Store submission; local installation requires separate test signing.'
} finally {
    # Only remove the fresh, resolved staging directory under this build's output.
    $outputPrefix = [System.IO.Path]::GetFullPath($outputRoot).TrimEnd('\') + '\'
    foreach ($scratch in @($stage, $priWork)) {
        $resolvedScratch = [System.IO.Path]::GetFullPath($scratch)
        if (-not $resolvedScratch.StartsWith($outputPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to clean staging outside $outputRoot"
        }
        if (Test-Path -LiteralPath $resolvedScratch) { Remove-Item -LiteralPath $resolvedScratch -Recurse -Force }
    }
}
