<#
.SYNOPSIS
    Runs the desktop client's UI audit: the real app, driven through its
    screens, with every visible item checked for layout and legibility faults.

.DESCRIPTION
    Starts the Qt client with --ui-audit against a throwaway profile. The
    client's UiAudit.qml creates an identity, then visits the Peers and Rooms
    lists, sample conversations, every Settings section and the dialogs and
    wizards, in each variant of the chosen matrix (base palette, skin, text
    size, window size). It checks every item on screen and writes:

      report.html   screenshots with each finding outlined, filterable
      report.json   the same data for tools
      shots/        one PNG per screen and scroll page
      stderr.txt    the client's own output, from which QML runtime warnings
                    (binding loops, TypeErrors, failed assignments) are listed

    Rules, all measured on the live scene graph:
      text-overflow, text-overflow-v  text larger than its box and not elided
      text-elided                     text cut short with an ellipsis (warn)
      text-squeezed, text-zero-size   wrapped text given no width, or no room
      text-tiny                       under 10 px at the designed size (warn)
      clipped, offscreen              text cut by a clipping container or the
                                      window edge (error), or a control's
                                      clickable area cut by 4 px or more
                                      (warn); content scrolled out of a list
                                      does not count
      escapes-dialog                  text or a control drawn outside its
                                      dialog, over the dimmed page
      overlap                         text or icons partly covering each other
                                      (floating controls over a scrolling list
                                      are exempt)
      contrast                        WCAG ratio below 4.5:1 (3:1 for large
                                      text) against the composited background;
                                      under 3:1 is an error
      input-clipped                   input text taller than its field
      small-target                    clickable area under 24 px (warn)
      unlabeled-control               icon button or toggle with no text,
                                      tooltip or accessible name (warn)
      rounded-corner, round-control   the desktop's angular rule: no radius,
                                      no Material round controls
      horizontal-scroll               a page that scrolls sideways (warn)

    Coverage limits: the Rooms tree lists only supernodes the backend knows,
    so with no peers it is audited empty (the sample node still feeds the room
    header and Settings > Network). Call, video and file-transfer states need
    a live peer and are not reached. Screenshots are renders of the item tree
    (grabToImage), not of the window: after a palette change they can show
    transparent areas, such as behind the window buttons, as white although
    the window is drawn correctly. The checks read the scene graph, not
    pixels, so findings are unaffected.

    The profile has UPnP, update checks, notifications and link previews
    turned off and no peers, so the run makes no network contact. Onboarding's
    last step (URI scheme, desktop shortcuts) is never run, and the identity's
    key is not written to the OS keyring.

    Accepted findings go in an allowlist (see -Allow), a JSON array of
    { "rule", "path", "text", "variant", "reason" } where path, text and
    variant are regular expressions and any may be omitted.

.PARAMETER Matrix
    quick (default): dark, light, text +100%, minimum window.
    full: also text -50% and +200%, light with large text in the minimum
    window, and every skin preset.

.PARAMETER Only
    Regular expression of scene names to audit, e.g. 'settings-' or
    'chat|rooms'. The first-launch passphrase screen counts as 'passphrase'.

.PARAMETER FailOn
    error (default), warn or none: which unallowed findings fail the run.

.PARAMETER Probe
    "x,y" in window coordinates. Each page then records the items painted
    under that point, top first (report.json, "probe"), for tracing where a
    colour on a screenshot comes from.

.EXAMPLE
    .\scripts\test_ui_audit.ps1 -Build
    .\scripts\test_ui_audit.ps1 -Matrix full
    .\scripts\test_ui_audit.ps1 -Only 'settings-network' -KeepProfile
#>
[CmdletBinding()]
param(
    [string] $Exe = '',
    [string] $QtBin = 'C:\Qt\6.8.3\msvc2022_64\bin',
    [string] $OutDir = '',
    [ValidateSet('quick', 'full')] [string] $Matrix = 'quick',
    [string] $Only = '',
    [string] $Allow = '',
    [ValidateSet('error', 'warn', 'none')] [string] $FailOn = 'error',
    [string] $Probe = '',
    [int] $TimeoutMinutes = 30,
    [switch] $Build,
    [switch] $KeepProfile,
    [switch] $Open
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$clientRoot = Join-Path $repoRoot 'rust/doubleslash-client'

if (-not (Test-Path (Join-Path $QtBin 'Qt6Core.dll'))) {
    throw "Qt not found at $QtBin. Pass -QtBin <Qt>\msvc2022_64\bin."
}

if ($Build) {
    $env:QMAKE = Join-Path $QtBin 'qmake6.exe'
    $env:PATH = "$QtBin;$env:PATH"
    Push-Location $clientRoot
    # Cargo reports progress on stderr, which Windows PowerShell turns into a
    # terminating error under 'Stop' when the caller redirects it.
    $ErrorActionPreference = 'Continue'
    try {
        cargo build --features qt-ui
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit $LASTEXITCODE)." }
    } finally {
        $ErrorActionPreference = 'Stop'
        Pop-Location
    }
}

