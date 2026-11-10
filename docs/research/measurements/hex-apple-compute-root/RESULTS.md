# AC-T7.3.1 / AC-T7.4.1: actual native Metal and API bridge results

ROOT coordinator Hex01a1aa77778471339bea3beff90d6bbd; Shards policy33df95b0,
R01–R16 rules SHA256df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
Only ROOT ran native operations and mutations. Implementer18eccf40 only compiled
the fixture; its compile evidence is retained in docs/hex-apple-compute.

Exact immutable ROOT source d3e936d12f89397715f8a3b34a4ca69160252f78,
Mac Studio M3 Ultra, macOS26.4/Darwin25.4.0. Required MetalregistryID4294968831
came from the earlier actual inventory; independent Metal enumeration and every
created resource's device.registryID agreed. Work is one round over one native
page (16384bytes / sizeof(u32) =4096 elements), three pages for two buffers and
oracle, plus unmeasured native compiler/driver overhead. Each native invocation
had an external151s guard derived from twice the slowest existing validation_run
record; none timed out. This guard is not a performance result.

| Actual invocation | Outcome |
| --- | --- |
| Selected original GPU kernel | 1 passed,0 failed,0 ignored; native command status4, no NSError,4096 correct outputs, wrapper references balanced. |
| Wrong native registryID9223372041149744639 | Expected101:0 passed,1 failed; missing/duplicate identity assertion before any dispatch/command, wrapper references balanced. |
| Mutation: GPU write replaced by preserving initialized wrong output | Expected101:0 passed,1 failed; native command still status4/no NSError, every wrong output caught by the CPU oracle, wrapper references balanced. |

Every output initially differs from its expected result. Only the mutated private
copy changes, exactly one shader assignment; the original frozen source remains
unchanged. No wrong-device submission occurred. The control demonstrates that a
successful command completion alone cannot satisfy the fixture.

The independent collector rehashed all original post-run source files against
signedGit archives and the mutation leaf against its recorded transformation.
It verified raw logs/exits/required counts and every Cargo/test child reaped,
private temporary directory empty. Full commands, PIDs, binary hashes and results
are in receipt.json and the lossless raw archive SHA256cded12ef996cf4b935a387466da3fe0d4ebc7c7a4cc3b1726386bca672cc5950.
The original and mutation binaries/trees/logs remain in ROOT's private cache.

Related exactdf9f6633 checks:1 Metal-key facts unit,1 GuestConfig/spec unit and1
strict environment unit passed; Mac CLI all-target and Linux ARM64musl node
Clippy passed. Public backend conformance reproduced0pass/1fail because its
stale WindowsARM exclusion disagreed with backend_of. Exact03f19b89 correction
then passed4 routing/backend units and WindowsARM64 CLI all-target Clippy.
No real WHP ARM64 VM was touched; the user's pause instruction persists.

This is actual GPU compute and fixture-reference evidence, not native OS/driver
leak freedom, process/container/VM attachment, exclusive physical binding,
IOReport load accuracy under work, comparative performance, allocations/page
fault profiling, or supported-platform matrix completion. Those remain owed.
