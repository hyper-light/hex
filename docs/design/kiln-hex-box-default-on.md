# Decision memo: turn `KILN_HEX_BOX` on by default for Linux runners

**For:** Ada (the flip is her call), via Kiln Distro. **Status:** recommended; evidence below.
**Scope:** Linux runners only. macOS/Windows are unaffected (the boxing path is a no-op stub there).

## What changes

Today every Linux CI leg and harness process shares the runner's cgroup (membership-only), so a heavy
leg's CPU, I/O and memory are not bounded against the runner. With `KILN_HEX_BOX=1` the runner vacates
itself to a `supervisor` leaf and runs each agent and leg in a Hex `BoxRoot` box with fair-share
`cpu.weight`/`io.weight`, a `pids.max` fork-bomb cap, and — the point of this memo — **the legs' parent
box carries `memory.high` at (machine memory − max(15%, 8 GiB))** while the **runner and agents carry
`memory.low`**. The flip makes that the default instead of opt-in.

## Why now — the adacachy incident

On 2026-10-22 at 11:37Z, production on adacachy **missed its liveness checks and restarted** because the
release pipeline built kiln there with a 14-core `rustc` at load ~30, and the build's memory starved the
host. That is precisely the failure this boxing prevents:

- `memory.high` on the legs' parent box means a build storm's RAM is **reclaimed/throttled inside the
  legs' box** before it can take the host's reserve — the host keeps at least max(15%, 8 GiB).
- `memory.low` on the runner and the agents means that under global pressure the kernel **protects their
  pages from reclaim**, so the runner keeps answering liveness while a leg is squeezed.

A boxed leg throttles itself; it does not restart the host.

## Evidence

- **AC-T7.1.2 — disk fairness:** `noisy_neighbor` victim fsync p99 is **1.73× idle (≤ 2×)** while a
  builder box saturates the disk (`bench/noisy_neighbor/RESULT.md`), the fix for the 2026-10-19
  disk-starvation incident. The same `io.weight` fairness ships here.
- **Boxing mechanism under real delegation:** the `the_boxed_path_holds_its_bounds_under_delegation`
  lab passes on fedora under a real `systemd-run --user --scope -p Delegate=yes` scope: vacate to
  `supervisor`, cpu/io/memory/pids controllers delegated, agent+leg fair-share bounds, the legs' parent
  `memory.high` set within a page of the computed value, `supervisor`+agents `memory.low` set, and the
  crash **sweep leaves no leg box** (T-T7.1.2).
- **Boxing scopes to runner daemons, confirmed:** the smoke found that kiln boxes only in a process
  that called `os::contain::hold_agents()` — the `kiln server` and `kiln agent` daemons do; `kiln
  workflow run` (a local tool) does not, by a deliberate guard. So turning the default on affects
  **deployed Linux runners** (where the adacachy build ran), not ad-hoc local `kiln workflow run`
  invocations — the flip can't accidentally box a developer's cgroup.
- **Dispatched-leg smoke (the integration):** _[pending a real-`kiln server` run — `bench/boxed_leg/`]_
  a leg dispatched to a scratch `kiln server` whose local_agent boxes (under a `Delegate=yes` scope,
  `KILN_HEX_BOX=1`), shown boxed with its bounds, the runner protected under an adacachy-shaped build
  storm, and no leg box left behind; plus a scaled 64 MiB-`memory.high` throttle demo. (The first slot
  run confirmed the guard above; this deploy-shaped run is the remaining end-to-end check.)
- **Overhead is negligible:** per-leg box create+start is **14.4 ms p50** (`bench/hex_start/`), and the
  node agent holds **98 B per box** (`bench/`, AC-T7.1.5). No measurable CI slowdown.

## Risk and rollback

- **Guarded and reversible:** the behavior is one flag. If anything regresses, set `KILN_HEX_BOX=0` on
  the runner and it returns to membership-only cgroups immediately — no data or format change.
- **Prerequisite already met:** the runner systemd unit sets `Delegate=yes`. Where delegation is absent,
  the code already refuses by name (a runner without a delegated subtree stays membership-only rather
  than silently running unbounded).
- **One code path:** the boxing reuses Hex's `BoxRoot` wholesale; no new cgroup code in kiln.
- **Portability:** Linux-only; macOS/Windows runners take the no-op path, unchanged.

## Recommendation

Turn `KILN_HEX_BOX` on by default for Linux runners. It converts the adacachy failure mode from a host
restart into a self-throttling leg, at ~14 ms and ~100 B per leg, behind a one-line rollback.
