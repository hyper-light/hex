# What a native macOS box can bound with public interfaces: measured

`box_probe.c` on adas-mac-studio-3 (Apple M4 Max, 14 cores, 36 GiB, macOS 26.2 25C56, Apple clang 21.0.0, `-O2`),
2026-10-31. These are the native-platforms research's §7 measurements (`docs/research/runtime/native-platforms.md`).
Records (each three passes of every row):

- `records/box-probe-374bdbf-20261031T233823Z.txt`: commit 374bdbf, kiln run r30, 23:38:23-23:38:49Z, load 1.86-2.46.
- `records/box-probe-9694770-rt-20261031T233931Z.txt`: commit 9694770, the watcher real-time, 23:39:31-23:39:45Z,
  load 3.74-4.01.

The bound is 256 MiB. Each row is 20 runs, and the figure given is the worst of three passes.

## A box is listed and read cheaply

| Box | read p50 | read p99 | per process |
|---|---|---|---|
| 1 process | 5.4-5.5 µs | 5.7 µs | 5.4 µs |
| 16 | 10.3-10.4 µs | 10.7-12.3 µs | 0.65 µs |
| 128 | 49.3-49.7 µs | 53.8-54.3 µs | 0.39 µs |

A read is the box's process group listed (`proc_listpids`) and each member's footprint read (`proc_pid_rusage`). A
same-user child reads at every flavor (`check`).

## A member that daemonizes leaves its group and parent, not its user

`escape`, every pass: `ppid=1 in_box_pgrp=0 child_of_leader=0 same_uid_listed=1`.

## A plain sleep is no timer to hold a bound with

One member allocating at full speed (23-25 GiB/s here: the probe prints MiB/s), polled every T:

| T | `usleep` overshoot p50 / max | kqueue `NOTE_CRITICAL` p50 / max |
|---|---|---|
| 1 ms | 164-186 / 187-204 MiB | 9.7-22.5 / 17.8-25.5 MiB |
| 10 ms | 689-1,137 / 1,540-2,007 MiB | 15.2-244 / 226-251 MiB |

`usleep` ran about six times long, at every QoS the probe tried. A one-shot kqueue timer marked `NOTE_CRITICAL`
(`sys/event.h`: "system does minimal timer coalescing") ran as asked.

## The bound held: poll at the headroom's pace, from a real-time watcher

Polled every (bound - footprint) / 256 GiB/s, at least 250 µs, on `NOTE_CRITICAL` timers, with the members at utility
QoS. The rate is above the 164 GiB/s a box's members together reached here.

| Members | box allocates | watcher at user-interactive QoS: overshoot max | watcher real-time: overshoot max |
|---|---|---|---|
| 1 | 24-26 GiB/s | 6.5-7.5 MiB | 6.6-7.4 MiB |
| 4 | 83-97 GiB/s | 21.4-52.4 MiB | 10.9-28.3 MiB |
| 8 | 146-153 GiB/s | 36.2-50.3 MiB | 30.7-41.5 MiB |
| 14 (every core) | 150-161 GiB/s | 334-482 MiB | 26.0-36.9 MiB |
| 28 (twice the cores) | 162-164 GiB/s (real-time record) | 846-1,458 MiB | 34.1-41.5 MiB, **did not reproduce**: see below |

The real-time watcher is a Mach time-constraint thread (`THREAD_TIME_CONSTRAINT_POLICY`: 250 µs period, 50 µs
computation). Once a box's members fill every core, a watcher at user-interactive QoS reads late; a real-time one
doesn't.

## What it costs

A box held at 95% of its bound (`hold`, real-time watcher):

| Floor | reads/s | the watcher's CPU |
|---|---|---|
| 250 µs | 3,618-3,639 | 4.7-5.5 ms/s of reads only (≤ 0.55% of a core); the whole watcher, wakes included, is 12-15%: see below |
| 1 ms | 973-974 | 2.0-2.4 ms/s (≤ 0.24%) |

A box far from its bound is read every 100 ms (the ceiling), and costs next to nothing.

