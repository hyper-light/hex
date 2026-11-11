$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$r='C:\Users\corph\.cache\hex-native-windows-hcs\win5de'
$incoming='C:\Users\corph\.cache\hex-native-windows-hcs\win5de-incoming'
$v=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');root=$r;root_present=(Test-Path -LiteralPath $r);incoming_present=(Test-Path -LiteralPath $incoming);source_revision='5de0ea4cb4a30f3cdcfce8ff1c62499d1379028c';files=[ordered]@{};parsed=[ordered]@{};logs=[ordered]@{};process_observations=@();errors=@();new_workloads=0;stop_calls=0}
function Digest([byte[]]$bytes){$h=[Security.Cryptography.SHA256]::Create();try{return ([BitConverter]::ToString($h.ComputeHash($bytes))).Replace('-','').ToLowerInvariant()}finally{$h.Dispose()}}
foreach($name in @('run.json','result.json')){
 $path=Join-Path $r $name
 if(-not (Test-Path -LiteralPath $path)){$v.files[$name]=@{present=$false};continue}
 try{
  $f=Get-Item -LiteralPath $path
  if($f.Length -gt 1048576){throw 'exact receipt exceeds one-MiB read budget'}
  $bytes=[IO.File]::ReadAllBytes($path)
  if($bytes.Length -gt 1048576){throw 'exact receipt grew past one-MiB read budget'}
  $v.files[$name]=@{present=$true;bytes=$bytes.Length;sha256=(Digest $bytes);raw_base64=[Convert]::ToBase64String($bytes);last_write_utc=$f.LastWriteTimeUtc.ToString('o')}
  $row=[Text.Encoding]::UTF8.GetString($bytes)|ConvertFrom-Json
  if($row.source_revision -cne $v.source_revision -or $row.source_archive_sha256 -cne '72f52a6ad052d60420ba8ff61de0fef481734528cf7ad29b672d8c180d771fc6'){throw 'receipt source pin differs; process observations refused'}
  $v.parsed[$name]=$row
 }catch{$v.errors+=@{path=$path;error=$_.Exception.Message}}
}
foreach($name in @('build.stdout.log','build.stderr.log','preflight.stdout.log','preflight.stderr.log')){
 $path=Join-Path $r $name
 if(-not (Test-Path -LiteralPath $path)){$v.logs[$name]=@{present=$false};continue}
 $stream=$null
 try{
  $stream=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite)
  $length=$stream.Length;$start=[Math]::Max([long]0,$length-65536);[void]$stream.Seek($start,[IO.SeekOrigin]::Begin)
  $buf=New-Object byte[] ([int]($length-$start));$n=$stream.Read($buf,0,$buf.Length)
  $tail=New-Object byte[] $n;[Array]::Copy($buf,$tail,$n)
  $v.logs[$name]=@{present=$true;observed_file_bytes=$length;tail_start=$start;tail_bytes=$n;tail_sha256=(Digest $tail);tail_raw_base64=[Convert]::ToBase64String($tail)}
 }catch{$v.errors+=@{path=$path;error=$_.Exception.Message}}finally{if($null -ne $stream){$stream.Dispose()}}
}
$ids=[Collections.Generic.HashSet[int]]::new()
foreach($row in $v.parsed.Values){
 if($row.driver_pid){[void]$ids.Add([int]$row.driver_pid)}
 foreach($phase in $row.phases){if($phase.guard.pid){[void]$ids.Add([int]$phase.guard.pid)}}
}
if($ids.Count -gt 16){throw 'recorded PID observation budget exceeded'}
foreach($number in $ids){
 $obs=[ordered]@{pid=$number;authority='PID recorded only by exact source-pinned owned receipt; original birth not persisted by this driver';original_birth_match='UNVERIFIED';queried_job_membership='not available through retained receipt; no new helper or job adoption';present=$null}
 $proc=$null
 try{
  try{$proc=[Diagnostics.Process]::GetProcessById($number)}catch [ArgumentException]{$obs.present=$false}
  if($null -ne $proc){
   $obs.present=$true;$obs.birth_utc=$proc.StartTime.ToUniversalTime().ToString('o');$obs.has_exited=$proc.HasExited
   if($obs.has_exited){$obs.exit_code=$proc.ExitCode}else{$obs.working_set_bytes=$proc.WorkingSet64;$obs.private_memory_bytes=$proc.PrivateMemorySize64;$obs.handle_count=$proc.HandleCount;$obs.cpu_ticks=$proc.TotalProcessorTime.Ticks}
  }
 }catch{$obs.observation_error=$_.Exception.Message}finally{if($null -ne $proc){$proc.Dispose()}}
 $v.process_observations+=,$obs
}
$v.proof_limit='Only exact retained file bytes and recorded PID observations. Birth matching, current whole-job absence, child absence and resource return are not inferred from missing files/PIDs, an elapsed guard or a disconnected client. No process scan, arguments read, kill, source mutation or backend call.'
$v.finished_utc=[DateTime]::UtcNow.ToString('o')
$v|ConvertTo-Json -Depth 24 -Compress

