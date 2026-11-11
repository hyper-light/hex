# ROOT's separately requested all-target Node/CLI compile on immutable L3 successor93 of frozen c579; no test execution.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';Set-StrictMode -Version 3
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$base='C:\Users\corph\.cache\hex-native-windows-hcs';$origin=$base+'\winpreflight2';$incoming=$origin+'-incoming';$root=$base+'\93-node-cli-check-v1'
if(Test-Path -LiteralPath $root){throw 'owned compile receipt exists'}
[void][IO.Directory]::CreateDirectory($root);[void][IO.Directory]::CreateDirectory(($root+'\temp'))
$env:TEMP=$root+'\temp';$env:TMP=$env:TEMP;$env:CARGO_HOME=$origin+'\cargo';$env:CARGO_TARGET_DIR=$origin+'\target'
$env:CARGO_BUILD_JOBS='1';$env:RUST_TEST_THREADS='1';$env:RAYON_NUM_THREADS='1';$env:RUSTUP_AUTO_INSTALL='0';$env:CARGO_PROFILE_DEV_DEBUG='0';$env:CARGO_PROFILE_TEST_DEBUG='0'
$utf8=[Text.UTF8Encoding]::new($false)
function Hash([string]$path){return (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()}
function Save([string]$name,$value){[IO.File]::WriteAllText(($root+'\'+$name),($value|ConvertTo-Json -Depth 16),$utf8)}
$proof=ConvertFrom-Json ([IO.File]::ReadAllText(($origin+'\result.json')))
if($proof.source_revision -ne '93abeaae82d97aaf64271e8fc29908538788bbb8' -or $proof.source_archive_sha256 -ne '194b9647a3812c7ff44bb6362c8a6888fa28636cb289ec6ba25ac594f956bf73' -or -not $proof.phases[0].guard.reaped -or $proof.phases[0].guard.activeAfter -ne 0){throw 'original frozen build ownership/source proof invalid'}
if((Hash ($incoming+'\guard.cs')) -ne '25fe189bb01f69f9cb3212a3fa31633b72edce36fa367d7e4c655ab3ebe61244' -or (Hash ($incoming+'\policy-byte-equality.json')) -ne '6a658eef10e8802ed3d73ca09e5f5acf63b5f5c767c27196d429d73fba739388'){throw 'signed guard or policy receipt differs'}
Add-Type -TypeDefinition ([IO.File]::ReadAllText(($incoming+'\guard.cs')))
$cargo=(& rustup.exe which --toolchain 1.99.1 cargo).Trim();if($LASTEXITCODE -ne 0){throw 'existing pinned cargo missing'}
$rustc=(& rustup.exe which --toolchain 1.99.1 rustc).Trim();if($LASTEXITCODE -ne 0){throw 'existing pinned rustc missing'}
$lld='C:\Users\corph\.rustup\toolchains\1.99.1-x86_64-pc-windows-msvc\lib\rustlib\x86_64-pc-windows-msvc\bin\rust-lld.exe'
if((Hash $lld) -ne '53d7ea9474ddbb0f0eda351f2dace3259e77f1175537f64d321c232bc8d29b68'){throw 'pinned existing linker differs'}
$env:RUSTC=$rustc;$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=$lld;$env:RUSTFLAGS='-C linker-flavor=lld-link -C target-feature=+crt-static';$env:PATH=[IO.Path]::GetDirectoryName($cargo)+';'+$env:PATH
$source=$origin+'\source'
function Source-Manifest(){$values=[ordered]@{};foreach($file in (Get-ChildItem -LiteralPath $source -Recurse -File|Sort-Object FullName)){$values[$file.FullName.Substring($source.Length+1).Replace('\','/')]=Hash $file.FullName};return $values}
Save 'source-before.json' (Source-Manifest)
$expected='29d0d11df8cf1225a828d801b2ae063c36d4c6b2d92b0fe383bd1f3842d076e5'
if((Hash ($root+'\source-before.json')) -ne $expected){throw 'frozen source changed since original93 preflight fixture failure'}
$v=[ordered]@{kind='ROOT requested immutable L3 successor93 of frozen c579 Node/CLI all-target compile; no executable accepted/run';source_revision=$proof.source_revision;source_archive_sha256=$proof.source_archive_sha256;source_manifest_before_sha256=$expected;target=$env:CARGO_TARGET_DIR;target_state='reuse same exclusively owned target/cache from terminal same terminal93 lib-test build';needs='cpus=1,mem=4g,disk=12g';phases=@();native_tests=0;hcs_calls=0;gpu_dispatches=0;runtime_clippy='NOT RUN: excluded by ROOT due known unchanged lint failures';policy_revision='0a6d91b3de0f10a8397868aa3794adab1e09afdc';rules_sha256=$proof.rules_sha256;exit_code=1}
Save 'run.json' $v
$out=$root+'\check.stdout.log';$err=$root+'\check.stderr.log'
$run=[NativeWindowsGuard]::Build($cargo,'check -p hex-node -p hex-cli --all-targets --locked --message-format=json',$source,$out,$err,900000)
$v.phases+=@{name='node-cli-all-targets-compile';command=@($cargo,'check','-p','hex-node','-p','hex-cli','--all-targets','--locked','--message-format=json');guard=$run;stdout_sha256=(Hash $out);stderr_sha256=(Hash $err)}
Save 'source-after.json' (Source-Manifest);$v.source_unchanged=((Hash ($root+'\source-after.json')) -eq $expected);$v.temp_entries=@(Get-ChildItem -LiteralPath ($root+'\temp') -Force).Count
$v.exit_code=if($run.exitCode -eq 0 -and -not $run.timedOut -and $run.reaped -and $run.activeAfter -eq 0 -and $v.source_unchanged){0}else{1}
Save 'result.json' $v;[Console]::WriteLine(($v|ConvertTo-Json -Depth 16));Get-Content -LiteralPath $err -Tail 15
exit $v.exit_code
