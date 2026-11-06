# Hex against kube-scheduler on the same kwok cluster (AC-T7.11.3)

**Bar:** Hex's placements per second over kube-scheduler's binds per second, the best configuration of
each, is at least 10x, on the same node and pod shapes and the same hardware, measured on fedora
(`AC-T7.11.3`, G6 step 5; the method is `docs/design/hex-sim.md` H63).
**Result: MET.** kube-scheduler's best is 1,164 binds/s (client QPS raised). Hex's best is 9,308,487
placements/s (12 shards), 7,998x. Class for class, one scheduler against one (Hex on 1 shard,
485,025/s), it is 417x. Every pod was bound or placed in every run.

| | config | runs | best pods/s | every pod |
|---|---|---|---|---|
| kube-scheduler v1.37.1 | defaults (client 50 QPS, burst 100) | 3 | 50.10 | bound, 3 of 3 |
| kube-scheduler v1.37.1 | raised (client 5000 QPS, burst 10000; apiserver limits off) | 3 | 1,164 | bound, 3 of 3 |
| Hex (hex-sim fast path) | 1 shard | 5 | 485,025 | placed, 5 of 5 |
| Hex (hex-sim fast path) | 4 shards | 5 | 5,485,928 | placed, 5 of 5 |
| Hex (hex-sim fast path) | 12 shards | 5 | 9,308,487 | placed, 5 of 5 |

Ratios over kube-scheduler's best (raised, 1,164): 1 shard 417x, 4 shards 4,714x, 12 shards 7,998x.
Over its defaults (50.10), Hex on 1 shard is 9,681x.

## The cluster, both sides

H63's shape, written once by `crates/hex-bench/examples/sched_kwok.rs`:
- 10,000 nodes of 11 CPU, 55 GiB and 110 pods.
- 50,000 of `scheduler_perf`'s pause pod (100m CPU, 500Mi), all created pending before the clock.

## kube-scheduler's side

`bench/sched/kwok/run.sh series` (its header says how), on fedora, unit
hex-g6s5-series-20261105T031255Z, 2026-11-05 03:23Z-04:56Z, each run under
`flock /home/adalundhe/.cache/hex-t7/fedora-timed.lock` on CPUs 4-15.
- **Versions** (binaries pinned by sha256 in `run.sh`): kube-apiserver, kube-controller-manager,
  kube-scheduler and kubectl v1.37.1; kwok and kwokctl v0.8.0 (go1.26.0); etcd 3.6.10.
- **Cluster:** `kwokctl create cluster --runtime binary --disable kube-scheduler`, so kwok's fake
  nodes and an API server backed by etcd; `raised` adds `--disable-qps-limits`.
- **kube-scheduler:** started only after every node is Ready and every pod exists, with
  `--secure-port 0 -v 1` and a `KubeSchedulerConfiguration` without leader election. `defaults`
  leaves its client at its own 50 QPS and burst 100 (`docs/research/scheduler/scale-and-shards.md:54`,
  SS[6] `:149-154`). `raised` sets `clientConnection.qps: 5000`, `burst: 10000`.
- **Timed:** from the first bind to the last, read from the API server's watch by
  `bench/sched/kwok/watch_binds.py`. Each run also lists the pods afterwards: `bound_by_api` was 50,000
  every time. The rate from scheduler start is beside it (raised best 1,125/s).
- The node and pod manifests were written by `sched_kwok manifests` built at e54d11e with the tree's
  then-uncommitted hex-sim changes; the shape doesn't depend on hex-sim, and `run.sh prep` checks the
  counts (10,000 nodes, 50,000 pods).

| run | pods/s first to last | load before | load after | docker ps |
|---|---|---|---|---|
| defaults 1 | 50.09 | 33.34 | 39.15 | 6 |
| raised 1 | 920.19 | 30.02 | 41.34 | 6 |
| defaults 2 | 50.10 | 20.13 | 16.21 | 7 |
| raised 2 | 1,163.82 | 18.80 | 27.49 | 20 |
| defaults 3 | 50.10 | 12.66 | 18.97 | 6 |
| raised 3 | 936.43 | 13.36 | 26.01 | 6 |

Co-tenant hex units during the series included hex-g5rec-pass, hex-g5tail-tail/tail2,
hex-g6-mttimed, hex-shards-gates, hex-sched-g6s2-measure/verify, hex-sched-s4-time, hex-lsm-aa and
hex-windows-gates (each run's `host.txt` lists them).

## Hex's side

`sched_kwok hex 5 1,4,12`, the release example built at 2651541 (the example's code is b299869's), on
fedora, unit hex-g6s5-hextime-20261105T043241Z, 2026-11-05, CPUs 4-15 under the timed lock (taken
04:51:11Z). It submits all 50,000 pods to one store before the clock, then times hex-sim's fast path
(sampled fit, compare-and-set commit, index update), one thread per shard over contiguous node slices,
from the first decision to the last commit. After each run `check` confirms every pod placed and no
node over its capacity or 110 pods (`check=ok` in all 15 runs).
- Load: 17.27 / 22.62 / 23.89 when the lock was taken; 9.28 before the unit started. 7 containers
  before, 6 after. Co-tenant hex units: hex-g6s5-series, hex-sched-g6s2-measure, hex-shards-gates,
  hex-g5tail-tail2.
- Host: fedora, `kiln-a094a12d.attlocal.net`, Intel Core i9-14900HX (24 cores, 32 threads).

| shards | runs, placements/s | best |
|---|---|---|
| 1 | 452,571; 485,025; 449,186; 432,856; 454,269 | 485,025 |
| 4 | 5,485,928; 3,085,753; 3,097,900; 3,142,380; 3,015,135 | 5,485,928 |
| 12 | 7,405,786; 9,282,224; 9,308,487; 7,475,719; 8,886,668 | 9,308,487 |

## What the comparison is and isn't

kube-scheduler's binds go through the API server to etcd, and each is a write a real cluster would
keep. Hex's placements commit by compare-and-set to hex-sim's in-memory store, in one process. That is
the comparison AC-T7.11.3 names, the same simulated cluster for both, but the gap holds the cost of
kube-scheduler's API round trips as well as its scheduling. The host ran loaded well past CLAUDE.md
§7's load average of 2 (noisy, as Ada directs for the swarm), and the load was higher during most
kube-scheduler runs than during Hex's.
