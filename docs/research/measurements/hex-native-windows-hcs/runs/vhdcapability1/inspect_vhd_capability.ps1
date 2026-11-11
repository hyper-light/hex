# Read-only current-token, API, source-VHD and NTFS geometry inspection; no privilege enablement.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$base='C:\Users\corph\.cache\hex-native-windows-hcs';$root=$base+'\vhd-capability-v1'
if(Test-Path -LiteralPath $root){throw 'owned capability receipt exists'}
[void][IO.Directory]::CreateDirectory($root);[void][IO.Directory]::CreateDirectory(($root+'\temp'));$env:TEMP=$root+'\temp';$env:TMP=$env:TEMP
Add-Type -TypeDefinition @'
using System;using System.Collections.Generic;using System.ComponentModel;using System.Runtime.InteropServices;using System.Text;
[assembly: DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
public static class OwnedVhdCapability {
 [StructLayout(LayoutKind.Sequential)] struct Storage {public uint device;public Guid vendor;}
 [StructLayout(LayoutKind.Sequential)] struct OpenParameters {public uint version,depth;}
 [StructLayout(LayoutKind.Explicit,Size=40)] struct DiskInfo {
  [FieldOffset(0)] public uint version;[FieldOffset(8)] public ulong virtualBytes;[FieldOffset(16)] public ulong physicalBytes;[FieldOffset(24)] public uint blockBytes;[FieldOffset(28)] public uint sectorBytes;
 }
 [DllImport("virtdisk.dll",CharSet=CharSet.Unicode)] static extern uint OpenVirtualDisk(ref Storage storage,string path,uint access,uint flags,ref OpenParameters parameters,out IntPtr disk);
 [DllImport("virtdisk.dll")] static extern uint GetVirtualDiskInformation(IntPtr disk,ref uint bytes,ref DiskInfo info,IntPtr used);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
 [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool OpenProcessToken(IntPtr process,uint access,out IntPtr token);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool GetTokenInformation(IntPtr token,int kind,IntPtr data,uint length,out uint needed);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool LookupPrivilegeNameW(string system,IntPtr luid,StringBuilder name,ref uint size);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool GetDiskFreeSpaceW(string path,out uint clusterSectors,out uint sectorBytes,out uint freeClusters,out uint clusters);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool GetVolumeInformationW(string root,StringBuilder volume,uint volumeSize,out uint serial,out uint maxName,out uint flags,StringBuilder fs,uint fsSize);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr LoadLibraryExW(string path,IntPtr reserved,uint flags);
 [DllImport("kernel32.dll",CharSet=CharSet.Ansi,ExactSpelling=true)] static extern IntPtr GetProcAddress(IntPtr module,string name);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool FreeLibrary(IntPtr module);
 static void Check(bool ok){if(!ok)throw new Win32Exception(Marshal.GetLastWin32Error());}
 public class Vhd {public uint openError,infoError;public ulong virtualBytes,physicalBytes;public uint blockBytes,sectorBytes;}
 public static Vhd Inspect(string path){Storage storage=new Storage{device=3,vendor=new Guid("ec984aec-a0f9-47e9-901f-71415a66345b")};OpenParameters parameters=new OpenParameters{version=1,depth=1};IntPtr disk;Vhd result=new Vhd();result.openError=OpenVirtualDisk(ref storage,path,0x00080000,0,ref parameters,out disk);if(result.openError!=0)return result;
  try{DiskInfo info=new DiskInfo{version=1};uint bytes=40;result.infoError=GetVirtualDiskInformation(disk,ref bytes,ref info,IntPtr.Zero);if(result.infoError==0){result.virtualBytes=info.virtualBytes;result.physicalBytes=info.physicalBytes;result.blockBytes=info.blockBytes;result.sectorBytes=info.sectorBytes;}return result;}finally{Check(CloseHandle(disk));}
 }
 public class Privilege {public string name;public uint attributes;public bool enabled;}
 public static List<Privilege> Privileges(){IntPtr token;Check(OpenProcessToken(GetCurrentProcess(),8,out token));IntPtr data=IntPtr.Zero;
  try{uint needed;GetTokenInformation(token,3,IntPtr.Zero,0,out needed);int error=Marshal.GetLastWin32Error();if(error!=122 || needed<4 || needed>1048576)throw new Win32Exception(error,"bounded current-token privilege extent");data=Marshal.AllocHGlobal((int)needed);Check(GetTokenInformation(token,3,data,needed,out needed));uint count=(uint)Marshal.ReadInt32(data);if((ulong)count*12+4>needed)throw new InvalidOperationException("privilege array extent");List<Privilege> result=new List<Privilege>();for(uint i=0;i<count;i++){IntPtr luid=IntPtr.Add(data,checked(4+(int)i*12));uint size=0;LookupPrivilegeNameW(null,luid,null,ref size);if(size==0 || size>=32767)throw new InvalidOperationException("privilege name extent");StringBuilder name=new StringBuilder((int)size+1);size++;Check(LookupPrivilegeNameW(null,luid,name,ref size));string text=name.ToString();if(text=="SeManageVolumePrivilege" || text=="SeBackupPrivilege" || text=="SeRestorePrivilege" || text=="SeCreateSymbolicLinkPrivilege" || text=="SeSecurityPrivilege"){uint attributes=(uint)Marshal.ReadInt32(luid,8);result.Add(new Privilege{name=text,attributes=attributes,enabled=(attributes&2)!=0});}}return result;
  }finally{if(data!=IntPtr.Zero)Marshal.FreeHGlobal(data);Check(CloseHandle(token));}
 }
 public class Geometry {public uint clusterSectors,sectorBytes,freeClusters,clusters,serial,maximumName,flags;public string fs;}
 public static Geometry Volume(){Geometry g=new Geometry();Check(GetDiskFreeSpaceW("C:\\",out g.clusterSectors,out g.sectorBytes,out g.freeClusters,out g.clusters));StringBuilder volume=new StringBuilder(261),fs=new StringBuilder(261);Check(GetVolumeInformationW("C:\\",volume,261,out g.serial,out g.maximumName,out g.flags,fs,261));g.fs=fs.ToString();return g;}
 public static Dictionary<string,bool> Exports(){IntPtr library=LoadLibraryExW("virtdisk.dll",IntPtr.Zero,0x800);if(library==IntPtr.Zero)throw new Win32Exception(Marshal.GetLastWin32Error());try{Dictionary<string,bool> result=new Dictionary<string,bool>();foreach(string name in new[]{"CreateVirtualDisk","OpenVirtualDisk","AttachVirtualDisk","DetachVirtualDisk","GetVirtualDiskInformation","GetVirtualDiskPhysicalPath"})result.Add(name,GetProcAddress(library,name)!=IntPtr.Zero);return result;}finally{Check(FreeLibrary(library));}}
}
'@ -OutputAssembly ($root+'\capability.dll')
Add-Type -Path ($root+'\capability.dll')
$nano='C:\Users\corph\.cache\hex-native-s4\ctrd\root\io.containerd.snapshotter.v1.windows\snapshots\1\blank.vhdx'
if((Get-FileHash -LiteralPath $nano -Algorithm SHA256).Hash.ToLowerInvariant() -ne '67a3974475c80752217719d36bcf282d4b2374a83387ba1716896f66d7433d10'){throw 'existing scratch input pin changed'}
$v=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');geometry=[OwnedVhdCapability]::Volume();current_token_selected_privileges=[OwnedVhdCapability]::Privileges();api_exports=[OwnedVhdCapability]::Exports();vhds=@();service_state=@(Get-Service -Name vmcompute,vds,StorSvc -ErrorAction SilentlyContinue|ForEach-Object{@{name=$_.Name;status=$_.Status.ToString()}});binary_sha256=(Get-FileHash -LiteralPath ($root+'\capability.dll') -Algorithm SHA256).Hash.ToLowerInvariant();create_attach_format_or_quota_calls=0;privilege_changes=0;service_driver_security_changes=0}
foreach($path in @($nano,($base+'\full-image-prep-v2\root\io.containerd.snapshotter.v1.windows\snapshots\1\blank.vhdx'))){$v.vhds+=@{path=$path;sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant();information=[OwnedVhdCapability]::Inspect($path)}}
$json=$v|ConvertTo-Json -Depth 20;[IO.File]::WriteAllText(($root+'\result.json'),$json,[Text.UTF8Encoding]::new($false));[Console]::WriteLine($json)
