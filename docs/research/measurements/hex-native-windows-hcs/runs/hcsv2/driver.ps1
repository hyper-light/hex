$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
Set-StrictMode -Version 3
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$root='C:/Users/corph/.cache/hex-native-windows-hcs/hcsv2'
$incoming='C:/Users/corph/.cache/hex-native-windows-hcs/hcsv2-incoming'
if(Test-Path -LiteralPath $root){throw 'owned root already exists'}
[void][IO.Directory]::CreateDirectory($root)
foreach($name in @('source','target','cargo','temp','scratch')){[void][IO.Directory]::CreateDirectory((Join-Path $root $name))}
$archive=Join-Path $incoming 'source.tar.gz'
if((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne '40cebb529fd46e59256feced0a67c4b5a1d9c050444858ff2930735dfca40157'){throw 'source hash differs'}
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
$result=[ordered]@{source_revision='ed6d3e54db61fb8407418868ca086a479f2eeb5e';source_archive_sha256='40cebb529fd46e59256feced0a67c4b5a1d9c050444858ff2930735dfca40157';policy_revision='0c598386ca1f7c65c6e0634dba77983e4abffff2';rules_sha256='df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3';role='assigned adversarial native Windows/HCS';owned_paths=@('NEW docs/research/measurements/hex-native-windows-hcs/**','NEW crates/hex-cli/tests/native_windows_*','NEW crates/hex-gpu/tests/native_windows_*');runner='f766f09294f2e17e3b8a631ffd4e6a4b';needs='cpus=1,mem=4g,disk=12g';root=$root;priority='BelowNormal';driver_pid=$PID;rustc=(& $rustc -Vv|Out-String);phases=@();native_tests=0;boot=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o')}
$leaf=Join-Path $source 'crates\hex-gpu\tests\completion_windows_compute.rs'
$result.leaf_sha256=Hash $leaf
Save 'run.json' $result
$buildOut=Join-Path $root 'build.stdout.log';$buildErr=Join-Path $root 'build.stderr.log'
$build=[NativeWindowsGuard]::Build($cargo,'test -p hex-cli --test native_windows_hcs_compute --locked --no-run --message-format=json',$source,$buildOut,$buildErr,900000)
$result.phases+=@{name='build';command=@($cargo,'test','-p','hex-cli','--test','native_windows_hcs_compute','--locked','--no-run','--message-format=json');guard=$build;stdout_sha256=(Hash $buildOut);stderr_sha256=(Hash $buildErr)}
Save 'run.json' $result
if($build.timedOut -or -not $build.reaped -or $build.activeAfter -ne 0 -or $build.exitCode -ne 0){Save 'result.json' $result;Get-Content -LiteralPath $buildErr -Tail 60;throw 'build failed/cleanup uncertain; original retained'}
$artifacts=@(Get-Content -LiteralPath $buildOut | ForEach-Object {try{$row=$_|ConvertFrom-Json;if($row.reason -eq 'compiler-artifact' -and $row.target.name -eq 'native_windows_hcs_compute' -and $row.executable){$row.executable}}catch{}})
if($artifacts.Count -ne 1){throw 'one exact host test binary required'}
$exe=$artifacts[0];$result.binary=$exe;$result.binary_sha256=Hash $exe
$compute='C:\Users\corph\.cache\hex-native-windows-hcs\hostv3\completion_windows_compute.exe'
$computeHash='4fdd12df17fbf9e97b8be3ea68fb22344294cbe2f71c8f36de14c8a94034f6e9'
if((Hash $compute) -ne $computeHash){throw 'host control binary changed'}
$env:HEX_NATIVE_WINDOWS_COMPUTE_EXE=$compute
$env:HEX_NATIVE_WINDOWS_COMPUTE_SHA256=$computeHash
$env:HEX_WINDOWS_GPU_COMPUTE_LUID='0000000000014c4d'
$env:HEX_WINDOWS_DEVICE_INTERFACE='\\?\PCI#VEN_1002&DEV_1586&SUBSYS_1FB31043&REV_C1#4&35fe04f8&0&0041#{1ca05180-a699-450a-9a0c-de4fbe3ddd89}'
$env:HEX_WINDOWS_DEVICE_CLASS='1ca05180-a699-450a-9a0c-de4fbe3ddd89'
$env:HEX_WINDOWS_DEVICE_KEY='50176'
$env:HEX_WINDOWS_LAYERS='C:\Users\corph\.cache\hex-native-s4\ctrd\root\io.containerd.snapshotter.v1.windows\snapshots\1'
$env:HEX_WINDOWS_DEVICE_SCRATCH=Join-Path $root 'scratch'
$env:HEX_WINDOWS_DEVICE_BOUNDS='{"cpu_max_milli":500,"memory_max":536870912}'
$env:HEX_WINDOWS_DEVICE_WAIT_MS='5000'
$env:HEX_WINDOWS_DEVICE_CATALOG=Join-Path $root 'devices'
$catalog="hex-devices-1`ngpu`t50176`tAMD Radeon(TM) 8060S Graphics`t4294967296`thcs`tPCIROOT(0)#PCI(0801)#PCI(0000)`t`n"
[IO.File]::WriteAllText($env:HEX_WINDOWS_DEVICE_CATALOG,$catalog,$utf8)
$result.catalog_sha256=Hash $env:HEX_WINDOWS_DEVICE_CATALOG
$result.compute_binary_sha256=$computeHash
$result.hcs_fixture_sha256=Hash (Join-Path $source 'crates\hex-cli\tests\native_windows_hcs_compute.rs')
if($result.boot -ne '2026-11-11T03:04:32.5000000Z'){throw 'boot changed; independent current join required'}
Save 'run.json' $result
# Known Nano Server lacks D3D12/DXGI/GDI. Run only the independently valid CPU lifecycle control.
# Selected compute remains explicitly unavailable until a GPU-capable pinned full Windows layer exists.
$system=Join-Path ($env:HEX_WINDOWS_LAYERS+'\Files') 'Windows\System32'
$result.compute_prerequisites=@(foreach($name in @('gdi32.dll','dxgi.dll','d3d12.dll','d3dcompiler_47.dll')){@{name=$name;exists=(Test-Path -LiteralPath (Join-Path $system $name))}})
$result.hcs_gpu_execution='UNVERIFIED: pinned available Nano image lacks required GPU user-mode API DLLs; selected compute not dispatched'
Save 'run.json' $result
$out=Join-Path $root 'hcs.stdout.log';$err=Join-Path $root 'hcs.stderr.log'
$run=[NativeWindowsGuard]::Run($exe,'--ignored --exact native_windows_hcs_owned_cpu_recovery_and_cleanup --nocapture --test-threads=1',$source,$out,$err,180000)
$result.native_tests=1
$result.phases+=@{name='hcs-ordinary-cpu-recovery';command=@($exe,'--ignored','--exact','native_windows_hcs_owned_cpu_recovery_and_cleanup','--nocapture','--test-threads=1');guard=$run;stdout_sha256=(Hash $out);stderr_sha256=(Hash $err);interface=$env:HEX_WINDOWS_DEVICE_INTERFACE;luid=$env:HEX_WINDOWS_GPU_COMPUTE_LUID;caller_wait_ms=5000}
$result.temp_entries=@(Get-ChildItem -LiteralPath (Join-Path $root 'temp') -Force).Count
$result.exit_code=if($run.exitCode -eq 0 -and $run.reaped -and -not $run.timedOut -and $run.activeAfter -eq 0){0}else{1}
Save 'result.json' $result
[Console]::WriteLine(($result|ConvertTo-Json -Depth 20))
Get-Content -LiteralPath $out;Get-Content -LiteralPath $err
exit $result.exit_code
