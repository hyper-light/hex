# The seam's create through its seeds

Goal 5 plan step 4 (decision 5 of `docs/design/runtime-architecture.md`; kiln plan `01a1877e05c7748ca8865548aab76445`)
puts each seam create through a seed: a `hex __oci-seed` the node agent started once clones the container's init into
its cgroup and namespaces, in place of the shim's re-exec of the hex binary. This probe reads three of the step's
claims on fedora: a seed's clone of an init against the 1.038 ms exec it replaces (criterion 3), the execs of the hex
binary in a seam create (criterion 2: none), and what a seed holds in memory (the bound's cost). A fourth reading, the
node's seeds' counts over a sustained phase, is what decision 5's bound of 32 is judged by.

## How it runs

`seed_path.sh <hex> <seam_oci>` copies both binaries into the run's directory and runs `seed_path_inside.sh` in the
bench container with the create breakdown's flags (privileged, a private cgroup namespace, no network, pinned to CPUs
4-31, the host's `/usr`, `/run` a tmpfs), on the OCI pass's bundles, its cgroup root vacated to a leaf and its
controllers handed down as `oci_inside.sh` leaves it. Each reading runs `seam_oci` in process as root, every create
through a seed:

- **clone**: `seam_oci`'s lifecycle phase (create, start, exec of `true` and delete of a `sleep` container), `N` timed
  after `WARMUP`, under a trace instance of the run's own that records `sched_waking` and `sched_process_fork` for the
  processes named `hex` (the seeds, and each init until it execs its program), and from run 3 `sched_switch` to them.
  `seed_path.py` takes, for each init a seed clones, from the seed's first wakeup after its last clone (the node's
  request arriving on its socket) to the init's birth (the fork event the seed's `clone3` fires in the seed, after
  `copy_process`, `kernel/fork.c:2661` at v6.19) and, from run 3, to the init's first run (the first switch to it):
  the interval decision 5's falsifier for lever 2 names, from the request's arrival to the init's first run. From run
  3 only a wakeup by the node agent (`seam_oci`) starts an interval; before, any wakeup of a seed did, so run 1's
  longest interval began at another wakeup. The interval leaves out the agent's own hand-off of the request and any
  wait in line for a seed.
- **execs**: the same phase, `EXECS` creates after 5, under `strace -f -e trace=execve` over the whole process tree:
  each exec of the hex binary by its first argument (`__oci-seed`, a seed's start; `__oci-init`, the shim; `__oci-exec`,
  the exec helper), and each program exec'd.
- **memory**: each seed's `Pss` (`smaps_rollup`) and `VmRSS`, sampled every 50 ms while the node serves `MEMORY_N`
  creates; from run 3 the seeds alone, since an init a seed cloned keeps the seed's arguments until it execs (it is
  told apart by its own pid namespace).
- **seeds**: `seam_oci`'s sustained phase (`SUSTAINED` lifecycles at concurrency 32) and the node's create seeds'
  counts over it (`seam_oci`'s `seeds`: creates that waited for a seed, seeds started, the most busy at once).

Why not the create breakdown's tracer, which step 4's criterion 3 names: its cuts are the shim's (parts 15 to 22, the
shim's spawn, exec and start-up and its child's clone), which a seed create no longer has, and its syscall events
recorded nothing on fedora when this was written while an instance's sched events did (one run: 133 forks, 0 syscall
events). So this probe times the seed's clone by sched events alone, which cost a create least.

Not a timed pass: the load is recorded, not gated. The clone's interval is the seed's own work and waits, so CPU others
use shows in it only as their share of the cores it runs on.

## Run 1 (2026-10-30, fedora): at ca45e1a

- **Host and load:** fedora-2 (Intel Core i9-14900HX, 32 threads, 62 GiB; Linux 6.19.10-300.fc44.x86_64), pinned to
  CPUs 4-31. 2026-10-30T04:43:10Z. The 1-minute load was 23.16 at the start and 21.87 at the end.
- **What else ran:** four long-lived containers (`issue556oss-control-plane`, `buildx_buildkit_kagent-builder-v0.23.00`,
  `kind-registry`, `kiln-t10-minio`); no other bench of Hex's.
- **Binaries:** `hex 0.1.0` of ca45e1a (on cp/t7-g5-s4), sha256 19ce3ac5229fe2f7, static-pie; `seam_oci` of ca45e1a,
  sha256 476faa01049a8797; strace 7.3.
- **Run:** `~/.cache/hex-bench/out/seed-path-20261030T044310Z` on fedora, the harness at ca45e1a with this probe
  uncommitted. N=300, WARMUP=20, EXECS=50, MEMORY_N=400; no `seeds` reading yet (its `seam_oci` came after).

