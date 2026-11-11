# Exact Windows GPU PNP identity to DXGI LUID

SOURCE / PRIMARY RESEARCH ONLY, 2026-11-10. No executable change, compilation, test,
native read, GPU command, HCS/VM operation or delegated child was performed.
The API composition below is an implementation proposal supported by pinned Microsoft
documentation; it is not an observed Jupiter mapping or compute pass.

Implementer: 01a1c640e6ad7127bcf34212f474d37c. Coordinator:
01a1aa77778471339bea3beff90d6bbd. Owned path only:
`docs/hex-windows-gpu-compute-mapping/**`.
Research parent: `8e5703b13c8649c8cfbc95d6650163c173fad4ca`;
branch `cp/t7-windows-gpu-compute-mapping`.
Preserved refs: Linux c646, Mac 0e/8cc, Windows witness source888/evidence3f3b.
ROOT explicitly ACKed this docs-only scope. Do not replay their older queued work.

## Decision and shortest supported route

Use the exact known Windows **device instance ID**, ask Configuration Manager for
that device's present **adapter-arrival interface**, open the OS-returned interface
through D3DKMT, then select DXGI by the returned LUID. Pass the resulting explicit
adapter to D3D12 and verify the D3D device reports that same LUID. This composes
documented identity-bearing APIs without choosing by model, vendor, ordinal, or
default device. The composition and required refusal points are our inference from
the individual contracts, not a native result. [arrival][arrival] [cm_list][cm_list]
[open_struct][open_struct] [enum_luid][enum_luid] [create][create] [device_luid][device_luid]

1. Start with ROOT's exact current PNP instance ID and the recorded PCI-to-PNP
   observation. Call the existing CM size/list API pair using
   `GUID_DISPLAY_DEVICE_ARRIVAL`, that **non-null instance-ID filter**, and
   `CM_GET_DEVICE_INTERFACE_LIST_PRESENT`. CM documents that this restricts the
   result to interfaces supported by that exact device. Keep the actual returned
   symbolic links; never manufacture one by changing a GUID suffix. [cm_list][cm_list]
2. Use each returned actual interface as the NUL-terminated
   `D3DKMT_OPENADAPTERFROMDEVICENAME.pDeviceName` input to
   `D3DKMTOpenAdapterFromDeviceName` (Gdi32). Its success returns both the owned
   `hAdapter` and `AdapterLuid`. The adapter-arrival documentation explicitly
   identifies this open as a way to access its adapter. Preserve NTSTATUS and do not
   treat it as a Win32 GetLastError/HRESULT result. [arrival][arrival] [open][open]
   [open_struct][open_struct]
3. Require an unambiguous current mapping. Empty results, failed opens, or
   conflicting successful LUIDs are unresolved/refused, never a reason to choose
   another GPU. Equivalent interface aliases may identify the same LUID; interface
   order is not identity. ROOT must verify this policy against actual returned
   aliases. Close every acquired KMT handle with `D3DKMTCloseAdapter`, recording
   any failure; this is not a Win32 CloseHandle handle. [close][close]
4. Request the exact returned LUID through `IDXGIFactory4::EnumAdapterByLuid`.
   Check the HRESULT and use that returned adapter explicitly in
   `D3D12CreateDevice`. Do not pass a null adapter: the documented null behavior
   chooses the first enumerated adapter. A null output pointer is only a capability
   check, not an actual created compute device. [enum_luid][enum_luid] [create][create]
5. Require `ID3D12Device::GetAdapterLuid()` to equal the expected LUID's full two
   32-bit fields, retaining the signed HighPart's bit pattern. Then record
   `GetNodeCount()`. It counts physical adapters associated with the device.
   A logical adapter with multiple physical nodes still needs an exact physical
   node association before a single-physical-GPU compute claim. Node zero is not
   established by enumeration order. [device_luid][device_luid] [node_count][node_count]

The LUID is a local runtime identity, not Hex's persistent physical PCI key; its
documented guarantee does not survive a system restart. Re-resolve the current
PNP/interface-to-LUID join for each native fixture invocation. Do not cache it or
copy key50176 into an AdapterLuid field. [device_luid][device_luid]

## The three class identifiers must remain distinct