if ($Exe -eq '') {
    # The client workspace builds into rust/target (target-dir = "../target").
    $Exe = Join-Path $repoRoot 'rust/target/debug/doubleslash-client.exe'
}
if (-not (Test-Path $Exe)) {
    throw "Client not found at $Exe. Build it with -Build, or pass -Exe."
}
$Exe = (Resolve-Path $Exe).Path

if ($Allow -eq '') {
    $defaultAllow = Join-Path $clientRoot 'tests/ui_audit/allow.json'
    if (Test-Path $defaultAllow) { $Allow = $defaultAllow }
}

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
if ($OutDir -eq '') {
    $OutDir = Join-Path $repoRoot "rust/target/ui-audit/$stamp"
}
New-Item -ItemType Directory -Force -Path (Join-Path $OutDir 'shots') | Out-Null
$OutDir = (Resolve-Path $OutDir).Path

# A profile of its own, so the user's identity, settings and history are
# never touched, and two runs never share one.
$profileDir = Join-Path ([System.IO.Path]::GetTempPath()) "doubleslash-ui-audit-$stamp-$PID"
New-Item -ItemType Directory -Force -Path $profileDir | Out-Null
$seed = @{
    upnp_enabled            = $false
    update_check_enabled    = $false
    notifications_enabled   = $false
    youtube_preview_enabled = $false
    auto_connect            = $false
    start_minimized         = $false
    minimize_to_tray        = $false
} | ConvertTo-Json
# No BOM: serde_json rejects one, and the client would then fall back to its
# defaults - UPnP and update checks on.
$utf8 = [System.Text.UTF8Encoding]::new($false)
[System.IO.File]::WriteAllText((Join-Path $profileDir 'settings.json'), $seed, $utf8)

$stderrPath = Join-Path $OutDir 'stderr.txt'
$stdoutPath = Join-Path $OutDir 'stdout.txt'
$reportPath = Join-Path $OutDir 'report.json'

$saved = @{}
foreach ($name in 'DOUBLESLASH_HOME', 'DOUBLESLASH_KEY_DIR', 'DOUBLESLASH_PASSPHRASE', 'DOUBLESLASH_PASSPHRASE_FILE',
                  'QML_XHR_ALLOW_FILE_WRITE', 'QML_XHR_ALLOW_FILE_READ', 'PATH',
                  'QT_QPA_PLATFORM') {
    $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}

