# oci_compare.ps1: native plan step 4's second criterion. Times create, start and delete of a container
# whose program waits, through Docker's engine (process isolation), containerd's runhcs shim and Hex's seam
# (crates/hex-bench/benches/windows_oci.rs, one untimed and one timed lifecycle per call), on the image
# oci_setup.ps1 pulled, each operation alone, N rounds after W untimed ones. The three are interleaved: each
# round runs one lifecycle of each, the order rotating by round, so none gets a quieter machine (the lead,
# 2026-11-06). The program is the same (`C:\hex\hex.exe selftest-box wait`), from the same folder mapped
# read-only at C:\hex. Each CLI call is a process start, so the CLIs' own cost is timed too (`version`
# alone) and recorded beside, never subtracted silently; the background load is recorded before and after
# each round, with the two readings of jupiter's NTFS stall, and a segment stops at the stall's bar.
#
#   powershell -NoProfile -File oci_compare.ps1 -HexBin <dir with hex.exe> -HexBench <windows_oci.exe>
#       -Layers <layer;...> -Out <json> [-N 299] [-W 3]
param(
    [Parameter(Mandatory = $true)][string]$HexBin,
    [Parameter(Mandatory = $true)][string]$HexBench,
    [Parameter(Mandatory = $true)][string]$Layers,
    [Parameter(Mandatory = $true)][string]$Out,
    # N for a stated p99 confidence (Ada's rule on constants, 2026-11-06): the largest of N samples is at
    # or above the true p99 unless all N fall below it, which happens with probability 0.99^N, so N >=
    # ln(0.05)/ln(0.99) = 298.07 gives 95% confidence that the reported p99 is not under the true one.
    [int]$N = 299,
    [int]$W = 3,
    # Seconds this segment may run before it stops after the round under way (0: no budget). A kiln run
    # ends what it leaves behind, so the record is made in segments within kiln's run cap, each resuming
    # after the last round in <Out>.rounds.jsonl (the lead, 2026-11-07).
    [int]$Budget = 0,
    # The lead's bar for a host in jupiter's NTFS stall (2026-11-08): System's threads waiting on free pages
    # and the machine's privileged time. The stall read 2,787-4,029 System threads in Wait/FreePage and
    # 66-96% kernel time (11-07/08), the clean boot of 02:51Z 28 threads and 2.8% (11-08 20:39Z). A round
    # read past either isn't of record: it goes to <Out>.stalled.jsonl and the segment stops (exit 4).
    # The FreePage count is the stall's leading sign, not its cost: every started process-isolated
    # container leaves about 4 such threads until a reboot, Docker's as Hex's (the lead's per-arm reading,
    # 2026-11-08 23:50Z), and round 9-28 timings stayed flat while it rose from 172 to 475 at 0.7-2.5%
    # privileged time (compare-7794bedd). 0 records it without stopping; privileged time is then the bar.
    [int]$StallFreePage = 500,
    [double]$StallPrivileged = 30,
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
        $script:hexRound = @{ create = $j.create_ms.samples[0]; start = $j.start_ms.samples[0]; delete = $j.delete_ms.samples[0]
            copy = $j.blank_copy_ms.samples[0]; exec = $j.exec_ms.samples[0] } }
}

# A reading that fails is data, not the end of the segment (jupiter, 2026-11-07 15:39Z: WMI answered
# Win32_Processor with "Server execution failed", 0x80080005, and the segment ended after one round).
function Reading([scriptblock]$read) { try { & $read } catch { "failed: $_" } }

