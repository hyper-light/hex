One Store family preparation boundary — source only, 2026-11-11

L3 implementer, policy7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506, rules SHA256
 df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
Current four binding blobs equal fully read0342. ROOT owns integration, admitted checks,
all native/fault/resource/profile/full gates and main. This source does not close Mac
scope/helper/CPU, final-charge or actual backend proof gaps.

Contract and exact dependencies

The required Books signature is L1 b924aec5b7dfb98e4ce6ce11a767c51c7b3a0d48. The
common helper is recorded::prepare_pool(store, pool, execution, cost, devices, family,
validate) -> Result<PoolRecord,Refusal>, in exactly that order. Family is
(&[u8;32], &[DeviceId], Option<&PoolRecord>); validate is a borrowed FnMut over the
SAME Records and &[PoolRecord], returning Result<(),Refusal>. No DTO/wire/new public
method/manager/cache/ledger or selector is introduced.

L1 Node consumer a5f22960acd5c9d98c69a48e1aa01a84860094a6 owns the initialized alias
expansion, existing F/full-alias method and attachment turns, then calls this helper.
Recorded uses its SAME initialized Catalog, expands aliases, and takes its existing
lifecycle.try_take_pool_configuration(F,full aliases) once. No new Turns state is needed.
L2 must join its actual configured-family validator and subset producers; do not compile
this unmatched method cohort alone or use an always-Ok production validator. ROOT keeps
all outside route/allocation literals, seven route_for calls and later physical-report
ports when applying these narrow Recorded/Store/pool hunks.

F is the existing full immutable configured hash; each proposed R uses the existing
set-bound formula. See full joint contracts at L2 d04c21f4c121111d54170e021dde897651bdac7b
and L1 2c6535f49f90e9290419e53549315ecd7364a71a. Those records pin official Kubernetes
DRA and Pod Overhead precedent and distinguish that precedent from Hex proof. This
owner does not compute a second family hash, inventory, workload selection or cost.

Atomic metadata span and error behavior

The Store already had one pool_preparation mutex in its shared Synced owner, covering
count-before-create. That same field is now a RwLock<()>: ordinary Box put/begin/made/
put_lifecycle/update/remove/remove_lifecycle/remove_strays and Pool grant/put/remove
enter shared access BEFORE their existing per-source/per-Box Store turns. Preparation
enters exclusive access BEFORE its Store configuration turn. All access uses try_read
or try_write; contention or poison returns a fallibly rendered typed Refusal::Os before
mutation. No new blocking wait, retry, polling or learned authority is added. Ordinary
writers can retain simultaneous shared access, preserving their separate record turns.
The callback is pure: no Store/Books reentry, provider/native call or asynchronous work.
Only bounded metadata reads, validation and durable metadata publication occur under
exclusive access; native drain/attach/finalize and Index acknowledgment remain outside.

Under that fence the owner reads one complete strict bounded Box snapshot and one sorted
Pool snapshot. Unreadable/malformed/duplicate/overbound retained files refuse using the
existing scanner. Box version/name/spec and canonical selected/conflict IDs are checked.
Every proposed and old-current device must occur in the full conflict set. The whole
expected current row must equal the snapshot, including generation, phase, original
cost/selection/allocation and introduced init/exit facts. None refuses a row already at
the proposed key. A conflicting preexisting proposed key also refuses before the callback.

The configured pure validator must prove actual F membership, deterministic R, initialized
profile/key agreement, at-most-one current, source-reference/ACK restrictions and catalog
bounds over all supplied rows. It cannot repick after a stale proposal. The global startup
adapter/config preflight is still separate; this helper does not replace native marker,
receipt or retained-configuration validation by claiming basic row checks suffice.

Return and transition contract

- Same proposed/current key: return that exact canonical row; differing devices/profile
  refuse. Move the row from the temporary snapshot; no deep infallible clone.
- Different R with expected current: refuse any consuming reference to the old configuration.
  Granted or Live changes only to Draining, preserving old key/scope/devices/full grant,
  init and exit. Persist it and return OLD row for actual native cleanup. Draining or Released
  returns OLD row unchanged. Prepared also returns OLD row unchanged: phase alone is not
  no-Intent/no-submission proof, and the adapter still owes its canonical cancellation proof.
- No expected current, after the validator proves no current/pending no-ref Released ACK:
  refuse any retained source reference to a missing proposed row, check the existing TURNS
  bound, then use the SAME existing fresh-scope/Prepared publication path. It grants no native
  resource or fresh observation. No entropy/native scope is created on stale/validator refusal.