| Identifier | Meaning/provenance |
| --- | --- |
| `{1CA05180-A699-450A-9A0C-DE4FBE3DDD89}` | `GUID_DISPLAY_DEVICE_ARRIVAL`: adapter interface, including render-only/compute-only types. The documentation explicitly discusses D3DKMT opening. [arrival][arrival] |
| `{5B45201D-F2F2-4F3B-85BB-30FF1F953599}` | `GUID_DEVINTERFACE_DISPLAY_ADAPTER`: despite its name, Microsoft describes **display views** supported by adapters. This is ROOT's observed interface below. [view][view] |
| `{4D36E968-E325-11CE-BFC1-08002BE10318}` | Display **setup** class supplied in ROOT's readback, not either interface class. It is not substituted into the CM interface query. |

Direct KMT acceptance of ROOT's existing 5b452 view-interface path is **unverified**.
The generic open contract says device name; it does not itself prove that this exact
Jupiter string will work. Use the documented adapter-arrival route above as the
research recommendation, or retain an actual successful direct-open/readback from
ROOT before treating the shortcut as established. [view][view] [arrival][arrival]
[open_struct][open_struct]

## Concrete ROOT observations; no inferred LUID

ROOT's signed telemetry proof432ce469 (source9190738c) records one Radeon(TM)8060S
Graphics, ADL25.20.32.06, PCI0000:c4:00.0 / Hex key50176, memory4294967296 bytes.
WMI's32-bit AdapterRAM4293918720 is not used as authoritative memory.
The committed OS inventory gives:

```text
PCI\VEN_1002&DEV_1586&SUBSYS_1FB31043&REV_C1\4&35FE04F8&0&0041
```

ROOT's later read-only pnputil observation (user message, not a lane native call)
reports an enabled interface and the same DEVPKEY_Device_InstanceId:

```text
\\?\PCI#VEN_1002&DEV_1586&SUBSYS_1FB31043&REV_C1#4&35fe04f8&0&0041#{5b45201d-f2f2-4f3b-85bb-30ff1f953599}
```

ROOT's raw query/log directory is
`~/.cache/hex-lead/windows-gpu-interface-read-v1/`; that raw local directory
was **not read or independently verified by this lane**. Known location paths,
also supplied by ROOT, are PCIROOT(0)#PCI(0801)#PCI(0000) and
ACPI(_SB_)#ACPI(PCI0)#ACPI(GPPA)#ACPI(VGA_).
No LUID value, successful KMT open, D3D feature level, node count or compute result
has yet been supplied to this note. These values are ROOT observations/input, not
fixture defaults, guessed schema or proof from matching model names.

The source record read here is ROOT parent8e5703b:
`docs/research/measurements/hex-windows-gpu-native/{RESULTS.md,os-inventory.json}`.
It explicitly disclaims HCS/DirectX/compute proof.

## Minimal existing seam and exact source gaps

- ROOT's `crates/hex-runtime/src/windows/oci/sys.rs::device_interfaces`
  (parent8e5703b, around1162) already uses the CM size/list functions, PRESENT flag,
  UTF16 paths and query opens. Its device-ID filter is currently null and it returns
  only listed/reachable counts. Therefore **that existing output cannot establish
  this identity mapping or compute**. Reuse those OS mechanisms in a future small
  owned fixture; do not introduce an inventory manager or widen this production
  count API merely for research. No code change was made here.
- The assigned witness source88876436/evidence3f3b already has an exact path
  `query` function and an ignored, privately mapped child test executable.
  That is a concrete future fixture seam for ROOT's selected-only/foreign checks.
  It currently performs CreateFileW query0, not D3D commands. The present read-only
  assignment does not authorize modifying it or adding test FFI.
- `crates/hex-gpu/src/adl.rs::identify` in parent8e5703b reads ADL bus/device/
  function and explicitly assigns PCI domain0; no PNP or DXGI LUID is returned.
  This is a **source observation**, not a newly reproduced defect. Existing ADL
  telemetry/model equality cannot replace the exact current PNP join.
- The scoped source search found no D3DKMTOpenAdapterFromDeviceName,
  EnumAdapterByLuid or D3D12CreateDevice call in the inspected runtime/GPU/CLI
  sources. This is a symbol search, not an aggregate audit or native capability
  result. ROOT owns any future dependency/feature/FFI decision and compiler proof.

