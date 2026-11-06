# oci_run_detached.ps1: runs oci_setup.ps1, oci_compare.ps1 and oci_teardown.ps1 in sequence, its output and
# exit codes written beside the record, for a run longer than one kiln run may last (Ada's OK for detached,
# bounded remote jobs, 2026-11-01): start it with Start-Process and read <Out>.status when it ends.
#
#   powershell -NoProfile -File oci_run_detached.ps1 -HexBin <dir> -HexBench <exe> -Layers <l;...> -Out <json> [-N 299] [-W 3]
param(
    [Parameter(Mandatory = $true)][string]$HexBin,
    [Parameter(Mandatory = $true)][string]$HexBench,
    [Parameter(Mandatory = $true)][string]$Layers,
    [Parameter(Mandatory = $true)][string]$Out,
    [int]$N = 299,
    [int]$W = 3
)
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$status = "$Out.status"
"started $(Get-Date -Format o)" | Set-Content $status
& powershell -NoProfile -ExecutionPolicy Bypass -File "$here\oci_teardown.ps1" *> "$Out.teardown0.log"
& powershell -NoProfile -ExecutionPolicy Bypass -File "$here\oci_setup.ps1" *> "$Out.setup.log"
"setup exit $LASTEXITCODE" | Add-Content $status
& powershell -NoProfile -ExecutionPolicy Bypass -File "$here\oci_compare.ps1" -HexBin $HexBin -HexBench $HexBench -Layers $Layers -Out $Out -N $N -W $W *> "$Out.log"
"compare exit $LASTEXITCODE" | Add-Content $status
& powershell -NoProfile -ExecutionPolicy Bypass -File "$here\oci_teardown.ps1" *> "$Out.teardown.log"
"ended $(Get-Date -Format o)" | Add-Content $status
