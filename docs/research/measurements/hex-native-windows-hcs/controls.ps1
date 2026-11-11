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

$env:HEX_WINDOWS_GPU_COMPUTE_INTERFACE='\\?\PCI#VEN_1002&DEV_1586&SUBSYS_1FB31043&REV_C1#4&35fe04f8&0&0041#{1ca05180-a699-450a-9a0c-de4fbe3ddd89}'
$env:HEX_WINDOWS_GPU_COMPUTE_LUID='0000000000014c4d';$env:HEX_WINDOWS_GPU_COMPUTE_WAIT_MS='5000'
if($result.boot -ne '2026-11-11T03:04:32.5000000Z'){throw 'boot changed; current identity join required'}
$original=[IO.File]::ReadAllBytes($leaf)
$originalHash=Hash $leaf
function Run-Witness([string]$name,[string]$binary,[bool]$expectedSuccess){
 $out=Join-Path $root ($name+'.stdout.log');$err=Join-Path $root ($name+'.stderr.log')
 $g=[NativeWindowsGuard]::Run($binary,'--ignored --exact root_windows_exact_adapter_executes_checked_kernel --nocapture --test-threads=1',$source,$out,$err,60000)
 $text=[IO.File]::ReadAllText($out)+[IO.File]::ReadAllText($err)
 $p=[ordered]@{name=$name;binary=$binary;binary_sha256=(Hash $binary);guard=$g;stdout_sha256=(Hash $out);stderr_sha256=(Hash $err);expected_success=$expectedSuccess;luid=$env:HEX_WINDOWS_GPU_COMPUTE_LUID}
 $p.validated=($g.reaped -and -not $g.timedOut -and $g.activeAfter -eq 0 -and (($g.exitCode -eq 0) -eq $expectedSuccess))
 if($name -eq 'wrong-luid'){$p.validated=($p.validated -and $text.Contains('KMT identity') -and $text.Contains('kmt=1/1 events=0/0 close_failed=false') -and -not $text.Contains('windows_compute_selected') -and -not $text.Contains('windows_compute_fence'))}
 if($name -eq 'missing-store'){$p.validated=($p.validated -and $text.Contains('GPU oracle: index=0 expected=1 actual=4294967295') -and $text.Contains('windows_compute_fence wait=0x0 value=1 device=Ok(())') -and $text.Contains('kmt=1/1 events=1/1 close_failed=false'))}
 if($expectedSuccess){$p.validated=($p.validated -and $text.Contains('elements=1024 mismatches=0') -and $text.Contains('kmt=1/1 events=1/1 close_failed=false'))}
 $result.native_tests+=1;$result.phases+=,$p;Save 'run.json' $result
 [Console]::WriteLine($text)
 return $p.validated
}
$controlBinary='C:\Users\corph\.cache\hex-native-windows-hcs\hostv3\completion_windows_compute.exe'
if((Hash $controlBinary) -ne '4fdd12df17fbf9e97b8be3ea68fb22344294cbe2f71c8f36de14c8a94034f6e9'){throw 'original host binary changed'}
$env:HEX_WINDOWS_GPU_COMPUTE_LUID='0000000000014c4c'
$okay=Run-Witness 'wrong-luid' $controlBinary $false
$env:HEX_WINDOWS_GPU_COMPUTE_LUID='0000000000014c4d'
if(-not $okay){Save 'result.json' $result;throw 'wrong-ID control failed for unrelated reason'}
$text=[Text.Encoding]::UTF8.GetString($original)
$store='output_words[thread_id.x] = input_words[thread_id.x] + thread_id.x + 1u;'
if(($text.Split([string[]]@($store),[StringSplitOptions]::None)).Count -ne 2){throw 'unique required output store seam absent'}
$mutation=$text.Replace($store,'// owned adversarial mutation: deliberately omit required output store')
[IO.File]::WriteAllText($leaf,$mutation,$utf8)
$result.mutation=[ordered]@{file=$leaf;original_sha256=$originalHash;mutated_sha256=(Hash $leaf);change='remove only required output store from embedded deterministic shader';restored=$false}
Save 'run.json' $result
function Build-Witness([string]$name){
 $out=Join-Path $root ($name+'.stdout.log');$err=Join-Path $root ($name+'.stderr.log')
 $g=[NativeWindowsGuard]::Build($cargo,'test -p hex-gpu --test completion_windows_compute --locked --no-run --message-format=json',$source,$out,$err,900000)
 $result.phases+=@{name=$name;guard=$g;stdout_sha256=(Hash $out);stderr_sha256=(Hash $err)};Save 'run.json' $result
 if($g.exitCode -ne 0 -or -not $g.reaped -or $g.timedOut -or $g.activeAfter -ne 0){Get-Content -LiteralPath $err -Tail 50;throw 'native witness build failed'}
 $artifacts=@(Get-Content -LiteralPath $out|ForEach-Object{try{$row=$_|ConvertFrom-Json;if($row.reason -eq 'compiler-artifact' -and $row.target.name -eq 'completion_windows_compute' -and $row.executable){$row.executable}}catch{}})
 if($artifacts.Count -ne 1){throw 'one exact witness executable required'}
 return $artifacts[0]
}
try {
 $mutated=Build-Witness 'mutation-build'
 $savedMutation=Join-Path $root 'missing-store.exe';[IO.File]::Copy($mutated,$savedMutation,$false)
 $okay=Run-Witness 'missing-store' $savedMutation $false
} finally {
 [IO.File]::WriteAllBytes($leaf,$original)
 $result.mutation.restored=((Hash $leaf) -eq $originalHash);Save 'run.json' $result
}
if(-not $okay -or -not $result.mutation.restored){Save 'result.json' $result;throw 'mutation did not fail at checked-output guard or source restore differed'}
$restored=Build-Witness 'restored-build'
$okay=Run-Witness 'restored' $restored $true
$result.temp_entries=@(Get-ChildItem -LiteralPath (Join-Path $root 'temp') -Force).Count
$result.exit_code=if($okay -and $result.mutation.restored){0}else{1}
Save 'result.json' $result
[Console]::WriteLine(($result|ConvertTo-Json -Depth 20))
exit $result.exit_code
