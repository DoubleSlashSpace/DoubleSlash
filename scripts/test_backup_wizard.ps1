param([string]$QtDir = 'C:/Qt/6.8.3/msvc2022_64')
$ErrorActionPreference = 'Stop'
# Only the backend is mocked; the wizard and its components are the shipping
# QML files. The test sits beside tests/qml/qmldir, which qmltestrunner loads
# as the test's own module, so every type that qmldir names has to be staged.
# test_qml_controls.ps1 keeps that list, so this runs the one file through it
# rather than keeping a second list that drifts (it had, and failed with
# "Type Icons unavailable").
$previousPlatform = $env:QT_QPA_PLATFORM
$env:QT_QPA_PLATFORM = 'offscreen'
try {
    & (Join-Path $PSScriptRoot 'test_qml_controls.ps1') -QtBin (Join-Path $QtDir 'bin') -TestFile 'tst_BackupWizard.qml'
} finally {
    # Left set, it sends every later Qt program in this shell (the client,
    # the UI audit) to the offscreen platform.
    $env:QT_QPA_PLATFORM = $previousPlatform
}
