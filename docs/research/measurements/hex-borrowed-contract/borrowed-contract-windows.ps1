$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = '__OWN_ROOT__'
$revision = '__SOURCE_REVISION__'
if (Test-Path -LiteralPath $root) { throw 'private borrowed root exists' }
[IO.Directory]::CreateDirectory($root) | Out-Null
$utf8 = [Text.UTF8Encoding]::new($false)
$archive = Join-Path $root 'source.tar.gz'
[IO.File]::Copy('__SOURCE_ARCHIVE_FILE__', $archive, $false)
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne '__ARCHIVE_SHA__') { throw 'source archive mismatch' }
$source = Join-Path $root 'source'
[IO.Directory]::CreateDirectory($source) | Out-Null
& C:\Windows\System32\tar.exe -xzf $archive -C $source
if ($LASTEXITCODE -ne 0) { throw 'source extraction failed' }
foreach ($directory in @('target','cache\cargo','temp')) { [IO.Directory]::CreateDirectory((Join-Path $root $directory)) | Out-Null }
$hashes = [ordered]@{}
foreach ($file in (Get-ChildItem -LiteralPath $source -Recurse -File -Force | Sort-Object FullName)) {
    $relative = $file.FullName.Substring($source.TrimEnd('\').Length+1).Replace('\','/')
    $hashes[$relative] = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
}
[IO.File]::WriteAllText((Join-Path $root 'source-hashes.json'), ($hashes | ConvertTo-Json -Depth 4), $utf8)
$seed = '__UPSTREAM_LOCK__'
if ((Get-FileHash -LiteralPath $seed -Algorithm SHA256).Hash.ToLowerInvariant() -ne '__SEED_SHA__') { throw 'lock seed mismatch' }
[IO.File]::Copy($seed, (Join-Path $source 'vendor\shards\Cargo.lock'), $false)
[IO.File]::Copy($seed, (Join-Path $root 'upstream.lock'), $false)
$cargo = (& rustup which --toolchain 1.99.1 cargo).Trim()
if ($LASTEXITCODE -ne 0) { throw 'pinned cargo unavailable' }
$rustc = (& rustup which --toolchain 1.99.1 rustc).Trim()
if ($LASTEXITCODE -ne 0) { throw 'pinned rustc unavailable' }
$env:PATH = [IO.Path]::GetDirectoryName($cargo) + ';' + $env:PATH
$env:RUSTC = $rustc
$env:CARGO_HOME = Join-Path $root 'cache\cargo'
$env:CARGO_TARGET_DIR = Join-Path $root 'target'
$env:CARGO_BUILD_JOBS = '1'
$env:RUST_TEST_THREADS = '1'
$env:RAYON_NUM_THREADS = '1'
$env:CARGO_PROFILE_TEST_CODEGEN_UNITS = '1'
$env:CARGO_PROFILE_DEV_CODEGEN_UNITS = '1'
$env:HEX_BORROWED_CASE_ROUNDS = '64'
$env:TEMP = Join-Path $root 'temp'
$env:TMP = $env:TEMP
$self = [Diagnostics.Process]::GetCurrentProcess()
$self.PriorityClass = [Diagnostics.ProcessPriorityClass]::BelowNormal
$recorded = @([IO.File]::ReadAllLines((Join-Path $source 'bench/native/guest/records.tsv')) | Where-Object { $_.StartsWith("validation_run`t") } | ForEach-Object { [long]($_.Split("`t")[2]) })
if ($recorded.Count -eq 0) { throw 'guard derivation absent' }
$guardMs = [int]([Math]::Ceiling(2*($recorded | Measure-Object -Maximum).Maximum/1000))
$result = [ordered]@{source=$revision;archive_sha256='__ARCHIVE_SHA__';seed_lock_sha256='__SEED_SHA__';os=[Environment]::OSVersion.VersionString;arch=[Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString();driver_pid=$self.Id;guard_ms=$guardMs;rounds=64;scope='ROOT borrowed process/socket and exact native Windows handle return; no VM/GPU/HCS';phases=[Collections.Generic.List[object]]::new()}
function Save-Result([string]$name) { [IO.File]::WriteAllText((Join-Path $root $name), ($result | ConvertTo-Json -Depth 16), $utf8) }
function Run-Phase([string]$name, [string]$executable, [string[]]$arguments, [string[]]$required) {
    $stdoutPath = Join-Path $root ($name+'.stdout.log')
    $stderrPath = Join-Path $root ($name+'.stderr.log')
    $stdout = [IO.File]::Open($stdoutPath,[IO.FileMode]::CreateNew)
    $stderr = [IO.File]::Open($stderrPath,[IO.FileMode]::CreateNew)
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName=$executable; $start.Arguments=($arguments -join ' '); $start.WorkingDirectory=$source
    $start.UseShellExecute=$false; $start.RedirectStandardOutput=$true; $start.RedirectStandardError=$true
    $child=[Diagnostics.Process]::new(); $child.StartInfo=$start
    $phase=[ordered]@{name=$name;executable=$executable;arguments=$arguments}
    try {
        if (-not $child.Start()) { throw 'owned child did not start' }
        $phase.pid=$child.Id; $result.phases.Add($phase); Save-Result 'run.json'
        $outCopy=$child.StandardOutput.BaseStream.CopyToAsync($stdout)
        $errCopy=$child.StandardError.BaseStream.CopyToAsync($stderr)
        if (-not $child.WaitForExit($guardMs)) {
            $phase.timed_out=$true; Save-Result 'run.json'
            & C:\Windows\System32\taskkill.exe /PID $child.Id /T /F | Out-Null
            if (-not $child.WaitForExit($guardMs)) { throw 'owned process tree cleanup unverified' }
        }
        if (-not $outCopy.Wait($guardMs) -or -not $errCopy.Wait($guardMs)) { throw 'owned output drain deadline' }
        $phase.exit=$child.ExitCode; $phase.reaped=$true
    } finally { $stdout.Dispose(); $stderr.Dispose(); $child.Dispose() }
    $raw=[IO.File]::ReadAllText($stdoutPath)+[IO.File]::ReadAllText($stderrPath)
    $phase.required=$true
    foreach ($needle in $required) { if (-not $raw.Contains($needle)) { $phase.required=$false } }
    $phase.stdout_sha256=(Get-FileHash -LiteralPath $stdoutPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $phase.stderr_sha256=(Get-FileHash -LiteralPath $stderrPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Save-Result 'run.json'
    if ($phase.exit -ne 0 -or -not $phase.required -or $phase.timed_out) { throw ($name+' failed') }
}
try {
    Run-Phase 'build' $cargo @('test','--manifest-path','vendor/shards/Cargo.toml','-p','shards-api','--lib','--test','hex_borrowed_contract','--no-run','--message-format=json') @()
    [IO.File]::Copy((Join-Path $source 'vendor\shards\Cargo.lock'), (Join-Path $root 'resolved.lock'), $false)
    $bins=@{}
    foreach ($line in [IO.File]::ReadAllLines((Join-Path $root 'build.stdout.log'))) {
        if ($line.StartsWith('{')) { $item=$line | ConvertFrom-Json; if ($item.reason -eq 'compiler-artifact' -and $item.executable) { $bins[$item.target.name]=$item.executable } }
    }
    if ($bins.Count -ne 2 -or -not $bins.ContainsKey('shards_api') -or -not $bins.ContainsKey('hex_borrowed_contract')) { throw 'unexpected artifact set' }
    $result.binaries=[ordered]@{}
    foreach ($key in $bins.Keys) { $result.binaries[$key]=@{path=$bins[$key];sha256=(Get-FileHash -LiteralPath $bins[$key] -Algorithm SHA256).Hash.ToLowerInvariant()} }
    Run-Phase 'canonical-borrowed' $bins['shards_api'] @('client::borrowed_tests::','--nocapture','--test-threads=1') @('8 passed; 0 failed; 0 ignored;')
    Run-Phase 'owned-zero' $bins['shards_api'] @('--exact','client::tests::a_patience_of_zero_dials_once','--nocapture','--test-threads=1') @('1 passed; 0 failed; 0 ignored;')
    Run-Phase 'owned-max' $bins['shards_api'] @('--exact','client::tests::a_patience_of_max_dials_until_the_guest_listens','--nocapture','--test-threads=1') @('1 passed; 0 failed; 0 ignored;')
    Run-Phase 'public-lifetime' $bins['hex_borrowed_contract'] @('--ignored','--exact','root_borrowed_drop_preserves_target_and_socket_and_dead_target_refuses','--nocapture','--test-threads=1') @('1 passed; 0 failed; 0 ignored;','borrowed_lifetime live_after_both_drops=true')
    Run-Phase 'native-handles' $bins['hex_borrowed_contract'] @('--ignored','--exact','root_borrowed_failures_and_drops_return_windows_handles','--nocapture','--test-threads=1') @('1 passed; 0 failed; 0 ignored;','borrowed_windows_handles rounds=64','all_rounds_equal=true')
    $result.exit=0
} catch { $result.exit=1; $result.error=$_.ToString() }
finally {
    $result.source_unchanged=$true
    $files=@(Get-ChildItem -LiteralPath $source -Recurse -File -Force | Where-Object { $_.FullName -ne (Join-Path $source 'vendor\shards\Cargo.lock') })
    if ($files.Count -ne $hashes.Count) { $result.source_unchanged=$false }
    foreach ($key in $hashes.Keys) { if ((Get-FileHash -LiteralPath (Join-Path $source $key) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $hashes[$key]) { $result.source_unchanged=$false } }
    $parent=Join-Path $root 'temp\shards-tests'
    if ((Test-Path -LiteralPath $parent) -and @(Get-ChildItem -LiteralPath $parent -Force).Count -eq 0) { [IO.Directory]::Delete($parent) }
    $result.temp_entries=@(Get-ChildItem -LiteralPath (Join-Path $root 'temp') -Force).Count
    if (-not $result.source_unchanged -or $result.temp_entries -ne 0) { $result.exit=1 }
    Save-Result 'result.json'
    [Console]::WriteLine(($result | ConvertTo-Json -Depth 16))
    if ($result.exit -ne 0 -and $result.phases.Count -gt 0) {
        $last=$result.phases[$result.phases.Count-1].name
        foreach ($stream in @('stdout','stderr')) { $log=[IO.File]::ReadAllText((Join-Path $root ($last+'.'+$stream+'.log'))); [Console]::WriteLine($log.Substring([Math]::Max(0,$log.Length-7000))) }
    }
}
exit $result.exit
