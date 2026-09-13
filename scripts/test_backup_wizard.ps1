param([string]$QtDir = 'C:/Qt/6.8.3/msvc2022_64')
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$fixtureRoot = Join-Path $repoRoot ('.tmp-run/backup-qml-' + [Guid]::NewGuid().ToString())
$moduleRoot = Join-Path $fixtureRoot 'ConquerD/Client'
New-Item -ItemType Directory -Path $moduleRoot -Force | Out-Null
# Only the backend is mocked. The wizard and theme are the shipping QML files.
# A minimal module lets qmltestrunner load them without the static Rust plugin.
Copy-Item -LiteralPath (Join-Path $repoRoot 'rust/conquerd-client/qml/BackupWizard.qml') -Destination $moduleRoot
Copy-Item -LiteralPath (Join-Path $repoRoot 'rust/conquerd-client/qml/Theme.qml') -Destination $moduleRoot
Set-Content -LiteralPath (Join-Path $moduleRoot 'qmldir') -Encoding ascii -Value @'
module ConquerD.Client
singleton Theme 1.0 Theme.qml
BackupWizard 1.0 BackupWizard.qml
'@
$env:QT_QPA_PLATFORM = 'offscreen'
& (Join-Path $QtDir 'bin/qmltestrunner.exe') -input (Join-Path $repoRoot 'rust/conquerd-client/tests/qml') -import $fixtureRoot
exit $LASTEXITCODE
