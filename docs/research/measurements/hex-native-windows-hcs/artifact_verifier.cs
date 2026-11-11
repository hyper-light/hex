using System;
using System.IO;
using System.Text;
using System.ComponentModel;
using System.Runtime.InteropServices;
[assembly: DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
public static class NativeArtifactVerifier {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateFileW(string path,uint access,uint sharing,IntPtr security,uint creation,uint flags,IntPtr template);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern uint GetFinalPathNameByHandleW(IntPtr file,StringBuilder path,uint length,uint flags);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
 public static string Canonical(string path) {
  IntPtr file=CreateFileW(path,128,7,IntPtr.Zero,3,0x02000000,IntPtr.Zero);
  if(file==new IntPtr(-1) || file==IntPtr.Zero)throw new Win32Exception(Marshal.GetLastWin32Error());
  try {
   // Win32 extended path ceiling including NUL; this is a caller read bound, never an allocation cache.
   StringBuilder name=new StringBuilder(32768);uint count=GetFinalPathNameByHandleW(file,name,32768,0);
   if(count==0)throw new Win32Exception(Marshal.GetLastWin32Error());if(count>=32768)throw new IOException("canonical native path exceeded Win32 extent");
   return name.ToString();
  } finally {if(!CloseHandle(file))throw new Win32Exception(Marshal.GetLastWin32Error());}
 }
 public static ushort PeMachine(string path) {
  using(FileStream file=File.OpenRead(path))using(BinaryReader bytes=new BinaryReader(file)) {
   if(file.Length<64 || bytes.ReadUInt16()!=0x5a4d)throw new IOException("artifact is not an MZ executable");
   file.Position=60;uint offset=bytes.ReadUInt32();if((ulong)offset+6>(ulong)file.Length)throw new IOException("PE header lies outside artifact");
   file.Position=offset;if(bytes.ReadUInt32()!=0x4550)throw new IOException("artifact has no PE signature");return bytes.ReadUInt16();
  }
 }
}
