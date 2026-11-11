# The existing Linux native owner accepts Box and Pool records

AC-T7.4.1; L1 implementer. Full Shards policy read at0c598386, current
`0a6d91b3de0f10a8397868aa3794adab1e09afdc` byte-compared equal in all four files;
rules SHA256 `df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
ROOT explicitly accepted the narrow Node/Supervisor OwnerKey/NativeRecord extension
and the bounded sorted-Vec table. The exact source/hash/dependency graph is in
`node-pool-native-source.json`. **SOURCE ONLY: compilation, controls and native
effects are unrun.** No local Cargo/build/test or additional campaign ran.

## One authority and original record

Node remains the CLI's single Arc and Store/Turns/attachment/Supervisor owner.
Books now implements pool_records, prepare_pool and with_pool_transaction through
L3's actual Store/common stack transaction. Both Box and Pool operations expand
Catalog conflict IDs before taking the SAME method/device exclusion turns. The
callback bundle supplies `Native.catalog = Some(self.attachments.as_ref())` from
the SAME initialized immutable TriArc; there is no catalog allocation, inventory
reopen, second Recorded wrapper or holder ledger. No transaction is boxed.

Borrowed launch/attach/finalize/usage callbacks take NativeRecord by value and
consume its actual class, stage, allocation, scope and commitment. The original
BoxSpec form/route/allocation stays in the same BoxRecord. Exact dedicated OCI
routes can launch their pinned VM backend; ShardsShared inner workloads cannot
launch a separate scope. Pool records use their real configuration/generation,
not a synthetic BoxSpec or fake BoxId. `managed_native_scope_id` encodes the real
128-bit scope solely as the existing cgroup path component; it creates no owner.

Node's Runtime backing methods read/check the retained PoolRecord before choosing
the initialized backend and before native turns. They neither recompute today's
capacity nor fall back to another backend. A changed generation/configuration
refuses. Existing returned BoxInfo projections and Node::render are untouched by
this increment; ROOT must apply node.rs's 13-line Runtime-method delta while
preserving its integrated full-BoxSpec render/attachment changes.

## Native ownership before resume

The Supervisor's SAME init and epoll-token indexes now key OwnerKey::Box/Pool.
Borrowed OwnerRef binary lookup copies no query key. The existing pidfd/run budget
still bounds live rows plus outstanding reservations. Both existing indexes
reserve their storage fallibly before any child creation; owned keys and the
TriArc receipt are prepared before the child. Vector insertion positions follow
the checked binary search and existing capacity. No VM process spawn, native
observation or Store write occurs while publishing/looking up its table row.
Ordinary Box ownership/reaping operations keep their existing sequence and
interface, with mechanical typed-key lookup changes; legacy ordinary allocation
and lock behavior is not certified by this VM increment.

Mutations shift O(n) rows; lookup is O(log n). This is a bounded source complexity
statement, not measured latency/contention/allocation evidence. No cache, second
process table, reaper, periodic polling loop or native global lock is added.

Managed receipts retain exact owner, NativeScope, immutable commitment and native
identity. Scope event descriptors open relative to the pinned cgroup directory;
create/get already pins that same directory. Cold absent-scope reconstruction
retains uncertainty and cannot obtain fresh usage credit through a path-only
BoxCgroup. Held exec, durable identity publication and gate resume remain the SAME
existing first-instruction cgroup/receipt path. Controller exit alone still does
not release claims: terminal proof requires whole-scope absence, followed by
canonical lifecycle/finalization. A stale generation/charge cannot reuse a receipt.

## Factual native usage

Supervisor uses L3's exact signed573e8671 `BoxCgroup::managed_usage`, through the
existing pinned descriptor/reader and strict shared core parser. Missing/read-error/
malformed/overflowing counters become the affected NativeRecord's typed Uncertain;
no unknown zero or inner guest RSS supplies host credit. The SAME Store holds its
reservation across that failure. Original `usage`/baseline behavior is unchanged.
The short-lived unsealed L1 cgroup proposal was removed before integration after
L3's explicit ownership message; its exact patch is retained privately at
`~/.cache/hex-complete-bindings/cgroup-managed-unused-proposal-v1.patch`, SHA256
`bf0f34f6a155274403c45f3e4ad2813837e8f2e91f881b8b26792eacd108c43c`.
It is not part of production or this checkpoint.

Two new pure controls cover shared Box/Pool budget/token lookup and unchanged Vec
capacity on publication; retained receipts reject altered generation/profile.
They create no Supervisor/thread/process/cgroup and are unrun. Scoped installed
rustfmt and diff checks pass. L3 supplies separate strict-reader controls; no tests
are repeated here. ROOT owns joined checks, real cgroup/boot/cleanup/fault proof,
R14 profiles/benchmarks, all full gates and main merges.

Remaining implementation: full initialized Capacity normalization/real Index-to-
native grant/measurement pipeline, read-only global retained-claim/backend preflight
before recovery effects, cold GPU observation/admission/handoff and L2's exact
signed report-pipe API carriage. Their separate source checkpoints are not replaced
by this primitive. Full FC/Shards/Docker GPU execution, cost measurements and
portability remain unverified; this is not a backend-completion claim.
