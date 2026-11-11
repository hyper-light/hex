# Independent native API prerequisite control. This is not a Hex Runtime/Store/keeper or GPU pass.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$root='C:\Users\corph\.cache\hex-native-windows-hcs\native-cpu-callback-v3'
if(Test-Path -LiteralPath $root){throw 'owned native control root already exists'}
[void][IO.Directory]::CreateDirectory($root)
foreach($n in @('temp','scratch','output')){[void][IO.Directory]::CreateDirectory((Join-Path $root $n))}
$env:TEMP=$root+'\temp';$env:TMP=$env:TEMP
$utf8=[Text.UTF8Encoding]::new($false)
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Threading;
using System.Runtime.InteropServices;
[assembly: DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
public static class NativeHcsBaseline {
 [StructLayout(LayoutKind.Sequential)] public struct Driver { public int flavour;public IntPtr home; }
 [StructLayout(LayoutKind.Sequential)] public struct Layer { public Guid id;public uint flags;public IntPtr path; }
 [StructLayout(LayoutKind.Sequential)] public struct ProcessInfo { public uint pid,reserved;public IntPtr input,output,error; }
 [UnmanagedFunctionPointer(CallingConvention.Winapi)] public delegate void Completion(IntPtr operation,IntPtr context);
 [DllImport("computecore.dll",ExactSpelling=true)] public static extern IntPtr HcsCreateOperation(IntPtr context,Completion callback);
 [DllImport("computecore.dll",ExactSpelling=true)] public static extern void HcsCloseOperation(IntPtr operation);
 [DllImport("computecore.dll",ExactSpelling=true)] public static extern int HcsCancelOperation(IntPtr operation);
 [DllImport("computecore.dll",ExactSpelling=true)] public static extern int HcsWaitForOperationResult(IntPtr operation,uint milliseconds,out IntPtr result);
 [DllImport("computecore.dll",ExactSpelling=true)] public static extern int HcsWaitForOperationResultAndProcessInfo(IntPtr operation,uint milliseconds,out ProcessInfo info,out IntPtr result);
 [DllImport("computecore.dll",ExactSpelling=true)] public static extern int HcsCreateComputeSystem(IntPtr id,IntPtr config,IntPtr operation,IntPtr security,out IntPtr system);
 [DllImport("computecore.dll",CharSet=CharSet.Unicode,ExactSpelling=true)] public static extern int HcsOpenComputeSystem(string id,uint access,out IntPtr system);
 [DllImport("computecore.dll",ExactSpelling=true)] public static extern int HcsStartComputeSystem(IntPtr system,IntPtr operation,IntPtr options);
 [DllImport("computecore.dll",ExactSpelling=true)] public static extern int HcsTerminateComputeSystem(IntPtr system,IntPtr operation,IntPtr options);
 [DllImport("computecore.dll",ExactSpelling=true)] public static extern void HcsCloseComputeSystem(IntPtr system);
 [DllImport("computecore.dll",ExactSpelling=true)] public static extern int HcsCreateProcess(IntPtr system,IntPtr config,IntPtr operation,IntPtr security,out IntPtr process);
 [DllImport("computecore.dll",ExactSpelling=true)] public static extern void HcsCloseProcess(IntPtr process);
 [DllImport("vmcompute.dll",CharSet=CharSet.Unicode,ExactSpelling=true)] public static extern int ActivateLayer(ref Driver driver,string scratch);
 [DllImport("vmcompute.dll",CharSet=CharSet.Unicode,ExactSpelling=true)] public static extern int PrepareLayer(ref Driver driver,string scratch,[In] Layer[] layers,uint count);
 [DllImport("vmcompute.dll",CharSet=CharSet.Unicode,ExactSpelling=true)] public static extern int GetLayerMountPath(ref Driver driver,string scratch,ref UIntPtr count,IntPtr buffer);
 [DllImport("vmcompute.dll",CharSet=CharSet.Unicode,ExactSpelling=true)] public static extern int UnprepareLayer(ref Driver driver,string scratch);
 [DllImport("vmcompute.dll",CharSet=CharSet.Unicode,ExactSpelling=true)] public static extern int DeactivateLayer(ref Driver driver,string scratch);
 [DllImport("kernel32.dll",ExactSpelling=true)] public static extern IntPtr LocalFree(IntPtr memory);
 [DllImport("kernel32.dll",ExactSpelling=true,SetLastError=true)] public static extern bool CloseHandle(IntPtr handle);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr handle,out uint code);
 public static uint NativeExit(IntPtr handle){uint code;if(!GetExitCodeProcess(handle,out code))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());return code;}
 public static void Okay(int hr,string phase){if(hr<0)throw new IOException(phase+" HRESULT=0x"+unchecked((uint)hr).ToString("x8"));}
 public static bool Missing(int hr){return (unchecked((uint)hr)&0x0fffffffU)==0x0037010eU;}
 public static string Document(IntPtr value){if(value==IntPtr.Zero)return null;try{int size=0;while(size<4194304 && Marshal.ReadInt16(value,size*2)!=0)size++;if(size==4194304)throw new IOException("HCS result string exceeded existing source bound");return Marshal.PtrToStringUni(value,size);}finally{LocalFree(value);}}
 public static string Volume(ref Driver driver,string scratch){UIntPtr length=UIntPtr.Zero;Okay(GetLayerMountPath(ref driver,scratch,ref length,IntPtr.Zero),"mount path extent");ulong units=length.ToUInt64();if(units==0 || units>32767)throw new IOException("native volume path extent");IntPtr bytes=Marshal.AllocHGlobal(checked((int)units*2));try{Okay(GetLayerMountPath(ref driver,scratch,ref length,bytes),"mount path");return Marshal.PtrToStringUni(bytes);}finally{Marshal.FreeHGlobal(bytes);}}
 public class Operation {
  public IntPtr handle,id,config;public int calls;public bool immediatePending,closed;public int? cancelHr;public string resultDocument;public ProcessInfo processInfo;
  readonly ManualResetEvent completed=new ManualResetEvent(false);readonly Completion callback;GCHandle context;
  public Operation(string systemId,string document){id=systemId==null?IntPtr.Zero:Marshal.StringToHGlobalUni(systemId);config=document==null?IntPtr.Zero:Marshal.StringToHGlobalUni(document);context=GCHandle.Alloc(this);callback=OnComplete;handle=HcsCreateOperation(GCHandle.ToIntPtr(context),callback);if(handle==IntPtr.Zero){Release();throw new IOException("HcsCreateOperation returned null");}}
  static void OnComplete(IntPtr operation,IntPtr context){Operation owner=(Operation)GCHandle.FromIntPtr(context).Target;Interlocked.Increment(ref owner.calls);owner.completed.Set();}
  public int Finish(int milliseconds,bool process){immediatePending=!completed.WaitOne(0);if(!completed.WaitOne(milliseconds)){cancelHr=HcsCancelOperation(handle);throw new IOException("finite native callback deadline; cancellation request is not terminal ownership; retain context and storage");}IntPtr result;int hr;if(process){ProcessInfo info;hr=HcsWaitForOperationResultAndProcessInfo(handle,0,out info,out result);processInfo=info;}else hr=HcsWaitForOperationResult(handle,0,out result);resultDocument=Document(result);if(calls!=1)throw new IOException("native completion callback count differs from one");HcsCloseOperation(handle);handle=IntPtr.Zero;Release();closed=true;GC.KeepAlive(callback);return hr;}
  public void SynchronousRefusal(int hr){if(hr>=0)throw new IOException("not a synchronous refusal");HcsCloseOperation(handle);handle=IntPtr.Zero;Release();closed=true;}
  void Release(){if(id!=IntPtr.Zero){Marshal.FreeHGlobal(id);id=IntPtr.Zero;}if(config!=IntPtr.Zero){Marshal.FreeHGlobal(config);config=IntPtr.Zero;}if(context.IsAllocated)context.Free();completed.Dispose();}
 }
}
'@ -OutputAssembly ($root+'\native_driver.dll')
if($null -eq ('NativeHcsBaseline' -as [type])){Add-Type -Path ($root+'\native_driver.dll')}
$base='C:\Users\corph\.cache\hex-native-s4\ctrd\root\io.containerd.snapshotter.v1.windows\snapshots\1'
$layerId='eece0d3f-503c-878e-9d9f-2a024dddbe4a'
$scratch=$root+'\scratch'
$identifier='native-hcs-cpu-'+[Guid]::NewGuid().ToString('N')
$journal=[ordered]@{kind='native HCS API CPU/callback prerequisite control; NOT Hex Runtime/Store/GPU proof';root=$root;id=$identifier;driver_pid=$PID;needs='cpus=1,mem=4g,disk=12g';source_production_reference='2c8855b220a36c57685e456a7b8c98d8368f7899';native_budget_ms=5000;phases=@();state='planned';system_absent_before=$false;system_absent_after=$false;prepared=$false;activated=$false;cleanup_errors=@()}
$journal.driver_binary_sha256=(Get-FileHash -LiteralPath ($root+'\native_driver.dll') -Algorithm SHA256).Hash.ToLowerInvariant()
function Save(){
 $bytes=$utf8.GetBytes(($journal|ConvertTo-Json -Depth 20));$file=[IO.FileStream]::new(($root+'\journal.json'),[IO.FileMode]::Create,[IO.FileAccess]::Write,[IO.FileShare]::Read)
 try{$file.Write($bytes,0,$bytes.Length);$file.Flush($true)}finally{$file.Dispose()}
}
function Check([int]$hr,[string]$phase){$journal.phases+=@{phase=$phase;hr=('0x{0:x8}' -f [uint32]([int64]$hr -band 4294967295))};Save;[NativeHcsBaseline]::Okay($hr,$phase)}
function Complete($operation,[string]$phase,[bool]$process){$hr=$operation.Finish(5000,$process);$journal.phases+=@{phase=$phase;hr=('0x{0:x8}' -f [uint32]([int64]$hr -band 4294967295));immediate_pending=$operation.immediatePending;callbacks=$operation.calls;closed=$operation.closed;result_document=$operation.resultDocument};Save;[NativeHcsBaseline]::Okay($hr,$phase)}
$driver=[NativeHcsBaseline+Driver]::new();$driver.flavour=1;$driver.home=[Runtime.InteropServices.Marshal]::StringToHGlobalUni('')
$layer=[NativeHcsBaseline+Layer]::new();$layer.id=[Guid]$layerId;$layer.path=[Runtime.InteropServices.Marshal]::StringToHGlobalUni($base)
$system=[IntPtr]::Zero;$processHandle=[IntPtr]::Zero;$safeCleanup=$false;$original=$null
Save
try{
 $found=[IntPtr]::Zero;$hr=[NativeHcsBaseline]::HcsOpenComputeSystem($identifier,268435456,[ref]$found)
 if($found -ne [IntPtr]::Zero){[NativeHcsBaseline]::HcsCloseComputeSystem($found);throw 'fresh own ID unexpectedly exists; untouched'}
 if(-not [NativeHcsBaseline]::Missing($hr)){Check $hr 'preflight own ID';throw 'preflight did not prove absence'}
 $journal.system_absent_before=$true;$journal.base_blank_sha256=(Get-FileHash -LiteralPath ($base+'\blank.vhdx') -Algorithm SHA256).Hash.ToLowerInvariant();Save
 [IO.File]::Copy(($base+'\blank.vhdx'),($scratch+'\sandbox.vhdx'),$false)
 Check ([NativeHcsBaseline]::ActivateLayer([ref]$driver,$scratch)) 'activate own scratch';$journal.activated=$true;Save
 Check ([NativeHcsBaseline]::PrepareLayer([ref]$driver,$scratch,[NativeHcsBaseline+Layer[]]@($layer),1)) 'prepare own scratch';$journal.prepared=$true;Save
 $volume=[NativeHcsBaseline]::Volume([ref]$driver,$scratch);$journal.volume=$volume;Save
 $document=[ordered]@{Owner='hex-native-windows-hcs-baseline';SchemaVersion=@{Major=2;Minor=1};ShouldTerminateOnLastHandleClosed=$true;Container=@{Storage=@{Path=($volume.TrimEnd('\')+'\');Layers=@(@{Id=$layerId;Path=$base})};MappedDirectories=@(@{HostPath=($root+'\output');ContainerPath='C:\native-output';ReadOnly=$false});Memory=@{SizeInMB=512};Processor=@{Count=1}}}|ConvertTo-Json -Depth 12 -Compress
 [IO.File]::WriteAllText(($root+'\submitted-hcs.json'),$document,$utf8)
 $journal.document_sha256=(Get-FileHash -LiteralPath ($root+'\submitted-hcs.json') -Algorithm SHA256).Hash.ToLowerInvariant();$journal.state='durable-before-native-create';Save
 $create=[NativeHcsBaseline+Operation]::new($identifier,$document)
 $hr=[NativeHcsBaseline]::HcsCreateComputeSystem($create.id,$create.config,$create.handle,[IntPtr]::Zero,[ref]$system)
 if($hr -lt 0){$create.SynchronousRefusal($hr);Check $hr 'synchronous native create'}
 Complete $create 'actual native create callback' $false;$safeCleanup=$true;$journal.state='created';Save
 $start=[NativeHcsBaseline+Operation]::new($null,$null);Check ([NativeHcsBaseline]::HcsStartComputeSystem($system,$start.handle,[IntPtr]::Zero)) 'native start submitted';Complete $start 'native start callback' $false
 $processDoc=@{CommandLine='C:\Windows\System32\cmd.exe /d /c "set /p native_hold= & echo native-hcs-cpu-checked-output>C:\native-output\cpu.txt"';WorkingDirectory='C:\';CreateStdInPipe=$true;CreateStdOutPipe=$false;CreateStdErrPipe=$false}|ConvertTo-Json -Compress
 $procOp=[NativeHcsBaseline+Operation]::new($null,$processDoc);Check ([NativeHcsBaseline]::HcsCreateProcess($system,$procOp.config,$procOp.handle,[IntPtr]::Zero,[ref]$processHandle)) 'native CPU process submitted';Complete $procOp 'native CPU process callback' $true
 $pidOfChild=$procOp.processInfo.pid
 foreach($handle in @($procOp.processInfo.output,$procOp.processInfo.error)){if($handle -ne [IntPtr]::Zero -and -not [NativeHcsBaseline]::CloseHandle($handle)){throw 'owned returned stdio handle close failed'}}
 $child=[Diagnostics.Process]::GetProcessById($pidOfChild)
 try{$heldNativeProcess=$child.Handle;if($heldNativeProcess -eq [IntPtr]::Zero){throw 'native CPU child observation handle unavailable'};$journal.child=@{pid=$pidOfChild;birth=$child.StartTime.ToUniversalTime().ToString('o');held_on_private_stdin=$true};Save;
 $writeHandle=[Microsoft.Win32.SafeHandles.SafeFileHandle]::new($procOp.processInfo.input,$true);$inputStream=[IO.FileStream]::new($writeHandle,[IO.FileAccess]::Write);try{$release=[Text.Encoding]::ASCII.GetBytes("go`r`n");$inputStream.Write($release,0,$release.Length);$inputStream.Flush()}finally{$inputStream.Dispose();$writeHandle.Dispose()};
 if(-not $child.WaitForExit(5000)){throw 'exact native CPU child did not exit'};$journal.child.dotnet_exit=$child.ExitCode;$journal.child.exit=[NativeHcsBaseline]::NativeExit($heldNativeProcess);$journal.child.reaped=$true;if($journal.child.exit -ne 0){throw 'native CPU child failed'}}finally{$child.Dispose()}
 $actual=[IO.File]::ReadAllText(($root+'\output\cpu.txt'))
 if($actual.TrimEnd("`r","`n") -ne 'native-hcs-cpu-checked-output'){throw 'native CPU output differs'}
 $journal.output=$actual;$journal.state='actual-cpu-output-verified';Save
}catch{$original=$_;$journal.original_error=$_.ToString();Save}
finally{
 if($processHandle -ne [IntPtr]::Zero){[NativeHcsBaseline]::HcsCloseProcess($processHandle)}
 if($system -ne [IntPtr]::Zero -and $safeCleanup){try{$end=[NativeHcsBaseline+Operation]::new($null,$null);Check ([NativeHcsBaseline]::HcsTerminateComputeSystem($system,$end.handle,[IntPtr]::Zero)) 'terminate own system submitted';Complete $end 'terminate own system callback' $false}catch{$journal.cleanup_errors+=@($_.ToString())}}
 if($system -ne [IntPtr]::Zero){[NativeHcsBaseline]::HcsCloseComputeSystem($system)}
 $found=[IntPtr]::Zero;$hr=[NativeHcsBaseline]::HcsOpenComputeSystem($identifier,268435456,[ref]$found)
 if($found -ne [IntPtr]::Zero){[NativeHcsBaseline]::HcsCloseComputeSystem($found)}
 $journal.system_absent_after=([NativeHcsBaseline]::Missing($hr) -and $found -eq [IntPtr]::Zero);Save
 if($journal.system_absent_after){
  if($journal.prepared){try{$unprepare=[NativeHcsBaseline]::UnprepareLayer([ref]$driver,$scratch);if(([int64]$unprepare -band 4294967295) -eq 2149515280){$journal.phases+=@{phase='unprepare own scratch';hr='0x801f0010';meaning='existing Hex FilterHeld classification: container termination already removed filter; detachment still required'};Save}else{Check $unprepare 'unprepare own scratch';$journal.prepared=$false}}catch{$journal.cleanup_errors+=@($_.ToString())}}
  if($journal.activated){try{Check ([NativeHcsBaseline]::DeactivateLayer([ref]$driver,$scratch)) 'deactivate own scratch';$journal.activated=$false;$length=[UIntPtr]::Zero;$mountAfter=[NativeHcsBaseline]::GetLayerMountPath([ref]$driver,$scratch,[ref]$length,[IntPtr]::Zero);$journal.mount_path_after_detach_hr=('0x{0:x8}' -f [uint32]([int64]$mountAfter -band 4294967295));if($mountAfter -ge 0){throw 'owned scratch still has native mount path after deactivate'};$journal.prepared=$false;Save}catch{$journal.cleanup_errors+=@($_.ToString())}}
 }
 [Runtime.InteropServices.Marshal]::FreeHGlobal($layer.path);[Runtime.InteropServices.Marshal]::FreeHGlobal($driver.home)
 $journal.exit_code=if($null -eq $original -and $journal.system_absent_after -and -not $journal.prepared -and -not $journal.activated -and $journal.cleanup_errors.Count -eq 0){0}else{1};Save
 [Console]::WriteLine(($journal|ConvertTo-Json -Depth 20 -Compress))
}
if($null -ne $original){throw $original}
exit $journal.exit_code
