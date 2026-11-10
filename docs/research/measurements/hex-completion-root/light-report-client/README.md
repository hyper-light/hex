# AC-T7.4.1: factual report wire and client integration

ROOT checked signed `696b94eff8575c0f046b2725cf9411b66859bcbc` in a new private
d132 arm64 macOS source/target/cache, admitted 1 CPU/4 GiB/12 GiB, requested nice
10, one Cargo job and one test thread. Four phases completed with exit zero:

| Phase | Passed/failed/ignored | Filtered | Cargo PID |
| --- | --- | --- | --- |
| Wire tests | 34/0/0 | 315 | 22897 |
| Physical report client RPC | 1/0/0 | 65 | 25038 |
| Append-only operation indexes | 1/0/0 | 65 | 25536 |
| API/core/client all-target strict Clippy | compile/lint exit 0 | — | 25538 |

The client control exercises an exact factual report, typed refusal, wrong-answer
rejection and no program-ledger growth. The index control checks 25 operations,
appending the physical report after Capacity without moving old indexes. The
wire run includes the corrected positional/unknown fixtures from4bbc. These are
model/socket harness results, not native runtime or physical GPU observations.
The nine reply-budget cases are a separate L4 run and are not claimed here.

All four recorded Cargo children and local CLI90369 ended and were reaped,
private temp was empty, and source manifests before/after matched every archived
input with no source changes. The independent data-only collector also exited
zero, reaped its CLI82733 and verified all members and source hashes.

The byte-exact compressed bundle is 185,375 bytes, SHA-256
`2a14c855eff5a54b2f477caaa5952deaf8f3d0c15fb2162a060e1a5aa787ec2d`.
It retains exact commands, result, log hashes, binary identities and source
manifests. The adjacent submission/collection receipts and scripts retain the
method, resource request and policy revision/hash. Original remote sources,
targets, binaries and caches remain intact. Earlier5e5 fixture failures remain
in `../light-report-initial/`; the newer4e7 BoxInfo schema is not covered by696.

Native attachment/compute, recovery and resource-return effects, complete backend
integration, supported target gates and final merge remain outstanding.
