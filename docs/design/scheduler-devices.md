# Physical devices in live placement (AC-T7.4.1)

Coordinator: Hex `01a1aa77778471339bea3beff90d6bbd`. Policy read at Shards
`cbd39284937d024640c56db6f8d8ef2bd9b0ee24`, rules SHA256
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
Implementers own scoped code and light checks; the coordinator owns all audits,
native effects, mutations, benchmarks/profiles, integration gates and publication.

## Observed gap and decision

At integrated `37021c35`, scheduler startup and refresh clear all device offers;
placement validation refuses all device-bearing requests. Passing model filters
does not establish actual attachment. The original pure-core recovery test at
`44a2df3b` also demonstrates a surviving GPU assignment colliding with a fresh
assignment after reboot changes GPU load order. The independent transient-load
control at `90230466` refuses retained boxes because of fresh-admission gates.
Both original failures are retained in the lifetime lane. These are actual model
failures, not physical GPU or security results.

Keep the existing picker, Index holders, file records and Runtime seam. Persist
the chosen physical binding in the runner's existing durable record before
Create can escape. Recovery resolves physical identity, never a newly preferred
device. The runtime receives only those identities in BoxSpec; the backend owns
their native configuration. A peer cannot supply a host path or choose another
binding through its payload. Unsupported native adapters refuse before effects.

Physical key namespaces are GPU PCI domain/bus/device/function (32/8/5/3 bits),
VFIO isolation-group number, block major/minor pair (32/32 bits), and nonzero
network interface index. Synthetic model slots carry no physical identity and
cannot become native grants. A known device changing identity must not inherit
the previous holder's reservation merely because its model and size match.

## Existing native mechanisms

Use [CDI 1.1 at 98a7d735](https://github.com/cncf-tags/container-device-interface/blob/98a7d735d9592c1ccca0130e06b0709f4b65ecbf/SPEC.md)
and its pinned Go selector/Apply implementation for Linux vendor edits. The
selection plan preserves global and selected-device operations. Actual injection
must include each operation or return a typed unsupported result; merely writing
a field ignored by Hex's native runtime is insufficient. The parser's pinned
grammar/version/merge decisions and focused results are in
`../research/measurements/hex-device-bridge-cdi/READBACK.md` and `RESULTS.md`.

For Windows, [HCS Device/DeviceType](https://learn.microsoft.com/en-us/virtualization/api/hcs/schemareference#device)
and [hcsshim 7d4e89bc](https://github.com/microsoft/hcsshim/blob/7d4e89bc32c031455054426674d6bd7bc03b3e08/internal/hcsoci/hcsdoc_wcow.go)
provide a precise DeviceInstance/LocationPath assignment. It requires schema2.2;
class-only legacy documents keep their existing schema2.1 output. Class-wide
GUID or GpuMirror alternatives do not represent a single selected device.
The source hashes and focused JSON tests are in
`../research/measurements/hex-device-bridge-windows/IMPLEMENTER.md`.

Alternatives rejected: hand-written vendor library/device recipes duplicate the
vendor's CDI configuration; accepting paths from peer payloads would enlarge the
peer's authority; choosing devices again in the runtime creates a second picker;
deriving surviving bindings from current load reproduces the observed collision.
No additional installed binary, central store, registry watcher or discovery
cache is introduced. Firecracker and Shards exposure remain governed by their
actual initialized API/backend capabilities, rather than the host's hardware alone.

## Backend catalog and record authority

The source-only shared runtime catalog binds physical IDs to administrator-controlled CDI
selection or HCS location data. It is immutable backend configuration, read on
ordinary backend startup; it is not a mutable reservation cache. Backend records
are the sole holder authority, including Creating and Deleting phases. A create
must hold the existing per-device turns while checking those records and making
its Creating record. Physical deletion precedes removal of that record. Invalid
records, multiple holders and unknown bindings refuse without repairing bytes.

The flat catalog starts with `hex-devices-1`; each following line has seven
tab-delimited fields: kind, decimal key, model, decimal size, adapter, source,
selector. CDI uses an absolute spec path and qualified selector. HCS uses an
opaque location source and an empty selector. Tabs/newlines cannot occur within
a field. This backend-only syntax permits fallible copies and a flat bounded
parser without adding a general JSON tree/escape allocator or configuration
framework. Version1 denotes this grammar, not a tuning value. The existing OCI
config byte ceiling bounds reads; the existing u16 scheduler index space bounds
entry count. Files exceeding the byte ceiling are refused, never truncated.

These declarations are trusted administrator configuration, as raw OCI bundles
and CDI hooks already are. Their hardware/model/size correspondence still needs
native validation before an offer can be published. Parsing a catalog is not a
proof that its declaration matches hardware or that an adapter grants only its
declared physical resource. A catalog entry is never permission to skip those
checks. File reload, hook revocation and hotplug cannot silently change a held
binding; missing or changed identities remain typed uncertainty.

## Current evidence and remaining gates

The standalone CDI parser has eight focused passing groups and core Clippy;
the HCS adapter has43 focused passing tests, including nine new cases, and core
Clippy. Their exact signed source and raw logs are retained. These are pure data
checks. Their combined registration, the API/driver/catalog/backend path, actual
device effects, retained-box recovery, cancellation/deletion/claims, relevant
guard-removal controls and the final supported matrix remain coordinator work.
The registered shared catalog passed its five focused tests and native runtime
all-target Clippy at exact signed dependency composition `a03657d1`; its two
source files are byte-identical to this integration. The node, driver, admission
offers and Recorded lifecycle now consume the exact claims in source. Those
combined changes are not yet validated, and Windows native attachment remains a
scoped implementation lane. There is no physical device attachment claim.

R01/R03/R04/R05/R06/R07/R13 require fallible added storage, typed causes, explicit
identity/phase transitions and one authority. R02/R09/R10 sources and original
model failures are linked above. R08/R14 have no comparative performance result;
allocation/fault and equivalent-workload measurements remain owed. R11/R12 have
no physical GPU/VFIO/block/network or WindowsARM pass; the paused VM remains
untouched. R15/R16 require signed scope checkpoints, private admitted remote
checks, preserved evidence and the30GiB local floor. Earlier943-test f3 gates do
not validate this later device implementation or complete the full scheduler goal.
