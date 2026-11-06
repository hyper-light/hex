# The Windows installer's lab (native plan step 6; HEX-INST-2, HEX-INST-4) on jupiter, run as a standard
# user around the one admin step an administrator takes (windows.ps1 install, then uninstall, each with
# its UAC prompt and the reboot Windows asks for). Each phase prints a record and exits non-zero at the
# first check that fails:
#   before          the machine is fresh of Hex (check lists nothing), the dry run's plan is saved, and an
#                   oci box is refused naming each feature that is off: a Windows image Containers, a Linux
#                   image HypervisorPlatform;
#   installed       every line of the plan is on the machine; no oci box is refused for a feature; the
#                   process class holds its contract; the oci class holds its contract and step 4's
#                   criteria on a matching image (-Layers, from bench/native/windows/oci_setup.ps1); a Linux
#                   image is refused only for the Shards guest (native plan step 5) until that lands;
#   uninstalled     check lists none of the plan's lines.
# usage: windows_lab.ps1 <before|installed|uninstalled> -Plan <file> [-Binary <hex.exe>]
#          [-TestBin <windows_process_box test exe>] [-OciTestBin <windows_oci_box test exe>] [-Layers <a;b;...>] [-MismatchLayers <a;b;...>]
param(
  [Parameter(Mandatory = $true, Position = 0)][ValidateSet('before', 'installed', 'uninstalled')][string]$Phase,
  [Parameter(Mandatory = $true)][string]$Plan,
  [string]$Binary,
  [string]$TestBin,
  [string]$OciTestBin,
  [string]$Layers,
  [string]$MismatchLayers
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3
$Install = Join-Path $PSScriptRoot '..\..\scripts\install\windows.ps1'

function Step([string]$What) { Write-Output ''; Write-Output "## $What" }
function Fail([string]$Why) { Write-Output "FAIL: $Why"; exit 1 }

# Runs one named test of test binary `Bin`; prints its output to the host and returns it (the function's
# one output), failing the lab if it fails.
function Invoke-Test([string]$Name, [string]$Bin = $TestBin) {
  # Its stderr lines are output here, not errors to stop on.
  $ErrorActionPreference = 'Continue'
  $out = & $Bin $Name --exact --nocapture 2>&1 | Out-String
  Write-Host $out
  if ($LASTEXITCODE -ne 0) { Fail "$Name (exit $LASTEXITCODE)" }
  return $out
}

# The two images' refusals (HEX-INST-2): each test prints the state it read as a "FEATURES: <name> <on|off>"
# line. Before install a feature the plan turns on reads off and any other on; after it, both read on.
function Test-Features([bool]$Installed) {
  $planned = @(Get-Content -LiteralPath $Plan | Where-Object { $_ -like 'feature *' })
  $tests = [ordered]@{
    'Containers'         = 'a_windows_image_is_refused_naming_containers_while_it_is_off'
    'HypervisorPlatform' = 'a_linux_image_is_refused_naming_what_its_guest_waits_on'
  }
  foreach ($feature in $tests.Keys) {
    $want = if (-not $Installed -and $planned -contains "feature $feature") { 'off' } else { 'on' }
    $said = Invoke-Test $tests[$feature]
    if ($said -notmatch "FEATURES: $feature $want") { Fail "$feature doesn't read $want" }
  }
}

Step "the machine: $((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')), $([Environment]::OSVersion.VersionString), as $([Security.Principal.WindowsIdentity]::GetCurrent().Name)"
switch ($Phase) {
  'before' {
    Step 'fresh of Hex: check lists nothing'
    & powershell -NoProfile -ExecutionPolicy Bypass -File $Install check
    if ($LASTEXITCODE -ne 0) { Fail 'the machine is not fresh of Hex' }
    Step 'the plan (install -DryRun)'
    & powershell -NoProfile -ExecutionPolicy Bypass -File $Install install -Binary $Binary -DryRun | Tee-Object -FilePath $Plan
    Step 'HEX-INST-2: an oci box is refused naming each feature that is off'
    Test-Features $false
    Write-Output 'PASS'
  }
  'installed' {
    Step 'every line of the plan is on the machine'
    $left = & powershell -NoProfile -ExecutionPolicy Bypass -File $Install check $Plan 6>&1 | Out-String
    Write-Output $left
    $lines = @(Get-Content -LiteralPath $Plan | Where-Object { $_ })
    $present = @($left -split "`n" | Where-Object { $_ -like 'left: *' })
    if ($present.Count -ne $lines.Count) { Fail "$($present.Count) of $($lines.Count) plan lines are on the machine" }
    Step 'HEX-INST-2: with both features on, no oci box is refused for a feature'
    Test-Features $true
    Step 'the process class holds its contract'
    Invoke-Test 'windows_process_boxes_hold_the_contract' | Out-Null
    Step "the oci class holds its contract and step 4's criteria on a matching image"
    if (-not $OciTestBin -or -not $Layers -or -not $MismatchLayers) { Fail 'the oci class needs -OciTestBin, -Layers and -MismatchLayers (oci_setup.ps1 writes them)' }
    $env:HEX_WINDOWS_LAYERS = $Layers
    $env:HEX_WINDOWS_MISMATCH_LAYERS = $MismatchLayers
    $said = Invoke-Test 'windows_oci_boxes_hold_the_contract_and_the_steps_criteria' $OciTestBin
    if ($said -match 'SKIP:') { Fail 'the oci suite skipped: it was given no image layers' }
    Write-Output 'PASS'
  }
  'uninstalled' {
    Step "HEX-INST-4: none of the plan's lines is on the machine"
    & powershell -NoProfile -ExecutionPolicy Bypass -File $Install check $Plan
    if ($LASTEXITCODE -ne 0) { Fail 'uninstall left the lines above' }
    Write-Output 'PASS'
  }
}
