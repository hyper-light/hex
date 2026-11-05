# Boxes on macOS and Windows, natively: what each OS gives, and where a Linux kernel can't be avoided

Source notes for Ada's requirement (2026-10-31): "Why can't we run natively on Windows and MacOS? This is a
requirement." It followed her asking whether goal 5's new Linux calls (`BPF_PROG_QUERY`, `mount_setattr`) were
cross-OS and cross-arch. They are cross-arch (libc's per-architecture syscall numbers, run as root on fedora's amd64
and the Mac Studio's arm64 Linux VM), but Linux's alone. This note says what each OS gives a box natively, with
sources, so the decision in `docs/design/runtime.md` ("each OS runs its own boxes natively") can cite it.

Sources were read at fixed revisions and copied into `~/.cache/hex-t7/cross-os/src/` on adacachy (2026-10-31):
Microsoft's WSL docs (`MicrosoftDocs/WSL` at `7b28cc1ee9b8`), Windows containers docs
(`MicrosoftDocs/Virtualization-Documentation` at `85e96f5075e5`), Win32 docs (`MicrosoftDocs/win32` at `ca8274decead`,
`MicrosoftDocs/sdk-api` at `656e9656bfae`), the OCI runtime spec v1.2.1, Apple's Containerization (`apple/containerization`
at `fd4a84a9aa85`), XNU `xnu-12377.1.9` (`apple-oss-distributions/xnu` at `f6217f8`), Shards at `dd074e44` (hyper-light's
microVM product), and the macOS 26.5 SDK of the Command Line Tools on adas-mac-studio-3 (macOS 26.2). Each quote is one
line of its source, copied exactly; `path:line` is at that revision.

## 1. A Linux image needs a Linux kernel

An OCI image for Linux holds Linux programs, and they reach every resource through Linux's system calls. Hex's `oci`
class isolates them with the same kernel's namespaces, cgroups, device programs and mounts. Neither macOS's kernel nor
Windows' implements that interface.

- Microsoft tried running Linux programs natively, by translation, and replaced it with a kernel in a VM:
  - `WSL/compare-versions.md:71`: "Linux binaries use system calls to perform functions such as accessing files,
    requesting memory, creating processes, and more. Whereas WSL 1 used a translation layer that was built by the WSL
    team, WSL 2 includes its own Linux kernel with full system call compatibility. Benefits include:"
  - `:73`: "- A whole new set of apps that you can run inside of WSL, such as **[Docker](tutorials/wsl-containers.md)**
    and more."
  - `:27`: "Full system call compatibility| ❌ | ✅" (WSL 1, WSL 2).
- Apple, which owns macOS, runs Linux containers in VMs:
  - `README.md:27` (Containerization): "Containerization executes each Linux container inside of its own lightweight
    virtual machine. Clients can create dedicated IP addresses for every container to remove the need for individual
    port forwarding. Containers achieve sub-second start times using an optimized [Linux kernel configuration](/kernel)
    and a minimal root filesystem with a lightweight init system."
- So a Linux image off Linux runs on a Linux kernel in a guest. What can be native is everything around it: Hex
  itself, the hypervisor it drives (§4), and each OS's own programs in each OS's own boxes (§2, §3).

## 2. Windows: containers and job objects, natively

### Windows containers (Windows images)

- Process isolation shares the host's kernel, as Linux containers do (`manage-containers/hyperv-container.md:19`):
  "This is the "traditional" isolation mode for containers and is what is described in the [Windows containers
  overview](../about/index.md). With process isolation, multiple container instances run concurrently on a given host
  with isolation provided through namespace, resource control, and other process isolation technologies. When running in
  this mode, containers share the same kernel with the host as well as each other.  This is approximately the same as
  how Linux containers run."
- The OCI runtime spec defines them over Windows' own API (`config-windows.md:4`): "The Windows container specification
  uses APIs provided by the Windows Host Compute Service (HCS) to fulfill the spec." Without `hyperv` they are process
  isolated (`:201`): "If omitted, the container MUST be run as a Windows Server container."
- Devices are the container's only by grant, the default policy's native equivalent
  (`deploy-containers/hardware-devices-in-containers.md:12`): "By default, Windows containers are given minimal access to
  host devices--just like Linux containers. There are certain workloads where it is beneficial--or even imperative--to
  access and communicate with host hardware devices. This guide covers which devices are supported in containers and how
  to get started." A grant names a device interface class (`config-windows.md:31`: "Today, Windows only supports a value
  of `class`"), for process-isolated containers only (`hardware-devices-in-containers.md:20`).
