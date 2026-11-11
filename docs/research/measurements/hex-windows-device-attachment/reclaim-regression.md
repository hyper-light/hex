# Assigned marker reclaim regression

AC-T7.4.1 / T-T7.1.2. This narrow continuation starts from signed
67f98335456a8501e6a5f82ec9f98770531d70ba, accepted by ROOT as d4fd91.
ROOT's source observation: startup reclaim considers an EndsWithNode marker
reclaimable even when its assigned compute system is still present. Unknown
device-create outcomes now deliberately retain that marker. This is a source
hypothesis, not a reproduced HCS or hardware defect.

The added fixture uses the actual marker reader and reclaim decision with counted
native callbacks. Its private filesystem holds one assigned marker, its owned
scratch folder and a sibling sentinel. A live-system observation must leave all
bytes/paths intact and call cleanup zero times. The absent-system positive control
then removes only that marker and owned folder, leaving the sibling unchanged.
The temporary directory owns bounded cleanup even on assertion failure. No HCS,
WMI, container, VM or GPU operation occurs in this fixture.

Source-first ce99413fa838b3d387af210f4ba20bb468b9c6bd actually failed the
new selected regression on Jupiter: 0 passed, 1 failed, 0 ignored, 63 filtered.
The live-system cleanup callback was called once instead of zero. Exact stdout
d314a150743858e9f278e63ec18013b43c3e64fbf3a0f57e2452172e99e47f8d
and stderr
60b761e90e0ed8c017d672d76bb4133b68a9b67b0be24121559f6650b6553ab9
are retained under reclaim-red-v3. Cargo284644 exited101/reaped; temp entries0.
This establishes the callback decision failure, not an actual HCS cleanup effect.

The successor leaves assigned markers untouched whenever their system exists.
The absent-system control and sibling checks remain unchanged. It also snapshots
Ready native handles and fallibly copied IDs under the sole map mutex, releases
that mutex before keeper handoff/durable writes, and saves each exact receipt
before writing it. Existing keeper serialization prevents a concurrent removal
from closing receipts mid-handoff. The new callback test checks map access and
retained receipt on a write refusal, then deletes its one ordinary empty Job.
The snapshot is temporary, at most MAX_BOXES entries; it is not another manager.
These successor tests are pending at this source checkpoint. The earlier exact67 native run
passed six selected Windows tests (57 filtered, none ignored) and Runtime
all-target Clippy. Original first-wrapper failure, first Clippy failure and all
raw checks remain preserved. ROOT owns actual HCS cancellation/absence, physical
device effects, mutations, seven-target/full gates and integration review.

Full current policy readback: Shards origin/main
33df95b08ad8f95bddeaa1c48acf0c64386e0eac, rules SHA256
df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
Implementer01a1c643 owns only the assigned Windows/OCI, native HCS marker sections,
associated tests and this evidence. No shared catalog, model or syscall edits.
Latest user instruction requires valid device support on Windows/Linux amd64 and
arm64 and macOS arm64. A platform refusal is not feature completion. Assigned
borrowed-volume attachment at the same marker remains in this scoped queue.