## What this says

- Corrected by the class's own path (below): a rootless box holds a 256 MiB bound to within 54 MiB with up to one
  member per core, not at twice the cores (56-587 MiB over; this probe's 34-42 MiB at 28 members did not reproduce),
  and the watcher costs 12-15% of a core near the bound, not the 0.55% its reads alone took. The overshoot is the
  box's rate times the floor, plus a read.
- The rate (256 GiB/s) is a tuning constant measured here. Another machine needs its own measurement (CLAUDE.md §5).
- This isn't a kernel cap. A box can pass its bound by the overshoot before it ends. On Linux and Windows the kernel
  refuses the allocation itself.

## The class's own path, measured (native plan step 1, 2026-11-01)

The probe above measures the raw interfaces. The `process` class itself — `hex_runtime::macos::MacRuntime`, a box
spawned with posix_spawn into its process group, its watcher a real thread on the rule's own code
(`hex_core::watch`), its members real `hex selftest-box hog` processes — is measured by `bench/native/macos/macos_box`
(`crates/hex-bench/benches/macos_box.rs`), and corrects two things the probe's figures above over-claimed.

Record `records/macos-box-ec3d08b-20261101T09Z.txt`, commit ec3d08b, adas-mac-studio-3 (M4 Max, 14 cores,
36 GiB, macOS 26.2 25C56), 2026-11-01, load 2.14, five passes, 256 MiB bound. The watcher has every rootless
improvement over the first record: a 150 µs Mach time-constraint computation budget sized above the read cost
(`rtbudget.c` — a smaller budget gets the thread demoted under the members' memory contention), a lean per-read
(no per-member `kqueue` watching, the escape diff only when membership changes), and floor pacing while a box is
still taking members on.

**Overshoot past the bound, by member count (MiB), 5 passes:**

| members | p1 | p2 | p3 | p4 | p5 |
|---|---|---|---|---|---|
| 1 | 0.2 | 3.5 | 2.2 | 2.1 | 4.1 |
| 4 | 20.1 | 20.8 | 5.5 | 6.8 | 6.2 |
| 8 | 47.9 | 20.5 | 7.3 | 16.0 | 14.7 |
| 14 (every core) | 29.4 | 2.3 | 31.1 | 14.3 | 21.0 |
| 28 (twice the cores) | 697.2 | 42.8 | 1034.5 | 0.3 | 230.7 |

- **Up to one member per core (1, 4, 8, 14), a box holds its 256 MiB bound to ≤48 MiB** — within the 64 MiB bar.
- **At twice the cores (28), the polling watcher can't hold it reliably**: 0.3-1034 MiB across passes. The cause is
  the real-time watcher being descheduled for milliseconds under 2×-core memory-bandwidth saturation — not the poll
  interval, the budget, or the read weight (all addressed): the standalone `rtbudget.c` probe holds 28 *simultaneous*
  allocators to ≤55 MiB, but the class under realistic load intermittently spikes. The original probe's "within
  42 MiB at 28" (above) does **not** reproduce. A box filling more than every core can't be bounded tightly by
  rootless footprint-polling; that is step 2's event-driven kernel ledger (`hex_core::ledger` + the privileged
  helper), with this polling the defense-in-depth layer.

**The watcher's CPU, a box held at 95% of its bound (2 s, main thread parked so only the watcher runs):**
14.11, 15.06, 15.46, 15.21, 14.84 % of a core — **not** the probe's "≤ 0.55%" (which counted only the
`proc_pid_rusage` call, not the wake). A box near its bound reads at the 250 µs floor (~4,000 reads/s) and each
wake's one-shot `NOTE_CRITICAL` timer re-arm plus the read costs ~12-17 µs. Adaptive backoff (reading a still box
rarely) was tried to reach ≤1% but reverted: `proc_pid_rusage`'s footprint and CPU both lag a fast allocator, so a
backoff keyed on them fires on a *growing* box and overshoots by gigabytes. So criterion 3's ≤1% is step 2's
event-driven ledger (no polling, ~0 CPU); a box far from its bound is still read every 100 ms and costs next to
nothing.

