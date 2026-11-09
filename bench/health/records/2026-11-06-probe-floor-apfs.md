# A healthy disk's probe latencies, and the probe's timeout rules replayed on them

The data a learned disk-probe floor rests on (`docs/design/failure-isolation-constants.md`,
`probe::MIN_TIMEOUT_NS`). fedora, the Linux lab, was unreachable from 14:30Z; this is macOS first.

- Host: adas-mac-studio-2 (Apple M4 Max), macOS 26.2, APFS on SSD; `sync_data` is `F_FULLFSYNC`
- Run: `crates/hex-node/examples/fsync_latency` at f880495 + its `--dump`, a one-byte rewrite and its
  flush every 10 ms for 600 s, under `~/.cache/hex-t7/sched-g5/fsync`; noisy, as run
- First run, 2026-11-06T17:14Z, load 4.96 → 9.62: 14,940 probes, p50 4.1 ms, p90 8.0, p99 16.0,
  p99.9 30.6, p99.99 66.1, max 147.3 ms; the slow ones in bursts (six within 0.5 s at 402 s)
- Second run, with the trace, 2026-11-06T17:25-17:35Z, load 7.40 → 3.33: 12,715 probes, p50 4.0 ms,
  p90 4.7, p99 7.5, p99.9 10.7, p99.99 21.4, max 34.0 ms

`bench/health/replay_probe.py` on the second run's trace (as `DiskProbe` judges: RFC 6298's SRTT and
RTTVAR, Karn, Suspect past a timeout, Drain past three or after three slow probes in a row):

```
rfc-clamped  false suspects      0  false drains    0  a stall at the end: suspected at   1000.0 ms, drained at   3000.0 ms
rfc-bare     false suspects    497  false drains    5  a stall at the end: suspected at      5.4 ms, drained at     16.3 ms
learned-max  false suspects      8  false drains    1  a stall at the end: suspected at     29.7 ms, drained at     89.1 ms
```

- `rfc-clamped` is today's rule: RFC 6298's 1 s floor hides the disk's tail and costs a second to
  suspect and three to drain.
- `rfc-bare` drops the floor: the timer alone sits inside the tail and drains a healthy disk 5 times
  in 10 minutes.
- `learned-max` floors the timer at the longest probe the disk has completed without a drain (RFC's 1 s
  prior before the first): 30x faster to suspect and to drain, but its one false drain is warm-up,
  before the floor has seen the tail. A warm-up long enough to see a 1-in-N tail takes about N probes:
  N follows from the false-drain rate the owner tolerates, which is a requirement to state, not a
  constant to pick.

## Hex's rule for learned waits, replayed (2026-11-06, after the lead's rule)

Seeded from the first run's recorded slowest (147.3 ms), then the slowest only rising with the node's
answers (Karn: none past the bound), Suspect past the slowest and Drain past twice it, on the second
run's trace (`replay_probe.py <trace> 147330000`):

```
recorded-2x (seed 147.3 ms) false suspects      0  false drains    0  a stall at the end: suspected at    147.3 ms, drained at    294.7 ms
rfc-clamped       false suspects      0  false drains    0  a stall at the end: suspected at   1000.0 ms, drained at   3000.0 ms
```

The record supplies what the learned floor lacked, a prior and so a warm-up: no false drain, and a
stall drained 10x sooner than under RFC 6298's floor.

## Ada's cost-based drain, replayed (2026-11-06)

Third run, the record (2026-11-06T18:31-18:41Z, adas-mac-studio-2, load 5.8 → 4.8, heavier): 23,764
probes, p50 8.2 ms, p90 18.0, p99 32.3, p99.9 67.8, p99.99 259, max 586.1 ms (`apfs-trace-3.txt`).

The second run's trace replayed with the third as the record (`replay_probe.py <trace> <record max>
<record trace>`). The cost rule drains at the age T minimizing the expected loss per probe, (1 - pi) *
S(T) * C + pi * L * T: S from the record's and the node's healthy answers (a Jeffreys half-count past the
slowest), pi the Jeffreys posterior of a stall per probe, C/L the measured re-placement time per
workload (an OCI start, ~2 ms; Shards' measured boot p99, 194 ms):

```
rfc-clamped       false suspects      0  false drains    0  a stall at the end: suspected at   1000.0 ms, drained at   3000.0 ms
recorded-2x (seed 586.1 ms) false suspects      0  false drains    0  a stall at the end: suspected at    586.1 ms, drained at   1172.2 ms
cost rule (re-placement 2 ms/workload, record 23764 answers): false drains    2  a stall at the end drained at     69.5 ms
cost rule (re-placement 194 ms/workload, record 23764 answers): false drains    0  a stall at the end drained at    275.2 ms
```

The cost rule prices what the learned-wait rule's 2x fixes: dear re-placement waits longer and drains
nothing healthy; cheap re-placement drains a stall 17x sooner and accepts two false drains, each a 2 ms
restart.

## The cost rule in hex-health, replayed (2026-11-06)

The same trace and record through `hex_health::probe::DiskProbe` (adas-mac-studio-2, release build,
`cargo run --release -p hex-health --example replay_probe -- apfs-trace.txt apfs-trace-3.txt 2 194`),
beside the exact form above:

```
cost rule in hex-health (re-placement 2 ms/workload, record 23764 answers): false drains    2  a stall at the end drained at     67.1 ms
cost rule in hex-health (re-placement 194 ms/workload, record 23764 answers): false drains    0  a stall at the end drained at    335.5 ms
cost rule (re-placement 2 ms/workload, record 23764 answers): false drains    2  a stall at the end drained at     69.5 ms
cost rule (re-placement 194 ms/workload, record 23764 answers): false drains    0  a stall at the end drained at    275.2 ms
```

The same false drains; the drain ages differ by the probe's histogram buckets (25%), whose highest
values are its candidate ages, where the exact form takes each answer.
