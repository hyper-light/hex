# The other-user refusal check (native plan step 8; Ada approved a temporary standard local user on jupiter
# for it, 2026-11-08). Makes a standard local user with a random password (RandomNumberGenerator, held in
# this script's memory and handed to the probe on stdin, never printed or written), a folder only this run
# uses, and a `hex node` serving a socket there; runs `stdio_probe refuse-as`; then, whatever happened,
# ends the node, deletes the account, its profile (if Windows made one) and the folder, and says whether
# each is gone. Run elevated (New-LocalUser needs it):
#   powershell -NoProfile -File other_user.ps1 -Probe <stdio_probe.exe> -Hex <hex.exe>
param([Parameter(Mandatory)][string]$Probe, [Parameter(Mandatory)][string]$Hex)
$ErrorActionPreference = 'Stop'

$name = 'hexprobe' + (Get-Random -Minimum 10000 -Maximum 99999)
$bytes = New-Object byte[] 24
[Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
# Upper, lower, a digit and a symbol, whatever the random part holds, for the password policy.
$pw = [Convert]::ToBase64String($bytes) + 'Aa1!'
$dir = Join-Path $env:ProgramData ('hex-probe-' + $name)
$node = $null
$sid = $null
try {
    $user = New-LocalUser -Name $name -Password (ConvertTo-SecureString $pw -AsPlainText -Force) `
        -AccountNeverExpires -UserMayNotChangePassword -Description 'Hex step 8 other-user refusal test (temporary)'
    $sid = $user.SID.Value
    # A standard user: the built-in Users group (by its well-known SID, whatever the system's language).
    Add-LocalGroupMember -SID 'S-1-5-32-545' -Member $name
    Write-Output "made standard user $name ($sid)"
    New-Item -ItemType Directory -Path $dir | Out-Null
    icacls $dir /grant "${name}:(OI)(CI)M" | Out-Null
    $exe = Join-Path $dir 'stdio_probe.exe'
    Copy-Item $Probe $exe

    $start = New-Object System.Diagnostics.ProcessStartInfo
    $start.FileName = $Hex
    $start.Arguments = "node --socket `"$dir\node.sock`" --state `"$dir\state`""
    $start.UseShellExecute = $false
    $start.RedirectStandardOutput = $true
    $node = [System.Diagnostics.Process]::Start($start)
    $ready = $node.StandardOutput.ReadLine()
    Write-Output "hex node (pid $($node.Id)): $ready"

    Write-Output "the password is $($pw.Length) characters"
    $env:HEX_PROBE_DIR = $dir
    # What a pipe to a native program is written in: without a byte-order mark, which would reach the probe
    # as the password's first character.
    $OutputEncoding = New-Object System.Text.UTF8Encoding $false
    $pw | & $exe refuse-as $name "$dir\node.sock"
    Write-Output "probe exit $LASTEXITCODE"
}
finally {
    $pw = $null
    if ($node -and -not $node.HasExited) { $node.Kill(); $node.WaitForExit() }
    if (Get-LocalUser -Name $name -ErrorAction SilentlyContinue) { Remove-LocalUser -Name $name }
    if ($sid) {
        Get-CimInstance Win32_UserProfile | Where-Object { $_.SID -eq $sid } | Remove-CimInstance
    }
    if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
    $account = if (Get-LocalUser -Name $name -ErrorAction SilentlyContinue) { 'STILL THERE' } else { 'gone' }
    $profile = if ($sid -and (Get-CimInstance Win32_UserProfile | Where-Object { $_.SID -eq $sid })) { 'STILL THERE' } else { 'gone' }
    $folder = if (Test-Path "C:\Users\$name") { 'STILL THERE' } else { 'gone' }
    $scratch = if (Test-Path $dir) { 'STILL THERE' } else { 'gone' }
    Write-Output "cleanup: account $name $account; its profile $profile; C:\Users\$name $folder; $dir $scratch"
}
