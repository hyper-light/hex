# Linux managed launch source checkpoint

AC-T7.4.1; implementer L1. This is SOURCE ONLY, not a compile, native effect,
allocation-profile, recovery, performance or completion result. The public intake
cohort is ROOT `628591f7c356503161cbd617f2530c4e1d2821f4`: SDK receipt/helper
`0277c13a`, L2's complete guest subtree, and L3's concrete `Native` callbacks.
The sibling `linux-managed-source.json` names the exact source hashes.

Policy reread: current Shards `ca3ab10c151aa5539e757f1cc76029746ccbb561`,
AGENTS, development-rules R01–R16, development-evidence, CLAUDE; rules SHA-256
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
The full Hex CLAUDE, completion charter and L1 brief were reread. Existing scope
includes the specifically authorized held-exec functions in Linux process/sys,
the same Supervisor receipt spans, and Node backend/constructor/routing spans.

## Source behavior and authority

One Node owns its Store, method/device turns, normalized catalog and native
Prober. Books' direct native views do not call capacity or attachment offers.
`physical_report` reads that same provider; initialized absence is an authority
plus empty devices, and failed independent counters remain `None`. The old
scalar Facts load seam is unchanged pending its actual frozen red control.

Node stores initialized backend adapters after Arc creation. Its Books view is a
weak coercion of that existing Arc. Transaction callbacks are borrowed on the
stack and use L3's actual `transaction(store,id,devices,&Native,callback)`.
Firecracker/Shards requests and persisted records dispatch by actual class
before ordinary native cleanup or method turns. ROOT's full-BoxSpec render
signature and normalized route catalog are preserved. A resource pool id is not
assumed equal to the physical provider authority.

For an explicit Shards box, the original canonical Creating record and immutable
allocation precede native launch. The host cgroup uses its full admitted CPU and
memory charge; the original workload caps stay unchanged inside the VM. The
native owner reserves the SAME init-table/epoll slot and fallibly allocates a
typed triomphe Arc before child creation. The SDK's safe unsizing helper returns
that same owner. There is no second Process row, reaper, physical picker or
holder ledger. The per-VM phase lock is taken before spawn; no global table lock
spans spawn, identity reads, canonical writes or gate release.

The VM's absolute binary and prepared arguments are copied fallibly before spawn.
Its native helper cache is explicitly under the Node state (`shards-native-cache`
via `SHARDS_CACHE`); workload variables and caller credentials are not inherited.
The exact broader host renderer environment contract is still being reconciled
with L2; native success is unproved. The gate uses the initialized backend's
existing measured/learned `first_dial_bound`, not a new guessed timeout.

## Held-exec mechanism and alternatives

The ordinary exec/vfork implementation is unchanged. Freezing its cgroup before
that clone would wait for exec while the child is frozen. The new held launch
instead uses COW `clone3(CLONE_PIDFD|CLONE_INTO_CGROUP)` without VM, FILES or VFORK
sharing. Where clone3/into-cgroup is refused, legacy clone creates the pidfd
atomically and the child joins an already-open cgroup.procs before foreign exec.
Plain fork followed by a fallible pidfd_open is not used.

All C strings, argv/env pointer arrays, stdio and gate descriptors are prepared
before clone. A prepared CString conversion checks capacity equals length so
Rust's into_boxed_slice cannot hide a shrink allocation. The child uses only
prepared COW memory, stack storage and signal-safe syscalls, then execve or _exit.
It actually closes unexpected inherited descriptors before waiting: CLOEXEC
alone would let another held sibling retain the parent's gate writer, preventing
EOF cancellation after parent death. Its four retained descriptors are the three
stdio sources and gate reader; close_range has a bounded /proc/self/fd fallback.

The child sends byte 2 only after joining and isolating its descriptors. The
parent owns the pidfd before any fallible identity read. Native PID width is
retained until then. The canonical identity callback runs before the parent
accepts byte 2 and sends byte 1 to authorize exec. Both directions use the SAME
finite absolute monotonic deadline derived from the supplied remaining budget;
EOF, wrong frame, error, timeout or cancellation prevents execution. No gate wait
is a connection lifetime or process completion timeout. Exact frame bytes are a
private two-state protocol, not tuning constants. Timespec literals express
POSIX nanoseconds/second.

Primary sources inspected:

