# This bounded body is reviewed by ROOT before dispatch, and runs inside the caller's creation-time job.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$root='C:\Users\corph\.cache\hex-native-windows-hcs\full-image-prep-v2'
if(Test-Path -LiteralPath $root){throw 'owned image preparation path exists'}
[void][IO.Directory]::CreateDirectory($root)
$utf8=[Text.UTF8Encoding]::new($false)
foreach($name in @('root','state','temp')){[void][IO.Directory]::CreateDirectory((Join-Path $root $name))}
$env:TEMP=$root+'\temp';$env:TMP=$env:TEMP
$serverExe='C:\Users\corph\.cache\hex-native-s4\containerd\bin\containerd.exe'
$ctr='C:\Users\corph\.cache\hex-native-s4\containerd\bin\ctr.exe'
if((Get-FileHash -LiteralPath $serverExe -Algorithm SHA256).Hash -ne '0E982834DBB5DEA61DF1449EC4A8ED365EA9F9A51714D904F259A58FB6BF7CAD'){throw 'existing containerd binary differs'}
if((Get-FileHash -LiteralPath $ctr -Algorithm SHA256).Hash -ne '457D4A4CF07DF0ADA953A5640CA9C2EA7B18F2F16E07BFA2469983072CCEAB70'){throw 'existing ctr binary differs'}
$pipeName='hex-native-hcs-full-image-v2'
$address='\\.\pipe\'+$pipeName
$namespace='hex-native-hcs-full-image-v2'
$reference='mcr.microsoft.com/windows/server@sha256:a62109569303cfe0ab49b1618383615f0477cdc285529b98d9b7085902d0e745'
$receipt=[ordered]@{root=$root;driver_pid=$PID;reference=$reference;address=$address;namespace=$namespace;phases=@();native_gpu_dispatches=0;container_workloads=0;service_install_restart_or_driver_changes=0}
function Save(){[IO.File]::WriteAllText(($root+'\result.json'),($receipt|ConvertTo-Json -Depth 20),$utf8)}
# Checked native pipe/held-process identity; System32-only, read-only observation handles.
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
[assembly: DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
public static class NativePrepIdentity {
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool GetNamedPipeServerProcessId(Microsoft.Win32.SafeHandles.SafePipeHandle pipe,out uint process);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetProcessTimes(IntPtr process,out long birth,out long exit,out long kernel,out long user);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr process,out uint code);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern uint GetCompressedFileSizeW(string path,out uint high);
 [DllImport("kernel32.dll")] static extern void SetLastError(uint code);
 public static long Birth(IntPtr process){long birth,exit,kernel,user;if(!GetProcessTimes(process,out birth,out exit,out kernel,out user))throw new Win32Exception(Marshal.GetLastWin32Error());return birth;}
 public static uint Exit(IntPtr process){uint code;if(!GetExitCodeProcess(process,out code))throw new Win32Exception(Marshal.GetLastWin32Error());return code;}
 public class Allocation {public long entries,files,reparseEntries;public ulong logicalBytes,allocatedFileBytesUpperBound;}
 public static Allocation Measure(string root,int entryBudget){Allocation result=new Allocation();Queue<string> pending=new Queue<string>();pending.Enqueue(root);while(pending.Count>0){string current=pending.Dequeue();foreach(string path in Directory.EnumerateFileSystemEntries(current)){if(++result.entries>entryBudget)throw new IOException("explicit owned allocation-entry budget exceeded");FileAttributes attributes=File.GetAttributes(path);if((attributes&FileAttributes.ReparsePoint)!=0){result.reparseEntries++;continue;}if((attributes&FileAttributes.Directory)!=0){pending.Enqueue(path);continue;}result.files++;result.logicalBytes=checked(result.logicalBytes+(ulong)new FileInfo(path).Length);SetLastError(0);uint high;uint low=GetCompressedFileSizeW(path,out high);int error=Marshal.GetLastWin32Error();if(low==uint.MaxValue && error!=0)throw new Win32Exception(error);result.allocatedFileBytesUpperBound=checked(result.allocatedFileBytesUpperBound+((ulong)high<<32)+low);}}return result;}
}
'@ -OutputAssembly ($root+'\identity.dll')
if($null -eq ('NativePrepIdentity' -as [type])){Add-Type -Path ($root+'\identity.dll')}
$receipt.identity_binary_sha256=(Get-FileHash -LiteralPath ($root+'\identity.dll') -Algorithm SHA256).Hash.ToLowerInvariant()
$receipt.disk_admission_bytes=34359738368L
$receipt.allocation_scope='GetCompressedFileSizeW over owned regular files; hardlinks double-counted (upper bound), reparse entries skipped and counted; NTFS directory/MFT metadata not attributed'
function Measure-Owned(){
 $allocation=[NativePrepIdentity]::Measure($root,1048576)
 $receipt.owned_allocation=$allocation;Save
 if($allocation.allocatedFileBytesUpperBound -gt $receipt.disk_admission_bytes -or $allocation.logicalBytes -gt $receipt.disk_admission_bytes){throw 'measured owned image payload exceeds admitted32GiB; retain partials and refuse completion'}
}
function Start-Owned([string]$exe,[string[]]$arguments,[string]$name){
 $start=[Diagnostics.ProcessStartInfo]::new();$start.FileName=$exe;$start.Arguments=($arguments -join ' ');$start.WorkingDirectory=$root;$start.UseShellExecute=$false;$start.CreateNoWindow=$true;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
 $process=[Diagnostics.Process]::new();$process.StartInfo=$start
 $stdout=[IO.File]::Open(($root+'\'+$name+'.stdout.log'),[IO.FileMode]::CreateNew);$stderr=[IO.File]::Open(($root+'\'+$name+'.stderr.log'),[IO.FileMode]::CreateNew)
 if(-not $process.Start()){$stdout.Dispose();$stderr.Dispose();throw 'owned native process did not start'}
 $held=$process.Handle
 if($held -eq [IntPtr]::Zero){throw 'owned native process has no held observation handle'}
 return [pscustomobject]@{process=$process;handle=$held;birth=[NativePrepIdentity]::Birth($held);stdout=$stdout;stderr=$stderr;outCopy=$process.StandardOutput.BaseStream.CopyToAsync($stdout);errCopy=$process.StandardError.BaseStream.CopyToAsync($stderr)}
}
function Finish-Streams($owned){
 if(-not $owned.outCopy.Wait(5000) -or -not $owned.errCopy.Wait(5000)){throw 'owned native output drain deadline'}
 $owned.stdout.Dispose();$owned.stderr.Dispose()
}
function Confirm-PipeOwner(){
 $connection=[IO.Pipes.NamedPipeClientStream]::new('.',$pipeName,[IO.Pipes.PipeDirection]::InOut)
 try{
  $connection.Connect(5000)
  [uint32]$observed=0
  if(-not [NativePrepIdentity]::GetNamedPipeServerProcessId($connection.SafePipeHandle,[ref]$observed)){throw [ComponentModel.Win32Exception]::new([Runtime.InteropServices.Marshal]::GetLastWin32Error())}
  $birth=[NativePrepIdentity]::Birth($server.handle);$exit=[NativePrepIdentity]::Exit($server.handle)
  if($observed -ne $receipt.server_pid -or $birth.ToString() -ne $receipt.server_birth_native -or $exit -ne 259){throw 'connected pipe does not name the exact live recorded containerd PID/birth; no ctr action authorized'}
  $receipt.pipe_owner_checks+=@{observed_server_pid=$observed;recorded_server_pid=$receipt.server_pid;birth_native=$birth.ToString();exit_code=$exit;matched=$true};Save
  return $connection
 }catch{$connection.Dispose();throw}
}
function Ctr([string]$name,[string[]]$arguments,[int]$budget){
 $connection=Confirm-PipeOwner
 $owned=$null
 try{
  $owned=Start-Owned $ctr (@('-a',$address,'-n',$namespace)+$arguments) $name
  $p=$owned.process
  $phase=[ordered]@{name=$name;pid=$p.Id;birth_native=$owned.birth.ToString();arguments=$arguments;budget_ms=$budget}
  $receipt.phases+=,$phase;Save
  if(-not $p.WaitForExit($budget)){$phase.timeout=$true;Save;$p.Kill();if(-not $p.WaitForExit(5000)){throw 'exact owned ctr child cleanup uncertain'};throw 'owned ctr deadline; original partial image/evidence retained'}
  $phase.exit=[NativePrepIdentity]::Exit($owned.handle);$phase.reaped=$true
  Finish-Streams $owned
  $phase.stdout_sha256=(Get-FileHash -LiteralPath ($root+'\'+$name+'.stdout.log') -Algorithm SHA256).Hash.ToLowerInvariant();$phase.stderr_sha256=(Get-FileHash -LiteralPath ($root+'\'+$name+'.stderr.log') -Algorithm SHA256).Hash.ToLowerInvariant();Save
  Measure-Owned
  if($phase.exit -ne 0){throw ('ctr phase failed '+$name)}
  return [IO.File]::ReadAllText(($root+'\'+$name+'.stdout.log'))
 }finally{if($null -ne $owned){$owned.process.Dispose()};$connection.Dispose()}
}

$server=$null
try{
 $receipt.pipe_owner_checks=@()
 $server=Start-Owned $serverExe @('--root',($root+'\root'),'--state',($root+'\state'),'--address',$address) 'containerd'
 $receipt.server_pid=$server.process.Id;$receipt.server_birth_native=$server.birth.ToString();Save
 $connection=Confirm-PipeOwner
 try{$receipt.private_pipe_connected=$connection.IsConnected}finally{$connection.Dispose()}
 Save
 $null=Ctr 'version' @('version') 10000
 $null=Ctr 'pull' @('images','pull','--snapshotter','windows','--platform','windows/amd64',$reference) 1000000
 $listing=Ctr 'snapshots' @('snapshots','--snapshotter','windows','ls') 10000
 $committed=@($listing -split '\r?\n'|Where-Object{$_ -match '\bCommitted\b'})
 $keys=@($committed|ForEach-Object{($_ -split '\s+')[0]});$parents=@($committed|ForEach-Object{($_ -split '\s+')[1]});$head=@($keys|Where-Object{$parents -notcontains $_})
 if($head.Count -ne 1){throw 'one exact newly imported image chain head required'}
 $scratchKey='hex-native-hcs-layer-path-read-v2'
 $mount=Ctr 'parent-paths' @('snapshots','--snapshotter','windows','prepare','--mounts',$scratchKey,$head[0]) 30000
 $null=Ctr 'remove-own-path-read' @('snapshots','--snapshotter','windows','rm',$scratchKey) 30000
 $options=@(($mount|ConvertFrom-Json)|ForEach-Object{$_.Options}|Where-Object{$_ -like 'parentLayerPaths=*'})
 if($options.Count -ne 1){throw 'one actual parent-layer array required'}
 $layers=@($options[0].Substring('parentLayerPaths='.Length)|ConvertFrom-Json)
 if($layers.Count -ne 2){throw 'pinned two-layer image did not import as two exact parents'}
 foreach($path in $layers){if(-not $path.StartsWith(($root+'\root\'),[StringComparison]::OrdinalIgnoreCase) -or -not (Test-Path -LiteralPath $path)){throw 'layer escaped new owned image root or is absent'}}
 $receipt.layers=$layers
 [IO.File]::WriteAllText(($root+'\layers.json'),($layers|ConvertTo-Json),$utf8)
 $receipt.required_dlls=@(foreach($name in @('ntdll.dll','gdi32.dll','dxgi.dll','d3d12.dll','d3dcompiler_47.dll')){$found=@(foreach($layer in $layers){$p=Join-Path $layer ('Files\Windows\System32\'+$name);if(Test-Path -LiteralPath $p){@{path=$p;sha256=(Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant()}}});@{name=$name;present=($found.Count -gt 0);files=$found}})
 if(@($receipt.required_dlls|Where-Object{-not $_.present}).Count -ne 0){throw 'full pinned image still lacks explicit native witness DLL prerequisites'}
 Measure-Owned
 $receipt.completed=$true;Save
}finally{
 if($null -ne $server){if([NativePrepIdentity]::Birth($server.handle).ToString() -ne $receipt.server_birth_native){throw 'owned server handle birth changed'};if([NativePrepIdentity]::Exit($server.handle) -eq 259){$server.process.Kill()};$receipt.server_reaped=$server.process.WaitForExit(5000);$receipt.server_exit=[NativePrepIdentity]::Exit($server.handle);Finish-Streams $server;$server.process.Dispose();Save}
}
[Console]::WriteLine(($receipt|ConvertTo-Json -Depth 20 -Compress))
