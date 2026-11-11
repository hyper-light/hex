# Exact retained HCS scope and own stopped-containerd metadata; no new actor or mutation.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$base='C:\Users\corph\.cache\hex-native-windows-hcs'
$helper=$base+'\native-cpu-callback-v3\native_driver.dll'
if((Get-FileHash -LiteralPath $helper -Algorithm SHA256).Hash.ToLowerInvariant() -ne 'faba83ca93d31489e026a35edc60bcbd34cb799ebd11dc9d3cfc88cfaaa272cd'){throw 'exact retained HCS observer differs'}
Add-Type -Path $helper
$lab=$base+'\hcs-cpu-exec-v1\scratch\hex-device-uopdHC'
$id='wd-hexdeviceuopdHC-cpu-recovery'
$v=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');id=$id;lab=$lab;processes=@();state_files=@();physical=@{};image_metadata=@();raw_files=[ordered]@{};collection_refusals=@();pre_cleanup_store_marker_bytes_retained=$false;pre_cleanup_bytes_gap='fixture deleted private marker/store during normal failure cleanup before external collection; no prior bytes reconstructed';native_workloads=0;mutations=0}
foreach($pidOfOwned in @(7828,14668,13244,5060,9244,6212)){
 $p=Get-Process -Id $pidOfOwned -ErrorAction SilentlyContinue
 if($null -eq $p){$v.processes+=@{pid=$pidOfOwned;present=$false}}else{$v.processes+=@{pid=$pidOfOwned;present=$true;birth_native=$p.StartTime.ToUniversalTime().ToFileTimeUtc().ToString();name=$p.ProcessName;working_set=$p.WorkingSet64;handles=$p.HandleCount}}
}
$found=[IntPtr]::Zero;$hr=[NativeHcsBaseline]::HcsOpenComputeSystem($id,268435456,[ref]$found)
$v.physical=@{absent=([NativeHcsBaseline]::Missing($hr) -and $found -eq [IntPtr]::Zero);open_hr=('0x{0:x8}' -f [uint32]([int64]$hr -band 4294967295));marker_exists=(Test-Path -LiteralPath ($lab+'\state\keeper\boxes\'+$id+'.scratch.json'));scratch_exists=(Test-Path -LiteralPath ($lab+'\scratch-cpu-recovery'))}
if($found -ne [IntPtr]::Zero){[NativeHcsBaseline]::HcsCloseComputeSystem($found)}
foreach($f in (Get-ChildItem -LiteralPath ($lab+'\state') -File -Recurse)){$v.state_files+=@{path=$f.FullName;bytes=$f.Length;sha256=(Get-FileHash -LiteralPath $f.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}}
$image=$base+'\full-image-prep-v2\root'
$snapshotter=$image+'\io.containerd.snapshotter.v1.windows'
$files=@()
foreach($dir in @($snapshotter,($snapshotter+'\snapshots'),($image+'\io.containerd.metadata.v1.bolt'),($image+'\io.containerd.content.v1.content\ingest'))){
 if(Test-Path -LiteralPath $dir){foreach($entry in (Get-ChildItem -LiteralPath $dir -Force)){$v.image_metadata+=@{path=$entry.FullName;directory=$entry.PSIsContainer;bytes=if($entry.PSIsContainer){$null}else{$entry.Length};last_write_utc=$entry.LastWriteTimeUtc.ToString('o')};if(-not $entry.PSIsContainer){$files+=,$entry}}}
}
if(Test-Path -LiteralPath ($snapshotter+'\snapshots')){foreach($snapshot in (Get-ChildItem -LiteralPath ($snapshotter+'\snapshots') -Directory)){
 foreach($entry in (Get-ChildItem -LiteralPath $snapshot.FullName -Force)){$v.image_metadata+=@{path=$entry.FullName;directory=$entry.PSIsContainer;bytes=if($entry.PSIsContainer){$null}else{$entry.Length};last_write_utc=$entry.LastWriteTimeUtc.ToString('o')};if(-not $entry.PSIsContainer -and $entry.Name -ne 'blank.vhdx'){$files+=,$entry}}
}}
$total=0L
foreach($file in $files){
 if($file.Length -gt 8388608 -or $total+$file.Length -gt 16777216 -or $file.Extension -notin @('.db','.json','.txt')){$v.collection_refusals+=@{path=$file.FullName;bytes=$file.Length;reason='raw budget or non-metadata extension; exact file retained remotely, not silently skipped'};continue}
 $key=$file.FullName.Substring($image.Length+1).Replace('\','/')
 $bytes=[IO.File]::ReadAllBytes($file.FullName)
 $v.raw_files[$key]=@{bytes=$bytes.Length;sha256=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant();base64=[Convert]::ToBase64String($bytes)};$total+=$bytes.Length
}
$v|ConvertTo-Json -Depth 20 -Compress
