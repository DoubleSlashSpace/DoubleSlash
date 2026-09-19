<#
.SYNOPSIS
    Build and package doubleslash-supernode for Windows x86_64.

.DESCRIPTION
    Produces:
      dist\doubleslash-supernode-X.X.X-win64.zip
      dist\doubleslash-supernode-X.X.X-win64.zip.sha256

    Run from the repository root:

        powershell -ExecutionPolicy Bypass -File scripts\build_supernode.ps1

        $env:DOUBLESLASH_RELEASE = '1'
        $env:DOUBLESLASH_BUILD_ID = 'release-1.0.0-abc123'
        powershell -ExecutionPolicy Bypass -File scripts\build_supernode.ps1
#>

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$Root = Split-Path -Parent $PSScriptRoot
$RustDir = Join-Path $Root 'rust'
$Dist = Join-Path $Root 'dist'

$Version = (Select-String -Path (Join-Path $RustDir 'doubleslash-supernode\Cargo.toml') -Pattern '^version\s*=' | Select-Object -First 1).Line -replace '.*"(.*)".*', '$1'
$Platform = 'win64'

$Profile = 'debug'
$CargoArgs = @('build', '-p', 'doubleslash-supernode')
if ($env:DOUBLESLASH_RELEASE -eq '1' -or $env:DOUBLESLASH_DEBUG -ne '1') {
    $Profile = 'release'
    $CargoArgs += '--release'
}

Write-Host "==> Building doubleslash-supernode v$Version for $Platform (profile: $Profile)"

Push-Location $RustDir
try {
    & cargo @CargoArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed (exit $LASTEXITCODE)"
    }
}
finally {
    Pop-Location
}

$Binary = Join-Path $RustDir "target\$Profile\doubleslash-supernode.exe"
if (-not (Test-Path $Binary)) {
    throw "Expected binary at $Binary"
}

$StagingName = "doubleslash-supernode-$Version-$Platform"
$Staging = Join-Path $Dist $StagingName
$Archive = Join-Path $Dist "$StagingName.zip"

New-Item -ItemType Directory -Force -Path $Dist | Out-Null
if (Test-Path $Staging) {
    Remove-Item -Recurse -Force $Staging
}
New-Item -ItemType Directory -Force -Path $Staging | Out-Null
Copy-Item $Binary (Join-Path $Staging 'doubleslash-supernode.exe') -Force

& node (Join-Path $Root 'scripts\generate_licenses.mjs') --product supernode --target x86_64-pc-windows-msvc --output (Join-Path $Staging 'licenses')
if ($LASTEXITCODE -ne 0) { throw 'Supernode license generation failed' }

if (Test-Path $Archive) {
    Remove-Item -Force $Archive
}
Compress-Archive -Path $Staging -DestinationPath $Archive -CompressionLevel Optimal
& node (Join-Path $Root 'scripts/licenses/verify_artifact.mjs') $Archive supernode
if ($LASTEXITCODE -ne 0) { throw 'Final supernode archive license validation failed' }
Remove-Item -Recurse -Force $Staging

$Hash = (Get-FileHash -Path $Archive -Algorithm SHA256).Hash.ToLower()
Set-Content -Path "$Archive.sha256" -Value "$Hash  $(Split-Path $Archive -Leaf)" -NoNewline

Write-Host "==> Package ready: $Archive"
Get-Content "$Archive.sha256"
