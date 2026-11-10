
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$root=Join-Path $env:USERPROFILE '.cache\hex-windows-device-attachment\kmt-read-v1'
if(Test-Path -LiteralPath $root){throw 'owned read root exists'}
[IO.Directory]::CreateDirectory($root)|Out-Null
$env:TEMP=$root;$env:TMP=$root
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class HexKmtRead {
 [StructLayout(LayoutKind.Sequential)] public struct Luid { public UInt32 LowPart; public Int32 HighPart; }
 [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] public struct Open {
  [MarshalAs(UnmanagedType.LPWStr)] public string DeviceName;
  public UInt32 Adapter;
  public Luid Id;
 }
 [StructLayout(LayoutKind.Sequential)] public struct Close { public UInt32 Adapter; }
 [DllImport("gdi32.dll", ExactSpelling=true)] public static extern Int32 D3DKMTOpenAdapterFromDeviceName(ref Open value);
 [DllImport("gdi32.dll", ExactSpelling=true)] public static extern Int32 D3DKMTCloseAdapter(ref Close value);
}
'@
$path='\\?\PCI#VEN_1002&DEV_1586&SUBSYS_1FB31043&REV_C1#4&35fe04f8&0&0041#{1ca05180-a699-450a-9a0c-de4fbe3ddd89}'
$open=[HexKmtRead+Open]::new();$open.DeviceName=$path
$status=[HexKmtRead]::D3DKMTOpenAdapterFromDeviceName([ref]$open)
$result=[ordered]@{source='ROOT read-only native identity witness, no D3D/compute/HCS';path=$path;open_ntstatus=$status;adapter_handle=$open.Adapter;low=$open.Id.LowPart;high=$open.Id.HighPart;open_struct_bytes=[Runtime.InteropServices.Marshal]::SizeOf($open);close_ntstatus=$null}
if($status -eq 0){
 $close=[HexKmtRead+Close]::new();$close.Adapter=$open.Adapter
 $result.close_ntstatus=[HexKmtRead]::D3DKMTCloseAdapter([ref]$close)
}
$utf8=[Text.UTF8Encoding]::new($false)
[IO.File]::WriteAllText((Join-Path $root 'result.json'),($result|ConvertTo-Json -Depth 5),$utf8)
[Console]::WriteLine(($result|ConvertTo-Json -Depth 5))
if($status -ne 0 -or $result.close_ntstatus -ne 0){throw 'KMT identity/owned-close unverified'}
