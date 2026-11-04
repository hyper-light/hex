# oci_compare.ps1: native plan step 4's second criterion, the competitors' half. Times create, start and
# delete of a container whose program waits, through Docker's engine (process isolation) and containerd's
# runhcs shim, on the image oci_setup.ps1 pulled, each operation alone, N times after W untimed rounds. The
# program is the one Hex's bench runs (`C:\hex\hex.exe selftest-box wait`), from the same folder mapped
# read-only at C:\hex, so the three run the same work (crates/hex-bench/benches/windows_oci.rs is Hex's
# half). Each CLI call is a process start, so the CLIs' own cost is timed too (`version` alone, per
# runtime) and recorded beside, for the record to state, never subtracted silently.
#
#   powershell -NoProfile -File oci_compare.ps1 -HexBin <dir with hex.exe> -Out <json> [-N 50] [-W 3]
param(
    [Parameter(Mandatory = $true)][string]$HexBin,
    [Parameter(Mandatory = $true)][string]$Out,
    [int]$N = 50,
    [int]$W = 3,
    [string]$Root = "$env:USERPROFILE\.cache\hex-native-s4",
    [string]$Image = "mcr.microsoft.com/windows/nanoserver:ltsc2025"
)
$ErrorActionPreference = "Stop"
$Ctrd = "\\.\pipe\hex-native-s4-containerd"
$Dock = "npipe:////./pipe/hex-native-s4-docker"
$ctr = "$Root\containerd\bin\ctr.exe"
$docker = "$Root\docker\docker.exe"

function Timed([scriptblock]$op) {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    & $op
    if ($LASTEXITCODE -ne 0) { throw "a timed call failed ($LASTEXITCODE): $op" }
    $sw.Stop()
    $sw.Elapsed.TotalMilliseconds
}

function Stats([double[]]$v) {
    $s = $v | Sort-Object
    $at = { param($p) $s[[math]::Max(1, [math]::Ceiling($p * $s.Count / 100)) - 1] }
    [ordered]@{ p50 = (& $at 50); p90 = (& $at 90); p99 = (& $at 99); max = $s[-1]; samples = $v }
}

$runs = [ordered]@{
    docker = @{
        create = { param($i) & $docker -H $Dock create --isolation process --name "hexcmp-$i" `
                -v "${HexBin}:C:\hex:ro" $Image 'C:\hex\hex.exe' selftest-box wait | Out-Null }
        start  = { param($i) & $docker -H $Dock start "hexcmp-$i" | Out-Null }
        delete = { param($i) & $docker -H $Dock rm -f "hexcmp-$i" | Out-Null }
        cli    = { & $docker -H $Dock version | Out-Null }
    }
    containerd = @{
        create = { param($i) & $ctr -a $Ctrd containers create --runtime io.containerd.runhcs.v1 `
                --mount "type=bind,src=$HexBin,dst=C:\hex,options=rbind:ro" $Image "hexcmp-$i" `
                'C:\hex\hex.exe' selftest-box wait | Out-Null }
        start  = { param($i) & $ctr -a $Ctrd tasks start -d "hexcmp-$i" | Out-Null }
        delete = { param($i)
            & $ctr -a $Ctrd tasks kill -s SIGKILL "hexcmp-$i" | Out-Null
            & $ctr -a $Ctrd tasks delete -f "hexcmp-$i" | Out-Null
            & $ctr -a $Ctrd containers delete "hexcmp-$i" | Out-Null }
        cli    = { & $ctr -a $Ctrd version | Out-Null }
    }
}

$record = [ordered]@{
    host     = (Get-CimInstance Win32_OperatingSystem | Select-Object Caption, BuildNumber)
    cpu      = (Get-CimInstance Win32_Processor | Select-Object -First 1 Name, NumberOfLogicalProcessors)
    load_pct = (Get-CimInstance Win32_Processor | Measure-Object LoadPercentage -Average).Average
    busiest  = (Get-Process | Sort-Object CPU -Descending | Select-Object -First 8 Name, Id, CPU)
    image    = $Image
    n        = $N
    warmup   = $W
}
foreach ($name in $runs.Keys) {
    $r = $runs[$name]
    $t = @{ create = @(); start = @(); delete = @(); cli = @() }
    for ($i = 0; $i -lt $W + $N; $i++) {
        $c = Timed { & $r.create $i }
        $s = Timed { & $r.start $i }
        $d = Timed { & $r.delete $i }
        $k = Timed { & $r.cli }
        if ($i -ge $W) { $t.create += $c; $t.start += $s; $t.delete += $d; $t.cli += $k }
    }
    $record[$name] = [ordered]@{
        create_ms = Stats $t.create; start_ms = Stats $t.start; delete_ms = Stats $t.delete
        cli_alone_ms = Stats $t.cli
    }
}
$record | ConvertTo-Json -Depth 6 | Set-Content $Out
Get-Content $Out
