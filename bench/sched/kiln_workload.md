# The recorded kiln workload (G2 step 3, AC-T7.4.3)

`kiln-ci-20261022-20261105.trace` is fourteen days of kiln's own CI, as hex-sim replays it: every leg
kiln ran for runs created from 2026-10-22T00:00:00Z up to 2026-11-05T00:00:00Z, on the runners that
ran them. `AC-T7.4.3` measures packing "by hex-sim with a recorded kiln workload"; this is that
workload, and `a_recorded_kiln_workload_replays_the_same_twice` (crates/hex-sim/tests/sim.rs) replays
it.

## Where and when it was recorded

- Source: the production kiln on adacachy (`http://127.0.0.1:7070`, public `http://adacachy:7070`),
  read-only through its HTTP API: `GET /api/runs` (paged on `created_at`), `GET /api/runs/:id` (each
  run's jobs) and `GET /api/agents` (each runner's facts). No database was opened and nothing was
  written to kiln. The token came from `kiln token` and was never printed or stored.
- Exported: 2026-11-05T21:39:09Z, by `bench/sched/kiln_export.py --since 2026-10-22T00:00:00Z
  --until 2026-11-05T00:00:00Z`.
- Why that API: kiln's newer `ci.runs.*` operations read only the CI engine's runs, and every one of
  these ran on kiln's older pipeline path, whose own read endpoints still serve them.
- `kiln-ci-20261022-20261105.export.json.gz` is the export itself (runs and jobs trimmed to the fields
  below, no commit messages or authors). `bench/sched/kiln_trace.py` turns it into the trace, and
  running it again on the export gives the committed trace byte for byte.

## What it holds

1,488 runs and 6,116 jobs. 4,655 jobs ran (they have a start and an end) and are the trace's legs;
1,461 never ran (cancelled or skipped before starting) and are counted, not traced.

| repository | legs |
|---|---:|
| hyper-light/kiln | 2,463 |
| hyper-light/slates | 1,777 |
| hyper-light/hyper-raft | 184 |
| hyper-light/hyperscale | 128 |
| hyper-light/mantle | 56 |
| hyper-light/hex | 15 |
| hyper-light/quiver | 14 |
| hyper-light/focal | 13 |
| hyper-light/amphora | 5 |

Each leg's arrival is its queue time, its run time its start to its end, and its need what kiln
measured it using: mean cores (CPU seconds over its run time) and peak RSS. 3,750 legs were measured.
905 were not (kiln recorded no CPU seconds or peak RSS for them) and take the median of the measured
legs of the same repository, pipeline and job; every one had such legs. 26 measured more CPU than
their runner has (CPU seconds past their run time times its threads; 10 failed, 10 timed out, 1 was
cancelled and 5 succeeded) and are capped at their runner; none measured more memory than it has.
59 were queued again after they started (a new attempt) and arrive at their start. 178 ran under
kiln's 1 s resolution and run for 1 s. Legs naming "bench" in a label, their pipeline or their job
are benchmarks; the rest build and test code and are builds. No leg asked for a GPU.

The nodes are the 10 runners with CPU and memory facts that ran at least one leg: adacachy (32
threads, 123 GiB, one NVIDIA GPU), jupiter (32, 124 GiB), fedora (32, 63 GiB) and seven Macs, Mac
Studios and MacBook Pros (14-32 threads, 36-128 GiB), each at its threads and memory. 2,682 legs ran
on Kubernetes pods, which kiln keeps no facts for: they are work here, but the pods are not nodes.
hex-sim's nodes carry no OS, so a replayed leg may land on a runner of another OS than the one it ran
on.

## What would make it wrong

The trace stands for kiln's CI if kiln's measurements are right. A leg whose CPU seconds or peak RSS
kiln mis-measured carries that error into its need; the 26 legs capped on CPU show some do. If kiln's
measured needs were systematically low (a cgroup that missed child processes, say), packing on this
trace would look better than on the hardware.

## One recorded day for G4's fair-share bar (AC-T7.5.2)

