# Borrowed connector native results

ROOT coordinator01a1aa77778471339bea3beff90d6bbd, 2026-11-10. Current Shards
policy a920c85b5c0c9d40aafc4f4cce32432c7c0aed79; full R01–R16/evidence/CLAUDE
read after resume, rules SHA256df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
Tested source d9d547c7d9211f6c42f1c2c884017a3981032889. The earlier source8e
results and their separate provenance remain in README.md and the8e proof files.

## Actual outcomes

| Run | Outcome |
| --- | --- |
| Native Mac26.4 arm64, direct API integration-test Clippy | Exit0, no warning allowance added. |
| Native Mac, MAX and finite non-MAX overflow | One explicitly selected case passes: valid/live and invalid/PID0 identities both refuse as Bounds, zero accepted connections. |
| Native Mac, target death between CONNECT and OK | One selected case passes: the server waits for the owned child's actual successful exit before sending OK; the API returns exact ProcessFailure::Exited. |
| Native Windows26200 amd64, same direct API Clippy | Exit0. |
| Native Windows, same overflow/death cases | Two selected cases pass, no failures/ignored cases. |
| Native Windows repeated failure/drop case | One selected case passes:64 complete measured rounds return exactly107 handles after each round. |
| Native Windows cold first round |96 before,107 after. The11-handle increase is UNATTRIBUTED, not a cold-start passing result or an allowance. |
| Windows zero-ready diagnostics |65 returned Read error tags, including the warm round. Tags alone are not independent proof of committed request/native Read execution. |
| Mac private mutation, final Held.check removed | Test exits101 as required. A second instrumented mutation independently records a forbidden returned stream immediately before Ok(stream), after the fixture has reaped its target. |
| Mac cross checks of combined Hex fixtures | CLI and runtime all-target Clippy exit0 for x86_64-pc-windows-msvc and aarch64-pc-windows-msvc. No native HCS/GPU fixture executed. |
| Shards vendor check on pristine d9 archive |40 source files,7 recorded changes,0 differences, exit0. |

These are correctness/resource observations, not timings or a general leak-free
claim. Windows ARM64 is cross-checked here, not natively executed; the paused ARM
VM remains untouched. Linux native coverage and the final seven-target/root-lab
combined gate remain unverified. Last approved-host admission read reported
adacachy2 offline and no idle CPU on adacachy. Fedora and laptop builds were not used.

## Oracle and source integrity

The new death fixture accepts CONNECT, reaps its own target after stdin EOF, then
sends OK. The normal implementation must return Exited for the same PID/start.
The first private mutation removes the final liveness guard and fails the case;
its101 alone would not distinguish a wrong error from a returned stream. The
second private mutation adds an observation immediately before the otherwise
unchanged Ok(stream). Its log contains:

```text
ROOT_forbidden_effect borrowed_stream_returned_after_target_death=true
```

The same test then reports0 passed/1 failed. This observes the forbidden result,
rather than counting a budget miss or unrelated compilation failure as a successful
mutation. Only client.rs differs from signed original source in this private copy;
the additional line is diagnostic instrumentation, never a production patch.
Original sources and all earlier mutations remain unchanged and retained.

The whole-invocation external guard is151 seconds, twice the maximum existing
validation_run record in bench/native/guest/records.tsv. Native API patience is
unchanged. All recorded child/collector PIDs were reaped, no guard timeout occurred,
and private temp entries are0. Independent readback verifies original source files
against the exact signed Git archives, every retained log digest, expected exit,
and registry package/checksum membership in the exact33df lock seed. Current d9
contains1915 source files. Artifacts remain in their owned runner cache roots.

| Independent proof | SHA256 |
| --- | --- |
| Mac new checks plus17c/0116 red histories |acf74b3b92801c67c722c725495c27563ce9a47554b1c09786d49fbcdb9de065|
| Windows new checks plus17c/0116 red histories |be130b49b7e22a1f31f464026b8e4c6bc1e405a29061f8de538780ce7f15a968|
| Observed mutation, both-Windows combined Clippy and vendor check |c936b51b1c5ee200bc5df111c3db7ec9e804ab7a6499cc51e87dcb7137415ba8|