## Alternatives and why they are not the primary identity proof

**PCI bus address query:** `KMTQAITYPE_ADAPTERADDRESS` /
`D3DKMT_ADAPTERADDRESS` supplies bus, device, function, but **no PCI segment/
domain**. It is useful corroboration (c4,0,0 for the observed adapter), insufficient
by itself for general full PCI identity. Do not infer segment0 merely because the
structure has no such member. [address][address]

**Reverse CCD path:** after obtaining a candidate LUID, DisplayConfigGetDeviceInfo
with GET_ADAPTER_NAME returns an adapter device name. Its header identifies the
LUID; an exact OS-backed device-instance association could corroborate the join.
However this call documents console-session access failures, and does not establish
coverage for headless/remote/container execution. It is a diagnostic alternative,
not silent fallback to a preferred adapter. [adapter_name][adapter_name]
[display_config][display_config]

**Physical-adapter PNP key:** KMT's physical-adapter PNP-key structure has an LDA
physical index, hardware/software key selector, caller buffer and in/out character
count. It is a possible linked-adapter follow-up, supported from Windows10 1703 /
WDDM2.2. Its string format, key-to-instance conversion and exact node correspondence
have not been verified here; do not assume a hardware registry-key string is already
the full device instance ID or choose index0. This route is not needed for the
proposed exact single-node PNP/interface route. [pnpkey][pnpkey]

**Interface-property shortcut:** CM_Get_Device_Interface_PropertyW reads a property
for an actual interface, using byte counts and a returned type. The InstanceId
documentation explicitly names devnode/SetupAPI access for its instance property.
This note does not infer that any property works through every interface API.
ROOT's actual pnputil InstanceId read is retained as supplied evidence; a future
in-process reverse property read must check its supported property, type, byte
length and result, or use a documented SetupAPI/devnode association. [cm_property][cm_property]
[instance][instance]

## Exact remaining ROOT proof

1. Obtain present adapter-arrival interfaces for the known instance ID in the actual
   execution context; retain the raw class/instance/path association. Size/list races
   must stop under ROOT's original bound with explicit uncertainty, not an endless
   loop, new arbitrary ceiling, replacement device or increased wait.
2. Record successful KMT mapping, complete LUID, native status and exact handle
   close. Check native SDK ABI/calling convention/exports on the actual target;
   none were compiled or invoked here.
3. Resolve that LUID to DXGI, reject an absent/changed/ambiguous mapping and software
   substitution, create the explicit D3D device, read back LUID and physical node
   count. Do not fall back to null/default/WARP or choose by model/ordinal.
4. Execute a bounded deterministic compute dispatch on that device, verify completed
   GPU work and every expected output plus errors/removal/cleanup. Creating a device
   or opening an interface is not compute. This note designs no worker, timing,
   shader compiler dependency, benchmark or production API.
5. Re-establish identity inside the actual granted container/guest context as
   applicable; a host-only mapping cannot prove HCS/VM attachment or exclusivity.
   Keep selected-only behavior independent of unavailable second-GPU hardware.
   Exact foreign-GPU compute/access refusal needs its own actual positive control.
6. Record native handles/references/resource cleanup, original errors and uncertain
   cleanup separately. Preserve all prior raw telemetry/interface evidence.
   ROOT owns native reads/commands/HCS/VM/mutations/audits/gates/profile/merge.

No OS-wide Unsupported refusal or missing hardware is counted as completion.
All native mapping/compute outcomes remain UNVERIFIED, not skipped passes.

Source-first notes commitc5e60644 retained an extra blank line at EOF, reported by
Git whitespace checking. Its notes-only successor removes that formatting issue;
no runtime source or result was changed and no native check was repeated.

## Policy and R01–R16 record

Current Shards origin/main a920c85b5c0c9d40aafc4f4cce32432c7c0aed79, rules SHA256
df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
Full current AGENTS/R01–R16/evidence read for this scope; current Shards CLAUDE,
managed plan and g121 handoff were read in this session at the same policy.
Full Hex CLAUDE at8e5703b and current HANDOFF reread.
POLICY_READ key `windows-gpu-compute-mapping-policy-a920-read-v1`, explicitly
ACKed by ROOT before notes. Research skill's primary-source/single-note method was
applied personally; user's explicit no-children instruction overrides its delegation
default. Only new notes in the assigned directory are changed.

