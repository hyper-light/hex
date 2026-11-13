# Hex: rules for every change

Hex is the scheduler and runtime: agent scheduling orchestration for infinite scale (github.com/hyper-light/hex; track T7 of kiln's critical path, kiln plan
`01a157fe102576a8bf44ccaa5d835de7`). It runs process boxes, OCI containers, Firecracker microVMs and Shards microVMs, and places them
from a laptop to millions of agents in one cluster, built on what Kubernetes, Borg and the rest
learned. It replaces Shards' own host-side scheduler and runtime.

These rules are absolute. They are kiln's critical-path bar (its master plan §3, kiln plan
`01a157fddb8276a2996246ea931c4808`), the bar Mantle, Focal, Slates and Shards hold. Production code is everything compiled
outside `#[cfg(test)]`, `tests/`, `benches/` and `examples/`.

## 1. Production code never panics

No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`, `unreachable!`, `assert!`, slice indexing
or unchecked arithmetic. Every failure is a typed error returned to a caller that handles it:
`checked_*` arithmetic (`saturating_*` only where saturation is the stated meaning), `try_from` for
narrowing, `.get(..)` for indexing; a poisoned lock, a closed channel or an ended task is an error to
return. The workspace lints (`Cargo.toml`, `clippy.toml`) encode this, and `scripts/check-production.sh`
is its gate: it forbids them, so an `#[allow]` or `#[expect]` for one in production code fails it. Test code opts out at its crate root
with `#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::disallowed_macros))]`.
Vendored code Hex changes follows the same rule.

## 2. Every resource has a bound

Every queue, cache, map, buffer, retry loop and wait has a stated bound and a stated overflow
behavior: a typed refusal, or an eviction by a stated rule. A loop over user-scaled data ends on a
counted budget or a deadline. Memory and disk are charged to budgets.

## 3. Typed refusals, never silent drops

What can't be done is refused with a typed error that says what and why. Nothing is dropped
silently: not an update, a message, a byte or a request.

## 4. Root causes

A bug fix starts with a failing test. A failing test is fixed at its cause, never by a longer
timeout, a retry or a raised limit. A test waits on the fact it needs, never on a wall-clock guess.

## 5. Decisions cite evidence

Each design decision is in `docs/design/` with its sources (peer-reviewed work and primary sources:
specifications, RFCs, kernel and vendor documentation), its evidence, and what would falsify it;
source notes go in `docs/research/`. A tuning constant is measured on the running hardware or cited,
and its comment says which.

## 6. Tests exercise behavior

Each test reads "do X, expect Y" and cites the requirement it serves (`AC-T7.n.k`, `T-T7.n.k`,
docs/requirements.toml). The kinds that apply: model-based and oracle tests, non-vacuity counters,
deterministic simulation with a nemesis, crash injection at every write, loom, Miri, shuttle and TSan,
hostile input to every parser, Elle-style isolation checks, and doc-truth tests.

## 7. Benchmarks are recorded

A performance claim names the run that measured it: a release binary with its hardware, commit, date
and load (load average under 2, nothing else building), best of N, recorded in `bench/`. Comparisons
are class for class, against named competitors on the same hardware.

## 8. One code path

A laptop, a fleet and global multi-region run the same code, and the same differential suite passes on
one node, three nodes and three regions. Each step up in scale adds only the choice it can't avoid.

## 9. Seams

Other products are reached only through their `-api` crates, and Hex only through its own: each
trait with a simulator implementation and a conformance suite both pass. New subsystems are pure cores
(sans-IO) driven by shells.

## 10. Observability is part of done

Metrics are `hex_*` in base units (`_total`, `_seconds`, `_bytes`) with no high-cardinality labels;
events and spans go on dotted targets with an `event` field. New names are agreed with kiln's Observe
owner (the Kiln Metrics agent). Every component ships its dashboard rows.

## 11. No runtime recursion

No function calls itself, directly or through others, and no algorithm walks a user-scaled structure
depth-first on the call stack. Work is flat: loops with counted budgets and bounded queues.

## 12. Portable

