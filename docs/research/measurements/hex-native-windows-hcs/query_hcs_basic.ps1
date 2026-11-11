# Same read-only Basic query as windows/oci/sys.rs::available, retaining the actual version data.
# Microsoft schema: Basic reports supported schema versions; no compute system is created.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$root='C:\Users\corph\.cache\hex-native-windows-hcs\hcs-service-basic-v1'
if(Test-Path -LiteralPath $root){throw 'owned service query exists'}
[void][IO.Directory]::CreateDirectory($root);[void][IO.Directory]::CreateDirectory(($root+'\temp'));$env:TEMP=$root+'\temp';$env:TMP=$env:TEMP
$owner=@{pid=$PID;birth=[Diagnostics.Process]::GetCurrentProcess().StartTime.ToUniversalTime().ToFileTimeUtc().ToString()}
[IO.File]::WriteAllText(($root+'\owner.json'),($owner|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
Add-Type -TypeDefinition @'
using System;using System.IO;using System.Runtime.InteropServices;
[assembly:DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
public static class NativeHcsBasic {
 [DllImport("computecore.dll",CharSet=CharSet.Unicode,ExactSpelling=true)] static extern int HcsGetServiceProperties(string query,out IntPtr result);
 [DllImport("kernel32.dll",ExactSpelling=true)] static extern IntPtr LocalFree(IntPtr value);
 public class Result {public int hr;public string json;public bool resultFreed;}
 public static Result Query(){IntPtr value;Result result=new Result();result.hr=HcsGetServiceProperties("{\"PropertyTypes\":[\"Basic\"]}",out value);if(value==IntPtr.Zero){result.resultFreed=true;return result;}
  try{int count=0;while(count<4194304 && Marshal.ReadInt16(value,count*2)!=0)count++;if(count==4194304)throw new IOException("existing HCS result character bound exceeded");result.json=Marshal.PtrToStringUni(value,count);}finally{result.resultFreed=LocalFree(value)==IntPtr.Zero;if(!result.resultFreed)throw new IOException("owned HCS result LocalFree failed");}return result;}
}
'@ -OutputAssembly ($root+'\query.dll')
if($null -eq ('NativeHcsBasic' -as [type])){Add-Type -Path ($root+'\query.dll')}
$query=[NativeHcsBasic]::Query()
$v=[ordered]@{owner=$owner;query='{"PropertyTypes":["Basic"]}';result=$query;query_binary_sha256=(Get-FileHash -LiteralPath ($root+'\query.dll') -Algorithm SHA256).Hash.ToLowerInvariant();computecore_sha256=(Get-FileHash -LiteralPath 'C:\Windows\System32\computecore.dll' -Algorithm SHA256).Hash.ToLowerInvariant();compute_system_creates=0;gpu_dispatches=0;service_changes=0;utc=[DateTime]::UtcNow.ToString('o')}
$json=$v|ConvertTo-Json -Depth 8;[IO.File]::WriteAllText(($root+'\result.json'),$json,[Text.UTF8Encoding]::new($false));[Console]::WriteLine($json)
if($query.hr -lt 0 -or -not $query.resultFreed){exit 1}
