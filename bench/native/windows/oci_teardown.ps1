# oci_teardown.ps1: ends what oci_setup.ps1 started, by the PIDs it recorded (each checked to still be the
# program it started), and leaves the downloads and stores in place unless -Remove is given, in which case
# the whole folder goes.
#
#   powershell -NoProfile -File oci_teardown.ps1 [-Root <dir>] [-Remove]
param(
    [string]$Root = "$env:USERPROFILE\.cache\hex-native-s4",
    [switch]$Remove
)
$ErrorActionPreference = "Stop"
foreach ($p in @(@{ file = "dockerd.pid"; exe = "dockerd" }, @{ file = "containerd.pid"; exe = "containerd" })) {
    $f = Join-Path $Root $p.file
    if (-not (Test-Path $f)) { continue }
    $id = [int](Get-Content $f)
    $proc = Get-Process -Id $id -ErrorAction SilentlyContinue
    if ($proc -and $proc.ProcessName -eq $p.exe -and $proc.Path -like "$Root*") {
        Stop-Process -Id $id
        Wait-Process -Id $id -Timeout 30
        "stopped $($p.exe) $id"
    } else {
        "skipped $($p.exe) ${id}: not the process oci_setup.ps1 started"
    }
    Remove-Item $f
}
# A stopped dockerd leaves its pidfile, which the next one would refuse to start over.
Remove-Item -ErrorAction SilentlyContinue (Join-Path $Root "dockerd.pid.engine")
if ($Remove) {
    if (-not $Root -or -not (Test-Path (Join-Path $Root "versions.txt"))) { throw "refusing to remove ${Root}: not oci_setup.ps1's folder" }
    Remove-Item -Recurse -Force $Root
    "removed $Root"
}
