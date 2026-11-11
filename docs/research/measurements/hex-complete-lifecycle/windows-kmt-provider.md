# Exact current Windows adapter query

Implementer L3, policy0342b85d9c808e3ced041a6c3ef0e9bf70438272 / rules
df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
Focused scan controls collected below; native GPU calls0. ROOT owns
actual Windows mapping/compute/HCS/VM proof, integration, full gates and merge.

windows::oci::sys::adapter_luid_for_instance(&str)->Result<[u8;8],Refusal> is a
query seam for the existing initialized provider. It has one exact PNP instance
input; it discovers no replacement device and supplies no reservation authority.
The present1CA adapter-arrival interfaces are queried with a nonnull instance
filter, then each actual returned interface is opened through D3DKMT. Successful
aliases must agree on the full LUID, including the signed HighPart bit pattern.
Every successful adapter handle is closed with D3DKMTCloseAdapter; an open/close
failure or ambiguity refuses the mapping and preserves raw CONFIGRET/NTSTATUS.
No ordinal/model/default/WARP/segment0 substitution is made. No driver is changed.

The existing MAX_INTERFACE_UNITS bounds the instance and CM multi-string. A
size/list race returns the actual failure without retry; malformed terminators
are refused. Copies reserve fallibly. The bounded process-wide loader stores only
the two Gdi32 function pointers (and its one load result), following the existing
HCS loader's System32-only lifetime; it stores no device mapping or inventory.
Every query reestablishes the boot-scoped native identity. LUID is not a persistent
physical PCI key. The same existing provider must still join its full PCI/UUID
identity to this PNP observation before advertising an attachment capability.

The exact source notes and immutable Microsoft source pins are already in
docs/hex-windows-gpu-compute-mapping/README.md (read in full). Primary contracts:

- https://learn.microsoft.com/en-us/windows-hardware/drivers/install/guid-display-device-arrival
- https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/d3dkmthk/nf-d3dkmthk-d3dkmtopenadapterfromdevicename
- https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/d3dkmthk/ns-d3dkmthk-_d3dkmt_openadapterfromdevicename
- https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/d3dkmthk/nf-d3dkmthk-d3dkmtcloseadapter
- https://learn.microsoft.com/en-us/windows/win32/api/cfgmgr32/nf-cfgmgr32-cm_get_device_interface_listw

SDK types are Microsoft's windows-sys0.61.2 Wdk::Graphics::Direct3D structures;
ROOT adds feature Wdk_Graphics_Direct3D to the existing dependency (no new version)
and extends the existing windows/oci/sys.rs unsafe rationale for the two queries.
No handwritten ABI or static Gdi32 load dependency is introduced.

The focused standalone controls compile the exact production scan/error/type
fragment with those SDK structures and fake native function pointers on admitted
d132. They check full signed LUID bits, equal aliases, ambiguous aliases, raw open
and close failures, exact handle close counts, malformed and empty interface
lists. This is no native CM/KMT call, complete Windows-source compile or actual
mapping proof. Run submit_kmt.py EXACT_SUBJECT UNIQUE_TAG:1CPU/4GiB/12GiB,
nice10/jobs1/testthreads1, precise no-sync, private source/target/cache/temp. The
worker records whole-source and exact excerpt hashes/ranges and separate lock.

R01/R04/R06/R07/R13: explicit refusal/handle/alias paths, fallible new buffers,
same bounded query owner; no allocation-free or leak claim. R02/R05/R12: primary
supported exact PNP query at existing Windows seam, no inventory/picker/cache.
R03: only existing bound, documented GUID/API/error widths. R09/R10: new required
provider feature with hostile-input/error controls, not a reproduced
native defect. R11/R15: real supported Windows/contexts/ABI/GPU/VM/full gates still
ROOT work. R08/R14: no performance claim, measurements remain ROOT. R16: owned
source only, no native calls, foreign processes/data or runner changes.

## Exact focused control result

Subject64db27811d6fe460c04e6191230c456b9c44da11 over fixed shared
6dbcfa4f89484a01a1419121f473bae746458294; source SHA256
470f7208ed97ffb30cbac3c63c8f7b6fff9524f0d34f55c59088d712a31644c8.
The admitted d132 private kmt-controls-v1 run selected the same five named scan
controls: **5 passed,0 failed,0 ignored,0 filtered; Cargo0**. The exact worker
and source transfer were submit_kmt.py64db2781 kmt-controls-v1. All recorded
Cargo children were reaped; private temp entries0; native backend calls0.
The standalone shell emitted one dead-code warning for AdapterLoadFailure,
which belongs to the omitted native loader. This was not a Clippy/full-file gate.

Retained kmt-evidence.tar.gz SHA256
ea4ec3598426ff1a8a5ee3a961e7d97ad8721339a74b82c47d18cd6cf4cee81e
contains raw Cargo logs, result/run metadata and the separate lock. Every raw
log member matches its recorded SHA256. kmt-evidence.json records the actual
result and shell limitations; test binary SHA256
38d2a5bf9680c55279f0a64d37ae424ad0e1ac2fa1c2f01d125ab9c2933b845b.
The transfer CLI exited1 after printing that actual result with "sent no receipt
of it". Its error is retained separately; a later admitted read-only collection
of those exact owned files exited0. No unchanged test was rerun or native
result inferred from the transport receipt failure.

## AMD native identity boundary

The current ADL reader discards native AdapterInfo.strPNPString, constructs
domain0 from a BDF without a segment, and groups rows by that value before the
provider can verify their identity. This is a source observation; it is not a
new native defect reproduction. Merely withholding these rows leaves the
required AMD path incomplete. L1 owns the necessary hex-gpu/facts extension:
retain the exact bounded native PNP string associated with each ADL adapter row
and its telemetry through the join, and deduplicate only after verified native
identity. Model/display/configuration strings and an ADL BDF cannot supply that
association. The existing Jupiter fixture's five logical adapter suffixes do
not license stripping suffixes or choosing an unverified survivor.

AMD's [AdapterInfo contract](https://gpuopen-librariesandsdks.github.io/adl/structAdapterInfo.html)
defines the PNP field separately from the display name. The pinned ADL header
already used by Hex is display-library84c881cb (gpu-telemetry.md G3). The same
Prober/Gpus owner must pass that native instance to this helper. Microsoft's
[CM contract](https://learn.microsoft.com/en-us/windows/win32/api/cfgmgr32/nf-cfgmgr32-cm_get_device_interface_listw)
filters currently active interfaces by the supplied device instance ID; an
empty ID enumerates all devices, which this helper refuses.

L2 then requires a unique positive native Vulkan match with deviceLUIDValid,
all eight LUID bytes and the supported physical node mask, returning the actual
nonzero deviceUUID. The [Vulkan identity contract](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceIDProperties.html)
also cautions that UUID persistence is not guaranteed across physical moves or
all driver versions: recovery must reestablish the join and retain uncertainty
on change. LUID is a boot-local observation, never a fabricated PCI integer.
L1 owns an explicitly opaque DeviceId/key representation and collision refusal
if a verified full PCI address is unavailable. No second inventory, Gpus open,
picker or holder ledger is introduced. Exact provider/API source integration
and actual Windows AMD/HCS/Shards compute proof remain open, ROOT-native work.
