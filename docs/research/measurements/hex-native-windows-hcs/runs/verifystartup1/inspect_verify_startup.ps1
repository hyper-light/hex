# Observe the exact early verifier steps; no image mutation or workload launch.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$base='C:\Users\corph\.cache\hex-native-windows-hcs';$root=$base+'\verify-startup-diagnostic-v1'
if(Test-Path -LiteralPath $root){throw 'owned startup diagnostic exists'}
[void][IO.Directory]::CreateDirectory($root)
$utf8=[Text.UTF8Encoding]::new($false)
$v=[ordered]@{owner=@{pid=$PID;birth=[Diagnostics.Process]::GetCurrentProcess().StartTime.ToUniversalTime().ToFileTimeUtc().ToString()};steps=@();native_gpu_dispatches=0;imports=0;server_starts=0}
function Step([string]$name){$v.steps+=@{phase=$name;utc=[DateTime]::UtcNow.ToString('o')};[IO.File]::WriteAllText(($root+'\progress.json'),($v|ConvertTo-Json -Depth 12),$utf8);[Console]::WriteLine($name);[Console]::Out.Flush()}
Step 'before-pinned-meter-hash'
$meter=$base+'\retained-image-meter-v2\meter.dll'
$v.meter_hash=(Get-FileHash -LiteralPath $meter -Algorithm SHA256).Hash.ToLowerInvariant()
if($v.meter_hash -ne '05f9bd152a160bfe018aba2348c2bf900e25020a88edabd892af891f0019ac4f'){throw 'meter pin changed'}
Step 'before-Add-Type-pinned-meter'
Add-Type -Path $meter
Step 'before-release-result-read'
$release=Get-Content -LiteralPath ($base+'\cached-release-v3\result.json') -Raw|ConvertFrom-Json
Step 'before-body-result-read'
$body=Get-Content -LiteralPath ($base+'\cached-image-import-v3\result.json') -Raw|ConvertFrom-Json
Step 'before-native-parent-hash'
$mountPath=$base+'\cached-image-import-v3\parent-paths.stdout.log'
$v.mount_hash=(Get-FileHash -LiteralPath $mountPath -Algorithm SHA256).Hash.ToLowerInvariant()
if($v.mount_hash -ne '360bd58c4e944a592bc5a297ab32488386922476f30a171d451cb68085ac2e95'){throw 'mount pin changed'}
Step 'before-parent-decode'
$mount=Get-Content -LiteralPath $mountPath -Raw
$decodedMount=$mount|ConvertFrom-Json
$options=@($decodedMount|ForEach-Object{$_.Options}|Where-Object{$_ -like 'parentLayerPaths=*'})
if($options.Count -ne 1){throw 'one actual native parent option required'}
$parentJson=$options[0].Substring('parentLayerPaths='.Length)
$legacy=@($parentJson|ConvertFrom-Json);$decoded=$parentJson|ConvertFrom-Json
$v.layers=@(foreach($path in $decoded){if($path -isnot [string]){throw 'parent is not a string'};$path})
Step 'before-original-error-read'
$v.original_error=[IO.File]::ReadAllText(($base+'\image-cached-controller-v3\prepare.stderr.log'))
Step 'before-progress-serialization'
$v.release_post=$release.post;$v.original_guard=$release.original_controller.guard;$v.reference=$body.reference
Step 'startup-complete'
$json=$v|ConvertTo-Json -Depth 12;[IO.File]::WriteAllText(($root+'\result.json'),$json,$utf8);[Console]::WriteLine($json)
