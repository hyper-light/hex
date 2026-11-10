# The Windows feature read's latency as the node meets it (native plan step 6, HEX-INST-9): each read is a
# fresh `hex setup features` process, the node's one cold read, timed from start to exit. Each round runs
# every binary once, in an order that alternates by round, and records beside it the machine's CPU load and
# the CPU time Defender's engine (MsMpEng) spent since the round before: what else the machine was doing.
# usage: feature_read_bench.ps1 -Binary <label>=<hex.exe>[,<label>=<hex.exe>] -Rounds <n> -Phase <name> -Out <csv>
param(
  [Parameter(Mandatory = $true)][string[]]$Binary,
  [Parameter(Mandatory = $true)][int]$Rounds,
  [Parameter(Mandatory = $true)][string]$Phase,
  [Parameter(Mandatory = $true)][string]$Out
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3

$runs = @($Binary | ForEach-Object { $_ -split ',' } | Where-Object { $_ } | ForEach-Object {
    $label, $path = $_ -split '=', 2
    [pscustomobject]@{ Label = $label; Path = (Resolve-Path -LiteralPath $path).Path }
  })
if (-not (Test-Path -LiteralPath $Out)) {
  'utc,phase,round,label,ms,exit,lines,cpu_load_pct,msmpeng_cpu_ms' | Set-Content -LiteralPath $Out -Encoding ascii
}
function Get-DefenderCpuMs {
  $p = @(Get-Process MsMpEng -ErrorAction SilentlyContinue)
  if ($p.Count -eq 0 -or $null -eq $p[0].TotalProcessorTime) { return -1 }
  return [int64]$p[0].TotalProcessorTime.TotalMilliseconds
}
$defender = Get-DefenderCpuMs
for ($r = 1; $r -le $Rounds; $r++) {
  $order = if ($r % 2 -eq 0) { $runs[($runs.Count - 1)..0] } else { $runs }
  foreach ($b in $order) {
    $load = [int](Get-CimInstance Win32_Processor | Measure-Object LoadPercentage -Average).Average
    $now = Get-DefenderCpuMs
    $spent = if ($now -ge 0 -and $defender -ge 0) { $now - $defender } else { -1 }
    $defender = $now
    $utc = (Get-Date).ToUniversalTime().ToString('HH:mm:ss.fff')
    $ErrorActionPreference = 'Continue'
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $said = & $b.Path setup features 2>&1 | Out-String
    $ms = $sw.ElapsedMilliseconds
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    $lines = @($said -split "`r?`n" | Where-Object { $_ -match ' (on|off)$' }).Count
    "$utc,$Phase,$r,$($b.Label),$ms,$code,$lines,$load,$spent" | Add-Content -LiteralPath $Out -Encoding ascii
  }
}
