# What a native Windows box bounds with a job object (docs/research/runtime/native-platforms.md §2, §7). Windows
# PowerShell 5.1, no compiler: the C# below is compiled by Add-Type. Run: powershell -NoProfile -File job_probe.ps1
#   create  a box's cost: a job with its limits, a program started suspended into it and run to its end, against the
#           same program started with no job (cmd.exe /c exit 0), 200 each, alternating
#   memory  a member committing 1 MiB at a time into a job capped at 256 MiB: where its commits are refused, and the
#           job's peak
#   tree    a member that starts a grandchild, then the job's last handle closed: whether both end
#   escape  a member asking for CREATE_BREAKAWAY_FROM_JOB: whether Windows lets it leave
param([int]$N = 200)
$ErrorActionPreference = 'Stop'
$dir = Join-Path $env:USERPROFILE '.cache\hexnative'
New-Item -ItemType Directory -Force -Path $dir | Out-Null

$native = @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class W {
    [StructLayout(LayoutKind.Sequential)] public struct BASIC {
        public long PerProcessUserTimeLimit, PerJobUserTimeLimit; public uint LimitFlags;
        public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize; public uint ActiveProcessLimit;
        public UIntPtr Affinity; public uint PriorityClass, SchedulingClass;
    }
    [StructLayout(LayoutKind.Sequential)] public struct IO { public ulong R, W, O, RB, WB, OB; }
    [StructLayout(LayoutKind.Sequential)] public struct EXTENDED {
        public BASIC Basic; public IO Io; public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed;
    }
    [StructLayout(LayoutKind.Sequential)] public struct CPU { public uint ControlFlags, CpuRate; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] public struct SI {
        public int cb; public string r, d, t; public int x, y, xs, ys, xc, yc, fill, flags; public short show, r2;
        public IntPtr r3, i, o, e;
    }
    [StructLayout(LayoutKind.Sequential)] public struct PI { public IntPtr Process, Thread; public uint Pid, Tid; }

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] public static extern IntPtr CreateJobObjectW(IntPtr a, string n);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern bool SetInformationJobObject(IntPtr j, int c, ref EXTENDED i, uint l);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern bool SetInformationJobObject(IntPtr j, int c, ref CPU i, uint l);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern bool QueryInformationJobObject(IntPtr j, int c, ref EXTENDED i, uint l, IntPtr r);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern bool AssignProcessToJobObject(IntPtr j, IntPtr p);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern uint ResumeThread(IntPtr t);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern uint WaitForSingleObject(IntPtr h, uint ms);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern IntPtr OpenProcess(uint a, bool i, uint pid);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern bool GetExitCodeProcess(IntPtr h, out uint c);
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    public static extern bool CreateProcessW(string app, StringBuilder cmd, IntPtr pa, IntPtr ta, bool inherit, uint flags,
                                             IntPtr env, string cwd, ref SI si, out PI pi);

    public const uint SUSPENDED = 0x4, NO_WINDOW = 0x08000000, BREAKAWAY = 0x01000000;
    public const uint ACTIVE_PROCESS = 0x8, JOB_MEMORY = 0x200, KILL_ON_CLOSE = 0x2000;

    // A process from a command line, with the given creation flags; throws with the Win32 error if refused.
    public static PI Start(string cmd, uint flags) {
        SI si = new SI(); si.cb = Marshal.SizeOf(typeof(SI)); PI pi;
        if (!CreateProcessW(null, new StringBuilder(cmd), IntPtr.Zero, IntPtr.Zero, false, flags | NO_WINDOW, IntPtr.Zero, null, ref si, out pi))
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        return pi;
    }

    // A box's job: memory capped for the whole job, a process count, its tree ended with its last handle, CPU hard-capped.
    public static IntPtr Job(ulong memory, uint processes, uint cpuRateHundredthsOfPercent) {
        IntPtr j = CreateJobObjectW(IntPtr.Zero, null);
        if (j == IntPtr.Zero) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        EXTENDED x = new EXTENDED();
        x.Basic.LimitFlags = JOB_MEMORY | ACTIVE_PROCESS | KILL_ON_CLOSE;
        x.Basic.ActiveProcessLimit = processes; x.JobMemoryLimit = new UIntPtr(memory);
        if (!SetInformationJobObject(j, 9, ref x, (uint)Marshal.SizeOf(typeof(EXTENDED))))
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        CPU c = new CPU(); c.ControlFlags = 0x1 | 0x4; c.CpuRate = cpuRateHundredthsOfPercent;
        if (!SetInformationJobObject(j, 15, ref c, (uint)Marshal.SizeOf(typeof(CPU))))
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        return j;
    }

    public static EXTENDED Query(IntPtr j) {
        EXTENDED x = new EXTENDED();
        if (!QueryInformationJobObject(j, 9, ref x, (uint)Marshal.SizeOf(typeof(EXTENDED)), IntPtr.Zero))
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        return x;
    }

    public static bool Alive(uint pid) {
        IntPtr h = OpenProcess(0x1000 | 0x00100000, false, pid);  // QUERY_LIMITED_INFORMATION | SYNCHRONIZE
        if (h == IntPtr.Zero) return false;
        bool alive = WaitForSingleObject(h, 0) == 0x102;  // WAIT_TIMEOUT: still running
        CloseHandle(h);
        return alive;
    }
}
'@

