# Exact Shards GPU implementation record

Lane L2 implementer: Kiln `01a1c7676c7c76cb998cb12303110488`.
Coordinator: `01a1aa77778471339bea3beff90d6bbd`.
Hex starting source: `b06b1760bb690c9e29a31f99efdb998cf8279c36`.
Full charter and lane brief were read from `scheduler-completion-20261110`.
Full Hex CLAUDE and current upstream Shards AGENTS, development-rules,
development-evidence and CLAUDE were read before edits. Policy revision:
`0342b85d9c808e3ced041a6c3ef0e9bf70438272`; rules SHA256:
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
ROOT acknowledged the readback and Hex ownership, then explicitly approved the
dedicated GPU guest design. Upstream changes remain held for the existing Shards
GPU owner's exact partition. No additional agent, goal or plan was created.

Owned Hex paths: `crates/hex-runtime/src/guest/**` and this directory. ROOT owns
integration, CLI/node configuration, dependency/vendor metadata and main merges.
L1 owns shared classes/catalog/native attachment, L3 owns the durable box holder
and node recovery/forwarding. They received concrete boundary requests.

## Decision and sources

The current shared guest cannot enforce distinct physical sets between boxes:
every guest process with its render node reaches that VM's renderer contexts.
This is a source observation and a new required feature, not a reproduced native
defect. ROOT approved a dedicated existing guest per GPU-bearing BoxId. CPU boxes
keep the existing shared guest, with no GPU. A global whitelist for the shared
guest would grant unrelated boxes the union of physical claims; guest driver
changes carrying authenticated per-container context grants are a substantially
broader alternative. Neither a second picker nor a second device-holder ledger
is introduced. The existing BoxRecord/Index retains device ownership; the new
receipt records which VM must end before releasing that owner.

Primary references read 2026-11-10:

- [Vulkan ID properties](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceIDProperties.html)
  distinguish device UUID from pipeline-cache UUID and describe persistence limits.
  Native identity is rejoined to a live UUID; a UUID is not substituted for durable
  native physical identity.
