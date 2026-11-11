# Current result

Current signed control source is `98b638447fa119811bfc5d2baf9ecba5c811bc95`,
using ROOT's exact API `0277c13a654290fb94429aaa3d53aa0c7643dc43` in the private
closure. The leaf SHA-256 is
`1f21f609ccf63f3bcbb2178098b09cae91c0bf86263005c65070eeee43e026b1`.
The original c23/4f source and every failed dispatch/compile remain preserved.

The original no-answer submission was resolved by an admitted exclusive-directory
fence before its driver entry point. The c23 API library then failed before test
compilation. After ROOT supplied 0277 and fallible receipt ownership, all four new
controls passed. Direct test-target Clippy failed in the unchanged ABI probe,
before reporting any fixture lint result. The remaining Clippy dependency belongs
to ROOT/L2. The four passing controls will retain their exact cohort attribution.

| Cohort | Compile | New controls | Direct test Clippy | Cleanup |
| --- | --- | --- | --- | --- |
| Original no-answer submission | Fenced before driver entry | 0 executed | Unrun | Exclusive directory fence succeeded, exit 0. Earlier no-answer records retained. |
| Exact c23 API workspace plus 4f owned leaf, private `185f8dd1` | Failed 101 in API `sim.rs` | 0 executed | Unrun after compile failure | Cargo 32454 and local CLI 13635 reaped. Source unchanged and temporary entries 0. |
| API0277 plus leaf98b, manually pruned lock, private `b0baf4ac` | Lock refusal 101 before Rust | 0 executed | Unrun | Cargo 36375 and CLI 56939 reaped. Source unchanged and temporary entries 0. |
| API0277 plus leaf98b, full reviewed seed and Cargo-generated lock, private `85df57ba` | Passed 0 | **4 passed, 0 failed, 0 ignored, 0 filtered** | **101**, ABI `gpu/probe.rs` lines 68/101 | Every recorded child and CLI reaped, source/binary unchanged, temporary entries 0. Metadata ran once. |

The four selected names and their behavioral oracles are in README.md. L2's 5d8
clone/scope/inline-stop tests retain L2 attribution. Earlier 05c/e29 client budget
results retain their original source attribution.

## Actual four-control cohort

The actual private path was
`/Users/adalundhe/.cache/hex-complete-managed-api/api-98b63844-85df57ba`.
The archive was 191659 bytes with 48 files, SHA-256
`0c2e7fc4ca867cb74edd8b404ced8eb1b561d77858dc14567d7c9fc6e20244a7`.
Its input is exact API0277 and leaf98b plus ROOT's complete reviewed 5bc1 lock seed.
The preparer and continuation source are signed at 00d60024 and 987faa72.

The metadata command completed with exit 0. My initial post-check then raised
`KeyError: checksum` on the seed's unused Git packages `hyper-quic`/`hyper-tls`.
That original traceback is retained. No Rust compilation had started. An admitted
read recovered the successful metadata journal and generated lock. The corrected
validator requires every selected package to have a registry source, checksum and
exact reviewed tuple. The same private job resumed after metadata. An exclusive
resume marker prevents duplicate compilation. Metadata was never repeated.

The generated lock SHA-256 is
`682818b7666624a04fd15ea10cc25a86c104cdd6e55a483c8f2ada7896b888e0`.
All 15 registry name/version/source/checksum tuples match both the original 5bc1
seed and ROOT's corrected 2af7 seed, SHA-256
`1e9ae2e5e95e103c27bce8e0f7294900984825d22c9926f770c63c872d547386`.
Its `unsize` dependency edge names the already reviewed `autocfg` 1.5.1. Static
source hashes were identical before compilation. The compiled and final source
manifests are byte-identical, each SHA-256
`16791032680ac4a2187b12989f673e3a09c0b6283411000c11a891484c5f899c`.

The immutable copied test binary SHA-256 is
`09df081b7314b626ecc670b2e4e7e0a80e1bcac65f35a033f2c6adfd784289b9`.
Both compiled and copied binary hashes remained identical after execution.
The four controls ran once on macOS arm64, Darwin 25.4.0, Rust 1.99.1, through
admitted d132 with 1 CPU/4 GiB/12 GiB, nice 10, jobs 1 and test threads 1.

| Recorded child | PID | Exit | Log SHA-256 |
| --- | --- | --- | --- |
| Metadata | 37224 | 0, reaped | `219baebbf6129f63b9d97901c66ddcf0b5e5594943a34a2ff6bb232be6722711` |
| Selected test compilation | 38972 | 0, reaped | `be620660e1f413dc3b5066c721c85a730deeb30dd3e0712cb09c6925530c213a` |
| Four controls | 39120 | 0, reaped | `ed6025cd98093b89befd65a6b293691027807c5904abf67dc6367a01bc5b4ec1` |
| Direct test-target Clippy | 39138 | 101, reaped | `17b9d0379bc0ede2f21ff457703a75ca8676380034dd59d7defd40854759e6b3` |

Initial metadata CLI 69099 exited 1 at the post-check exception. Continuation CLI
98299 exited 1 after collecting the actual Clippy failure. Both are reaped. No
command timed out. The fixture's raw log records all five receipt destructions,
all supplied native writer closures and no forced cleanup. Uncertain observation
remains uncertain. Foreign attach remains pending with zero stops. The terminal
case consumes the actual API marker while native writer copies remain open.