# The member: commits memory until refused, tries to break away, or starts a grandchild and waits.
# Its usings are $native's, which comes first when the two are compiled together.
$member = @'
public static class M {
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr VirtualAlloc(IntPtr a, UIntPtr s, uint t, uint p);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool VirtualFree(IntPtr a, UIntPtr s, uint t);
    public static int Main(string[] a) {
        if (a[0] == "alloc") {
            IntPtr[] got = new IntPtr[65536];
            int mib = 0;
            for (;;) {
                IntPtr p = VirtualAlloc(IntPtr.Zero, new UIntPtr(1u << 20), 0x3000, 0x04);  // MEM_COMMIT|MEM_RESERVE, RW
                if (p == IntPtr.Zero) {
                    int err = Marshal.GetLastWin32Error();
                    // Gives back 16 MiB before writing, so the write itself has room.
                    for (int k = 1; k <= 16 && mib - k >= 0; k++) VirtualFree(got[mib - k], UIntPtr.Zero, 0x8000);
                    Console.WriteLine("refused_after_mib=" + mib + " error=" + err);
                    return 0;
                }
                for (int i = 0; i < (1 << 20); i += 4096) Marshal.WriteByte(p, i, 1);
                got[mib++] = p;
                if (mib >= got.Length) { Console.WriteLine("never_refused"); return 1; }
            }
        }
        if (a[0] == "breakaway") {
            try { W.Start("cmd.exe /c exit 0", W.BREAKAWAY); Console.WriteLine("breakaway=allowed"); }
            catch (System.ComponentModel.Win32Exception e) { Console.WriteLine("breakaway=refused error=" + e.NativeErrorCode); }
            return 0;
        }
        if (a[0] == "grandchild") {
            W.PI g = W.Start("ping -n 60 127.0.0.1", 0);
            Console.WriteLine("grandchild=" + g.Pid);
            Console.Out.Flush();
            System.Threading.Thread.Sleep(60000);
            return 0;
        }
        return 2;
    }
}
'@

Add-Type -TypeDefinition $native
$exe = Join-Path $dir 'hex_member.exe'
Add-Type -TypeDefinition ($native + $member) -OutputAssembly $exe -OutputType ConsoleApplication

function Pct($v, $p) { $s = $v | Sort-Object; $s[[Math]::Min($s.Count - 1, [int][Math]::Floor($s.Count * $p / 100))] }
$sw = [Diagnostics.Stopwatch]::new()
$os = Get-CimInstance Win32_OperatingSystem
"host $env:COMPUTERNAME | $($os.Caption) $($os.Version) | $((Get-CimInstance Win32_Processor | Select-Object -First 1).Name) | $(Get-Date -Format o)"

