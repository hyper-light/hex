# Same-owner pool transaction and native producer checkpoint

SOURCE ONLY. No compiler, unit, HCS, GPU, VM or native campaign was run for this
checkpoint. ROOT owns joined checks and native proof; the released Windows and
Firecracker testers retain their separate fixed campaign sources. Earlier original
red/green archives remain unchanged and do not validate this new cohort.

Policy: implementer L3; origin/main 0c598386ca1f7c65c6e0634dba77983e4abffff2,
rules df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
The four policy blobs equal the previously fully read ca3 receipt. Full reread on
this resume is recorded in policy-0c.json. Production ownership remains the
acknowledged lifecycle paths; L1 owns Linux Node/Supervisor and devices declarations,
L2 owns guest code, ROOT owns manifests/CLI/shared registration.

## Concrete source contract

`Recorded` implements the exact Books/Transaction pool methods from L1 7e43f841.
One stack Transaction stores Box or Pool owner, the existing Store, declared device
turns and borrowed Native callbacks. There is no boxed transaction, Process row,
second Store, picker or holder cache. `Native` adds `catalog: Option<&Catalog>`;
Node must borrow the SAME initialized attachments allocation here. Launch, attach,
finalize and usage callbacks take `NativeRecord<'_>` by value, whose Box/Pool variants
borrow the actual canonical record. No native callback runs under a native-table
mutex or Store write turn. The outer method/device turn spans its completion.

Pool admission and reporting call the existing `Catalog::holder(id, boxes, pools)`;
Box admission calls its existing prepare operation. Thus group conflicts and exact
observer handoff have one implementation. Pool Granted/Live/Draining retains
Backing; Prepared/Released has no physical grant. A logical handoff to a Creating,
no-init Box is insufficient native permission: actual Box launch also requires the
SAME exact Released PoolRecord with positive terminal exit and matching route,
profile and full device set. Keep that row until the new held Box identity is
published. Pool draining updates use immutable canonical state, not fresh placement.

Store's first-pool count and publication are serialized in a Mutex embedded in
its existing shared Synced allocation; it stores no inventory/count cache. Only
configuration preparation acquires it, after the configuration Store turn. Normal
Box publication is unchanged. The existing TURNS bound covers the configured rows;
clones cannot concurrently pass count-before-create. Native effects never occur
under this lock. Exclusive file creation continues to fence a second process for
that exact configuration; multi-process Store authority is not introduced.

Windows and Mac use their SAME bounded native table keyed by `OwnerKey::{Box,Pool}`.
Their SAME fallibly allocated typed JobBox/ProcessBox owners implement ManagedProcess.
Pool configuration and fresh scope are distinct: the canonical generation and
commitment bind recovery/finalization; a table key or filename cannot establish
an incarnation. Public ordinary id getters are now fallible: a pool cannot be
exposed as a fabricated BoxId. Windows ordinary OCI consumers propagate that error.

Windows keeps atomic job binding, suspended controller, canonical publication,
keeper object commitment, PID/birth and CompareObjectHandles checks. The pool
keeper filename is disjoint from legacy held-<BoxId>; it still encodes the SAME
scope/commitment/keeper receipt. Finalization requires positive whole-job empty,
then exact pointer/keeper guarded disposal. Textual name absence alone never grants
cleanup. Mac keeps its existing held-spawn/watcher/controller-retention protocol;
this change carries Pool identity through it without changing memory cadence.

Windows ordinary native create/adopt/alive/discard checks committed Execution,
Windows image OS and actual target architecture before native effects. Slot route
projection joins the SAME native pointer after the native info call, with no lock
through that call. CPU-only HCS advertises its actual NativeWindows route even
without a GPU catalog; the guest augments that SAME Capacity. Device eligibility
bits are local snapshot positions, never persisted physical identity.