- [PCI extension](https://docs.vulkan.org/refpages/latest/refpages/source/VK_EXT_pci_bus_info.html)
  and [PCI properties](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDevicePCIBusInfoPropertiesEXT.html)
  require checking per-device extension support, then using the exact PCI tuple.
  Missing support does not authorize model/vendor/index fallback.
- [MoltenVK v1.4.2 MVKDevice.mm](https://github.com/KhronosGroup/MoltenVK/blob/v1.4.2/MoltenVK/MoltenVK/GPUObjects/MVKDevice.mm),
  `populateDeviceIDProperties`, exposes the native Metal registry ID in deviceLUID
  as big-endian bytes. That permits an exact native join with this renderer pin.
- [Vulkan-Headers v1.3.252](https://github.com/KhronosGroup/Vulkan-Headers/blob/v1.3.252/include/vulkan/vulkan_core.h)
  specifies the ABI sizes and tags; [Microsoft build tools](https://learn.microsoft.com/en-us/cpp/build/building-on-the-command-line?view=msvc-170)
  documents explicit ARM64 target toolchain selection.

The full renderer filter/transport artifacts at Hex
`6219e61c9e3f559579b2632f873005a47f521cc8` are retained unchanged. Their 32/19/20
fake-driver/protocol passes are historical focused evidence, not a new renderer,
VMM, VM, GPU or Windows ARM result. Upstream application/build remains open.

## Source checkpoint and invariants

`selected.rs` encodes immutable BoxId, sorted exact host GPU IDs, configured
vCPU/memory and `Intent -> Running(identity) -> Stopping(identity)`. Intent is
synced before launch; process identity is recorded before relay connection or
workload staging. A lost identity remains typed uncertainty. An inconsistent
selection or configured envelope refuses recovery. Receipt removal requires
positive terminal/absence observation. A malformed file is not absence.

`dedicated.rs` routes each BoxId to its own Booted connection and scoped program
connection. Stop bypasses the relay; an unresponsive guest cannot prevent a stop
attempt. A stop result alone does not remove the receipt: wait_for must observe
terminal. A partial creation cannot cause a second launch/retry. The shared CPU
path still uses the existing guest. Explicit Shards workloads translate to the
same OCI payload inside that backend; Firecracker is refused at this adapter.

Configured VM CPU milli and memory bytes are exact unit conversions. Admission
must charge that complete VM envelope before boot, with payload bounds inside
it. This helper is not proof that VMM host overhead has been measured/charged;
L1/ROOT integration and measurements remain owed. No arbitrary memory allowance
or new timeout is introduced.

Receipt reads are bounded by the existing native configuration-byte ceiling;
device counts by the existing u16 device space; guest routes/listings by
MAX_BOXES. Text is counted before fallible allocation. State is per owned VM;
event clients invalidate on closed connection and die with that VM. No inventory
cache, state polling or new reconnect timer is added. Existing learned boot
bounds provide protocol wait deadlines; no latency/performance claim follows.

## Required integration and unfinished work

This is source progress, not readiness. The current branch intentionally awaits
the exact shared source cohort rather than inserting production stubs:

- ROOT `9d9dccfbd82c8e7448d67166525114d2a41a8db5` supplies append-only public
  Shards/Firecracker variants and wire integration. Original owned source remains
  based on b06; any private check overlay must record these exact blobs.
- Proposed upstream API `PhysicalGpu` (PCI, Metal registry ID, LUID),
  `GuestSpec.gpu_devices`: API signed source327424a1 followed by corrected
  36d9254f is now available. Both ROOTs released the exact upstream partition.
  Real renderer native-identity join, context setter/ACK, selected memory and
  Windows ARM renderer remain in progress on private `hex/physical-gpu-0342`.
- L1 catalog `Binding::ShardsGpu { physical }` supplies trusted native bindings;
  guest-only `Binding::GuestGpu` maps trusted boot host IDs to actual virtio GPU
  DRM nodes. Peer payloads cannot create that mapping. The actual guest startup
  integration and native CDI cgroup/device-node effects remain required.
- L3 node recovery must call `guest.validate_binding(spec)` before adopt/alive;
  forwarding must use `evented_for(id)` / `program_client_for(id)`.
- ROOT must charge the configured per-box VM envelope plus measured native
  parent/VMM costs, validate retained claims and terminal cleanup, then perform
  native/VM/E2E/fault/mutation/profiling/benchmark and seven-target gates.

## R01–R16 accounting

R01/R03/R04/R05/R06/R07/R12/R13: explicit typed states, fallible bounded new
buffers, checked unit/count conversions, existing public guest/backend seams,
scoped ownership, no fallback, no new tuning values. Local scoped rustfmt and
diff checks are permitted; compiler/light checks are still pending at this pin.
R02/R09/R10: primary sources and alternatives above; no native reproduction or
effect is claimed. Focused receipt and routing behavior tests are being added.
R08/R14: comparative performance, allocation/copy/fault/parent costs remain
coordinator work. R11: seven-target and real KVM/HVF/WHP coverage, Windows ARM
included, remains unverified; paused Windows ARM VM untouched, Intel Mac excluded.
R15/R16: signed named-path checkpoints; light checks only admitted d132
CPU1/memory4GiB/disk12GiB, nice10/jobs1/threads1, private source/target/cache/temp,
precise no-sync. No local builds/tests or native actors, no runner changes or
foreign data/PID changes. No active check/job; originals/evidence preserved.

## Successor source and focused API result

Hex a16d5bf3 supplies the concrete `WithGuest::route`, `evented_for(id)` and
`program_client_for(id)` interfaces consumed by L3. The successor removes the
new unchecked Arc allocation for route slots: one fallibly reserved `Vec<Slot>`
contains the existing route state, and a stack reservation moves its prior
Guest out while native/file/channel work runs. The table lock protects only
claim/publish/remove. The same BoxId is excluded; sibling/CPU calls remain
independent. Existing Arc<Booted>, Arc<dyn Guest> and Link constructors are
pre-existing public/lifetime seams, not claimed fallible by this change.

The durable receipt now retains the original backend class and exact typed
native physical selectors as well as host IDs. A changed native catalog binding
refuses adoption instead of reinterpreting a retained ID. Explicit CPU Shards
boxes also use a dedicated VM; generic CPU OCI compatibility remains shared.
Only the exact receipt owner's inner OCI BoxInfo is normalized to its original
host class. Foreign IDs/classes refuse, including in a dedicated listing.

ROOT's common Node owner/weak Books transaction seam is the integration target:
one Store, method/device Turns, records and admission authority. No
Recorded<Node> wrapper, strong owner cycle, parallel picker or holder ledger.
VM envelope charging still needs this actual shared admission path. Per-VM
receipts/connection routes are not that accounting authority.

API36d9254f's focused d132 run selected exactly3 tests:3 passed,0 failed,0 ignored;
strict API library Clippy0 and mechanical policy0. The private scaffold contains
all actual API/ABI/afunix/testdir sources and required test includes with the
original workspace's package/dependency/lint definitions. Workspace membership
and irrelevant TLS path patches alone are narrowed in that scaffold. Cargo's
first --locked refusal was retained; `cargo update --workspace --offline`
pruned unused lock entries with no new/changed package tuple or registry pin.
The passing test binary SHA256 is
`61c980eabe9b3721dc26c3cf8d5f484538c3bb5f7d73531a463f13b26dc8b67c`.
All child PIDs were waited/reaped; private TMPDIR entries0. API parsing and
argument checks are not native selection or VMM/VM proof. Raw proof collection
is pending under `~/.cache/hex-complete-shards/light-api-36d9254f-v3` locally
and the same-named runner scope. Original full-archive transfer timeout,
Python extractor-version refusal and original lock refusal remain preserved.

Source trace of the existing API: stop sends a kill; wait_for consumes shared
end state. Exit::watch waits for/reaps the child but also records error paths;
End::Stopped can mask a wait-status observation error when stopping is set.
Thus a normal or failed End is not independent native GPU/helper reclamation
proof. Linux uses a render-server thread with thread workers in the same VM
process; macOS/Windows render in process, with Windows launcher/job semantics
also relevant. Exact kernel/process/job/driver resource witnesses and the error
case remain ROOT-owned and unverified. No complete cleanup/absence claim follows
from model End values or source inspection.


## Same-owner Books and initialized cost successor (source only)

Consumes exact shared Catalog/Books API14e2c1979dea3ea1a925e8c86561b360d4987175,
RuntimeCost/Allocation APIcc4dae242599991de6c86419b7c166ffe425137e and ROOT
Lifecycle cause export6d7f24fe. No test/build result is claimed for this successor.
ROOT actual d83 guest compile retained E0624 for private Booted::host_info;
this source carries pub(super) and ROOT is authorized to apply that one line to
its old integrated source independently.

GuestRuntime::bind_books(Weak<dyn devices::Books>)->Result<(),Refusal> initializes
one inline OnceLock. Rebinding is refused; same pointer registration is idempotent;
owner death remains a refusal. No Arc::new/Box::new slot/transaction allocation.
Linux registration coerces existing Arc<Node> to Arc<dyn Books> then downgrades.
macOS/Windows use existing Arc<Recorded<WithGuest<P>>> the same way; ROOT wires
post-Arc registration, L3 implements Books on the sole Store/method/device Turns.
No second Recorded, Store, holder ledger or strong Arc cycle is added.

GuestRuntime::owns_record(spec) is uses_books && (explicit Shards || selected
devices). L3 Recover::owns_records is route(spec.class)==Guest && that predicate.
Ordinary CPU OCI keeps its existing outer Recorded lifecycle. Dedicated create
enters shared stack transaction once, validates all sole-Store holders, puts original
BoxSpec Creating before launch effects, then Ready only after host class/id checks.
Failed create retains Creating. Dedicated methods reread original class/spec under
that same method/device ownership. Delete puts Deleting and removes only after
native deletion returns proof; uncertainty preserves original grants. VM receipts
remain process/boot receipts only. Public recorded_box is a routing snapshot, never
a turn or grant. WithGuest consults persisted class before native inspect and forces
exact Shards to Guest; ordinary configured Windows OCI remains Host. L3 direct async
forwarding must hold canonical Turns until reply and normalize Inspect to the retained
original class. ROOT CLI registration/recovery integration is still owed.

with_runtime_cost requires the single L1 RuntimeCost profile and exact evidence id;
fixed CPU/memory floor and ceiling equal configured vcpus*1000 and MiB*(1<<20).
The shared cost.allocate checks guest allowance, explicit quanta and host allowance.
Dedicated capacity starts with the SAME owner's native physical pool, measured device
facts and catalog.holder over sole records; it no longer appends report(None).
Capacity.cost is that initialized profile. Missing measured cost cannot advertise
Shards via classes or create a dedicated VM. Before launch allocation.cost must equal
that profile, allocation.guest CPU/memory must equal configured actual VM and unchanged
workload caps must remain covered. No separate formula/charge or guessed overhead.
The host allocation is retained by original host record; rewritten inner native OCI
clears that host allocation so the VMM/relay overhead is not charged twice.
Scheduler candidate/admission propagation and native measurements remain L1/ROOT inputs.

Kernel boot authority is fallibly reserved within Linux COMMAND_LINE_SIZE2048 before
formatting. Oversized or noncanonical selections fail before Intent/VM effects.
Selected route lookup does not fall through to an unrelated shared CPU guest after
terminal or missing dedicated receipt. Recovery requires exact retained receipt;
stop reconstruction uses immutable selector/vCPU/memory even if configured selector
changed, though missing current conflict catalog may still prevent canonical turn
acquisition and must not be treated as absence. Inner metadata normalizes only exact
owned BoxId; foreign boxes refuse.

Primary resource precedent: Kubernetes Pod Overhead accounts runtime overhead during
scheduling and cgroup sizing, https://kubernetes.io/docs/concepts/scheduling-eviction/pod-overhead/.
We consume its scheduling-before-effects principle, not its cluster constants. Its
runtime overhead is analogous to this one profile; the exact formula/types are L1's.

New focused controls (UNRUN): same-owner/rebind/dead weak registration; missing/forged/
stale/smaller cost receipt; exact canonical transaction entry and nonready refusal;
backend-explicit route with Windows ordinary Host; forged boot owner/set/class and
kernel byte overflow. Earlier dedicated reentry/sibling/CPU lock and stop models
remain source only unless ROOT reports exact results. Native renderer selection ACK,
physical/helper termination and Windows PCI/PNP/KMT-LUID live correlation remain
UNVERIFIED. In particular Some End can reflect failed upstream observation; neither
that value nor VMM PID exit alone proves selected renderer workers/helpers ended.
All R01–R16 gaps in this document remain binding; no source checkpoint is completion.
