# Preserve original stopped checkpoints and current committed metadata without a server/import.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$base='C:\Users\corph\.cache\hex-native-windows-hcs';$root=$base+'\committed-metadata-v1'
if(Test-Path -LiteralPath $root){throw 'owned metadata receipt already exists'}
[void][IO.Directory]::CreateDirectory($root)
$rows=@(
 @{name='before-snapshots.db';path=($base+'\cached-image-import-v3\original-metadata\snapshots.db');pin='391ce09fe56632a48f9af17c0a1b51311f1971396773c2363a328aba18ae5d8b'},
 @{name='before-containerd.db';path=($base+'\cached-image-import-v3\original-metadata\containerd.db');pin='1874a57aa5915be302f83318873c9c4c57d3bea9a76c3e0ef466963339fc6eef'},
 @{name='after-snapshots.db';path=($base+'\full-image-prep-v2\root\io.containerd.snapshotter.v1.windows\metadata.db')},
 @{name='after-containerd.db';path=($base+'\full-image-prep-v2\root\io.containerd.metadata.v1.bolt\meta.db')})
$v=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');server_pid=16416;server_recorded_birth='134388480816347837';files=@();new_imports=0;server_starts=0;native_gpu_dispatches=0}
$p=Get-Process -Id 16416 -ErrorAction SilentlyContinue
if($null -ne $p -and $p.StartTime.ToUniversalTime().ToFileTimeUtc().ToString() -eq $v.server_recorded_birth){throw 'recorded server remains live; stopped metadata copy refused'}
$total=0L
foreach($row in $rows){$length=(Get-Item -LiteralPath $row.path).Length;if($length -gt 8388608 -or $total+$length -gt 16777216){throw 'existing metadata collection byte budget exceeded'};$bytes=[IO.File]::ReadAllBytes($row.path);$hash=(Get-FileHash -LiteralPath $row.path -Algorithm SHA256).Hash.ToLowerInvariant();if($row.ContainsKey('pin') -and $row.pin -ne $hash){throw 'original stopped metadata checkpoint differs'};$copy=$root+'\'+$row.name;[IO.File]::WriteAllBytes($copy,$bytes);if((Get-FileHash -LiteralPath $copy -Algorithm SHA256).Hash.ToLowerInvariant() -ne $hash){throw 'owned raw metadata copy differs'};$v.files+=@{name=$row.name;source=$row.path;bytes=$bytes.Length;sha256=$hash;base64=[Convert]::ToBase64String($bytes)};$total+=$bytes.Length}
$json=$v|ConvertTo-Json -Depth 8;[IO.File]::WriteAllText(($root+'\result.json'),$json,[Text.UTF8Encoding]::new($false));[Console]::WriteLine($json)