**Why the rule, and its limit.** macOS offers no rootless kernel cap on a process tree's memory: the private jetsam
limit (`memorystatus_control(MEMORYSTATUS_CMD_SET_MEMLIMIT_PROPERTIES)`, `SYS_memorystatus_control` 440) returns
`EPERM` for an unprivileged process, on self and on a child (measured 2026-11-01). So a rootless box's memory is an
eviction by a stated rule (CLAUDE.md §2), not a hard cap, and the overshoot above is inherent to that: it bounds a
box of up to one member per core, and a kernel-enforced cap (jetsam, with the privileged helper's entitlement) is a
later step's. The rate (256 GiB/s) is a tuning constant measured here; another machine needs its own (CLAUDE.md §5).

## Rootless memory bound: the full map, and the better mechanism for step 2 (2026-11-01)

Ada asked whether there's a better mechanism than polling. There is — macOS has a per-task **physical-footprint
ledger limit** (`task_set_phys_footprint_limit`, `mach/task.h`) that fires a Mach `EXC_RESOURCE` the instant a task
crosses it: event-driven, near-overshoot-free, ~zero watcher CPU. Apportioning a box's bound across its members as
per-task limits bounds the whole tree (the limits sum to the bound, so by pigeonhole the tree can't exceed it without
some member crossing its share and firing). But **every rootless path to a kernel-enforced or event-driven per-task
memory bound is refused**, measured on adas-mac-studio-3 (`mem_bound_probe.c`, record
`records/mem-bound-18fd653-20261101T060308Z.txt`):

| mechanism | rootless result |
|---|---|
| `task_set_phys_footprint_limit` (Mach ledger limit) | `KERN_NO_ACCESS` (kr 8) |
| `memorystatus_control(SET_MEMLIMIT_PROPERTIES)` (jetsam) | `EPERM`, on self and a child |
| `setrlimit(RLIMIT_AS)` (address space) | `EINVAL`; not enforced (allocated 515 MB past a 192 MB limit) |
| `setrlimit(RLIMIT_RSS)` (resident) | `EINVAL`; not enforced |
| `EVFILT_MEMORYSTATUS` (kqueue memory event) | not in the public SDK |
| `posix_spawnattr_setjetsam_np` (spawn-time jetsam limit) | symbol absent from libSystem (`dlsym` → NULL) |
| `proc_rlimit_control` | exists, but no memory cap — only CPU/wakeups monitors and `RLIMIT_FOOTPRINT_INTERVAL`, which *tracks* footprint over an interval (a read), not an event or a cap |

The investigation is exhaustive: every rootless entry point to a kernel-enforced or event-driven per-task
memory bound on macOS is closed. In-process allocator interposition (`DYLD_INSERT_LIBRARIES`) is not a bound
either — library validation rejects it for platform binaries (the common case, e.g. `/usr/bin` toolchains),
and it would count mapped VM, not footprint.

So **polling `proc_pid_rusage` is the only rootless mechanism** for a process tree's footprint on macOS — nothing
kernel-enforced, nothing event-driven, is available unprivileged.

Ada's call (2026-11-01) is a **two-layer bound**. The **primary** layer is **step 2's**: the installer's privileged
helper arms the per-task physical-footprint ledger limit (`task_set_phys_footprint_limit`), apportioned across the
box's members so the limits sum to the bound, and catches the Mach `EXC_RESOURCE` the kernel fires the instant the
tree crosses it — kernel-enforced, event-driven, near-zero overshoot, ~zero watcher CPU, bounding the whole tree,
better than the polling rule and than Linux's own path (which has no per-task notification). The polling rule stays
as the **defense-in-depth second layer** ("it's a good defense in depth mechanism") — the whole bound on a node with
no helper, and a backstop beneath the ledger limit otherwise. Step 1 ships the polling layer (this record's limits:
≤ one member per core, ~12-15% of a core held at its bound); step 2 adds the ledger layer, where the overshoot and
watcher-CPU criteria are met.
