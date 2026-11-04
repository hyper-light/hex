# Hex's create "under load": what grows (the lead's question, 2026-11-04)

**The question.** The hooks fix's interleaved A/B (`bench/oci-validation/hooks-conditions-20261104.txt` §7, exec_ab.sh,
load 22-61) put Hex's create p50 near 19 ms against libcrun's 2.4 ms in the same rounds. The seam's create had been
0.713 ms at load 2.8 (`create_breakdown.md`, 2026-11-03). That looked like a create path about 26x worse under
load. Which part grows, and on what does it wait?

**The finding: it is the box record's two fsyncs, when the node's state is on disk, not the load.** exec_ab.sh
starts its container without `--tmpfs /run`, unlike create_breakdown.sh and host.sh's OCI pass (6f7ba39). So a
root node's container state and records land on the container's overlay (btrfs underneath, `compress=zstd:1`),
where a record must outlive the host and is fsynced (goal 5 extended step 5's durable record). libcrun syncs
nothing. Run back to back, the same binaries, harness and lifecycle phase, with only `/run` differing:

| run | the node's state | load, start → end | Hex create p50 / p99 (ms) | libcrun p50 / p99 (ms) |
|---|---|---|---|---|
| create-breakdown-20261104T161018Z | `/run` a tmpfs | 24.0 → 18.2 | 0.439 / 1.372 | 1.715 / 2.763 |
| create-breakdown-20261104T161115Z | `/run` on the container's overlay | 17.5 → 15.7 | 19.409 / 41.099 | 2.084 / 5.284 |

The disk run had the lower load. The parts, pooled over 1,500 traced creates a run, each cut whole:

| part | tmpfs p50 / p99 | disk p50 / p99 |
|---|---|---|
| 22, the record's fsync (kernel) | 0.000 / 0.000 | **4.078 / 20.525** |
| 24, the directory's fsync (kernel) | 0.000 / 0.000 | **5.149 / 20.780** |
| 23, the record closed and renamed; the store's directory opened | 0.000 / 0.000 | 0.092 / 0.271 |
| 18, the init's start time read; state.json written and renamed | 0.023 / 0.071 | 0.067 / 0.238 |
| 5, the state directory made; config.json read and parsed | 0.018 / 0.039 | 0.055 / 0.173 |
| 11, the init's clone into its cgroup and namespaces | 0.080 / 0.480 | 0.119 / 0.512 |
| the parts' p50s summed | 0.428 | 10.087 |

The two fsyncs are 9.2 of the disk run's 10.1 ms. Every other part is within 3x of its tmpfs value: overlay
operations, and the host's I/O. On tmpfs the path doesn't grow with load either: create-breakdown-20261104T150143Z,
at load 56 falling to 27 (CPU pressure avg10 37.7% at its start), had the seam's create at 0.467 ms against libcrun's
2.122. The traced creates in the disk run spread from 8.6 to 19.7 ms by round, the fsync's own variance.

**What it waits on.** The fsync(2) of the record's temp file and of the store's directory, on btrfs through overlayfs,
while other tenants write (I/O pressure avg10 about 5% throughout). No pool, sleep, poll period or seed patience is on
the path: parts 9-17 (seed, clone, init, handshake) are within 1.5x of tmpfs. Off-CPU stacks weren't needed, since
the probe's kprobes cut the fsyncs as their own parts.

**Setup.** fedora (kiln-a094a12d), i9-14900HX, Linux 6.19.10-300.fc44, Docker 29.8.2. The container on CPUs 4-15,
`--memory 8g --oom-score-adj 1000`; units hex-hooks-loadparts2-20261104T161018Z through hex-slot, each run under
the timed lock. The harness is cp/t7-g5x-integrate f825ac0's bench/competitive, a scratch copy; for the disk run, a
second copy with only its `--tmpfs /run` removed. Binaries: f369f6f's static-glibc release (hex 0bf490040d4d57d1,
seam_oci cc3f8fb4d8bf0229). Run: N=1000 WARMUP=100 ROUNDS=3 HEXBENCH_DEVFS=1, root, the pruned path, no owner.
The final tree, 891245f, couldn't be cut: its seed takes the spec from a sealed memfd (5bc0a2c), and the reader still
looks for the seed's read of config.json (create-breakdown-20261104T160938Z refused every create). The
integration agent has it. 5bc0a2c doesn't touch the record's writes.

**Not done.** No fix, as the lead asked. The (c) runs (exec_ab) ran with exec_ab.sh's hard-coded `--memory=40g`
and no OOM score, not the 8g cap; the runs here honor BENCH_MEMORY and BENCH_OOM.

## On the integrated tree, through exec_ab's own lifecycle (the lead's follow-on)

The lead asked to reproduce the 19 ms on cp/t7-g5x-integrate 891245f with exec_ab's lifecycle (each create timed alone,
starts, execs and deletes in the same cycle, host.sh's pass of record's shape). The breakdown's `seam` row above is that
same seam_oci lifecycle phase, so the cycle's shape isn't what differs; `/run` is. Two runs of exec_ab.sh on 891245f's
static release binaries (hex f35641592e47972d, seam_oci 85fefcccf5e5b641), back to back, each under the timed lock; unit
hex-hooks-execab-20261104T161801Z through hex-slot. Scratch copies of 891245f's exec_ab.sh: (X) its `--memory=40g`
replaced by BENCH_MEMORY/BENCH_OOM (8g, 1000), nothing else; (Y) the same plus `--tmpfs /run:rw,exec,nosuid,size=1g`.
N=1000 WARMUP=100 ROUNDS=3, CPUs 4-15, root, every round's errors {}. Pooled, p50 / p99 (ms):

| run | load, start → end | Hex create | Hex start | Hex exec | Hex delete | libcrun create |
|---|---|---|---|---|---|---|
| (X) state on the overlay, exec-ab-20261104T161801Z | 25.6 → 47.1 | 17.082 / 37.292 | 1.001 / 2.237 | 1.497 / 2.969 | 18.724 / 39.181 | 13.825 / 22.003 |
| (Y) `/run` a tmpfs, exec-ab-20261104T162449Z | 75.5 → 59.9 | 2.005 / 3.959 | 0.902 / 2.591 | 1.314 / 2.899 | 2.444 / 5.242 | 13.776 / 41.646 |

The tmpfs alone takes 15 ms off create, and off delete, whose record removal syncs too, at a higher load. Start and exec
don't move. libcrun's create in these rounds is 13.8 ms (its delete 10.3) at load 25-75, against 1.7-2.1 in the
breakdown at load 18-24. That is the host's load on libcrun's own path, so in (Y) Hex's create is 0.15x libcrun's.
Hex's 2.0 ms in (Y), against 0.44 in the breakdown at load 24, is the host's load at 52-75; how much of that is Hex's
own sensitivity would need a lower-load pair, and fedora has been 16-75 all day. create_serial's off-CPU and lock
modes weren't needed for the 15 ms: the breakdown's parts 22 and 24 name the two fsyncs.
