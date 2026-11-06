# kiln's headroom pacer stops Mac test runs (report for kiln, 2026-11-06)

## What is paced

kiln's runner on the Macs (adas-mac-studio-2, adas-mac-studio.local-2, adas-mac-studio-3) stops processes in
a kiln run's process tree with SIGSTOP, for up to 19-38 s at a time. That covers the test binary, the
programs it starts and their helpers.

- Step 5's agent measured a 20 ms sleep taking 19 s inside a kiln run.
- `ps -o stat` shows the paced processes in state `T`.

## The A/B that shows it

The same trees and the same Mac, run two ways.

**Under kiln runs: 0 of 3 passed.** The integrate agent's runs of `macos_process_box` at 5ec923e and fb9084f,
on studio-2 and .local-2:
- `a_programs_end_is_seen_when_it_ends_not_at_the_waits_budget` saw ends 10-38 s late. 60 programs that
  each hit a 256 MiB cap normally die in well under a second; under the pacer, 40+ of them took 12-38 s.
- The contract test (HEX-RT-4) failed in 5 of 6 runs.
- One build took 6m45s.
- Log: `~/.cache/hex-t7/integrate/ab-5ec923e-1.log` on studio-2.

**As launchd one-shots, outside kiln's run tree: 7/7 in 3 of 3 runs.** No `T` state was seen.
- At 5ec923e: `hex-macos-late-20261106T074934Z` (studio-2, 07:49-07:51Z, 28-30 s per run). A sampler logged
  every `T`-state process each 2 s and saw none. Logs: `~/.cache/hex-t7/macos/logs/hex-macos-late-20261106T074934Z.{log,stopped,exit}`.
- At fb9084f: the integrate agent's one-shots, 24.3, 24.6 and 23.6 s.

Hex's code was unchanged between the two.

## What it breaks

- **Timing tests.** Any test that waits on an event with a budget fails when the pacer stops it, or its
  child, past the budget: exit-seen-late, the contract's waits, memory-bound kills. On the Macs, kiln runs
  can't be the evidence for Hex's tests.
- **Availability.** Since about 08:10Z on 2026-11-06, the Macs keep dropping off kiln mid-command:
  - "disconnected from the kiln server"
  - "did not answer in 15s", including one-line probes with no sync
  - "did not answer the sync in 60s"
  - "offline"
  - studio-2 listed at "0 of 0 cores · 0 MB free"

  For hours, no Mac work could start at all, launchd one-shots included, since they are launched through
  kiln.

## The code it points at

kiln's headroom guard, `crates/kiln/src/headroom`. Its learned "kiln server" detector blames local work on
remote runners for production's delay on adacachy, and paces those runners (SIGSTOP) to a learned CPU
budget. The Macs' work never runs on adacachy, so pacing them can't relieve it.

The pacer's own record of the pids it stopped is `~/.local/share/kiln/agent/headroom/paced/*.json` on each
Mac. It has not been read for this report, because the Macs were unreachable.

## Ask

Don't pace remote runners for adacachy's load, or exempt runs that ask not to be paced, and find why the
Macs' kiln connection drops since ~08:10Z. Until then, Hex runs Mac tests as launchd one-shots outside
kiln's run tree.