`Recorded::backing_usage(key)` reads the exact canonical PoolRecord, takes its same
pool/device turn and invokes Tx.managed_usage against the SAME native receipt. It
does not credit guest-inner counters or retain another accounting ledger.

## Exact dependent source and intake

- Parent a3571e0873e32e5b1bec6fdafbb9e962d513d9af: Store/Turns pool owner source.
- Core d83a1eb097088b2b14033f6159f11cf40c0da22d +27f6c910 invariant successor:
  OwnerKey/NativeRecord/PoolRecord (already ROOT imported).
- L1 7e43f84144b32873fea4f54b8c38aa05bbb33054: devices Books/Transaction,
  sole Catalog holder and API backing profile/full-set checks; includes its API
  predecessors. L1 42de0312055be903378213d4f6f869c1149095f9 adds Runtime::backing_usage.
- L2 703469ec96157f9a75388eb3e70b9699bcbe6b74 adds the SAME-catalog getter;
  its named guest/books.rs is a private dependency overlay, not an L3 edit or commit.
- ROOT SDK eff3/vendor0016 at 2c8855b2 supplies ManagedReceipt and RuntimePaths.
  No manifests/lock/vendor files are changed here.
- Recorded starts exact ROOT 9ccb495b75639e09c12985182177f37394eb3f8f plus
  L3 f58 projection. All seven ROOT route_for dispatches and typed Box holder ports
  remain. Windows/mod.rs starts the same exact ROOT 9ccb leaf. Mac starts L3
  4839+37b, retaining ordinary allocation/route None literals.
- ROOT must keep its canonical recorded_authority_tests.rs registration/5f fixture
  closure while joining; the new recorded_pool_tests.rs is required by recorded.rs.

Production hashes and fixture closure are in pool-transaction-source.json.
Intake named files/deltas into the matched cohort, retaining unrelated ROOT changes.
Do not float to a branch or claim the standalone SDK result is this cohort's check.

## Focused controls and remaining work

Six new narrow controls are source only, UNRUN: actual Store+Tx rejects pre-grant
usage, stale generation blocks all native callbacks/removal, native Box/Pool table
keys remain distinct, keeper paths cannot alias, CPU-only native Windows route is
present, and Mac typed uncertainty retains exact pool generation. These use counted
or deliberately forbidden callbacks, not a fake native success. Native scope-empty,
crash-before/after durable release, helper membership/escape, forbidden-effect and
same-ID replacement proofs remain ROOT/tester work.

Mac cold attach remains observation-only when it cannot pin native cleanup authority;
controller exit does not become scope Released. Mac aggregate hard CPU enforcement
is still incomplete and remains the next independent production task. It is not
reported as Unsupported completion. No new CPU constant or native call is introduced
here. See macos-managed-source.md and the pinned XNU research for those limits.

R01: new key/table copies and growth use try_owned/try_clone/try_reserve; receipt
owners stay typed TriArc allocations before native effects. Existing general Box
JSON parse/encode, diagnostic formatting, std thread internals and legacy native
constructor allocations are unresolved audit gaps, not allocation-safe claims.
R02/R03/R05/R12: reuse the already researched OS primitives and existing owner seams;
no new native primitive, timing constant, collection dependency or cache. Earlier
primary contracts remain in managed-native.md, native-scope.md, windows-cold-scope.md
and macos-managed-source.md. Synthetic Box ownership/parallel ledgers were rejected
by the explicit user contract. R04/R06/R07/R13: explicit canonical phase mapping,
immutable identity and exact guarded finalization; native tables retain only native
process/boot receipts. R09/R10: feature source, not a newly claimed reproduced native
bug; original artifacts are preserved. R08/R14: no speed/leak/resource claim; profiling
and comparisons owed ROOT. R11: supported target checks and actual native effects
unrun for this cohort. R15/R16: no dispatch, foreign process/data action, runner change,
main merge or push; scoped formatting/diff only. All remaining gates stay explicit.