`kiln-ci-20261025-four-repos.trace` is the 2026-10-25 UTC day of the export above, made by
`kiln_trace.py kiln-ci-20261022-20261105.export.json.gz kiln-ci-20261025-four-repos.trace --day 2026-10-25
--repos hyper-light/kiln,hyper-light/slates,hyper-light/hyperscale,hyper-light/hyper-raft --owners`
(2026-11-05): 599 legs, an owner per repository, each of weight 1, on the same 10 runners.

Why that day: AC-T7.5.2's bar is four equal-weight repositories. Of the fourteen days it is the only one
where four repositories each ran 20 legs or more: kiln 286, slates 262, hyperscale 31, hyper-raft 20.
Those four are the bar's four. focal's and hex's one leg each are left out and counted in its header.
On every other day the third and fourth repositories ran under 20 legs, or only two repositories were busy.

What it showed (hex-sim, `tests/fair.rs` `four_equal_weight_repositories_over_a_recorded_day`,
`HEX_SIM_RECORDED_DAY` set to it, 2026-11-05 on jupiter): no contention. Replayed on its runners, the
four were never all waiting at once (0.00 contended hours), so there are no shares under contention to
hold to 2% of 25%, and the test refuses it ("no contention in the recording"). No day of the fourteen has
four repositories each asking for more than a quarter of the cluster: kiln and slates ask for nearly all
of it, and the rest ask for little.

### The bar on recorded days, measured (2026-11-05, for Ada's call on AC-T7.5.2)

`crates/hex-sim/tests/recorded.rs`, hex-sim run on jupiter (Windows x86_64, release; hex-sim is
deterministic, so any machine gives these numbers), bands-then-DRF beside submit order (FIFO) on the
same input. "Parts" are each owner's part of the dominant shares held while every owner had work waiting.

**(a) 2026-10-25, four equal-weight owners, on the largest cluster at which each asks for more than a
quarter of it.** Sizing: each owner's mean ask over the day (sum of need times run time, over 24 h)
allows a cluster CPU just under four times its mean millicores, or a memory just under four times its
mean bytes. Each owner is held by its CPU limit or else by its memory limit. The candidates take each
owner's CPU limit as the CPU, and the least memory limit of the owners that CPU leaves as the memory;
the largest is the one whose smaller fraction of the 10 real runners is largest. Result: one node of
1,497 millicores (hyper-raft's CPU sets it) and 9.49 GiB (hyperscale's memory sets it). 212 of the 599
legs are larger than that node (kiln 165, slates 42, hyper-raft 5) and are refused at admission.

| policy | contended | parts: kiln, slates, hyperscale, hyper-raft | worst from 25% | bar (5,000 ppm) |
|---|---|---|---:|---|
| DRF | 14.33 h, one stretch | 2.9%, 35.0%, 32.5%, 29.6% | 220,640 ppm | missed |
| FIFO | 1.46 h, one stretch | 0.1%, 1.7%, 69.9%, 28.3% | 448,998 ppm | missed |

Reading: kiln asks for more than a quarter only through legs too large for the cluster. Those of its
legs that fit ask for 0.012 cores over the day, under 1% of the node, so no policy can hold it at 25%.

**(b) kiln against slates, two equal-weight owners, at the 10 real runners**, on 2026-10-30
(`kiln-ci-20261030-kiln-slates.trace`, 170 legs). Of the fourteen days, run one by one under DRF, it is
the one where the two waited together longest: 560 us in all, in 3 stretches (FIFO: 420 us). Other days
range down to none on 2026-10-22.

| policy | contended | parts: kiln, slates | worst from 50% | bar (10,000 ppm) |
|---|---|---|---:|---|
| DRF | 560 us, 3 stretches | 85.2%, 14.8% | 351,665 ppm | missed |
| FIFO | 420 us, 3 stretches | 100%, 0% | 500,000 ppm | missed |

Reading: at the real cluster kiln and slates never wait together for longer than the control plane's
commit latency, so there is no contention for a share to be measured over.

On none of the fourteen days does the bar's precondition hold: four repositories each asking for
more than a quarter of the cluster they ran on. The generated four-repository day of
`tests/fair.rs` (worst 479 ppm under DRF, 133,982 under FIFO) is where it holds.
