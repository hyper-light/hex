# Borrowed-connector source review at 8e5703b1

AC-T7.4.1 / HEX-RT-17. Reviewed exact ROOT source
`8e5703b13c8649c8cfbc95d6650163c173fad4ca`, also this docs-only branch's base.
The reviewed fixture is `vendor/shards/crates/api/tests/hex_borrowed_contract.rs`,
SHA256 `0439cbed32aade2ab8b19e106a87a6183a37a7da6d6dcb3bbe2c1b774c04cd13`.
Line numbers below refer to that exact revision, not later corrections.

Implementer `01a1c643e475709b8a829553d1100be1`; coordinator
`01a1aa77778471339bea3beff90d6bbd`. Full current Shards AGENTS, R01–R16,
development-evidence and CLAUDE were read at
`a920c85b5c0c9d40aafc4f4cce32432c7c0aed79`; rules SHA256
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
Hex CLAUDE and current handoff were also read. Scope: this fixture and the
`33df95b08ad8f95bddeaa1c48acf0c64386e0eac` client/Held contract, including its
existing focused contract examples. Only `docs/hex-borrowed-contract-review/**`
is changed. Builds, tests, native invocations, jobs and production edits by this
reviewer: **zero**. Findings are source analysis, not native reproductions.

## Accepted findings and warranted corrections

1. **Cold handles are excluded — lines 274–275.** A complete round runs before
   the baseline. A first-use-only retained handle can become part of that
   baseline and pass every later equality. Record a cold before/after cut as
   well as steady rounds; preserve and attribute any delta rather than warming
   it away or inventing an allowance. Aggregate equality is not per-handle
   ownership, allocator or leak-freedom proof.

2. **Only the MAX sentinel exercises overflow — lines 213–220.** The special
   `Duration::MAX` refusal does not exercise non-MAX `Instant::checked_add`
   overflow. Add a finite, non-MAX input whose unrepresentability is established
   by the platform's actual checked-add predicate. Require typed Bounds before
   identity/dial effects. This guards the separate checked-add branch at
   33df `client.rs:995`, not just its MAX filter.

3. **Target death occurs before a fresh dial — lines 171–175.** Both successful
   handshakes finish before target exit. This does not challenge the final
   Held check at 33df `client.rs:1029` (ROOT 8e `client.rs:1032`). Have the peer
   observe CONNECT, establish the owned child's actual exit, then send OK;
   require `ProcessFailure::Exited` and no returned stream. A deadline failure
   does not count as reaching this guard. Keep unconditional stream/peer/child
   cleanup and the explicit external guard. ROOT owns the independent guard
   removal and restored pass.

4. **Zero's typed branch is ambiguous — lines 226–245.** Any Borrowed refusal
   passes, as do no accepted connection and a zero-byte read (the empty prefix
   matches REQUEST). This can exercise an early Dial path without exercising
   the intended unanswered Read. Assert and record the actual Ready phase and
   permitted I/O cause; distinguish Dial from observed Read. A legitimate
   immediate Windows Dial/WouldBlock refusal is not itself a contract defect
   and must not be forced into a Read claim.

5. **Overflow lacks a live no-dial witness — lines 213–223.** The listener is
   bound only after the overflow call, so that call cannot prove no dial was
   attempted. Bind the private listener before invalid-input checks and assert
   zero accepts. Existing 33df `contract.rs:48` supplies that stronger shape.

## Old requirements and evidence boundary

The 33df client contract requires readiness-only zero, nonrepresentable patience
refused before identity/dial, one deadline beginning before identity and shared
by partial I/O, and post-handshake liveness before returning a stream. Borrowed
drop owns neither the target nor its socket directory. PID/start matching does
not authenticate the socket peer (`client.rs:973–984`).

Existing focused examples separately cover wrong identity/no dial, exact bytes,
clone/half-close, withheld and dribbled replies, malformed replies and overflow.
This new leaf adds public target/drop and repeated native-handle observations;
it does not replace those examples. A typed expiration alone is not an elapsed
deadline measurement. No new timing or resource result was measured here.

ROOT reported Mac 12 selected passes, proof prefix `9663496d`. ROOT separately
reported Windows build exit 0, eight canonical passes, the public lifetime pass
and 64 steady rounds with 107 handles before/after, proof prefix `42042ccc`.
Those are coordinator-supplied results, not executions or independently inspected
raw artifacts of this review. The Windows result is explicitly **not cold** and
does **not establish the Read branch**. Original Windows 1-pass/7-fail bind/pair
InvalidInput logs were retained; ROOT attributed them to its overlong TEMP and
reran the same compiled binaries with a fresh short private TEMP. No production
fix or Windows compile defect is inferred from that harness failure.

ROOT accepted the findings and owns the new tests, cold observations, exact
typed witnesses, native invocations and mutation/restoration. This review does
not certify a later SHA, all platforms, allocation/resource return, HCS/GPU,
VM behavior, performance or final gates. Earlier native fixture refs `13fbf1ba`
and `eefaa97d`, their raw failures/evidence and all prior lane refs remain intact.

R01/R03–R07/R12/R13: no runtime or test mechanism changed. R02/R09/R10: pinned
source observations and separately attributed reports, no reproduced-production
bug claim. R08/R14: no performance/profiling or leak-free claim. R11/R15: bounded
source review only; ROOT owns actual platform/effect validation and merges.
R16: no runner, target, cache, process or native resource was created or removed.
