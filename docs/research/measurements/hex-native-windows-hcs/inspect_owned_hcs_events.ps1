# Existing events only, bounded to recorded owned systems and their actual receipt timestamps.
# Never enable a channel or alter logging/security/service configuration.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$base='C:\Users\corph\.cache\hex-native-windows-hcs';$root=$base+'\owned-hcs-events-v1'
if(Test-Path -LiteralPath $root){throw 'owned event receipt exists'}
[void][IO.Directory]::CreateDirectory($root)
$utf8=[Text.UTF8Encoding]::new($false)
$v=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');owner=@{pid=$PID;birth=[Diagnostics.Process]::GetCurrentProcess().StartTime.ToUniversalTime().ToFileTimeUtc().ToString()};host=@{};scopes=@();channel_changes=0;native_workloads=0}
$version=Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
$v.host=@{build=$version.CurrentBuildNumber;ubr=$version.UBR;build_lab=$version.BuildLabEx;display_version=$version.DisplayVersion}
$scopes=@(@{id='wd-hexdeviceBvutM3-cpu-recovery';scope='hcs-cpu-exec-v2';receipt='result.json'},@{id='native-hcs-full-image-cpu-311010846a9341f286171e1d49010879';scope='native-full-image-cpu-v1';receipt='journal.json'},@{id='native-hcs-full-image-gpu-95ccc39064f44e1d87bbf1b5d6fd28b4';scope='native-full-image-gpu-v1';receipt='journal.json'})
$total=0L
foreach($scope in $scopes){
 $file=Get-Item -LiteralPath ($base+'\'+$scope.scope+'\'+$scope.receipt)
 $from=(Get-Item -LiteralPath ($base+'\'+$scope.scope)).CreationTimeUtc
 $row=[ordered]@{id=$scope.id;receipt=$file.FullName;start_utc=$from.ToString('o');end_utc=$file.LastWriteTimeUtc.ToString('o');queries=@()}
 foreach($log in @('Microsoft-Windows-Hyper-V-Compute-Admin','Microsoft-Windows-Hyper-V-Compute-Operational','Microsoft-Windows-Containers-Wcifs/Operational')){
  $q=[ordered]@{log=$log;event_budget=256;read_count=0;matching=@();error=$null}
  try{$events=@(Get-WinEvent -FilterHashtable @{LogName=$log;StartTime=$from;EndTime=$file.LastWriteTimeUtc} -MaxEvents 256 -ErrorAction Stop);$q.read_count=$events.Count;foreach($event in $events){$xml=$event.ToXml();if($xml.IndexOf($scope.id,[StringComparison]::OrdinalIgnoreCase) -lt 0){continue};$total+=[Text.Encoding]::UTF8.GetByteCount($xml);if($total -gt 4194304){throw 'existing HCS result-sized event collection budget exceeded'};$q.matching+=@{record=$event.RecordId;id=$event.Id;provider=$event.ProviderName;utc=$event.TimeCreated.ToUniversalTime().ToString('o');xml=$xml}}}catch{$q.error=$_.ToString()}
  $row.queries+=,$q
 }
 $v.scopes+=,$row
 [IO.File]::WriteAllText(($root+'\progress.json'),($v|ConvertTo-Json -Depth 14),$utf8)
}
$json=$v|ConvertTo-Json -Depth 14;[IO.File]::WriteAllText(($root+'\result.json'),$json,$utf8);[Console]::WriteLine($json)
