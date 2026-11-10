# AC-T7.4.1: root-only actual Windows wire/census control, without a runtime backend or VM.
param([Parameter(Mandatory=$true)][string]$Hex, [Parameter(Mandatory=$true)][string]$SourceRevision,
      [Parameter(Mandatory=$true)][string]$RunParent, [Parameter(Mandatory=$true)][int]$DeadlineSeconds,
      [Parameter(Mandatory=$true)][int]$MaxRows)
$ErrorActionPreference = 'Stop'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass = 'BelowNormal'
if ($DeadlineSeconds -le 0 -or $MaxRows -le 0) { throw 'Supply positive existing operator bounds' }
$budget = [TimeSpan]::FromSeconds($DeadlineSeconds)
$root = Join-Path $RunParent ('wire-marker-' + [Guid]::NewGuid().ToString('N'))
if (Test-Path $root) { throw 'Refuse to reuse an existing evidence directory' }
[IO.Directory]::CreateDirectory($root) | Out-Null
$utf8 = New-Object Text.UTF8Encoding($false)
function WriteJson($path, $value) { [IO.File]::WriteAllText($path, ($value | ConvertTo-Json -Depth 32), $utf8) }
function StartOwned($taskArguments, $census) {
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $Hex
    foreach ($word in $taskArguments) {
        if ($word.Contains('"') -or $word.EndsWith('\') -or $word.Contains("`n") -or $word.Contains("`r")) {
            throw 'This private fixture accepts literal arguments without quotes, final backslash or newline'
        }
    }
    $info.Arguments = (($taskArguments | ForEach-Object { '"' + $_ + '"' }) -join ' ')
    $info.WorkingDirectory = $root
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.EnvironmentVariables['HEX_WAITS_CENSUS'] = $census
    $info.EnvironmentVariables['HEX_WAITS_CENSUS_MAX'] = [string]$MaxRows
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $info
    if (-not $process.Start()) { throw 'Owned process did not start' }
    return $process
}
function StopOwned($process) {
    if ($null -ne $process -and -not $process.HasExited) { $process.Kill() }
    if ($null -ne $process -and -not $process.WaitForExit([int]$budget.TotalMilliseconds)) {
        throw 'Exact owned child was not reaped within the existing operator cleanup budget'
    }
}
function ReadRows($path) {
    $rows = @()
    foreach ($line in [IO.File]::ReadAllLines($path)) {
        if ($line -notmatch '^(\S+) ([>!]?)([0-9]+)$') { throw "Malformed or truncated census: $path" }
        $rows += @{name=$Matches[1]; marker=$Matches[2]; nanos=[UInt64]::Parse($Matches[3])}
    }
    return $rows
}
$server = $null
$calls = @()
$report = @{kind='actual-native-wire-refusal-marker'; source_revision=$SourceRevision; profile='debug';
    binary=@{path=$Hex; sha256=(Get-FileHash -Algorithm SHA256 $Hex).Hash.ToLower()};
    root=$root; deadline_seconds=$DeadlineSeconds; deadline_basis='Existing120s administrative cancellation policy, not a runtime seed';
    max_rows=$MaxRows; backend_count=0; vm_count=0; seed_generation='forbidden: focused protocol controls';
    status='failed'; cleanup_reaped=$false}
try {
    $socket = Join-Path $root 's.sock'
    $config = Join-Path $root 'config.json'
    WriteJson $config @{cluster=$root; node=0; state=(Join-Path $root 'state'); socket=$socket; backends=@(); peers=@()}
    $server = StartOwned @('schedule','serve','--config',$config) (Join-Path $root 'server.tsv')
    $report.server_pid = $server.Id
    $serverError = $server.StandardError.ReadToEndAsync()
    $ready = $server.StandardOutput.ReadLineAsync()
    if (-not $ready.Wait($budget) -or $ready.Result -ne "ready $socket") { throw 'Actual scheduler readiness failed' }
    foreach ($case in @('answered','refused')) {
        $request = Join-Path $root ($case + '.json')
        if ($case -eq 'answered') { WriteJson $request @{Status=@{key=@{owner=0; epoch=1; seq=0}}} }
        else { WriteJson $request @{Describe=@{from=0; cluster='a-different-owned-fixture-realm'}} }
        $census = Join-Path $root ($case + '.tsv')
        $child = StartOwned @('schedule','call','--socket',$socket,'--request',$request) $census
        $calls += $child
        $stdout = $child.StandardOutput.ReadToEndAsync()
        $stderr = $child.StandardError.ReadToEndAsync()
        if (-not $child.WaitForExit([int]$budget.TotalMilliseconds)) { throw 'Actual CLI exceeded the existing operator budget' }
        $text = $stdout.Result
        [IO.File]::WriteAllText((Join-Path $root ($case + '.stdout')), $text, $utf8)
        [IO.File]::WriteAllText((Join-Path $root ($case + '.stderr')), $stderr.Result, $utf8)
        $rows = @(ReadRows $census)
        $name = if ($case -eq 'answered') { 'client.answer.schedule.status' } else { 'client.answer.schedule.describe' }
        $selected = @($rows | Where-Object {$_.name -eq $name})
        $expectedExit = if ($case -eq 'answered') { 0 } else { 1 }
        $expectedMarker = if ($case -eq 'answered') { '' } else { '!' }
        if ($child.ExitCode -ne $expectedExit -or $selected.Count -ne 1 -or $selected[0].marker -ne $expectedMarker) {
            throw 'Actual wire outcome/census marker mismatch'
        }
        if ($case -eq 'answered' -and ($text | ConvertFrom-Json).tracked -ne $false) { throw 'Unexpected status body' }
        $report[$case] = @{pid=$child.Id; exit=$child.ExitCode; rows=$rows; census=$census;
            census_sha256=(Get-FileHash -Algorithm SHA256 $census).Hash.ToLower()}
    }
    $report.status = 'passed'
} catch { $report.error = $_.Exception.Message; throw }
finally {
    try {
        foreach ($child in $calls) { StopOwned $child }
        StopOwned $server
        if ($null -ne $serverError) { [IO.File]::WriteAllText((Join-Path $root 'server.stderr'), $serverError.Result, $utf8) }
        $report.cleanup_reaped = $true
    } catch { $report.cleanup_error = $_.Exception.Message; $report.status = 'failed'; throw }
    finally { WriteJson (Join-Path $root 'result.json') $report }
}
Write-Output ('result ' + (Join-Path $root 'result.json'))
