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

## The parts, (A) tmpfs and (B) disk, and the record's fsync's share of (B)

A box whose bundle is on disk keeps its record on disk, fsynced (goal 5 extended step 5; decision 1), so (B) is what a
node with disk bundles pays on every create, not only a bench's setup. Pooled over 1,500 traced creates a run,
create-breakdown-20261104T161018Z (A) and -161115Z (B), the same binaries and run as above. p50 / p99 / mean, ms:

| # | part | (A) tmpfs | (B) disk |
|---|---|---|---|
| 1 | the call reaches the node; the spec checked; its turn taken; the record looked for | 0.009 / 0.023 / 0.011 | 0.013 / 0.045 / 0.017 |
| 2 | its cgroup looked for; a watch slot reserved; the box root counted | 0.006 / 0.016 / 0.006 | 0.009 / 0.026 / 0.011 |
| 3 | the cgroup made (kernel) | 0.078 / 0.300 / 0.089 | 0.100 / 0.334 / 0.126 |
| 4 | its bounds written | 0.019 / 0.300 / 0.029 | 0.028 / 0.093 / 0.035 |
| 5 | the state directory made; config.json read and parsed; its device policy planned | 0.018 / 0.039 / 0.023 | 0.055 / 0.173 / 0.068 |
| 6 | the exec FIFO made; the device program's instructions made | 0.023 / 0.040 / 0.025 | 0.048 / 0.151 / 0.058 |
| 7-8 | the device program loaded and attached (held: none) | 0 / 0 / 0 | 0 / 0 / 0 |
| 9 | the pipes; a namespace made ahead taken; the request to a seed | 0.024 / 0.092 / 0.031 | 0.051 / 0.215 / 0.065 |
| 10 | the seed's clone prepared | 0.018 / 0.087 / 0.023 | 0.034 / 0.096 / 0.040 |
| 11 | the init's clone into its cgroup and namespaces | 0.080 / 0.480 / 0.102 | 0.119 / 0.512 / 0.157 |
| 12 | the init's first steps, to its join of the network namespace made ahead | 0.028 / 0.126 / 0.041 | 0.038 / 0.128 / 0.052 |
| 13 | the namespace made ahead joined (setns) | 0.002 / 0.015 / 0.003 | 0.003 / 0.018 / 0.005 |
| 14 | the init's root attached and its mounts made | 0.052 / 0.286 / 0.066 | 0.073 / 0.249 / 0.095 |
| 15 | the root chrooted into | 0.001 / 0.002 / 0.001 | 0.001 / 0.004 / 0.001 |
| 16 | the hostname; the process become; ready sent | 0.010 / 0.028 / 0.013 | 0.018 / 0.050 / 0.021 |
| 17 | the agent wakes to the ready signal; the init's stat opened | 0.010 / 0.137 / 0.017 | 0.015 / 0.128 / 0.021 |
| 18 | the init's start time read; state.json written and renamed | 0.023 / 0.071 / 0.025 | 0.067 / 0.238 / 0.087 |
| 19 | the record encoded | 0.010 / 0.033 / 0.012 | 0.016 / 0.055 / 0.020 |
| 20 | its temp file made (kernel) | 0.003 / 0.011 / 0.003 | 0.014 / 0.058 / 0.017 |
| 21 | the record written | 0.002 / 0.010 / 0.002 | 0.004 / 0.020 / 0.006 |
| **22** | **the record's fsync (kernel)** | 0 / 0 / 0 | **4.078 / 20.525 / 5.921** |
| 23 | the record closed and renamed; the store's directory opened | 0 / 0 / 0 | 0.092 / 0.271 / 0.114 |
| **24** | **the directory's fsync (kernel)** | 0 / 0 / 0 | **5.149 / 20.780 / 7.001** |
| 25 | the init watched; the box's info made; the call returns | 0.012 / 0.029 / 0.013 | 0.062 / 0.141 / 0.068 |
| | the parts summed | 0.428 (p50s) / 0.535 (means) | 10.087 (p50s) / 14.009 (means) |

