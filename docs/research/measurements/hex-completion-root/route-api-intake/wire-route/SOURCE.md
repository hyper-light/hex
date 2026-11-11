# Initialized route/backing wire source

Source-only checkpoint; no build, test, native effect or readiness result.
Implementer L4 owns only hex-core wire.rs, wire/tests.rs and this new evidence
directory. ROOT owns integration, outside callers, all gates and main.

POLICY_READ: current Shards ca3ab10c151aa5539e757f1cc76029746ccbb561;
docs/development-rules.md SHA-256
df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
Full AGENTS, R01–R16, development-evidence, Shards CLAUDE and Hex CLAUDE read.
Charter and validation brief remain binding. Base is exact ROOT
8ccb2f81b32dabe0427f084a4aeab59bfbfbcc00. Contract is the four frozen API files
at L1 73547edda8a5188b0a40b3c2aa5d7a5da99be163, not an unstated overlay.

## D-route-wire: existing bounded message seam

Primary sources are the frozen API runtime/spec validators and existing
hex-core wire Writer/Reader/device count/physical-ID codec at these pins.
ROOT explicitly chose this seam and DTO; no architecture or native mechanism
is introduced. JSON fallback, a second catalog and infallible DTO cloning are
unnecessary. The actual native backing and selected-route authority remain
with their existing owners. Wire receipts convey facts/requests, not proof
that a backend executed.

Append-only tags (all older tags retained):

| Message | Tag |
| --- | --- |
| Call::InitializeBacking(BackingGrant) | 0x0014 |
| Call::ReleaseBacking(BackingKey) | 0x0015 |
| Answer::BackingOffer(BackingOffer) | 0x800f |
| Refusal::Backing { key, cause } | 10 |

Capacity's complete previous body is followed by routes, device_routes and
backings, each with a u32 count. BoxSpec and BoxInfo retain their previous
allocation tail followed by an optional RouteReceipt. Existing source-derived
FNV wire identity covers these definitions and exact API runtime/spec bytes;
there is no manual identity number.

Fixed widths derive from the DTO and existing integer codecs: RouteKey is
five enum bytes; BackingKey is pool[32] then generation[16]. A RouteOffer
requires seven bytes plus optional RuntimeCost and BackingKey. A BackingOffer
requires 205 bytes plus nine per physical ID; a BackingGrant requires 228 plus
nine per ID. RuntimeCost retains 152 bytes. Phases are Prepared=0, Granted=1,
Live=2, Draining=3, Released=4; lifecycle causes retain Pending=0,
Uncertain=1, Busy=2. No timing constants or native defaults are added.

The public 64-route eligibility word bounds both routes and backings. Decoding
checks counts and checked minimum-byte products against the received body
before any list reserve, then parses/validates each item before fallible
reserve. Physical claims use the same DeviceId parser, u16 index bound and
sorted/unique provisioning validator. Capacity validation preserves route and
physical order, pairs masks to the canonical device list, rejects absent bits,
ambiguous routes, absent backing references and duplicate pool generations.
Provisioned but unmeasured IDs stay claims, not invented DeviceInfo entries.

New malformed route/backing errors are typed Fault values with static redacted
details. Writing reserves checked complete fixed/provisioning regions before
copying them. No new wait, cache, actor, picker, manager, syscall or native effect
exists. BoxInfo has no device list: its route validation checks declared
class/topology only and is not evidence of selection or current native status.

Request/Call/Answer no longer derive Clone because owning Vec-backed DTOs
have fallible copies. The source inspection found test-only whole-message
clones; ROOT handles the outside client fixtures. Owned wire tests use
Capacity::try_clone only on valid values, move malformed values into the
encoder, and compare borrowed decoded requests. Old property literals and
tail offsets are updated without disabling cases or altering their guards.

## Validation and remaining work

Scoped rustfmt ran on the two owned Rust files. No Rust compile or test ran:
ROOT's exact joined API/reexport/outside-caller pin is required first.
Focused new route/backing roundtrip, hostile mask/count/reference/boundary
controls are the next source checkpoint. Do not rerun the unchanged codec
suite. ROOT owns all native/full gates; native, VM, GPU, mutation, performance,
resource restoration and cross-target behavior remain unverified here.

R01: checked byte arithmetic, fallible list reserve, no new production panic.
R02: pinned authoritative DTO and existing codec sources above; no broader design.
R03: every new numeric width/tag derives from explicit protocol/API contract.
R04: explicit backing phases and typed refusal causes preserved without granting authority.
R05: existing codec seam; no new waits/caches/managers.
R06/R07/R13: focused codec helpers and comments for bounds and factual limits.
R08/R14: no performance claim; ROOT profiling/comparator work remains owed.
R09: new feature source, no live native defect claimed or masked.
R10: exact source manifest retained; actual focused results pending joined source.
R11/R12: shared integer/byte codec, seven-target runtime coverage unrun.
R15: implementer source only; ROOT audits/integration/merge.
R16: no runner/cache deletion or shared data change.
