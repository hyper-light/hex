# Native scheduler/CDI device fixture — source checkpoint

Status: implementation only. **Zero tests executed, zero native effects observed.**
The two added tests are explicitly ignored and require ROOT's separate hardware admission.
This leaf is not registered in its parent on this branch. An unregistered build or an
ignored/zero-filter run is not a passing native test.

## POLICY_READ and ownership

Implementer: Kiln `01a1c640e6ed710d8a8c919b4aa1fc8c` (Hex relay).
Coordinator: `01a1aa77778471339bea3beff90d6bbd` (ROOT).
Fixed Hex base: `1aa4ff800fbbb887e9f94dd30dae073344fe2b8e`.
Branch: `cp/t7-native-device-fixture`; previous tested Driver `6483c6f2` and evidence
`56586dff` remain on `cp/t7-device-driver`; previous CDI and relay branches remain intact.

Read current Shards origin/main `33df95b08ad8f95bddeaa1c48acf0c64386e0eac` in full:
AGENTS.md, docs/development-rules.md R01–R16, docs/development-evidence.md,
CLAUDE.md, docs/design/managed-runtime-plan.md, and `/Users/adalundhe/g121-handoff.md`.
Rules SHA-256: `df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
Hex CLAUDE.md fully read; SHA-256
`8708b80967e5517ae9137eb642600668ac02d164a4c2145ad8462a37889d424e`.
Full `/Users/adalundhe/.cache/hex-lead/HANDOFF-codex.md` SHA-256
`9104e00bf88ca5c2e2e554f749642e8e2a03debe57029c4c8e44747a2347ee72`.
Readback sent to ROOT with stable key `native-device-fixture-policy-33df95-1aa4-v1`;
ROOT acknowledged. After compaction the current rules and handoff were reread;
the policy revision/hashes remained unchanged.

Own only NEW `crates/hex-cli/tests/oci_devices/native_scheduler.rs` and this evidence
directory. ROOT owns parent registration, production/shared files, API changes,
integration, review, native validation, audits, profiling, benchmarks, heavy checks,
VMs, mutations, full gates and merges. Explicit Hex implementation/light-only scope
supersedes broader Shards R15 campaign authorization. No children, goals or plans.
No laptop builds/tests. No hardware runs, new guests, runner changes or native campaign.

## Exact integration and helper contract

ROOT registers sibling modules in `crates/hex-cli/tests/oci_devices.rs`:

```rust
#[path = "oci_devices/native_devices.rs"]
mod native_devices;
#[path = "oci_devices/native_scheduler.rs"]
mod native_scheduler;
```

The discovery leaf is owned by existing lifetime agent
`01a1c640e6ad7127bcf34212f474d37c`, not included or modified here. Consumer agreement:

```rust
pub(super) enum Vendor { Nvidia, Amd } // PartialEq
pub(super) struct NativeGpu {
    pub vendor: Vendor,
    pub id: hex_api::DeviceId,
    pub pci: String,
    pub model: String,
    pub memory_bytes: u64,
    pub node: std::path::PathBuf,
    pub major: u32, pub minor: u32, pub mode: u32, pub uid: u32, pub gid: u32,
}
pub(super) fn discover(dir: &std::path::Path)
    -> Result<Vec<NativeGpu>, DiscoveryError>; // typed Debug error
pub(super) struct FixtureFiles { pub cdi: std::path::PathBuf, pub catalog: std::path::PathBuf }
pub(super) fn write_fixture(state: &std::path::Path, gpus: &[NativeGpu])
    -> Result<FixtureFiles, DiscoveryError>;