# System's share of the machine and the machine's privileged time, from one second's counter sample.
function Counters() {
    $s = (Get-Counter -Counter @('\Processor(_Total)\% Privileged Time', '\Process(System)\% Processor Time') `
            -SampleInterval 1 -MaxSamples 1).CounterSamples
    $of = { param($like)
        $v = ($s | Where-Object { $_.Path -like $like } | Select-Object -First 1).CookedValue
        if ($null -eq $v) { throw "no counter $like in the sample" }
        $v }
    @{ privileged = [math]::Round((& $of '*\% privileged time'), 1)
        system = [int]((& $of '*process(system)*') / [Environment]::ProcessorCount) }
}

function Load() {
    $k = Reading { Counters }
    [ordered]@{
        cpu_pct = Reading { (Get-CimInstance Win32_Processor | Measure-Object LoadPercentage -Average).Average }
        system_pct = if ($k -is [string]) { $k } else { $k.system }
        privileged_pct = if ($k -is [string]) { $k } else { $k.privileged }
        # .NET's FreePage is the kernel's FreePage and WrFreePage alike.
        system_freepage = Reading { @((Get-Process -Id 4).Threads |
                Where-Object { $_.ThreadState -eq 'Wait' -and $_.WaitReason -eq 'FreePage' }).Count }
        c_free_gb = Reading { [math]::Round((Get-PSDrive C).Free / 1GB, 1) }
    }
}

# Why a reading puts the host past the stall bar, or $null when it's clean. A reading that failed says
# nothing of the host, so it doesn't count as clean.
function Stalled($load) {
    $fp = $load.system_freepage
    $pr = $load.privileged_pct
    if ($fp -is [string] -or $pr -is [string]) { return "the host's state wasn't read (FreePage: $fp; privileged: $pr)" }
    if ($StallFreePage -gt 0 -and $fp -gt $StallFreePage) { return "$fp System threads wait on free pages, past $StallFreePage" }
    if ($pr -gt $StallPrivileged) { return "privileged time $pr%, past $StallPrivileged%" }
    $null
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
# Leftovers of an earlier run that ended mid-round (jupiter, 2026-11-06: a "hexcmp-40" from a run cut by a
# wedged kernel collided with this run's round 40), removed by name from this script's own stores; the
# daemons are oci_setup.ps1's private ones, so nothing else's containers are seen.
foreach ($c in (& $docker -H $Dock ps -a -q --filter "name=hexcmp-")) { & $docker -H $Dock rm -f $c | Out-Null }
foreach ($c in ((& $ctr -a $Ctrd containers ls -q) | Where-Object { $_ -like "hexcmp-*" })) {
    & $ctr -a $Ctrd tasks kill -s SIGKILL $c 2>$null | Out-Null
    & $ctr -a $Ctrd tasks delete -f $c 2>$null | Out-Null
    & $ctr -a $Ctrd containers delete $c | Out-Null
}

$names = @("docker", "containerd", "hex")
# A failed call is data, not the end of the run: it is recorded per runtime with its message, the round's
# container removed, and the run goes on (jupiter, 2026-11-06: Docker's create failed at round ~96 with
# "ExpandScratchSize ... semaphore timeout"). Each round is appended to <Out>.rounds.jsonl as it ends, with
# everything the record needs, and a segment resumes after the file's last round.
$roundsOut = "$Out.rounds.jsonl"
$first = 0
if (Test-Path $roundsOut) {
    $last = Get-Content $roundsOut | Where-Object { $_ } | Select-Object -Last 1 | ConvertFrom-Json
    if ($last) { $first = [int]$last.round + 1 }
}
$began = Get-Date
$i = $first
$stalled = $null
# Each round carries when it began and ended (UTC), so a window of the host's history can be taken out of
# the record by time (the lead, 2026-11-08: rounds from 16:21Z to 00:06Z on 11-07/08 are retaken), and the
# host's state read before it (`load`) and after it (`load_end`), one reading between two rounds serving
# both. A round whose after-reading passes the stall bar is set aside, so the next segment runs it again.
$before = Load
try {
$why = Stalled $before
if ($why) { $stalled = "before round ${i}: $why" }
for (; -not $stalled -and $i -lt $W + $N; $i++) {
    if ($Budget -gt 0 -and ((Get-Date) - $began).TotalSeconds -gt $Budget) { break }
    $round = [ordered]@{ round = $i; began = (Get-Date).ToUniversalTime().ToString('o'); load = $before }
    for ($k = 0; $k -lt $names.Count; $k++) {
        $name = $names[($i + $k) % $names.Count]
        $r = $runs[$name]
        try {
            if ($name -eq "hex") {
                & $r.create $i
                if ($LASTEXITCODE -ne 0) { throw "Hex's bench exited $LASTEXITCODE" }
                $round[$name] = [ordered]@{ create = $script:hexRound.create; start = $script:hexRound.start
                    delete = $script:hexRound.delete; copy = $script:hexRound.copy; exec = $script:hexRound.exec }
            } else {
                $c = Timed { & $r.create $i }
                $s = Timed { & $r.start $i }
                $d = Timed { & $r.delete $i }
                $q = Timed { & $r.cli }
                $round[$name] = [ordered]@{ create = $c; start = $s; delete = $d; cli = $q }
            }
        } catch {
            $round[$name] = [ordered]@{ failed = "$_" }
            # Its container, if one was made, goes before the next round reuses no name.
            if ($name -eq "docker") { & $docker -H $Dock rm -f "hexcmp-$i" 2>$null | Out-Null }
            if ($name -eq "containerd") {
                & $ctr -a $Ctrd tasks kill -s SIGKILL "hexcmp-$i" 2>$null | Out-Null
                & $ctr -a $Ctrd tasks delete -f "hexcmp-$i" 2>$null | Out-Null
                & $ctr -a $Ctrd containers delete "hexcmp-$i" 2>$null | Out-Null
            }
        }
    }
    $round.ended = (Get-Date).ToUniversalTime().ToString('o')
    $before = Load
    $round.load_end = $before
    $why = Stalled $before
    if ($why) {
        $round.stalled = $why
        $round | ConvertTo-Json -Depth 5 -Compress | Add-Content "$Out.stalled.jsonl"
        $stalled = "round ${i}: $why"
        break
    }
    $round | ConvertTo-Json -Depth 5 -Compress | Add-Content $roundsOut
}
} finally {
    $why = if ($stalled) { " stopped at the stall bar, $stalled" } else { "" }
    "segment rounds $first..$($i - 1) began $($began.ToUniversalTime().ToString('o')) ended $((Get-Date).ToUniversalTime().ToString('o'))$why" | Add-Content "$Out.segments"
}
if ($stalled) { "segment stopped at the stall bar: $stalled (set aside in $Out.stalled.jsonl)"; exit 4 }
if ($i -lt $W + $N) { "segment ended at its budget: $i of $($W + $N) rounds"; exit 3 }

# The record, from every round in the file (all segments), the W untimed rounds left out.
$all = Get-Content $roundsOut | Where-Object { $_ } | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.round -ge $W }
$record.load = @($all | ForEach-Object { $_.load })
$record.load_end = @($all | ForEach-Object { $_.load_end })
$record.segments = @(Get-Content "$Out.segments")
foreach ($name in $names) {
    $ok = @($all | ForEach-Object { $_.$name } | Where-Object { $_ -and -not $_.failed })
    $record[$name] = [ordered]@{
        create_ms = Stats ($ok | ForEach-Object { $_.create }); start_ms = Stats ($ok | ForEach-Object { $_.start })
        delete_ms = Stats ($ok | ForEach-Object { $_.delete })
        failures = @($all | Where-Object { $_.$name.failed } | ForEach-Object { [ordered]@{ round = $_.round; why = $_.$name.failed } })
    }
    if ($name -ne "hex") { $record[$name].cli_alone_ms = Stats ($ok | ForEach-Object { $_.cli }) }
}
$hexOk = @($all | ForEach-Object { $_.hex } | Where-Object { $_ -and -not $_.failed })
$record.hex.blank_copy_ms = Stats ($hexOk | ForEach-Object { $_.copy })
$record.hex.exec_ms = Stats ($hexOk | ForEach-Object { $_.exec })
$record | ConvertTo-Json -Depth 6 | Set-Content $Out
Get-Content $Out
