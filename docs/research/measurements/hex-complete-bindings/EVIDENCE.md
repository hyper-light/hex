# Scheduler/runtime binding completion

Implementer: Kiln `01a1c7676c2d7463b227be301180d1be`; coordinator:
`01a1aa77778471339bea3beff90d6bbd`. Hex base
`b06b1760bb690c9e29a31f99efdb998cf8279c36`. POLICY_READ accepted by ROOT before
edits: Shards `0342b85d9c808e3ced041a6c3ef0e9bf70438272`, development-rules SHA256
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`. Full charter,
lane brief, Hex CLAUDE.md, current Shards AGENTS.md, development rules/evidence
and CLAUDE.md read. Ownership is the charter's bindings-firecracker lane; ROOT
additionally granted only the necessary classes literal in
`crates/hex-sim/tests/needs.rs`. No additional agents/goals/plans.

## Source checkpoint 1: explicit backend payload

ROOT selected append-only `Class::{Firecracker,Shards}` and
`ClassSpec::{Firecracker(Box<OciSpec>),Shards(Box<OciSpec>)}`. `BoxSpec.devices`
continues carrying exact host identities. The initialized backend owns binary,
kernel, initrd and native mapping configuration; the payload cannot choose those.
OCI remains Hex's native OCI implementation with image/CDI compatibility, not
Docker Engine. Firecracker means Firecracker on Linux KVM, never Shards.

The spec validator checks the same guest workload for each explicit VM backend.
The simulator uses its existing box/init state, with no additional lifecycle or
selection manager. A focused test covers JSON, unchanged claims, create/start,
invalid guest workload and cleanup. Local scoped rustfmt completed; **no builds
or tests have run**. ROOT owns wire tags 2/3 and spec tags 3/4, shared exports,
CLI registration and combined checks. L2/L3 own their downstream matches.

## Firecracker source research

## Source checkpoint 2: one physical slot across initialized backends

`model::Device.classes: Classes` records the actual adapter mask. Old JSON and
synthetic constructors default to OCI only. The existing Index admits/binds only
through this mask and retains it during recovery/held refresh. The native offer
bridge coalesces matching `(kind,key)` aliases into one physical slot, unions
their backend masks, and refuses holder or metadata disagreement. ROOT owns the
independent oracle's corresponding mask predicate in `hex-sim/src/spec.rs`.

Runtime registration checks its actual advertised classes; payload and demand
must name the same backend; recovery/reuse checks the returned physical box's
class. Device verification queries that specific API class, removing the OCI
assumption. New pure controls exercise a single GPU across three backends,
uninitialized attachment refusal, alias conflicts and endpoint/payload mismatch.
Scoped rustfmt completed; **not yet compiled/tested**. Light checks will use
ROOT's exact `9d9dccfbd82c8e7448d67166525114d2a41a8db5` cohort with these named
owned changes and recorded integration overlays. No native effects claimed.

R01–R16 status above remains applicable: no new wait/cache/picker/holder ledger,
bounded temporary offer identity index, no measured tuning value. All native,
mutation, profiling, performance and complete platform gates remain ROOT-owned
and unverified. The new masks permit only real adapter ceilings; they do not
establish that those native adapters are implemented or validated yet.

## Firecracker source research (continued)

Official main on 2026-11-10 was
`778a26c0594c80025ed798088a66c2511ba618de` (API 1.19.0-dev), with PCI support
but no device-passthrough endpoint in its Swagger file. Hardware inference
[#1179](https://github.com/firecracker-microvm/firecracker/issues/1179) is parked;
VFIO [#5679](https://github.com/firecracker-microvm/firecracker/issues/5679) is
open. The official VFIO development source includes actual preboot passthrough at
`500efc6bb169705f5d9417cb411e3f6723e051fe`; the docs-only PR6398 head is
`7a3ca9041363ec9343f28911cd572e61dbafb384`, based on that revision. The private
research clone is `/Users/adalundhe/.cache/hex-complete-bindings/firecracker`.
Its CONTRIBUTING.md was read; no AGENTS.md/CLAUDE.md exists in that pinned tree.
No upstream extension edits or native calls have run. Source/dependency/security
review and ROOT source-pin acknowledgment remain required before extension edits.

## R01–R16 accounting at source checkpoint 1

| Rules | Evidence/result and remaining work |
|---|---|
| R01 | Added dispatch is exhaustive and returns existing typed validation errors. Source review only; final panic/clippy gate owed. |
| R02/R03 | Backend distinction follows ROOT's explicit chosen shape and existing OciSpec seam. No tuning constants introduced. Firecracker research above is preliminary; native data owed. |
| R04 | Existing Created/start/terminal simulator state used. Native lifecycle and recovery remain ongoing. |
| R05 | Existing Runtime/BoxSpec used; no new picker, holder ledger, wait or cache in this checkpoint. |
| R06/R07/R13 | Small exhaustive dispatch and backend configuration authority comments; full implementation ongoing. |
| R08/R14 | No performance claims. Coordinator comparative benchmarks, allocations and page-fault profiles owed. |
| R09/R10 | This is new required functionality, not a claimed reproduced bug. Added behavior test unrun; native/guard mutation evidence owed to coordinator. |
| R11/R12 | API/simulator portable source; seven-target and real KVM/HVF/WHP effects unverified. macOS arm64 only. |
| R15 | Signed source checkpoint for integration; unit/light checks not yet run. ROOT owns gates/main/native/fault campaigns; implementer never merges main. |
| R16 | Local free space 173 GiB before edits; no build or destructive cleanup. Original refs/experiments retained. |
