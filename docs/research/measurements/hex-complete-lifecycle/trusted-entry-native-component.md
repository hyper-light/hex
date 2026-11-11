Trusted-entry native component on exact ROOT786775e3

SOURCE ONLY. The fixed entry/driver leaf is the next owned checkpoint, not part
of this component. No feature/module/CLI registration, full2ef/fab, helper codec,
worker protocol, native execution, build or test is included. ROOT owns all of
those integrations and the actual measurement. Three new pure controls are UNRUN.

crates/hex-runtime/src/macos/sys.rs factors the existing attribute/action setup
once into PreparedSpawn. Ordinary spawn retains its flags, actions, exact call
order, path/error loop and old local teardown order (pointer vectors, actions,
attributes). The new code is entirely under native-trusted-entry-measurement;
ROOT must register that feature before any matched compile. No dead-code/lint
waiver is introduced.

trusted_entry_native::arguments(selector) is the FIRST main branch's bounded
_NSGetArgc/_NSGetArgv reader. It checks pointer/count/width/NUL/ARG_MAX budgets,
returns no allocation for a nonmatching selector, and copies matching argv bytes
fallibly into owned OsStrings. No raw pointer leaves it or crosses an exec.
Pinned Rust args_os would instead copy the entire argv infallibly before .next.
Its SAFETY premise is the immutable loader argv at first dispatch before argument
mutation; it does not make dyld/stdlib initialization single-threaded or panic-free.

Setexec::new borrows ONLY this ordinarily execed entry's prepared Spawn and shared
native attribute/action setup. It requires one explicit absolute image and adds
SETEXEC to START_SUSPENDED. replace first enforces a rootless unchanged-UID caller,
sets and reads back this process's own NPROC soft/hard0, checks its original raw
expiry, and clears CLOEXEC only on the entry-owned stdio source copies. It records
source flags and actual limits before native spawn. The returned PID is ALWAYS
captured, including unexpected success return; the focused entry leaf must bind
its already-prepared direct-child observer and preserve immediate facts/cleanup.
This component alone is not a launch caller or an unexpected-child cleanup result.

ImageEvents preallocates the existing Kqueue before effects; it is notification
only. The SAME BoxProc is the sole wait/reap owner. One PID/one report/SIGCHLD
bounds the raw event array. It validates exact PID/filter/flags/errno and retains
raw proc flags/fflags/data. Finite native timespec conversion has typed refusal;
it never uses the ordinary overflow-to-infinite timeout fallback. EINTR ends on
the SAME raw expiry, with no new period, allowance or timeout increase.

expiry samples CLOCK_UPTIME_RAW BEFORE the remaining original Instant duration;
the sum is conservative. The receiver compares that SAME raw expiry directly.
It does not construct a later Instant or add tolerance. Ports of the narrow
existing clock and task-name/audit-token wrappers are included, not their old
helper module/worker/CPU machinery. Capture joins expected PID/birth before and
after the genuine kernel token query; task-name rights are deallocated on every
acquired-right path. Continue/Kill use only the native token and retain errors;
there is no PID/PGID signal fallback. The driver must recapture after each exec
because XNU changes the token's exec version.

New/reached unsafe inventory for ROOT CLAUDE rationale:

- libc::_NSGetArgc/_NSGetArgv and bounded initialized argv pointer reads: first
  main dispatch only, loader-owned pointers/count and NUL limits; existing safe
  std args APIs allocate before selection. Libc crt_externs.h at71bbe350 declares
  these accessors; pinned Rust58cf8f9c uses them on Apple.
- clock_gettime(CLOCK_UPTIME_RAW): initialized timespec, checked errno/sec/nsec/
  overflow, SAME Apple Rust Instant clock. No clock/credential/host change.
- posix_spawnattr_getflags/setflags and posix_spawn SETEXEC: initialized owned
  attributes/actions and live terminated pointer arrays, prepared in the fresh
  execed context, with actual PID out-parameter. No COW child calls this adapter.
- getrlimit/setrlimit RLIMIT_NPROC: this rootless process alone, initialized exact
  ABI objects, positive readback of soft=hard0 before replacing its image.
- fcntl F_DUPFD_CLOEXEC/GETFD/SETFD plus OwnedFd::from_raw_fd: initial stdio copied
  into exclusive owned descriptors, flags changed only on those entry copies.
- finite libc::kevent: exact fixed output array and finite checked timespec; no
  second waiter/reaper, generic event manager or helper namespace.
- mach_task_self/task_name_for_pid/task_info(TASK_AUDIT_TOKEN)/
  mach_port_deallocate and proc_signal_with_audittoken: exact eight-word ABI,
  expected identity joins, task-name right owned/released, allowed signals only.

Primary revisions: Apple XNU/Libsyscall f6217f891ac0bb64f3d375211650a4c1ff8ca1ea
(kern_exec/kern_fork, spawn/filter wrappers, libproc/Mach headers); Apple Libc
71bbe350ab79eef58113991d817ccc6165061a64 crt_externs.h; Rust
58cf8f9c35b355199727627d07ae34a4e2846d67 args/unix.rs and sys/time/unix.rs.
Exact bytes/hashes/URLs are in the JSON and retained caches. Installed-kernel or
libSystem byte equivalence is not claimed. ROOT's prior standalone-C data stays
at its original source attribution; no native safety result follows here.

Policy7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506 / rules
 df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3,
L3 implementer, initial ACK and this narrow source release retained. Local
rustfmt parser only; all3 controls/native/compiler/strict gates UNRUN. R01-R16
proof remains incomplete: new primitive guards are source, not pass; native
pre-main/image/TOCTOU/closed-workers, resource profiles, final helper and whole
scope release remain unproved. No canonical charge/row/terminal phase is changed.
