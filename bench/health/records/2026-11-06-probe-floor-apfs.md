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
