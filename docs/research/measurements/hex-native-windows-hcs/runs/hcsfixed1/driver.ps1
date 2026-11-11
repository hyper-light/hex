$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
Set-StrictMode -Version 3
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$root='C:/Users/corph/.cache/hex-native-windows-hcs/hcsfixed1'
$incoming='C:/Users/corph/.cache/hex-native-windows-hcs/hcsfixed1-incoming'
if(Test-Path -LiteralPath $root){throw 'owned root already exists'}
[void][IO.Directory]::CreateDirectory($root)
foreach($name in @('source','target','cargo','temp','scratch')){[void][IO.Directory]::CreateDirectory((Join-Path $root $name))}
$archive=Join-Path $incoming 'source.tar.gz'
if((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne 'a46ce6594ef3845316a081a136b3388858520b96702fc1603d7fa725a3081e66'){throw 'source hash differs'}
& C:\Windows\System32\tar.exe -xzf $archive -C (Join-Path $root 'source')
if($LASTEXITCODE -ne 0){throw 'source extraction failed'}
$source=Join-Path $root 'source'
$utf8=[Text.UTF8Encoding]::new($false)
$env:TEMP=Join-Path $root 'temp';$env:TMP=$env:TEMP
$env:CARGO_HOME=Join-Path $root 'cargo';$env:CARGO_TARGET_DIR=Join-Path $root 'target'
$env:CARGO_PROFILE_DEV_DEBUG='0';$env:CARGO_PROFILE_TEST_DEBUG='0'
$env:CARGO_BUILD_JOBS='1';$env:RUST_TEST_THREADS='1';$env:RAYON_NUM_THREADS='1';$env:RUSTUP_AUTO_INSTALL='0'
$cargo=(& rustup.exe which --toolchain 1.99.1 cargo).Trim();if($LASTEXITCODE -ne 0){throw 'pinned cargo unavailable'}
$rustc=(& rustup.exe which --toolchain 1.99.1 rustc).Trim();if($LASTEXITCODE -ne 0){throw 'pinned rustc unavailable'}
$lld='C:\Users\corph\.rustup\toolchains\1.99.1-x86_64-pc-windows-msvc\lib\rustlib\x86_64-pc-windows-msvc\bin\rust-lld.exe'
if((Get-FileHash -LiteralPath $lld -Algorithm SHA256).Hash.ToLowerInvariant() -ne '53d7ea9474ddbb0f0eda351f2dace3259e77f1175537f64d321c232bc8d29b68'){throw 'pinned already-installed Rust LLD differs'}
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=$lld
$env:RUSTFLAGS='-C linker-flavor=lld-link -C target-feature=+crt-static'
$env:RUSTC=$rustc;$env:PATH=[IO.Path]::GetDirectoryName($cargo)+';'+$env:PATH
Add-Type -TypeDefinition ([IO.File]::ReadAllText((Join-Path $incoming 'guard.cs')))
Add-Type -TypeDefinition ([IO.File]::ReadAllText((Join-Path $incoming 'artifact_verifier.cs'))) -OutputAssembly (Join-Path $root 'artifact-verifier.dll')
if($null -eq ('NativeArtifactVerifier' -as [type])){Add-Type -Path (Join-Path $root 'artifact-verifier.dll')}
function Hash([string]$p){return (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant()}
function Save([string]$name,$value){[IO.File]::WriteAllText((Join-Path $root $name),($value|ConvertTo-Json -Depth 20),$utf8)}
$result=[ordered]@{source_revision='985d7767232780f74ac95b2e2aa966130730b34d';source_archive_sha256='a46ce6594ef3845316a081a136b3388858520b96702fc1603d7fa725a3081e66';policy_revision='0c598386ca1f7c65c6e0634dba77983e4abffff2';rules_sha256='df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3';role='assigned adversarial native Windows/HCS';owned_paths=@('NEW docs/research/measurements/hex-native-windows-hcs/**','NEW crates/hex-cli/tests/native_windows_*','NEW crates/hex-gpu/tests/native_windows_*');runner='f766f09294f2e17e3b8a631ffd4e6a4b';needs='cpus=1,mem=4g,disk=12g';root=$root;priority='BelowNormal';driver_pid=$PID;rustc=(& $rustc -Vv|Out-String);phases=@();native_tests=0;boot=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o')}
$leaf=Join-Path $source 'crates\hex-gpu\tests\completion_windows_compute.rs'
$result.leaf_sha256=Hash $leaf
$result.build_cohort='new signed native HCS fixture cohort after reproduced missing-provider setup refusal; pinned existing LLD/flags, no production fix or linker/PDB cause conclusion'
$result.linker=$lld;$result.linker_sha256='53d7ea9474ddbb0f0eda351f2dace3259e77f1175537f64d321c232bc8d29b68';$result.rustflags=$env:RUSTFLAGS
$result.cargo_profile_dev_debug='0';$result.cargo_profile_test_debug='0'
$policy=Get-Content -LiteralPath (Join-Path $incoming 'policy-byte-equality.json') -Raw|ConvertFrom-Json
if($policy.rules_sha256 -ne $result.rules_sha256 -or @($policy.files.PSObject.Properties|Where-Object{-not $_.Value.byte_equal_to_fully_read_revision}).Count -ne 0){throw 'current policy byte-equality receipt invalid'}
$result.policy_current_revision=$policy.current_policy_revision;$result.policy_fully_read_revision=$policy.fully_read_revision;$result.policy_byte_equality_sha256=Hash (Join-Path $incoming 'policy-byte-equality.json')
$result.target_initially_empty=(@(Get-ChildItem -LiteralPath $env:CARGO_TARGET_DIR -Force).Count -eq 0)
if(-not $result.target_initially_empty){throw 'native artifact target was not initially empty'}
$result.target_canonical_before=[NativeArtifactVerifier]::Canonical($env:CARGO_TARGET_DIR)
function Source-Manifest(){
 $values=[ordered]@{}
 foreach($file in (Get-ChildItem -LiteralPath $source -Recurse -File|Sort-Object FullName)){$values[$file.FullName.Substring($source.Length+1).Replace('\','/')]=Hash $file.FullName}
 return $values
}
$before=Source-Manifest;Save 'source-before.json' $before
$result.source_manifest_before_sha256=Hash (Join-Path $root 'source-before.json')
Save 'run.json' $result
$buildOut=Join-Path $root 'build.stdout.log';$buildErr=Join-Path $root 'build.stderr.log'
$build=[NativeWindowsGuard]::Build($cargo,'test -p hex-cli --test native_windows_hcs_compute --locked --no-run --message-format=json',$source,$buildOut,$buildErr,900000)
$result.phases+=@{name='build';command=@($cargo,'test','-p','hex-cli','--test','native_windows_hcs_compute','--locked','--no-run','--message-format=json');guard=$build;stdout_sha256=(Hash $buildOut);stderr_sha256=(Hash $buildErr)}
Save 'run.json' $result
if($build.timedOut -or -not $build.reaped -or $build.activeAfter -ne 0 -or $build.exitCode -ne 0){Save 'result.json' $result;Get-Content -LiteralPath $buildErr -Tail 60;throw 'build failed/cleanup uncertain; original retained'}
$after=Source-Manifest;Save 'source-after-build.json' $after
$result.source_manifest_after_build_sha256=Hash (Join-Path $root 'source-after-build.json')
$result.source_unchanged_before_native=($result.source_manifest_before_sha256 -eq $result.source_manifest_after_build_sha256)
if(-not $result.source_unchanged_before_native){Save 'result.json' $result;throw 'source changed during build; native dispatch refused'}
$artifacts=@(Get-Content -LiteralPath $buildOut|ForEach-Object{
 $row=$_|ConvertFrom-Json
 if($row.reason -eq 'compiler-artifact' -and $row.target.name -eq 'native_windows_hcs_compute'){$row}
})
if($artifacts.Count -ne 1){throw 'one exact native test artifact required'}
$artifact=$artifacts[0]
if($artifact.fresh -cne $false -or $artifact.profile.test -cne $true -or $artifact.target.kind -notcontains 'test' -or $artifact.target.crate_types -notcontains 'bin' -or -not $artifact.executable){throw 'artifact must be newly emitted fresh:false test-profile test/bin, never a cache hit or path-only result'}
$expectedManifest=[NativeArtifactVerifier]::Canonical((Join-Path $source 'crates\hex-cli\Cargo.toml'))
$expectedLeaf=[NativeArtifactVerifier]::Canonical((Join-Path $source 'crates\hex-cli\tests\native_windows_hcs_compute.rs'))
if(-not [string]::Equals([NativeArtifactVerifier]::Canonical($artifact.manifest_path),$expectedManifest,[StringComparison]::OrdinalIgnoreCase) -or -not [string]::Equals([NativeArtifactVerifier]::Canonical($artifact.target.src_path),$expectedLeaf,[StringComparison]::OrdinalIgnoreCase)){throw 'artifact manifest/target source differs from exact pinned test'}
$targetNow=[NativeArtifactVerifier]::Canonical($env:CARGO_TARGET_DIR)
if(-not [string]::Equals($targetNow,$result.target_canonical_before,[StringComparison]::OrdinalIgnoreCase)){throw 'owned target canonical directory changed'}
$exe=[NativeArtifactVerifier]::Canonical($artifact.executable)
if(-not $exe.StartsWith(($result.target_canonical_before.TrimEnd('\')+'\'),[StringComparison]::OrdinalIgnoreCase)){throw 'artifact canonical path escaped initially empty owned target'}
$machine=[NativeArtifactVerifier]::PeMachine($exe)
if($machine -ne 34404 -or -not $result.rustc.Contains('host: x86_64-pc-windows-msvc')){throw 'artifact/toolchain target is not native amd64 Windows MSVC ABI'}
$result.artifact=$artifact;$result.artifact_machine='IMAGE_FILE_MACHINE_AMD64 0x8664';$result.binary=$exe;$result.binary_sha256=Hash $exe;$result.artifact_verified_before_dispatch=$true
Save 'run.json' $result
$result.production_base='2c8855b220a36c57685e456a7b8c98d8368f7899'
$result.hcs_fixture_sha256=Hash (Join-Path $source 'crates\hex-cli\tests\native_windows_hcs_compute.rs')
$result.native_dispatch='none: freshly emitted artifact only; separate guarded execution required'
$result.source_unchanged_after_build=$result.source_unchanged_before_native
$result.temp_entries=@(Get-ChildItem -LiteralPath (Join-Path $root 'temp') -Force).Count
$result.exit_code=0
Save 'result.json' $result
[Console]::WriteLine(($result|ConvertTo-Json -Depth 20))
exit 0