| Reading | Result |
|---|---|
| a seed's clone of an init, request to child (ms) | p50 0.256, p90 0.411, p99 0.875, max 15.778; n=319 of 320, all by one seed |
| execs over 55 creates | 113 in all; the hex binary 2 (`__oci-seed` 2, `__oci-init` 0, `__oci-exec` 0); `/usr/bin/sleep` 55, `/usr/bin/true` 55 |
| a seed's memory, 44 samples | `Pss` KiB min 723, median 830, max 1,754; `VmRSS` KiB median 1,564 |

- **The clone.** Its p50 is a quarter of the 1.038 ms exec it replaces (`hex --version`, spawned, exec'd and ended; f1,
  f3) and under the 0.878 ms of the shim's spawn and exec in the breakdown's run 1. One create of the 320 has no
  interval: its request was on the socket before its seed blocked, so the seed had no wakeup to time from. The max is
  the first create's: its seed's first wakeup was at its own start, before the request.
- **The execs.** Each create's init execs only its program, never hex: 0 per create. The hex binary's two execs are
  the node's two seeds' starts (its first, and the spare kept idle beside it), made once, ahead of the creates.
- **The memory.** Two seeds were up: the one serving the creates and the idle spare, which share the binary's pages,
  so a seed's `Pss` is about half its `VmRSS`.

## Run 2 (2026-10-30, fedora): at e302ac8, with the seeds' counts

- **Host and load:** as run 1's. 2026-10-30T04:58:17Z, beside Focal's fleet test (34 `s5-focal*` containers) and a
  burst of kiln CI jobs: the 1-minute load 36.23 at the start and 37.49 at the end.
- **Binaries:** `hex` as run 1's (e302ac8 changes no line it's built from; the build's sha256 is the same,
  19ce3ac5229fe2f7); `seam_oci` of e302ac8, sha256 cf0fa3001bbf1d87, which reads the create seeds' counts.
- **Run:** `~/.cache/hex-bench/out/seed-path-20261030T045817Z` on fedora, the harness at e302ac8. N=300, WARMUP=20,
  EXECS=50, MEMORY_N=400, SUSTAINED=10000.

| Reading | Result |
|---|---|
| a seed's clone of an init, request to child (ms) | p50 0.209, p90 0.262, p99 0.856, max 0.989; n=320 of 320, all by one seed |
| execs over 55 creates | 113 in all; the hex binary 2 (`__oci-seed` 2, `__oci-init` 0, `__oci-exec` 0); `/usr/bin/sleep` 55, `/usr/bin/true` 55 |
| a seed's memory, 58 samples of 2 seeds | `Pss` KiB min 706, median 830, max 1,754; `VmRSS` KiB min 1,564, median 1,564, max 2,488 |
| the seeds over a sustained phase, 10,000 lifecycles at concurrency 32 (1,116/s) | asked 10,000, waited 45, refused 0; started 32, restarted 0, failed to start 0, died 0, hung 0; most busy at once 32 |

- **The clone** is run 1's or less: p50 0.209 ms, a fifth of the 1.038 ms exec it replaces.
- **The execs** are run 1's: 0 per create.
- **The memory.** A seed's `VmRSS` is 1.5 to 2.4 MiB (the busier seed's the larger), its `Pss` 0.7 to 1.7 MiB with
  two seeds sharing the binary's pages. So the bound of 32 seeds caps them at 32 x 2,488 KiB, about 78 MiB, counted
  as if they shared nothing.
- **The seeds' counts.** Under this load the sustained phase started all 32 seeds and had all 32 busy at once. A
  seed is busy from its create's handing to its answer, and at concurrency 32 its clone waits on `cgroup_mutex`
  (`CLONE_INTO_CGROUP` takes it in `copy_process`; `create_serial.md`'s run 3, under the same load, timed a fork into
  a cgroup at 1.014 ms), so the creates in flight hold their seeds longer than the clone's p50 at concurrency 1: 32
  busy is the clones' queue on the lock, not a need for 32 (fewer seeds would move that queue into the pool's line
  without changing the rate). 45 of the 10,000 creates waited for a seed, none was refused; the count doesn't say
  whether a create waited for a seed starting or for one busy.

## Run 3 (2026-10-30, fedora): at 72797aa, the clone to the init's first run

- **Host and load:** as run 1's. 2026-10-30T06:06:13Z, beside the four long-lived containers and kiln CI jobs: the
  1-minute load 25.53 at the start and 29.59 at the end.
- **Binaries:** `hex 0.1.0` of 87f4aff (on cp/t7-g5-s4), sha256 32ef384ba04ff6ea, static-pie, built in the pass image
  (`hexbench/pass:44`, its glibc-static); `seam_oci` of 72797aa, sha256 cb3a2089e909cc7e.
- **Run:** `~/.cache/hex-bench/out/seed-path-20261030T060613Z` on fedora, the harness at 72797aa. N=300, WARMUP=20,
  EXECS=50, MEMORY_N=400, SUSTAINED=10000.

