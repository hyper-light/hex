# The `process` class's competitors on macOS and Windows, class for class

The question (native plan step 7, CLAUDE.md §7): what does each OS ship, or what is the best free tool,
that starts a program in a bounded group of processes, ends it, and bounds its memory, so Hex's macOS and
Windows `process` classes are benchmarked against named competitors on the same machine. Linux's are
systemd's transient scopes and services (`bench/competitive/README.md`). Sources read 2026-11-05; the
probes were single runs on adas-mac-studio-3 (macOS 26.2, 25C56), not measurements.

## macOS

- **`taskpolicy -m <MiB>`** (built in). `man taskpolicy` (macOS 26.2): "-m limit Run the program with
  specified memory limit (in MiB)." system_cmds-1042.120.1 `taskpolicy/taskpolicy.c:269` sets
  `POSIX_SPAWN_JETSAM_MEMLIMIT_ACTIVE_FATAL | POSIX_SPAWN_JETSAM_MEMLIMIT_INACTIVE_FATAL` and `:271` calls
  `posix_spawnattr_setjetsam_ext(&attr, flags, jetsam_priority, memlimit_mb, memlimit_mb)`
  (https://raw.githubusercontent.com/apple-oss-distributions/system_cmds/system_cmds-1042.120.1/taskpolicy/taskpolicy.c):
  the same fatal kernel cap as Hex's layer one. `DESIGN`: per process, no group, no delete (the bench ends
  it with a signal). Probe: `taskpolicy -m 64 /bin/dd bs=256m count=2` exited 137; a forked child escaped.
- **launchd transient jobs** (`launchctl bootstrap gui/$UID <plist>` / `bootout`). `man launchd.plist`
  (macOS 26.2): "When a job dies, launchd kills any remaining processes with the same process group ID as
  the job." Its `HardResourceLimits` `ResidentSetSize` is advisory ("the system will prefer to take memory
  from processes that are exceeding"); probe: dd under a 64 MiB hard RSS ran all 512 MiB. The undocumented
  `JetsamProperties`/`JetsamMemoryLimit` showed "jetsam memory limit (active) = (unlimited)" for a gui-domain
  job (probe); whether the system domain honours it needs root (untested). So: a group lifecycle with no
  memory bound for a user's job.
- Not class for class: `sandbox-exec` ("The sandbox-exec command is DEPRECATED.", no memory control);
  Apple's `container` ("run Linux containers as lightweight virtual machines", apple/container README:6 at
  92ff182) is the `oci` class through a VM.

## Windows

- **Process Governor (`procgov`)**, MIT, 4.1.26190 (2026-07-08), `winget install procgov`. README at
  41f4900 (https://raw.githubusercontent.com/lowleveldesign/process-governor/41f49007c24bdfbfd1f63b58f94e86ce3b45b9f7/README.md):7
  "It uses a job object for this purpose."; `--maxjobmem` (:126) is "the maximum committed memory for all
  the processes that belong to a given job object". `procgov-lib/Win32JobModule.cs:184` sets
  `JOB_OBJECT_LIMIT_JOB_MEMORY`; `Win32ProcessModule.cs:584/655` creates the process `CREATE_SUSPENDED`
  with `PROC_THREAD_ATTRIBUTE_JOB_LIST`, Hex's own start path. Gap: whether its job is kill-on-close (how
  its delete ends a tree) is unread; the bench says how it ends one.
- **HostProcess (hcsshim job) containers** through containerd: hcsshim v0.15.0
  `internal/jobcontainers/oci.go:63-75` maps `io.microsoft.container.memory.sizeinmb` to the job limit,
  `internal/jobobject/limits.go:87-88` sets `JOB_OBJECT_LIMIT_JOB_MEMORY` and `:72` `KILL_ON_JOB_CLOSE`.
  Gaps: Windows 11 client support and driving it without a CRI pod are unconfirmed; it needs containerd
  and an image on jupiter.
- Not class for class: Docker `--isolation=process` is the `oci` class; Windows Sandbox is
  "hypervisor-based virtualization" and one instance at a time.

## What the bench takes

macOS: `taskpolicy -m` (the same cap, one process) and launchd `bootstrap`/`bootout` (the shipped group
lifecycle, no memory bound for a user's job). Windows: Process Governor; HostProcess containers if jupiter
can run them, else left out with the reason. Each figure names its run (CLAUDE.md §7).
