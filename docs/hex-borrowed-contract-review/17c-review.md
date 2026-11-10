# Read-only follow-up at 17c99c58

Reviewed source: `17c99c58494c25ad6b0e35e0ad9891888bcdf632`.
Only `vendor/shards/crates/api/tests/hex_borrowed_contract.rs` was reviewed in
this follow-up. Its 373-line blob has SHA256
`b5916ff3383506df17fa572ca51c0218e6d3827945eaf2412a1fd186316d61cf`.
The original [8e review](README.md) and exact-base receipt remain unchanged.

Role: implementer `01a1c643e475709b8a829553d1100be1`; coordinator
`01a1aa77778471339bea3beff90d6bbd`. Current fully read policy remains
`a920c85b5c0c9d40aafc4f4cce32432c7c0aed79`, rules SHA256
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
The owned scope remains `docs/hex-borrowed-contract-review/**`. No source edits,
builds, tests, native invocations or jobs were performed by this reviewer.

## Improvements confirmed in source

- Lines 181–209 bind the private listener before testing both live and pid0
  identities with MAX and a non-MAX duration. The platform checked-add predicate
  establishes the latter's overflow; each case requires typed Bounds and no
  accepted connection. This addresses the missing finite-overflow/live-listener
  example.
- Lines 214–232 require CONNECT before closing the owned child's stdin, wait
  for actual successful child exit, then send OK. The result must be exactly
  `ProcessFailure::Exited` with the expected identity. This now challenges the
  final Held check rather than only pre-dial validation.
- Lines 283–310 require an I/O refusal with an allowed Ready phase and
  WouldBlock/Interrupted cause. They distinguish the returned Dial, Write and
  Read tags rather than treating every Borrowed error as equivalent.
- Lines 354–360 record the previously excluded first-round handle counts.
  Cold and steady observations can now be reported separately.

## Remaining interpretation limits

**The Read flag is a returned tag, not an independent call/byte witness.**
Lines 308–310 compute `read_phase_observed` from the refusal metadata. Lines
312–325 still permit no accepted connection, an empty prefix read, or a reset.
A zero-only shortcut returning a synthetic Read/WouldBlock tag could therefore
pass this branch without sending CONNECT. For an actual peer-side Read-path
claim, require the corresponding exact request/closure witness, or retain
uncertainty where native reset behavior prevents it. Otherwise describe the
measurement as a returned Read phase. A legitimate immediate Dial refusal is
still valid; do not force it into a Read claim.

**Cold growth remains observational.** Lines 354–360 print cold before/after
and equality; only the steady comparison at 361–367 fails the fixture. A cold
delta therefore needs an explicit uncertain classification in the collector or
result discussion. Passing steady rounds cannot establish cold resource return.
No arbitrary delta allowance or leak-free conclusion is warranted.

**A deadline miss is not post-handshake evidence.** Line 229 keeps the original
conformance patience. If owned-child startup/exit and delivery of OK consume it,
a compliant client can report expiration before reaching the final Held check.
Treat that result as a fixture budget miss, not a reproduced liveness defect.
ROOT's guard-removal run must fail for the relevant missing-Exited behavior;
neither an expiration nor an unrelated setup failure proves the mutation.
Do not change a production wait to obtain a passing fixture.

## Execution and provenance boundary

At this review, ROOT reported new Mac/Windows checks and the Mac Held-check
removal running. Their outcomes were not yet supplied or inspected here. The
8e native proof archives committed in 17c remain evidence of **8e**, not passing
checks of the new 17c leaf. The prior 107-handle result remains steady-only.
No new Windows compile blocker was identified by reading; this is not a Windows
compile or runtime pass claim.

ROOT owns native invocation, precise phase/resource interpretation, meaningful
mutation/restoration, later-source review and integration. This docs-only
follow-up introduces no production fix, platform exception or new mechanism.