| Rules | Applicable record and remaining limits |
| --- | --- |
| R01/R04/R06/R07/R13 | No executable edits. Native failure/ownership/identity boundaries are specified as research obligations, not implemented or checked behavior. |
| R02/R03/R05 | Pinned Microsoft sources below; exact identity composition, alternatives and ambiguity/size/lifetime gaps retained. No new constant/cache/picker/manager proposed. |
| R08/R14 | No latency, resource, allocation, leak or comparison claim. All profiles/benchmarks remain ROOT-owned. |
| R09/R10 | Research requirement, not a reproduced bug or fix. This signed source-first note records source evidence and unknowns; no red/green test or mutation result is claimed. |
| R11/R12 | Windows documentation/method only; actual amd64/arm64/context/API availability unverified. No FFI/production/test/dependency/installed binary change. |
| R15/R16 | Implementer docs-only, no children/goals/jobs/build/tests/native/VM. Git reads/HTTP reads/tiny note/signing only; prior refs/evidence and all caches preserved. Local free207444020KiB before note (>30GiB); no deletes. ROOT integration/gates/merge owed. |

## Immutable primary sources

The Microsoft documentation repos were resolved through their public commit API;
the exact raw files were fetched in memory and SHA256 measured. No SDK or binary
was installed. No third-party article is used as API authority.

- MicrosoftDocs/windows-driver-docs:
  `7908c35fbacf43436572231d4503d542d30c5859`.
- MicrosoftDocs/windows-driver-docs-ddi:
  `7515063cea4c9e98db6a92986c5b4ddb0463fd16`.
- MicrosoftDocs/sdk-api:
  `656e9656bfaef34dd29cd7ee9e1146aec17e924c`.

The following link labels are immutable commit/file links, not moving branch URLs.
Byte/hash values identify the full referenced Markdown, not an excerpt.

| Source | Bytes | SHA256 |
| --- | ---: | --- |
| [arrival][arrival] | 2537 | `268aae36188c3694c12a96b42d2f5a4d24ce52c3457db27cc5d7a41e53a71c40` |
| [view][view] | 1972 | `99672e17d618ac47d304ca6efdbaa03cfc4957e11149159cc4126103f80b28d6` |
| [instance][instance] | 2477 | `b1e3cf99f26aa02aaca8fdd91832ade1a97192a71560ddaa3d162c52fb4b19b2` |
| [open][open] | 2721 | `e3df61d06cdc1aab8f35f72e772c93bf9748e0ed88b755daa7fda31d961a423c` |
| [open_struct][open_struct] | 2713 | `874bb4df7f08805461be230872de52412a930444eb6f819df04eefbfc60dd173` |
| [close][close] | 2147 | `0dcb93181d22cdf215ecaf71822d93e19d2873dcfab346afc84a01004103b159` |
| [address][address] | 2013 | `262548d0a5e0631fb60c2e876672978becdac69c56279cdb8836118adac96706` |
| [pnpkey][pnpkey] | 2141 | `b1fe76e261c868ccff2fdf9e42d3cab2330e7332c741d756239b548442bb8ebb` |
| [cm_list][cm_list] | 7822 | `c0797ec6936868e377f5173dc2cd4bfa6046696eaab4f87bb86be5d3b101457a` |
| [cm_property][cm_property] | 4432 | `5a1e9e9c08d6447828d0969780940997a3f3287ee386effb479559de4d2ee507` |
| [enum_luid][enum_luid] | 3753 | `ad1ede066e5a65bf1f75fa28052c369fb73b86bb5e051f0bb9cbbb35dde5d0df` |
| [create][create] | 6598 | `116beb02c807bae0ed7174f1e093fd9f6106e37f311a001903baf8aff7fccd23` |
| [device_luid][device_luid] | 2330 | `d8afb7ee3254dc365d699de51277456d48dca970b3271124432b6ead08183148` |
| [node_count][node_count] | 1860 | `cb881ca2df2074c2984a17f708de5eb09f6f95a4fbe5421361d1b0974925cf1b` |
| [display_config][display_config] | 5034 | `6f74eb88f21118b6d199a1424579284102996de5b9f96f817156480b120a2bda` |
| [adapter_name][adapter_name] | 2586 | `5eaa1f01b9f71d1923f7f4f67cd473436948af8d38ad383f4ab2e7bbeddc9095` |

