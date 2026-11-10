# Scheduler completion integration (AC-T7.4.1)

Ada requested four independent Kiln implementation agents, full implementation and a
validated main merge. Her backend clarification covers Docker/OCI containers,
Firecracker microVMs and Shards microVMs. This is active new work after the earlier
goal clear; old stopped agents and their source/evidence remain preserved.

ROOT coordinator: `01a1aa77778471339bea3beff90d6bbd`. Current policy read in full:
Shards `0342b85d9c808e3ced041a6c3ef0e9bf70438272`, R01–R16 SHA-256
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
All four implementers returned matching policy and ownership readbacks before edits;
ROOT acknowledged them. Agents own production implementation and light checks.
ROOT owns integration, actual native/VM/fault/mutation validation, audits,
profiling, benchmarks, final gates and main merge/push.

| Lane | Kiln agent | Owned behavior |
| --- | --- | --- |
| Bindings | `01a1c7676c2d7463b227be301180d1be` | Common physical selection, OCI attachment, Firecracker runtime |
| Shards | `01a1c7676c7c76cb998cb12303110488` | Exact physical selector, guest/VMM/renderer integration |
| Lifecycle | `01a1c7676cb570a88abb4acad962a2d9` | Durable create/cancel/recovery and physical cleanup |
| Validation | `01a1c7676cfc7297a41236a8e71ee188` | Executable backend oracles, receipts, matrix/profile tooling |

## First integration

L1 source `e38544eca80aa952809147e9d46d5ec318368210` adds explicit
`Class::{Firecracker,Shards}` and corresponding `ClassSpec` variants, carrying
the same validated OCI workload. Host binaries, kernels and native mappings belong
to initialized backend configuration; peer payloads carry exact host DeviceIds.
ROOT integrated that source as `ff8b28ec6cb61b21d29e515ec67cdec081d2da6a`.

ROOT extends the existing wire enum tags append-only: class tags 0/1 remain
process/OCI, 2/3 are Firecracker/Shards; spec tags 0/1/2 remain process/OCI/bundle,
3/4 are Firecracker/Shards. Both use the existing OCI codec, retaining exact
claims. The source-derived wire identity changes, so mixed peers refuse their
hello instead of misreading variants. Focused request/capacity/info coverage checks
backend identity and host claims, and generated hostile-message coverage includes
the new workload and info variants. This is a protocol implementation, not proof
that a native adapter is initialized or that any GPU was attached.

ROOT also supplies L4's Windows SDK dev-feature integration for the standalone
explicit-adapter compute witness. The existing pinned `windows` 0.62.2 dependency
is reused; the lockfile adds it only to hex-gpu's dependency list. This enables
test compilation and does not execute GPU work.

## Current results and outstanding work

At this checkpoint the new integration is SOURCE ONLY. Scoped formatting and
`git diff --check` pass; no new compile/test/native result is claimed. Existing
evidence in the input tree retains its original source attribution. Downstream
backend matches and actual runtime implementations are in progress in their
owners' branches. ROOT will check the integrated exact source before readiness.

Read-only runner inventory found the approved Linux GPU host adacachy2 offline
and adacachy overloaded; no Linux validation was dispatched and no Fedora
fallback was used. Jupiter reports Windows build26200, Containers and WHP
enabled, and one AMD Radeon8060S. The retained image-layer path still exists.
This preflight created no container/VM or GPU dispatch; it is not HCS attachment,
compute, second-GPU isolation or resource-return proof.

Complete native selection/binding, same-survivor recovery, correct cancellation,
physical cleanup, supported-target builds, comparative profiles and combined gates
remain required. Each backend must execute its actual runtime. A shared Shards
GPU flag, pure pass, successful compilation or unavailable-device refusal cannot
close another backend's obligation. No main merge or push has occurred for this
new batch.
