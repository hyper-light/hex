# The host's guest client, before and after its buffer and stack changes (native plan step 5, constants)

The lead's rule: a learned buffer or stack size is kept only if neither latency nor throughput gets worse,
measured on the host it runs on. The change measured is d625933:
- the control reader's buffer goes from a picked 64 KiB to one whole frame (`stream::MAX_FRAME`);
- the stdio copiers' stack goes from a picked 64 KiB to std's default;
- the stdio connections lose their send timeout.

before = 589e019, after = 7a2e8c3, each with crates/hex-client/tests/guest_client_bench.rs. The client execs
`cat` through a stand-in agent on loopback TCP and measures two things:
- start: `exec` to the first stdout byte, p50, p99 and max over N;
- throughput: 1024 MiB through stdin and back out through stdout.

- machine: Adas-Mac-Studio-2 (Mac16,9, M4 Max, macOS 26.2), release builds, launchd one-shots
  hex.ns5.dcbench.20261106T180111Z (N=2000, 3 rounds) and hex.ns5.dcbench2.20261106T180140Z (N=1000, 4
  rounds; 35 s between runs so loopback's TIME_WAIT (2 × MSL 15 s) frees the ports a run used), interleaved
  before and after, under the host's own co-tenants (load average 5.9-10.6, as printed per run), nothing added.
- One extra "after" set (3 rounds, N=2000, load 10.3-10.6) came from hex.ns5.dcbench.20261106T180047Z, whose
  "before" side didn't build. The third round of hex.ns5.dcbench.20261106T180111Z's "after" ran out of loopback
  ports (the bench's own TIME_WAIT, not the client) and is left out.

| | before | after |
|---|---|---|
| start p50, best / median (µs) | 97.7 / 105.4 | 98.1 / 100.7 |
| start p99, best / median (µs) | 170.5 / 280.8 | 139.3 / 233.1 |
| throughput, best / median (MiB/s) | 1576.6 / 1483.4 | 1553.6 / 1478.7 |
| runs | 7 | 9 |

Runs (start p50 µs, p99 µs, MiB/s):
- before: (101.9, 280.8, 1576.6) (105.5, 170.5, 1493.9) (105.4, 293.3, 1458.4) (99.1, 257.6, 1451.9)
  (111.9, 282.9, 1524.4) (97.7, 255.7, 1481.3) (106.7, 299.9, 1483.4)
- after: (99.2, 249.4, 1449.4) (98.1, 139.3, 1478.7) (100.5, 162.5, 1489.8) (100.7, 166.6, 1553.6)
  (107.8, 171.9, 1493.2) (117.5, 233.1, 1523.0) (102.4, 288.3, 1399.5) (99.8, 294.8, 1474.4)
  (149.8, 491.0, 517.1, a co-tenant burst during that run: its start max reached 1.95 ms)

Reading: start p50 is equal (best 97.7 vs 98.1 µs, 0.4%), and after's p99 and median p50 are lower. Throughput's
best differs by 1.5% and its median by 0.3%, inside the 8% spread of the before runs alone (1452-1577). Neither is
worse beyond the runs' own noise. That judgement is the parent's and the lead's to confirm; the numbers are as
measured.

Not measured here: the guest's relay (Linux only, crates/hex-node/tests/relay_exec_bench.rs, queued for fedora),
and Windows hosts (jupiter down).

## The lead's call (2026-11-06)

Accepted as not worse on the Mac (median throughput −0.3%, inside the 8% run-to-run spread; p50 median better;
p99 18% better). CONDITION: the same interleaved before/after runs on the Linux relay (fedora) and the WHP host
(jupiter) when they are back; if either shows a throughput loss beyond its own run-to-run spread, the
one-frame reader goes back to a buffer learned from the descriptor (kind (a)), measured again.
