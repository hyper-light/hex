$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$root='C:\Users\corph\.cache\hex-native-windows-hcs\path-probe-v1'
if(Test-Path -LiteralPath $root){throw 'owned path probe exists'}
[void][IO.Directory]::CreateDirectory($root)
[void][IO.Directory]::CreateDirectory(($root+'\temp'));$env:TEMP=$root+'\temp';$env:TMP=$env:TEMP
Add-Type -TypeDefinition @'
using System;using System.ComponentModel;using System.Runtime.InteropServices;
[assembly: DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
public static class NativeLongPathProbe {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern uint GetFileAttributesW(string path);
 public static string Attributes(string path){uint value=GetFileAttributesW(path);if(value==uint.MaxValue)return "ERROR_"+Marshal.GetLastWin32Error();return "0x"+value.ToString("x8");}
}
'@ -OutputAssembly ($root+'\probe.dll')
Add-Type -Path ($root+'\probe.dll')
$path='C:\Users\corph\.cache\hex-native-windows-hcs\full-image-prep-v2\root\io.containerd.snapshotter.v1.windows\snapshots\1\Files\Program Files\WindowsApps\Microsoft.DesktopAppInstaller_1.21.10121.0_neutral_split.scale-100_8wekyb3d8bbwe\microsoft.system.package.metadata'
$legacyError=$null;$legacyCount=$null
try{$legacyCount=@([IO.Directory]::EnumerateFileSystemEntries($path)).Count}catch{$legacyError=$_.ToString()}
$v=[ordered]@{path=$path;utf16_units=$path.Length;dotnet_legacy_error=$legacyError;dotnet_legacy_count=$legacyCount;normal_attributes=[NativeLongPathProbe]::Attributes($path);extended_attributes=[NativeLongPathProbe]::Attributes(('\\?\'+$path));binary_sha256=(Get-FileHash -LiteralPath ($root+'\probe.dll') -Algorithm SHA256).Hash.ToLowerInvariant();host_config_changes=0;utc=[DateTime]::UtcNow.ToString('o')}
$json=$v|ConvertTo-Json -Depth 10
[IO.File]::WriteAllText(($root+'\result.json'),$json,[Text.UTF8Encoding]::new($false));[Console]::WriteLine($json)
