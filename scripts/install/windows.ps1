# Hex's installer for Windows: the one admin step (native plan step 6; docs/design/runtime.md, decision
# "the installer, per OS"; sources in docs/research/runtime/installer-per-os.md). It turns on the
# Windows features Hex's classes need beyond the process class (whose job objects need no admin):
#   - Containers, for the oci class's Windows images through the Host Compute Service;
#   - HypervisorPlatform, for the Shards guest that carries Linux images;
# and puts the hex binary at %ProgramFiles%\Hex\hex.exe. Windows may ask for one reboot to finish
# turning the features on; the installer asks before restarting.
# Every thing it makes is a line of its plan ("<kind> <target>"), written to the manifest before it is
# made; uninstall undoes the manifest's lines and then lists what is left, which is nothing (a feature
# turned off finishes at the reboot Windows asks for).
#
# usage: windows.ps1 install -Binary <hex.exe> [-DryRun] [-NoRestart]
#        windows.ps1 uninstall [-DryRun] [-NoRestart]
#        windows.ps1 check [<plan file>]
# Run from a PowerShell; install and uninstall ask for elevation once (UAC) when not already elevated.
# -DryRun prints the plan and makes nothing; check lists what of a plan (default: the manifest's, else
# the fixed paths) is still on the machine and exits 1 when anything is.
param(
  [Parameter(Mandatory = $true, Position = 0)][ValidateSet('install', 'uninstall', 'check')][string]$Verb,
  [Parameter(Position = 1)][string]$PlanFile,
  [string]$Binary,
  [switch]$DryRun,
  [switch]$NoRestart
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3

$Dir = Join-Path $env:ProgramFiles 'Hex'
$Bin = Join-Path $Dir 'hex.exe'
$State = Join-Path $env:ProgramData 'Hex'
$Manifest = Join-Path $State 'manifest'
$Features = @('Containers', 'HypervisorPlatform')

function Die([string]$Why) {
  [Console]::Error.WriteLine("windows.ps1: $Why")
  exit 1
}

# Whether an optional feature is on, read without elevation (Win32_OptionalFeature, InstallState 1).
function Test-Feature([string]$Name) {
  $f = Get-CimInstance -ClassName Win32_OptionalFeature -Filter "Name='$Name'"
  return ($null -ne $f) -and ($f.InstallState -eq 1)
}

# The plan: every thing install makes, one "<kind> <target>" line each.
function Get-Plan {
  foreach ($f in $Features) { if (-not (Test-Feature $f)) { "feature $f" } }
  if (-not (Test-Path -LiteralPath $Dir)) { "folder $Dir" }
  "file $Bin"
  if (-not (Test-Path -LiteralPath $State)) { "folder $State" }
  "file $Manifest"
}

function Test-Present([string]$Kind, [string]$Target) {
  switch ($Kind) {
    'feature' { return (Test-Feature $Target) }
    'file' { return (Test-Path -LiteralPath $Target -PathType Leaf) }
    'folder' { return (Test-Path -LiteralPath $Target -PathType Container) }
    default { Die "unknown plan line kind $Kind" }
  }
}

function Invoke-Check([string[]]$Lines) {
  $left = 0
  foreach ($line in $Lines) {
    if (-not $line) { continue }
    $kind, $target = $line -split ' ', 2
    if (Test-Present $kind $target) {
      Write-Host "left: $line"
      $left = 1
    }
  }
  # The listing goes to the host, so the function's one output is its status.
  if ($left -eq 0) { Write-Host "nothing of Hex's is left" }
  return $left
}

function Test-Admin {
  $me = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
  return $me.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

# Runs this script again elevated (one UAC prompt) with the same arguments, and ends with its status.
function Invoke-Elevated {
  $argv = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"", $Verb)
  if ($Binary) { $argv += @('-Binary', "`"$((Resolve-Path -LiteralPath $Binary).Path)`"") }
  if ($NoRestart) { $argv += '-NoRestart' }
  $p = Start-Process -FilePath (Get-Process -Id $PID).Path -Verb RunAs -Wait -PassThru -ArgumentList $argv
  exit $p.ExitCode
}

function Request-Restart([bool]$Needed) {
  if (-not $Needed) { return }
  Write-Output 'Windows asks for one reboot to finish.'
  if ($NoRestart) { Write-Output 'Restart when convenient (-NoRestart).'; return }
  Restart-Computer -Confirm
}

function Install-Hex {
  if (-not $Binary) { Die 'install needs -Binary <hex.exe>' }
  if (-not (Test-Path -LiteralPath $Binary -PathType Leaf)) { Die "$Binary isn't a file" }
  if ($DryRun) { Get-Plan; return }
  if (-not (Test-Admin)) { Invoke-Elevated }
  if (Test-Path -LiteralPath $Manifest) { Die "Hex is installed already ($Manifest); uninstall first" }
  $before = @{}
  Get-WindowsOptionalFeature -Online | ForEach-Object { $before[$_.FeatureName] = "$($_.State)" }
  $plan = @(Get-Plan)
  New-Item -ItemType Directory -Force -Path $State | Out-Null
  # The plan is the manifest from the start, so an install cut short is undone by uninstall too.
  Set-Content -LiteralPath $Manifest -Value $plan -Encoding utf8
  New-Item -ItemType Directory -Force -Path $Dir | Out-Null
  Copy-Item -LiteralPath $Binary -Destination $Bin -Force
  $restart = $false
  foreach ($line in $plan) {
    $kind, $target = $line -split ' ', 2
    if ($kind -ne 'feature') { continue }
    $r = Enable-WindowsOptionalFeature -Online -FeatureName $target -All -NoRestart
    if ($r.RestartNeeded) { $restart = $true }
  }
  # -All turns on the features a feature needs too: each one it turned on is install's to undo.
  $also = Get-WindowsOptionalFeature -Online | Where-Object {
    "$($_.State)" -like 'Enable*' -and $before[$_.FeatureName] -notlike 'Enable*' -and $Features -notcontains $_.FeatureName
  } | ForEach-Object { "feature $($_.FeatureName)" }
  if ($also) { Add-Content -LiteralPath $Manifest -Value $also -Encoding utf8 }
  Write-Output "installed: $Bin; uninstall with: windows.ps1 uninstall"
  Request-Restart $restart
}

function Uninstall-Hex {
  $lines = if (Test-Path -LiteralPath $Manifest) { @(Get-Content -LiteralPath $Manifest) } else { @(Get-Plan) }
  if ($DryRun) { $lines; return }
  if (-not (Test-Admin)) { Invoke-Elevated }
  $restart = $false
  # Features off in the reverse of the order they came on (a feature -All brought is listed after its own).
  [array]::Reverse($lines)
  foreach ($line in $lines) {
    $kind, $target = $line -split ' ', 2
    switch ($kind) {
      'feature' {
        $r = Disable-WindowsOptionalFeature -Online -FeatureName $target -NoRestart
        if ($r.RestartNeeded) { $restart = $true }
      }
      'file' { if ($target -ne $Manifest) { Remove-Item -LiteralPath $target -Force -ErrorAction SilentlyContinue } }
      'folder' { }
      default { Die "unknown manifest line kind $kind" }
    }
  }
  Remove-Item -LiteralPath $Manifest -Force -ErrorAction SilentlyContinue
  # Folders install made, once emptied of what it put there.
  foreach ($line in $lines) {
    $kind, $target = $line -split ' ', 2
    if ($kind -eq 'folder' -and (Test-Path -LiteralPath $target) -and -not (Get-ChildItem -LiteralPath $target -Force)) {
      Remove-Item -LiteralPath $target -Force
    }
  }
  # A feature turned off stays listed on until the reboot Windows asks for; check after it.
  $files = @($lines | Where-Object { $_ -notlike 'feature *' })
  if ((Invoke-Check $files) -ne 0) { Die 'uninstall left the lines above' }
  if ($restart) { Write-Output "the features finish turning off at the reboot; then: windows.ps1 check <plan>" }
  Request-Restart $restart
}

switch ($Verb) {
  'install' { Install-Hex }
  'uninstall' { Uninstall-Hex }
  'check' {
    $lines = if ($PlanFile) { @(Get-Content -LiteralPath $PlanFile) }
    elseif (Test-Path -LiteralPath $Manifest) { @(Get-Content -LiteralPath $Manifest) }
    else { @(Get-Plan) }
    exit (Invoke-Check $lines)
  }
}
