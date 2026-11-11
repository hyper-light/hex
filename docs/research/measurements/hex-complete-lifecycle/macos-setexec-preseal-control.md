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
