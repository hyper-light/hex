# Charged bootstrap control deadlines (AC-T7.4.1)

Fixed ROOT source parent: `807168c38996fdfda096c0ae9c748bb4167777ec`.
Implementer policy: fully read Shards `7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506`,
rules SHA256 `df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`;
four policy blobs unchanged; Hex CLAUDE byte-identical to the fully read e852.

Core adds `capacity_before(Class, Instant)`, `physical_report_before(Instant)` and
`create_before(BoxSpec, Instant)`. Each validates the original absolute instant
and uses the SAME existing `call_before`, whose smaller configured and learned
waits retain precedence. No value is stored as a connection lifetime. Capacity
and physical-report answers retain their ordinary `check()` validation, typed
refusals and wrong-answer rejection. Create retains the old OCI init validation
and Info matching; the old create body is shared with an optional cap, moving
its existing request allocation without adding a second allocation or owner.
The ordinary Runtime::create calls that path with None. Runtime trait, wire,
DTOs, native provider, picker and ownership are unchanged.

L1 owns consumption in FC native.rs/pool.rs, passing the SAME overall creation
Instant through constructor, capacity/report and create. Staging remains its
existing operation-scoped StagingStream contract. A timeout or refused cleanup
never proves a Box/native scope absent. This checkpoint does not certify stage,
VM startup, driver identity, workload completion or native release behavior.

This source is independent of the already signed 5c10 constructor and 2a1b Drop
patches; ROOT composes exact deltas without waiting for this lane's later owner
ports. The original budget source, including the 6753 regression, is retained verbatim
as a prefix; two focused wrapper controls are appended. Builds/checks/tests/native by L4: 0.

ROOT review successor: metadata queries now retain their actual single effective
caller/configured cap through transport and validation, checking remaining
authority before returning Capacity/PhysicalReport. Their shared private
plain_before goes to the same Conn::call_before; no cap is recomputed after
the answer. A smaller learned transport wait still applies as before.

A caller timeout leaves **receiver-side creation uncertain**. It is evidence
neither that creation stopped nor that the workload never ran. The existing
Call::Create Box allocation is an explicit inherited R01 allocation gap. Moving
that old operation into the shared path does not fix it, and this patch makes
no blanket allocation-safety claim. No receiver cancellation or release is
inferred from a client deadline.

The exact two new names and hashes are in controls-source.json. All three
wrappers are exercised against the existing Offers/Stalled memory transport:
expired-before-send, exact decoded request, valid typed response roundtrip,
wrong-answer refusal, typed remote refusal, and actual unarmed/no-relative-cap
answer expiry. No model reimplements the public API. The actor case captures
send/result observations, ends only its own Shared connection, and joins before
assertions even on a missed observation. Existing 25 ms caller / 100 ms external
fixture policies are reused without changing prior limits. These controls do
not inject validation delay, run a receiver VM, or establish physical cleanup.
The original Drop test is unchanged and must retain its separate original/red
attribution; these are only two new selected controls, not an old-suite replay.