| Reading | Result |
|---|---|
| a seed's clone of an init, request to the init's birth (ms) | p50 0.225, p90 0.264, p99 1.039, max 1.907; n=320 of 320, all by one seed |
| the same, request to the init's first run (ms) | p50 0.234, p90 0.278, p99 1.050, max 1.918; n=320 |
| execs over 55 creates | 113 in all; the hex binary 2 (`__oci-seed` 2, `__oci-init` 0, `__oci-exec` 0); `/usr/bin/sleep` 55, `/usr/bin/true` 55 |
| a seed's memory, 52 samples of 2 seeds | `Pss` KiB min 731, median 838, max 1,690; `VmRSS` KiB min 1,572, median 1,572, max 2,424 |
| the seeds over a sustained phase, 10,000 lifecycles at concurrency 32 (1,518/s) | asked 10,000, waited 46, refused 0; started 21, restarted 0, failed to start 0, died 0, hung 0; most busy at once 20 |

- **The clone** to the init's first run is 0.234 ms p50, under a quarter of the 1.038 ms exec it replaces; its p50s
  to the init's birth and to its first run are 9 us apart.
- **The execs** are runs 1 and 2's: 0 per create.
- **The seeds' counts.** Under a lighter load than run 2's the sustained phase used 21 seeds, 20 busy at most: how
  many a run holds tracks how long its clones wait on `cgroup_mutex`, so it moves with the host's load.

## Run 4 (2026-10-31, fedora): at 9669133, the device program held (goal 5 extended, step 1)

- **Why:** goal 5 extended, step 1's first criterion: a default-policy seam create after the node's start makes no
  `BPF_PROG_LOAD` and no `BPF_PROG_ATTACH`. The probe gains its `bpf` reading for it: `strace -f -y -e trace=bpf` over
  the execs reading's 55 creates, the node's start with them.
- **Host and load:** as run 1's. 2026-10-31T21:04:33Z to 21:05:13Z, beside kiln CI and the long-lived containers: the
  1-minute load 35.77 at the start and 41.57 at the end. The host's clock: a burst took 0.124 ms at rest on a P-core
  before, and 0.119 after.
- **Binaries:** `hex 0.1.0` of 4e518c0, sha256 2a475c14f822c00d, static-pie, built in the Fedora 44 pass image;
  `seam_oci` of 4e518c0, sha256 8a83b136fdcb4b8c. 4e518c0 is 32db274's code with a CLAUDE.md line, a commit the
  branch has since dropped, kept as cp/t7-g5x-s1-measured.
- **Run:** `~/.cache/hex-bench/out/seed-path-20261031T210433Z` on fedora, the harness at 9669133 (its env.txt names
  7953067: the same commit before the branch dropped a CLAUDE.md change, kept as cp/t7-g5x-s1-measured). N=300,
  WARMUP=20, EXECS=50, MEMORY_N=400, SUSTAINED=10000.

| Reading | Result |
|---|---|
| a seed's clone of an init, request to the init's birth (ms) | p50 0.266, p90 0.626, p99 1.381, max 1.888; n=319, all by one seed |
| the same, request to the init's first run (ms) | p50 0.280, p90 0.700, p99 1.393, max 1.899; n=319 |
| execs over 55 creates | 113 in all; the hex binary 2 (`__oci-seed` 2, `__oci-init` 0, `__oci-exec` 0); `/usr/bin/sleep` 55, `/usr/bin/true` 55 |
| bpf calls over 55 creates and the node's start | `BPF_PROG_QUERY` 1, `BPF_PROG_GET_FD_BY_ID` 1, `BPF_OBJ_GET_INFO_BY_FD` 2; `BPF_PROG_LOAD` 0, `BPF_PROG_ATTACH` 0 |
| a seed's memory, 59 samples of 2 seeds | `Pss` KiB min 700, median 1,292, max 1,698; `VmRSS` KiB min 1,580, median 2,440, max 2,440 |
| the seeds over a sustained phase, 10,000 lifecycles at concurrency 32 (844/s) | asked 10,000, waited 39, refused 0; started 32, restarted 0, failed to start 0, died 0, hung 0; most busy at once 32 |

- **The device program:** no create loaded or attached a program, so 0 of each per create. The node's start made the
  four calls: it asked which program the oci boxes' root carries (`BPF_PROG_QUERY`), opened the one there by its id
  and read its identity and its translated instructions (`BPF_PROG_GET_FD_BY_ID`, two `BPF_OBJ_GET_INFO_BY_FD`), found
  the default's, attached by the node of the clone reading before it in the same container, and kept it.
- **The execs** are the earlier runs': 0 per create.
- **The load** was heavier than run 3's (35.77 against 25.53), and the clone's p90 and the sustained rate show it.
