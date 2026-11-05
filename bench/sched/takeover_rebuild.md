# A takeover's rebuild of a slice's plane (T-T7.11.1)

**What it sets:** `sim::REBUILD_NS_PER_RECORD`, the virtual time hex-sim charges a takeover for each
store record its rebuild reads (`docs/design/hex-sim.md` H11). **Result: 130 ns a record (best of 5).**

## What was measured

`crates/hex-sim/tests/shards.rs`, `rebuild_cost`, in release. It runs 1,000,000 workloads that never
end on 10,000 generated nodes (seed 7) under 8 shards, so each slice holds an eighth of them placed.
It then rebuilds slice 3's plane from that store five times, wall-clock (`ControlPlane::rebuild`,
the function a takeover runs). Each rebuild walks all 1,000,000 records and keeps the slice's
125,000. The best of the five, divided by the records the slice kept, is the cost a record.

## The run of record

- Host: fedora, `kiln-a094a12d.attlocal.net`, Intel Core i9-14900HX (24 cores, 32 threads).
- Placement: `taskset -c 4-15`, under `flock /home/adalundhe/.cache/hex-t7/fedora-timed.lock`.
- Commit: `8a2ef32` (cp/t7-sched-g6), release (`cargo test --release`).
- Date: 2026-11-05, unit hex-sched-g6s2-measure-20261105T042030Z, started 04:25:32Z.
- Load: 37.90 / 35.89 / 31.76 at the start, 5.66 / 18.27 / 25.28 after. Co-tenant containers:
  issue556oss-control-plane, two buildx_buildkit_kiln-w08 builders, buildx_buildkit_kagent-builder,
  kind-registry, kiln-t10-minio. MemAvailable 50.9 GB.
- Output: `rebuild of slice 3: 125000 of 1000000 records, best of 5 16256334 ns, 130 ns a record`.

The host was loaded well past CLAUDE.md §7's load average of 2, so this reading overstates the cost.
A quieter host would charge less, and the takeover bound has room either way. At 130 ns, a full
slice of 125,000 records costs 16 ms against T-T7.11.1's 5 s.

## Takeover, in the same unit

At the provisional 1,000 ns a record, `a_lost_shard_is_taken_over_within_five_seconds` measured
detection 919 ms, a rebuild of 21,965 records (21 ms), and takeover 941 ms (loss to first commit).
21,965 workloads were running on the lost slice, and the slice made 21,965 commits after the
takeover. The mutant that rebuilds placed workloads as pending made the test fail (exit 101).

At the measured 130 ns (unit hex-sched-g6s2-verify-20261105T043322Z, 2026-11-05 04:33Z, release,
CPUs 16-31): detection 919 ms, a rebuild of 21,965 records (2 ms), and **takeover 922 ms** against
the 5 s bar. 21,965 were running on the lost slice, all undisturbed, and lost-or-double was 0. In the
same unit every hex-sim test passed in release and debug (the 1M takeover test took 12.6 s release,
91.8 s debug), and `cargo clippy -p hex-sim --all-targets` was clean.
