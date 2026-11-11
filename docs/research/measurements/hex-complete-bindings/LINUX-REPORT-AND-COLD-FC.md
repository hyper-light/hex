# Linux managed report carriage and charged Firecracker observation

This is an implementation-only source checkpoint under policy
`0a6d91b3de0f10a8397868aa3794adab1e09afdc`, rules SHA-256
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
ROOT retains integration and native/fault/mutation/performance acceptance. No Cargo,
native process, KVM, VFIO or driver operation was executed by this implementation lane.

The paired source manifest records exact before/after files and dependency pins.
The baseline source remains in the private `linux-report-baseline-v1` directory;
its hashes and Git blobs are retained in the manifest. Function-source equality
describes text, not compiler output or native behavior.

## Managed report descriptor

The existing Node, Store, turns, Supervisor and managed receipt own the process.
Node verifies that the SDK request, writer, complete selected physical set and
canonical native scope agree before reserving the native envelope. The request's
absolute deadline can only shorten the caller's remaining launch budget. Supervisor
returns after identity publication and gate resume; it does not wait on the report,
renderer or guest readiness while a report pipe might need draining.

The held process consumes the writer, obtains a final CLOEXEC duplicate above stdio,
and drops the original before clone. Only that final owned descriptor is rendered by
the pinned SDK into four argv words. Its strings and pointer slots are checked against
the existing native exec block and per-string limits before clone. Parent descriptors
never become inheritable. Neither a parent raw FD nor a stale SDK plan FD is used.

The ordinary `sys::child` signature and callers remain intact. It calls the same
extracted `child_with_fd` body with `None`. The original body is byte-identical after
removing the single new optional report block, as recorded in the manifest. The held
child alone passes `Some(final_fd)`. Its fixed descriptor allowlist retains stdio,
the gate reader and this exact writer. After the ordinary global CLOEXEC sweep, only
this child's exact admitted writer has CLOEXEC cleared. Clone uses a private FD table;
the parent flag and unrelated descriptors are untouched. The shared body's signal,
limit, cwd and exec operations remain the existing implementation. Each unsafe block
states the ownership and post-clone syscall-only invariant.

The Shards CLI's paired signed source adopts/validates the explicit report/lifeline
pipes before helpers, immediately marks them CLOEXEC, and restores only its own
admitted descriptors immediately before the same-process VMM exec. ROOT still owes
actual parent/helper/VMM descriptor closure, sibling/death/cancel and guard-removal
proof. Pure argv-bound and scope/writer-parity controls exist but are unrun.

## Charged cold Firecracker observer

A prebound VFIO GPU can disappear from the host driver provider. Catalog strings,
BAR lengths and missing counters cannot replace current GPU facts. The new observer
uses a real canonical PoolRecord through the same Books, Store, device turns, managed
Supervisor and native scope. It creates no host workload BoxSpec, child table, reaper,
inventory cache or second Gpus provider.

Preparation records configuration identity and the complete canonical configured VFIO
GPU IDs without inventing measured DeviceInfo. The configuration digest binds the
fixed observer cost, executable pin, boot paths and full sorted claim set. The observer
cost is the configured floor with an equal ceiling; original quantum and explicit
guest/host allowances remain. Allocation and machine construction use the same formula,
and charge the host VMM allowance once. Unit resources in the controls are synthetic,
not measured costs.

After the same Index grants resources and exact claims, the Pool transaction persists
Granted before VFIO inspection, boot files or native launch. The private readiness
listener exists before the VMM starts. The same managed receipt publishes native
identity while held in the charged cgroup. Guest readiness uses the strict 12-byte
shared frame and non-reserved port; the whole operation retains the configured finite
boot deadline. The official pinned Firecracker assignment API supplies authoritative
host SBDF to actual guest SBDF. Only a real guest driver's PhysicalReport joined to
that witness can provide model, memory and independent optional counters. Failed
observations supply no fresh GPU facts and preserve canonical pool/box claims.

The observer becomes Live only after this actual join. GPU Box handoff first persists
the original Creating record/scope/route/allocation/full selected set, releases its
turn, and drains the observer through the same whole-scope terminal witness. Only a
positively Released pool can hand the device to the exact retained Box incarnation.
Interrupted grants do not boot a replacement; uncertainty retains claims. Recovery
dispatches persisted execution and performs the common retained Box+Pool preflight
before configuration preparation or native queries. The existing Node rendering,
original workload, route and charge remain canonical.

Endpoint ownership now consumes L4's fallible LinkOwner, so it creates no unchecked
std Arc endpoint. The same client finite operation budget and overall create budget
remain distinct from connection lifetime or workload completion.

## Remaining closure and evidence

The source includes real observer initialize/release/recovery/usage methods, but the
Service-to-Index Prepared backing bootstrap handoff is the next implementation
increment. Released-pool rearming and current observations from an active GPU Box
also remain to be completed. The full configured claim set currently requires a
workload selecting that complete set. Strict whole-owner cold native metadata
preflight is being coordinated with L3's existing recovery seam. None of these gaps
is counted as hardware-only or as complete initialized GPU capability.

ROOT owns joined compile/light execution, Linux first-instruction cgroup and FD
transfer/mutation proof, persistence/cold recovery/fault acceptance, actual resource
measurements and native GPU execution. Available approved hardware currently has
native NVIDIA/AMD drivers and lacks the required readable/writable VFIO group nodes;
no binding or shared host change is authorized. Existing CPU/KVM QA is preparatory
evidence for its exact separate source, not evidence for this new pool/Store path.

R01: new vectors/strings and link owners reserve fallibly; no new unsafe allocation
or post-spawn owner is introduced. R02/R03: official preboot PCI assignment and
existing SDK/held-exec seams are pinned; fixed frame/FD/stdout counts and native
CPU/MiB units are protocol bounds, while all runtime budgets/costs are configured.
R04/R06/R07/R13: transitions, affected identity, cancellation and uncertainty are
explicit; safety comments explain copied tables and admitted descriptor ownership.
R05/R12: existing owner and event/deadline waits are reused, without cache/picker/
polling or separate manager. R08/R14: no performance/latency/allocation claim;
coordinator measurements remain owed. R09/R10: this is required new functionality,
not a claimed reproduced native bug; exact source and unrun controls are retained.
R11: Linux is the actual Firecracker platform; Shards descriptor peers cover their
own platforms, and supported-target/native proof is unverified. R15/R16: scoped
implementer source only, no new agents/jobs, no deletion of foreign data or runner,
credentials, services, drivers or security changes.
