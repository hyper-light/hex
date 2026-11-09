# The cost rule tried on the peer grade (F2), 2026-11-06

`bench/health/replay_grade.py 200` on adacachy (python 3, the gray nemesis's model: 4-200 nodes, ten
pieces of work a node a window, noise 0.7-1.5x, one node 2.5-8x slower from a 5-20 s onset, ten windows
after it). The probe's cost form, carried over: bar `x` and run length `k` chosen by the least expected
loss `(1 - pi) S(x)^k C + pi k W (x - 1)` over the healthy ratios learned, `pi` Jeffreys with no recorded
gray incidents, Suspect past the largest healthy ratio, ended runs learned back.

```
C=2 ms: false suspects=662 false drains=122 undetected=0 undrained=168 drained within 4 windows=0
C=194 ms: false suspects=652 false drains=0 undetected=0 undrained=200 drained within 4 windows=0
```

Today's rule (2x its peers, 3 windows in a row) on the same nemesis in hex-sim, 1,000 seeds: every slow
node suspected within 2 windows and drained within 4, no wrong verdict
(`a_slow_node_is_suspected_in_2s_and_drained_in_4s`).

Why it fails: with no gray incident on record, the prior of a slow node per node-window is
`1/2 / (n + 1)`, so waiting looks nearly free and the rule waits past every bound (no gray node drained
in 4 windows at either price); at a cheap re-placement it also drains healthy nodes at their new maxima.
The sequential form (drain when the posterior odds times the observed excess pass `C`) fails the other
way: the odds after one window beyond the healthy maximum are about 1, so it drains at every new healthy
maximum. Either way the missing input is the prior and the slow nodes' own distribution: a record of
gray incidents (how often a node turns slow, and how slow) from the fleet. None exists: kiln's CI
records give job durations by runner, but no runner is marked as having been gray, and their spread on
one runner (1,134 jobs of a repeated job, job over that runner's median: p99 2.92x, max 3.84x) is wider
than the design's 2.5x class, so they can't stand for healthy peers of one hardware class either.
