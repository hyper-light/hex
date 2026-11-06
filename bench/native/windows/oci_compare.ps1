# oci_compare.ps1: native plan step 4's second criterion. Times create, start and delete of a container
# whose program waits, through Docker's engine (process isolation), containerd's runhcs shim and Hex's seam
# (crates/hex-bench/benches/windows_oci.rs, one untimed and one timed lifecycle per call), on the image
# oci_setup.ps1 pulled, each operation alone, N rounds after W untimed ones. The three are interleaved: each
# round runs one lifecycle of each, the order rotating by round, so none gets a quieter machine (the lead,
# 2026-11-06). The program is the same (`C:\hex\hex.exe selftest-box wait`), from the same folder mapped
# read-only at C:\hex. Each CLI call is a process start, so the CLIs' own cost is timed too (`version`
# alone) and recorded beside, never subtracted silently; the background load is recorded per round.
#
#   powershell -NoProfile -File oci_compare.ps1 -HexBin <dir with hex.exe> -HexBench <windows_oci.exe>
#       -Layers <layer;...> -Out <json> [-N 50] [-W 3]
param(
    [Parameter(Mandatory = $true)][string]$HexBin,
    [Parameter(Mandatory = $true)][string]$HexBench,
    [Parameter(Mandatory = $true)][string]$Layers,
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
        # ContainerAdministrator, as HCS runs Hex's launcher for a spec naming no user: nanoserver's default
        # ContainerUser can't read the mapped folder (jupiter, 2026-11-06: "Access is denied").
        create = { param($i) & $docker -H $Dock create --isolation process --user ContainerAdministrator --name "hexcmp-$i" `
                -v "${HexBin}:C:\hex:ro" $Image 'C:\hex\hex.exe' selftest-box wait | Out-Null }
        start  = { param($i) & $docker -H $Dock start "hexcmp-$i" | Out-Null }
        delete = { param($i) & $docker -H $Dock rm -f "hexcmp-$i" | Out-Null }
        cli    = { & $docker -H $Dock version | Out-Null }
    }
    containerd = @{
        create = { param($i) & $ctr -a $Ctrd containers create --runtime io.containerd.runhcs.v1 --user ContainerAdministrator `
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

$hexOut = Join-Path $Root "hex-round.json"
$hexRound = @{}
$runs["hex"] = @{
    # One untimed then one timed lifecycle in the bench, its timed samples read back per operation.
    create = { param($i)
        & $HexBench $hexOut 1 1 $HexBin $Layers | Out-Null
        $j = Get-Content $hexOut -Raw | ConvertFrom-Json
        $script:hexRound = @{ create = $j.create_ms.samples[0]; start = $j.start_ms.samples[0]; delete = $j.delete_ms.samples[0] } }
}

function Load() {
    [ordered]@{
        cpu_pct = (Get-CimInstance Win32_Processor | Measure-Object LoadPercentage -Average).Average
        system_pct = [int]((Get-Counter '\Process(System)\% Processor Time' -SampleInterval 1 -MaxSamples 1).CounterSamples[0].CookedValue / [Environment]::ProcessorCount)
    }
}

$record = [ordered]@{
    host     = (Get-CimInstance Win32_OperatingSystem | Select-Object Caption, BuildNumber)
    cpu      = (Get-CimInstance Win32_Processor | Select-Object -First 1 Name, NumberOfLogicalProcessors)
    busiest  = (Get-Process | Sort-Object CPU -Descending | Select-Object -First 8 Name, Id, CPU)
    image    = $Image
    n        = $N
    warmup   = $W
    load     = @()
}
$names = @("docker", "containerd", "hex")
$t = @{}
foreach ($name in $names) { $t[$name] = @{ create = @(); start = @(); delete = @(); cli = @() } }
for ($i = 0; $i -lt $W + $N; $i++) {
    if ($i -ge $W) { $record.load += (Load) }
    for ($k = 0; $k -lt $names.Count; $k++) {
        $name = $names[($i + $k) % $names.Count]
        $r = $runs[$name]
        if ($name -eq "hex") {
            & $r.create $i
            if ($LASTEXITCODE -ne 0) { throw "Hex's bench failed ($LASTEXITCODE) at round $i" }
            $c = $script:hexRound.create; $s = $script:hexRound.start; $d = $script:hexRound.delete; $q = 0
        } else {
            $c = Timed { & $r.create $i }
            $s = Timed { & $r.start $i }
            $d = Timed { & $r.delete $i }
            $q = Timed { & $r.cli }
        }
        if ($i -ge $W) { $t[$name].create += $c; $t[$name].start += $s; $t[$name].delete += $d; $t[$name].cli += $q }
    }
}
foreach ($name in $names) {
    $record[$name] = [ordered]@{
        create_ms = Stats $t[$name].create; start_ms = Stats $t[$name].start; delete_ms = Stats $t[$name].delete
        cli_alone_ms = Stats $t[$name].cli
    }
}
$record | ConvertTo-Json -Depth 6 | Set-Content $Out
Get-Content $Out
