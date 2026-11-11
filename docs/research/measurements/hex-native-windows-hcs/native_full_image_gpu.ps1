# Independent native API prerequisite control. Its result does not establish Hex Runtime/Store/keeper/recovery correctness.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$root='C:\Users\corph\.cache\hex-native-windows-hcs\native-full-image-gpu-v1'
if(Test-Path -LiteralPath $root){throw 'owned native control root already exists'}
[void][IO.Directory]::CreateDirectory($root)
foreach($n in @('temp','scratch','output')){[void][IO.Directory]::CreateDirectory((Join-Path $root $n))}
$env:TEMP=$root+'\temp';$env:TMP=$env:TEMP
$utf8=[Text.UTF8Encoding]::new($false)
$helper='C:\Users\corph\.cache\hex-native-windows-hcs\native-cpu-callback-v3\native_driver.dll'
if((Get-FileHash -LiteralPath $helper -Algorithm SHA256).Hash.ToLowerInvariant() -ne 'faba83ca93d31489e026a35edc60bcbd34cb799ebd11dc9d3cfc88cfaaa272cd'){throw 'existing native callback helper changed'}
Add-Type -Path $helper
$base='C:\Users\corph\.cache\hex-native-windows-hcs\full-image-prep-v2\root\io.containerd.snapshotter.v1.windows\snapshots\1'
$top=$base.Substring(0,$base.Length-1)+'3'
$topId='03596187-311f-868e-a2d7-c81ec876b600'
$layerId='03596187-331f-868e-a2d7-c81ec876b876'
# These layer IDs are the fixed core/hcs.rs layer_guid FNV metadata, never hardware GUIDs.
if((Get-FileHash -LiteralPath ($base+'\blank.vhdx') -Algorithm SHA256).Hash.ToLowerInvariant() -ne 'ea36b14a9e2ea129b5fd7c3e5af570b10f67501f9190bd6a7825666ad79bcfa0'){throw 'original base blank changed'}
$scratch=$root+'\scratch'
$identifier='native-hcs-full-image-gpu-'+[Guid]::NewGuid().ToString('N')
$journal=[ordered]@{kind='independent native HCS exact DeviceInstance GPU execution; NOT Hex Runtime/Store/recovery proof';root=$root;id=$identifier;driver_pid=$PID;driver_birth=[Diagnostics.Process]::GetCurrentProcess().StartTime.ToUniversalTime().ToFileTimeUtc().ToString();actual_gpu_output_verified=$false;needs='cpus=1,mem=4g,disk=12g';source_production_reference='2c8855b220a36c57685e456a7b8c98d8368f7899';native_budget_ms=5000;phases=@();state='planned';system_absent_before=$false;system_absent_after=$false;prepared=$false;activated=$false;cleanup_errors=@()}
$journal.driver_binary_sha256=(Get-FileHash -LiteralPath $helper -Algorithm SHA256).Hash.ToLowerInvariant()
function Save(){
 $bytes=$utf8.GetBytes(($journal|ConvertTo-Json -Depth 20));$file=[IO.FileStream]::new(($root+'\journal.json'),[IO.FileMode]::Create,[IO.FileAccess]::Write,[IO.FileShare]::Read)
 try{$file.Write($bytes,0,$bytes.Length);$file.Flush($true)}finally{$file.Dispose()}
}
function Check([int]$hr,[string]$phase){$journal.phases+=@{phase=$phase;hr=('0x{0:x8}' -f [uint32]([int64]$hr -band 4294967295))};Save;[NativeHcsBaseline]::Okay($hr,$phase)}
function Complete($operation,[string]$phase,[bool]$process){$hr=$operation.Finish(5000,$process);$journal.phases+=@{phase=$phase;hr=('0x{0:x8}' -f [uint32]([int64]$hr -band 4294967295));immediate_pending=$operation.immediatePending;callbacks=$operation.calls;closed=$operation.closed;result_document=$operation.resultDocument};Save;[NativeHcsBaseline]::Okay($hr,$phase)}
$receiptPath='C:\Users\corph\.cache\hex-native-windows-hcs\post-import-verify-v4\result.json'
if((Get-FileHash -LiteralPath $receiptPath -Algorithm SHA256).Hash.ToLowerInvariant() -ne '7ea4fec0d58399004a02737b2b47e59bf32ece71fbf44c0f670d084601d2e57d'){throw 'verified full image receipt changed'}
$image=ConvertFrom-Json ([IO.File]::ReadAllText($receiptPath))
if($image.exit_code -ne 0 -or -not $image.verified_for_execution -or $image.layers.Count -ne 2 -or $image.layers[0] -ne $top -or $image.layers[1] -ne $base){throw 'full image native parents not established'}
$journal.image_receipt_sha256='7ea4fec0d58399004a02737b2b47e59bf32ece71fbf44c0f670d084601d2e57d';$journal.image_reference=$image.reference;$journal.layer_inputs=$image.layers;$journal.required_dlls=$image.required_dlls
$compute='C:\Users\corph\.cache\hex-native-windows-hcs\hostv3\completion_windows_compute.exe'
if((Get-FileHash -LiteralPath $compute -Algorithm SHA256).Hash.ToLowerInvariant() -ne '4fdd12df17fbf9e97b8be3ea68fb22344294cbe2f71c8f36de14c8a94034f6e9'){throw 'pinned actual compute witness changed'}
if((Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o') -ne '2026-11-11T03:04:32.5000000Z'){throw 'boot changed; current native join required'}
$driver=[NativeHcsBaseline+Driver]::new();$driver.flavour=1;$driver.home=[Runtime.InteropServices.Marshal]::StringToHGlobalUni('')
$layer=[NativeHcsBaseline+Layer]::new();$layer.id=[Guid]$layerId;$layer.path=[Runtime.InteropServices.Marshal]::StringToHGlobalUni($base)
$topLayer=[NativeHcsBaseline+Layer]::new();$topLayer.id=[Guid]$topId;$topLayer.path=[Runtime.InteropServices.Marshal]::StringToHGlobalUni($top)
$system=[IntPtr]::Zero;$processHandle=[IntPtr]::Zero;$safeCleanup=$false;$original=$null
Save
try{
 $found=[IntPtr]::Zero;$hr=[NativeHcsBaseline]::HcsOpenComputeSystem($identifier,268435456,[ref]$found)
 if($found -ne [IntPtr]::Zero){[NativeHcsBaseline]::HcsCloseComputeSystem($found);throw 'fresh own ID unexpectedly exists; untouched'}
 if(-not [NativeHcsBaseline]::Missing($hr)){Check $hr 'preflight own ID';throw 'preflight did not prove absence'}
 $journal.system_absent_before=$true;$journal.base_blank_sha256=(Get-FileHash -LiteralPath ($base+'\blank.vhdx') -Algorithm SHA256).Hash.ToLowerInvariant();Save
 [IO.File]::Copy(($base+'\blank.vhdx'),($scratch+'\sandbox.vhdx'),$false)
 Check ([NativeHcsBaseline]::ActivateLayer([ref]$driver,$scratch)) 'activate own scratch';$journal.activated=$true;Save
 Check ([NativeHcsBaseline]::PrepareLayer([ref]$driver,$scratch,[NativeHcsBaseline+Layer[]]@($topLayer,$layer),2)) 'prepare own scratch';$journal.prepared=$true;Save
 $volume=[NativeHcsBaseline]::Volume([ref]$driver,$scratch);$journal.volume=$volume;Save
 $document=[ordered]@{Owner='hex-native-windows-hcs-baseline';SchemaVersion=@{Major=2;Minor=2};ShouldTerminateOnLastHandleClosed=$true;Container=@{AssignedDevices=@(@{Type='DeviceInstance';LocationPath='PCIROOT(0)#PCI(0801)#PCI(0000)'});Storage=@{Path=($volume.TrimEnd('\')+'\');Layers=@(@{Id=$topId;Path=$top},@{Id=$layerId;Path=$base})};MappedDirectories=@(@{HostPath=($root+'\output');ContainerPath='C:\native-output';ReadOnly=$false},@{HostPath=([IO.Path]::GetDirectoryName($compute));ContainerPath='C:\device-probe';ReadOnly=$true});Memory=@{SizeInMB=512};Processor=@{Count=1}}}|ConvertTo-Json -Depth 12 -Compress
 [IO.File]::WriteAllText(($root+'\submitted-hcs.json'),$document,$utf8)
 $journal.document_sha256=(Get-FileHash -LiteralPath ($root+'\submitted-hcs.json') -Algorithm SHA256).Hash.ToLowerInvariant();$journal.state='durable-before-native-create';Save
 $create=[NativeHcsBaseline+Operation]::new($identifier,$document)
 $hr=[NativeHcsBaseline]::HcsCreateComputeSystem($create.id,$create.config,$create.handle,[IntPtr]::Zero,[ref]$system)
 if($hr -lt 0){$create.SynchronousRefusal($hr);Check $hr 'synchronous native create'}
 Complete $create 'actual native create callback' $false;$safeCleanup=$true;$journal.state='created';Save
 $start=[NativeHcsBaseline+Operation]::new($null,$null);$hr=[NativeHcsBaseline]::HcsStartComputeSystem($system,$start.handle,[IntPtr]::Zero);if($hr -lt 0){$start.SynchronousRefusal($hr)};Check $hr 'native start submitted';Complete $start 'native start callback' $false
 $processDoc=@{CommandLine='C:\Windows\System32\cmd.exe /d /v:on /c "set /p native_hold= & C:\device-probe\completion_windows_compute.exe --ignored --exact root_windows_exact_adapter_executes_checked_kernel --nocapture --test-threads=1 >C:\native-output\gpu.log 2>&1 & echo !errorlevel!>C:\native-output\gpu.exit.txt"';Environment=@{SystemRoot='C:\Windows';WINDIR='C:\Windows';PATH='C:\Windows\System32';TEMP='C:\Windows\Temp';TMP='C:\Windows\Temp';HEX_WINDOWS_GPU_COMPUTE_INTERFACE='\\?\PCI#VEN_1002&DEV_1586&SUBSYS_1FB31043&REV_C1#4&35fe04f8&0&0041#{1ca05180-a699-450a-9a0c-de4fbe3ddd89}';HEX_WINDOWS_GPU_COMPUTE_LUID='0000000000014c4d';HEX_WINDOWS_GPU_COMPUTE_WAIT_MS='5000';RUST_TEST_THREADS='1'};WorkingDirectory='C:\';CreateStdInPipe=$true;CreateStdOutPipe=$false;CreateStdErrPipe=$false}|ConvertTo-Json -Depth 8 -Compress
 $journal.compute_binary_sha256='4fdd12df17fbf9e97b8be3ea68fb22344294cbe2f71c8f36de14c8a94034f6e9';$journal.compute_process_document=$processDoc;Save
 $procOp=[NativeHcsBaseline+Operation]::new($null,$processDoc);$hr=[NativeHcsBaseline]::HcsCreateProcess($system,$procOp.config,$procOp.handle,[IntPtr]::Zero,[ref]$processHandle);if($hr -lt 0){$procOp.SynchronousRefusal($hr)};Check $hr 'native GPU process submitted';Complete $procOp 'native GPU process callback' $true
 $pidOfChild=$procOp.processInfo.pid
 foreach($handle in @($procOp.processInfo.output,$procOp.processInfo.error)){if($handle -ne [IntPtr]::Zero -and -not [NativeHcsBaseline]::CloseHandle($handle)){throw 'owned returned stdio handle close failed'}}
 $child=[Diagnostics.Process]::GetProcessById($pidOfChild)
 try{$heldNativeProcess=$child.Handle;if($heldNativeProcess -eq [IntPtr]::Zero){throw 'native GPU wrapper child observation handle unavailable'};$journal.child=@{pid=$pidOfChild;birth=$child.StartTime.ToUniversalTime().ToString('o');held_on_private_stdin=$true};Save;
 $writeHandle=[Microsoft.Win32.SafeHandles.SafeFileHandle]::new($procOp.processInfo.input,$true);$inputStream=[IO.FileStream]::new($writeHandle,[IO.FileAccess]::Write);try{$release=[Text.Encoding]::ASCII.GetBytes("go`r`n");$inputStream.Write($release,0,$release.Length);$inputStream.Flush()}finally{$inputStream.Dispose();$writeHandle.Dispose()};
 if(-not $child.WaitForExit(5000)){throw 'exact native GPU wrapper child did not exit'};$journal.child.dotnet_exit=$child.ExitCode;$journal.child.exit=[NativeHcsBaseline]::NativeExit($heldNativeProcess);$journal.child.reaped=$true;if($journal.child.exit -ne 0){throw 'native GPU wrapper child failed'}}finally{$child.Dispose()}
 $actual=[IO.File]::ReadAllText(($root+'\output\gpu.log'))
 $gpuExit=[IO.File]::ReadAllText(($root+'\output\gpu.exit.txt')).Trim()
 $journal.gpu_child_exit=$gpuExit;$journal.gpu_output_sha256=(Get-FileHash -LiteralPath ($root+'\output\gpu.log') -Algorithm SHA256).Hash.ToLowerInvariant();$journal.gpu_output=$actual;Save
 if($gpuExit -ne '0' -or -not $actual.Contains('windows_compute_verified luid=0000000000014c4d elements=1024 mismatches=0') -or -not $actual.Contains('windows_compute_fence wait=0x0 value=1 device=Ok(())') -or -not $actual.Contains('windows_compute_handles kmt=1/1 events=1/1 close_failed=false')){throw 'actual HCS GPU exit/oracle/fence/close witness incomplete'}
 $journal.actual_gpu_output_verified=$true;$journal.state='actual-gpu-output-verified';Save
}catch{$original=$_;$journal.original_error=$_.ToString();Save}
finally{
 if($processHandle -ne [IntPtr]::Zero){[NativeHcsBaseline]::HcsCloseProcess($processHandle)}
 if($system -ne [IntPtr]::Zero -and $safeCleanup){try{$end=[NativeHcsBaseline+Operation]::new($null,$null);$hr=[NativeHcsBaseline]::HcsTerminateComputeSystem($system,$end.handle,[IntPtr]::Zero);if($hr -lt 0){$end.SynchronousRefusal($hr)};Check $hr 'terminate own system submitted';Complete $end 'terminate own system callback' $false}catch{$journal.cleanup_errors+=@($_.ToString())}}
 if($system -ne [IntPtr]::Zero){[NativeHcsBaseline]::HcsCloseComputeSystem($system)}
 $found=[IntPtr]::Zero;$hr=[NativeHcsBaseline]::HcsOpenComputeSystem($identifier,268435456,[ref]$found)
 if($found -ne [IntPtr]::Zero){[NativeHcsBaseline]::HcsCloseComputeSystem($found)}
 $journal.system_absent_after=([NativeHcsBaseline]::Missing($hr) -and $found -eq [IntPtr]::Zero);Save
 if($journal.system_absent_after){
  if($journal.prepared){try{$unprepare=[NativeHcsBaseline]::UnprepareLayer([ref]$driver,$scratch);if(([int64]$unprepare -band 4294967295) -eq 2149515280){$journal.phases+=@{phase='unprepare own scratch';hr='0x801f0010';meaning='existing Hex FilterHeld classification: container termination already removed filter; detachment still required'};Save}else{Check $unprepare 'unprepare own scratch';$journal.prepared=$false}}catch{$journal.cleanup_errors+=@($_.ToString())}}
  if($journal.activated){try{Check ([NativeHcsBaseline]::DeactivateLayer([ref]$driver,$scratch)) 'deactivate own scratch';$journal.activated=$false;$length=[UIntPtr]::Zero;$mountAfter=[NativeHcsBaseline]::GetLayerMountPath([ref]$driver,$scratch,[ref]$length,[IntPtr]::Zero);$journal.mount_path_after_detach_hr=('0x{0:x8}' -f [uint32]([int64]$mountAfter -band 4294967295));if($mountAfter -ge 0){throw 'owned scratch still has native mount path after deactivate'};$journal.prepared=$false;Save}catch{$journal.cleanup_errors+=@($_.ToString())}}
 }
 [Runtime.InteropServices.Marshal]::FreeHGlobal($topLayer.path);[Runtime.InteropServices.Marshal]::FreeHGlobal($layer.path);[Runtime.InteropServices.Marshal]::FreeHGlobal($driver.home)
 $journal.exit_code=if($null -eq $original -and $journal.system_absent_after -and -not $journal.prepared -and -not $journal.activated -and $journal.cleanup_errors.Count -eq 0){0}else{1};Save
 [Console]::WriteLine(($journal|ConvertTo-Json -Depth 20 -Compress))
}
if($null -ne $original){throw $original}
exit $journal.exit_code
