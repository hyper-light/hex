# Read-only verification of completed import; original controller failure remains byte-exact.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$base='C:\Users\corph\.cache\hex-native-windows-hcs';$root=$base+'\post-import-verify-v2'
if(Test-Path -LiteralPath $root){throw 'owned verification scope exists'}
[void][IO.Directory]::CreateDirectory($root)
$utf8=[Text.UTF8Encoding]::new($false)
$owner=@{pid=$PID;birth=[Diagnostics.Process]::GetCurrentProcess().StartTime.ToUniversalTime().ToFileTimeUtc().ToString()}
[IO.File]::WriteAllText(($root+'\owner.json'),($owner|ConvertTo-Json),$utf8)
$meter=$base+'\retained-image-meter-v2\meter.dll'
if((Get-FileHash -LiteralPath $meter -Algorithm SHA256).Hash.ToLowerInvariant() -ne '05f9bd152a160bfe018aba2348c2bf900e25020a88edabd892af891f0019ac4f'){throw 'pinned CNG helper differs'}
Add-Type -Path $meter
function Hash([string]$p){return (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant()}
$release=Get-Content -LiteralPath ($base+'\cached-release-v3\result.json') -Raw|ConvertFrom-Json
$body=Get-Content -LiteralPath ($base+'\cached-image-import-v3\result.json') -Raw|ConvertFrom-Json
$mountPath=$base+'\cached-image-import-v3\parent-paths.stdout.log'
if((Hash $mountPath) -ne '360bd58c4e944a592bc5a297ab32488386922476f30a171d451cb68085ac2e95'){throw 'actual native parent mount receipt changed'}
$mount=Get-Content -LiteralPath $mountPath -Raw
$decodedMount=$mount|ConvertFrom-Json
$options=@($decodedMount|ForEach-Object{$_.Options}|Where-Object{$_ -like 'parentLayerPaths=*'})
if($options.Count -ne 1){throw 'one exact native parent array option required'}
$parentJson=$options[0].Substring('parentLayerPaths='.Length)
$legacy=@($parentJson|ConvertFrom-Json)
$decoded=$parentJson|ConvertFrom-Json
$layers=@(foreach($path in $decoded){if($path -isnot [string]){throw 'native layer path is not a string'};$path})
$errorPath=$base+'\image-cached-controller-v3\prepare.stderr.log'
$v=[ordered]@{owner=$owner;current_phase='decoded-native-parent-receipt';kind='read-only actual imported parent/DLL verification after original controller failure';reference=$body.reference;original_release_sha256=(Hash ($base+'\cached-release-v3\result.json'));original_body_sha256=(Hash ($base+'\cached-image-import-v3\result.json'));original_error=[IO.File]::ReadAllText($errorPath);original_error_sha256=(Hash $errorPath);native_mount_sha256=(Hash $mountPath);native_mount_text=$mount;legacy_wrapped_count=$legacy.Count;decoded_parent_count=$layers.Count;layers=$layers;required_dlls=@();known_owned_processes=@();charged_pre=$release.pre;charged_post=$release.post;original_guard=$release.original_controller.guard;verified_for_execution=$false;new_imports=0;server_starts=0;native_gpu_dispatches=0;exit_code=1;utc=[DateTime]::UtcNow.ToString('o')}
[IO.File]::WriteAllText(($root+'\progress.json'),($v|ConvertTo-Json -Depth 25),$utf8)
try{
 if($layers.Count -ne 2){throw 'actual decoded two-layer parent array missing'}
 foreach($path in $layers){if(-not $path.StartsWith(($base+'\full-image-prep-v2\root\io.containerd.snapshotter.v1.windows\snapshots\'),[StringComparison]::OrdinalIgnoreCase) -or -not (Test-Path -LiteralPath $path)){throw 'actual imported parent escaped own image root or absent'}}
 if($layers -notcontains ($base+'\full-image-prep-v2\root\io.containerd.snapshotter.v1.windows\snapshots\1')){throw 'original committed base not reused'}
 if($body.reference -ne 'mcr.microsoft.com/windows/server@sha256:a62109569303cfe0ab49b1618383615f0477cdc285529b98d9b7085902d0e745'){throw 'wrong pinned image reference'}
 foreach($name in @('cached-import','cached-archive-producer','snapshots-after','parent-paths','remove-own-path-read')){$phase=@($body.phases|Where-Object{$_.name -eq $name});if($phase.Count -ne 1 -or $phase[0].exit -ne 0 -or -not $phase[0].reaped){throw ('native import/verification/cleanup incomplete: '+$name)}}
 if($release.original_controller.guard.timedOut -or -not $release.original_controller.guard.reaped -or $release.original_controller.guard.activeAfter -ne 0 -or -not $release.child.reaped -or $body.cleanup_errors.Count -ne 0 -or $release.cleanup_errors.Count -ne 0){throw 'original owned cleanup incomplete'}
 if($null -eq $release.post -or $release.post.allocatedBytes -gt 34359738368L){throw 'whole charged-root allocation overrun/unknown'}
 $after=Get-Content -LiteralPath ($base+'\cached-image-import-v3\snapshots-after.stdout.log') -Raw
 if(@($after -split '\r?\n'|Where-Object{$_ -match '^sha256:49639eb7e32ce1d8a18cc2504b183a024475038fcba89f069a50751e8eef5a8d\s' -and $_ -match '\bCommitted\b'}).Count -ne 1){throw 'exact native committed full-image head missing'}
 foreach($name in @('ntdll.dll','gdi32.dll','dxgi.dll','d3d12.dll','d3dcompiler_47.dll')){
  $files=@(foreach($layer in $layers){$path=$layer+'\Files\Windows\System32\'+$name;if(Test-Path -LiteralPath $path){$v.current_phase='CNG-file-hash '+$path;[IO.File]::WriteAllText(($root+'\progress.json'),($v|ConvertTo-Json -Depth 25),$utf8);$length=(Get-Item -LiteralPath $path).Length;@{path=$path;bytes=$length;sha256=[NativeRetainedImage]::Hash($path,$length,10000)}}})
  $v.required_dlls+=@{name=$name;present=($files.Count -gt 0);files=$files}
 }
 if(@($v.required_dlls|Where-Object{-not $_.present}).Count -ne 0){throw 'full imported image lacks actual explicit compute DLL prerequisites'}
 foreach($owned in @($release.child)+@($body.phases)){
  $p=Get-Process -Id ([int]$owned.pid) -ErrorAction SilentlyContinue
  if($null -eq $p){$v.known_owned_processes+=@{pid=$owned.pid;present=$false}}else{$birth=$p.StartTime.ToUniversalTime().ToFileTimeUtc().ToString();$v.known_owned_processes+=@{pid=$owned.pid;present=$true;birth_native=$birth;recorded_birth=$owned.birth_native;owned_birth_still_present=($birth -eq $owned.birth_native)};if($birth -eq $owned.birth_native){throw 'recorded owned process remains after cleanup'}}
 }
 $v.current_phase='verified-read-only';$v.verified_for_execution=$true;$v.exit_code=0
}catch{$v.verification_error=$_.ToString()}
$json=$v|ConvertTo-Json -Depth 25;[IO.File]::WriteAllText(($root+'\result.json'),$json,$utf8);[Console]::WriteLine($json)
exit $v.exit_code
