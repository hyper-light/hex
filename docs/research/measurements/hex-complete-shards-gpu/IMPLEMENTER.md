# Exact Shards GPU implementation record

Lane L2 implementer: Kiln `01a1c7676c7c76cb998cb12303110488`.
Coordinator: `01a1aa77778471339bea3beff90d6bbd`.
Hex starting source: `b06b1760bb690c9e29a31f99efdb998cf8279c36`.
Full charter and lane brief were read from `scheduler-completion-20261110`.
Full Hex CLAUDE and current upstream Shards AGENTS, development-rules,
development-evidence and CLAUDE were read before edits. Policy revision:
`0342b85d9c808e3ced041a6c3ef0e9bf70438272`; rules SHA256:
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
ROOT acknowledged the readback and Hex ownership, then explicitly approved the
dedicated GPU guest design. Upstream changes remain held for the existing Shards
GPU owner's exact partition. No additional agent, goal or plan was created.

Owned Hex paths: `crates/hex-runtime/src/guest/**` and this directory. ROOT owns
integration, CLI/node configuration, dependency/vendor metadata and main merges.
L1 owns shared classes/catalog/native attachment, L3 owns the durable box holder
and node recovery/forwarding. They received concrete boundary requests.

## Decision and sources

The current shared guest cannot enforce distinct physical sets between boxes:
every guest process with its render node reaches that VM's renderer contexts.
This is a source observation and a new required feature, not a reproduced native
defect. ROOT approved a dedicated existing guest per GPU-bearing BoxId. CPU boxes
keep the existing shared guest, with no GPU. A global whitelist for the shared
guest would grant unrelated boxes the union of physical claims; guest driver
changes carrying authenticated per-container context grants are a substantially
broader alternative. Neither a second picker nor a second device-holder ledger
is introduced. The existing BoxRecord/Index retains device ownership; the new
receipt records which VM must end before releasing that owner.

Primary references read 2026-11-10:

- [Vulkan ID properties](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceIDProperties.html)
  distinguish device UUID from pipeline-cache UUID and describe persistence limits.
  Native identity is rejoined to a live UUID; a UUID is not substituted for durable
  native physical identity.
- [PCI extension](https://docs.vulkan.org/refpages/latest/refpages/source/VK_EXT_pci_bus_info.html)
  and [PCI properties](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDevicePCIBusInfoPropertiesEXT.html)
  require checking per-device extension support, then using the exact PCI tuple.
  Missing support does not authorize model/vendor/index fallback.
- [MoltenVK v1.4.2 MVKDevice.mm](https://github.com/KhronosGroup/MoltenVK/blob/v1.4.2/MoltenVK/MoltenVK/GPUObjects/MVKDevice.mm),
  `populateDeviceIDProperties`, exposes the native Metal registry ID in deviceLUID
  as big-endian bytes. That permits an exact native join with this renderer pin.
- [Vulkan-Headers v1.3.252](https://github.com/KhronosGroup/Vulkan-Headers/blob/v1.3.252/include/vulkan/vulkan_core.h)
  specifies the ABI sizes and tags; [Microsoft build tools](https://learn.microsoft.com/en-us/cpp/build/building-on-the-command-line?view=msvc-170)
  documents explicit ARM64 target toolchain selection.

The full renderer filter/transport artifacts at Hex
`6219e61c9e3f559579b2632f873005a47f521cc8` are retained unchanged. Their 32/19/20
fake-driver/protocol passes are historical focused evidence, not a new renderer,
VMM, VM, GPU or Windows ARM result. Upstream application/build remains open.

## Source checkpoint and invariants

`selected.rs` encodes immutable BoxId, sorted exact host GPU IDs, configured
vCPU/memory and `Intent -> Running(identity) -> Stopping(identity)`. Intent is
synced before launch; process identity is recorded before relay connection or
workload staging. A lost identity remains typed uncertainty. An inconsistent
selection or configured envelope refuses recovery. Receipt removal requires
positive terminal/absence observation. A malformed file is not absence.

`dedicated.rs` routes each BoxId to its own Booted connection and scoped program
connection. Stop bypasses the relay; an unresponsive guest cannot prevent a stop
attempt. A stop result alone does not remove the receipt: wait_for must observe
terminal. A partial creation cannot cause a second launch/retry. The shared CPU
path still uses the existing guest. Explicit Shards workloads translate to the
same OCI payload inside that backend; Firecracker is refused at this adapter.

Configured VM CPU milli and memory bytes are exact unit conversions. Admission
must charge that complete VM envelope before boot, with payload bounds inside
it. This helper is not proof that VMM host overhead has been measured/charged;
L1/ROOT integration and measurements remain owed. No arbitrary memory allowance
or new timeout is introduced.

Receipt reads are bounded by the existing native configuration-byte ceiling;
device counts by the existing u16 device space; guest routes/listings by
MAX_BOXES. Text is counted before fallible allocation. State is per owned VM;
event clients invalidate on closed connection and die with that VM. No inventory
cache, state polling or new reconnect timer is added. Existing learned boot
bounds provide protocol wait deadlines; no latency/performance claim follows.

## Required integration and unfinished work

This is source progress, not readiness. The current branch intentionally awaits
the exact shared source cohort rather than inserting production stubs:

- ROOT `9d9dccfbd82c8e7448d67166525114d2a41a8db5` supplies append-only public
  Shards/Firecracker variants and wire integration. Original owned source remains
  based on b06; any private check overlay must record these exact blobs.
- Proposed upstream API `PhysicalGpu` (PCI, Metal registry ID, LUID),
  `GuestSpec.gpu_devices`, real renderer native-identity join, context setter/ACK,
  selected memory and Windows ARM renderer remain upstream partition work.
- L1 catalog `Binding::ShardsGpu { physical }` supplies trusted native bindings;
  guest-only `Binding::GuestGpu` maps trusted boot host IDs to actual virtio GPU
  DRM nodes. Peer payloads cannot create that mapping. The actual guest startup
  integration and native CDI cgroup/device-node effects remain required.
- L3 node recovery must call `guest.validate_binding(spec)` before adopt/alive;
  forwarding must use `evented_for(id)` / `program_client_for(id)`.
- ROOT must charge the configured per-box VM envelope plus measured native
  parent/VMM costs, validate retained claims and terminal cleanup, then perform
  native/VM/E2E/fault/mutation/profiling/benchmark and seven-target gates.

## R01–R16 accounting

R01/R03/R04/R05/R06/R07/R12/R13: explicit typed states, fallible bounded new
buffers, checked unit/count conversions, existing public guest/backend seams,
scoped ownership, no fallback, no new tuning values. Local scoped rustfmt and
diff checks are permitted; compiler/light checks are still pending at this pin.
R02/R09/R10: primary sources and alternatives above; no native reproduction or
effect is claimed. Focused receipt and routing behavior tests are being added.
R08/R14: comparative performance, allocation/copy/fault/parent costs remain
coordinator work. R11: seven-target and real KVM/HVF/WHP coverage, Windows ARM
included, remains unverified; paused Windows ARM VM untouched, Intel Mac excluded.
R15/R16: signed named-path checkpoints; light checks only admitted d132
CPU1/memory4GiB/disk12GiB, nice10/jobs1/threads1, private source/target/cache/temp,
precise no-sync. No local builds/tests or native actors, no runner changes or
foreign data/PID changes. No active check/job; originals/evidence preserved.