$fwd = { param($p) $p.Replace('\', '/') }
$arguments = @("--ui-audit=$(& $fwd $OutDir)", "--ui-audit-matrix=$Matrix")
if ($Only -ne '') { $arguments += "--ui-audit-only=$Only" }
if ($Probe -ne '') { $arguments += "--ui-audit-probe=$Probe" }
if ($Allow -ne '') { $arguments += "--ui-audit-allow=$(& $fwd (Resolve-Path $Allow).Path)" }
# Start-Process joins arguments with spaces; quote each one.
$argLine = ($arguments | ForEach-Object { '"' + $_ + '"' }) -join ' '

Write-Host "UI audit: $Matrix matrix$(if ($Only) { ", scenes matching '$Only'" })"
Write-Host "  client  $Exe"
Write-Host "  profile $profileDir"
Write-Host "  output  $OutDir"

$timedOut = $false
try {
    $env:DOUBLESLASH_HOME = $profileDir
    $env:DOUBLESLASH_KEY_DIR = $profileDir
    $env:DOUBLESLASH_PASSPHRASE = $null
    $env:DOUBLESLASH_PASSPHRASE_FILE = $null
    # The real window system: on the offscreen or minimal platform there are no
    # system fonts and a made-up screen, so every text measurement is wrong.
    # A QML test run in this session may have left it set.
    $env:QT_QPA_PLATFORM = $null
    $env:QML_XHR_ALLOW_FILE_WRITE = '1'
    $env:QML_XHR_ALLOW_FILE_READ = '1'
    # The cargo build has no deployed Qt beside it; this Qt must win over any
    # other on PATH.
    $env:PATH = "$QtBin;$env:PATH"

    $proc = Start-Process -FilePath $Exe -ArgumentList $argLine -PassThru `
        -RedirectStandardError $stderrPath -RedirectStandardOutput $stdoutPath
    if (-not $proc.WaitForExit($TimeoutMinutes * 60 * 1000)) {
        $timedOut = $true
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    }
} finally {
    foreach ($name in $saved.Keys) {
        [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process')
    }
    if (-not $KeepProfile) {
        Remove-Item -Recurse -Force $profileDir -ErrorAction SilentlyContinue
    }
}

# QML runtime warnings are findings in their own right: a binding loop or a
# TypeError in a delegate is often the cause of what the audit sees.
$warnings = @()
if (Test-Path $stderrPath) {
    $warnings = @(Get-Content $stderrPath | Where-Object {
        $_ -match '\.qml:\d+|Binding loop|TypeError|ReferenceError|Unable to assign|Cannot read propert|\[ui-audit\]'
    } | Where-Object { $_ -notmatch '\[ui-audit\] done:' } | Sort-Object -Unique)
}
$warnings | Set-Content -Encoding utf8 (Join-Path $OutDir 'qml-warnings.txt')

if ($timedOut) {
    Write-Host "The client did not finish within $TimeoutMinutes minutes and was stopped." -ForegroundColor Red
}
if (-not (Test-Path $reportPath)) {
    Write-Host "No report was written. Last lines of the client's output:" -ForegroundColor Red
    if (Test-Path $stderrPath) { Get-Content $stderrPath -Tail 40 }
    exit 2
}

$report = Get-Content -Raw -Encoding utf8 $reportPath | ConvertFrom-Json
$htmlPath = Join-Path $OutDir 'report.html'
if ((Test-Path $htmlPath) -and $warnings.Count -gt 0) {
    $escaped = ($warnings | ForEach-Object { [System.Net.WebUtility]::HtmlEncode($_) }) -join "`n"
    $section = "<section><h2>QML runtime warnings ($($warnings.Count))</h2><pre>$escaped</pre></section>"
    $html = Get-Content -Raw -Encoding utf8 $htmlPath
    [System.IO.File]::WriteAllText($htmlPath, $html.Replace('<!--QML_WARNINGS-->', $section), $utf8)
}

$s = $report.summary
Write-Host ""
Write-Host ("{0} errors, {1} warnings, {2} allowed (unique findings across {3} variants)" -f `
    $s.errors, $s.warnings, $s.allowed, @($report.variants).Count) `
    -ForegroundColor $(if ($s.errors -gt 0) { 'Red' } elseif ($s.warnings -gt 0) { 'Yellow' } else { 'Green' })
$byRule = $s.byRule.PSObject.Properties | Sort-Object { -$_.Value }
foreach ($r in $byRule) { Write-Host ("  {0,-18} {1}" -f $r.Name, $r.Value) }

$openFindings = @($s.unique | Where-Object { -not $_.allowed })
$top = @($openFindings | Where-Object { $_.severity -eq 'error' } | Select-Object -First 25)
if ($top.Count -gt 0) {
    Write-Host ""
    Write-Host "Errors (first $($top.Count)):" -ForegroundColor Red
    foreach ($i in $top) {
        $text = if ($i.text) { " `"$($i.text)`"" } else { '' }
        $where = (@($i.where) | Select-Object -First 3) -join ', '
        Write-Host "  [$($i.rule)] $($i.path)$text"
        Write-Host "      $($i.message)"
        Write-Host "      worst in $($i.worst); seen in $(@($i.where).Count): $where"
    }
}
if (@($report.errors).Count -gt 0) {
    Write-Host ""
    Write-Host "Run errors:" -ForegroundColor Red
    foreach ($e in $report.errors) { Write-Host ("  [{0} / {1}] {2}" -f $e.variant, $e.scene, $e.message) }
}
if ($warnings.Count -gt 0) {
    Write-Host ""
    Write-Host "$($warnings.Count) QML runtime warnings (qml-warnings.txt)." -ForegroundColor Yellow
}
Write-Host ""
Write-Host "Report: $htmlPath"
if ($Open) { Start-Process $htmlPath }

$failed = switch ($FailOn) {
    'error' { $s.errors -gt 0 }
    'warn' { ($s.errors + $s.warnings) -gt 0 }
    default { $false }
}
if ($timedOut -or @($report.errors).Count -gt 0) { $failed = $true }
exit $(if ($failed) { 1 } else { 0 })
