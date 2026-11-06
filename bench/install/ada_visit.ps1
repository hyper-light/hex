# Ada's one visit at jupiter (native plan steps 4 and 6): everything that needs an administrator, in one
# elevated PowerShell, one UAC prompt. The agents run every other check over kiln as the ordinary user, while
# this window waits for them:
#   1. the staged files are checked against their SHA256SUMS;
#   2. step 4's oci_setup.ps1: containerd and Docker's engine, the image pulled, its layers written
#      (dockerd needs an administrator);
#   3. step 6's install (scripts\install\windows.ps1 install): hex.exe into %ProgramFiles%\Hex. Containers and
#      HypervisorPlatform are already on at jupiter, so it turns on no feature and Windows asks for no reboot;
#   4. it says "installed" and waits (at most -WaitMinutes) for the agents' `jupiter.sh installed` to finish;
#   5. step 6's uninstall, then it says "uninstalled" for the agents' `jupiter.sh uninstalled`;
#   6. a reboot only if Windows asked for one (it says so; it never restarts on its own).
# Every step's output goes to a transcript in the visit folder, which the agents read back.
#
#   powershell -ExecutionPolicy Bypass -File ada_visit.ps1 -Stage <staged folder>
param(
  [Parameter(Mandatory = $true)][string]$Stage,
  [string]$Visit = "$env:USERPROFILE\.cache\hex-t7-windows\visit",
  [int]$WaitMinutes = 90
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3

$me = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $me.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  # The one UAC prompt: this script again, elevated, with the same arguments.
  $argv = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"", '-Stage', "`"$Stage`"",
    '-Visit', "`"$Visit`"", '-WaitMinutes', $WaitMinutes)
  $p = Start-Process -FilePath (Get-Process -Id $PID).Path -Verb RunAs -Wait -PassThru -ArgumentList $argv
  exit $p.ExitCode
}

New-Item -ItemType Directory -Force -Path $Visit | Out-Null
Start-Transcript -Path (Join-Path $Visit 'transcript.txt') -Append | Out-Null
function Say([string]$What) { Write-Host ''; Write-Host "== $What ($((Get-Date).ToUniversalTime().ToString('HH:mm:ssZ')))" }
function Mark([string]$Name) { (Get-Date).ToUniversalTime().ToString('o') | Set-Content (Join-Path $Visit $Name) }
$restart = $false
try {
  Say "1. the staged files at $Stage against their SHA256SUMS"
  foreach ($line in Get-Content (Join-Path $Stage 'SHA256SUMS')) {
    if (-not $line.Trim()) { continue }
    $want, $file = $line -split '\s+\*?', 2
    $got = (Get-FileHash -Algorithm SHA256 (Join-Path $Stage $file)).Hash
    if ($got -ne $want.ToUpper()) { throw "$file is $got, not the staged $want" }
    Write-Host "ok $file"
  }

  Say '2. step 4: oci_setup.ps1 (containerd, Docker''s engine, the image and its layers)'
  & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Stage 'oci_setup.ps1')
  if ($LASTEXITCODE -ne 0) { throw "oci_setup.ps1 exited $LASTEXITCODE" }

  Say '3. step 6: install'
  & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Stage 'scripts\install\windows.ps1') install `
    -Binary (Join-Path $Stage 'hex.exe') -NoRestart | Tee-Object -Variable said
  if ($LASTEXITCODE -ne 0) { throw "windows.ps1 install exited $LASTEXITCODE" }
  if ("$said" -match 'reboot') { $restart = $true }

  Say "4. installed: waiting (at most $WaitMinutes min) for the agents' checks; leave this window open"
  Mark 'installed.ready'
  $deadline = (Get-Date).AddMinutes($WaitMinutes)
  while (-not (Test-Path (Join-Path $Visit 'installed.done'))) {
    if ((Get-Date) -gt $deadline) { throw "the agents' installed checks didn't finish within $WaitMinutes min" }
    Start-Sleep -Seconds 10
  }
  Write-Host "the agents' checks: $(Get-Content (Join-Path $Visit 'installed.done'))"

  Say '5. step 6: uninstall'
  & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Stage 'scripts\install\windows.ps1') uninstall `
    -NoRestart | Tee-Object -Variable said
  if ($LASTEXITCODE -ne 0) { throw "windows.ps1 uninstall exited $LASTEXITCODE" }
  if ("$said" -match 'reboot') { $restart = $true }
  Mark 'uninstalled.ready'

  Say '6. done'
  if ($restart) { Write-Host 'Windows asked for a reboot: restart jupiter when Amphora''s work allows, then tell the lead.' }
  else { Write-Host 'No reboot needed. You can close this window.' }
  Mark 'visit.done'
} catch {
  Write-Host "STOPPED: $_"
  "$_" | Set-Content (Join-Path $Visit 'visit.failed')
  exit 1
} finally {
  Stop-Transcript | Out-Null
}
