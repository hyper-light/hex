# Stable native identity and guest driver readiness

An initialized Shards/Venus guest may have no GPU in Hex's existing Prober. That
provider currently reads NVML, AMD native interfaces and Metal; it has no Vulkan
enumeration/UUID path. An empty PhysicalReport cannot prove that Venus is ready and
must not be replaced by fabricated PCI IDs, model, memory or load.

Official Mesa source was read at tag `mesa-26.2.6`, commit
`8e87a503ebd91408c88596d1b9eeb5f4c0b2088d`:
[vn_physical_device.c](https://gitlab.freedesktop.org/mesa/mesa/-/raw/8e87a503ebd91408c88596d1b9eeb5f4c0b2088d/src/virtio/vulkan/vn_physical_device.c).
The retained source SHA-256 is
`607514d2b512833e8b7f66df088d253d2daf82436561ec443320c107bd7cbd14`.
In `vn_physical_device_init_uuids`, lines472–499, guest device UUID is derived from
vendor and device IDs, whereas driver UUID is derived from driver name and info.
The guest device UUID is therefore not a raw physical UUID and can collide for
different cards of the same model. A deterministic transformation alone cannot
recover physical authority. This is pinned source analysis, not a live statement
about the currently staged guest Mesa version.

[Vulkan ID properties](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceIDProperties.html)
and [memory properties](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceMemoryProperties2.html)
describe factual driver observations. Those observations do not grant host claims.
[Mesa Venus documentation](https://docs.mesa3d.org/drivers/venus.html) describes the
guest driver/protocol mechanism. Actual driver-visible heap sizes must remain
distinguishable from current host memory pressure and cannot manufacture counters.

Production stable DeviceId authority remains the same native grant, complete
selected set, VMM scope/ACK and validated actual render-node correspondence.
An independent compute UUID oracle remains a separate ROOT/native-QA acceptance
question. It must not be silently weakened into guessed PCI, ordinal or model
equality. Firecracker real PCI passthrough instead uses its authoritative actual
guest SBDF assignment witness and real native guest driver report.

The smallest proposed Shards readiness seam is an explicitly owned, paid inner
guest observer Program, executed through the same initialized guest Runtime after
the charged native backing boot. It can report the actual driver-visible Vulkan
set and heap facts with bounded output and finite configured patience. It must
neither open a second host Gpus provider nor pretend to be a host Pool-as-Box.
Any such program/type/artifact scope needs an explicit producer and checked staging
contract. L2/ROOT are coordinating that source scope; no observer binary, new DTO,
provider implementation or claim of driver readiness is included here. Source and
protocol proof cannot substitute for native workload acceptance.