Linux (glibc and musl), macOS and Windows, on x86_64 and aarch64, but for macOS on x86_64 (Ada, 2026-11-09:
"windows, macos, and linux (of most any major flavor) compatible. AMD 64 and ARM64 excepting MacOS x86_64"). Platform code sits behind `cfg` with equivalent working adapters on every supported target.
Ada, 2026-11-10: valid device requests must work across these targets; a platform-based
`unsupported` refusal is an implementation gap, never completion. Intel Mac remains excluded.
`unsafe` is denied; where an OS interface needs it,
its file is named here with the reason, and every block carries a `// SAFETY:` comment stating its
invariant. The files that use `unsafe` (each with `#![allow(unsafe_code)]` at its top):

- `crates/hex-runtime/src/linux/sys.rs`: the `process` class clones a program straight into its box's
  cgroup (`clone3` with `CLONE_INTO_CGROUP` and `CLONE_PIDFD`, vfork-class on the trampoline below,
  decision 7) and executes it in the child, which shares the agent's memory until its execve and leaves its
  errno there when it can't, or, where
  the kernel refuses that clone, joins it to the cgroup in the forked child before `execve` (`pre_exec`);
  Rust can express neither safely. Its managed-VM launch also uses a COW clone with atomic pidfd
  ownership and a finite private pipe gate: the trusted child joins the cgroup and isolates inherited
  descriptors, then waits until the canonical identity is durable before foreign exec; EOF cancels.
  This held child uses prepared memory and signal-safe syscalls alone, with no vfork sharing.
  And the `close_range` call, with the walk of `/proc/self/fd` by borrowed raw
  descriptors where the kernel refuses it, by which every descriptor from 3 up is marked close-on-exec before
  foreign code runs (those two children, a hook's, a container's init, an exec'd program's child, a seed:
  CVE-2024-21626's class), which rustix doesn't wrap.
- `crates/hex-runtime/src/linux/oci/sys.rs`: the `oci` class's `fork`, the `clone3` calls that start a
  container in its cgroup (from the shim, or from a create seed as the node agent's own child, `CLONE_PARENT`
  with `exit_signal` 0, rootless and as root: decision 5) and that clone an exec'd program into it with a
  pidfd the node agent watches —
  as the agent's own child (`CLONE_PARENT`, `exit_signal` 0) where a privileged seed may, else through an
  intermediate (the exec seed, decision 3), `waitpid`, `unshare`, `setns`, `kill`, `prctl` capability calls,
  inherited-fd adoption, the `bpf` calls that load and attach a container's cgroup device program
  and that open a loaded one by id and read its identity and translated instructions (so containers
  with one device policy share one program), the `seccomp(2)` call that loads a container's filter
  without no_new_privs when its spec doesn't ask for it, the `listmount` and `statmount` calls by
  which the classic init finds the cgroup filesystems a recursive read-only bind carries (the libc
  crate numbers them for m68k alone), the `mount_setattr(2)` call by which the
  pruned-seed init makes a mount read-only or sets its propagation by descriptor, there being no
  `/proc/self/fd` to name it by path in its namespace (goal 5's pruned root, decision 5), the signal
  mask and `signalfd` through which
  `hex oci exec` passes signals on, the signal-disposition reset and `raise` by which
  `hex oci exec` ends the way its program did when the node agent drives it (seam mode), the
  `_exit` by which a seed's forked intermediate and program end without running the seed's `atexit`
  handlers, and the vfork-style `clone3` (`CLONE_VM | CLONE_VFORK`) by which a node agent's thread starts
  an exec'd program, or a `process` box's program, as posix_spawn does: an inline-assembly trampoline for x86_64 and aarch64 that runs
  the child on a stack of its own (mapped with a guard page), every signal blocked around the clone, and
  the child's `execve` and signal reset, none of which rustix wraps safely. The process is
  single-threaded (it re-exec'd) where that matters, but for the vfork-style clone's child, which runs in
  the multi-threaded agent's memory and so makes system calls alone on what was made before the clone,
  as posix_spawn's child does.
- `crates/hex-runtime/src/macos/sys.rs`: the macOS `process` class, which has no cgroup, so a box is a
  process group under a watcher that reads its members' footprints and ends it past a bound (decision
  "each OS runs its own boxes natively"). `posix_spawn` starts each member in the box's process group
  (`POSIX_SPAWN_SETPGROUP`, `SETSIGDEF`, `SETSIGMASK`, `POSIX_SPAWN_CLOEXEC_DEFAULT` so only its stdio is
  inherited, `posix_spawnattr_set_qos_class_np` to clamp its QoS, `posix_spawnattr_setjetsam_ext` to arm a
  fatal per-process memory cap at the box's whole bound (step 1's `_ext` backstop: the kernel SIGKILLs a
  member that crosses it, rootless, with no overshoot), and `adddup2`/`addchdir` file actions);
  `proc_listpids(PROC_PGRP_ONLY)` lists the group (and `PROC_UID_ONLY` a user, for the box helper's
  membership, step 2) and `proc_pid_rusage` reads each member's footprint, start time and exit, and
  `proc_pidinfo(PROC_PIDTBSDINFO)` its run state, so a box's hold continues only the members it stopped,
  and `proc_pidinfo(PROC_PIDTASKINFO)` whether a member has a thread running or waiting for a core, so a run
  that gave it none shows nothing of its rate; `proc_pidinfo(PROC_PIDLISTTHREADS, PROC_PIDTHREADINFO)` read
  a VM process's threads by name and CPU, and `PROC_PIDREGIONINFO`/`PROC_PIDREGIONPATHINFO` its memory map
  entry by entry (`struct proc_regioninfo`, which libc doesn't bind, Hex declares), so the guest-cost
  producer tells the VMM's own CPU and memory from its guest's (AC-T7.4.1);
  `kqueue`/`kevent` carry its one-shot `NOTE_CRITICAL` timer, a user event to wake
  it, and `EVFILT_PROC` (`NOTE_FORK`/`NOTE_EXIT`/`NOTE_EXITSTATUS`, the dead member's wait status in the
  event, so a cap kill is told from an ordinary SIGKILL without racing its reaper) on members, and, for the
  node's socket on macOS (native plan step 8, `hex-node`'s `serve::macos`, one kqueue on its I/O thread),
  level-triggered `EVFILT_READ`/`EVFILT_WRITE` on the listener and connections; `getpeereid` names the
  socket's peer, so only the node's user and root are served; `mach_timebase_info` converts Mach ticks;
  `sysctlbyname("kern.maxproc")` bounds a listing, `sysctlbyname("kern.argmax")` a program's arguments
  and environment, and `sysctlbyname("kern.maxprocperuid")` is a box-user box's default process cap, and the
  node's facts read `hw.memsize`, `hw.physicalcpu`, `hw.pagesize`, `vm.page_free_count`,
  `vm.page_speculative_count` and `kern.hv_support` by it (`hex-node`'s `facts`); `getrusage`
  and `getloadavg` read the node's own cost for the bench and observability. rustix wraps the `kqueue`,
  `kill`/`killpg` and `waitid` beside these, which the class calls through it; libc binds the rest as
  `unsafe` foreign functions, and `posix_spawnattr_set_qos_class_np` and `posix_spawnattr_setjetsam_ext`
  Hex declares itself (the macOS 26 SDK ships neither prototype, nor the jetsam flag values, which are
  xnu's `bsd/sys/spawn_internal.h`, though libSystem exports the symbols). Rust can express none of these
  foreign calls, nor the raw `posix_spawnattr_t` the spawn-attribute setters write, safely, and macOS
  offers no safe wrapper. Managed VM controllers use the SAME native owner and process group:
  `POSIX_SPAWN_START_SUSPENDED` holds foreign code until its exact canonical PID/birth is durable;
  read-only `sigaction` rejects auto-reaping/custom child ownership before launch, and `getpgid`
  positively checks native membership. Passive `EVFILT_SIGNAL` for SIGCHLD on that controller's
  existing kqueue observes waitable zombie publication without changing handlers or masks. The sole
  waiter retains WNOWAIT ownership until durable Released/finalization. These calls need the same
  foreign ABI isolation; they do not establish cold cleanup authority or an aggregate CPU cap.
  Its managed helper also reads `clock_gettime(CLOCK_UPTIME_RAW)` into one initialized,
  checked timespec to transfer an original same-host absolute operation deadline. This is
  Rust1.99.1 Instant's Apple clock; locked rustix1.1.5 exposes no safe Apple uptime-raw
  clock variant. The sender samples the native clock before the remaining Instant budget
  so conversion cannot extend authority; the receiver checks the same clock and exact
  helper identity. Read failure, negative/invalid timespec or arithmetic overflow refuses.
  This read-only foreign call needs the existing file's isolation and SAFETY invariant;
  it changes no clock and grants no lifecycle, release or resource authority.
  The opt-in `native-trusted-entry-measurement` path also reads the loader-owned
  `_NSGetArgc`/`_NSGetArgv` before the first standard argv collection, with native
  count/NUL/ARG_MAX bounds and fallible owned copies. Its measurement-only fresh
  Hex entry uses prepared attributes/actions for SETEXEC plus START_SUSPENDED,
  exact returned-PID storage, initialized RLIMIT_NPROC0 readback and descriptor
  copies/flags. Finite kevent output and the original CLOCK_UPTIME_RAW expiry
  observe one exact known controller. Genuine task-name/audit-token queries own
  and release their Mach rights; token-fenced CONT/KILL never fall back to PID
  signals and tokens are recaptured after exec. These public native interfaces
  have no equivalent safe workspace wrapper; all reached unsafe blocks remain
  in this existing sys file with explicit SAFETY obligations. The focused safe
  process child grants no whole-scope Released or refund and never continues the
  foreign image. Source pins and unresolved pre-main, image/TOCTOU, resource and
  closed-worker obligations are in trusted-entry-native-component.md and
  trusted-entry-measurement-source.md. No native result is asserted by registration.
- `crates/hex-runtime/src/macos/helper_sys.rs`: the macOS box helper daemon's privileged syscalls (the
  `process` class's box-user mode, decision "each OS runs its own boxes natively"; step 2). The daemon,
  as root, starts a box's programs as the box's pool user: a `fork` whose child drops to the user
  (`setgid`/`setuid`), opens a session for a leader (`setsid`), caps the user's processes
  (`setrlimit(RLIMIT_NPROC)`) and the program's descriptors (`RLIMIT_NOFILE`), puts the three stdio descriptors on 0/1/2 (`dup2`), closes the rest
  (`getdtablesize` then `close`, there being no `closefrom` in libc) and `execve`s the program — every
  call async-signal-safe, as the Linux runtime's `pre_exec` is, since between fork and exec a
  multi-threaded process may touch no lock. It then arms each member: `task_for_pid` for the member's
  task, `task_set_phys_footprint_limit` for its ledger limit, and
  `task_set_exception_ports`(`EXC_MASK_RESOURCE`) to route a crossing to the daemon's port
  (`mach_port_allocate`/`mach_port_insert_right`), so a crossing notifies rather than kills. The daemon
  receives those crossings with `mach_msg` on that port and replies, freeing the thread and task send
  rights the message carried with `mach_port_deallocate`, and re-apportions the box (the event-driven
  rebalance, no polling). `posix_spawn` can't change uid, so the start is a fork, not a spawn; libc binds
  the POSIX calls and the Mach routines are declared as `unsafe` foreign functions, macOS offering no safe
  wrapper.
- `crates/hex-runtime/src/windows/sys.rs`: the Windows `process` class, whose box is a job object nested
  under the node's. It creates the job (`CreateJobObjectW`), sets its hard limits
  (`SetInformationJobObject`: the commit cap, active-process cap, kill-on-close and no breakaway, and the
  CPU weight or hard cap), starts each member
  suspended inside the job and nowhere else (`CreateProcessW` with `CREATE_SUSPENDED` and a
  `PROC_THREAD_ATTRIBUTE_JOB_LIST`, so it is a job member from creation, and a
  `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` so only its three standard handles are inherited —
  `InitializeProcThreadAttributeList`/`UpdateProcThreadAttribute`/`DeleteProcThreadAttributeList`),
  resumes it (`ResumeThread`), waits on it and reads its end (`WaitForSingleObject`,
  `GetExitCodeProcess`), marks its standard handles inheritable (`SetHandleInformation`), reads the job's
  accounting (`QueryInformationJobObject`), ends the job
  (`TerminateJobObject`) or one member alone (`TerminateProcess`, a program's own kill) and closes its
  handles (`CloseHandle`), and reads the machine's processor count
  (`GetActiveProcessorCount`), the measured worker stack extent (`GetCurrentThreadStackLimits`),
  the enclosing job's process ceiling (`IsProcessInJob` and `QueryInformationJobObject`), and, for the node's facts (`hex-node`'s `facts`), its memory
  (`GlobalMemoryStatusEx`, also its commit room for admission) and a volume's size and free space (`GetDiskFreeSpaceExW`). For the `oci` class's Shards guest it reads the default shared roots
  (docs/design/shards-guest.md §4a): the users folder from its known folder (`SHGetKnownFolderPath` with
  `FOLDERID_UserProfiles`, its string freed by `CoTaskMemFree`) and the temp folder (`GetTempPath2W`).
  Where its boxes outlive the node (the keeper, `super::keeper`; decision
  "the Windows node, on one completion port"), it names each box's job (`CreateJobObjectW` with a name,
  reopened by the next node with `OpenJobObjectW`), and a keeper holds a second handle to each: the node
  duplicates the job into the keeper and closes it there on delete (`DuplicateHandle` into another process
  and with `DUPLICATE_CLOSE_SOURCE`, the keeper opened by `OpenProcess` with `PROCESS_DUP_HANDLE`), starts
  the keeper as it starts a member but in no job (`CreateProcessW` with the `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`
  alone, its three standard handles the NUL device), so the keeper, which outlives the node, holds none of
  the node's handles, names a process by its creation time (`GetProcessTimes`) so a reused pid is told
  apart, compares an observation duplicate of a keeper-held handle with the original native job
  (`CompareObjectHandles`) before closing that remote value, so a recycled value cannot authorize
  disposal of a different object, and makes and signals the keeper's stop (`CreateEventW` named, `SetEvent`). Its HCS request
  wake event is reset before scanning durable requests (`ResetEvent`), and the keeper waits on its
  stop and request events together (`WaitForMultipleObjects`), retaining native creation ownership
  after the node ends without periodically probing the request folder. `windows-sys` binds these as `unsafe` foreign
  functions, and std's own
  job-object support and its `PROC_THREAD_ATTRIBUTE_JOB_LIST`/`_HANDLE_LIST` process creation are
  unstable; Rust can express none of them safely.
  The guest observer also uses synchronous `WSAPoll`/`WSAGetLastError` on its two
  borrowed output sockets. Only active endpoints enter the fixed stack array; the finite
  deadline and native array remain owned for the call. No data, socket mode, timeout option,
  asynchronous operation or handle ownership changes. These Winsock calls have no safe SDK wrapper.

- `crates/hex-runtime/src/windows/wmi.rs`: the Windows node reads the optional features Hex's installer
  turns on (Containers, HypervisorPlatform) from WMI in process rather than starting PowerShell for them.
  On a thread of its own it initializes COM (`CoInitializeEx`, multithreaded, paired with
  `CoUninitialize`), makes WMI's in-process locator (`CoCreateInstance` of `WbemLocator`), connects to
  `ROOT\CIMV2` (`IWbemLocator::ConnectServer`), sets the proxy's security (`CoSetProxyBlanket`: Windows
  authentication, call level, impersonate), runs one WQL query semisynchronously and forward-only
  (`IWbemServices::ExecQuery`), reads its instances with `IEnumWbemClassObject::Next` bounded by the time
  left, and each instance's properties with `IWbemClassObject::Get`. The `windows` crate binds these COM
  methods and functions as `unsafe`, and Rust can express none of them safely. It reads the two values
  `Get` fills from the VARIANT's union by its type tag (VT_BSTR's BSTR, VT_I4's i32), there being no safe
  read: the `windows` crate's safe conversions call propsys.dll, which nanoserver doesn't carry, and
  hex.exe is an `oci` box's launcher there.
- `crates/hex-runtime/src/windows/oci/sys.rs`: the Windows `oci` class, a Windows image run process-isolated
  through the Host Compute Service with a job of Hex's nested in its silo (decision "the Windows `oci` class is
  an HCS silo holding a Hex job"). It resolves at run time (`LoadLibraryExW` from System32, `GetProcAddress`)
  and calls HCS's operation, compute-system and process calls (`HcsCreateOperation`,
  `HcsWaitForOperationResult[AndProcessInfo]`, `HcsCancelOperation`, `HcsCloseOperation`,
  `HcsCreateComputeSystem`, `HcsOpenComputeSystem` (by which the next node reopens a box a killed node left
  running, to take it back), `HcsStartComputeSystem`, `HcsTerminateComputeSystem`, `HcsCloseComputeSystem`,
  `HcsCreateProcess`, `HcsCloseProcess`, `HcsGetServiceProperties`), freeing HCS's strings (`LocalFree`), and
  vmcompute's layer calls that make and remove a box's scratch as hcsshim and runhcs do (`ActivateLayer`,
  `PrepareLayer`, `GetLayerMountPath`, `UnprepareLayer`, `DeactivateLayer`: undocumented; the documented
  storage calls made a scratch no container finished starting on, decision record) — resolved rather than
  linked, because windows-sys links HCS's at load time (and binds none of vmcompute's), and a host without
  the Containers feature would then not start the node at all. It opens a box's launcher and members by pid (`OpenProcess`), hands the launcher the box's
  job and the caller's standard handles (`DuplicateHandle`), ends a launcher that never replied
  (`TerminateProcess`), checks a member's job (`IsProcessInJob`), adopts in the launcher the handle values
  the node gave it, and reads the host's build and kind (`RtlGetVersion`), an image's build from its base
  layer's `ntdll.dll` (`GetFileVersionInfoW`, `VerQueryValueW`), and a device interface class's members
  (`CM_Get_Device_Interface_List_SizeW`, `CM_Get_Device_Interface_ListW`) and whether each opens from where
  it runs (`CreateFileW`, a query open). The exact present adapter interface for a PNP instance is
  opened by `D3DKMTOpenAdapterFromDeviceName` to obtain its native full LUID, asked its physical
  adapter count (`D3DKMTQueryAdapterInfo`, `KMTQAITYPE_PHYSICALADAPTERCOUNT`: a linked adapter's
  LUID names no one GPU) and closed by `D3DKMTCloseAdapter`; the existing Windows SDK types define
  that ABI. Rust can express none of these
  foreign calls, the function pointers resolved at run time, nor a handle value crossing processes, safely.
- `crates/hex-gpu/src/apple/sys.rs`: Apple Silicon GPU identity and telemetry for the same
  `hex-gpu` inventory/sample seam. Metal's device registry identifiers and the native Objective-C,
  CoreFoundation, IOKit/IOReport interfaces require foreign calls and explicit retain/release ownership;
  Rust cannot express those calls safely through this workspace's existing dependencies. Keep them
  isolated here with checked type/count/lifetime boundaries and a SAFETY explanation per block.
  This adapter is required by Ada's 2026-11-10 full macOS device-support instruction.
- `crates/hex-gpu/src/nvml/sys.rs`: hex-gpu's NVIDIA telemetry (`docs/design/gpu-telemetry.md` G1). It loads
  NVML at run time, so a node without NVIDIA's driver starts and says why it reads no NVIDIA GPU: `dlopen`,
  `dlsym` and `dlerror` on Linux, `LoadLibraryExW` from System32 alone and `GetProcAddress` on Windows, each
  resolved address transmuted to its NVML function's type (nvml.h); the same loader and transmute load ADL
  for `adl/sys.rs`. Through those pointers it calls
  `nvmlInit_v2`, `nvmlSystemGetDriverVersion`, `nvmlDeviceGetCount_v2`, `nvmlDeviceGetHandleByIndex_v2`,
  `nvmlDeviceGetName`, `nvmlDeviceGetPciInfo_v3`, `nvmlDeviceGetMemoryInfo` and
  `nvmlDeviceGetUtilizationRates`, each writing into the caller's frame. A device handle is `Send` and `Sync`
  (`unsafe impl`): NVML is thread-safe, and a handle stays valid until `nvmlShutdown`, which Hex never calls.
  Rust can express none of these foreign calls, the function pointers resolved at run time, nor an opaque
  handle's thread-safety, safely. (Approved by Ada, 2026-11-09: "I approve that unsafe block.")
- `crates/hex-gpu/src/windows_identity/sys.rs`: Windows AMD's actual present PCI devnode join.
  Configuration Manager's locate, device-ID and property calls read only the exact ADL-reported
  PNP instance into fixed initialized buffers. Checked native type/size, termination, complete PCI
  location grammar and BusNumber/Address corroboration establish the full segment/BDF before
  grouping. These SDK calls are unsafe; no pointer or handle escapes and no driver state changes.
- `crates/hex-gpu/src/adl/sys.rs`: hex-gpu's AMD telemetry on Windows (`docs/design/gpu-telemetry.md` G3).
  It loads AMD's ADL (`atiadlxx.dll`, from System32 alone) at run time through `nvml/sys.rs`'s loader, so a
  node without AMD's driver (ARM64 Windows among them) starts and says why it reads no AMD GPU; makes one ADL
  context per `Gpus` with `ADL2_Main_Control_Create`, handed an allocator that refuses (every buffer is
  Hex's own), and ends it with `ADL2_Main_Control_Destroy`; and through the resolved pointers calls
  `ADL2_Adapter_NumberOfAdapters_Get`, `ADL2_Adapter_AdapterInfo_Get`, `ADL2_Adapter_MemoryInfo2_Get`,
  `ADL2_Graphics_VersionsX2_Get`, `ADL2_New_QueryPMLogData_Get` and `ADL2_Adapter_VRAMUsage_Get`, each
  writing into the caller's buffer (PMLog's with room behind it for a larger table than AMD's header
  declares, the pointer the whole buffer's). A context is `Send` and `Sync` (`unsafe impl`): every call
  through it takes `&mut`, so a shared reference reaches none. Rust can express none of these foreign
  calls, the function pointers resolved at run time, nor a context's thread-safety, safely. (Approved by Ada,
  2026-11-09: "go grab the AMD bits needed and the other unsafe is fine.")
- `crates/hex-afunix/src/lib.rs`: the Windows node agent's socket and its client, an AF_UNIX stream on one
  I/O completion port (decision "the Windows node, on one completion port"). It starts Winsock
  (`WSAStartup`), borrows their sockets through `AsSocket`, sets blocking mode (`ioctlsocket`,
  `FIONBIO`), makes, binds, listens on, connects, shuts down and closes AF_UNIX sockets (`WSASocketW`
  overlapped and not inherited, `bind`, `listen`, `connect`, `shutdown`, `closesocket`), bounds a client's
  makes the close of a connection the node has ended a reset
  (`setsockopt` `SO_LINGER`, no linger: afunix.sys doesn't wake every peer's read with a graceful
  end-of-stream) and sends and receives on a client's blocking socket (`send`, `recv`),
  makes the port and puts sockets on it (`CreateIoCompletionPort`), posts a wake and waits on it
  (`PostQueuedCompletionStatus`, `GetQueuedCompletionStatusEx`), starts a connection's overlapped accept,
  receive and send (`AcceptEx` with `SO_UPDATE_ACCEPT_CONTEXT`, `WSARecv`, `WSASend`), each operation's
  `OVERLAPPED` and buffer held on the heap by the kernel from its start until its completion is dequeued,
  reads a peer's pid (`WSAIoctl` `SIO_AF_UNIX_GETPEERPID`) and whether its process runs as this user or
  LocalSystem (`OpenProcess`, `OpenProcessToken`, `GetTokenInformation` `TokenUser`, `EqualSid`,
  `IsWellKnownSid`), gives the peer's process handles of its own to an exec's asked streams and closes ones
  it was never told of (`OpenProcess` with `PROCESS_DUP_HANDLE`, held for the connection; `DuplicateHandle`
  into it, and with `DUPLICATE_CLOSE_SOURCE` and no target), and owns the handles the node gave this
  process for a message (the values its envelope names), and watches a program's end (`OpenProcess`,
  then ntdll's wait completion packet,
  which the kernel queues to the port when the process ends, no thread between: `NtCreateWaitCompletionPacket`,
  `NtAssociateWaitCompletionPacket`, `NtCancelWaitCompletionPacket`; undocumented in the SDK, used by the
  Windows thread pool and Go's runtime, and chosen over `RegisterWaitForSingleObject` by measurement) and watches a host's own connection, a socket it borrows, by the network events
  Winsock records on an event of its own (`WSAEventSelect`; the event made and reset by `CreateEventW` and
  `ResetEvent`), whose signal such a packet queues to the port. `windows-sys` binds these as `unsafe` foreign
  functions, but for the ntdll calls,
  which Hex declares itself; std has no AF_UNIX socket or completion port on Windows, and a buffer the kernel
  writes after the call returns is beyond Rust's borrow rules. Rust can express none of them safely.
- `crates/hex-wait/src/macos/sys.rs`: the kqueue a thread waits on in Hex's condition variable on macOS
  (`hex_wait::Condvar`, and so `hex_wait::mpsc`'s receive), and to a deadline (`hex_wait::rest_until`;
  docs/design/deadline-waits.md). XNU times std's condition variable waits, and its
  semaphores' and `nanosleep`'s, with the leeway of the waiting thread's latency QoS tier (at utility QoS up to
  four times the wait and 75 ms, doubled while the user is idle), and a timer marked `NOTE_CRITICAL` alone with
  none. rustix makes the kqueue (kqueue(2)); `libc::kevent` adds its user event (`EVFILT_USER`, `EV_CLEAR`),
  triggers it (`NOTE_TRIGGER`) and, as the thread waits, arms a one-shot timer to its deadline (`EVFILT_TIMER`,
  `EV_ONESHOT`, `NOTE_NSECONDS | NOTE_CRITICAL`), which rustix's own `kevent`, itself `unsafe`, can't mark
  critical. No event names a descriptor or carries a pointer. Rust can express none of these foreign calls safely.

