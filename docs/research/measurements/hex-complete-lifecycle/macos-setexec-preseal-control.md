Standalone Darwin pre-exec mechanism fixture — SOURCE ONLY

L3 implementer, policy7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506, rules SHA256
 df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
ROOT explicitly requested this measurement-only C fixture before a production fork/SETEXEC
remedy. No Hex/Shards production, sys wrapper, helper/ABI, Cargo, service or driver changes.
No compilation, syntax-only compiler, test/native process or new agent was run by L3.

Preserved original and question

ROOT06e's exact ProcessBox original reached the actual relevant failure: controller91333,
birth14353033260682, had Code0/ended and only a zombie in its group; escaped91334,
birth14353033512707, session91334, remained live, but terminal returned Released. Its
private lifeline/native exit/controller reap/escaped absent checks all completed BEFORE
the oracle. Full log SHA25614c6f6051e4cdf39358d6ef6914ee1532376e94fef040bccefbb649036df1ecc
was read from mac-scope-06e-results-proof; raw archive54c14f09e7fb568ffcc51d660365a111999201e20d57cce09ac175597f8bb9b6
is ROOT-verified. The older a102 setup-failure actor cleanup is separately UNKNOWN. No
query or signal of those old actors is performed here. The existing independent NPROC
primitive PASS is retained, not rerun by this fixture. These are new combined mechanisms.

Primary basis is the full2ba039ccc73d0378eb9567a8743df7ef381f5e61 research and its nine
pinned XNU/libsyscall sources. That note distinguishes kernel SETEXEC/limit/identity and
START_SUSPENDED/jetsam paths from unproved actual loader/HVF/Vulkan/closed-worker behavior.
No successful result here would make NPROC0 alone or controller exit/PGID empty a native
whole-scope proof. The complete Shards presealed-worker protocol remains separate.

One binary, four finite cases

macos_setexec_preseal_control.c is a standalone macOS arm64 measurement program. Its
only parent invocation is --memory-mib EXPLICIT_NATIVE_BOUND; there is no default cap.
ROOT chooses that positive MiB input within its actual admitted disposable memory budget,
records the input and remaining host/job headroom, and uses an immutable source/binary
path in its owned private cohort. No configuration value becomes a production allowance.
The one existing39.507-second test hang guard covers ALL cases and every actor; no reset,
retry, increased delay, timer period or wall-clock sleep is introduced.

The fixed cases run sequentially, stopping at the first failure:

1. Combined successful replacement: same direct child PID/birth, NPROC soft=hard0,
   prepared public posix_spawn SETEXEC+START_SUSPENDED, SAME Hex background QoS,
   fatal active/inactive jetsam attributes, empty signal mask/default dispositions,
   own process group, CLOEXEC_DEFAULT and explicit stdio/report inheritance. Parent
   requires native NOTE_EXEC, actual stopped state and same positive birth. The private
   canary must be unreadable with EAGAIN before explicit SIGCONT, and the parent's writer
   CLOEXEC flags must be unchanged. After resume the image reports its exact identity,
   zero soft/hard limit, denied nonallowlisted descriptor, own background QoS, fork and
   ordinary spawn EAGAIN, hard-raise EPERM and successfully created/joined required thread.
2. Missing image: exact private owned executable-name derivative is checked absent before
   effects. The trusted child must report native ENOENT and end127; parent owns/reaps it.
3. Post-activation invalid session attributes: the trusted child becomes its own session
   leader, then uses the deliberately incompatible prepared session/group attributes.
   The expected native terminal result is SIGKILL, not a returned successful exec or a
   report-pipe EOF interpreted as success. It executes no foreign canary.
4. Fatal-memory combination: same pre-exec hold/identity/limit/QoS/FD checks, then a fixed
   image touches incompressible data up to the explicit cap plus one native MiB. It
   requires SIGKILL, NOTE_EXIT_MEMORY and a pre-reap peak at least the requested cap.
   Reaching the touch bound or a missing canary/native detail is failure, never skipped.
   This is a bounded mechanism witness, not profiling or proof of a universal memory-
   overshoot guarantee; global memory pressure and actual host context must be retained.

Parent/child safety and error ownership

All attribute/file-action objects, argv/environment strings, decimal fields, paths,
stdio/private-pipe descriptors and observer storage are prepared BEFORE each COW fork.
The trusted trampoline contains only native clock, own FD close/read/write, own NPROC
setrlimit, the negative case's own setsid, getpid, the prepared public posix_spawn call
and _exit. It performs no allocator, formatter, mutex/lock, destructor, dynamic dispatch,
credential change or private syscall. Successful SETEXEC never returns to this image.
Source functions' SAFETY comments identify exact initialized native buffers/attributes,
stable prebuilt storage and unreaped direct-child identities. Libsystem's optional
filtering behavior is neither disabled nor changed; its pinned source limits and actual
host compatibility remain part of ROOT's measurement, not an assumed universal property.

