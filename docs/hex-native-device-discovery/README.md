# Native device fixture discovery: source checkpoint

SOURCE ONLY. No compilation, parser test, native hardware probe, device open,
attachment, audit, mutation or heavy campaign has executed for this source.
ROOT requested this immediate signed checkpoint before optional light checks.
The parent registration and physical proof remain ROOT-owned.

Fixed Hex base1aa4ff800fbbb887e9f94dd30dae073344fe2b8e; isolated branch
cp/t7-native-device-discovery. Implementer01a1c640e6ad7127bcf34212f474d37c,
coordinator01a1aa77778471339bea3beff90d6bbd. Owned ONLY the new
crates/hex-cli/tests/oci_devices/native_devices.rs and this new directory.
Previous tested device-recovery d2/evidence7875, ea30 and ee refs are preserved.

Full POLICY_READ: fetched Shards origin/main33df95b08ad8f95bddeaa1c48acf0c64386e0eac;
AGENTS, R01-R16, evidence, CLAUDE, managed-runtime-plan, Hex CLAUDE at fixedbase
and current Hex/g121 handoffs. Rules SHA256
df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
ROOT accepted readback. Narrow Hex implementation/light-only scope persists.

## Exact sibling contract

All items are pub(super), with no dependency/Cargo or parent-file changes:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vendor { Nvidia, Amd }
struct NativeGpu {
    pub vendor: Vendor, pub id: hex_api::DeviceId, pub pci: String,
    pub model: String, pub memory_bytes: u64, pub node: PathBuf,
    pub major: u32, pub minor: u32, pub mode: u32, pub uid: u32, pub gid: u32,
}
fn discover(dir: &Path) -> Result<Vec<NativeGpu>, DiscoveryError>;
struct FixtureFiles { pub cdi: PathBuf, pub catalog: PathBuf }
fn write_fixture(state: &Path, gpus: &[NativeGpu]) -> Result<FixtureFiles, DiscoveryError>;
```

Discovery uses existing facts::Prober (and thus hex-gpu), retaining its exact model
and memory. It returns all measured, independently mapped GPUs; it chooses no pair,
load preference, assignment or grant. Relay owns native_scheduler.rs and filters
the vendor/pair; the production Service picker determines the actual chosen ID.
Typed errors distinguish probe failure, no telemetry, missing, ambiguous,
malformed, exhausted, IO, CDI/catalog and JSON failures. No successful placeholder
device is returned. Empty telemetry refuses with the existing reason.

NVIDIA proc directory PCI and Bus Location must match the measured PCI; Device
Minor names the exact /dev/nvidiaN, not NVML order. Character stat must match the
driver ABI major195 and observed regular-GPU minor0..247. AMD uses the renderD
device PCI symlink and its sysfs dev major:minor, then stats /dev/dri/renderD*;
it never derives a minor from an enumeration index. Missing/duplicate PCI mappings,
non-character nodes, rdev disagreement and shared rdev across distinct GPUs refuse.
The helper does not call production attachment-identity validation.

Caller supplies a new existing absolute private state directory. write_fixture
restats selected nodes and uses create_new for native-gpus.json and devices;
existing files are never replaced. IO failure may leave newly created partial
fixture files; the caller owns ordinary temporary-directory cleanup. Generated
CDI uses per-device hostPath, exact stat numbers/mode/uid/gid, read permission and
the shared container alias /dev/hex-selected-gpu requested by relay. No globals,
other GPU, shared control nodes, library mounts or vendor hooks are generated.
The flat seven-field catalog carries actual PCI IDs/model/memory and CDI names.
Both parsers validate generated syntax before publication. Relay can use this
writer or its explicitly assigned fixture alias renderer; there is no duplicate
discovery. Catalog reversal/restart and actual selected/other open proof are its
source responsibility and ROOT's execution responsibility.

## Primary evidence and bounds

- NVIDIA official current source pinned via git ls-remote to
  [e6a2e015 nv-procfs.c](https://github.com/NVIDIA/open-gpu-kernel-modules/blob/e6a2e01579fbbd1ca2ed43fef2ee2589e9bd8bd9/kernel-open/nvidia/nv-procfs.c#L145):
  PCI location and Device Minor are emitted from the same driver object.
  [Character-number ABI](https://github.com/NVIDIA/open-gpu-kernel-modules/blob/e6a2e01579fbbd1ca2ed43fef2ee2589e9bd8bd9/kernel-open/common/inc/nv-chardev-numbers.h#L25)
  supplies195 and the regular-device minor range, not guessed device numbers.
- Linux v6.18 resolves to7d0a66e4bb9081d75c82ec4957c50034cb0ea449.
  [DRM sysfs source](https://github.com/torvalds/linux/blob/7d0a66e4bb9081d75c82ec4957c50034cb0ea449/drivers/gpu/drm/drm_sysfs.c#L514)
  relates render names/dev_t to the hardware parent.
  [Render-node contract](https://github.com/torvalds/linux/blob/7d0a66e4bb9081d75c82ec4957c50034cb0ea449/Documentation/gpu/drm-uapi.rst#L108)
  documents the per-device render endpoint and explains why ordinal identity
  and unplug continuity cannot be assumed.
- CDI v0.8.0 tag e66544063aa7760c4ea6330ce9e6c757f8e61df2
  [specification](https://github.com/cncf-tags/container-device-interface/blob/e66544063aa7760c4ea6330ce9e6c757f8e61df2/SPEC.md):
  hostPath needs0.5.0, the generated minimum; device edits apply only on selection.
- Existing Hex Prober, hex-gpu PCI formatting, API DeviceId and Catalog are read
  at fixed1aa4. The small fixture PCI reader is independent correlation over their
  public string shape; no direct transitive hex-gpu dependency is added.

Directory/output counts use the existing u16/API MAX_DEVICES space. Relevant
NVIDIA lines use the exact field tags plus maximum u32/PCI printf widths;
unrelated model/UUID/debug lines stream past without retention. Sysfs dev text
is bounded by two u32 decimal widths, colon and newline. Duplicate validation
sorts bounded temporary IDs/mappings; no persistent cache, holder map or picker.
No performance/allocation/hardware-security claim follows from this source.

## Status and R01-R16

Scoped rustfmt and source diff checks only; parser counts UNRUN. Two synthetic
parser/encoding controls are present, with explicit synthetic metadata; they do
not discover hardware or substitute null/zero devices. Linux discovery/writing
functions are not a macOS implementation. All-platform support remains required
ROOT work; this narrowly assigned Linux fixture makes no overall completion or
PlatformUnsupported acceptance claim.

R01/R04/R06/R07/R13: typed failures, explicit measured-to-verified result, bounded
format readers, ordinary file-drop ownership; tests may assert. R02/R03/R05/R12:
primary pins and existing Prober/ABI/CDI seams above, no new runtime manager/cache,
wait, syscall binding, unsafe, installed helper or production change. R08/R14:
no profiles/benchmarks/competitive or allocation claims. R09/R10: new fixture,
not a reproduced native defect; all native before/after/mutation proof is owed.
R11: hardware/all-target/attachment correctness unverified. R15/R16: no children,
goals, automatic dispatch, laptop build/test or native campaign; optional light
parser checks only on admitted d1321CPU/4GiB/12GiB nice10/jobs1/threads1 with NEW
private paths/no-sync. No active owned job or cleanup actor. ROOT owns parent
registration, physical effects/cleanup, native/catalog integration and full gates.
