# Print the product version: {major}.{minor}.{build}.0
# major.minor come from rust/doubleslash-client/Cargo.toml.
# build is DOUBLESLASH_BUILD_NUMBER, or git rev-list --count HEAD.
# The fourth component stays 0 so this string is also the Microsoft Store identity.
# A shallow clone is an error here. Build scripts stamp 0 in that case; packaging must not.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$cargo = Join-Path $root 'rust\doubleslash-client\Cargo.toml'
$text = Get-Content -LiteralPath $cargo -Raw
if ($text -notmatch '(?m)^version\s*=\s*"(\d+)\.(\d+)\.\d+"') {
    Write-Error "Cannot read the client release version from $cargo"
    exit 1
}
$major = [int]$Matches[1]
$minor = [int]$Matches[2]
if ($major -lt 1 -or $major -gt 65535 -or $minor -gt 65535) {
    Write-Error "Client version $major.$minor is outside the Windows version range."
    exit 1
}
if ($env:DOUBLESLASH_BUILD_NUMBER) {
    $build = [int]$env:DOUBLESLASH_BUILD_NUMBER
} else {
    $shallow = (& git -C $root rev-parse --is-shallow-repository).Trim()
    if ($LASTEXITCODE -ne 0) { Write-Error 'git rev-parse failed'; exit 1 }
    if ($shallow -eq 'true') {
        Write-Error 'Shallow checkout. Fetch full history or set DOUBLESLASH_BUILD_NUMBER.'
        exit 1
    }
    $build = [int](& git -C $root rev-list --count HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { Write-Error 'git rev-list failed'; exit 1 }
}
if ($build -lt 0 -or $build -gt 65535) {
    Write-Error "Build number $build exceeds the 65535 limit of Windows and Microsoft Store versions."
    exit 1
}
Write-Output "$major.$minor.$build.0"
