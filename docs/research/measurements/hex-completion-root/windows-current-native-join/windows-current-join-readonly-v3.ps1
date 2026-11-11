$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3
$self = [System.Diagnostics.Process]::GetCurrentProcess()
$self.PriorityClass = 'BelowNormal'
$lab = Join-Path $env:USERPROFILE '.cache\hex-lead\current-gpu-join-8ccb-readonly-v3'
if (Test-Path -LiteralPath $lab) { throw 'private read-only proof directory already exists' }
[void](New-Item -ItemType Directory -Path $lab)
$env:TEMP = $lab
$env:TMP = $lab
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class HexReadonlyGpuJoin {
 [StructLayout(LayoutKind.Sequential)] public struct Luid { public UInt32 Low; public Int32 High; }
 [StructLayout(LayoutKind.Sequential)] public struct Open { public IntPtr DeviceName; public UInt32 Adapter; public Luid Id; }
 [StructLayout(LayoutKind.Sequential)] public struct Close { public UInt32 Adapter; }
 [DllImport("cfgmgr32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)] public static extern UInt32 CM_Get_Device_Interface_List_SizeW(out UInt32 length, ref Guid kind, string instance, UInt32 flags);
 [DllImport("cfgmgr32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)] public static extern UInt32 CM_Get_Device_Interface_ListW(ref Guid kind, string instance, IntPtr buffer, UInt32 length, UInt32 flags);
 [DllImport("gdi32.dll", ExactSpelling=true)] public static extern Int32 D3DKMTOpenAdapterFromDeviceName(ref Open value);
 [DllImport("gdi32.dll", ExactSpelling=true)] public static extern Int32 D3DKMTCloseAdapter(ref Close value);
}
'@
$kind = [Guid]'1ca05180-a699-450a-9a0c-de4fbe3ddd89'
$rows = @()
$opened = 0
$closed = 0
$failed = $false
$cards = @(Get-CimInstance Win32_VideoController | Select-Object Name,PNPDeviceID,DriverVersion)
foreach ($card in $cards) {
 [uint32]$count = 0
 $status = [HexReadonlyGpuJoin]::CM_Get_Device_Interface_List_SizeW([ref]$count,[ref]$kind,$card.PNPDeviceID,0)
 if ($status -ne 0 -or $count -eq 0 -or $count -gt 65536) { throw "CM size/refusal status=$status count=$count; caller read budget65536 UTF16 code units" }
 $buffer = [Runtime.InteropServices.Marshal]::AllocHGlobal([int]$count * 2)
 try {
  $status = [HexReadonlyGpuJoin]::CM_Get_Device_Interface_ListW([ref]$kind,$card.PNPDeviceID,$buffer,$count,0)
  if ($status -ne 0) { throw "CM actual interface read refused status=$status" }
  $chars = New-Object char[] ([int]$count)
  [Runtime.InteropServices.Marshal]::Copy($buffer,$chars,0,[int]$count)
  $actual = -join $chars
  $interfaces = @($actual.Split([char[]]@([char]0),[StringSplitOptions]::RemoveEmptyEntries))
 } finally { [Runtime.InteropServices.Marshal]::FreeHGlobal($buffer) }
 foreach ($path in $interfaces) {
  $name = [Runtime.InteropServices.Marshal]::StringToHGlobalUni($path)
  try {
   $open = New-Object HexReadonlyGpuJoin+Open
   $open.DeviceName = $name
   $status = [HexReadonlyGpuJoin]::D3DKMTOpenAdapterFromDeviceName([ref]$open)
   $closeStatus = $null
   $luid = $null
   if ($status -eq 0) {
    $opened++
    try {
     $high = [BitConverter]::ToUInt32([BitConverter]::GetBytes([int]$open.Id.High),0)
     $luid = '{0:x8}{1:x8}' -f $high,$open.Id.Low
    } finally {
     $close = New-Object HexReadonlyGpuJoin+Close
     $close.Adapter = $open.Adapter
     $closeStatus = [HexReadonlyGpuJoin]::D3DKMTCloseAdapter([ref]$close)
     if ($closeStatus -eq 0) { $closed++ } else { $failed = $true }
    }
   } else { $failed = $true }
   $rows += [ordered]@{pnp_instance=$card.PNPDeviceID;name=$card.Name;driver=$card.DriverVersion;interface=$path;interface_class=$kind.ToString();open_ntstatus=$status;luid=$luid;close_ntstatus=$closeStatus}
  } finally { [Runtime.InteropServices.Marshal]::FreeHGlobal($name) }
 }
}
$result = [ordered]@{scope='ROOT exact current CM PRESENT filtered-PNP -> KMT LUID query only; no compute/HCS/VM/driver/service changes';pid=$PID;priority=$self.PriorityClass.ToString();pointer_bytes=[IntPtr]::Size;open_struct_bytes=[Runtime.InteropServices.Marshal]::SizeOf([type][HexReadonlyGpuJoin+Open]);adapter_count=$cards.Count;rows=$rows;kmt_opened=$opened;kmt_closed=$closed;wrapper_closes_balanced=($opened -eq $closed);private_proof=$lab}
$json = $result | ConvertTo-Json -Depth 7 -Compress
[IO.File]::WriteAllText((Join-Path $lab 'result.json'),$json)
Write-Output ('HEX_WINDOWS_JOIN_READONLY '+$json)
if ($failed -or $rows.Count -eq 0 -or $opened -ne $closed) { exit 1 }
