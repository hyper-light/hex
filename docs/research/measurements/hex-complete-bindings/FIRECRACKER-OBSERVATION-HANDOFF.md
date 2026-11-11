# Current native guest facts and explicit generation handoff

This successor to e7dcda75/e6f6caf9 is SOURCE ONLY. No compile, test, VM, GPU,
KVM or remote job was run by L1. The exact source manifest and original controls
remain separate from ROOT's joined checks/native QA.

Firecracker capacity now uses current observations from the actual charged observer
or retained Ready GPU Box, joined through the official VMM host-SBDF/guest-SBDF
witness and the same guest Node's provider. It no longer falls back to an open-time
host NVML/ADL inventory for a VFIO-bound card. Missing observations grant no fresh
metadata/eligibility. Canonical Store holders and the complete physical claim set
remain retained across transient driver/counter loss. Independent counters remain
optional and are never invented as zero.

The same native guest must additionally expose a validated native Linux OCI
attachment for each assigned guest GPU. A factual driver report alone cannot
prove the container payload can use that device. The check accepts only the
actual guest native OCI route and its physical eligibility mask, permits the
exact retained inner Box holder when observing a live workload, refuses foreign
holders, and refuses extra driver-visible GPUs outside the VMM's complete selected
set. It does not assign a synthetic guest PCI identity, use the Shards GuestGpu
recipe, install CDI/driver tools or trust admin model/memory as current measurement.
The pinned guest image must contain its real driver/userspace and a validated
native CDI catalog; without them the observer cannot become Live. ROOT owns
the actual artifact closure and native GPU acceptance.

Observations from a retained GPU Box use its exact persisted scope, same method/
conflicting-device turns and managed native receipt. Only Ready is observable.
All protocol calls during a caller's create retain that caller's absolute deadline;
ordinary report/control calls retain their configured finite per-operation budget.
No inventory cache, provider reopening, parallel table, background polling or new
timeout is introduced. Catalog lookup is a fallibly collected/sorted temporary
index, with no repeated all-distinct insertion algorithm. Duplicate physical facts
from two scopes refuse instead of creating a second slot.

Rearming preserves the old positive terminal receipt until the SAME Index has
consumed it. Service first normalizes the actual Released Capacity snapshot into
both existing views. Driver then calls the existing Runtime::release_backing under
its operation receipt and outside the core/model mutex, requiring the exact old
key/profile/full selected set and Released phase. The native owner can then remove
only that Released PoolRecord, only when no retained Box still references its key
and its incarnation-owned directory is absent. Preparing the next generation uses
the same Store/turn owner; a fresh scope is never a reusable socket/public BoxId.
It does not boot a VM. A following ordinary snapshot may admit that Prepared
generation. One bounded pass handles Prepared grants and Released acknowledgements;
newly discovered generations are not initialized recursively.

Box deletion does not automatically remove the old terminal receipt. This avoids
hiding a Released fact from an Index that still holds the old Live grant. Known
Box holders suppress unnecessary acknowledgement calls; an unread fact never
means no retained reference, and the native Store performs the final reference
check. At whole-owner startup, preflight completes before configured preparation.
These are required new lifecycle behaviors, not claimed native reproductions.

Two added source controls remain UNRUN:

- `observer_needs_exact_driver_set_and_real_native_container_binding` exercises
  actual report/attachment-mask/holder readiness with bounded synthetic data.
- `native_rearm_waits_for_the_original_released_snapshot` exercises real Driver/
  peer state with an unlocked pure native callback and private regular-file rows.

The original VMM output-sink control/body from70e71c73 is preserved unchanged.
Its read-only-sink hypothesis is still unexecuted and its remedy remains held.
Original route/pending red/green controls and expectations are unchanged. ROOT
owns all actual scope/descriptor/driver cleanup, faults/mutations, resource
measurements and supported-target execution. The configured observer's complete
claim-set requirement remains explicit; arbitrary proper-subset provisioning and
its native acceptance are not claimed complete by this checkpoint. Whole-owner
cold native metadata preflight remains coordinated with L3's matching native seam.

R01: new lists and snapshots are bounded/fallible, and metadata/identity fields are
checked before mutation. R02/R03: the pinned official preboot assignment witness,
existing native OCI attachment validation and fixed API route/physical bounds are
used; runtime costs/budgets stay explicit, unmeasured values are not guessed.
R04/R06/R07/R13: Released acknowledgement, retained Box reference, new generation
and uncertainty are distinct transitions with affected identity. R05/R12: same
Store, turns, native receipt and report provider; no cache/manager/picker. R08/R14:
no performance, allocation or comparative result. R09/R10: new-feature source and
unrun controls retained; no invented reproduction/pass. R11/R15/R16: implementer
source-only Linux adapter, ROOT joined/native/full gates owed, no new campaign,
foreign data/PID, secrets, driver, runner or service changes.