# create: a box against a bare start, alternating
$box = New-Object 'System.Collections.Generic.List[double]'; $bare = New-Object 'System.Collections.Generic.List[double]'
for ($i = 0; $i -lt $N; $i++) {
    $sw.Restart()
    $pi = [W]::Start('cmd.exe /c exit 0', 0)
    [W]::WaitForSingleObject($pi.Process, [uint32]::MaxValue) | Out-Null
    $sw.Stop(); $bare.Add($sw.Elapsed.TotalMilliseconds)
    [W]::CloseHandle($pi.Process) | Out-Null; [W]::CloseHandle($pi.Thread) | Out-Null

    $sw.Restart()
    $j = [W]::Job(256MB, 64, 5000)
    $pi = [W]::Start('cmd.exe /c exit 0', [W]::SUSPENDED)
    if (-not [W]::AssignProcessToJobObject($j, $pi.Process)) { throw "assign refused" }
    [W]::ResumeThread($pi.Thread) | Out-Null
    [W]::WaitForSingleObject($pi.Process, [uint32]::MaxValue) | Out-Null
    [W]::CloseHandle($j) | Out-Null
    $sw.Stop(); $box.Add($sw.Elapsed.TotalMilliseconds)
    [W]::CloseHandle($pi.Process) | Out-Null; [W]::CloseHandle($pi.Thread) | Out-Null
}
"create n=$N box_ms p50={0:N3} p99={1:N3} max={2:N3} | bare_ms p50={3:N3} p99={4:N3} max={5:N3}" -f (Pct $box 50), (Pct $box 99), (Pct $box 100), (Pct $bare 50), (Pct $bare 99), (Pct $bare 100)

# memory: commits refused at the job's cap
$j = [W]::Job(256MB, 64, 10000)
$out = Join-Path $dir 'alloc.txt'
$pi = [W]::Start("cmd.exe /c `"`"$exe`" alloc > `"$out`"`"", [W]::SUSPENDED)
[W]::AssignProcessToJobObject($j, $pi.Process) | Out-Null
[W]::ResumeThread($pi.Thread) | Out-Null
[W]::WaitForSingleObject($pi.Process, 60000) | Out-Null
$x = [W]::Query($j)
"memory job_limit_bytes={0} peak_job_bytes={1} over_bytes={2} peak_process_bytes={3} member: {4}" -f 256MB, $x.PeakJobMemoryUsed.ToUInt64(), ([int64]$x.PeakJobMemoryUsed.ToUInt64() - 256MB), $x.PeakProcessMemoryUsed.ToUInt64(), ((Get-Content $out) -join ' ')
[W]::CloseHandle($j) | Out-Null

# tree: a grandchild, then the job's last handle closed
$j = [W]::Job(256MB, 64, 10000)
$out = Join-Path $dir 'grandchild.txt'
$pi = [W]::Start("cmd.exe /c `"`"$exe`" grandchild > `"$out`"`"", [W]::SUSPENDED)
[W]::AssignProcessToJobObject($j, $pi.Process) | Out-Null
[W]::ResumeThread($pi.Thread) | Out-Null
$g = $null
for ($t = 0; $t -lt 100 -and -not $g; $t++) { Start-Sleep -Milliseconds 100; if (Test-Path $out) { $g = (Get-Content $out | Select-String 'grandchild=(\d+)').Matches.Groups[1].Value } }
$before = [W]::Alive([uint32]$g)
[W]::CloseHandle($j) | Out-Null
Start-Sleep -Milliseconds 500
"tree grandchild=$g alive_before_close=$before alive_after_close=$([W]::Alive([uint32]$g)) member_alive_after_close=$([W]::Alive($pi.Pid))"

# escape: CREATE_BREAKAWAY_FROM_JOB from inside a job that doesn't allow it
$j = [W]::Job(256MB, 64, 10000)
$out = Join-Path $dir 'breakaway.txt'
$pi = [W]::Start("cmd.exe /c `"`"$exe`" breakaway > `"$out`"`"", [W]::SUSPENDED)
[W]::AssignProcessToJobObject($j, $pi.Process) | Out-Null
[W]::ResumeThread($pi.Thread) | Out-Null
[W]::WaitForSingleObject($pi.Process, 30000) | Out-Null
"escape $((Get-Content $out) -join ' ')"
[W]::CloseHandle($j) | Out-Null
