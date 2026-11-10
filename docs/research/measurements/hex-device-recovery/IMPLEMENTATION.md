# Exact chosen-device recovery

This follow-up to source-only draft 649f8137 implements ROOT's accepted model
defect and physical-identity contract. It is SOURCE ONLY at this checkpoint;
targeted remote unit/Clippy results are still owed. Original collision source
44a2df3b and original-source transient control 90230466 remain unchanged as a
byte-identical prefix of the lane leaf. Their actual reds are frozen separately.

`Device.key: Option<u64>` is optional Copy metadata, serde-defaulted and omitted
when None. A known identity is `(DeviceKind, key)`. GPU constructors/traces retain
None unless the caller supplies an identity. The key does not affect fresh GPU
ranking. Index construction and refresh reject duplicate known identities;
refresh also compares the key of every held indexed device before any change.
Live refresh remains strict about held indices and shapes. Boot can map a known
physical identity to a different current index after an unheld device disappears.

The runner row's first atom is:

```text
bindings1:<owner>:<count>(:p:<gpu|vfio|block|net>:<u64-key>|:s:<u16-index>)*
```

It is followed by the existing space-separated state and exact d1 demand. The
shell's state/demand split and peer message kinds/codecs are unchanged. `p` means
physical identity looked up in the current catalog; `s` means an explicitly
synthetic slot whose current key must be None. There is no fallback between the
representations. Count is bounded by actual current device cardinality, indices
by the existing u16 schema, and physical keys by u64; parsing overflow, missing,
duplicate, out-of-range and trailing claims refuse. Row formatting uses checked
byte bounds derived from those decimal widths, tags and demand length. No new
arbitrary cardinality ceiling is introduced.

The exact choices from existing `Index::holds` are serialized with runner demand
in the same Records.put before Told::Create. `Ran` has no new Held field. Boot
decodes only temporary choices, validates stable class/platform/exposure, device
kind/count/model/total-size, resource room and reservation overlap, then reserves
those indices directly in the existing Index. Retained running boxes do not run
the fresh load/memory/hotness picker. The original-source four-case transient red
establishes why receiver and corresponding local-owner recovery must differ from
fresh policy. Remote/pending owner estimates retain their prior fresh path.
Recovery edits are staged until all claims and the ledger restore validate, so a
later bad claim cannot rewrite earlier durable rows before refusing. This does
not turn independent Records.put operations into a cross-row storage transaction.

Legacy rows with no exclusive device demand recover with empty chosen indices.
Legacy device-bearing rows have no provable chosen assignment and refuse typed
Damaged, preserving input bytes. Missing physical keys or mismatching stable
constraints similarly refuse; another same-shaped device is never substituted.

Driver access is through existing admission holders, with no catalog cache:

```rust
pub fn assigned_devices(&self, key: Key) -> Option<model::Held>;
pub fn assigned_device_specs(&self, key: Key) -> Result<Option<Vec<model::Device>>, Stop>;
```

Known active local runners return Some (empty for CPU-only); unknown, remote-only
and ended runners return None. Device values are copied from the current Index
table into a fallibly reserved vector bounded by the selected holder count.
ROOT's actual backend bridge must reject synthetic None identities and map known
identities to its API/catalog. This source alone proves neither physical catalog
pinning, hotplug continuity nor native GPU attachment/security.

Focused leaf additions cover the single put before Create with before/after
crash seams, malformed/legacy device claims and exact byte preservation, overlap
after an earlier staged recovery rewrite, matching legacy CPU/replay/delete,
physical reindex and same-shaped replacement, and exact Index refusal atomicity.
All effects are pure model observations; no hardware/VM is started. Existing
matching recovery controls in peer/index will be selected in private light checks.

Policy/roles: current cbd39284937d024640c56db6f8d8ef2bd9b0ee24, full R01–R16
rules SHA256 df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3;
implementer01a1c640e6ad7127bcf34212f474d37c, ROOT01a1aa77778471339bea3beff90d6bbd.
Expanded owned paths are Device metadata in model.rs, durable recovery in
peer.rs/index.rs, the lane leaf, and this evidence directory. No children/goals,
native/VM/GPU campaigns, profiling, benchmarks, gates or merges are assigned here.
ROOT owns those and combined review. Approved light checks use d132 only, NEW
private source/target/cache, cpus1/mem4g/disk12g, nice10/jobs1/threads1, no-sync.
Local source-only formatting/Git is permitted; no laptop build/test is run.

R01/R04/R06/R07/R13: typed refusals and explicit ephemeral recovery choices,
checked derived bounds/fallible new scratch reservations, no persistent parallel
authority. R02/R03/R05/R12: existing Records/Index seams and supplied physical
identity contract, no new picker/cache/wait/manager/API dependency; repository
source and the exact retained reds are the decision evidence. Known-key validation
sorts at most the actual catalog; decoding physical claims performs bounded
catalog lookups per claim. No cost/complexity improvement is claimed.
Specifically, duplicate known-ID validation reserves at most N `(DeviceKind,u64)`
pairs, where N is the actual catalog length (at most the existing 65,536-device
u16 index space), then sort plus adjacent comparison: O(N log N) time and O(N)
temporary elements. It neither scans every prior prefix nor changes catalog order
or selected holders. No stored validation cache or benchmark is introduced.
R08/R14: no performance/allocation claim or profiling/benchmark run. R09/R10:
original model reds retained; fixed execution and ROOT mutation evidence owed.
R11: portable pure source, supported-target and native attachment proofs owed
ROOT. R15/R16: signed source checkpoint, explicit unrun light scope, protected
refs/artifacts retained; ROOT handles local disk recovery, with only tiny source
writes and in-memory streaming allowed while bulk local transfers are held.

## First actual fixed light run, retained separately

At exact signed 9bc06bf9, approved private r10 executed the leaf9/0, existing
peer24/0 and index25/0 (all zero ignored; respectively158/143/142 filtered).
The original collision and four transient cases passed unchanged; physical
reindex, replacement, malformed/overlap, persistence and CPU/replay controls
also passed with their cleanup witnesses. This is a pure model/unit result,
not native attachment. All source hashes before/after match.

The same run's library Clippy returned101 for one new `manual_contains` style
lint. This follow-up only replaces that expression with the equivalent
`holders.contains(&Some(workload))`; revalidation is pending at this checkpoint.
The first green/lint-failure run is retained without overwriting any original.
Leaf log SHA256 e0b1d1b3be42a8ef2fc356a6d2b1f922e8e7cc7a97ffc81ea437728a97a16fce;
Clippy log SHA256 1284d0716be57a4319e265564edbd23d62657bec0715e216f11bd318ccd1ebe7.
