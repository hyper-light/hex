# Query only recorded owned keeper/process identities and exact recorded HCS IDs; never stop by name.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$base='C:\Users\corph\.cache\hex-native-windows-hcs';$root=$base+'\owned-absence-v3'
if(Test-Path -LiteralPath $root){throw 'owned absence receipt exists'}
[void][IO.Directory]::CreateDirectory($root)
$helper=$base+'\native-cpu-callback-v3\native_driver.dll'
if((Get-FileHash -LiteralPath $helper -Algorithm SHA256).Hash.ToLowerInvariant() -ne 'faba83ca93d31489e026a35edc60bcbd34cb799ebd11dc9d3cfc88cfaaa272cd'){throw 'pinned query helper differs'}
Add-Type -Path $helper
$v=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');processes=@();systems=@();guard_receipts=@();stop_calls=0;native_workloads=0;complete_driver_com_leak_attribution='UNVERIFIED'}
$owners=@(@{path='hcs-cpu-exec-v2\scratch\hex-device-BvutM3\state\keeper\keeper.owner'},@{path='hcs-gpu-exec-v1\scratch\hex-device-WOCqWb\state\keeper\keeper.owner'},@{path='hcs-gpu-exec-v2\scratch\hex-device-2pkdrR\state\keeper\keeper.owner'})
foreach($owner in $owners){$text=[IO.File]::ReadAllText(($base+'\'+$owner.path)).Trim();$parts=$text.Split([char[]]@(' ',"`r","`n","`t"),[StringSplitOptions]::RemoveEmptyEntries);if($parts.Count -ne 2){throw 'recorded keeper owner malformed'};$owner.pid=[uint32]::Parse($parts[0]);$owner.birth=[uint64]::Parse($parts[1]).ToString()}
$owners+=@(@{pid=15796;birth='134388510203219089';path='native-full-image-gpu-v1\journal.json'},@{pid=16416;birth='134388480816347837';path='cached-image-import-v3\result.json'},@{pid=6396;birth='134388480398329103';path='cached-release-v3\result.json'})
foreach($owner in $owners){$p=Get-Process -Id ([int]$owner.pid) -ErrorAction SilentlyContinue;$row=@{pid=$owner.pid;recorded_birth=$owner.birth;record=$owner.path;present=($null -ne $p);exact_recorded_process_absent=($null -eq $p)};if($null -ne $p){$row.current_birth=$p.StartTime.ToUniversalTime().ToFileTimeUtc().ToString();$row.exact_recorded_process_absent=($row.current_birth -ne $owner.birth)};$v.processes+=,$row}
foreach($id in @('wd-hexdeviceBvutM3-cpu-recovery','native-hcs-full-image-cpu-311010846a9341f286171e1d49010879','native-hcs-full-image-gpu-95ccc39064f44e1d87bbf1b5d6fd28b4')){$found=[IntPtr]::Zero;$hr=[NativeHcsBaseline]::HcsOpenComputeSystem($id,268435456,[ref]$found);$v.systems+=@{id=$id;hr=('0x{0:x8}' -f [uint32]([int64]$hr -band 4294967295));absent=([NativeHcsBaseline]::Missing($hr) -and $found -eq [IntPtr]::Zero)};if($found -ne [IntPtr]::Zero){[NativeHcsBaseline]::HcsCloseComputeSystem($found)}}
foreach($scope in @('hcs-cpu-exec-v2','hcs-gpu-exec-v1','hcs-gpu-exec-v2','hcsfixed1','winpreflight1','c579-node-cli-check-v1')){$p=$base+'\'+$scope+'\result.json';$receipt=ConvertFrom-Json ([IO.File]::ReadAllText($p));foreach($phase in $receipt.phases){$v.guard_receipts+=@{scope=$scope;phase=$phase.name;guard=$phase.guard}}}
$v.owned_keepers_processes_absent=(@($v.processes|Where-Object{-not $_.exact_recorded_process_absent}).Count -eq 0)
$v.recorded_hcs_systems_absent=(@($v.systems|Where-Object{-not $_.absent}).Count -eq 0)
$v.completed_guard_jobs_empty_and_reaped=(@($v.guard_receipts|Where-Object{-not $_.guard.reaped -or $_.guard.activeAfter -ne 0}).Count -eq 0)
$json=$v|ConvertTo-Json -Depth 14;[IO.File]::WriteAllText(($root+'\result.json'),$json,[Text.UTF8Encoding]::new($false));[Console]::WriteLine($json)
if(-not $v.owned_keepers_processes_absent -or -not $v.recorded_hcs_systems_absent -or -not $v.completed_guard_jobs_empty_and_reaped){exit 1}