[arrival]: https://github.com/MicrosoftDocs/windows-driver-docs/blob/7908c35fbacf43436572231d4503d542d30c5859/windows-driver-docs-pr/install/guid-display-device-arrival.md
[view]: https://github.com/MicrosoftDocs/windows-driver-docs/blob/7908c35fbacf43436572231d4503d542d30c5859/windows-driver-docs-pr/install/guid-devinterface-display-adapter.md
[instance]: https://github.com/MicrosoftDocs/windows-driver-docs/blob/7908c35fbacf43436572231d4503d542d30c5859/windows-driver-docs-pr/install/devpkey-device-instanceid.md
[open]: https://github.com/MicrosoftDocs/windows-driver-docs-ddi/blob/7515063cea4c9e98db6a92986c5b4ddb0463fd16/wdk-ddi-src/content/d3dkmthk/nf-d3dkmthk-d3dkmtopenadapterfromdevicename.md
[open_struct]: https://github.com/MicrosoftDocs/windows-driver-docs-ddi/blob/7515063cea4c9e98db6a92986c5b4ddb0463fd16/wdk-ddi-src/content/d3dkmthk/ns-d3dkmthk-_d3dkmt_openadapterfromdevicename.md
[close]: https://github.com/MicrosoftDocs/windows-driver-docs-ddi/blob/7515063cea4c9e98db6a92986c5b4ddb0463fd16/wdk-ddi-src/content/d3dkmthk/nf-d3dkmthk-d3dkmtcloseadapter.md
[address]: https://github.com/MicrosoftDocs/windows-driver-docs-ddi/blob/7515063cea4c9e98db6a92986c5b4ddb0463fd16/wdk-ddi-src/content/d3dkmthk/ns-d3dkmthk-_d3dkmt_adapteraddress.md
[pnpkey]: https://github.com/MicrosoftDocs/windows-driver-docs-ddi/blob/7515063cea4c9e98db6a92986c5b4ddb0463fd16/wdk-ddi-src/content/d3dkmthk/ns-d3dkmthk-_d3dkmt_query_physical_adapter_pnp_key.md
[cm_list]: https://github.com/MicrosoftDocs/sdk-api/blob/656e9656bfaef34dd29cd7ee9e1146aec17e924c/sdk-api-src/content/cfgmgr32/nf-cfgmgr32-cm_get_device_interface_listw.md
[cm_property]: https://github.com/MicrosoftDocs/sdk-api/blob/656e9656bfaef34dd29cd7ee9e1146aec17e924c/sdk-api-src/content/cfgmgr32/nf-cfgmgr32-cm_get_device_interface_propertyw.md
[enum_luid]: https://github.com/MicrosoftDocs/sdk-api/blob/656e9656bfaef34dd29cd7ee9e1146aec17e924c/sdk-api-src/content/dxgi1_4/nf-dxgi1_4-idxgifactory4-enumadapterbyluid.md
[create]: https://github.com/MicrosoftDocs/sdk-api/blob/656e9656bfaef34dd29cd7ee9e1146aec17e924c/sdk-api-src/content/d3d12/nf-d3d12-d3d12createdevice.md
[device_luid]: https://github.com/MicrosoftDocs/sdk-api/blob/656e9656bfaef34dd29cd7ee9e1146aec17e924c/sdk-api-src/content/d3d12/nf-d3d12-id3d12device-getadapterluid.md
[node_count]: https://github.com/MicrosoftDocs/sdk-api/blob/656e9656bfaef34dd29cd7ee9e1146aec17e924c/sdk-api-src/content/d3d12/nf-d3d12-id3d12device-getnodecount.md
[display_config]: https://github.com/MicrosoftDocs/sdk-api/blob/656e9656bfaef34dd29cd7ee9e1146aec17e924c/sdk-api-src/content/winuser/nf-winuser-displayconfiggetdeviceinfo.md
[adapter_name]: https://github.com/MicrosoftDocs/sdk-api/blob/656e9656bfaef34dd29cd7ee9e1146aec17e924c/sdk-api-src/content/wingdi/ns-wingdi-displayconfig_adapter_name.md
