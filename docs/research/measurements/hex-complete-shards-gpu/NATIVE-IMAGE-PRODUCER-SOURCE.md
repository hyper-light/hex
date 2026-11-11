Native image initialization producer — source only, 2026-11-11

ROOT authorized this producer within the existing guest/API/private helpers
partition. It implements declarations, typed preparation and actual file-content
verification. It implements no native process launch, helper opcode, worker
protocol, FFI or closed-member proof. Policy7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506,
rules df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3,
implementer; four policy blobs remain byte-identical to0342.

The existing evidence is the only declaration document

load_runtime_inputs(file,expected_sha,evidence) performs the same bounded cost
and evidence reads and checks the complete evidence SHA against RuntimeCost.id.
It then parses an optional prefix, without writing or re-encoding any input:

```
hex-shards-native-images 1
cli SIZE FULL_SHA256
kernel SIZE FULL_SHA256
cpu-vmm SIZE FULL_SHA256
gpu-vmm none

<original opaque measurement evidence bytes>
```

The four roles and their order are exact. Only either VMM row may be `none`, and
at least one VMM declaration is required. SIZE is a positive canonical u64 decimal;
FULL_SHA256 is exactly64 ASCII hex digits. A GPU declaration is never inferred
from a CPU declaration. Both variants can be declared. There are no image paths,
UUIDs, model/ordinal mappings or provider facts in this section. The original
measurement body may contain arbitrary bytes; the parser does not recurse into it.
The existing MAX_CONFIG_BYTES bounds the entire document before parsing, with no
new data-dependent allocation in this fixed-role reader.

Absent prefix yields legacy None. A malformed PRESENT prefix refuses; an old
evidence identity is never upgraded by adding bytes, guessing fields or rehashing
on the caller's behalf. To supply new declarations, ROOT must provide an actual
reviewed evidence document and its matching RuntimeCost.id/cost-file digest.
Fixture bodies are explicitly models, not accepted cost measurements.

GuestConfig::from_env_with_native_inputs returns GuestInputs{config,cost,
native_images}. The old from_env_with_cost API remains available. The new
GuestRuntime::with_native_images value setter checks the SAME initialized cost
identity and refuses replacement of an already bound declaration; a later cost
change cannot detach the declarations. It does no discovery or boot. The actual
managed_plan consumer selects prepare_native_boot only when these inputs exist.
Legacy preparation retains its old path, and native_startup then returns the
typed ImageMissing refusal; it cannot authorize the future fixed-role mechanism.

Current authority joins, traced at ROOTc576d4c2

cost.rs verifies the cost JSON digest, RuntimeCost::check, and SHA(evidence)==id.
books.rs checks the fixed CPU/memory floor/ceiling against GuestConfig and retains
the original Allocation. shared.rs::pool_configuration includes cost.id, the full
profile formula, binary/kernel/init paths, runtime roots, CPU/memory, staged path
and ordered shares. gpu_backing_pool includes that existing hash, observer identity
and exact host selectors. Box native_commitment hashes scope plus original Spec;
Pool native_commitment includes key/execution/cost/allocation/devices. Putting
declarations in the SAME evidence therefore uses existing identities. No family
hash formula, old receipt bytes, route DTO or charge ledger changes here.

Those existing joins do not verify executable bytes by themselves. The new SDK
ImageIdentity is a positive length/full digest; NativeImages binds CLI, kernel,
optional CPU/GPU VMM declarations to that evidence identity. NativeImages.cli
names the foreign Shards CLI, not Hex's trusted bootstrap image. Verification of
the SAME Hex bootstrap remains L3's separate native owner obligation.

Typed SDK and actual carried-image producer

API source7e306de84a999156b4b90c4b62efa6f1e2025e7c plus successor
4b0e3f4e3913047b5caf08e529f5f52c53102b95 retain kernel, ordered shares/RO,
explicit console kept value, full declared kernel pin, exact runtime roots and
renderer variant before flattening argv. NativeBoot borrows the initialized roots;
PreparedBoot owns fallibly copied paths/share storage. native_startup() returns
only a borrowed typed view. Replacing its initialized runtime roots refuses.
recorded_console_bytes_at reads the SAME existing console-learning algorithm at
the explicit runtime home. It retains the existing console-record/hash-cache
allocation and I/O seams; its cache identity is not the native kernel identity.
Ordinary arguments(), owning and borrowed connection bodies remain unchanged.