`actual-0277-98b/` retains full metadata/compiler/control/Clippy logs, exact command
arguments, input/compiled/final source manifests, generated lock and member hashes.
`metadata-postcheck-recovery/` retains the original exception and independent
recovery. No native/backend proof is claimed by this API model/pipe cohort.

The Clippy findings are `chunks_exact_to_as_chunks` at ABI `gpu/probe.rs:68` and
`collapsible_if` at line 101. ROOT and L2 received the exact actual result before
any outside-scope edit. A fixed signed ABI cohort is required for Clippy-only
continuation. Production API/ABI and shared Cargo metadata are outside this lane.

## Source archive and lock

The source archive contains 48 files and is 169045 bytes, SHA-256
`cc9dce5f65605bc7a3e8cf4d02af6546fe73555569e18dd45df3897566ccf9a4`.
It is retained at
`/Users/adalundhe/.cache/hex-complete-managed-api/submissions/source-4f466f45/source.tar.gz`.
`source-4f466f45/closure.json` records every file's source pin, path and hash.
Its registry package tables are the exact reachable subset of upstream bb763's
lock seed. The complete lock seed SHA-256 is
`1ecb41e738c4c4133a1968873e424efce5b2a65899dd8f40f37ac6aaffec8133`.
Cargo accepted the exact generated lock with `--locked`. The complete source
manifest, including that lock, was unchanged after compilation. There was no
compiled test executable.

Archive generation used `/opt/homebrew/bin/python3.12` on the local machine for
source export/hash work. The initial system `python3` invocation failed importing
`tomllib` before creating its destination. The supplied preparer requires Python
3.11 or later. No local Rust build/test was involved.

## Original dispatch and recovery

Original job path is
`/Users/adalundhe/.cache/hex-complete-managed-api/api-4f466f45-bb1cbe82`.
Local raw receipts live under
`/Users/adalundhe/.cache/hex-complete-managed-api/submissions/api-4f466f45-bb1cbe82`.

| Action | Actual outcome |
| --- | --- |
| Exact archive submission, CLI PID 87639 | Reaped, exit 1. `d1323898b255a3ed435eb83fa0f7b0cf did not answer in 15s` at 2026-11-11T00:19:24.750766Z. |
| Same private-path inspection/exclusive fence, CLI PID 94596 | Reaped, exit 1. Same no-answer error at 2026-11-11T00:20:07.485591Z. |
| Connector inspection/exclusive fence, `sync=false` | Runner admission reported room for 1 CPU, 4 GiB memory and 12 GiB disk. Same 15-second no-answer error, empty output. |

Both original CLI commands used `--needs cpus=1,mem=4g,disk=12g --no-sync`. Connector
recovery used the same explicit needs and `sync=false`. Only the already owned
job path was addressed. Raw logs, exact CLI arguments and the connector receipt
are preserved in `dispatch-no-answer/`.

The remote driver's first action is exclusive `mkdir(exist_ok=False)` on its
unique root. Recovery attempts that same operation. A successful recovery mkdir
fences a delayed original command before archive extraction or checks. If the
original mkdir wins, recovery only inspects its receipt files. No successful
remote fence or inspection receipt had arrived at the initial evidence checkpoint.
The subsequent connector recovery succeeded with exit 0. Its exclusive mkdir won
before the original driver could enter. This proves the original command cannot
start any checks later. `dispatch-no-answer/resolution-success.json` retains the
exact command, admission metadata and successful raw receipt.

## Actual production compile failure

The fresh job was
`/Users/adalundhe/.cache/hex-complete-managed-api/api-4f466f45-185f8dd1`.
It used the same signed 4f leaf and exact archive, fresh source/target/cache/temp,
and the originally authorized d132 envelope. Cargo PID 32454 returned 101 without
timing out. Its complete driver receipt lists no executed test binaries. Clippy
was not dispatched after that compilation failure.

The compiler reported E0308 at `crates/api/src/sim.rs:327`. `process_exited` calls
`lock(&self.shared.state)`, while the helper at line 169 requires `&Shared`. The
unchanged c23 API library therefore fails before the new fixture compiles. The
same compile also emitted two `fetch_update` deprecation warnings in managed
client code at lines 1565/1663 on toolchain 1.99.1. Their direct Clippy outcome
remains unrun.

The exact compile log SHA-256 is
`7c82d11bf75ec1f8a8a69197d8530940f6c63985261ff1e76224f74034324dad`.
`original-c23-compile-red/` retains the full log, driver result, before/after source
equality receipt, exact submission arguments and raw transport output. Its
`SHA256.json` records the retained member hashes. The generated lock SHA-256 is
`01a47a8fba5c5ffe56b16b71d406fa77fed43dcf3726b19076fe8afe44b63f13`.

ROOT and the L2 production owner received the exact failure before any proposed
production edit. The current assignment permits changes to the new leaf and its
evidence. An exact reviewed SDK successor is required for another dependent run.
No unchanged failing compile or old control suite is being repeated.

Physical/native owner effects, guest/GPU/VM integration, other supported targets,
mutations, audits, profiling, benchmarks and full gates remain ROOT work.
