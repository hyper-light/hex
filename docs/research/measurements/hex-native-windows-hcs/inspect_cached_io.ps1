# One exact-owned identity/IO observation; no process selection by args and no mutation.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$base='C:\Users\corph\.cache\hex-native-windows-hcs';$root=$base+'\cached-io-observer-v1'
if(Test-Path -LiteralPath $root){throw 'owned IO observer exists'}
[void][IO.Directory]::CreateDirectory($root);[void][IO.Directory]::CreateDirectory(($root+'\temp'));$env:TEMP=$root+'\temp';$env:TMP=$env:TEMP
Add-Type -TypeDefinition @'
using System;using System.ComponentModel;using System.Runtime.InteropServices;
[assembly: DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
public static class OwnedCachedIo {
 [StructLayout(LayoutKind.Sequential)] public struct Io {public ulong readOps,writeOps,otherOps,readBytes,writeBytes,otherBytes;}
 [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenProcess(uint access,bool inherit,uint pid);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetProcessTimes(IntPtr process,out long birth,out long end,out long kernel,out long user);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetProcessIoCounters(IntPtr process,out Io io);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr process,out uint code);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
 public class Fact {public uint pid,exit;public int openError;public long birth,kernel100ns,user100ns;public bool exact;public Io io;}
 static void Check(bool okay){if(!okay)throw new Win32Exception(Marshal.GetLastWin32Error());}
 public static Fact Inspect(uint pid,long expected){Fact result=new Fact{pid=pid};IntPtr process=OpenProcess(0x1000,false,pid);if(process==IntPtr.Zero){result.openError=Marshal.GetLastWin32Error();return result;}try{long end;Check(GetProcessTimes(process,out result.birth,out end,out result.kernel100ns,out result.user100ns));result.exact=result.birth==expected;if(!result.exact)return result;Check(GetExitCodeProcess(process,out result.exit));Check(GetProcessIoCounters(process,out result.io));return result;}finally{Check(CloseHandle(process));}}
}
'@ -OutputAssembly ($root+'\observer.dll')
Add-Type -Path ($root+'\observer.dll')
$body=Get-Content -LiteralPath ($base+'\cached-image-import-v3\result.json') -Raw|ConvertFrom-Json
$facts=@(foreach($phase in $body.phases){[OwnedCachedIo]::Inspect([uint32]$phase.pid,[int64]$phase.birth_native)})
$v=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');facts=$facts;archive_complete=($null -ne $body.archive);archive=if($null -ne $body.archive){$body.archive}else{$null};binary_sha256=(Get-FileHash -LiteralPath ($root+'\observer.dll') -Algorithm SHA256).Hash.ToLowerInvariant();new_imports=0;native_gpu_dispatches=0;timer_or_resource_changes=0}
$json=$v|ConvertTo-Json -Depth 20;[IO.File]::WriteAllText(($root+'\result.json'),$json,[Text.UTF8Encoding]::new($false));[Console]::WriteLine($json)
