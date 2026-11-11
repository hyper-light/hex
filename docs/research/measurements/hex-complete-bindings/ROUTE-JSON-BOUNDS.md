# Bounded decoding of initialized route ownership

AC-T7.4.1, R01/R03/R05/R06/R10. Implementer L1; current policy
`0a6d91b3de0f10a8397868aa3794adab1e09afdc`, four policy blobs byte-equal to
fully read `0c598386`, rules SHA256
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.

ROOT's f3 source review identified a NEW allocation boundary: derived serde Vec
decoding could allocate route/backing/mask/claim lists before `RuntimeRoutes::share`
checked their lengths. It was not merely inherited generic serde allocation.
This successor preserves immutable f3 and the separate in-flight ROOT4f route
control cohort. No allocation/OOM reproduction, passing control or native effect
is claimed here; this is required new-feature input handling, source-first.

The SAME private bounded sequence visitor now serves API Capacity route/mask/
backing fields, scheduler RuntimeRoutes' identical fields, and the nested physical
claim vectors in BackingOffer/BackingGrant. Three specialized Capacity serde field
entrypoints share it across the crate boundary without a new DTO/module/registry.
No wire schema, tags, field order, ownership, selection, fallback or resource
formula changes. Serde defaults retain empty legacy native Capacity fields;
absent/null scheduler routes remain absent, and empty sequences allocate no Vec.

Bounds are the existing u64 eligibility width (`MAX_RUNTIME_ROUTES=64`) and existing
u16 physical address space (`MAX_DEVICES=65536`). Every backing needs a referencing
route, so the backing vector uses the SAME 64-entry bound. Physical claims may be
repeated between competing provisioning offers; each offer/grant still has its
existing bounded sorted/unique validation before effects. Parsing preserves input
order. Structural/topology/identity checks remain mandatory after decoding.

An excessive advertised sequence length refuses without element decoding. For an
unknown-length stream, the next excess element receives a rejecting seed before
its value or nested claims can be materialized. Checked geometric growth clamps
to the actual bound, and each Vec expansion uses `try_reserve_exact`; no input size
hint is used as allocation authority. Overflow/refusal uses fixed redacted serde
errors. This is source reasoning, not an allocation/performance measurement.
Inherited serde machinery/error construction, existing older DeviceInfo/String
vectors and unrelated allocators remain explicit broader R01 review work; this
increment does not certify every serde/library path panic-free.

Four bounded pure controls are **unrun**: exact/over route counts and an enormous
size hint with counted element visits; actual Capacity route/mask/backing fields;
exact/over nested offer/grant claims; and actual scheduler shared-owner decoding
including null/empty CPU paths. No hardware, Gpus provider, native controller or
process starts. Scoped installed-toolchain rustfmt and diff checks passed; no
local Cargo/build/test, new dispatch, production route remedy or campaign ran.
ROOT owns the joined light checks and later R14/native/full validation. The
companion manifest pins the two source files and exact selected control names.
The model JSON fixture needs ROOT's `serde_json.workspace = true` dev-dependency
edge in hex-sim and its matching lock edge; the workspace already pins1.0.154.
No new crate version is requested. L1 does not edit shared manifests/lockfiles.
