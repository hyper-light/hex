# Execute the freshly emitted hcsllvm1 artifact through its canonical-equivalent DOS path.
# Original extended-path cmd.exe refusal remains retained; this changes no source, linker or binary.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';Set-StrictMode -Version 3
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$root='C:\Users\corph\.cache\hex-native-windows-hcs\hcs-gpu-exec-v1'
if(Test-Path -LiteralPath $root){throw 'owned execution root exists'}
[void][IO.Directory]::CreateDirectory($root)
foreach($name in @('temp','scratch')){[void][IO.Directory]::CreateDirectory((Join-Path $root $name))}
$env:TEMP=Join-Path $root 'temp';$env:TMP=$env:TEMP
$env:RUST_TEST_THREADS='1';$env:RAYON_NUM_THREADS='1'
$utf8=[Text.UTF8Encoding]::new($false)
function Hash([string]$p){return (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant()}
function Save([string]$name,$value){[IO.File]::WriteAllText((Join-Path $root $name),($value|ConvertTo-Json -Depth 20),$utf8)}
$origin='C:\Users\corph\.cache\hex-native-windows-hcs\hcsllvm1'
$incoming=$origin+'-incoming'
if((Hash ($incoming+'\guard.cs')) -ne '25fe189bb01f69f9cb3212a3fa31633b72edce36fa367d7e4c655ab3ebe61244' -or (Hash ($incoming+'\artifact_verifier.cs')) -ne 'ba9fd6f413f1c108633f9ebb0da617dc6fe5ac93c19f412f4b3cbe432318ff4e'){throw 'exact signed artifact verifier/guard source changed'}
Add-Type -TypeDefinition ([IO.File]::ReadAllText(($incoming+'\guard.cs')))
Add-Type -TypeDefinition ([IO.File]::ReadAllText(($incoming+'\artifact_verifier.cs')))
$proof=Get-Content -LiteralPath ($origin+'\result.json') -Raw|ConvertFrom-Json
$artifact=$proof.artifact
if($proof.source_revision -ne 'ed6d3e54db61fb8407418868ca086a479f2eeb5e' -or $proof.source_archive_sha256 -ne '40cebb529fd46e59256feced0a67c4b5a1d9c050444858ff2930735dfca40157' -or -not $proof.target_initially_empty -or -not $proof.artifact_verified_before_dispatch -or -not $proof.source_unchanged_before_native -or -not $proof.source_unchanged_after_native){throw 'original fresh-build/source proof invalid'}
if($artifact.fresh -cne $false -or $artifact.profile.test -cne $true -or $artifact.target.kind -notcontains 'test' -or $artifact.target.crate_types -notcontains 'bin' -or $artifact.target.name -ne 'native_windows_hcs_compute'){throw 'not the single freshly emitted exact test artifact'}
$source=$origin+'\source'
if([NativeArtifactVerifier]::Canonical($artifact.manifest_path) -ne [NativeArtifactVerifier]::Canonical(($source+'\crates\hex-cli\Cargo.toml')) -or [NativeArtifactVerifier]::Canonical($artifact.target.src_path) -ne [NativeArtifactVerifier]::Canonical(($source+'\crates\hex-cli\tests\native_windows_hcs_compute.rs'))){throw 'wrong manifest/test source'}
$target=[NativeArtifactVerifier]::Canonical(($origin+'\target'))
if($target -ne $proof.target_canonical_before){throw 'initially empty target identity changed'}
$exe=[IO.Path]::GetFullPath($artifact.executable)
if(-not $exe.StartsWith('C:\',[StringComparison]::OrdinalIgnoreCase) -or $exe.Length -ge 260){throw 'explicit bounded DOS executable alias required'}
$canonical=[NativeArtifactVerifier]::Canonical($exe)
if($canonical -ne $proof.binary -or -not $canonical.StartsWith(($target.TrimEnd('\')+'\'),[StringComparison]::OrdinalIgnoreCase)){throw 'DOS alias does not name the same canonical emitted artifact'}
$binaryHash='d459c4803f79aa238ccf53514fd76e1c92d32df4e9113f06b56298d7e1f7dd38'
if((Hash $exe) -ne $binaryHash -or [NativeArtifactVerifier]::PeMachine($exe) -ne 34404){throw 'verified fresh binary changed'}
function Source-Manifest(){
 $values=[ordered]@{}
 foreach($file in (Get-ChildItem -LiteralPath $source -Recurse -File|Sort-Object FullName)){$values[$file.FullName.Substring($source.Length+1).Replace('\','/')]=Hash $file.FullName}
 return $values
}
Save 'source-before.json' (Source-Manifest)
$expectedSource='4eccc885a65b2839ec45c17fd2aaecaf9e70bdd003d386d7f70353496b21da86'
if((Hash ($root+'\source-before.json')) -ne $expectedSource){throw 'post-build pinned source manifest changed before actual HCS'}
if((Hash ($incoming+'\policy-byte-equality.json')) -ne '6a658eef10e8802ed3d73ca09e5f5acf63b5f5c767c27196d429d73fba739388'){throw 'exact current policy receipt changed'}
$result=[ordered]@{kind='actual selected HCS compute/recovery on completed pinned full image; same fresh LLD artifact, no rebuild';origin=$origin;origin_result_sha256=(Hash ($origin+'\result.json'));source_revision=$proof.source_revision;source_archive_sha256=$proof.source_archive_sha256;source_manifest_before_sha256=$expectedSource;artifact=$artifact;binary=$exe;binary_canonical=$canonical;binary_sha256=$binaryHash;needs='cpus=1,mem=4g,disk=12g';policy_current_revision='0a6d91b3de0f10a8397868aa3794adab1e09afdc';rules_sha256=$proof.rules_sha256;policy_byte_equality_sha256=$proof.policy_byte_equality_sha256;phases=@();native_tests=0;boot=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o')}
Save 'run.json' $result
$listOut=$root+'\list.stdout.log';$listErr=$root+'\list.stderr.log'
$list=[NativeWindowsGuard]::Run($exe,'--list --format terse',$source,$listOut,$listErr,10000)
$result.phases+=@{name='DOS-alias-load-and-exact-test-list';guard=$list;stdout_sha256=(Hash $listOut);stderr_sha256=(Hash $listErr)}
if($list.exitCode -ne 0 -or $list.timedOut -or -not $list.reaped -or $list.activeAfter -ne 0 -or -not ([IO.File]::ReadAllText($listOut)).Contains('native_windows_hcs_selected_compute_survives_restart: test')){Save 'result.json' $result;throw 'same artifact DOS load/test-list failed before HCS'}
$imageReceipt='C:\Users\corph\.cache\hex-native-windows-hcs\post-import-verify-v4\result.json'
if(-not (Test-Path -LiteralPath $imageReceipt)){throw 'UNVERIFIED full pinned image verification incomplete; no GPU dispatch'}
$preparedImage=Get-Content -LiteralPath $imageReceipt -Raw|ConvertFrom-Json
if($preparedImage.exit_code -ne 0 -or $preparedImage.verified_for_execution -ne $true -or $preparedImage.original_guard.activeAfter -ne 0 -or -not $preparedImage.original_guard.reaped -or $null -eq $preparedImage.charged_post -or $preparedImage.charged_post.allocatedBytes -gt 34359738368L){throw 'UNVERIFIED full image/reaping/disk evidence incomplete; no GPU dispatch'}
if($preparedImage.reference -ne 'mcr.microsoft.com/windows/server@sha256:a62109569303cfe0ab49b1618383615f0477cdc285529b98d9b7085902d0e745' -or $preparedImage.layers.Count -ne 2 -or @($preparedImage.required_dlls|Where-Object{-not $_.present}).Count -ne 0){throw 'UNVERIFIED exact full image/head/DLL prerequisites; no GPU dispatch'}
$result.image_receipt_sha256=Hash $imageReceipt;$result.full_image_reference=$preparedImage.reference;$result.layer_inputs=$preparedImage.layers;$result.full_image_dlls=$preparedImage.required_dlls
$result.original_image_controller_error_preserved=$preparedImage.original_error
$compute='C:\Users\corph\.cache\hex-native-windows-hcs\hostv3\completion_windows_compute.exe'
$computeHash='4fdd12df17fbf9e97b8be3ea68fb22344294cbe2f71c8f36de14c8a94034f6e9'
if((Hash $compute) -ne $computeHash){throw 'host control binary changed'}
$env:HEX_NATIVE_WINDOWS_COMPUTE_EXE=$compute
$env:HEX_NATIVE_WINDOWS_COMPUTE_SHA256=$computeHash
$env:HEX_WINDOWS_GPU_COMPUTE_LUID='0000000000014c4d'
$env:HEX_WINDOWS_DEVICE_INTERFACE='\\?\PCI#VEN_1002&DEV_1586&SUBSYS_1FB31043&REV_C1#4&35fe04f8&0&0041#{1ca05180-a699-450a-9a0c-de4fbe3ddd89}'
$env:HEX_WINDOWS_DEVICE_CLASS='1ca05180-a699-450a-9a0c-de4fbe3ddd89'
$env:HEX_WINDOWS_DEVICE_KEY='50176'
$env:HEX_WINDOWS_LAYERS=($preparedImage.layers -join ';')
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
# The actual image import/head/DLL and owned cleanup receipts gate this dispatch.
$result.hcs_gpu_execution='actual selected and CPU-sibling compute test dispatched only after full image evidence; no prior pass claim'
Save 'run.json' $result
$out=Join-Path $root 'hcs.stdout.log';$err=Join-Path $root 'hcs.stderr.log'
$run=[NativeWindowsGuard]::Run($exe,'--ignored --exact native_windows_hcs_selected_compute_survives_restart --nocapture --test-threads=1',$source,$out,$err,180000)
$result.native_tests=if(([IO.File]::ReadAllText($out)).Contains('running 1 test')){1}else{0}
$result.phases+=@{name='hcs-selected-gpu-compute-exclusion-recovery';command=@($exe,'--ignored','--exact','native_windows_hcs_selected_compute_survives_restart','--nocapture','--test-threads=1');guard=$run;stdout_sha256=(Hash $out);stderr_sha256=(Hash $err);interface=$env:HEX_WINDOWS_DEVICE_INTERFACE;luid=$env:HEX_WINDOWS_GPU_COMPUTE_LUID;caller_wait_ms=5000}
$postNative=Source-Manifest;Save 'source-after-native.json' $postNative
$result.source_unchanged_after_native=((Hash (Join-Path $root 'source-after-native.json')) -eq $expectedSource)
$result.temp_entries=@(Get-ChildItem -LiteralPath (Join-Path $root 'temp') -Force).Count
$result.exit_code=if($run.exitCode -eq 0 -and $run.reaped -and -not $run.timedOut -and $run.activeAfter -eq 0 -and $result.source_unchanged_after_native){0}else{1}
Save 'result.json' $result
[Console]::WriteLine(($result|ConvertTo-Json -Depth 20))
Get-Content -LiteralPath $out;Get-Content -LiteralPath $err
exit $result.exit_code