Each proof has an adjacent receipt. Reproduction submitters, runner drivers and
independent collectors are retained in this directory. They use exclusive private
paths: choose a new absent path for a new run, and never relaunch an uncertain job.

## Failures retained and corrected

At17c99c58 both native builds passed, then direct API Clippy failed on the single
excluded-host loop introduced by Hex0003. Patch0006 checks the same Intel Mac pair
directly; no backend behavior or lint allowance changes. At0116e44b native builds
passed, then Clippy required SAFETY comments immediately before unsafe inside the
test assertion macros. Patch0007 moves the existing native invariants to those
calls, preserving behavior. The d9 checks pass on both hosts.

The first vendor check reused a standalone-test source directory containing its
generated vendor/shards/Cargo.lock and correctly reported one extra file. A new
private extraction of the exact signed d9 archive passes with40 files/7 changes/
0 differences. The generated seed lock remains with its original experiment; it
was neither committed nor recorded as a vendor change. Both check logs and source
integrity outcomes are retained.

Earlier Windows8e long-temp bind/pair failures and a0-selected Unix-only owned
compatibility case remain in the8e proof. The latter is unverified on Windows;
it is not included in passing counts. Mac owned zero/MAX compatibility passed
at8e; borrowed production client/stream/taken code is byte-unchanged in this batch.

## Scope and policy accounting

| Rule | Evidence or concrete remaining work |
| --- | --- |
|R01|Borrowed production unchanged; conformance correction preserves typed error handling; direct native Clippy clean. No claim that lint proves all implicit panic paths.|
|R02|Current upstream33df owns Ready/Until/shared handshake. Native process API sources and pinned Windows identity research are linked from the method and mapping record; this batch adds no runtime architecture.|
|R03|No production tuning value added. MAX-minus-one-nanosecond is a boundary input;64 rounds derive from identity width; guard derives from retained validation records. No cold-handle allowance.|
|R04|Tests distinguish Bounds, exact Exited and typed handshake I/O phases; raw results retain failures and uncertainty.|
|R05|One existing Held owner and complete-operation handshake deadline; no new runtime cache, watcher or polling. Test control waits use explicit events/EOF/join under the external guard.|
|R06|The only production edit replaces a one-item loop with the same direct check.|
|R07|Native test calls carry immediate SAFETY/ownership invariants; no unsafe lint suppression.|
|R08|No speed/resource ratio or competing-baseline claim. Broader comparative work remains ROOT-owned.|
|R09|17c and0116 exact live native lint failures precede their corrections; raw failures retained. Test setup failures are classified separately.|
|R10|Normal live exit/drop/overflow checks, observed forbidden mutation effect, immutable source/log readbacks and reproduction drivers retained.|
|R11|Actual Mac arm64/Windows amd64, both Windows cross-target fixture checks; Linux and final seven-target/native VM coverage still owed.|
|R12|Existing shared API preserved; necessary native process observations are isolated by cfg inside tests. No extra installed binary.|
|R13|A single test file exercises the public API; no parallel runtime or authority.|
|R14|Exact steady-state handle observations only. Cold +11 remains unattributed; no complete native leak, allocation, latency or performance claim.|
|R15|Read-only adversarial review/implementation roles and ACKs recorded; all native/mutation/integration/vendor validation done by ROOT. No main merge/push for this unfinished device batch.|
|R16|Owned private cache/targets and all failed experiments retained; laptop remains above30GiB floor; no runner restart or unrelated process stop.|

## Upstream disposition and remaining work

The old18,736-byte c55 production patch remains an obsolete proposal. Current
upstream33df already implements the corrections the peer requested. The new
upstream-borrowed-contract-tests-d9d547c7.patch is test-only against that exact API;
upstream-provenance-d9.json records source/test/patch hashes and signed commit
presence. No e9 checkpoint or stale production patch is submitted for merge.

Trace/attribute the cold +11 Windows handles before any cold leak-freedom claim;
ROOT has assigned a source/light-only diagnostic extension and retains actual
native execution. Complete Linux coverage when an approved runner admits it.
Full GPU identity/attachment/compute/recovery across supported targets remains
part of the active larger goal, not established by these connector tests.
