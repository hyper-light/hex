# macOS: a rootless per-process memory cap, armed at spawn (step 1's `_ext` backstop)

Source notes for native plan step 1's extension: a macOS `process` box caps each member it spawns at
the box's whole bound, so a single-member box is bounded by the kernel with no privilege and no watcher
overshoot. This complements, and is distinct from, the running-task memory interfaces, all of which are
privileged or absent rootless (`docs/research/runtime/native-platforms.md` §3,
`bench/native/macos/mem_bound_probe.c`).

## The interface

`posix_spawnattr_setjetsam_ext(posix_spawnattr_t *attr, short flags, int priority, int memlimit_active,
int memlimit_inactive)` — a private but linkable SPI (`_posix_spawnattr_setjetsam_ext` is exported from
`libsystem_kernel.dylib`; prototype from xnu `libsyscall/wrappers/spawn/spawn_private.h`, "All memory
limits are in MiB."). It records a per-process physical-footprint limit in the spawn attributes, so the
limit is live from the spawned program's first instruction. The parent needs no privilege to set an
attribute for the child it spawns — unlike `task_set_phys_footprint_limit` or `memorystatus_control` on a
*running* task, which need root (`kern_memorystatus.c`), the EPERM/KERN_NO_ACCESS results
`mem_bound_probe.c` recorded. Arming at spawn is the rootless path those probes did not cover.

Flags (`xnu bsd/sys/spawn_internal.h`, `psa_jetsam_flags`):

| flag | value | meaning |
|---|---|---|
| `POSIX_SPAWN_JETSAM_SET` | `0x8000` | the attributes carry jetsam settings |
| `POSIX_SPAWN_JETSAM_MEMLIMIT_ACTIVE_FATAL` | `0x04` | the limit is fatal while the process is active |
| `POSIX_SPAWN_JETSAM_MEMLIMIT_INACTIVE_FATAL` | `0x08` | the limit is fatal while it is inactive |

Step 1 sets `SET | ACTIVE_FATAL | INACTIVE_FATAL` (`0x800c`) with `memlimit_active == memlimit_inactive ==
the box bound in MiB`, so the cap is fatal regardless of the member's active/inactive state. `priority` is
a jetsam band (40, a middling band); a *fatal* per-process limit kills the process when it exceeds the
limit regardless of band, so the band only orders kills under genuine system pressure.

## What it does, measured (`bench/native/macos/jetsam_probe.c`, record `jetsam-probe-20261101T2052Z.txt`)

On adas-mac-studio-3 (macOS 26.2, build 25C56), rootless (uid 501): a child capped at N MiB and
allocating past it with an incompressible fill is `SIGKILL`ed by the kernel at exactly N MiB — 0 MiB
overshoot at both 64 and 256 MiB. (A compressible fill, e.g. `memset` of a constant, is shrunk by the VM
compressor so `phys_footprint` never reaches the cap and the kill never fires; the fill must be
incompressible.)

The cap is **per process and does not survive a fork** (`fork` resets the kernel's memory limit,
`kern_fork.c`; re-arming a running task needs root) — step 2's finding, recorded in
`docs/research/runtime/macos-box-user-and-ledger.md`. So the cap bounds a box only while every member is
one Hex spawned (each armed); a member that forks yields an unarmed child the cap does not bound, and the
box's bound then rests on the polling watcher again.

## Telling the kill apart (rootless, before reap)

A member killed at its fatal cap dies by `SIGKILL`, indistinguishable by signal from an ordinary kill. To
account it as a memory end, the node must tell the two apart:

- **kqueue `NOTE_EXIT_MEMORY`** (`sys/event.h`, `0x00020000`): asked for with `NOTE_EXIT_DETAIL`, it is
  *not* set for a per-process fatal-memlimit kill (fflags `0x86000000`). It signals system memory
  pressure, not a per-process limit. Not usable here.
- **`proc_pidinfo(PROC_PIDEXITREASONINFO)`**: returns `ENOMEM` rootless. Not usable.
- **`proc_pid_rusage(RUSAGE_INFO_V4)` on the zombie**: `ri_lifetime_max_phys_footprint` is preserved
  after death and reads the cap exactly, rootless, before reap. **Chosen.** An armed member seen as a
  zombie whose peak footprint reached its cap was killed at the cap — sound, because reaching the fatal
  cap *is* death, so a member that exited cleanly can never show a peak at or above its cap. The node's
  watcher watches each member for `NOTE_EXIT` (a Mach real-time thread, so it reads the zombie's peak
  before the member's own waiter reaps it) and watches `NOTE_FORK` so an unarmed child reverts the box to
  the polling pace at once.

## Bearing on the design

This makes step 1's single-member overshoot and held-box watcher CPU ~0 rootlessly, without the step-2
privileged helper, for a box of at most one member. It does not bound a multi-member box's footprint sum
(each member is capped at the whole bound, not its share) — step 2's root re-apportioning owns that. The
runtime design narrative's "no rootless kernel cap" wording (`docs/design/runtime.md`,
`bench/native/macos/RESULT.md`) predates the spawn-time path and is step 2's to reconcile.