```

Helper signed source `c646df710c267ebaa57ca262e7db777310296eb0`, leaf SHA-256
`41fa9f910bb3f91cc2b645eee1abc3745c089182292de31e699c0f8c7bb0191f`.
The full consumed discovery/writer implementations were read at that fixed pin.
After source83947 the helper added the requested common selected-node alias;
the corrected consumer uses its `write_fixture` instead of a second JSON writer.
It writes CDI 0.5.0 (minimum for hostPath, accepted by the pinned CDI parser) and
the existing native catalog format, validated with production parsers before use.
No discovery, picker, identity, holder ledger or OCI renderer is reimplemented.
The helper file remains absent from this branch; ROOT composes both signed leaves.

Pipeline: control Client → actual Server/Service → backend Client → actual
Server/Node → existing native Catalog/CDI renderer/runtime. No SimRuntime,
runtime wrapper, fake telemetry, synthetic PCI identity or manual physical claim.
The payload leaves `spec.devices` empty. Actual native BoxRecords determine what
the production picker selected. Catalog model/memory must match production Prober.

## Intended observations and cleanup

Each vendor case requires root, delegated cgroup v2, the requested vendor GPU plus
any other verified GPU with a distinct physical ID and character-device rdev, and explicit caller-supplied
`HEX_NATIVE_DEVICE_BOUNDS` containing valid finite `cpu_max_milli` and `memory_max`.
There is no guessed workload limit or default. ROOT admits room for two GPU boxes
plus one CPU sibling and native node/helper overhead. Demand caps equal these bounds.

ROOT supplied the actual approved adacachy2 envelope: **one NVIDIA RTX 5060 Ti and
one AMD Radeon 610M**, not two of either vendor. Both vendor cases support this
mixed pair. Discovery chooses the requested-vendor primary and any remaining
verified companion as fixture inputs; this is not placement. The first request
includes `gpu.model==` with the exact measured primary model. Whitespace is encoded
as underscores according to the existing needs grammar; production performs model
normalization/matching and physical selection. Fresh work requests the remaining
companion's measured model, permitting different vendors/models. No synthetic load
or identity is substituted. CPU/memory caps and native lab overhead/headroom have
not been supplied or measured in this lane: ROOT must supply/admit them explicitly.

Both actual GPU nodes are bound read-only to `/dev/hex-visible-gpu-0` and `-1`.
Each device's CDI edits name `/dev/hex-selected-gpu`, with that device's actual
hostPath/type/rdev/mode/uid/gid and read-only cgroup permission. The production
choice determines which visible alias is selected. Intended exact open results:
selected CDI node succeeds; selected visible node succeeds; other visible GPU
returns EPERM (errno 1). ENOENT, ENXIO and every other errno fail this test.

The first GPU box and a CPU sibling are made before restart. The fixture drops
the clients, both servers, Service and Node, reverses only its private catalog rows,
and reopens the same real state. Ordinary Node recovery must adopt both physical
boxes. The durable key, physical BoxId, entire recorded spec/claim and live init
`(pid,start_ticks)` must remain identical, and backend inspect must report adoption.
The same access checks run again; fresh GPU work must claim the other physical ID.
The actual CPU sibling executes before and after restart. Native capacity holders
must name the two exact boxes.

The body is caught so ordinary cleanup runs after assertions and errors. Every
returned key gets Service Delete; success requires an ended reply without refusals.
Any leftover box is recorded as failure before a fallback ordinary private Node
list/delete attempts cleanup, so fallback cannot mask failed scheduler deletion.
Cleanup checks no native boxes, holders, known records, containers or box cgroups.
Owned service/server/node handles are then stopped. Only this new private cgroup
root is removed. No broad PID search, signal, takeover, guest or cotenant operation.
Private state/receipts are retained with TempDir::keep on success or failure;
uncertain cleanup preserves state and fails. Failures before native startup have
no boxes to clean. ROOT must retain stdout/stderr/panic output as well as JSONL.

The fixed child workload emits at most three short lines, so completion can be
waited before reading its stdout pipe. Waits use existing conformance PATIENCE;
timeout kills/reaps only the exact returned Proc and remains a failure. Reads use
existing MAX_MESSAGE/MAX_RECORD_BYTES plus one byte to detect oversize input.
Server limits/workers come from actual Node sizes. No new wait/cache/manager.

## Evidence and explicit remaining work

Original signed source `83947ec9c7f97f46b22f824be9ebd1e4e4858dc2` is preserved;
its two-same-vendor prerequisite is superseded by this mixed-pair correction.
Original leaf hash: `797de681d05d3e06982fa94eee90a1cb1d96f52ef8b44434f71cf5cb6185e702`.
No result from that source is relabeled. Source and helper remain uncompiled here.
Local allowed check: scoped rustfmt only (exit 0); no compiler or test invocation.
No private remote job is active at this source checkpoint. Local free space was
218,616,220 KiB before this small source checkpoint; no bulk transfer or deletion.
`SHA256SUMS` identifies the exact leaf. No red/green native counts or cleanup
witnesses exist yet; the JSONL fields describe future observers, not results.

ROOT owes helper/parent integration, meaningful Linux compilation/Clippy and both
explicitly selected hardware cases, raw logs/source/binary/toolchain hashes, exact
selected counts, physical access and cleanup witnesses, and final combined gates.
The source checks only character-node access; it does not claim CUDA/ROCm compute,
vendor libraries/shared controller nodes, a full toolkit configuration or GPU work.
NVIDIA/AMD/CPU host prerequisites and any native failure remain unverified.

This Linux fixture does not fulfill Windows/macOS device support. Ada requires the
complete path on Linux major flavors and Windows amd64/arm64 and macOS arm64;
ROOT owns those implementations/proofs. A platform refusal is not completion.

R01: test assertions only; production unchanged. R02/R03: existing real seams,
CDI 1.1.0 and production identity/parser contracts; bounds are explicit/protocol-derived.
R04: running/down ownership and typed control/refusal decoding; uncertainty retained.
R05: actual owners, notifications/completion, no cache or second picker. R06/R07/R13:
focused fixture helpers and comments for visibility/authority and cleanup obligations.
R08/R14: no performance claim or run; ROOT-owned. R09: no production fix or claimed
reproduction. R10: exact source and intended/observed distinction recorded here.
R11/R12: Linux-only native fixture, remaining supported-platform proof explicitly owed.
R15: scoped implementation only, source first, no self-merge. R16: floor checked,
all old branches/raw/artifacts preserved, no unrelated data/process changes.