The final native child writer is the actual already-owned inherited FD explicitly named
in file actions. Only that final mapping is rendered into prebuilt arguments. Parent
writer inheritance flags are not cleared; the child-specific inherit action carries it.
This is fork's inherited-number case, not proof that a parent's raw number survives
SCM_RIGHTS or Windows duplication. Product report request/scope and final remapped writer
checks remain required in their actual native launchers. No console parsing, generic
argv rewriting, discovery cache or private endpoint-name identity is introduced.

The parent records Child ID immediately, queries native birth, and retains the only
terminal waiter. A private gate prevents the trusted child entering the replacement
before that observation and native watch. Passive kqueue SIGCHLD plus process exec/exit
filters do not change handlers/masks. The parent rejects auto-reaping/custom SIGCHLD,
root, UID transitions or setuid/setgid test executables before effects. Native state,
clock and PID queries check full outputs. All waits are readiness notifications with the
original absolute CLOCK_UPTIME_RAW expiry; no periodic state probes or inferred absence.

Terminal observation uses waitid NOWAIT, then one positive native exit/peak read and the
same parent's actual reap. If setup/native observation fails, cleanup acts only on this
exact unreaped direct child after native birth revalidation and records its outcome.
Already-reaped or changed identities cannot be signaled. A missing witness/expired budget
is UNKNOWN. A setup failure, malformed private frame or pipe EOF never becomes a semantic
green or Released. Error and cleanup error are printed separately. main_cleanup=REAPED
means only that exact fixture controller; it is not a whole Shards scope statement.

If NPROC unexpectedly permits either fixed forbidden creation, the image publishes that
actual returned child's PID/birth before cleanup, through a typed bounded private frame.
The image itself remains that child's sole waiter, uses its already-prepared notification
observer and collects the exact child. Such a creation always makes the mechanism fail,
even if cleanup succeeds. An unreadable/failing nested cleanup is UNKNOWN and ROOT must
retain the immediate receipt; no foreign process enumeration or textual-PID cleanup is
allowed. Private pipe ends and prepared native objects are closed/destroyed by their
existing fixture owner after actual child outcome. A killed parent still needs ROOT's
exact owned actor/whole-process guard; ordinary cleanup code is not a crash-proof claim.

Native constants and fixed bounds

The parent uses the same Hex spawn flags/file actions as current macos/sys.rs, adding
SETEXEC only for this measurement. QoS background is the SDK constant0x09. Jetsam flags
0x8000|0x04|0x08 and priority40 are exactly existing Hex values, with signed-short bits
preserved; both native limits are the explicit supplied MiB value. No new resource
constant is selected. MiB is the native attribute unit. Decimal storage32 covers native
u64/int inputs; the private frame is statically checked against PIPE_BUF. One fixed
process filter plus one SIGCHLD filter explains the two-element event drain. The only
two unexpected-creation frames correspond to the two explicit negative attempts. The
four cases are fixed behavior cuts, not a sweep or resource manager. The xorshift fill
is test data copied from the existing incompressible native probe strategy, not a tuning
value or resource allowance.

ROOT execution and interpretation

After source review, ROOT compiles this exact named leaf on its admitted remote Mac,
with a unique private output/root/cache/source and finite outer process/cleanup guard.
No laptop build is authorized. A reproducible compile shape is:

    clang -std=c17 -O0 -Wall -Wextra -Werror macos_setexec_preseal_control.c -o OWNED_BINARY
    OWNED_BINARY --memory-mib ROOT_RECORDED_NATIVE_BOUND

ROOT supplies actual absolute owned paths/needs/nice/jobs/headroom and source/binary/
SDK/OS/kernel hashes; those placeholders are not a lane dispatch or handoff to Ada.
Record complete stdout/stderr, all four case outcomes, frame/PID/birth/native stopped/
exit/reap/peak facts, parent limits unchanged and exact cleanup. Missing symbols, compile
failures, low-cap loader death or precondition errors are not an original semantic result.
Do not rerun the prior standalone NPROC pass or old escape original as part of this queue.
No production fork/SETEXEC remedy, no2ef intake, no Shards worker seal or native VM/GPU/
CPU correctness claim follows from source alone. Positive combination data and the exact
presealed closed-worker protocol are prerequisites to the next production source.

R01–R16

This is test/measurement code only, no product resource/terminal authority. R01/R04/R05/
R06/R07/R13: preallocated trampoline, finite readiness waits, typed private frames, explicit
sole ownership and outcome/cleanup distinction. R02/R03/R12: reviewed pinned primary path
and existing native invariants/values. R09/R10: exact actual06e red preserved; new control
UNCOMPILED/UNRUN. R11: macOS arm64-only fixture, not portable product support proof.
R08/R14: no performance/latency/allocation/fault/competitor claim; ROOT owns those campaigns.
R15/R16: source-only implementer, no jobs/native/extra agents/foreign process changes;
ROOT reviews, admits, executes and preserves raw plus unresolved native/profile/full gates.

Diagnostic successor after ROOT a6b2

