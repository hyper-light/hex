# Read-only PE import/image preflight, following Microsoft's PE/COFF format.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$exe='C:\Users\corph\.cache\hex-native-windows-hcs\hcsllvm1\target\debug\hex.exe'
$source='C:\Users\corph\.cache\hex-native-windows-hcs\hcsllvm1\source'
$root='C:\Users\corph\.cache\hex-native-windows-hcs\launcher-imports-v1'
if(Test-Path -LiteralPath $root){throw 'owned import receipt exists'}
[void][IO.Directory]::CreateDirectory($root)
$build='C:\Users\corph\.cache\hex-native-windows-hcs\hcsllvm1\build.stdout.log'
$artifacts=@(Get-Content -LiteralPath $build|ForEach-Object{$r=$_|ConvertFrom-Json;if($r.reason -eq 'compiler-artifact' -and $r.target.name -eq 'hex' -and $null -ne $r.executable){$r}})
if($artifacts.Count -ne 1 -or $artifacts[0].fresh -cne $false -or $artifacts[0].executable -ne $exe -or $artifacts[0].manifest_path -ne ($source+'\crates\hex-cli\Cargo.toml')){throw 'exact fresh CLI artifact proof differs'}
$file=[IO.File]::OpenRead($exe);$reader=[IO.BinaryReader]::new($file)
try{
 function Seek([long]$offset,[long]$count){if($offset -lt 0 -or $count -lt 0 -or $offset+$count -gt $file.Length){throw 'PE metadata escaped measured file extent'};$file.Position=$offset}
 Seek 0 64;if($reader.ReadUInt16() -ne 23117){throw 'not MZ'}
 Seek 60 4;$pe=$reader.ReadUInt32();Seek $pe 24;if($reader.ReadUInt32() -ne 17744 -or $reader.ReadUInt16() -ne 34404){throw 'not AMD64 PE'}
 $count=$reader.ReadUInt16();Seek ($pe+20) 2;$optional=$reader.ReadUInt16();$opt=$pe+24;Seek $opt $optional
 if($reader.ReadUInt16() -ne 523){throw 'not PE32+'}
 Seek ($opt+120) 8;$importRva=$reader.ReadUInt32();$importSize=$reader.ReadUInt32()
 $sections=@();for($i=0;$i -lt $count;$i++){Seek ($opt+$optional+$i*40+8) 16;$virtualSize=$reader.ReadUInt32();$rva=$reader.ReadUInt32();$rawSize=$reader.ReadUInt32();$raw=$reader.ReadUInt32();$sections+=@{rva=[long]$rva;size=[long]$rawSize;raw=[long]$raw}}
 function Offset([long]$rva){foreach($section in $sections){if($rva -ge $section.rva -and $rva -lt $section.rva+$section.size){return ($section.raw+$rva-$section.rva)}};throw 'PE RVA has no file-backed section'}
 $imports=@();$terminated=$false
 for($n=0;$n -lt [long][Math]::Floor($importSize/20);$n++){
  Seek ((Offset $importRva)+$n*20) 20;$lookup=$reader.ReadUInt32();$stamp=$reader.ReadUInt32();$forward=$reader.ReadUInt32();$name=$reader.ReadUInt32();$address=$reader.ReadUInt32()
  if($lookup -eq 0 -and $stamp -eq 0 -and $forward -eq 0 -and $name -eq 0 -and $address -eq 0){$terminated=$true;break}
  Seek (Offset $name) 1;$bytes=[Collections.Generic.List[byte]]::new();$ended=$false
  for($k=0;$k -lt 32767 -and $file.Position -lt $file.Length;$k++){$c=$reader.ReadByte();if($c -eq 0){$ended=$true;break};$bytes.Add($c)}
  if(-not $ended){throw 'DLL import name exceeded native path/file bound'}
  $dll=[Text.Encoding]::ASCII.GetString($bytes.ToArray());if($dll -match '[\\/:]'){throw 'DLL import is not an explicit basename'}
  $nano='C:\Users\corph\.cache\hex-native-s4\ctrd\root\io.containerd.snapshotter.v1.windows\snapshots\1\Files\Windows\System32\'+$dll
  $imports+=@{name=$dll;present_in_nano=(Test-Path -LiteralPath $nano);api_set_contract=($dll -like 'api-ms-*' -or $dll -like 'ext-ms-*')}
 }
 if(-not $terminated){throw 'PE import table not null terminated in its declared extent'}
}finally{$reader.Dispose();$file.Dispose()}
$v=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');source_revision='ed6d3e54db61fb8407418868ca086a479f2eeb5e';binary=$exe;binary_sha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant();bytes=(Get-Item -LiteralPath $exe).Length;artifact=$artifacts[0];imports=$imports;api_set_missing_files_are_not_loader_failures=$true;native_dispatches=0;production_cause='UNVERIFIED until actual image loader exit/behavior is captured'}
$json=$v|ConvertTo-Json -Depth 20
[IO.File]::WriteAllText(($root+'\result.json'),$json,[Text.UTF8Encoding]::new($false));[Console]::WriteLine($json)
