$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$base='C:\Users\corph\.cache\hex-native-windows-hcs';$root=$base+'\import-receipts-v1'
if(Test-Path -LiteralPath $root){throw 'owned receipt inspection exists'}
[void][IO.Directory]::CreateDirectory($root)
$owner=@{pid=$PID;birth=[Diagnostics.Process]::GetCurrentProcess().StartTime.ToUniversalTime().ToString('o')}
[IO.File]::WriteAllText(($root+'\owner.json'),($owner|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
$v=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');owner=$owner;original_error=$null;mount=$null;after=$null;legacy_count=$null;decoded_count=$null;layers=$null;dll_attributes=@();reads=@();native_gpu_dispatches=0;new_imports=0;server_starts=0}
foreach($item in @(@{key='original_error';path=($base+'\image-cached-controller-v3\prepare.stderr.log')},@{key='mount';path=($base+'\cached-image-import-v3\parent-paths.stdout.log')},@{key='after';path=($base+'\cached-image-import-v3\snapshots-after.stdout.log')})){$v.reads+=@{phase='before-read';path=$item.path};[IO.File]::WriteAllText(($root+'\progress.json'),($v|ConvertTo-Json -Depth 20),[Text.UTF8Encoding]::new($false));$v[$item.key]=[IO.File]::ReadAllText($item.path)}
$document=$v.mount|ConvertFrom-Json;$options=@($document|ForEach-Object{$_.Options}|Where-Object{$_ -like 'parentLayerPaths=*'})
if($options.Count -ne 1){throw 'one native parent-array option required'}
$text=$options[0].Substring('parentLayerPaths='.Length);$legacy=@($text|ConvertFrom-Json);$decoded=$text|ConvertFrom-Json
$v.legacy_count=$legacy.Count;$v.decoded_count=$decoded.Count;$v.layers=$decoded
$probe=$base+'\path-probe-v1\probe.dll'
if((Get-FileHash -LiteralPath $probe -Algorithm SHA256).Hash.ToLowerInvariant() -ne '1e55f67b9c9c0de2e7ffa0deeb0b4afdd8104264d1d8672143ca925f7fb57ad3'){throw 'read-only attribute probe differs'}
Add-Type -Path $probe
foreach($layer in $decoded){foreach($name in @('ntdll.dll','gdi32.dll','dxgi.dll','d3d12.dll','d3dcompiler_47.dll')){$path=$layer+'\Files\Windows\System32\'+$name;$v.dll_attributes+=@{path=$path;attributes=[NativeLongPathProbe]::Attributes(('\\?\'+$path))}}}
$json=$v|ConvertTo-Json -Depth 20;[IO.File]::WriteAllText(($root+'\result.json'),$json,[Text.UTF8Encoding]::new($false));[Console]::WriteLine($json)
