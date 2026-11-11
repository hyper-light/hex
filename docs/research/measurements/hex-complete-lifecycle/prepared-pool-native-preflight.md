# Prepared Pool no-submission callback

This checkpoint supplies the common L3 producer for ROOT's required
`Transaction::check_unsubmitted_pool() -> Result<(), Refusal>`. It is source only.
It adds no native call, absence inference, terminal result, resource refund or VM
launch. ROOT must join the required trait, real native producer and caller before
checking it.

The existing concrete stack transaction rereads its canonical Pool row through
the SAME Store. It verifies the exact generation and owned physical conflict
keys, then accepts only `Prepared` with allocation, init and exit all absent.
Wrong owner, missing row, stale generation, malformed row and every later phase
refuse before the borrowed native callback. The caller already holds the SAME
configuration/device turns. No Store turn or shared native table mutex spans
the native metadata read.

`Native.unsubmitted_pool` borrows a synchronous callback on that stack. Recorded
supplies its existing `Recover::preflight_managed` adapter. WithGuest explicitly
forwards this native check to its initialized host, preserving the SAME authority
instead of falling through to Recover's typed refusal. The Windows and Mac
concrete Recover implementations already forward to their native owners. This
checkpoint changes neither native implementation nor the independent global
recovery barrier.

The required trait is L1 `8641a1d08b2baa5e2a7938df747466a9d38bb393`.
The real Linux Node/Supervisor producer is L1
`1aee0be8d8d9bbcb4fbb2fef43f30aa5fb4203ee`: exact owner metadata presence
refuses; after dropping that short table guard it checks only the pinned configured
root and exact native scope path. It does not run a whole-root singleton scan or
reject an unrelated retained sibling. Its presence/error/private-file controls
are separate from this common callback's counted controls.

On the existing ROOT Windows adapter, Prepared has no native commitment: any
retained native slot or keeper record refuses, and metadata read/lock failures
propagate. On the existing ROOT Mac adapter, any retained native owner also
refuses this unallocated shape; absence is only its metadata half of the backend's
independent no-Intent proof. Neither result proves that a submitted VM ended.
The unimported persistent Mac helper source `2ef` is not a dependency or a validated
replacement here. Its eventual matched producer must preserve these exact checks
and the independent native-scope uncertainty.

L2 must still join the canonical configuration/generation and its real no-Intent
proof under the SAME Pool transaction before cancellation publication. This
method never writes Released itself. The later Index ACK/removal of an already
cancelled unallocated Released row is a separate contract discussion; this
checkpoint deliberately keeps the public method Prepared-only.

## Focused source controls

The new leaf `recorded_unsubmitted_pool_tests.rs` registers four units:

1. An exact Prepared row plus an ordinary retained CPU sibling calls only the
   scoped callback once, preserving the raw Pool bytes, canonical rows and fsync
   counters. Launch, attach, finalize and usage callbacks panic if reached.
2. A positive native-presence refusal and an actual typed I/O refusal from the
   callback propagate separately, retaining the same bytes and Prepared state.
3. Granted before init, Live, Draining and allocated Released all refuse before
   native callback entry. The fixture introduces init while Granted, then advances
   through the legal existing transitions; it does not bypass Granted-to-Live.
4. Stale/missing generation, ordinary Box ownership and unallocated Released
   refuse without entering the callback or altering the canonical owner.

The six old Native literals in the Pool/report/handoff leaves receive only the
required panic-if-called callback field. Their prior assertions, fixtures and
production originals are retained at the parent pin. No old failure oracle is
weakened. This is a new required feature, not a claim that any native defect or
existing removal defect was reproduced. The original Draining reference control
and ROOT's actual 740d after-pass remain distinct.

ROOT's focused selection after matching producers is
`recorded::unsubmitted_pool_tests::`; all four units are UNRUN by L3. No local or
remote compilation, tests, native calls or new job were dispatched. Scoped
rustfmt and `git diff --check` are source checks only.

## Policy and remaining evidence

L3 implementer read full current Shards policy at
`7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506`; AGENTS, R01–R16, development-evidence
and CLAUDE bytes equal fully read `0342b85d9c808e3ced041a6c3ef0e9bf70438272`.
Rules SHA-256 is `df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
Hex charter/lifecycle/CLAUDE were reread at this resumed source boundary.

R01/R04/R06/R07/R13: no new container, owner, unchecked numeric operation, clone,
scope synthesis or native handle is introduced. Existing Store decode/path and
diagnostic allocations remain existing audited gaps, not allocation-safe or
leak-free claims. Ownership checks preserve exact phase/generation and the native
refusal. R02/R05/R12: reuse the existing native metadata adapters and SAME stack
Tx; there is no new OS mechanism, cache, waiter, timeout, polling or manager.
R03: no production constant is added. Test numbers identify fixtures only.
R09/R10: new feature controls are source only; no native bug/pass attribution.
R08/R14: no performance claim, allocation profile or comparative result. R11:
matched Linux/macOS/Windows compilation and actual native metadata behavior are
ROOT's remaining work. R15/R16: no new job or runner mutation; signed named-path
source only, preserved prior evidence, no main merge or push by L3.

## Accepted unallocated cancellation ACK follow-up

ROOT subsequently accepted private reuse of this SAME borrowed callback for the
explicit Index ACK of a canonical Released Pool with allocation, init and exit
all absent. This represents a never-submitted cancellation, not a submitted native
scope becoming empty. The public `check_unsubmitted_pool` gate remains Prepared-only.

The existing concrete `Transaction::remove_pool` first checks its exact owner and
generation. For only that exact unallocated Released shape it invokes
`Native.unsubmitted_pool` before Store removal. Positive metadata or any read/lock
error returns the native refusal without writing/removing. With a successful
metadata check, the existing Store removal still rechecks the canonical generation,
Released phase and strict complete Box reference scan under the SAME metadata
authority before unlink. No source turn is held across native I/O. Reused generations
and concurrent reference publication cannot bypass the existing canonical check.
Allocated Released follows its existing adapter-finalize/native-positive-proof path;
this change does not reinterpret exit or allocation fields as native proof.

The exact scoped Node native successor is
`431ca4173ade1c26eef7b2860e9f14af48912455`. Its private metadata checker accepts only
the two unallocated Prepared/Released shapes; public method dispatch remains separate.
The concrete Windows/Mac Recover adapters keep their existing metadata checks. No
new SDK method, native call, receipt, resource row or cache is added.

One appended counted Store/Tx control observes Prepared check -> cancellation
Released -> public Prepared method refusal -> positive-presence/error ACK refusals
with unchanged raw bytes/fsync counts -> exact successful ACK removal -> fresh
generation -> stale old ACK refusal before callback. The unrelated CPU sibling
remains unchanged throughout. The original four unit bodies are byte-identical to
`0597523f90fdc665e4f1d3393665544d47443bdf`; the added control and all native behavior
are UNRUN. This is the new family-cancellation feature contract, not attribution of
a reproduced existing removal defect. ROOT owns matching the L1/L2 producers/callers,
the five common controls, native absence/cleanup proof, strict lints and full gates.