- Linux v6.19 [clone ABI](https://raw.githubusercontent.com/torvalds/linux/v6.19/include/uapi/linux/sched.h)
  and [fork implementation](https://raw.githubusercontent.com/torvalds/linux/v6.19/kernel/fork.c):
  atomic pidfd output, 88-byte aligned-u64 clone_args, cgroup and COW semantics.
- Linux v6.19 [cgroup v2 documentation](https://docs.kernel.org/6.19/admin-guide/cgroup-v2.html):
  process inheritance, scoped kill and populated notifications.
- Rust 1.99 [CString](https://raw.githubusercontent.com/rust-lang/rust/1.99.0/library/alloc/src/ffi/c_str.rs)
  and [Vec](https://raw.githubusercontent.com/rust-lang/rust/1.99.0/library/alloc/src/vec/mod.rs):
  conversion/shrink allocation boundary.
- The existing native descriptor sweep and cgroup events read determine the
  inherited 4096-byte getdents and 256-byte kernel-event buffers. Bounds reject
  overflow/truncation. The descriptor namespace itself bounds the finite sweep.

## Terminal and recovery boundary

Controller exit is cached by this same Supervisor, including an unreaped held
launch cancelled on error. Whole-scope release additionally requires that the
owned cgroup.events reports no populated descendants. `terminal` can report
Process while helpers remain; `wait_for` returns Some only for Released. Stop
does not wait behind the observation lock. Waits subscribe to pidfd/cgroup events;
the existing supervisor sweep handles cancelled held children if epoll setup
failed, without another timer or reaper.

L2 durably writes its selected receipt's Released fact/status before
`finalize_managed`; then the SAME matching native owner/cgroup is finalized, the
canonical row removed, and only then the selected receipt unlinked. No old
receipt may target a newly reused native owner.

**Required next cold-recovery increment remains open:** a reusable id/socket/PID
is not incarnation proof. Missing same-Supervisor receipt currently returns typed
Uncertain before cold cleanup, retaining claims. L3 owns the agreed canonical
BoxRecord NativeScope128/Store preparation helper; L1 will consume exact signed
`prepare_managed_scope` and `&BoxRecord` native callbacks. That scope must be
durable before selected boot Intent/native effects. This checkpoint is not a
claim of complete cold recovery.

## ROOT integration and checks still owed

ROOT supplies module/metadata registration; no lane manifest edits. Runtime
needs `pub mod firecracker;` (its platform implementation is internally cfg'd).
Node needs the direct triomphe edge alongside its already present shards-api.
The CLI keeps Arc<Node>, opens one Prober, calls `bind_prober`, constructs
configured adapters with `device_catalog` and `backend_books`, then calls
`install_backends` before recovery/serving. No second Recorded wrapper is used.
Firecracker configuration remains `Config::from_file` on the one explicit
`HEX_FIRECRACKER_CONFIG` path; it has no default budgets. Its existing concrete
adapter/full source and source-pinned upstream MSI patch still need the common
native receipt/cold-scope increment and initialized guest measurement admission.

Scoped rustfmt and diff checks ran locally, with auto-install disabled. No local
build/test, native ioctl, GPU open, driver change, process launch or VM effect ran.
Pure string/deadline unit controls were added but are UNRUN. ROOT's already
recorded five cost/client passes were not repeated. ROOT owns coherent compile,
all native gate ordering/parent-death/cancellation/fault/descendant escape tests,
seven-target checks and full integration.

ROOT's approved Linux451 read-only receipt reports KVM present/read-write,
NVIDIA 0000:01:00.0 in IOMMU13 on nvidia, AMD 0000:79:00.0 in IOMMU25 on amdgpu,
and neither /dev/vfio/13 nor /dev/vfio/25 available read-write. Physical VFIO
execution is therefore not proved or currently available without a separately
approved safe host scope. No driver rebinding is authorized here. BAR/config
metadata is not VRAM or a current sample.

R01 remains open for ROOT's inherited Supervisor BTree pre-effect insertion and
full native allocation audit; no postspawn Arc/map allocation was added. R02/03
primary sources and literal/deadline derivations are above. R04/05 state,
cancel/terminal waits and sole ownership are described but native proof is owed.
R06/07/12/13 source review, R09/10 actual controls/faults, R11 portability, and
R15 integration are unverified. R08/14 have no performance claim or measurements.
R16 preserves all source/evidence and does not change runners. A policy receipt,
format pass or source checkpoint does not close any of those required gates.
