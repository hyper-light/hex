# Whole charged-root observation after the released import and new native cases.
# This is an operational allocation observation, not a filesystem quota or peak proof.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$base='C:\Users\corph\.cache\hex-native-windows-hcs';$root=$base+'\whole-envelope-meter-v3'
if(Test-Path -LiteralPath $root){throw 'owned post meter already exists'}
[void][IO.Directory]::CreateDirectory($root)
$meter=$base+'\retained-image-meter-v2\meter.dll'
if((Get-FileHash -LiteralPath $meter -Algorithm SHA256).Hash.ToLowerInvariant() -ne '05f9bd152a160bfe018aba2348c2bf900e25020a88edabd892af891f0019ac4f'){throw 'pinned allocation helper differs'}
Add-Type -Path $meter
$v=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');charged_root=$base;included_paths=@(Get-ChildItem -LiteralPath $base -Force|ForEach-Object{$_.FullName});envelope_bytes=34359738368L;allocation=$null;error=$null;host_free_bytes_before=[IO.DriveInfo]::new('C').AvailableFreeSpace;estimated_admission_is_not_a_quota=$true;peak_proven=$false;unattributed_metadata='extra named streams and volume-wide MFT/security/journal not attributed by FileStandardInfo';original_image_snapshots_retained=$true;imports=0;server_starts=0;native_gpu_dispatches=0;exit_code=1;policy_revision='0a6d91b3de0f10a8397868aa3794adab1e09afdc';rules_sha256='df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3'}
try{$v.allocation=[NativeRetainedImage]::Measure($base,1048576,120000);$v.measured_overrun=($v.allocation.allocatedBytes -gt $v.envelope_bytes);$v.observed_headroom=$v.envelope_bytes-$v.allocation.allocatedBytes;if(-not $v.measured_overrun){$v.exit_code=0}}catch{$v.error=$_.ToString()}
$v.host_free_bytes_after=[IO.DriveInfo]::new('C').AvailableFreeSpace;$v.finished_utc=[DateTime]::UtcNow.ToString('o')
$json=$v|ConvertTo-Json -Depth 12;[IO.File]::WriteAllText(($root+'\result.json'),$json,[Text.UTF8Encoding]::new($false));[Console]::WriteLine($json)
exit $v.exit_code
