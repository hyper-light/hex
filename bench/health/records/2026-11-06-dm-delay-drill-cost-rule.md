# AC-T7.10.3 dm-delay drill under the cost rule: a false drain in warm-up (2026-11-06)

fedora (kiln-a094a12d), i9-14900HX, 6.19.10-300.fc44, drill on CPUs 4-15 under
`fedora-timed.lock`, `bench/health/dm_delay_drill.sh` at b51dcd5 plus this change (the container named,
and a cordon before the stall reported). Re-placement priced at 194 ms a unit (Shards' boot p99), the
record the drill's own 120 s healthy baseline under its load.

Run 1, 20:55:48Z, load 25.1 (the drill before it reported a cordon before the stall):

```
baseline fsync count=11374 p50_us=5288 p90_us=9784 p99_us=19902 p999_us=63584 p9999_us=152056 max_us=155980
drill probe=on units=8 deadline_ms=10000 period_ms=100 drain_age_at_stall_ms=50 marked_ms=58 drained_ms=8 moved=8 writes=5007 failed=0
drill probe=off units=8 deadline_ms=10000 period_ms=100 drain_age_at_stall_ms=0 marked_ms=none drained_ms=none moved=0 writes=1360 failed=8
```

Run 2, 21:00:17Z, load 73.0:

```
baseline fsync count=10054 p50_us=4842 p90_us=10493 p99_us=65010 p999_us=175405 p9999_us=312570 max_us=384419
drill probe=on units=8 deadline_ms=10000 period_ms=100 drain_age_at_stall_ms=167 cordoned_before_stall=true marked_ms=230 drained_ms=19 moved=8 writes=4177 failed=0
drill probe=off units=8 deadline_ms=10000 period_ms=100 drain_age_at_stall_ms=0 cordoned_before_stall=false marked_ms=none drained_ms=none moved=0 writes=924 failed=8
```

No work failed with the probe on, and all 8 units failed with it off, but the probe drained the healthy
disk during the 10 s warm-up, before the stall (run 2 reports it; run 1's drain 8 ms after the stall and
before its mark says the same). Its drain age (50 ms, 167 ms) sat below the baseline's own p99.9 (63.6 ms,
175 ms): with the stall prior over the node's own ~100 probes, `pi` is about 0.5% a probe, so waiting
looks dear and the rule drains inside the healthy tail. The record's ~10,000 healthy probes of the same
disk are stall-free observations the prior leaves out; counting them puts `pi` near 5e-5 and the drain
age near the record's slowest. That change is the lead's to rule on (their 19:03Z (a): the node's own
probes).
