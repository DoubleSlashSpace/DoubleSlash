<#
.SYNOPSIS
    Drive and collect evidence for acceptance section 1 (group-key convergence, live).

.DESCRIPTION
    Runs several desktop profiles side by side on this machine and merges their
    group-key log lines into one timeline. Room creation, invites, joining voice,
    and talking stay manual: see docs/ACCEPTANCE.md section 1 for the steps.

    Each profile runs from its own copy of dist\DoubleSlash under
    <Root>\bin\<Profile>, so a firewall rule can cut one profile's network
    without touching the others. The copies stay out of dist\DoubleSlash on
    purpose: build_msix.ps1 packages everything in that folder.

    The client truncates its log on every start, so Start first moves the
    previous session's log aside as logs\run-<time>.log. Report reads those
    too, which keeps the pre-kill half of a kill/restart step.

    Every action is written to <Root>\actions.log and shows up in the
    timeline beside the log lines, so a rotation can be matched to the step
    that caused it. Use -Action Mark to note manual steps.

.EXAMPLE
    .\scripts\acceptance_group_key.ps1 -Action Start              # A, B and C
    .\scripts\acceptance_group_key.ps1 -Action Mark -Note "C left (rotation 1)"
    .\scripts\acceptance_group_key.ps1 -Action Kill -Profile B
    .\scripts\acceptance_group_key.ps1 -Action Start -Profile B
    .\scripts\acceptance_group_key.ps1 -Action Cut -Profile B     # elevated shell
    .\scripts\acceptance_group_key.ps1 -Action Restore -Profile B
    .\scripts\acceptance_group_key.ps1 -Action Report
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Start', 'Kill', 'Cut', 'Restore', 'Freeze', 'Thaw', 'Mark', 'Status', 'Report', 'Cleanup')]
    [string]$Action,

    # Profiles the action applies to. Start, Kill and Status default to all three.
    [ValidatePattern('^[A-Za-z0-9_-]+$')]
    [string[]]$Profile = @('A', 'B', 'C'),

    [string]$Root = 'Z:\ds-accept',

    # Free text for -Action Mark.
    [string]$Note,

    # Profiles that must end on the same epoch for Report's verdict. C is the
    # one that joins and leaves to force rotations, so it is not expected to.
    [string[]]$Members = @('A', 'B'),

    # A key request answered more slowly than this is flagged in Report.
    [double]$RecoverySeconds = 5,

    # How long Cut waits for the profile's log to show the connection dropping.
    [int]$VerifySeconds = 45,

    # Log at info instead of debug. The group-key lines are info either way.
    [switch]$InfoOnly,

    # With Cleanup, also delete the per-profile bundle copies.
    [switch]$RemoveBinaries
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$bundle = Join-Path $repoRoot 'dist\DoubleSlash'
$firewallGroup = 'DoubleSlash acceptance'
$actionsLog = Join-Path $Root 'actions.log'

function Get-ProfileHome([string]$name) { Join-Path $Root $name }
function Get-ProfileBin([string]$name) { Join-Path (Join-Path $Root 'bin') $name }
function Get-ProfileExe([string]$name) { Join-Path (Get-ProfileBin $name) 'DoubleSlash.exe' }
function Get-ProfileLogDir([string]$name) { Join-Path (Get-ProfileHome $name) 'logs' }

function Write-ActionLog([string]$text) {
    New-Item -ItemType Directory -Force -Path $Root | Out-Null
    $stamp = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.ffffffZ')
    Add-Content -LiteralPath $actionsLog -Value "$stamp $text" -Encoding UTF8
}

