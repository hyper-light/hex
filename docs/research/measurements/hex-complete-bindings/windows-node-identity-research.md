# Windows PCI identity, Vulkan LUID/node mask, and renderer selection

Read-only research, 2026-11-11; L1 implementer. Policy receipt remains
`7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506`, rules SHA-256
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
Only this note is added. No production/FFI/ABI edits, native queries, build/test,
driver action or extra agent. Primary-source downloads are retained under
`~/.cache/hex-complete-bindings/windows-node-identity-primary-v1/`; earlier SDK
headers remain under `windows-node-mask-primary-v1/`.

The candidate join is plausible **when the actual Vulkan physical device exposes
PCI information and valid Windows IDs**, but it is not an established Hex/native
join. Read the PCI tuple, LUID and node mask from the **same** Vulkan handle;
corroborate that tuple and LUID against the existing native physical owner. Do not
calculate a Vulkan mask from a KMT physical-adapter ordinal. Missing observations
remain unavailable, never guessed identity or measured GPU capacity.

## What the primary contracts establish

| Value | Meaning relevant to this join | Consequence |
| --- | --- | --- |
| Hex PCI `DeviceId` | Existing packed domain/bus/device/function identity, not a LUID or enumeration position. | Keep this durable selection; resolve its current OS correspondence. |
| Vulkan PCI properties | `pciDomain`, `pciBus`, `pciDevice`, `pciFunction` describe that physical device's PCI address. | Compare all four fields, including nonzero segment/domain. [Pinned Vulkan source](https://github.com/KhronosGroup/Vulkan-Docs/blob/fa30d4e312e843193601be7cb926c7e25d9ec885/chapters/devsandqueues.adoc#L839-L859). |
| Vulkan ID properties | If `deviceLUIDValid` is false, LUID/mask are undefined. If true on Windows, the LUID corresponds to its DXGI adapter and the mask has exactly one set bit. It denotes the corresponding D3D12 node for an individual linked-adapter device; otherwise the specified mask is 1. | Require validity and consume the **observed** mask. These conditions do not license fabricating one. [Pinned ID contract](https://github.com/KhronosGroup/Vulkan-Docs/blob/fa30d4e312e843193601be7cb926c7e25d9ec885/chapters/devsandqueues.adoc#L590-L646). |
| KMT `PhysicalAdapterIndex` | Index within an LDA chain, used by the physical-adapter PNP-key query. | Its number is not an established Vulkan/D3D12 mask bit. [Pinned KMT documentation](https://github.com/MicrosoftDocs/windows-driver-docs-ddi/blob/af296fd6a04138c44c0ed24de2fdff5d79e639ef/wdk-ddi-src/content/d3dkmthk/ns-d3dkmthk-_d3dkmt_query_physical_adapter_pnp_key.md). |
| D3D12 node index/mask | Linked adapters appear through one DXGI adapter; D3D12 masks encode its own zero-based node indices. `GetNodeCount` reports associated physical adapters. | A shared LUID alone can describe several hardware nodes. [Pinned multi-adapter contract](https://github.com/MicrosoftDocs/win32/blob/6f41993721f0d764311159caa092421e354a8d70/desktop-src/direct3d12/multi-engine.md), [GetNodeCount](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/nf-d3d12-id3d12device-getnodecount). |
| Vulkan `deviceUUID` | Driver-reported device identity, distinct from pipeline-cache identity; the specification also warns about persistence/conformance limitations. | Do not manufacture a UUID from PCI/model/LUID or use it to replace the existing durable holder. [Pinned UUID caveats](https://github.com/KhronosGroup/Vulkan-Docs/blob/fa30d4e312e843193601be7cb926c7e25d9ec885/chapters/devsandqueues.adoc#L648-L694). |

The reviewed Khronos and Microsoft pins do **not** establish an ordering relation
between KMT's physical-adapter chain and the D3D12 node numbering reported by a
Vulkan implementation. D3D12's documented index-to-bit rule is within D3D12;
substituting a KMT ordinal is an additional unproved cross-API inference.
Consequently `nodeMask = 1 << PhysicalAdapterIndex` is not this note's design.
The KMT SDK also distinguishes engine `NodeOrdinal` from `PhysicalAdapterIndex`;
neither is an enumeration index returned by `vkEnumeratePhysicalDevices`.
[SDK definitions](https://github.com/microsoft/win32metadata/blob/71033001b4479c6566546b68d32276e70a8d68b9/generation/WinSDK/RecompiledIdlHeaders/shared/d3dkmthk.h#L2229-L2233).

## Conditional direct join to investigate

This is a proposed evidence chain, not implemented behavior or acceptance:

1. Start with the canonical selected Hex PCI identity and its **existing** native
   provider receipt. Resolve the exact present PNP instance, full PCI address and
   current adapter interface/LUID through that same authority. Existing alias
   corroboration must agree; no renamed LUID-as-PCI identity. Microsoft describes
   the complete single-/multi-segment LocationInfo formats and device-name KMT
   mapping; the SDK structure explicitly returns `AdapterLuid`.
   [Segment source](https://github.com/MicrosoftDocs/windows-driver-docs/blob/e33653d9339e4bc9ee46cb5b3122315516d8577b/windows-driver-docs-pr/pci/obtaining-pci-segment-values.md),
   [KMT mapping](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/d3dkmthk/nf-d3dkmthk-d3dkmtopenadapterfromdevicename),
   [pinned return structure](https://github.com/microsoft/win32metadata/blob/71033001b4479c6566546b68d32276e70a8d68b9/generation/WinSDK/RecompiledIdlHeaders/shared/d3dkmthk.h#L2500-L2505).
2. In the actual renderer's Vulkan instance/ICD, enumerate its bounded candidate
   handles. Check `VK_EXT_pci_bus_info` **per physical device** before requesting
   its PCI structure. The extension depends on properties2 support and is not
   guaranteed on Windows or every PCI GPU; lack of support is not a zero address.
   [Pinned extension revision 2](https://github.com/KhronosGroup/Vulkan-Docs/blob/fa30d4e312e843193601be7cb926c7e25d9ec885/appendices/VK_EXT_pci_bus_info.adoc),
   [official extension reference](https://docs.vulkan.org/refpages/latest/refpages/source/VK_EXT_pci_bus_info.html).
3. Query the supported properties2 chain on each candidate. Match the full PCI
   tuple to the selected provider receipt, then corroborate valid LUID equality
   and retain the returned one-bit mask. Keep the actual Vulkan handle/UUID from
   that same observation; do not compare model/vendor/ordinal instead. Reject
   contradictory, missing or ambiguous correspondence. A partial candidate list
   must not silently turn a linked adapter into a singleton. **Inference:** these
   independent observations are a candidate join, not an atomic hardware snapshot;
   revalidation must remain inside the canonical launch/boot authority.
4. Pass the established result into the existing renderer selection seam, before
   exposing a device or creating its workload context. The selector must identify
   the matched physical node, not every device sharing its LUID. Preserve the same
   host grant, native scope, renderer acknowledgement and observed guest set; the
   observation itself supplies neither a holder grant nor telemetry readings.

A LUID is two native 32-bit fields, with a signed high field; compare the complete
native representation consistently. A raw integer reinterpretation of a PCI key
is unrelated. [LUID structure](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-luid).

## Missing extension, linked devices, and singleton hardware

**Absent PCI extension:** the direct PCI-to-Vulkan candidate path above is absent.
Do not chain an unsupported property structure, accept an untouched all-zero
buffer as PCI domain zero, or declare native mapping established by vendor/model.
The extension's platform caveat is explicit; supported Windows hardware must be
observed, not inferred from the specification. [Extension source](https://github.com/KhronosGroup/Vulkan-Docs/blob/fa30d4e312e843193601be7cb926c7e25d9ec885/appendices/VK_EXT_pci_bus_info.adoc).

**Linked adapter:** KMT can query a physical member's PNP key, but that establishes
which member its KMT index names, not that index's Vulkan node mask. Where PCI
properties are available, the direct tuple join can avoid ordinal translation.
Without them, a general per-node alternative still needs a documented and
independently verified bridge. LUID-only equality is insufficient to authorize a
particular member. [PNP query](https://github.com/MicrosoftDocs/windows-driver-docs-ddi/blob/af296fd6a04138c44c0ed24de2fdff5d79e639ef/wdk-ddi-src/content/d3dkmthk/ns-d3dkmthk-_d3dkmt_query_physical_adapter_pnp_key.md).

**Restricted singleton candidate:** an actual successful query of the same
adapter's physical count could establish that it contains only one physical
member. That is a hardware fact to read, not a default from one visible Vulkan
candidate. The KMT count structure provides `Count`; query failure is not 1.
Even here, actual valid Vulkan IDs, the returned mask, current PNP/LUID equality
and unambiguous renderer correspondence remain necessary. This cannot be used as
a general linked-device implementation or fabricated `deviceLUIDValid`.
[Count contract](https://github.com/MicrosoftDocs/windows-driver-docs-ddi/blob/af296fd6a04138c44c0ed24de2fdff5d79e639ef/wdk-ddi-src/content/d3dkmthk/ns-d3dkmthk-_d3dkmt_physical_adapter_count.md).

Singleton hardware is not the same as one GPU selected, one Vulkan device exposed
by a filter, one ADL row, one display output, one device group, or one renderer
process. Multiple ICD views can also make matching ambiguous. Those observations
must not shrink the physical authority being validated.

## Existing Hex/renderer source boundary

Read at exact Hex ROOT `2a80ea3707532bcf9119e2e9551ddebb64719a7f`:
`vendor/shards/crates/abi/src/gpu.rs` has `PhysicalGpu::Luid(u64)`, without a node
mask. That selector alone cannot represent distinct nodes sharing the same LUID.
This is a source limitation, not a reproduced native defect or permission to
change the ABI. Existing `Books::physical_luid(DeviceId)` similarly supplies only
the current LUID; it is not a mask/UUID proof or another inventory provider.

The separately reviewed Hex artifact at
`6219e61c9e3f559579b2632f873005a47f521cc8`,
`docs/hex-venus-physical-filter/README.md`, defines a per-context UUID selector
and rejects missing/ambiguous host matches. Its source explicitly leaves
native-ID-to-UUID correspondence outside the filter. It is an artifact/protocol
source, not evidence of this Windows join being installed or executed. A verified
same-handle UUID could feed that existing kind of seam; it must not be guessed,
and the renderer must still validate its actual handle/group visibility.

The smallest research direction is therefore **one native identity owner plus
renderer-local, supported property observation**, with exact boot/grant scope.
No additional global picker, telemetry provider, inventory cache or model table
is indicated by these contracts. General LDA support, singleton-only acceptance
and an unavailable join must remain distinguishable. None is native proof.

## Immutable source receipts and outstanding proof

Khronos official tag `v1.4.368` (tag object
`0a129832bea301c5b11f3f14d2825e468c4ff956`, dated 2026-11-06) resolves to commit
`fa30d4e312e843193601be7cb926c7e25d9ec885`. The Microsoft documentation snapshots
below are public source commits, not inferred SDK ABI versions. Every digest is
SHA-256 of the retained raw file.

| Raw file | Official source revision | SHA-256 |
| --- | --- | --- |
| `vk-devsandqueues.adoc` | Khronos commit above, `chapters/devsandqueues.adoc` | `9a28a6b978bc4bf57fcaaf263488b4894129518be8aa2dc8b23269be43ade6ad` |
| `vk-pci-extension.adoc` | Same, `appendices/VK_EXT_pci_bus_info.adoc` | `155d1db6f5ceb86884c304d551da68c015878eaf2107e36a535b436a625215ef` |
| `ms-multi-engine.md` | MicrosoftDocs/win32 `6f41993721f0d764311159caa092421e354a8d70` | `8d3c3ae98743fdebe1bd413294f18c2291f251c004977c977bf08f2f20887f12` |
| `ms-kmt-physical-pnp.md` | MicrosoftDocs/windows-driver-docs-ddi `af296fd6a04138c44c0ed24de2fdff5d79e639ef` | `b1fe76e261c868ccff2fdf9e42d3cab2330e7332c741d756239b548442bb8ebb` |
| `ms-kmt-physical-count.md` | Same documentation commit | `017ba414f4297a6057fcadc512dd4d5409c4b7e3419f59682c33b83d059954e0` |
| Existing `shared-d3dkmthk.h` | microsoft/win32metadata `71033001b4479c6566546b68d32276e70a8d68b9` | `57de671f84da844e98dc166e5cff7057726d87900e27c522bb5c0f6402043565` |

The SDK header pins KMT physical count, physical PNP key, returned adapter LUID
and the separate physical/engine fields; it does not supply a missing ordinal-to-
Vulkan-mask rule. No unversioned driver sample, forum post or third-party GPU dump
was used as proof.

Actual availability on the approved Windows host, linked/identical-device
correspondence, unsupported-extension/invalid-LUID behavior, renderer-selected
native compute, reordering/restart/recovery, and scope/claim cleanup remain
unverified. Source review cannot establish those effects. Existing historical
host-compute and CM/KMT receipts retain their own source/boot attribution; this
research made no new hardware observation or acceptance claim.
