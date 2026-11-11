# PROPOSAL: cached-only continuation. ROOT reviews this exact body before any dispatch.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$root='C:\Users\corph\.cache\hex-native-windows-hcs\cached-image-import-v3'
$image='C:\Users\corph\.cache\hex-native-windows-hcs\full-image-prep-v2'
if(Test-Path -LiteralPath $root){throw 'fresh owned controller root already exists'}
[void][IO.Directory]::CreateDirectory($root)
foreach($name in @('state','temp','metadata','original-metadata')){[void][IO.Directory]::CreateDirectory((Join-Path $root $name))}
$env:TEMP=$root+'\temp';$env:TMP=$env:TEMP
$utf8=[Text.UTF8Encoding]::new($false)
$serverExe='C:\Users\corph\.cache\hex-native-s4\containerd\bin\containerd.exe'
$ctr='C:\Users\corph\.cache\hex-native-s4\containerd\bin\ctr.exe'
if((Get-FileHash -LiteralPath $serverExe -Algorithm SHA256).Hash -ne '0E982834DBB5DEA61DF1449EC4A8ED365EA9F9A51714D904F259A58FB6BF7CAD' -or (Get-FileHash -LiteralPath $ctr -Algorithm SHA256).Hash -ne '457D4A4CF07DF0ADA953A5640CA9C2EA7B18F2F16E07BFA2469983072CCEAB70'){throw 'existing binary pin changed'}
$pipeName='hex-native-hcs-cached-import-v3';$address='\\.\pipe\'+$pipeName
$namespace='hex-native-hcs-full-image-v2'
$reference='mcr.microsoft.com/windows/server@sha256:a62109569303cfe0ab49b1618383615f0477cdc285529b98d9b7085902d0e745'
$receipt=[ordered]@{kind='cached-only local import continuation; original first-layer committed state retained';root=$root;owned_image_root=$image;namespace=$namespace;address=$address;reference=$reference;native_gpu_dispatches=0;container_workloads=0;registry_fetch_commands=0;service_or_driver_changes=0;phases=@();pipe_owner_checks=@();cleanup_errors=@();disk_admission_bytes=34359738368L;source_production_reference='2c8855b220a36c57685e456a7b8c98d8368f7899';policy_current_revision='0a6d91b3de0f10a8397868aa3794adab1e09afdc';rules_sha256='df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3';policy_byte_equality_sha256='6a658eef10e8802ed3d73ca09e5f5acf63b5f5c767c27196d429d73fba739388'}
function Save(){[IO.File]::WriteAllText(($root+'\result.json'),($receipt|ConvertTo-Json -Depth 20),$utf8)}
function Hash([string]$path){return (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()}
# Checkpoint both stopped databases before the first server action; preserve inactive snapshot2.
foreach($db in @(@{relative='io.containerd.snapshotter.v1.windows\metadata.db';hash='391ce09fe56632a48f9af17c0a1b51311f1971396773c2363a328aba18ae5d8b';name='snapshots.db'},@{relative='io.containerd.metadata.v1.bolt\meta.db';hash='1874a57aa5915be302f83318873c9c4c57d3bea9a76c3e0ef466963339fc6eef';name='containerd.db'})){
 $source=$image+'\root\'+$db.relative;if((Hash $source) -ne $db.hash){throw 'original stopped metadata changed; refuse automatic continuation'}
 [IO.File]::Copy($source,($root+'\original-metadata\'+$db.name),$false)
 if((Hash ($root+'\original-metadata\'+$db.name)) -ne $db.hash){throw 'original metadata checkpoint mismatch'}
}
$identityPath=$image+'\identity.dll'
if((Hash $identityPath) -ne '846ab5c89f048c7d0dd10c690515ce50814b947c0b5c3f3c11f46b4a539e27c6'){throw 'pinned checked pipe identity helper differs'}
Add-Type -Path $identityPath
$meter='C:\Users\corph\.cache\hex-native-windows-hcs\retained-image-meter-v2\meter.dll'
if((Hash $meter) -ne '05f9bd152a160bfe018aba2348c2bf900e25020a88edabd892af891f0019ac4f'){throw 'pinned extended-path allocation helper differs'}
Add-Type -Path $meter
function Measure-Owned(){
 $prior=[NativeRetainedImage]::Measure($image,1048576,120000);$control=[NativeRetainedImage]::Measure($root,1048576,120000)
 $receipt.image_allocation=$prior;$receipt.controller_allocation=$control;$receipt.total_scoped_allocated_bytes=$prior.allocatedBytes+$control.allocatedBytes;Save
 if($receipt.total_scoped_allocated_bytes -gt $receipt.disk_admission_bytes){throw 'measured32GiB overrun; retain original/partial state and refuse completion'}
}
$blobs=@(@{digest='a62109569303cfe0ab49b1618383615f0477cdc285529b98d9b7085902d0e745';bytes=597L},@{digest='620d54468172e4e8185670f4830dfd5bbf4f20dc2b1852b3327a3c51b8d727c6';bytes=789L},@{digest='e5dcf3bc996337655b83ea98c2e05a33a75e36a0357b01d7d433dbabcd1509a7';bytes=4481593239L},@{digest='58b297594fae8487acb87861188616be04290bce96e6531d21161e836082b2f2';bytes=2175606732L})
$content=$image+'\root\io.containerd.content.v1.content'
foreach($blob in $blobs){$p=$content+'\blobs\sha256\'+$blob.digest;if([NativeRetainedImage]::Hash($p,$blob.bytes,60000) -ne $blob.digest){throw 'cached blob differs; no registry fallback'}}
[IO.File]::WriteAllText(($root+'\metadata\oci-layout'),'{"imageLayoutVersion":"1.0.0"}',$utf8)
$index=@{schemaVersion=2;mediaType='application/vnd.oci.image.index.v1+json';manifests=@(@{mediaType='application/vnd.docker.distribution.manifest.v2+json';digest='sha256:a62109569303cfe0ab49b1618383615f0477cdc285529b98d9b7085902d0e745';size=597;platform=@{architecture='amd64';os='windows';'os.version'='10.0.26100.33822'}})}|ConvertTo-Json -Depth 10 -Compress
[IO.File]::WriteAllText(($root+'\metadata\index.json'),$index,$utf8)
$receipt.oci_layout_sha256=Hash ($root+'\metadata\oci-layout');$receipt.index_sha256=Hash ($root+'\metadata\index.json');$receipt.blobs=$blobs
# Single bounded pump hashes the actual archive stream forwarded to ctr; no archive-sized disk copy.
Add-Type -TypeDefinition @'
using System;using System.IO;using System.Diagnostics;using System.Security.Cryptography;using System.Threading.Tasks;
public static class NativeCachedArchivePump {
 public class Result {public long bytes;public string sha256;}
 public static Task<Result> Start(Stream source,Stream target,long maximum,int milliseconds){return Task.Run(()=>{
  Stopwatch time=Stopwatch.StartNew();long total=0;byte[] bytes=new byte[1048576];
  using(SHA256Cng hash=new SHA256Cng()){int count;while((count=source.Read(bytes,0,bytes.Length))!=0){if(time.ElapsedMilliseconds>=milliseconds)throw new IOException("original archive import deadline");total=checked(total+count);if(total>maximum)throw new IOException("declared OCI archive extent exceeded");hash.TransformBlock(bytes,0,count,bytes,0);target.Write(bytes,0,count);}target.Flush();hash.TransformFinalBlock(bytes,0,0);return new Result{bytes=total,sha256=BitConverter.ToString(hash.Hash).Replace("-","").ToLowerInvariant()};}
 });}
}
'@ -OutputAssembly ($root+'\pump.dll')
Add-Type -Path ($root+'\pump.dll')
$receipt.identity_binary_sha256=Hash $identityPath;$receipt.pump_binary_sha256=Hash ($root+'\pump.dll')
function Start-Owned([string]$exe,[string[]]$arguments,[string]$name,[bool]$withInput,[bool]$streamOutput){
 $start=[Diagnostics.ProcessStartInfo]::new();$start.FileName=$exe;$start.Arguments=($arguments -join ' ');$start.WorkingDirectory=$root;$start.UseShellExecute=$false;$start.CreateNoWindow=$true;$start.RedirectStandardInput=$withInput;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
 $p=[Diagnostics.Process]::new();$p.StartInfo=$start
 $out=if($streamOutput){$null}else{[IO.File]::Open(($root+'\'+$name+'.stdout.log'),[IO.FileMode]::CreateNew)}
 $err=[IO.File]::Open(($root+'\'+$name+'.stderr.log'),[IO.FileMode]::CreateNew)
 if(-not $p.Start()){throw 'owned process start failed'};$held=$p.Handle;if($held -eq [IntPtr]::Zero){throw 'held native process handle unavailable'}
 $record=[ordered]@{name=$name;pid=$p.Id;birth_native=([NativePrepIdentity]::Birth($held)).ToString();executable=$exe;arguments=$arguments}
 $receipt.phases+=,$record;Save
 return [pscustomobject]@{process=$p;handle=$held;record=$record;stdout=$out;stderr=$err;outCopy=if($streamOutput){$null}else{$p.StandardOutput.BaseStream.CopyToAsync($out)};errCopy=$p.StandardError.BaseStream.CopyToAsync($err)}
}
function End-Owned($owned,[bool]$terminate){
 if($null -eq $owned){return}
 if(([NativePrepIdentity]::Birth($owned.handle)).ToString() -ne $owned.record.birth_native){throw 'held owned process birth mismatch'}
 if($terminate -and [NativePrepIdentity]::Exit($owned.handle) -eq 259){$owned.process.Kill()}
 $owned.record.reaped=$owned.process.WaitForExit(5000);$owned.record.exit=[NativePrepIdentity]::Exit($owned.handle);Save
 if(-not $owned.record.reaped){throw 'exact owned child not reaped'}
 if($null -ne $owned.outCopy){if(-not $owned.outCopy.Wait(5000)){throw 'owned stdout drain uncertain'};$owned.stdout.Dispose()}
 if(-not $owned.errCopy.Wait(5000)){throw 'owned stderr drain uncertain'};$owned.stderr.Dispose();$owned.process.Dispose()
}
function Confirm-PipeOwner(){
 $pipe=[IO.Pipes.NamedPipeClientStream]::new('.',$pipeName,[IO.Pipes.PipeDirection]::InOut)
 try{$pipe.Connect(5000);[uint32]$seen=0;if(-not [NativePrepIdentity]::GetNamedPipeServerProcessId($pipe.SafePipeHandle,[ref]$seen)){throw [ComponentModel.Win32Exception]::new([Runtime.InteropServices.Marshal]::GetLastWin32Error())};$birth=([NativePrepIdentity]::Birth($server.handle)).ToString();$exit=[NativePrepIdentity]::Exit($server.handle);if($seen -ne $server.record.pid -or $birth -ne $server.record.birth_native -or $exit -ne 259){throw 'connected pipe is not exact recorded live own server'};$receipt.pipe_owner_checks+=@{pid=$seen;birth_native=$birth;matched=$true;exit=$exit};Save;return $pipe}catch{$pipe.Dispose();throw}
}
function Ctr([string]$name,[string[]]$arguments,[int]$budget){
 $pipe=Confirm-PipeOwner;$owned=$null
 try{$owned=Start-Owned $ctr (@('-a',$address,'-n',$namespace)+$arguments) $name $false $false;if(-not $owned.process.WaitForExit($budget)){throw 'finite ctr phase deadline'};$code=[NativePrepIdentity]::Exit($owned.handle);End-Owned $owned $false;$owned=$null;if($code -ne 0){throw ('ctr command failed: '+$name+' exit '+$code)};return [IO.File]::ReadAllText(($root+'\'+$name+'.stdout.log'))}finally{if($null -ne $owned){End-Owned $owned $true};$pipe.Dispose()}
}
Measure-Owned
$server=$null;$producer=$null;$consumer=$null;$pipe=$null
try{
 $server=Start-Owned $serverExe @('--root',($image+'\root'),'--state',($root+'\state'),'--address',$address) 'containerd' $false $false
 $pipe=Confirm-PipeOwner;$pipe.Dispose();$pipe=$null
 $before=Ctr 'snapshots-before' @('snapshots','--snapshotter','windows','ls') 10000
 if(@($before -split '\r?\n'|Where-Object{$_ -match '^sha256:7dd5847c8729728bf9037115c47152e0dbb67e2349b82aabde1f0d7d0387f1bc\s' -and $_ -match '\bCommitted\b'}).Count -ne 1){throw 'committed cached base absent; refuse full re-import fallback'}
 $tar='C:\Windows\System32\tar.exe';$receipt.tar_binary_sha256=Hash $tar;if($receipt.tar_binary_sha256 -ne 'a3ac62fd4cdf619a7c53f71ca3fe5fb42f72020975670ff29afe3603f6416a8e'){throw 'existing System32 tar binary differs'}
 $argsTar=@('--format','ustar','-b','1','-cf','-','-C',($root+'\metadata'),'oci-layout','index.json','-C',$content)+@($blobs|ForEach-Object{'blobs/sha256/'+$_.digest})
 # Six ustar files; each header/data rounded to512, with one terminal512 record slack beyond two end blocks.
 $extent=1536L;foreach($b in $blobs){$extent+=512L+([long][Math]::Ceiling($b.bytes/512.0))*512L};foreach($n in @('oci-layout','index.json')){$extent+=512L+([long][Math]::Ceiling((Get-Item -LiteralPath ($root+'\metadata\'+$n)).Length/512.0))*512L}
 $receipt.archive_maximum_bytes=$extent;Save
 $pipe=Confirm-PipeOwner
 $consumer=Start-Owned $ctr @('-a',$address,'-n',$namespace,'images','import','--local','--digests','--base-name','mcr.microsoft.com/windows/server','--platform','windows/amd64','--snapshotter','windows','-') 'cached-import' $true $false
 $producer=Start-Owned $tar $argsTar 'cached-archive-producer' $false $true
 $timer=[Diagnostics.Stopwatch]::StartNew();$pump=[NativeCachedArchivePump]::Start($producer.process.StandardOutput.BaseStream,$consumer.process.StandardInput.BaseStream,$extent,1000000)
 if(-not $pump.Wait(1000000)){throw 'original1000s cached import pipeline deadline'}
 $receipt.archive=$pump.Result;$consumer.process.StandardInput.Close();Save
 $remaining=1000000-[int]$timer.ElapsedMilliseconds;if($remaining -le 0 -or -not $producer.process.WaitForExit($remaining)){throw 'original1000s producer deadline'}
 $remaining=1000000-[int]$timer.ElapsedMilliseconds;if($remaining -le 0 -or -not $consumer.process.WaitForExit($remaining)){throw 'original1000s importer deadline'}
 $producerExit=[NativePrepIdentity]::Exit($producer.handle);$consumerExit=[NativePrepIdentity]::Exit($consumer.handle)
 End-Owned $producer $false;$producer=$null;End-Owned $consumer $false;$consumer=$null;$pipe.Dispose();$pipe=$null
 Measure-Owned
 if($producerExit -ne 0 -or $consumerExit -ne 0){throw 'cached-only archive/import failed; retain original and new state, no retry'}
 $after=Ctr 'snapshots-after' @('snapshots','--snapshotter','windows','ls') 10000
 if(@($after -split '\r?\n'|Where-Object{$_ -match '^sha256:49639eb7e32ce1d8a18cc2504b183a024475038fcba89f069a50751e8eef5a8d\s' -and $_ -match '\bCommitted\b'}).Count -ne 1){throw 'exact two-layer head not committed'}
 $mount=Ctr 'parent-paths' @('snapshots','--snapshotter','windows','prepare','--mounts','native-cached-v3-path-read','sha256:49639eb7e32ce1d8a18cc2504b183a024475038fcba89f069a50751e8eef5a8d') 30000
 $null=Ctr 'remove-own-path-read' @('snapshots','--snapshotter','windows','rm','native-cached-v3-path-read') 30000
 $options=@(($mount|ConvertFrom-Json)|ForEach-Object{$_.Options}|Where-Object{$_ -like 'parentLayerPaths=*'})
 if($options.Count -ne 1){throw 'one native parent-layer array required'};$layers=@($options[0].Substring('parentLayerPaths='.Length)|ConvertFrom-Json)
 if($layers.Count -ne 2){throw 'exact pinned two-layer image not established'}
 foreach($path in $layers){if(-not $path.StartsWith(($image+'\root\'),[StringComparison]::OrdinalIgnoreCase) -or -not (Test-Path -LiteralPath $path)){throw 'parent escaped exact owned image root or absent'}}
 if($layers -notcontains ($image+'\root\io.containerd.snapshotter.v1.windows\snapshots\1')){throw 'committed original base was replaced; no repick/re-import accepted'}
 $receipt.layers=$layers
 $receipt.required_dlls=@(foreach($name in @('ntdll.dll','gdi32.dll','dxgi.dll','d3d12.dll','d3dcompiler_47.dll')){$found=@(foreach($layer in $layers){$p=$layer+'\Files\Windows\System32\'+$name;if(Test-Path -LiteralPath $p){@{path=$p;sha256=(Hash $p)}}});@{name=$name;files=$found;present=($found.Count -gt 0)}})
 if(@($receipt.required_dlls|Where-Object{-not $_.present}).Count -ne 0){throw 'complete pinned image still missing explicit compute prerequisites'}
 Measure-Owned;$receipt.completed=$true;Save
}finally{
 foreach($owned in @($producer,$consumer)){if($null -ne $owned){try{End-Owned $owned $true}catch{$receipt.cleanup_errors+=@($_.ToString());Save}}}
 if($null -ne $pipe){$pipe.Dispose()}
 if($null -ne $server){try{End-Owned $server $true}catch{$receipt.cleanup_errors+=@($_.ToString());Save}}
 try{Measure-Owned}catch{$receipt.cleanup_errors+=@($_.ToString());Save}
 $receipt.raw_log_hashes=@(Get-ChildItem -LiteralPath $root -File -Filter '*.log'|ForEach-Object{@{path=$_.FullName;bytes=$_.Length;sha256=(Hash $_.FullName)}});Save
}
[Console]::WriteLine(($receipt|ConvertTo-Json -Depth 20 -Compress))
if($receipt.cleanup_errors.Count -ne 0){throw 'cached import cleanup/measurement uncertainty retained'}
