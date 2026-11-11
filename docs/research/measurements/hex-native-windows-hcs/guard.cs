// Disposable caller guard. Processes enter the owned nested job at CreateProcess, before code runs.
// Microsoft STARTUPINFOEX PROC_THREAD_ATTRIBUTE_JOB_LIST; no inherited handles or breakaway rights.
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
public static class NativeWindowsGuard {
 [StructLayout(LayoutKind.Sequential)] struct Startup { public uint cb; public IntPtr reserved,desktop,title; public uint x,y,xsize,ysize,xchars,ychars,fill,flags; public ushort show,reserved2; public IntPtr reserved3,input,output,error; }
 [StructLayout(LayoutKind.Sequential)] struct StartupEx { public Startup start; public IntPtr attributes; }
 [StructLayout(LayoutKind.Sequential)] struct ProcessInfo { public IntPtr process,thread; public uint pid,tid; }
 [StructLayout(LayoutKind.Sequential)] struct BasicLimit { public long processTime,jobTime; public uint flags; public UIntPtr minWorking,maxWorking; public uint activeLimit; public UIntPtr affinity; public uint priority,scheduling; }
 [StructLayout(LayoutKind.Sequential)] struct IoCounters { public ulong readOps,writeOps,otherOps,readBytes,writeBytes,otherBytes; }
 [StructLayout(LayoutKind.Sequential)] struct ExtendedLimit { public BasicLimit basic; public IoCounters io; public UIntPtr processMemory,jobMemory,peakProcessMemory,peakJobMemory; }
 [StructLayout(LayoutKind.Sequential)] struct Completion { public IntPtr key,port; }
 [StructLayout(LayoutKind.Sequential)] struct Accounting { public long user,kernel,periodUser,periodKernel; public uint faults,total,active,terminated; }
 public class Result { public uint pid,exitCode,activeAfter,activeBeforeCleanup,processes,terminated; public bool residualsTerminated; public bool timedOut,reaped; public ulong peakJobMemory; public long user100ns,kernel100ns; }
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateJobObjectW(IntPtr security,string name);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool SetInformationJobObject(IntPtr job,int kind,IntPtr data,uint size);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr job,int kind,IntPtr data,uint size,IntPtr returned);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool InitializeProcThreadAttributeList(IntPtr list,int count,uint flags,ref UIntPtr size);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool UpdateProcThreadAttribute(IntPtr list,uint flags,IntPtr attribute,IntPtr value,UIntPtr bytes,IntPtr previous,IntPtr returned);
 [DllImport("kernel32.dll")] static extern void DeleteProcThreadAttributeList(IntPtr list);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateProcessW(string executable,StringBuilder command,IntPtr processSecurity,IntPtr threadSecurity,bool inherit,uint flags,IntPtr environment,string cwd,ref StartupEx startup,out ProcessInfo process);
 [DllImport("kernel32.dll",SetLastError=true)] static extern uint WaitForSingleObject(IntPtr handle,uint milliseconds);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr process,out uint code);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool TerminateJobObject(IntPtr job,uint code);
 [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr CreateIoCompletionPort(IntPtr file,IntPtr existing,IntPtr key,uint concurrency);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetQueuedCompletionStatus(IntPtr port,out uint message,out IntPtr key,out IntPtr context,uint milliseconds);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
 static void Check(bool ok) { if(!ok) throw new Win32Exception(Marshal.GetLastWin32Error()); }
 static T Query<T>(IntPtr job,int kind) { int size=Marshal.SizeOf(typeof(T));IntPtr data=Marshal.AllocHGlobal(size);try { Check(QueryInformationJobObject(job,kind,data,(uint)size,IntPtr.Zero));return (T)Marshal.PtrToStructure(data,typeof(T)); } finally { Marshal.FreeHGlobal(data); } }
 public static Result Run(string executable,string arguments,string cwd,string stdout,string stderr,uint milliseconds) { return RunCore(executable,arguments,cwd,stdout,stderr,milliseconds,false); }
 public static Result Build(string executable,string arguments,string cwd,string stdout,string stderr,uint milliseconds) { return RunCore(executable,arguments,cwd,stdout,stderr,milliseconds,true); }
 static Result RunCore(string executable,string arguments,string cwd,string stdout,string stderr,uint milliseconds,bool reapResiduals) {
  if(milliseconds==0 || milliseconds==uint.MaxValue) throw new ArgumentException("finite positive caller guard required");
  IntPtr job=CreateJobObjectW(IntPtr.Zero,null);if(job==IntPtr.Zero)throw new Win32Exception(Marshal.GetLastWin32Error());
  IntPtr attributes=IntPtr.Zero,jobValue=IntPtr.Zero,limits=IntPtr.Zero,port=IntPtr.Zero,association=IntPtr.Zero;ProcessInfo process=new ProcessInfo();bool initialized=false;
  try {
   port=CreateIoCompletionPort(new IntPtr(-1),IntPtr.Zero,IntPtr.Zero,1);if(port==IntPtr.Zero)throw new Win32Exception(Marshal.GetLastWin32Error());
   Completion completion=new Completion();completion.key=job;completion.port=port;int associationSize=Marshal.SizeOf(typeof(Completion));association=Marshal.AllocHGlobal(associationSize);Marshal.StructureToPtr(completion,association,false);Check(SetInformationJobObject(job,7,association,(uint)associationSize));
   ExtendedLimit limit=new ExtendedLimit();limit.basic.flags=0x2000|0x200|0x10;limit.basic.affinity=(UIntPtr)1;limit.jobMemory=(UIntPtr)(4UL*1024*1024*1024);
   int size=Marshal.SizeOf(typeof(ExtendedLimit));limits=Marshal.AllocHGlobal(size);Marshal.StructureToPtr(limit,limits,false);Check(SetInformationJobObject(job,9,limits,(uint)size));
   UIntPtr bytes=UIntPtr.Zero;InitializeProcThreadAttributeList(IntPtr.Zero,1,0,ref bytes);attributes=Marshal.AllocHGlobal((IntPtr)bytes.ToUInt64());Check(InitializeProcThreadAttributeList(attributes,1,0,ref bytes));initialized=true;
   jobValue=Marshal.AllocHGlobal(IntPtr.Size);Marshal.WriteIntPtr(jobValue,job);Check(UpdateProcThreadAttribute(attributes,0,(IntPtr)0x0002000D,jobValue,(UIntPtr)IntPtr.Size,IntPtr.Zero,IntPtr.Zero));
   StartupEx startup=new StartupEx();startup.start.cb=(uint)Marshal.SizeOf(typeof(StartupEx));startup.attributes=attributes;
   string command="cmd.exe /d /s /c \"\""+executable+"\" "+arguments+" 1>\""+stdout+"\" 2>\""+stderr+"\"\"";
   Check(CreateProcessW(Environment.GetEnvironmentVariable("SystemRoot")+"\\System32\\cmd.exe",new StringBuilder(command),IntPtr.Zero,IntPtr.Zero,false,0x08084000,IntPtr.Zero,cwd,ref startup,out process));
   Result result=new Result();result.pid=process.pid;Console.WriteLine("OWNED_PROCESS pid="+process.pid+" guard_ms="+milliseconds+" job_at_creation=true");
   Stopwatch elapsed=Stopwatch.StartNew();uint waited=WaitForSingleObject(process.process,milliseconds);result.timedOut=waited==258;
   if(waited!=0) { Check(TerminateJobObject(job,1460));result.reaped=WaitForSingleObject(process.process,5000)==0; }
   else result.reaped=true;
   Check(GetExitCodeProcess(process.process,out result.exitCode));
   // Root-process signal may precede the final accounting update/last child exit. Wait on the
   // kernel job notification under the same whole-phase budget, never on an invented sleep.
   result.activeBeforeCleanup=Query<Accounting>(job,1).active;
   if(reapResiduals && result.activeBeforeCleanup>0){Check(TerminateJobObject(job,0));result.residualsTerminated=true;}
   bool empty=false;
   while(elapsed.ElapsedMilliseconds < milliseconds) {
    uint message;IntPtr key,context;uint remaining=(uint)(milliseconds-elapsed.ElapsedMilliseconds);
    if(!GetQueuedCompletionStatus(port,out message,out key,out context,remaining))break;
    if(key==job && message==4){empty=true;break;}
   }
   if(!empty){result.timedOut=true;Check(TerminateJobObject(job,1460));}
   Accounting accounting=Query<Accounting>(job,1);result.activeAfter=accounting.active;result.processes=accounting.total;result.terminated=accounting.terminated;result.user100ns=accounting.user;result.kernel100ns=accounting.kernel;
   ExtendedLimit ended=Query<ExtendedLimit>(job,9);result.peakJobMemory=ended.peakJobMemory.ToUInt64();
   return result;
  } finally {
   if(process.thread!=IntPtr.Zero)Check(CloseHandle(process.thread));if(process.process!=IntPtr.Zero)Check(CloseHandle(process.process));
   if(initialized)DeleteProcThreadAttributeList(attributes);if(attributes!=IntPtr.Zero)Marshal.FreeHGlobal(attributes);if(jobValue!=IntPtr.Zero)Marshal.FreeHGlobal(jobValue);if(limits!=IntPtr.Zero)Marshal.FreeHGlobal(limits);Check(CloseHandle(job));if(port!=IntPtr.Zero)Check(CloseHandle(port));if(association!=IntPtr.Zero)Marshal.FreeHGlobal(association);
  }
 }
}