ROOT's exact original C body compiled with Clang21 and the explicitly recorded installed
SDK26.5 (`macOS26.4` is the OS, not SDK equivalence). The first NORMAL child98027,
birth14402359308526, ended127 before NOTE_EXEC. The existing readiness frame was READY;
the parent retained EPROTO100 and positively reaped that exact child. Parent NPROC
limits stayed10666/16000. No mechanism passed and the later three cuts did not execute.
Full437-byte native log SHA-256:
`3c1fd9d0de1f872d5fc05c499753d6e6606a4630ec432e3f9e980fa745a40a49`.
The adjacent ROOT CLI receipt was read in full; removing its one appended framing
newline reproduces that raw length/hash. ROOT retains the original source and raw.

ROOT requested only observability of the already-written PREEXEC_ERROR frame. The
successor adds one nonblocking read of that SAME ready pipe if NORMAL/MEMORY lacks
NOTE_EXEC. It prints the byte count, read errno, framing/expected-child validity and
native operation errno. Partial/absent/malformed output is a missing diagnostic,
never an exec, ownership or success witness. It spends no additional waiting budget.
EPROTO100, stop-first-failure, the original39.507s deadline, cleanup and every mechanism
oracle are unchanged. No native errno or SETEXEC cause is inferred from the original
discarded frame. The successor is source-only, UNCOMPILED/UNRUN by L3, with no new job,
production code, native primitive, child, image, fallback or resource constant.

Unexpected-return ownership completion before replay

ROOT additionally identified an unexercised fixture ownership gap: SETEXEC can never
be assumed to replace the controller merely because that flag was requested. A
filter/implementation returning to this image might have created another child.
The combined successor passes a real PID output to posix_spawn. Any zero return or
positive returned PID enters a failure-only path. It validates the exact unreaped
direct-child relationship before a native PID/birth query, publishes the returned
PID/birth/query/spawn result immediately on the SAME private ready pipe, and keeps
the trampoline as that child's sole terminal waiter. Nonpositive/self/unknown output
confers no signal authority. Native cleanup still joins a fresh native birth before
any signal, then requires positive wait/exit/native-rusage/reap. Its outcome/status
is published separately; read, send, cleanup or deadline failure remains UNKNOWN.
An unexpected return always fails the mechanism, including when cleanup succeeds.

The existing cleanup algorithm is factored into a native-only core shared with its
original parent/image diagnostic wrapper. No formatting, heap, locks, destructor,
credential change, process enumeration or private syscall enters the trampoline.
The parent now consumes pre-exec receipts before waiting for controller exit, so a
late/hung child cleanup does not discard the immediate owned PID/birth evidence.
Pipe EOF continues to supply no exec-success witness: the original NOTE_EXEC,
same-birth stopped task, absent canary, explicit resume and all negative/terminal
oracles remain mandatory. There are still four cuts, no retry and one unchanged
39.507s absolute deadline; no native child is started by the implementation lane.

One native observer is created in the trampoline BEFORE any possible spawn. All
event storage is prepared before COW fork. The kernel queue descriptor cannot be
prepared in the parent and reused: Apple's pinned kqueue manual explicitly says
fork does not inherit it. The child creates/marks its own queue CLOEXEC and registers
the preprepared passive SIGCHLD event; errors publish PREEXEC_ERROR before any
foreign execution. This is the existing public kqueue/kevent primitive, not a new
worker or signal-handler/mask change. proc_pid_rusage is warmed on the parent before
fork and its pinned libproc body directly delegates to the proc-info syscall.
Existing read/write/clock/FD/proc/wait/kill wrappers stay native-only in that child.
Native call/queue behavior after fork still requires ROOT's actual matched run.

Primary pin `apple-oss-distributions/xnu@f6217f891ac0bb64f3d375211650a4c1ff8ca1ea`:

- [kqueue manual](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/man/man2/kqueue.2),
  23602B, SHA48302f5a7c664d9323dca51593253a30e7263161a6ea514fcb89cdcedd03d2a4:
  queue noninheritance and finite event timeout contract.
- [wait manual](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/man/man2/wait.2),
  8764B, SHA2178648ce5091cba439ca601216ec3f368ed64487f91385f68b65e7b5e5e5409:
  positive PID chooses a direct child; WNOHANG/reap status; an exiting parent reparents
  unreaped children, which is why this failure branch retains its waiter.
- [libproc wrapper](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/libsyscall/wrappers/libproc/libproc.c),
  28996B, SHA5c8a771427e04c5e1d55c4cdf5a077f3b52c5be4a091683b970084bfdda76df2:
  proc_pid_rusage directly invokes its native proc-info entry, without allocation.
- [public syscall table](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/syscalls.master),
  59004B, SHA7e866891439ad5b3464777d2cacc3b82fc2e7f439fb5a8686c3e6f160d5aeab5:
  kqueue/kevent public generated syscall entries. No private syscall is used.

This is a newly completed measurement failure-ownership path, not a reproduced
SETEXEC/filter behavior or a production release fix. Original9a587/909 source and
a6b2 raw stay unchanged. a6b2 established only its exact controller reap; it supplied
no all-descendant cleanup proof. The combined successor is UNCOMPILED/UNRUN, no new
unsafe Rust/SDK/native production change or2ef intake. Actual HVF/Vulkan worker
compatibility and a positively closed fixed member set remain separate prerequisites.
