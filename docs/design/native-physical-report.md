# Native observations across the existing runtime seam (AC-T7.4.1)

ROOT coordinator01a1aa77778471339bea3beff90d6bbd. Policyca3ab10c151aa5539e757f1cc76029746ccbb561, rulesdf2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3. Source checkpoint5e5a0f5047bb1ef452e9a0dd6c3b70c0198df8ca; integration/native results remain separate.

The CLI currently runs scheduler Service and its runtime node as separate processes. Runtime backends inside the node share its one existing native provider and Store/turn authority. Service reads a factual snapshot through that same Runtime/socket protocol. It opens no additional GPU provider, and observations grant no holder. Original source anchors are scheduler::Service::open, hex-cli/src/schedule.rs, facts::Prober, Runtime and wire's existing capacity call.

Rust1.99.1 [Arc documentation](https://doc.rust-lang.org/std/sync/struct.Arc.html) describes shared ownership of one heap allocation and Weak references for avoiding cycles. Accordingly Arc/Weak connects adapters within the native node; a separate scheduler uses a serialized report rather than a process-local pointer.

[Kubernetes device-plugin documentation](https://kubernetes.io/docs/concepts/extend-kubernetes/compute-storage-net/device-plugins/#device-plugin-implementation) separates device health reporting from allocation operations. Its unhealthy-device handling retains assignments while reducing fresh allocatable capacity. Hex keeps that separation at its existing Runtime seam and uses its current persistent records; it adds no plugin daemon, watcher or reservation ledger.

| Alternative | Result |
|---|---|
| Embed a second Node in Service | Changes the existing split CLI and duplicates native ownership. |
| Independently open Gpus in each consumer | Duplicate native inventory/driver ownership and incompatible delta sampling. |
| Share Arc inside a node and report over its existing wire | Preserves the CLI and one native authority; chosen. |

PhysicalReport names the stable native Store/turn authority and contains PhysicalDevice wrappers around existing DeviceInfo plus independent optional busy-per-mille and memory-used counters. None is unknown, Some(0) is an observed zero. Allocated memory beyond a working-set budget remains pressure, rather than making an identity disappear. Reports cannot contain holders or duplicate IDs, and a non-GPU cannot carry GPU counters. Initialized CPU-only providers report no verified devices without inventing absence proof for a retained claim. Missing native initialization is an explicit refusal.

The appended wire operation has no descriptors. Its writer checks the report and one complete encoded-body size before fallible reservation; its reader checks count/minimum bytes before visiting elements and reserves each decoded element fallibly. Hostile unknown units, counts, truncation and holder attempts are covered by source controls. Existing Capacity encoding is unchanged. The source-derived wire digest requires both peers to rebuild. The new per-operation wait census starts measuring; no guessed calibration is introduced.

Multiple backend routes for a physical identity must use this same native authority. A native report is not permission to bypass current route/identity checks at create or on recovery. Persistent Creating/Ready/Deleting rows, original selections and admitted cost receipts stay authoritative until physical cleanup is proved. No periodic inventory cache, repicker or authority enlargement is introduced.

The first exact API report control passed at5e5 (one selected); combined codec/Clippy collection was still running when this record was written. Current read-only [Windows facts](../research/measurements/hex-completion-root/native-readonly/README.md) establish available native inputs only. Real split-process consumers, unknown-counter refusal with independent CPU progress, exact native joins, forbidden effects, meaningful mutations/restores, supported-platform gates and resource/latency profiles remain required. No performance or native-attachment result follows from this design.