**The record's fsync's share of (B):** the two fsyncs (22, 24) are 9.227 ms of the parts' 10.087 ms p50 sum (91.5%),
and 12.922 ms of their 14.009 ms mean (92.2%). The box's record as a whole (parts 19-24) is 9.353 ms p50 (92.7%), 13.080
mean (93.4%). Everything else in (B) sums to 0.734 ms at p50 against (A)'s 0.428: 1.7x, the overlay's metadata
operations (parts 5, 6, 18, 20) and the host. The parts cover the traced rounds, whose creates were 9.78 ms at p50. The
untraced seam rounds of the same run were 19.41 ms, the fsync's own variance by round (the traced rounds 8.6, 19.7 and
8.6 ms). At p99 each fsync is about 20.5 ms.

## A rootless node: which of its boxes pay, and what it costs on its disk

**Which boxes pay.** A record is durable, fsynced file and directory, when any directory it names lies off tmpfs
(`crates/hex-node/src/store.rs:608-613`); otherwise it is written once, with no sync (`:309-345`). On a rootless node:
- **Container state** sits under `$XDG_RUNTIME_DIR` (`/run/user/<uid>`), tmpfs on a systemd host. A **bundle** box
  (`ClassSpec::Bundle`, the caller's bundle) names only that state, so its record is non-durable: **no fsync**.
- An **OciSpec** box (`ClassSpec::Oci`, rendered by the node) also names its rendered bundle and writable layer,
  under the node's **state directory** (`<state>/bundles/<id>`, node_socket.rs's `rendered`), which is on disk. Its record
  is durable: **two fsyncs on every create**, and its delete's update and removal sync too.
- A **process** box names no directory: non-durable, no fsync.
- A rootless node with **no `XDG_RUNTIME_DIR`** keeps container state in its state directory on disk
  (node_socket.rs's `Kept::Disk`), so every `oci` box pays.

kiln's runners today (`docs/design/runtime.md`, consumer #2 and CP-T7.1's HexSandbox): the sandbox links the node for
**process** boxes (no fsync), and the container executor shells out to `hex oci` (the runtime alone, no node record).
So neither of kiln's current paths pays this. A node making OciSpec boxes does, as goal 3's `docker run`-like boxes and
the seam's OciSpec class are made, or one without a tmpfs runtime directory.

**What it costs there.** The store's durable write reproduced: the temp file opened `O_CREAT|O_TRUNC`, ~1.5 KB
written, fsynced, renamed into place, its directory opened and fsynced (`store.rs:345-375`). Against the non-durable
write (`O_CREAT|O_EXCL`, written, no sync). Python's os calls, the same system calls, not Hex's code. On a rootless
node's state directory's filesystem (`$HOME`, btrfs `compress=zstd:1`, not an overlay) and its runtime directory
(`/run/user/1000`, tmpfs): 5 rounds of 1,000 each, interleaved, single-threaded on CPUs 4-15 under the timed lock. Unit
hex-hooks-recfsync-20261104T163103Z through hex-slot; load 111.3 at the start, 55.4 at the end, I/O pressure avg60 22.7%.
p50 / p99 / mean, ms:

| where | durable write, whole | of it, the file's fsync | of it, the directory's fsync | non-durable write |
|---|---|---|---|---|
| `$HOME` (btrfs), a rootless node's state directory | 15.634 / 37.859 / 16.263 | 7.595 / 22.361 / 8.527 | 6.257 / 20.387 / 7.344 | 0.074 / 1.274 / 0.112 |
| `/run/user/1000` (tmpfs), its runtime directory | 0.012 / 0.022 / 0.013 | 0.000 / 0.001 / 0.000 | 0.000 / 0.000 / 0.000 | 0.006 / 0.012 / 0.007 |

At this load the pair costs 15.6 ms at p50 and 37.9 at p99 on the rootless state directory's disk, near the root
container overlay's 9.2 ms p50 (parts 22 and 24, at load 17). The two fsyncs are 89% of the durable write's p50 and 98% of
its mean; the rename and opens are the rest. The script is `~/.cache/hex-t7/hooks/bin/record_fsync.py` on fedora; its output is beside the
unit's log.
