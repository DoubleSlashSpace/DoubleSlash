param(
    [string]$QtBin = "C:\Qt\6.8.3\msvc2022_64\bin"
)

$ErrorActionPreference = "Stop"
$clientRoot = Join-Path $PSScriptRoot "../rust/doubleslash-client"
$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("doubleslash-qml-" + [guid]::NewGuid())
$moduleRoot = Join-Path $temporaryRoot "DoubleSlash/Client"
try {
    New-Item -ItemType Directory -Path $moduleRoot -Force | Out-Null
    Copy-Item (Join-Path $clientRoot "tests/qml/qmldir") $moduleRoot
    # Every component the tests in tests/qml instantiate. The directory is run
    # as a whole, so a component missing here fails as "not a type".
    foreach ($component in @("Theme", "StyledButton", "StyledTextField",
                             "JumpToCurrentButton", "HistoryAnchor", "BackupWizard")) {
        Copy-Item (Join-Path $clientRoot "qml/$component.qml") $moduleRoot
    }
    $env:QT_QUICK_CONTROLS_STYLE = "Material"
    & (Join-Path $QtBin "qmltestrunner.exe") -input (Join-Path $clientRoot "tests/qml") -import $temporaryRoot
    if ($LASTEXITCODE -ne 0) {
        throw "Qt Quick control tests failed (exit $LASTEXITCODE)."
    }
} finally {
    Remove-Item -Recurse -Force $temporaryRoot
}