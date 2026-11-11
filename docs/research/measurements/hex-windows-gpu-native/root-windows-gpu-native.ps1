$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = '__OWN_ROOT__'
$expectedArchive = '__ARCHIVE_SHA__'
$revision = '__SOURCE_REVISION__'
if (Test-Path -LiteralPath $root) { throw 'private native light path already exists' }
[IO.Directory]::CreateDirectory($root) | Out-Null
$archive = Join-Path $root 'source.tar.gz'
[IO.File]::Copy('__SOURCE_ARCHIVE_FILE__', $archive, $false)
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expectedArchive) { throw 'exact source archive hash mismatch' }
$source = Join-Path $root 'source'
[IO.Directory]::CreateDirectory($source) | Out-Null
& C:\Windows\System32\tar.exe -xzf $archive -C $source
if ($LASTEXITCODE -ne 0) { throw 'System32 tar extraction failed' }
$utf8 = [Text.UTF8Encoding]::new($false)
foreach ($directory in @('target','cache\cargo','temp')) { [IO.Directory]::CreateDirectory((Join-Path $root $directory)) | Out-Null }
$cargo = (& rustup which --toolchain 1.99.1 cargo).Trim()
if ($LASTEXITCODE -ne 0) { throw 'existing pinned cargo unavailable; no install attempted' }
$rustc = (& rustup which --toolchain 1.99.1 rustc).Trim()
if ($LASTEXITCODE -ne 0) { throw 'existing pinned rustc unavailable; no install attempted' }
$env:PATH = [IO.Path]::GetDirectoryName($cargo) + ';' + $env:PATH
$env:RUSTC = $rustc
$env:CARGO_HOME = Join-Path $root 'cache\cargo'
$env:CARGO_TARGET_DIR = Join-Path $root 'target'
$env:CARGO_BUILD_JOBS = '1'
$env:RUST_TEST_THREADS = '1'
$env:RAYON_NUM_THREADS = '1'
$env:CARGO_PROFILE_TEST_CODEGEN_UNITS = '1'
$env:CARGO_PROFILE_DEV_CODEGEN_UNITS = '1'
$env:TEMP = Join-Path $root 'temp'
$env:TMP = $env:TEMP
$process = [Diagnostics.Process]::GetCurrentProcess()
$process.PriorityClass = [Diagnostics.ProcessPriorityClass]::BelowNormal
$hashes = [ordered]@{}
foreach ($file in (Get-ChildItem -LiteralPath $source -Recurse -File -Force | Sort-Object FullName)) {
    $relative=$file.FullName.Substring($source.TrimEnd('\').Length+1).Replace('\','/')
    $hashes[$relative]=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
}
[IO.File]::WriteAllText((Join-Path $root 'source-hashes.json'),($hashes|ConvertTo-Json -Depth 4),$utf8)
$recorded = @([IO.File]::ReadAllLines((Join-Path $source 'bench/native/guest/records.tsv')) | Where-Object { $_.StartsWith("validation_run`t") } | ForEach-Object { [long]($_.Split("`t")[2]) })
if ($recorded.Count -eq 0) { throw 'missing existing validation guard record' }
$guardMs=[int]([Math]::Ceiling(2*($recorded | Measure-Object -Maximum).Maximum/1000))
$result = [ordered]@{
    source_revision=$revision; archive_sha256=$expectedArchive; root=$root
    os=[Environment]::OSVersion.VersionString; arch=[Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    needs='cpus=1,mem=4g,disk=12g'; priority='BelowNormal'; driver_pid=$process.Id
    cargo=(& $cargo --version | Out-String).Trim(); rustc=(& $rustc --version | Out-String).Trim()
    source_hashes=$hashes; phases=[Collections.Generic.List[object]]::new()
    scope='__CHECK_SCOPE__'
}
function Save-Result([string]$name) {
    [IO.File]::WriteAllText((Join-Path $root $name), ($result | ConvertTo-Json -Depth 16), $utf8)
}
function Run-Phase([string]$name, [string[]]$arguments) {
    $stdoutPath=Join-Path $root ($name+'.stdout.log')
    $stderrPath=Join-Path $root ($name+'.stderr.log')
    $stdout=[IO.File]::Open($stdoutPath,[IO.FileMode]::CreateNew)
    $stderr=[IO.File]::Open($stderrPath,[IO.FileMode]::CreateNew)
    $start=[Diagnostics.ProcessStartInfo]::new()
    $start.FileName=$cargo; $start.Arguments=($arguments -join ' '); $start.WorkingDirectory=$source
    $start.UseShellExecute=$false; $start.RedirectStandardOutput=$true; $start.RedirectStandardError=$true
    $child=[Diagnostics.Process]::new(); $child.StartInfo=$start
    $phase=[ordered]@{name=$name; executable=$cargo; arguments=$arguments}
    try {
        if (-not $child.Start()) { throw 'owned cargo did not start' }
        $phase.owned_pid=$child.Id; $result.phases.Add($phase); Save-Result 'run.json'
        $outCopy=$child.StandardOutput.BaseStream.CopyToAsync($stdout)
        $errCopy=$child.StandardError.BaseStream.CopyToAsync($stderr)
        if (-not $child.WaitForExit($guardMs)) {
            $phase.timed_out=$true; Save-Result 'run.json'
            throw ('owned cargo deadline reached; exact live PID retained '+$child.Id)
        }
        if (-not $outCopy.Wait($guardMs) -or -not $errCopy.Wait($guardMs)) { throw 'owned output drain deadline reached' }
        $phase.exit_code=$child.ExitCode; $phase.child_reaped=$true
    } finally { $stdout.Dispose(); $stderr.Dispose(); $child.Dispose() }
    $phase.stdout_sha256=(Get-FileHash -LiteralPath $stdoutPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $phase.stderr_sha256=(Get-FileHash -LiteralPath $stderrPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($name -eq 'windows-unit') {
        $text=[IO.File]::ReadAllText($stdoutPath)
        $phase.selected_lines=@($text -split '\r?\n' | Where-Object { $_ -match '^test windows::.*device_.* \.\.\. (ok|FAILED|ignored)$' })
        $phase.summaries=@($text -split '\r?\n' | Where-Object { $_.StartsWith('test result:') })
        $phase.expected_selected_ok=($phase.selected_lines.Count -eq __EXPECTED_TESTS__ -and @($phase.selected_lines | Where-Object { -not $_.EndsWith(' ... ok') }).Count -eq 0)
    }
    Save-Result 'run.json'
    return ($phase.exit_code -eq 0 -and ($name -ne 'windows-unit' -or $phase.expected_selected_ok))
}
$okay=Run-Phase 'windows-gpu' @('test','-p','hex-gpu','--test','hardware','--locked','--','--nocapture','--test-threads=1')
$raw=[IO.File]::ReadAllText((Join-Path $root 'windows-gpu.stdout.log'))+[IO.File]::ReadAllText((Join-Path $root 'windows-gpu.stderr.log'))
$result.actual_gpu_required=($raw.Contains('1 passed; 0 failed; 0 ignored;') -and $raw.Contains('found Gpu { vendor: Amd') -and $raw.Contains('sample 1:') -and -not $raw.Contains('skipped:'))
$okay=$okay -and $result.actual_gpu_required
$result.remaining_temp_entries=@(Get-ChildItem -LiteralPath (Join-Path $root 'temp') -Force).Count
$result.source_unchanged=(@(Get-ChildItem -LiteralPath $source -Recurse -File -Force).Count -eq $hashes.Count)
foreach ($key in $hashes.Keys) {
    if ((Get-FileHash -LiteralPath (Join-Path $source $key) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $hashes[$key]) { $result.source_unchanged=$false }
}
$result.exit_code=if ($okay -and $result.source_unchanged) {0} else {1}
Save-Result 'result.json'
[Console]::WriteLine((@{source=$revision;exit=$result.exit_code;actual_gpu_required=$result.actual_gpu_required;source_unchanged=$result.source_unchanged;phases=$result.phases;temp_entries=$result.remaining_temp_entries}|ConvertTo-Json -Depth 16))
exit $result.exit_code
