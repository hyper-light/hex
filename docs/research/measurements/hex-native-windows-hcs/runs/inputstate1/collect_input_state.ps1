# Read-only collection after exact owned server/job reaping; no ctr operation or new actor.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$base='C:\Users\corph\.cache\hex-native-windows-hcs'
$identity=$base+'\full-image-prep-v2\identity.dll'
if((Get-FileHash -LiteralPath $identity -Algorithm SHA256).Hash.ToLowerInvariant() -ne '846ab5c89f048c7d0dd10c690515ce50814b947c0b5c3f3c11f46b4a539e27c6'){throw 'retained identity helper differs'}
Add-Type -Path $identity
$hcs=$base+'\native-cpu-callback-v3\native_driver.dll'
if((Get-FileHash -LiteralPath $hcs -Algorithm SHA256).Hash.ToLowerInvariant() -ne 'faba83ca93d31489e026a35edc60bcbd34cb799ebd11dc9d3cfc88cfaaa272cd'){throw 'retained native API helper differs'}
Add-Type -Path $hcs
$report=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');kind='read-only retained input/log/allocation and exact owned ID absence';roots=[ordered]@{};files=[ordered]@{};total_bytes=0;native_gpu_dispatches=0;new_container_workloads=0;server_install_restart=0}
foreach($leaf in @('full-image-prep-v2','image-prep-controller-v2','native-cpu-callback-v1','native-cpu-callback-v2','native-cpu-callback-v3')){
 $root=$base+'\'+$leaf
 $meta=[ordered]@{allocation=$null;allocation_error=$null;index=[ordered]@{}}
 try{$meta.allocation=[NativePrepIdentity]::Measure($root,1048576)}catch{$meta.allocation_error=$_.ToString()}
 if($null -ne $meta.allocation -and ($meta.allocation.logicalBytes -gt 34359738368L -or $meta.allocation.allocatedFileBytesUpperBound -gt 34359738368L)){$meta.measured_over_32g=$true}
 $files=@(Get-ChildItem -LiteralPath $root -File|Where-Object{$_.Extension -in @('.json','.log','.ps1','.cs')})
 if(Test-Path -LiteralPath ($root+'\output\cpu.txt')){$files+=Get-Item -LiteralPath ($root+'\output\cpu.txt')}
 foreach($file in $files){
  if($file.Length -gt 8388608 -or $report.total_bytes+$file.Length -gt 33554432){throw 'explicit raw collection budget exceeded; preserve original'}
  $relative=$leaf+'/'+$file.FullName.Substring($root.Length+1).Replace('\','/')
  $bytes=[IO.File]::ReadAllBytes($file.FullName)
  $meta.index[$relative]=@{bytes=$bytes.Length;sha256=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}
  $report.files[$relative]=[Convert]::ToBase64String($bytes);$report.total_bytes+=$bytes.Length
 }
 if($leaf -like 'native-cpu-*'){
  $journal=Get-Content -LiteralPath ($root+'\journal.json') -Raw|ConvertFrom-Json
  $found=[IntPtr]::Zero;$hr=[NativeHcsBaseline]::HcsOpenComputeSystem($journal.id,268435456,[ref]$found)
  $meta.exact_system_id=$journal.id;$meta.exact_system_absent=([NativeHcsBaseline]::Missing($hr) -and $found -eq [IntPtr]::Zero)
  $meta.open_hr=('0x{0:x8}' -f [uint32]([int64]$hr -band 4294967295))
  if($found -ne [IntPtr]::Zero){[NativeHcsBaseline]::HcsCloseComputeSystem($found)}
  $driver=[NativeHcsBaseline+Driver]::new();$driver.flavour=1;$driver.home=[Runtime.InteropServices.Marshal]::StringToHGlobalUni('')
  try{$extent=[UIntPtr]::Zero;$mount=[NativeHcsBaseline]::GetLayerMountPath([ref]$driver,($root+'\scratch'),[ref]$extent,[IntPtr]::Zero);$meta.scratch_mount_hr=('0x{0:x8}' -f [uint32]([int64]$mount -band 4294967295))}finally{[Runtime.InteropServices.Marshal]::FreeHGlobal($driver.home)}
 }
 $report.roots[$leaf]=$meta
}
$report|ConvertTo-Json -Depth 20 -Compress