A no-ref Released row remains an ACK cleanup queue entry; its phase alone does not authorize
replacement. Native positive whole-scope termination, durable local Released, guarded native
finalize including last-helper closure, canonical Released, SAME Index observe/refund, then
explicit existing release acknowledgment/removal are still the required order. Referenced
Released parents stay accounting-only/nonfresh and may coexist with a disjoint current R.
The configured callback, not a parallel owner/cache, identifies that family and ACK queue.

Research and alternatives

Official Rust1.99.1 source58cf8f9c35b355199727627d07ae34a4e2846d67 documents nonblocking
try_read/try_write, poisoning, and lack of an ordering guarantee:
https://doc.rust-lang.org/std/sync/struct.RwLock.html . Exact std/sync/poison/rwlock.rs is
48728B SHAfa19fbb8f0cd3c55f83f5f90d682821455170eccda1f3ef90088f02e04aaa81a.
Pinned sys/sync/rwlock/mod.rs is972B SHA800959dcab669de71206b5492b81deb2765a183d323e2807daefee35fa00ae19;
Linux/Windows use futex.rs13013B SHAe05bc3eb45129408d232e867ce9f4bed49fdeb3b8ea435f1585c976fc1a6dd01;
Mac arm64 uses queue.rs31562B SHAa2b1b14e4ecb9b66e8691680c09bfedefa0fadda7d90521e784ef859f5d760ca.
Inspected new/try acquire/guard release and their state fields at these pins. The queue
implementation replaced lazily allocated pthread locks; this path uses only try methods,
never its blocking/thread-waiter construction path. These are source facts, not allocation,
latency, starvation or leak measurements. The OS-dependent fairness guarantee is absent.
Primary bytes/manifests are retained under ~/.cache/hex-complete-lifecycle/research/store-family-prepare-v1.

Keeping the old preparation mutex alone misses direct ordinary writers. A new global
reference/epoch table would duplicate authority. Locking every Box/Pool turn from a stale
snapshot adds multiple acquisition/failure paths and still needs first-publication fencing.
A single exclusive mutex on every writer would unnecessarily serialize unrelated records.
Reusing the existing field with shared ordinary access is the smallest same-owner fence.
It can refuse preparation during active writes and ordinary mutations during preparation;
no throughput/fairness claim is made. No custom refcount/allocator/unsafe/dependency is added.

Focused evidence and remaining work

Five new private-file/model units are registered only in Store pool's family_preparation
module: strict expected-row + validator/stale gating; Granted/Live old-scope drain and
release/ACK ordering; unchanged Prepared without proof; paused snapshot versus ordinary
Box/Pool writers; malformed sibling preserving bytes with callback count0. The pause is an
explicit unit-test interleaving using bounded channels and the existing39.507s test hang
guard, not production I/O or a measured runtime reaction allowance. Native terminal facts
in these units are explicitly injected model state. No native VM/HCS/GPU is started.
All five are UNRUN. L3 ran scoped rustfmt and git diff --check only, both0; no compile,
Clippy, test or native dispatch. ROOT must run the matched b924+actual consumers cohort.

The separate original Draining-first-new-reference hypothesis is de2a92a116e39e97a6192055b9218cfea77fda90.
It and c9b originals remain unchanged; no semantic fix is included without ROOT's actual
original receipt. The prior source-reference/removal behavior is reused, with borrowed
snapshots and fallible malformed-error copying rather than an infallible clone.

R01 limitations remain explicit: existing Store path/listed/scanner BTreeMap insertion,
serde decode allocations, legacy Box encoding/diagnostic formatting and initial Store Arc
construction are inherited unresolved allocation sites. This is not panic-free/allocation-
safe completion of the whole Store. New selected storage and refusal copies reserve fallibly;
row reuse moves its existing Vec. Pure snapshots and metadata errors retain claims.
R02/R03/R05/R12: pinned contracts/alternatives and existing bounds above. R04/R06/R07/R13:
explicit branch/lock/ownership obligations. R09/R10: new feature controls UNRUN, separate
old original preserved. R11: all seven target compiler/runtime checks owed to ROOT.
R08/R14: profiling/benchmarks/allocation/fault/competitor targets unmeasured. R15/R16:
source-only implementer, no jobs/caches/foreign processes changed; ROOT owns integration,
full/native campaigns/evidence acceptance/main. No remaining applicable gap is marked passed.
