# Read-only correction after pathprobe1: 264 UTF-16 units fail legacy APIs; extended query succeeds.
# API definitions: https://learn.microsoft.com/windows/win32/api/fileapi/nf-fileapi-findfirstfilew
# https://learn.microsoft.com/windows/win32/api/winbase/ns-winbase-file_id_info
# https://learn.microsoft.com/windows/win32/api/winbase/ns-winbase-file_standard_info
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$root='C:\Users\corph\.cache\hex-native-windows-hcs\retained-image-meter-v1'
if(Test-Path -LiteralPath $root){throw 'owned retained meter exists'}
[void][IO.Directory]::CreateDirectory($root)
[void][IO.Directory]::CreateDirectory(($root+'\temp'));$env:TEMP=$root+'\temp';$env:TMP=$env:TEMP
Add-Type -TypeDefinition @'
using System;using System.Collections.Generic;using System.ComponentModel;using System.Diagnostics;using System.IO;using System.Runtime.InteropServices;using System.Security.Cryptography;
[assembly: DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
public static class NativeRetainedImage {
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct FindData {
  public uint attributes;public long created,accessed,written;public uint sizeHigh,sizeLow,reserved0,reserved1;
  [MarshalAs(UnmanagedType.ByValTStr,SizeConst=260)] public string name;
  [MarshalAs(UnmanagedType.ByValTStr,SizeConst=14)] public string alternate;
 }
 [StructLayout(LayoutKind.Sequential)] struct FileId {public ulong volume,low,high;}
 [StructLayout(LayoutKind.Sequential)] struct Standard {public long allocated,end;public uint links;public byte deletePending,directory;}
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr FindFirstFileW(string path,out FindData data);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool FindNextFileW(IntPtr search,out FindData data);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool FindClose(IntPtr search);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateFileW(string path,uint access,uint share,IntPtr security,uint creation,uint flags,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true,EntryPoint="GetFileInformationByHandleEx")] static extern bool Identity(IntPtr file,int kind,out FileId info,uint bytes);
 [DllImport("kernel32.dll",SetLastError=true,EntryPoint="GetFileInformationByHandleEx")] static extern bool Extent(IntPtr file,int kind,out Standard info,uint bytes);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool CloseHandle(IntPtr file);
 public class Allocation {public long entries,files,directories,reparseEntries,duplicateIds;public ulong retainedLogicalBytes,logicalBytesWithHardlinkDuplicates,allocatedBytes,regularFileAllocation,directoryAllocation,reparseAllocation;public int maximumUtf16Path;public long elapsedMs;}
 static void Check(bool ok){if(!ok)throw new Win32Exception(Marshal.GetLastWin32Error());}
 static void Budget(Stopwatch time,int milliseconds){if(time.ElapsedMilliseconds>=milliseconds)throw new IOException("original read-only measurement caller deadline");}
 public static Allocation Measure(string root,int entries,int milliseconds) {
  Allocation result=new Allocation();Stopwatch time=Stopwatch.StartNew();Queue<string> pending=new Queue<string>();HashSet<string> seen=new HashSet<string>(StringComparer.Ordinal);
  pending.Enqueue("\\\\?\\"+root.TrimEnd('\\'));
  while(pending.Count!=0){Budget(time,milliseconds);string directory=pending.Dequeue();FindData item;IntPtr search=FindFirstFileW(directory+"\\*",out item);
   if(search==new IntPtr(-1))throw new Win32Exception(Marshal.GetLastWin32Error(),directory);
   try {while(true){
    if(item.name!="." && item.name!=".."){
     Budget(time,milliseconds);if(++result.entries>entries)throw new IOException("owned one-million-entry measurement bound exceeded");
     string path=directory+"\\"+item.name;if(path.Length>=32767)throw new IOException("extended native path ceiling");result.maximumUtf16Path=Math.Max(result.maximumUtf16Path,path.Length);
     bool dir=(item.attributes&16)!=0,reparse=(item.attributes&1024)!=0;if(dir)result.directories++;else result.files++;if(reparse)result.reparseEntries++;
     if(dir && !reparse)pending.Enqueue(path);
     IntPtr file=CreateFileW(path,128,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero);if(file==new IntPtr(-1))throw new Win32Exception(Marshal.GetLastWin32Error(),path);
     try{FileId identity;Standard extent;Check(Identity(file,18,out identity,24));Check(Extent(file,1,out extent,(uint)Marshal.SizeOf(typeof(Standard))));
      if(extent.allocated<0 || extent.end<0)throw new IOException("negative native file extent");
      result.logicalBytesWithHardlinkDuplicates=checked(result.logicalBytesWithHardlinkDuplicates+(ulong)extent.end);
      string key=identity.volume.ToString("x16")+identity.low.ToString("x16")+identity.high.ToString("x16");
      if(!seen.Add(key))result.duplicateIds++;
      else{result.retainedLogicalBytes=checked(result.retainedLogicalBytes+(ulong)extent.end);result.allocatedBytes=checked(result.allocatedBytes+(ulong)extent.allocated);if(reparse)result.reparseAllocation=checked(result.reparseAllocation+(ulong)extent.allocated);else if(dir)result.directoryAllocation=checked(result.directoryAllocation+(ulong)extent.allocated);else result.regularFileAllocation=checked(result.regularFileAllocation+(ulong)extent.allocated);}
     }finally{Check(CloseHandle(file));}
    }
    if(!FindNextFileW(search,out item)){int error=Marshal.GetLastWin32Error();if(error!=18)throw new Win32Exception(error,directory);break;}
   }}finally{Check(FindClose(search));}
  }
  result.elapsedMs=time.ElapsedMilliseconds;return result;
 }
 public static string Hash(string path,long expected,int milliseconds){Stopwatch time=Stopwatch.StartNew();using(FileStream input=File.OpenRead(path))using(SHA256Cng hash=new SHA256Cng()){
  if(input.Length!=expected)throw new IOException("pinned blob extent differs");byte[] buffer=new byte[1048576];long total=0;int count;
  while((count=input.Read(buffer,0,buffer.Length))!=0){Budget(time,milliseconds);total=checked(total+count);if(total>expected)throw new IOException("pinned blob grew");hash.TransformBlock(buffer,0,count,buffer,0);}
  if(total!=expected)throw new IOException("pinned blob ended early");hash.TransformFinalBlock(buffer,0,0);return BitConverter.ToString(hash.Hash).Replace("-","").ToLowerInvariant();
 }}
}
'@ -OutputAssembly ($root+'\meter.dll')
Add-Type -Path ($root+'\meter.dll')
$image='C:\Users\corph\.cache\hex-native-windows-hcs\full-image-prep-v2'
$report=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');root=$image;read_only=$true;meter_binary_sha256=(Get-FileHash -LiteralPath ($root+'\meter.dll') -Algorithm SHA256).Hash.ToLowerInvariant();scope='GetFileInformationByHandleEx FileStandardInfo AllocationSize; hardlinks deduplicated by FileIdInfo volume+128-bit ID; reparse objects opened without following, directories traversed without reparse; extra named streams and volume-wide MFT metadata not attributed';allocation=$null;error=$null;blobs=@();native_workloads=0;server_starts=0;host_changes=0;disk_envelope=34359738368L}
try{$report.allocation=[NativeRetainedImage]::Measure($image,1048576,120000);$report.measured_overrun=($report.allocation.allocatedBytes -gt $report.disk_envelope)}catch{$report.error=$_.ToString()}
$blobRoot=$image+'\root\io.containerd.content.v1.content\blobs\sha256'
foreach($blob in @(@{digest='a62109569303cfe0ab49b1618383615f0477cdc285529b98d9b7085902d0e745';bytes=597L},@{digest='620d54468172e4e8185670f4830dfd5bbf4f20dc2b1852b3327a3c51b8d727c6';bytes=789L},@{digest='e5dcf3bc996337655b83ea98c2e05a33a75e36a0357b01d7d433dbabcd1509a7';bytes=4481593239L},@{digest='58b297594fae8487acb87861188616be04290bce96e6531d21161e836082b2f2';bytes=2175606732L})){
 $row=[ordered]@{expected=$blob.digest;expected_bytes=$blob.bytes;path=($blobRoot+'\'+$blob.digest);actual=$null;error=$null}
 try{$row.actual=[NativeRetainedImage]::Hash($row.path,$blob.bytes,60000);$row.matches=($row.actual -eq $row.expected)}catch{$row.error=$_.ToString()}
 $report.blobs+=,$row
}
$json=$report|ConvertTo-Json -Depth 20
[IO.File]::WriteAllText(($root+'\result.json'),$json,[Text.UTF8Encoding]::new($false));[Console]::WriteLine($json)
if($null -ne $report.error -or $report.measured_overrun -or @($report.blobs|Where-Object{$null -ne $_.error -or -not $_.matches}).Count -ne 0){exit 1}