- Production is Windows Server's (`hyperv-container.md:76`): "> You should continue to use Windows Server as the host for
  production deployments. By using this feature on Windows 10 Pro and Enterprise, you must also ensure that your host and
  container version tags match, otherwise the container may fail to start or exhibit undefined behavior."

### Job objects (the `process` class)

A job object bounds a process tree as a cgroup does, with hard limits:

- `ProcThread/job-objects.md:11`: "A *job object* allows groups of processes to be managed as a unit."
- Children join their parent's job (`:32`): "After a process is associated with a job, by default any child processes
  it creates using" CreateProcess "are also associated with the job."
- Memory, the whole job's (`ns-winnt-jobobject_extended_limit_information.md:78`): "... this member specifies the limit
  for the virtual memory that can be committed for the job."
- CPU, a hard cap (`ns-winnt-jobobject_cpu_rate_control_information.md:101`): "The job's CPU rate is a hard limit. After
  the job reaches its CPU cycle limit for the current scheduling interval, no threads associated with the job will run
  until the next interval." A weight is the other mode (`:88`).
- Processes (`ns-winnt-jobobject_basic_limit_information.md:102`): "Establishes a maximum number of simultaneously active
  processes associated with the job."
- The whole tree ends with the job (`job-objects.md:80`): "if the job has the JOB\_OBJECT\_LIMIT\_KILL\_ON\_JOB\_CLOSE flag
  specified, closing the last job object handle terminates all associated processes and then destroys the job object
  itself."