Full private sourcef7f8b03b2e142a5e2bf42f610a297f991ac17f6f ports only that
API delta over70b and adds helpers.rs::verify_managed_vm. This private getter
selects this binary's actual carried CPU/GPU bytes/full SHA constant, compares
them to the bound declaration, hashes the embedded bytes, then hashes the entire
actual extracted regular file. It returns that SAME open File. The supplied path
is only a locator, never image identity or role authority; cached prefix/length,
SHARDS_VM_BINARY or a request's arbitrary hash is no substitute for these checks.
The getter is not yet called by the held native worker startup consumer.

The common safe verifier rejects relative/NUL paths, nonregular files and final
symlink/reparse points; checks declared length, reads exactly that count plus an
EOF witness, checks open/path metadata before/after, and compares the full digest.
It uses one SHA-256 compression-block stack buffer and the original Instant,
checking the deadline around operations. All I/O failures remain typed. It adds
no worker, reaper, repeated probe, cache or unbounded buffer. It rewinds the same
open File before return rather than reopening/copying its path.

Limits that must survive the native join

A verified open File is not an exec token or an immutable byte seal. Subsequent
same-inode writes and path replacement cannot be certified for posix_spawn(path).
Windows metadata here is size/times/attributes, not a new kernel image identity.
The next native adapter must retain/consume the open witness and prove its actual
held final image before CONT. Intermediate path components and native UID/image
policy are also native-owner obligations. A blocking filesystem syscall is not
forcibly cancelled by the deadline checks; no new cancellation worker is claimed.
The existing cost reader still supplies the bounded, digest-verified data snapshot,
not a native executable handle. Existing var_os/serde/cost/console allocation
boundaries are not a global R01 compliance or performance result.

Future fixed-role authority still needs the initialized plan stored in the SAME
native Scope and helper receipt before any worker resume. Keep the NativeLaunch
controller publisher unchanged; L3's existing helper Publisher/FileId/lock is the
candidate durable active-member writer. No duplicate selected-VM member ledger is
introduced. Initial CLI/VMM pre-main seal, bounded native carriage, Share/Said/Grants
member identity/publication/closed state, held same-PID exec/token refresh, helper
death/final charge and complete native GPU/driver absence remain unimplemented or
unproven. Durable local Released ->finalize including last-helper absence/reap ->
canonical Released/removal ->Index refund remains mandatory.

ROOT integration and focused controls

ROOT applies only the incremental API0b4->4b0e source delta, preserving its existing
vendor/borrowed/ARM patches. Full f7f8 is a separate source cohort, not VMM adoption.
The full CLI needs the existing shards-api dependency as a normal dependency:

```
[dependencies]
shards-api = { path = "../api" }
```

It already exists under dev-dependencies. No new crate/version or shared manifest
was changed by L2. Future native startup must actually call verify_managed_vm;
no dead-code allowance or placeholder worker call was added to hide that gap.

ROOT's CLI configuration snippet, after loading the same runtime roots:

```
let GuestInputs { config, cost, native_images } =
    GuestConfig::from_env_with_native_inputs().map_err(failed)?;
// Construct the existing configured provider/GuestRuntime exactly as today.
let mut guest = guest.with_runtime_cost(cost).map_err(failed)?;
if let Some(images) = native_images {
    guest = guest.with_native_images(images).map_err(failed)?;
}
// Existing observer input and canonical registration follow; no startup probe.
```

New SDK3, Hex2 and private helper1 controls are SOURCE ONLY/UNRUN. They cover
retained typed rights/kept0, missing exact variant/legacy, actual full-file
digest/size/deadline/symlink/retained-handle ownership, unchanged evidence bytes,
real runtime/preparation joins, cost rebinding, hostile role/count/hash/header
inputs, and embedded/declaration/extracted identity disagreement. Original
observer3 passes at c576 and all previous controls remain attributed to their
original source. Local scoped rustfmt/parser and diff checks only were run.
ROOT owns the matched dependency/source checks, actual artifacts/measurements,
native forbidden-effect/closure/compute/recovery/fault proofs and final gates.

Source-only fixture successor: API68941d44c7c39d27f0e9ade5d1d0e7169af53002
and full04fbe8cc57054158ab66db57d8d1da4d3408f753 keep the new pure-plan
socket locator short regardless of private TMPDIR length. No socket is created or
dialed; artifact paths, assertions and budgets are unchanged. The matching Hex
control uses the same syntactic path-root locator. These new controls remain UNRUN.