Vendored Shards (`vendor/shards`, `vendor/shards-gpu`) is Shards' own code at one upstream revision, with
no Hex change but those `vendor/patches` records, each offered upstream (`vendor/MANIFEST.toml`): shards-gpu's
Windows arm64 packaging, and shards-api's start of D231's run host holding only its stdio (posix_spawn and
CreateProcessW, `image/spawn.rs`). Its `unsafe` follows Shards' rules, not this list.

## 13. Lean code

Code states what the system does now: no speculative abstraction, no dead or commented-out code.
Comments explain why.

## Requirement IDs

`docs/requirements.toml` is the registry: the track's goals (`CP-T7.n`), their criteria
(`AC-T7.n.k`) and tests (`T-T7.n.k`), from kiln's plan, with Hex's own IDs beside them as
its design names them. Code comments, commits, tests and benchmarks cite them.

## Vendoring

`vendor/MANIFEST.toml` names what Hex vendors, each at one revision. `scripts/vendor.py` (kiln's
copy, unchanged) takes a project at a revision with its recorded changes re-applied
(`vendor/patches/<project>/`, each a row of the project's `UPSTREAM.md`), and `check` proves each
vendored tree is its upstream plus those changes. Another hyper-light product is a dependency by name
and version (`amphora-api = "0.1"`), patched to its vendored copy in `Cargo.toml`'s
`[patch.crates-io]`; kiln patches the same names to its own copies, so everything builds against one
revision of each (kiln's docs/vendoring.md). No package takes the name of a crate kiln already links:
`hex` is crates.io's hex encoding.

## Gates

Every commit passes these on its final tree. `bash scripts/gates.sh` runs them in order and stops at
the first failure; kiln CI runs it on every push and pull request (`.kiln/ci.yaml`).

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --target x86_64-unknown-linux-gnu --target aarch64-unknown-linux-gnu --target x86_64-unknown-linux-musl --target aarch64-unknown-linux-musl -- -D warnings
cargo clippy --workspace --all-targets --locked --target aarch64-apple-darwin -- -D warnings
cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc --target aarch64-pc-windows-msvc -- -D warnings
bash scripts/check-production.sh
cargo test --workspace --locked
python3 scripts/vendor.py check
```

## Working here

The person's standing rules (kiln's master plan §8):
- Merging to main, pushing anywhere, deploying, and creating repositories take the person's word.
- Never print a secret or a token; a secret typed into a sign-in is never logged or kept.
- Stop only the processes you started, by the PIDs you recorded; never pick them by command line.
- Guard every `rm` (`"${T:?}"`). Labs and big files go under `~/.cache`, never `/tmp`.
- Build with `nice -n 10` and your own `CARGO_TARGET_DIR`. Heavy builds and benchmarks go to fedora or
  jupiter: adacachy runs production. A benchmark takes its slot on kiln's bench calendar.
- No bare `git stash`; rebase and squash only onto a fixed SHA; gate every push on every check's exit
  code. The node's wire has one identity, a digest of its definitions (`hex_core::wire::IDENTITY`): change it
  as the work needs, and a peer of another wire is refused at the hello, typed, naming both (Ada, 2026-11-09).
- Built, merged and live are three different things: say which.

## Where things are

- `docs/design/`: the design and its decision records.
- `docs/research/`: the source notes decisions cite.
- `docs/requirements.toml`: the requirement registry.
- `STATUS.md`: what works today, and what closes each open piece.
- `bench/`: benchmarks and their records.
- `vendor/`: what Hex vendors (`vendor/MANIFEST.toml`).
