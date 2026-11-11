$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
Set-StrictMode -Version 3
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$root='__ROOT__'
$incoming='__INCOMING__'
if(Test-Path -LiteralPath $root){throw 'owned root already exists'}
[void][IO.Directory]::CreateDirectory($root)
foreach($name in @('source','target','cargo','temp','scratch')){[void][IO.Directory]::CreateDirectory((Join-Path $root $name))}
$archive=Join-Path $incoming 'source.tar.gz'
if((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne '__SOURCE_SHA__'){throw 'source hash differs'}
& C:\Windows\System32\tar.exe -xzf $archive -C (Join-Path $root 'source')
if($LASTEXITCODE -ne 0){throw 'source extraction failed'}
$source=Join-Path $root 'source'
$utf8=[Text.UTF8Encoding]::new($false)
$env:TEMP=Join-Path $root 'temp';$env:TMP=$env:TEMP
$env:CARGO_HOME=Join-Path $root 'cargo';$env:CARGO_TARGET_DIR=Join-Path $root 'target'
$env:CARGO_BUILD_JOBS='1';$env:RUST_TEST_THREADS='1';$env:RAYON_NUM_THREADS='1';$env:RUSTUP_AUTO_INSTALL='0'
$cargo=(& rustup.exe which --toolchain 1.99.1 cargo).Trim();if($LASTEXITCODE -ne 0){throw 'pinned cargo unavailable'}
$rustc=(& rustup.exe which --toolchain 1.99.1 rustc).Trim();if($LASTEXITCODE -ne 0){throw 'pinned rustc unavailable'}
$env:RUSTC=$rustc;$env:PATH=[IO.Path]::GetDirectoryName($cargo)+';'+$env:PATH
Add-Type -TypeDefinition ([IO.File]::ReadAllText((Join-Path $incoming 'guard.cs')))
function Hash([string]$p){return (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant()}
function Save([string]$name,$value){[IO.File]::WriteAllText((Join-Path $root $name),($value|ConvertTo-Json -Depth 20),$utf8)}
$result=[ordered]@{source_revision='__REVISION__';source_archive_sha256='__SOURCE_SHA__';policy_revision='0c598386ca1f7c65c6e0634dba77983e4abffff2';rules_sha256='df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3';role='assigned adversarial native Windows/HCS';owned_paths=@('NEW docs/research/measurements/hex-native-windows-hcs/**','NEW crates/hex-cli/tests/native_windows_*','NEW crates/hex-gpu/tests/native_windows_*');runner='f766f09294f2e17e3b8a631ffd4e6a4b';needs='cpus=1,mem=4g,disk=12g';root=$root;priority='BelowNormal';driver_pid=$PID;rustc=(& $rustc -Vv|Out-String);phases=@();native_tests=0;boot=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o')}
$leaf=Join-Path $source 'crates\hex-gpu\tests\completion_windows_compute.rs'
$result.leaf_sha256=Hash $leaf
Save 'run.json' $result
$buildOut=Join-Path $root 'build.stdout.log';$buildErr=Join-Path $root 'build.stderr.log'
$build=[NativeWindowsGuard]::Run($cargo,'test -p hex-gpu --test completion_windows_compute --locked --no-run --message-format=json',$source,$buildOut,$buildErr,900000)
$result.phases+=@{name='build';command=@($cargo,'test','-p','hex-gpu','--test','completion_windows_compute','--locked','--no-run','--message-format=json');guard=$build;stdout_sha256=(Hash $buildOut);stderr_sha256=(Hash $buildErr)}
Save 'run.json' $result
if($build.timedOut -or -not $build.reaped -or $build.activeAfter -ne 0 -or $build.exitCode -ne 0){Save 'result.json' $result;Get-Content -LiteralPath $buildErr -Tail 60;throw 'build failed/cleanup uncertain; original retained'}
$artifacts=@(Get-Content -LiteralPath $buildOut | ForEach-Object {try{$row=$_|ConvertFrom-Json;if($row.reason -eq 'compiler-artifact' -and $row.target.name -eq 'completion_windows_compute' -and $row.executable){$row.executable}}catch{}})
if($artifacts.Count -ne 1){throw 'one exact host test binary required'}
$exe=$artifacts[0];$result.binary=$exe;$result.binary_sha256=Hash $exe
$env:HEX_WINDOWS_GPU_COMPUTE_INTERFACE='\\?\PCI#VEN_1002&DEV_1586&SUBSYS_1FB31043&REV_C1#4&35fe04f8&0&0041#{1ca05180-a699-450a-9a0c-de4fbe3ddd89}'
$env:HEX_WINDOWS_GPU_COMPUTE_LUID='0000000000014c4d'
$env:HEX_WINDOWS_GPU_COMPUTE_WAIT_MS='5000'
if($result.boot -ne '2026-11-11T03:04:32.5000000Z'){throw 'boot differs from v4; current independent identity query required before GPU work'}
Save 'run.json' $result
$out=Join-Path $root 'compute.stdout.log';$err=Join-Path $root 'compute.stderr.log'
$run=[NativeWindowsGuard]::Run($exe,'--ignored --exact root_windows_exact_adapter_executes_checked_kernel --nocapture --test-threads=1',$source,$out,$err,60000)
$result.native_tests=1
$result.phases+=@{name='host-compute';command=@($exe,'--ignored','--exact','root_windows_exact_adapter_executes_checked_kernel','--nocapture','--test-threads=1');guard=$run;stdout_sha256=(Hash $out);stderr_sha256=(Hash $err);interface=$env:HEX_WINDOWS_GPU_COMPUTE_INTERFACE;luid=$env:HEX_WINDOWS_GPU_COMPUTE_LUID;fence_budget_ms=5000}
$result.leaf_unchanged=((Hash $leaf) -eq $result.leaf_sha256)
$result.temp_entries=@(Get-ChildItem -LiteralPath (Join-Path $root 'temp') -Force).Count
$result.exit_code=if($run.exitCode -eq 0 -and $run.reaped -and -not $run.timedOut -and $run.activeAfter -eq 0 -and $result.leaf_unchanged){0}else{1}
Save 'result.json' $result
[Console]::WriteLine(($result|ConvertTo-Json -Depth 20))
Get-Content -LiteralPath $out;Get-Content -LiteralPath $err
exit $result.exit_code