function Get-ProfileProcesses([string]$name) {
    $bin = Get-ProfileBin $name
    @(Get-Process -Name 'DoubleSlash', 'QtWebEngineProcess' -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and $_.Path.StartsWith($bin + '\', [StringComparison]::OrdinalIgnoreCase) })
}

function Get-MainProcess([string]$name) {
    $exe = Get-ProfileExe $name
    @(Get-ProfileProcesses $name | Where-Object { $_.Path -ieq $exe }) | Select-Object -First 1
}

function Test-Admin {
    $principal = New-Object Security.Principal.WindowsPrincipal ([Security.Principal.WindowsIdentity]::GetCurrent())
    $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Sync-ProfileBundle([string]$name) {
    if (-not (Test-Path -LiteralPath (Join-Path $bundle 'DoubleSlash.exe'))) {
        throw "No packaged client at $bundle. Build it with build_win64.ps1 first; a Cargo exe has no Qt beside it."
    }
    $bin = Get-ProfileBin $name
    New-Item -ItemType Directory -Force -Path $bin | Out-Null
    # Incremental mirror: a rebuilt dist is picked up, an unchanged one costs seconds.
    & robocopy.exe $bundle $bin /MIR /NFL /NDL /NJH /NJS /NP /R:2 /W:1 `
        /XF 'doubleslash-installer.exe' 'vc_redist.x64.exe' 'vc_redist.x86.exe' | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "robocopy failed copying the bundle for profile $name (exit $LASTEXITCODE)." }
    $global:LASTEXITCODE = 0
}

function Get-IdentityLine([string]$name) {
    $logDir = Get-ProfileLogDir $name
    if (-not (Test-Path -LiteralPath $logDir)) { return $null }
    $files = @(Get-ChildItem -LiteralPath $logDir -Filter '*.log' | Sort-Object LastWriteTime -Descending)
    foreach ($file in $files) {
        $hit = Select-String -LiteralPath $file.FullName -Pattern 'Identity unlocked: (\S+) \((\S+)\)' -Encoding UTF8 |
            Select-Object -First 1
        if ($hit) { return $hit.Matches[0].Groups[1].Value }
    }
    $null
}

function Get-Identities([string[]]$names) {
    $ids = [ordered]@{}
    foreach ($name in $names) {
        $id = Get-IdentityLine $name
        if ($id) { $ids[$name] = $id }
    }
    $ids
}

# The client elects the member whose public id sorts lowest, byte-wise, with
# base64 padding ignored (room_session.rs `elected_keyer_for`).
function Get-OrdinalKeyerOrder($ids) {
    $names = New-Object System.Collections.Generic.List[string]
    foreach ($k in $ids.Keys) { $names.Add([string]$k) }
    $names.Sort([Comparison[string]] {
            param($x, $y)
            [string]::CompareOrdinal($ids[$x].TrimEnd('='), $ids[$y].TrimEnd('='))
        })
    , $names.ToArray()
}

function Start-Profile([string]$name) {
    if (Get-MainProcess $name) {
        Write-Host "$name is already running."
        return
    }
    Sync-ProfileBundle $name
    $home_ = Get-ProfileHome $name
    $logDir = Get-ProfileLogDir $name
    New-Item -ItemType Directory -Force -Path $logDir | Out-Null

    # The client truncates its log on start; keep the previous session.
    $current = Join-Path $logDir 'doubleslash-client.log'
    if (Test-Path -LiteralPath $current) {
        $stamp = (Get-Item -LiteralPath $current).LastWriteTimeUtc.ToString('yyyyMMdd-HHmmss')
        $archive = Join-Path $logDir "run-$stamp.log"
        $n = 1
        while (Test-Path -LiteralPath $archive) { $archive = Join-Path $logDir "run-$stamp-$n.log"; $n++ }
        Move-Item -LiteralPath $current -Destination $archive
    }

    $saved = @{
        DOUBLESLASH_HOME    = $env:DOUBLESLASH_HOME
        DOUBLESLASH_KEY_DIR = $env:DOUBLESLASH_KEY_DIR
        RUST_LOG            = $env:RUST_LOG
    }
    try {
        $env:DOUBLESLASH_HOME = $home_
        $env:DOUBLESLASH_KEY_DIR = $home_
        if ($InfoOnly) { $env:RUST_LOG = 'doubleslash_client=info,warn' }
        else { $env:RUST_LOG = 'doubleslash_client=debug,doubleslash_opus=info,warn' }
        $proc = Start-Process -FilePath (Get-ProfileExe $name) -WorkingDirectory (Get-ProfileBin $name) -PassThru
    }
    finally {
        foreach ($key in $saved.Keys) {
            if ($null -eq $saved[$key]) { Remove-Item -LiteralPath "Env:$key" -ErrorAction SilentlyContinue }
            else { Set-Item -LiteralPath "Env:$key" -Value $saved[$key] }
        }
    }
    Write-ActionLog "[action] start $name (pid $($proc.Id))"
    Write-Host "Started $name (pid $($proc.Id)), home $home_"
}

function Stop-Profile([string]$name) {
    $procs = Get-ProfileProcesses $name
    if ($procs.Count -eq 0) {
        Write-Host "$name is not running."
        return
    }
    # A hard kill on purpose: Section 1 step 4 kills the process, and closing the
    # window would only send it to the tray.
    $procs | Stop-Process -Force
    Write-ActionLog "[action] kill $name"
    Write-Host "Killed $name."
}

function Invoke-NetworkCut([string]$name) {
    if (-not (Test-Admin)) { throw 'Cut needs an elevated PowerShell (firewall rules). Freeze works without one, but is not the same test.' }
    $exe = Get-ProfileExe $name
    if (-not (Test-Path -LiteralPath $exe)) { throw "Profile $name has no bundle copy yet; Start it first." }
    $display = "$firewallGroup`: cut $name"
    foreach ($direction in 'Outbound', 'Inbound') {
        New-NetFirewallRule -DisplayName "$display ($direction)" -Group $firewallGroup -Direction $direction `
            -Program $exe -Action Block -Profile Any | Out-Null
    }
    $cutAt = [DateTime]::UtcNow
    Write-ActionLog "[action] cut network for $name"
    Write-Host "Blocked $name's network. Waiting up to $VerifySeconds s for its log to show the drop..."

    # A new block rule is not guaranteed to end flows that were already open,
    # so confirm the cut from the client's own log rather than assume it.
    $current = Join-Path (Get-ProfileLogDir $name) 'doubleslash-client.log'
    $pattern = 'WebSocket closed|attempting rejoin|connection lost|timed out|Disconnected'
    $deadline = $cutAt.AddSeconds($VerifySeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Seconds 3
        $hit = Select-String -LiteralPath $current -Pattern $pattern -Encoding UTF8 |
            Where-Object { (ConvertTo-LogTime $_.Line) -gt $cutAt } | Select-Object -First 1
        if ($hit) {
            Write-Host "Confirmed: $($hit.Line)"
            return
        }
    }
    Write-Warning "No disconnect in $name's log after $VerifySeconds s. The firewall may not have ended its open connections; do not count this step until the cut is confirmed (run B on another machine, or unplug it)."
}

function Undo-NetworkCut([string]$name) {
    if (-not (Test-Admin)) { throw 'Restore needs an elevated PowerShell.' }
    $rules = @(Get-NetFirewallRule -Group $firewallGroup -ErrorAction SilentlyContinue |
        Where-Object { $_.DisplayName -like "*: cut $name (*" })
    if ($rules.Count -eq 0) { Write-Host "No cut rule for $name."; return }
    $rules | Remove-NetFirewallRule
    Write-ActionLog "[action] restore network for $name"
    Write-Host "Restored $name's network."
}

function Set-Frozen([string]$name, [bool]$frozen) {
    if (-not ('DsAcceptProc' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class DsAcceptProc {
    [DllImport("ntdll.dll")] public static extern int NtSuspendProcess(IntPtr handle);
    [DllImport("ntdll.dll")] public static extern int NtResumeProcess(IntPtr handle);
}
'@
    }
    $proc = Get-MainProcess $name
    if (-not $proc) { throw "$name is not running." }
    if ($frozen) { $status = [DsAcceptProc]::NtSuspendProcess($proc.Handle) }
    else { $status = [DsAcceptProc]::NtResumeProcess($proc.Handle) }
    if ($status -ne 0) { throw ('NtSuspend/ResumeProcess failed: 0x{0:X8}' -f $status) }
    $verb = if ($frozen) { 'freeze' } else { 'thaw' }
    Write-ActionLog "[action] $verb $name"
    Write-Host "$verb $name done."
}

function ConvertTo-LogTime([string]$line) {
    if ($line -match '^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?Z)') {
        return [DateTime]::Parse($Matches[1], [Globalization.CultureInfo]::InvariantCulture,
            [Globalization.DateTimeStyles]::AdjustToUniversal -bor [Globalization.DateTimeStyles]::AssumeUniversal)
    }
    [DateTime]::MinValue
}

function Show-Status {
    $ids = Get-Identities $Profile
    foreach ($name in $Profile) {
        $proc = Get-MainProcess $name
        $state = if ($proc) { "running (pid $($proc.Id))" } else { 'stopped' }
        $id = if ($ids.Contains($name)) { $ids[$name].Substring(0, 8) } else { '(no identity yet)' }
        Write-Host ("{0,-3} {1,-22} {2}" -f $name, $state, $id)
    }
    if ($ids.Count -ge 2) {
        $order = Get-OrdinalKeyerOrder $ids
        Write-Host ''
        Write-Host "Keyer order (lowest id first): $($order -join ' < ')"
        Write-Host "With everyone present the keyer is $($order[0]). Capture that profile's log above all."
    }
    if (Test-Admin) {
        $rules = @(Get-NetFirewallRule -Group $firewallGroup -ErrorAction SilentlyContinue)
        if ($rules.Count -gt 0) { Write-Host "Active cut rules: $(($rules | ForEach-Object DisplayName) -join '; ')" }
    }
}

function New-Report {
    $ids = Get-Identities $Profile
    if ($ids.Count -eq 0) { throw "No profile under $Root has logged an identity yet." }

    # 8-character id prefix -> profile label, the form the group-key lines use.
    $labels = @{}
    foreach ($name in $ids.Keys) { $labels[$ids[$name].Substring(0, 8)] = $name }

    $pattern = '\[group-key\]|Identity unlocked|doubleslash-client \S+ starting|WebSocket closed|attempting rejoin|SFU joining room'
    $events = New-Object System.Collections.Generic.List[object]
    foreach ($name in $Profile) {
        $logDir = Get-ProfileLogDir $name
        if (-not (Test-Path -LiteralPath $logDir)) { continue }
        $files = @(Get-ChildItem -LiteralPath $logDir -Filter '*.log' | Where-Object { $_.Name -like 'run-*' -or $_.Name -eq 'doubleslash-client.log' })
        foreach ($file in $files) {
            foreach ($hit in (Select-String -LiteralPath $file.FullName -Pattern $pattern -Encoding UTF8)) {
                $time = ConvertTo-LogTime $hit.Line
                if ($time -eq [DateTime]::MinValue) { continue }
                $text = ($hit.Line -replace '^\S+\s+\S+\s+\S+:\s', '')
                $events.Add([pscustomobject]@{ Time = $time; Who = $name; Text = $text })
            }
        }
    }
    if (Test-Path -LiteralPath $actionsLog) {
        foreach ($line in (Get-Content -LiteralPath $actionsLog -Encoding UTF8)) {
            $time = ConvertTo-LogTime $line
            if ($time -eq [DateTime]::MinValue) { continue }
            $events.Add([pscustomobject]@{ Time = $time; Who = '>>'; Text = ($line -replace '^\S+\s', '') })
        }
    }
    $sorted = @($events | Sort-Object Time)

    function Format-Ids([string]$text) {
        foreach ($prefix in $labels.Keys) { $text = $text -replace [regex]::Escape($prefix), $labels[$prefix] }
        $text
    }

    # Walk the timeline once, tracking each profile's view of each room.
    $finalEpoch = @{}      # "who|room" -> last epoch that profile held or minted
    $askSince = @{}        # "who|room" -> time of the first unanswered key request
    $rejoinAt = @{}        # "who|room" -> voice rejoin of a room that profile had keyed before
    $recoveries = New-Object System.Collections.Generic.List[object]
    $flags = New-Object System.Collections.Generic.List[string]
    $departures = New-Object System.Collections.Generic.List[string]
    $rooms = New-Object System.Collections.Generic.HashSet[string]

    foreach ($e in $sorted) {
        $t = $e.Text
        if ($e.Who -eq '>>') { continue }
        # Recovery after a restart is timed from the voice rejoin, not the
        # process start, so unlocking and reopening the room do not count.
        if ($t -match "SFU joining room '([^']+)'") {
            $key = "$($e.Who)|$($Matches[1].Substring(0, [Math]::Min(8, $Matches[1].Length)))"
            if ($finalEpoch.ContainsKey($key)) { $rejoinAt[$key] = $e.Time }
            continue
        }

        $room = $null; $epoch = $null
        if ($t -match 'installed epoch (\d+) for room (\S+) from') { $epoch = $Matches[1]; $room = $Matches[2] }
        elseif ($t -match 'minting epoch (\d+) for room (\S+) to') { $epoch = $Matches[1]; $room = $Matches[2] }
        elseif ($t -match 'rotating room (\S+) to epoch (\d+)') { $room = $Matches[1]; $epoch = $Matches[2] }
        elseif ($t -match 'room (\S+) moved ahead of us; minting epoch (\d+)') { $room = $Matches[1]; $epoch = $Matches[2] }
        elseif ($t -match "asked for room (\S+)'s key; sealing epoch (\d+)") { $room = $Matches[1]; $epoch = $Matches[2] }

        if ($t -match 'no key for room (\S+); asking') {
            $key = "$($e.Who)|$($Matches[1])"
            if (-not $askSince.ContainsKey($key)) { $askSince[$key] = $e.Time }
        }
        if ($t -match 'keyer view of room (\S+) changed via (\S+): -\[([^\]]*)\]') {
            $gone = $Matches[3]
            if ($gone) { $departures.Add(("{0:HH:mm:ss.fff}  {1} dropped [{2}] from room {3} via node {4}" -f $e.Time, $e.Who, (Format-Ids $gone), $Matches[1], $Matches[2])) }
        }
        if ($t -match 'rejecting key epoch|giving up waiting for ack|not electing|malformed key|could not seal|could not canonicalize') {
            $flags.Add(("{0:HH:mm:ss.fff}  {1}  {2}" -f $e.Time, $e.Who, (Format-Ids $t)))
        }

        if ($room) {
            [void]$rooms.Add($room)
            $key = "$($e.Who)|$room"
            $finalEpoch[$key] = [int]$epoch
            if ($t -match '^\[group-key\] installed') {
                if ($askSince.ContainsKey($key)) {
                    $recoveries.Add([pscustomobject]@{ Who = $e.Who; Room = $room; Kind = 'key request'; Seconds = ($e.Time - $askSince[$key]).TotalSeconds; At = $e.Time; Epoch = $epoch })
                    $askSince.Remove($key)
                }
                if ($rejoinAt.ContainsKey($key)) {
                    $recoveries.Add([pscustomobject]@{ Who = $e.Who; Room = $room; Kind = 'rejoin to key'; Seconds = ($e.Time - $rejoinAt[$key]).TotalSeconds; At = $e.Time; Epoch = $epoch })
                    $rejoinAt.Remove($key)
                }
            }
        }
    }

    $out = New-Object System.Collections.Generic.List[string]
    $out.Add("Group-key convergence report, $([DateTime]::UtcNow.ToString('u'))")
    $out.Add("Repo HEAD $(& git -C $repoRoot rev-parse --short HEAD 2>$null); bundle $((Get-Item -LiteralPath (Join-Path $bundle 'DoubleSlash.exe')).LastWriteTime)")
    $out.Add('')
    $out.Add('Identities')
    foreach ($name in $ids.Keys) { $out.Add("  $name  $($ids[$name])") }
    if ($ids.Count -ge 2) {
        $order = Get-OrdinalKeyerOrder $ids
        $out.Add("  Keyer order: $($order -join ' < ')  (the keyer is the lowest id present)")
    }

    $out.Add('')
    $out.Add('Final epoch per room')
    $verdicts = New-Object System.Collections.Generic.List[string]
    foreach ($room in $rooms) {
        $cells = foreach ($name in $Profile) {
            $key = "$name|$room"
            if ($finalEpoch.ContainsKey($key)) { "$name=$($finalEpoch[$key])" } else { "$name=-" }
        }
        $out.Add("  room $room  $($cells -join '  ')")
        $memberEpochs = @($Members | ForEach-Object { $k = "$_|$room"; if ($finalEpoch.ContainsKey($k)) { $finalEpoch[$k] } })
        if ($memberEpochs.Count -eq $Members.Count) {
            $distinct = @($memberEpochs | Sort-Object -Unique)
            if ($distinct.Count -eq 1) { $verdicts.Add("  room ${room}: $($Members -join ' and ') agree on epoch $($distinct[0])") }
            else { $verdicts.Add("  room ${room}: MISMATCH, $($Members -join '/') ended on $($memberEpochs -join '/')") }
        }
    }
    $out.Add('')
    $out.Add("Epoch verdict (members: $($Members -join ', '))")
    if ($verdicts.Count -eq 0) { $out.Add('  No room has epoch lines from every member yet.') } else { $out.AddRange($verdicts) }

    $out.Add('')
    $out.Add("Recovery times (flagged over $RecoverySeconds s)")
    if ($recoveries.Count -eq 0) { $out.Add('  None recorded.') }
    foreach ($r in $recoveries) {
        $mark = if ($r.Seconds -gt $RecoverySeconds) { '  SLOW' } else { '' }
        $out.Add(("  {0:HH:mm:ss.fff}  {1}  room {2}  {3,-12} {4,7:N1} s  -> epoch {5}{6}" -f $r.At, $r.Who, $r.Room, $r.Kind, $r.Seconds, $r.Epoch, $mark))
    }
    $pending = @($askSince.Keys)
    foreach ($key in $pending) {
        $parts = $key.Split('|')
        $out.Add(("  {0} room {1}: STILL ASKING since {2:HH:mm:ss.fff}, never installed" -f $parts[0], $parts[1], $askSince[$key]))
    }
    foreach ($key in @($rejoinAt.Keys)) {
        $parts = $key.Split('|')
        $out.Add(("  {0} room {1}: rejoined at {2:HH:mm:ss.fff} and has not installed a key since (fine if it is the keyer)" -f $parts[0], $parts[1], $rejoinAt[$key]))
    }

    $out.Add('')
    $out.Add('Members dropped from a keyer''s view (expected only when you killed, cut, or removed that profile)')
    if ($departures.Count -eq 0) { $out.Add('  None.') } else { $departures | ForEach-Object { $out.Add("  $_") } }

    $out.Add('')
    $out.Add('Warnings')
    if ($flags.Count -eq 0) { $out.Add('  None.') } else { $flags | ForEach-Object { $out.Add("  $_") } }

    $summaryLines = $out.Count
    $out.Add('')
    $out.Add('Timeline (UTC; >> marks script actions and notes)')
    foreach ($e in $sorted) {
        # Identity lines keep the full id: they are the legend for the labels.
        $text = if ($e.Text -like 'Identity unlocked*') { $e.Text } else { Format-Ids $e.Text }
        $out.Add(("{0:HH:mm:ss.fff}  {1,-2}  {2}" -f $e.Time, $e.Who, $text))
    }

    $reportDir = Join-Path $Root 'reports'
    New-Item -ItemType Directory -Force -Path $reportDir | Out-Null
    $path = Join-Path $reportDir ("group-key-{0}.txt" -f [DateTime]::Now.ToString('yyyyMMdd-HHmmss'))
    Set-Content -LiteralPath $path -Value $out -Encoding UTF8
    $out | Select-Object -First $summaryLines | ForEach-Object { Write-Host $_ }
    Write-Host ''
    Write-Host "Full timeline: $path"
}

if ($Action -in 'Cut', 'Restore', 'Freeze', 'Thaw' -and -not $PSBoundParameters.ContainsKey('Profile')) {
    throw "$Action needs -Profile; it would otherwise apply to every profile."
}

switch ($Action) {
    'Start' { foreach ($name in $Profile) { Start-Profile $name } }
    'Kill' { foreach ($name in $Profile) { Stop-Profile $name } }
    'Cut' { foreach ($name in $Profile) { Invoke-NetworkCut $name } }
    'Restore' { foreach ($name in $Profile) { Undo-NetworkCut $name } }
    'Freeze' { foreach ($name in $Profile) { Set-Frozen $name $true } }
    'Thaw' { foreach ($name in $Profile) { Set-Frozen $name $false } }
    'Mark' {
        if (-not $Note) { throw 'Mark needs -Note.' }
        Write-ActionLog "[note] $Note"
        Write-Host "Noted: $Note"
    }
    'Status' { Show-Status }
    'Report' { New-Report }
    'Cleanup' {
        if (Test-Admin) { Get-NetFirewallRule -Group $firewallGroup -ErrorAction SilentlyContinue | Remove-NetFirewallRule }
        else { Write-Warning 'Not elevated: any cut rules were left in place.' }
        if ($RemoveBinaries) {
            foreach ($name in $Profile) {
                if (Get-ProfileProcesses $name) { throw "$name is still running; Kill it first." }
            }
            Remove-Item -LiteralPath (Join-Path $Root 'bin') -Recurse -Force -ErrorAction SilentlyContinue
            Write-Host 'Removed bundle copies. Profiles and logs are kept.'
        }
    }
}
