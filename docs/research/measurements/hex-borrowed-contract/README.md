# Current borrowed connector contract

ROOT Hex coordinator01a1aa77778471339bea3beff90d6bbd. Full current Shards policy
at a920c85b5c0c9d40aafc4f4cce32432c7c0aed79 read; R01–R16 hash
df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.

The peer reviewed an older18,736-byte c55 patch. That artifact is retained as an
obsolete proposal, not a current merge request. Current vendor33df already uses
Ready for zero, rejects MAX/overflow before identity/dial, starts one absolute
deadline before identity, and shares the CONNECT/OK path with remaining-time
updates for each partial I/O. Public owned Guest::connect no-clock compatibility
remains separate. VendorAPI source is unchanged through upstream a920; Hex's
three prior patches concern WindowsARM recognition, sharedGPU enable and stale
backend conformance, not borrowed production behavior.

Existing canonical upstream tests cover zero-withheld on Unix, overflow/no-dial,
identity mismatch, withheld/dribbled answer, bytes/clone/half-close, typed replies
and raw readiness. They were previously compiled, not selected by Hex's GPU API
check. ROOT must execute them now; source inspection alone is not a passing gate.

The new ignored public API test file adds a real owned test-process stand-in:
stream+clone drops leave its PID/start and socket directory alive, a reconnect
works, then the explicitly ended child yields a Process refusal before dial.
The child is this same test binary, requires an explicit fixture environment,
and waits for stdin EOF; no Guest/adoption/watch/export or real VM is used.

Windows-only ignored repeated test requires HEX_BORROWED_CASE_ROUNDS, positive
and at most64 (the u64 identity bit width). ROOT supplies the workload, one
wrong-start bit per round. After initializing each path once, every complete
round must return exactly to GetProcessHandleCount baseline: wrongstart,
missing socket, MAX-before-identity, zero-withheld one attempt, and successful
connect+clone+drop+server join. Query/synchronize handles and sockets have no
permitted drift. A held reply is never answered before zero returns. A Windows
connect that is still pending may refuse during Dial; no forced Read-phase
assumption replaces readiness semantics. No absence, timeout or skip counts as
pass. Root external whole-invocation guard is mandatory for all blocking fixture
operations and child cleanup. Native OS/kernel leak freedom is a separate claim.

Microsoft GetProcessHandleCount/GetProcessTimes contracts were read from official
Win32 documentation. The pseudo-handle is borrowed; each Child handle remains
owned for its query. MacOS/libproc start-abstime and Linux proc/stat starttime
match the existing API identity formats, independently queried for the owned
fixture. Every minimal test-only unsafe block states its native ownership/ABI.
No new production constant, API, dependency, watcher, cache or native import.

ROOT native results at signed8e5703b13c8649c8cfbc95d6650163c173fad4ca:

| Host | Selected behavior | Actual result |
| --- | --- | --- |
| Mac26.4 arm64 | Canonical borrowed cases |9 passed,0 failed,0 ignored.|
| Mac26.4 arm64 | Owned zero/MAX compatibility |2 exact cases passed.|
| Mac26.4 arm64 | Public child/drop/dead-target lifetime |1 exact ignored case explicitly selected and passed.|
| Windows26200 amd64 | Canonical borrowed cases |8 passed,0 failed,0 ignored.|
| Windows26200 amd64 | Public child/drop/dead-target lifetime |1 exact ignored case explicitly selected and passed.|
| Windows26200 amd64 | Repeated failure/clone/drop handles |1 case passed;64 complete rounds each exactly107 handles before/after. Steady-state only.|

Independent source/log readers verified all1869 original source files against the
signed8e Git archive. Mac proofSHA2569663496d8a25659f292def7abe996b2e503e58de32b0528f6d007ee84e0a9ae1;
Windows proof42042cccab479be156702cd0cf3866d6d1494f2542be7aee8d160d4c63db0f88.
All recorded children and collectors reaped, private temp entries0. Each registry
package/checksum remains a subset of the exact33df lock seed. Sources, lockfiles,
logs and receipts are in the accompanying proofs; drivers are retained here.

The initial Windows long-temp attempt built successfully, then reported1 pass/
7 failures with InvalidInput at AF_UNIX bind/pair, before those connector behaviors.
Shortening only its private temporary path made the same immutable binaries pass
all8 canonical cases. The next attempt selected0 owned-zero tests: those upstream
compatibility cases are Unix-only, so Windows owned compatibility remains unverified;
it is not included in the10 passing borrowed cases. No timeout was raised and no
production code was changed for either setup/control failure. Both raw attempts
are retained. These cases use controlled muxer sockets and an owned test-process
stand-in, not an actual VM or device assignment; no general native leak or speed
claim follows from steady-state handle equality.

Read-only adversarial review identified missing finite non-MAX overflow, target
death between CONNECT and OK, first-use handle accounting and precise zero-readiness
phase classification. The follow-up source adds those tests/observations. Its
new results and the post-handshake-guard mutation are PENDING, not covered by the
8e proof. Cold handle deltas are explicitly reported; a nonzero cold delta remains
uncertainty requiring attribution, never an allowed drift or a leak-free claim.
Linux native and seven-target combined checks remain unrun: approvedadacachy2 is
offline andadacachy currently has no admitted idle CPU. No Fedora/laptop checks,
runner restart or unowned process/VM changes. ROOT retains integration, native
mutation/effects/fullgates, profiling, benchmarking and merge responsibilities.

The follow-up17c99c58 built both native targets successfully but direct API Clippy
failed on conformance.rs:88 single_element_loop, before new native tests ran.
That loop came from Hex0003 narrowing the excluded set to Intel Mac. The small
0006 correction checks the same pair directly, without a lint allowance; no
backend or borrowed semantics change. Raw red runs remain in their private
source/target roots. New native tests/mutation and corrected Clippy are pending.
