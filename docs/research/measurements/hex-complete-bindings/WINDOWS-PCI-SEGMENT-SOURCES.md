# Windows PCI segment: documented property and corroboration

Read-only primary research requested by ROOT after the user's segment question.
L1 implementer, policy0a6d91b3/rulesdf2. No parser, provider, catalog, native query,
build/test, driver/HCS/VM/GPU operation or new agent/job was introduced here.
This supplements the earlier WINDOWS-PROVIDER-JOIN.md: ADL and KMT address fields
alone still lack a segment, but Windows has a documented separate property route.

## Direct Microsoft source

[Obtaining PCI Segment Values](https://learn.microsoft.com/en-us/windows-hardware/drivers/pci/obtaining-pci-segment-values)
identifies DEVPKEY_Device_LocationInfo and user-mode CM_Get_DevNode_Property or
SetupDiGetDeviceProperty. It explicitly requires parsing the returned string and
documents these two forms:

| Native PCI topology | Documented property form |
| --- | --- |
| Single segment | `PCI bus 1, device 2, function 3` |
| Multiple segments | `PCI segment 1 bus 2, device 3, function 4` |

The public Markdown is pinned at MicrosoftDocs/windows-driver-docs commit
`e33653d9339e4bc9ee46cb5b3122315516d8577b`, path
`windows-driver-docs-pr/pci/obtaining-pci-segment-values.md`:
[exact source](https://github.com/MicrosoftDocs/windows-driver-docs/blob/e33653d9339e4bc9ee46cb5b3122315516d8577b/windows-driver-docs-pr/pci/obtaining-pci-segment-values.md).
Raw SHA-256 `4a27024cd419b0e4d01312e071ff131155ed3160797da007bc1c4aaec3ea3ef4`,
1200 bytes. File metadata ms.date is2023-03-03; that source commit is2025-03-25.
The rendered page reports2023-03-14. These dates are distinct from a native result.

[DEVPKEY_Device_LocationInfo](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/devpkey-device-locationinfo)
defines a bus-specific DEVPROP_TYPE_STRING and explicitly marks it nonlocalized.
It originates in the bus driver's device-text response; installers have read/write
access. Its exact Markdown revision is
`c708f7b244f8d1f9dfbb35238226b08e44a8cf02`, path
`windows-driver-docs-pr/install/devpkey-device-locationinfo.md`, SHA-256
`cf519c7682934d00aadae8ad2799bfd8949e17625ec50908e2b41e9564492ca9`, 2898 bytes.
Thus this is actual native property evidence, not a cryptographic hardware serial.

## Single-segment zero is a composed, documented inference

UEFI PI specification1.9, Volume5 §1.2, defines segment zero for a system with
one PCI segment in its [PCI-segment term](https://uefi.org/specs/PI/1.9/V5_Introduction.html#term-PCI-segment).
The versioned introduction HTML was retrieved directly and retained at
`~/.cache/hex-complete-bindings/windows-pci-segment-primary-v1/uefi-pi-introduction-0.html`,
47,791 bytes, SHA-256
`adbef6214d96b423dbe7467cc95fc4783931a5439f479bce98caf173c34a1e78`.
The web tool's initial403 is retained as a failed retrieval; the direct public
versioned URL returned this content without credentials or any service change.

Combining Microsoft's documented topology-dependent forms with UEFI's definition
supports interpreting an exact recognized native single-segment PCI form as
segment0. This is an inference from two primary contracts. It is **not** a rule to
fill zero whenever a property is missing, empty, unrecognized, malformed, truncated
or missing a token. A multi-segment form must retain its explicit number. Native
PCI/PNP context and independent BDF/LUID correspondence remain required, and any
contradictory actual full-PCI observation must refuse rather than be overwritten.

## Localization and format caveats

The property reference labels LocationInfo nonlocalized, and the PCI-specific page
documents concrete English token order. However the generic
[IRP_MN_QUERY_DEVICE_TEXT contract](https://learn.microsoft.com/en-us/windows-hardware/drivers/kernel/irp-mn-query-device-text)
passes an LCID, permits bus-dependent/optional location information and describes
locale fallback. This does not justify a permissive digit extractor. A production
parser should recognize only the documented complete PCI grammars, checked
unsigned decimal ranges and exact terminators, and return typed unknown for any
other bus/provider/language/format. It must check native DEVPROP type, actual byte
length, valid UTF16 and termination before parsing. No fallback to an unfiltered
device, localized display name, ordinal, PCIROOT number or guessed address is safe.

The native property must come from the exact actual device instance, not from a
config string or an unrelated enumeration result. A size/list race or failed
query is uncertainty; a bounded refusal is preferable to an unbounded retry.
Cases needed before acceptance include both documented forms, nonzero segments,
wrong type, missing/embedded terminator, alternate text, negative/overflow/out-of-
range numbers, trailing data and inconsistent independent BDF/native identity.

## Corroborate the same device across APIs

1. Retain the actual ADL AdapterInfo PNP/UDID aliases and counter pairing in the
   ONE existing provider. Its BDF alone cannot choose the segment or physical GPU.
   Recorded ADL rows include multiple aliases, so all present aliases need a
   consistent native result; model/ordinal/suffix is not identity.
2. Resolve the exact PNP instance through CM. Read LocationInfo and compare its
   BDF with actual ADL, native BusNumber/Address, and KMT adapter-address output
   when available. Query actual LocationPaths from that same instance for HCS,
   without manufacturing a path. LocationPaths represents device-tree placement;
   it is distinct from LocationInfo and is not itself a segment-number rule.
3. Use the already implemented actual adapter-arrival1ca interface ->KMT full
   LUID join, requiring unambiguous present aliases and exact handle closure.
   Do not reinterpret the stable packed PCI DeviceId as that current-boot LUID.
4. For Shards, compare actual Vulkan valid LUID, UUID and physical-node mask to
   the same current mapping and scoped native report. If the actual device exposes
   VK_EXT_pci_bus_info, require its full domain/BDF to agree as additional evidence;
   no zero-filled extension structure stands in for absent support. The Vulkan
   [ID contract](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceIDProperties.html)
   and [PCI properties](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDevicePCIBusInfoPropertiesEXT.html)
   define those separate observations. A multi-node logical adapter still needs
   exact physical-node correspondence.

## Existing live evidence and what it does not establish

Native QA's already-retained currentidentity1 query at06:12:36.5714381Z returned
Type=String with the single-segment grammar, bus196/device0/function0, for the same
exact PNP instance as its actual1ca/KMT full LUID14c4d mapping. Raw cli.log SHA-256
`0fea22a68ba8a8e7506a06c69b264ec91ac6e16900188877321664214a796021`, script SHA-256
`e57f23277f7060bd97c4b6feca9118fc79ee607e22af6d845713bcca5a66be5c`, signed
fixture783231c9. L1 did not repeat that query. This is a concrete candidate for the
documented composition, not a new production join pass, PCI-extension result,
container assignment, guest execution or proof of every alias/provider/locale.

The official property method removes the rationale for blanket vendor exclusion
as finished behavior. Implementation should be per-device: verified observations
can proceed; missing/conflicting observations preserve typed uncertainty, existing
claims, independent counters and unrelated CPU/device progress. New native scope
and code changes remain subject to the exact reviewed owner boundary; ROOT/QA own
actual native mapping, execution, faults/mutation, resource and supported-target
acceptance. No new public identity variant, wire DTO, catalog or provider follows
merely from this research record.
