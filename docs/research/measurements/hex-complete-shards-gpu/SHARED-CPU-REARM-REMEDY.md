Shared CPU incarnation rearm — narrow fixed0f75 remedy, 2026-11-11

SOURCE ONLY / AFTER UNRUN. ROOT actual0f752b1078192ce4596021839118b80014dd25f6
compiled and selected the unchanged7e6 original:0pass/1fail/0ignore at its new-key
oracle, after every charge/controller-only/whole-scope/finalize/capacity/ACK
precondition. Full original log1687a4f32b70fb90ffd0e43228c914e3cda217d52579e8ac5894827bfe24664f
and raw e1f8d5b78a04a63e007981d450e3c54cf3e5888ac7192fc5384500d025b69159 were
read/rehashed locally. No owner rerun or native invocation occurred.

The existing once-set shared_pool key could neither be cleared nor bind a later
incarnation; release_pool returned its already-Released offer without ACK removal.
The remedy makes that SAME single generation binding resettable. It replaces its
OnceLock<Key> with Mutex<Option<Key>>, not another owner or resource/holder ledger.
All reads copy the small key under a short fallible lock. Bind permits only no key
or that exact key; clear permits only that exact key. No I/O, native call or user
callback occurs under this lock. Poison returns a typed refusal. Canonical Store,
Pool/device/method turns, native scope/receipt, RuntimeCost and Index stay authority.
The binding still fences changed generations during metadata-only preflight; it
is not used to infer physical absence or replace canonical generation checks.

Public release_backing first reads canonical phase. An already-Released shared
key enters the explicit ACK path. Other phases use the existing release_pool;
that cleanup continues to publish local Released, finalize the original native
owner, publish canonical Released, clear matching retained boot state and remove
its old local receipt. It does not clear the generation binding or prepare/boot a
replacement. A concurrent cleanup that later encounters Released still only
returns its offer. No implicit cleanup call becomes an ACK.

ACK takes the SAME original Pool turn, rereads exact key/execution/configuration,
requires Released and no retained workload reference, and checks any remaining
local receipt belongs to that same row and is Released. Its original native
finalization/removal is idempotent for the crash cut before local-file removal.
Only after canonical remove_pool succeeds is the matching generation binding
cleared. The next existing prepare_backing can therefore ask the SAME Store for a
fresh unallocated Prepared incarnation with unchanged configuration/cost; it starts
no VM. Old grants/ACKs cannot authorize the new key. Presence/error/reference
failure does not clear the binding. No timeout or retry is added.

The fixed0f75 original and every prior unit body are unchanged. One additional
source-only actual-runtime control stages a retained shared workload after the
old scope's positive model end and verifies ACK refuses with the old row/binding
intact. After the reference is removed, ACK/prepare yields a new key; replay of the
old key leaves that new canonical row, generation/preparation/removal counters and
all native launch/finalize/stop counters unchanged. Native outcomes are explicit
model inputs, not a native termination campaign.

Later full family join ports

- Keep3bdb/3896 family module, GPU preparation/current target/tuple/phase-aware
  cleanup and GPU release_backing else branch. Port only the new shared branch,
  key field/accessors and shared ACK method; do not copy these older whole files.
- In3bdb's factored backing_configuration and all other shared-key comparisons use
  shared_key()?. Keep its pure b924 expected-row/retained-reference validator and
  real PoolRecord return. No Books call is added under the family fence.
- Preserve ROOT's six-path/7Recover::route_for ports, original BoxInfo allocation
  and route,6fb fixture extensions/oracle, normalizer guard and existing class
  routing. Existing original charge/spec rewriting and fixed VM formula do not
  change.
- The fixed original cohort predates the required Tx8641/059752/70b364/431ca
  cancellation closure. It intentionally retains its previous fail-closed
  no-receipt unallocated Released handling; this patch makes no newly tested
  never-submitted cancellation claim. On the full joined family source, retain
  its exact cleanup-readable cancelled shape and private remove_pool native
  absence check. Shared Prepared cancellation must use the same Prepared-only
  native check as GPU cancellation once that real producer cohort is joined.

The native process controller is not scope emptiness. Mac06e's false-release
failure still needs L2/L3 fixed-worker source closure. Actual shared VM resource/driver
termination, measured overhead, mutation/concurrency/fault/restart effects and
supported-target proof remain ROOT validation. This source does not solve those by clearing a key.
Scoped rustfmt/parser and diff checks exited0; compiler/AFTER/strict/native checks
are unrun here. R01–R16 and policy7d7/df2 still bind this implementer checkpoint.
