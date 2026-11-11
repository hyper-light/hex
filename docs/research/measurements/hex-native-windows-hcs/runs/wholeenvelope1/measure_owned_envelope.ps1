# Read-only complete charged scope; allocation observation is not a future NTFS metadata bound.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$base='C:\Users\corph\.cache\hex-native-windows-hcs'
$root=$base+'\whole-envelope-meter-v1'
if(Test-Path -LiteralPath $root){throw 'owned envelope meter exists'}
[void][IO.Directory]::CreateDirectory($root)
$meter=$base+'\retained-image-meter-v2\meter.dll'
if((Get-FileHash -LiteralPath $meter -Algorithm SHA256).Hash.ToLowerInvariant() -ne '05f9bd152a160bfe018aba2348c2bf900e25020a88edabd892af891f0019ac4f'){throw 'pinned extended path allocation helper differs'}
Add-Type -Path $meter
$receipt=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');charged_root=$base;includes='ALL recursively retained owned preparation, source/target/cargo/temp, incoming archives, raw logs, controllers and inactive snapshot2 under charged_root';envelope_bytes=34359738368L;allocation=$null;error=$null;included_paths=@(Get-ChildItem -LiteralPath $base -Force|ForEach-Object{$_.FullName});remaining_layer_verified_tar_extent=5583841280L;tar_extent_is_not_ntfs_allocation_bound=$true;new_ntfs_metadata_growth_bound=$null;metadata_growth_enforced=$false;peak_bound_proven=$false;pre_mutation_decision='RefusedPeakUnproven';server_starts=0;import_calls=0;native_gpu_dispatches=0;unattributed_current_metadata='extra named streams and volume-wide MFT/security/journal metadata not attributed by FileStandardInfo';policy_current_revision='0a6d91b3de0f10a8397868aa3794adab1e09afdc';rules_sha256='df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3'}
try{
 $receipt.allocation=[NativeRetainedImage]::Measure($base,1048576,120000)
 $receipt.measured_current_allocation_lower_bound=$receipt.allocation.allocatedBytes
 $receipt.current_known_allocation_exceeds_envelope=($receipt.allocation.allocatedBytes -gt $receipt.envelope_bytes)
 $receipt.current_known_allocation_headroom=$receipt.envelope_bytes-$receipt.allocation.allocatedBytes
}catch{$receipt.error=$_.ToString()}
$receipt.finished_utc=[DateTime]::UtcNow.ToString('o')
$json=$receipt|ConvertTo-Json -Depth 20
[IO.File]::WriteAllText(($root+'\result.json'),$json,[Text.UTF8Encoding]::new($false));[Console]::WriteLine($json)
