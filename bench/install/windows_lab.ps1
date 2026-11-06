# The Windows installer's lab (native plan step 6; HEX-INST-2, HEX-INST-4) on jupiter, run as a standard
# user around the one admin step an administrator takes (windows.ps1 install, then uninstall, each with
# its UAC prompt and the reboot Windows asks for). Each phase prints a record and exits non-zero at the
# first check that fails:
#   before          the machine is fresh of Hex (check lists nothing), the dry run's plan is saved (asking
#                   hex for the features once, as in asks), and an oci box is refused naming the features
#                   that are off;
#   asks            the dry run alone, through a hex that counts its runs: windows.ps1 asks for one
#                   feature listing a run (HEX-INST-9), however many features its plan reads;
#   installed       every line of the plan is on the machine, the oci box's refusal now says only what the
#                   class still waits on (both features on), and the process class holds its contract;
#   uninstalled     check lists none of the plan's lines.
# usage: windows_lab.ps1 <before|asks|installed|uninstalled> -Binary <hex.exe> -TestBin <windows_process_box test exe> -Plan <file>
param(
  [Parameter(Mandatory = $true, Position = 0)][ValidateSet('before', 'asks', 'installed', 'uninstalled')][string]$Phase,
  [Parameter(Mandatory = $true)][string]$Plan,
  [string]$Binary,
  [string]$TestBin
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3
$Install = Join-Path $PSScriptRoot '..\..\scripts\install\windows.ps1'

function Step([string]$What) { Write-Output ''; Write-Output "## $What" }
function Fail([string]$Why) { Write-Output "FAIL: $Why"; exit 1 }

# Runs one named test of the Windows test binary; prints its output to the host and returns it (the
# function's one output), failing the lab if it fails.
function Invoke-Test([string]$Name) {
  # Its stderr lines are output here, not errors to stop on.
  $ErrorActionPreference = 'Continue'
  $out = & $TestBin $Name --exact --nocapture 2>&1 | Out-String
  Write-Host $out
  if ($LASTEXITCODE -ne 0) { Fail "$Name (exit $LASTEXITCODE)" }
  return $out
}

# The dry run's plan, saved to $Plan, through a hex that counts its runs: a .cmd beside the plan notes each
# run's arguments, one line each, then runs $Binary with them. The dry run only reads the binary it is given
# (install copies it), so the counting hex stands in for it there alone. Fails unless windows.ps1 ran hex
# exactly once, for `setup features`: the features' state is one listing a run.
function Invoke-CountedPlan {
  $dir = Split-Path -Parent ([IO.Path]::GetFullPath($Plan))
  $runs = Join-Path $dir 'hex-runs'
  $counting = Join-Path $dir 'hex-counting.cmd'
  $real = (Resolve-Path -LiteralPath $Binary).Path
  New-Item -ItemType File -Force -Path $runs | Out-Null
  Set-Content -LiteralPath $counting -Encoding ascii -Value @(
    '@echo off', "echo %*>>`"$runs`"", "`"$real`" %*", 'exit /b %ERRORLEVEL%')
  & powershell -NoProfile -ExecutionPolicy Bypass -File $Install install -Binary $counting -DryRun | Tee-Object -FilePath $Plan
  if ($LASTEXITCODE -ne 0) { Fail "the dry run failed (exit $LASTEXITCODE)" }
  $said = @(Get-Content -LiteralPath $runs | Where-Object { $_ })
  Remove-Item -LiteralPath $counting, $runs -Force
  Write-Output "hex runs: $($said.Count) ($($said -join '; '))"
  if ($said.Count -ne 1 -or $said[0].Trim() -ne 'setup features') {
    Fail "windows.ps1 ran hex $($said.Count) times for one plan; it asks for the features once a run"
  }
}

Step "the machine: $((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')), $([Environment]::OSVersion.VersionString), as $([Security.Principal.WindowsIdentity]::GetCurrent().Name)"
switch ($Phase) {
  'before' {
    Step 'fresh of Hex: check lists nothing'
    & powershell -NoProfile -ExecutionPolicy Bypass -File $Install check -Binary $Binary
    if ($LASTEXITCODE -ne 0) { Fail 'the machine is not fresh of Hex' }
    Step 'the plan (install -DryRun), asking hex for the features once'
    Invoke-CountedPlan
    Step 'HEX-INST-2: an oci box is refused naming the features that are off'
    $said = Invoke-Test 'an_oci_box_is_refused_naming_the_features_that_are_off'
    if ($said -notmatch "Hex's installer turns") { Fail 'the refusal names no feature that is off' }
    Write-Output 'PASS'
  }
  'asks' {
    Step 'HEX-INST-9: the dry run asks hex for the features once'
    Invoke-CountedPlan
    Write-Output 'PASS'
  }
  'installed' {
    Step 'every line of the plan is on the machine'
    $left = & powershell -NoProfile -ExecutionPolicy Bypass -File $Install check $Plan -Binary $Binary 6>&1 | Out-String
    Write-Output $left
    $lines = @(Get-Content -LiteralPath $Plan | Where-Object { $_ })
    $present = @($left -split "`n" | Where-Object { $_ -like 'left: *' })
    if ($present.Count -ne $lines.Count) { Fail "$($present.Count) of $($lines.Count) plan lines are on the machine" }
    Step 'HEX-INST-2: with both features on, the oci box is refused only for what the class waits on'
    $said = Invoke-Test 'an_oci_box_is_refused_naming_the_features_that_are_off'
    if ($said -notmatch 'neither is built yet') { Fail 'a feature still reads off after install' }
    Step 'the process class holds its contract'
    Invoke-Test 'windows_process_boxes_hold_the_contract' | Out-Null
    Write-Output 'PASS'
  }
  'uninstalled' {
    Step "HEX-INST-4: none of the plan's lines is on the machine"
    & powershell -NoProfile -ExecutionPolicy Bypass -File $Install check $Plan -Binary $Binary
    if ($LASTEXITCODE -ne 0) { Fail 'uninstall left the lines above' }
    Write-Output 'PASS'
  }
}