- Nested since Windows 8 (`:26`). Limit messages on the completion port may be lost, except one class (`:63`: "delivery
  of messages to the completion port is not guaranteed"; "Notifications for limits set with
  **JobObjectNotificationLimitInformation** are guaranteed to arrive at the completion port."). CLAUDE.md §3 forbids a
  silent drop, so a box's limit events come from that class, or from reading the job's accounting.

## 3. macOS: confinement natively, bounds partly

### Confinement: App Sandbox, inherited

- Apple supports App Sandbox and deprecates the old profile call. Shards' note (`docs/research/macos-confinement.md:4`):
  "`API_DEPRECATED("No longer supported", macos(10.5, 10.8))` and says "This header is". Command-line tools adopt App
  Sandbox with a bundle identifier and its entitlement (`:14-17`).
- A child of a sandboxed process is sandboxed, whatever it is. Apple DTS (Quinn, developer.apple.com/forums/thread/706390,
  "Resolving App Sandbox Inheritance Problems"): "If a sandboxed process runs another program as a child process, that
  child process always inherits its sandbox from the parent." and "A process is not allowed to change its sandbox." So a
  box's launcher, signed for App Sandbox, confines every program the box runs: a compiler, a script, an unsigned tool.
- Its cost, measured by Shards on an M5 Max (`macos-confinement.md:67`): "App Sandbox costs a launch about 3.1 ms at".
- What it can't do as Linux's mounts do: a read-only directory can't be granted between processes
  (`macos-confinement.md:75-77`: "A bookmark" / "passed between processes grants read and write or nothing: a read-only
  one grants" / "nothing [PM M70]."), and device access comes from entitlements fixed when the launcher is signed.

### Bounds: no public cap for a process tree's memory or CPU

- `RLIMIT_AS` is enforced, but per process and on the address space mapped, not memory used (XNU
  `osfmk/vm/vm_map.c:3678`: "* Address space limit enforcement (RLIMIT_AS and RLIMIT_DATA):"; `:3683-3684`:
  "if (map->size_limit != RLIM_INFINITY &&" / "map->size > map->size_limit) {"; set at exec, `bsd/kern/kern_exec.c:1531`).
  A program that reserves more than it uses would fail under it, and N processes get N limits.
- `RLIMIT_NPROC` counts a user's processes, not a box's (`bsd/kern/kern_fork.c:269-270`: "count = chgproccnt(uid, 1);" /
  "rlimit_nproc_cur = proc_limitgetcur(parent_proc, RLIMIT_NPROC);").
- The SDK's public interfaces are priority, not rate (`usr/include/sys/resource.h:120`, `PRIO_DARWIN_BG`; `:528-529`,
  `IOPOL_THROTTLE`, `IOPOL_UTILITY`; QoS classes in `sys/qos.h`), and per-process usage (`libproc.h:111`,
  `proc_pid_rusage`). The jetsam limit call has no public header (only its number, in `sys/syscall.h`).
- So a native macOS box bounds memory by Hex's own rule: it reads each member's footprint, and ends the box when it
  passes its bound (CLAUDE.md §2's eviction by a stated rule). CPU is bounded by priority alone.
- Measured on the Mac Studio (`bench/native/macos/RESULT.md`, commits 374bdbf and 9694770, 2026-10-31):
  - A box reads in 49.3-49.7 µs p50 at 128 processes.
  - A plain `usleep` ran about six times long. A kqueue timer marked `NOTE_CRITICAL` (XNU `bsd/sys/event.h:316`:
    "system does minimal timer coalescing") ran as asked.
  - Polled every (bound - footprint) / 256 GiB/s, at least 250 µs, from a Mach time-constraint thread, a box held a
    256 MiB bound to within 42 MiB, with its members allocating up to 164 GiB/s, twice as many as the cores included.
    The cost of that floor is the watcher's wakes, not its reads. The reads of a box held at 95% of its bound
    took at most 0.55% of a core, but a wake costs 8-17 µs of the thread's own time whatever waits on it, so the
    whole watcher costs 4-8% of a core while a box sits at the floor (native plan step 1's measurement,
    2026-11-01; the record that timed only the reads is corrected on its branch). Overshoot and CPU trade
    against each other through the floor: the rate times the floor, against a wake per floor.
  - Without the time-constraint thread, a box that fills every core passed its bound by up to 1.46 GiB.
  - So the bound holds to the box's rate times the floor, not exactly as Linux's `memory.max` does, and the rate is
    a constant measured per machine.

### Membership: only a user of its own keeps a box whole

A box's rule reads its members, so Hex must know them all, including one that forks, calls `setsid()` and loses its
parent, as build servers do (Gradle's, Bazel's, sccache's).

- macOS can't follow forks (XNU `bsd/sys/event.h:364`): "* NOTE_TRACK, NOTE_TRACKERR, and NOTE_CHILD are no longer
  supported as of 10.5". So such a member leaves its box's process group, session and parent.
- A process can't change its audit session unprivileged (`bsd/security/audit/audit_syscalls.c:1126`, `suser`), but no
  `proc_info` flavor reads another process's session, so a node can't list a box by it.
- Processes can be listed by user (`bsd/sys/proc_info.h:54`): "#define PROC_UID_ONLY           4". A user ID is
  inherited, can't be dropped unprivileged, and makes `RLIMIT_NPROC` a kernel-enforced cap on the box's processes
  (`kern_fork.c:269-270`).
- So a macOS box that can't be escaped runs as a user of its own, from a pool made at install, as Nix's build users
  are. Starting a program as another user takes privilege, so the node needs a privileged helper there. A box
  without one is tracked by its process group, and a member that daemonizes leaves it.

### A box user's program ends where only the helper can see how

The helper starts a box user's programs (`fork`, then `setuid`), so it is their parent and the node isn't. Read at
apple-oss-distributions/xnu main `f6217f891ac0` (2026-11-05):

- Only the parent reaps. `waitid_nocancel` walks the caller's own children (`bsd/kern/kern_exit.c:3194`,
  `PCHILDREN_FOREACH(q, p)`) and, finding none that matches, returns `ECHILD` (:3359-3361); `wait4_nocancel` does
  the same (:2926, :3082). So the node can't collect a box user's program's end, and the helper must reap each one
  it starts, or it stays a zombie under the box's user.
- Anyone may watch the exit. `filt_procattach` (`bsd/kern/kern_event.c:1099`) checks the caller only when it asks
  for `NOTE_EXIT | NOTE_EXITSTATUS` together (:1116-1133: the parent, a ptrace parent, or `cansignal(..., SIGKILL)`,
  else `EACCES`), so a plain `NOTE_EXIT` watch of another user's process is allowed, and a process `proc_find`
  can't resolve is refused `ESRCH` (:1110-1112).
- No exit falls between the two. `proc_prepareexit` moves the exiting process to the zombie list and sets
  `P_REF_DEAD` ("will not be visible via proc_find", :2127-2129) before `proc_exit` (:2159) posts `NOTE_EXIT`
  (:2561), and `proc_find` takes no reference on a `P_REF_DEAD` process (`bsd/kern/kern_proc.c:811-812`). So a
  watch attached after the event is refused `ESRCH`, never left waiting for an event already sent.
- So the node watches a box user's program's `NOTE_EXIT` on a kqueue of its own, and on it (or on `ESRCH`) asks
  the helper for the end, which the helper reaps as parent, waiting for the zombie as the process class does
  (`NOTE_EXIT` comes before `SZOMB`, `kern_exit.c:2561` then :2631).
- So no request blocks for long: the helper's own watch has the exit event queued when the node asks, and its reap
  waits only for the zombie the same exit path makes. The helper bounds that wait at 100 ms
  (`helper_daemon::REAP_PATIENCE`), a guard against a node that asks about a program still running (it then answers
  "running" and holds its connection no longer), not a tuning. So the helper serves one connection at a time and
  needs no thread per waiting program, nor a cap on them.
- Falsified if a `NOTE_EXIT` watch of another user's process is refused unprivileged on a shipping macOS, or a box
  user's program's wait reads `Exit::Unknown` while it runs (the root lab's conformance run, `boxlab.sh`).

## 4. Linux images on macOS and Windows: a guest that Shards boots

Shards boots Linux guests through each OS's own hypervisor (`README.md:424-425`: "Shards uses `/dev/kvm` on Linux,
Hypervisor.framework on macOS and the" / "Windows Hypervisor Platform on Windows."), from one codebase on both
architectures (`:428`):

- `docs/design/architecture.md:203`: "| macOS | aarch64 / arm64 | Hypervisor.framework (arm64 API) | booting Linux;
  snapshots with cold and warm restore |"
- `:205`: "| Windows | x86_64 / amd64 | Windows Hypervisor Platform | booting Linux: SMP, virtio-blk, pmem and vsock; no
  snapshots yet (D140) |"
- `:204` and `:206`: macOS on x86_64 and Windows on arm64 are "planned"; `:197`: "the guest arch is always the host arch."
- Its host-guest channel is vsock, guest ports mapped to host Unix sockets (D12, `:59`); its guest is a pinned kernel
  and an embedded init (D28, `:1727`).

Hex's node in such a guest runs the `oci` class unchanged, goal 5's device programs and `mount_setattr` included
(CLAUDE.md §8). Hex reaches Shards only through a Shards `-api` crate (CLAUDE.md §9), which doesn't exist yet.

## 5. What this gives

| A box needs | Linux | Windows | macOS |
|---|---|---|---|
| its own OS's programs | cgroup box; `oci` | job object; Windows container | App Sandbox launcher |
| memory bound | `memory.max` | job memory limit | Hex's footprint rule |
| CPU | weight, cap | weight, hard cap | priority only |
| processes | `pids.max` | active-process limit | `RLIMIT_NPROC` of the box's own user |
| membership | the cgroup | the job | the box's own user (a privileged helper starts it) |
| devices | device program | device classes, by grant | entitlements, at signing |
| Linux images | native | Shards guest | Shards guest |

## 6. What would falsify it

- A public macOS interface that caps a process tree's memory or CPU: then §3's rules give way to it.
- A Windows client edition supporting process isolation for production: then Windows containers aren't Windows
  Server's alone (`hyperv-container.md:76` says otherwise today).
- An App Sandbox launcher whose child escapes its sandbox (§3's DTS statement): then macOS boxes need another wall.
- A Linux image running on macOS or Windows without a Linux kernel at full syscall compatibility: WSL 1 is the
  counterexample on record (§1).

## 7. Open, to measure

- macOS: a member that daemonizes, against its box's process group and its user (`bench/native/macos/box_probe.c`
  `escape`); the footprint rule's read cost and how far a box can pass its bound before it ends (`read`, `over`);
  the privileged helper's start cost; App Sandbox per box (all
  launchers of one identity share one container, `macos-confinement.md:93-94`); launch cost on the Mac Studio.
- Windows: HCS from Rust; a container's create and start against Docker's and containerd's (runhcs) on the same host;
  job-object box create against the Linux box; Windows on arm64.
- The Shards `-api` crate, with Shards' owner.
