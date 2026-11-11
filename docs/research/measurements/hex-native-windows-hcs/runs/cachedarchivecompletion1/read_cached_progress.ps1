# One read of owned actor/phase facts; growth is measured only after the existing archive IO completion.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$base='C:\Users\corph\.cache\hex-native-windows-hcs'
$v=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');release=$null;body=$null;owned_processes=@();growth_after_archive_io=$null;uncertainty=@();new_imports=0;native_gpu_dispatches=0}
foreach($item in @(@{key='release';path=($base+'\cached-release-v3\result.json')},@{key='body';path=($base+'\cached-image-import-v3\result.json')})){
 try{if(Test-Path -LiteralPath $item.path){$v[$item.key]=Get-Content -LiteralPath $item.path -Raw|ConvertFrom-Json}else{$v.uncertainty+=@('owned phase ledger absent: '+$item.path)}}catch{$v.uncertainty+=@($_.ToString())}
}
if($null -ne $v.body){
 foreach($entry in $v.body.phases){$pidOfOwned=[int]$entry.pid;$p=Get-Process -Id $pidOfOwned -ErrorAction SilentlyContinue;if($null -eq $p){$v.owned_processes+=@{pid=$pidOfOwned;present=$false}}else{$birth=$p.StartTime.ToUniversalTime().ToFileTimeUtc().ToString();$v.owned_processes+=@{pid=$pidOfOwned;present=$true;birth_native=$birth;recorded_birth=$entry.birth_native;birth_matches=($birth -eq $entry.birth_native);name=$p.ProcessName;handles=$p.HandleCount;working_set=$p.WorkingSet64}}}
 if($null -ne $v.body.archive){
  $meter=$base+'\retained-image-meter-v2\meter.dll';if((Get-FileHash -LiteralPath $meter -Algorithm SHA256).Hash.ToLowerInvariant() -ne '05f9bd152a160bfe018aba2348c2bf900e25020a88edabd892af891f0019ac4f'){throw 'meter differs'}
  Add-Type -Path $meter
  try{$v.growth_after_archive_io=[NativeRetainedImage]::Measure($base,1048576,120000)}catch{$v.uncertainty+=@($_.ToString())}
 }
}
$v|ConvertTo-Json -Depth 25 -Compress
