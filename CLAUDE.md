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

Linux, macOS and Windows, on x86_64 and aarch64. Platform code sits behind `cfg` with a portable path
beside it, or a typed refusal naming the platform. `unsafe` is denied; where an OS interface needs it,
its file is named here with the reason, and every block carries a `// SAFETY:` comment stating its
invariant. The files that use `unsafe` (each with `#![allow(unsafe_code)]` at its top):

- `crates/hex-runtime/src/linux/sys.rs`: the `process` class clones a program straight into its box's
  cgroup (`clone3` with `CLONE_INTO_CGROUP` and `CLONE_PIDFD`) and executes it in the child, or, where
  the kernel refuses that clone, joins it to the cgroup in the forked child before `execve` (`pre_exec`);
  Rust can express neither safely.
- `crates/hex-runtime/src/linux/oci/sys.rs`: the `oci` class's `fork`, the `clone3` calls that start a
  container in its cgroup (from the shim, or from a create seed as the node agent's own child, `CLONE_PARENT`
  with `exit_signal` 0, rootless and as root: decision 5) and that clone an exec'd program into it with a
  pidfd the node agent watches —
  as the agent's own child (`CLONE_PARENT`, `exit_signal` 0) where a privileged seed may, else through an
  intermediate (the exec seed, decision 3), `waitpid`, `unshare`, `setns`, `kill`, `prctl` capability calls,
  inherited-fd adoption, the `bpf` calls that load and attach a container's cgroup device program
  and that open a loaded one by id and read its identity and translated instructions (so containers
  with one device policy share one program), the `seccomp(2)` call that loads a container's filter
  without no_new_privs when its spec doesn't ask for it, the signal mask and `signalfd` through which
  `hex oci exec` passes signals on, the signal-disposition reset and `raise` by which
  `hex oci exec` ends the way its program did when the node agent drives it (seam mode), the
  `_exit` by which a seed's forked intermediate and program end without running the seed's `atexit`
  handlers, and the vfork-style `clone3` (`CLONE_VM | CLONE_VFORK`) by which a node agent's thread starts
  an exec'd program as posix_spawn does: an inline-assembly trampoline for x86_64 and aarch64 that runs
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
  membership, step 2) and `proc_pid_rusage` reads each member's footprint, start time and exit; `thread_policy_set(THREAD_TIME_CONSTRAINT_POLICY)` makes the watcher a Mach
  time-constraint thread; `kqueue`/`kevent` carry its one-shot `NOTE_CRITICAL` timer, a user event to wake
  it, and `EVFILT_PROC` (`NOTE_FORK`/`NOTE_EXIT`/`NOTE_EXITSTATUS`, the dead member's wait status in the
  event, so a cap kill is told from an ordinary SIGKILL without racing its reaper) on members;
  `mach_timebase_info` converts Mach ticks; `sysctlbyname("kern.maxproc")` bounds a listing and
  `sysctlbyname("kern.argmax")` a program's arguments and environment; `getrusage`
  and `getloadavg` read the node's own cost for the bench and observability. rustix wraps the `kqueue`,
  `kill`/`killpg` and `waitid` beside these, which the class calls through it; libc binds the rest as
  `unsafe` foreign functions, and `posix_spawnattr_set_qos_class_np` and `posix_spawnattr_setjetsam_ext`
  Hex declares itself (the macOS 26 SDK ships neither prototype, nor the jetsam flag values, which are
  xnu's `bsd/sys/spawn_internal.h`, though libSystem exports the symbols). Rust can express none of these
  foreign calls, nor the raw `posix_spawnattr_t` the spawn-attribute setters write, safely, and macOS
  offers no safe wrapper.
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
  (`SetInformationJobObject`: the commit cap, active-process cap, kill-on-close and no breakaway, the CPU
  weight or hard cap, and the memory notification the one class Windows guarantees), starts each member
  suspended inside the job and nowhere else (`CreateProcessW` with `CREATE_SUSPENDED` and a
  `PROC_THREAD_ATTRIBUTE_JOB_LIST`, so it is a job member from creation, and a
  `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` so only its three standard handles are inherited —
  `InitializeProcThreadAttributeList`/`UpdateProcThreadAttribute`/`DeleteProcThreadAttributeList`),
  resumes it (`ResumeThread`), waits on it and reads its end (`WaitForSingleObject`,
  `GetExitCodeProcess`), marks its standard handles inheritable (`SetHandleInformation`), reads the job's
  accounting and its guaranteed memory-limit violation (`QueryInformationJobObject`), ends the job
  (`TerminateJobObject`) and closes its handles (`CloseHandle`), and reads the machine's processor count
  (`GetActiveProcessorCount`). For the `oci` class's Shards guest it reads the default shared roots
  (docs/design/shards-guest.md §4a): the users folder from its known folder (`SHGetKnownFolderPath` with
  `FOLDERID_UserProfiles`, its string freed by `CoTaskMemFree`) and the temp folder (`GetTempPath2W`).
  Where its boxes outlive the node (the keeper, `super::keeper`; decision
  "the Windows node, on one completion port"), it names each box's job (`CreateJobObjectW` with a name,
  reopened by the next node with `OpenJobObjectW`), and a keeper holds a second handle to each: the node
  duplicates the job into the keeper and closes it there on delete (`DuplicateHandle` into another process
  and with `DUPLICATE_CLOSE_SOURCE`, the keeper opened by `OpenProcess` with `PROCESS_DUP_HANDLE`), names
  a process by its creation time (`GetProcessTimes`) so a reused pid is told apart, waits on a process or
  the keeper's stop (`WaitForMultipleObjects`), and makes and signals that stop (`CreateEventW` named,
  `SetEvent`). `windows-sys` binds these as `unsafe` foreign functions, and std's own
  job-object support and its `PROC_THREAD_ATTRIBUTE_JOB_LIST`/`_HANDLE_LIST` process creation are
  unstable; Rust can express none of them safely.
- `crates/hex-runtime/src/windows/oci/sys.rs`: the Windows `oci` class, a Windows image run process-isolated
  through the Host Compute Service with a job of Hex's nested in its silo (decision "the Windows `oci` class is
  an HCS silo holding a Hex job"). It resolves at run time (`LoadLibraryExW` from System32, `GetProcAddress`)
  and calls HCS's operation, compute-system and process calls (`HcsCreateOperation`,
  `HcsWaitForOperationResult[AndProcessInfo]`, `HcsCancelOperation`, `HcsCloseOperation`,
  `HcsCreateComputeSystem`, `HcsStartComputeSystem`, `HcsTerminateComputeSystem`, `HcsCloseComputeSystem`,
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
  it runs (`CreateFileW`, a query open). Rust can express none of these
  foreign calls, the function pointers resolved at run time, nor a handle value crossing processes, safely.

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
cargo clippy --workspace --all-targets --locked --target x86_64-unknown-linux-gnu --target aarch64-unknown-linux-gnu --target aarch64-unknown-linux-musl -- -D warnings
cargo clippy --workspace --all-targets --locked --target aarch64-apple-darwin --target x86_64-apple-darwin -- -D warnings
cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc -- -D warnings
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
  code; wire formats change append-only.
- Built, merged and live are three different things: say which.

## Where things are

- `docs/design/`: the design and its decision records.
- `docs/research/`: the source notes decisions cite.
- `docs/requirements.toml`: the requirement registry.
- `STATUS.md`: what works today, and what closes each open piece.
- `bench/`: benchmarks and their records.
- `vendor/`: what Hex vendors (`vendor/MANIFEST.toml`).
