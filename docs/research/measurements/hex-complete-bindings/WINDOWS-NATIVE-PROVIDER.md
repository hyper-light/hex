# Windows AMD native identity and factual offers

L1 implementer; policy0a6d91b3de0f10a8397868aa3794adab1e09afdc, rules
SHA256df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
Full charter/lane/Hex CLAUDE and four Shards policy files reloaded after resume.
ROOT explicitly partitioned the safe GPU files, new CM-only native adapter and
facts consumer; L3 retains the existing adapter-arrival1ca/KMT/HCS implementation.

This is SOURCE ONLY. No compiler, test, native property query, GPU, HCS or VM call
was executed by L1. ROOT owns coherent metadata/compilation and native QA owns
source-specific actual mapping, assignment and execution. Prior capacity red and
independent host compute receipts remain attributed to WINDOWS-PROVIDER-JOIN.md.

## Actual symptom and decision

Native QA fixture985d7767 against production2c8855b2 refused capacity before HCS
create: initialized route lacks unambiguous native physical identity. The old
provider assumed ADL segment0 and facts deliberately excluded every Windows AMD
GPU. Neither a blanket exclusion nor removing that guard alone is completion.

WINDOWS-PCI-SEGMENT-SOURCES.md pins the documented LocationInfo grammars,
single-segment zero composition and existing actual currentidentity1 observation.
The implementation resolves the EXACT present ADL-reported PNP devnode using
CM_LOCATE_DEVNODE_NORMAL. It never chooses a phantom, strips an alias suffix,
selects an ordinal, takes a configured address or opens a second Gpus. It compares
the devnode's actual returned instance ID, full LocationInfo grammar, native
BusNumber/Address and ADL BDF BEFORE grouping. Only the documented single-segment
form supplies zero; missing, malformed or contradictory values supply no GPU.

ADL's maximum250 rows bounds candidate, alias and diagnostic storage. Collections
reserve fallibly. Candidate sort/group uses full domain/BDF and original row
position for deterministic first counter receipt. Same BDF on different segments
is independent. A PNP alias observed at contradictory full addresses or conflicting
same-address model metadata withholds the affected group. Actual verified PNP/UDID
aliases remain beside the existing ADL counter index. A nonpresent row is a typed
Skipped::Adl observation; it cannot manufacture an alias for another row.

Each sample re-resolves retained aliases without another inventory/context and
without a timer/cache/retry. Loss/change keeps the original identity receipt and
returns unread counters; no survivor is repicked. This adds native property I/O;
latency/contention/allocation profiling is unmeasured and owed under R14.

## Concrete API and consumer

- Ordinary Gpus::open itself verifies the Windows AMD mapping. There is no legacy
  open path that advertises a guessed segment0 identity.
- hex_gpu::windows_identity::pci_for_instance(&str)->Result<PciAddress,Unread>:
  safe CM-only exact-instance mapping. PciAddress retains the existing four fields.
- Gpus::windows_aliases(Identity)->Result<&[WindowsAlias],Unread>: borrowed aliases
  from that SAME counter receipt, conditioned on its last current identity sample.
- WindowsAlias::instance()/udid() borrow the actual ADL strings.
- windows_identity::same_luid validates all aliases against the caller's existing
  KMT mapper. It does not equate a packed PCI key with a current-boot LUID or reject
  a LUID merely because its integer bits are zero.

Prober native report/device paths use L3's already-public
hex_runtime::windows::oci::sys::adapter_luid_for_instance. Missing/conflicting
current joins qualify only that AMD device as unattributable. Factual report keeps
its stable identity/model/total and exposes unknown independent counters; fresh
physical attachment view withholds it. Verified other vendors/devices and CPU-only
progress continue. A verified identity with one unsupported counter keeps the
other real reading. Holder authority remains SAME Store/Turns, never telemetry.

CM's exact CONFIGRET is Unread::WindowsCm (not a mislabeled Win32 errno). The ADL
receipt retains current CM identity status. The factual RPC already represents
unavailable counter knowledge as None; it is not a diagnostic/error/grant channel.
KMT refusal or disagreement maps to unavailable attribution there; original native
KMT errors remain observable through the existing mapper and QA receipts.

## Bounds, native safety and primary sources

Native calls use initialized fixed buffers, validated output kind/actual byte
count, ASCII-valid UTF16 with exactly one terminating NUL, checked decimal/ranges
and complete grammar. Location buffer is derived from the longest accepted u32
segment/u8 bus/device<32/function<8 text, including separators and NUL. Instance
buffer is cfgmgr32.h MAX_DEVICE_ID_LEN200. No native pointer/handle escapes.
The native adapter only reads devnodes/properties; no driver, registry write,
service, token, HCS or GPU open is added. Every unsafe call has its SAFETY invariant.

Additional primary contracts retrieved read-only:

- [CM_Locate_DevNodeW](https://learn.microsoft.com/en-us/windows/win32/api/cfgmgr32/nf-cfgmgr32-cm_locate_devnodew): NORMAL reads only the currently configured device tree.
- [CM_Get_DevNode_PropertyW](https://learn.microsoft.com/en-us/windows/win32/api/cfgmgr32/nf-cfgmgr32-cm_get_devnode_propertyw): type, byte count, complete buffer and reserved flags.
- [Device instance ID](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/device-instance-ids): system-supplied identity and MAX_DEVICE_ID_LEN bound.
- [BusNumber](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/devpkey-device-busnumber) and [Address](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/devpkey-device-address): native bus-specific corroboration.
- Microsoft win32metadata devpkey.h at71033001b4479c6566546b68d32276e70a8d68b9,
  SHA2566d59670e0157b4dbe3b279785491d74058fea7c548016cfc90696542f7094a80,
  39556B, retained ~/.cache/hex-complete-bindings/windows-provider-primary-v1/.
  [Exact header](https://github.com/microsoft/win32metadata/blob/71033001b4479c6566546b68d32276e70a8d68b9/generation/WinSDK/RecompiledIdlHeaders/shared/devpkey.h).

The header labels BusNumber/Address UINT32 whereas the property pages say INT32;
the complete four-byte integer of either documented type is accepted only when
it equals the independently checked physical range. Negative/high-bit/other type,
size mismatch, truncated property or size race refuses; there is no retry. Header
also marks LocationInfo localizable whereas its property reference says no. Only
the pinned complete PCI grammars are recognized; other localization is unavailable,
never a digit-extraction fallback. Native calls are observations, not an atomic
hardware snapshot or cryptographic identity. Attachment/native scope acceptance,
hotplug/fault controls and actual guest execution remain required.

## Integration and focused controls

ROOT-only metadata: add existing windows-sys feature Win32_Devices_Properties;
CLAUDE allowlist new hex-gpu/src/windows_identity/sys.rs for CM-only FFI. No version,
wire DTO, DeviceInfo field, catalog identity, provider or holder ledger is added.
Facts consumer requires L3's existing public exact-instance KMT getter in joined
source. Do not dispatch an unmatched branch or silently stub that dependency.

Seven new pure controls are source-only/UNRUN:

1. complete_single_and_nonzero_segment_properties_are_checked
2. truncated_or_unterminated_properties_never_supply_a_default
3. every_actual_alias_must_have_the_same_current_luid
4. native_segments_precede_grouping_and_keep_exact_alias_counter_pairs
5. missing_or_conflicting_native_properties_cannot_publish_a_guessed_gpu
6. native_alias_loss_withholds_fresh_counters_and_retains_the_original_receipt
7. facts::tests::current_windows_join_qualifies_only_that_devices_fresh_offer

Existing ADL recording controls now receive a separately supplied fixture-native
property map. Synthetic other buses use distinct synthetic PNP instances, not a
renamed Jupiter alias. Their assertions about ADL counter calls/values are retained.
The new source contains no Vulkan UUID/provider: stable native PCI grant authority,
current-boot LUID mapping and independent compute UUID oracle remain separate.

## R01–R16 review and open evidence

| Rules | Source/accounting and remaining proof |
| --- | --- |
| R01 | New vectors/strings reserve fallibly; checked fields/access; no new unchecked clone/panic. Parent Gpus legacy aggregation allocations are unchanged, not certified by this source record. |
| R02/R03 | Primary sources/actual original red above; native count/grammar/physical bounds documented. No tuning budgets or new dependency versions. |
| R04 | Per-device verified/current-unread state, typed CONFIGRET, no fresh grant on uncertainty; same retained identity/counter receipt. |
| R05 | One provider, no timer/cache/retry; per-sample alias property reads add unmeasured I/O. |
| R06/R07 | Focused pure parser/group/qualification and isolated CM-only FFI with SAFETY explanations. |
| R08/R14 | No performance claim; coordinator comparative/profile/native allocation work owed. |
| R09/R10 | Original native red retained; seven source controls unrun; no fabricated green/mutation. |
| R11/R12 | Shared parser/group behavior; Windows native adapter required by CM API. Windows amd64/arm64 compile/native, other target regressions remain ROOT checks. |
| R13 | No mandatory public inventory field, identity variant, wire change or extra mapper/manager. |
| R15/R16 | Implementer source only; no jobs/native calls/runner changes. Signed named paths; originals/cache/evidence preserved. |

Not complete: source-specific compile/strict gates, actual same-device CM/KMT join
acceptance for all present aliases, native HCS prerequisites/assignment/compute,
Shards Vulkan mapping/guest readiness/dispatch, hotplug/fault/mutation, full resource
and cleanup evidence. Source/protocol proof cannot replace actual execution.
