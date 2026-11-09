# What a seam call costs over the node agent's socket

`seam_oci` (`crates/hex-bench/benches/seam_oci.rs`) drives the `oci` class through the `Runtime` seam by either of
its drivers, each once a round in the OCI pass's shuffle (`oci_inside.sh`): in process (`hex-seam`, the node agent
in seam_oci's process), and over the node agent's local socket (`hex-seam-socket`, a `hex node` process seam_oci
starts and reaches through `hex-client`). Beside the lifecycle and throughput phases, its `rtt` phase times
`inspect` of one running container, whose work (the box's record and its container's state, read) is the same
through either driver, so the drivers' difference is the socket's cost per call. Decision 3 of
`docs/design/runtime-architecture.md`, goal 3 plan step 4; `summarize.py` prints the table below from a run's JSON.

## Run 1 (2026-10-27, fedora)

- **Host:** kiln-a094a12d.attlocal.net (fedora), Intel Core i9-14900HX (32 CPUs, the container pinned to 4-31),
  kernel 6.19.10-300.fc44.x86_64, 62 GiB.
- **When and load:** 2026-10-27T16:49:41Z to 16:56:11Z. The 1-minute load was 11.95 at the start and 15.10 at
  the end, from other agents' builds and CI on the host: noisy, so not of record by CLAUDE.md §7 (`LOAD_GATE=0`).
  The figures below are differences and ratios within this run, its passes interleaved round by round.
- **Hex:** `hex 0.1.0 (0f41abb)`, sha256 5635c7c6d97eb050…, and `seam_oci` sha256 d7994bfd358be958…, both the
  release build of 0f41abb on fedora. **libcrun:** 1.30.1 (commit 079ff6a7), in process, driver 5c356f2139adc232.
- **Harness:** `bench/competitive` at 0f41abb, `host.sh` with `PHASES=oci RUNTIMES=libcrun STANDIN=0 ROUNDS=2
  N=500 WARMUP=50 LIFECYCLES=1000 HEXBENCH_PHASES="lifecycle throughput rtt"`. Out dir on fedora:
  `~/.cache/hex-bench-step4/out/20261027T164941Z/`. Every pass recorded errors `{}`.

### The socket's cost per call

p50 / p99 in ms, best of 2 by p50.

| Call | in process | over the socket | the socket's cost at p50 |
|---|---|---|---|
| inspect of a running box (`rtt`, 500 each) | 0.022 / 0.027 | 0.053 / 0.211 | 0.031 |
| start | 0.209 / 0.460 | 0.336 / 0.623 | 0.126 |
| exec of `true`, to its end | 1.562 / 1.972 | 1.766 / 2.441 | 0.204 |
| create | 18.01 / 21.86 | 17.20 / 21.66 | within the noise (−0.81) |
| delete | 6.69 / 10.96 | 6.18 / 8.92 | within the noise (−0.51) |

Each round alike: `rtt` p50 0.0219 and 0.0236 in process, 0.0572 and 0.0528 over the socket; start 0.209 and 0.230
against 0.336 and 0.411; exec 1.562 and 1.674 against 1.795 and 1.766.

- **A call over the socket costs 0.031 ms more at p50**, its request and answer between two processes: the client
  sends, the agent's I/O thread reads and hands the call to a worker, the worker answers, and the client's reading
  thread hands the answer to the caller. Its p99 is 0.211 against 0.027 in process, those four wakeups meeting a
  loaded host's run queues.
- **An exec costs more, 0.20 ms**, because its program's end is a second message: the agent's I/O thread sees the
  program's pidfd turn readable, reaps it and sends the end, which the client's reading thread hands to the waiting
  `Proc`. In process the `Proc` waits on the pidfd itself.
- **A start costs more than an inspect, 0.13 and 0.18 ms in the two rounds**, though both are one round trip. This
  run doesn't say why. A likely cause: a start follows a create's 17 ms, so its wakeups find the agent's threads
  idle, where the `rtt` phase's inspects come back to back. An `rtt` phase with a pause between its calls would test
  it.
- **It costs nothing like a spawn**, decision 3's falsifier: spawning and waiting for `/usr/bin/true` was 0.666 ms
  at p50 on fedora in the bench's setting (`oci_breakdown.md` §2, run 1), twenty times the socket's cost per call and
  three times what it adds to an exec.
- **Throughput holds over the socket:** at concurrency 1, 8 and 32 (32 threads sharing one client, so 32 calls in
  flight on one connection), 42, 54 and 55 lifecycles/s against 43, 55 and 52 in process (best of 2). What limits
  both here is the node's create (about 17 ms at p50 on this loaded host), not the socket. libcrun did 63, 308 and
  359.

## Run 2 (2026-11-09, adacachy2): the client's reading thread removed

- **What it compares:** `hex-client` at 601513bb (A), whose reading thread handed each answer to the caller waiting
  for it, against 8ded733f (B), where whoever waits on the connection reads it, one at a time, so an answer wakes the
  caller it answers and no thread between (goal 16, the socket's per-call cost). Only seam_oci differs, as it links the
  client: A's sha256 3537edbef1c60edd, B's ed6094a51de98385. The node, `hex` 2a45892e0b333eae, is the same binary in
  every pass.
- **Host and load:** adacachy2, its bench containers in a Linux VM (kernel 7.3.3-1-cachyos, x86_64, 32 CPUs). Four
  passes in ABBA order, 2026-11-09T18:31:40Z to about 18:41Z; the 1-minute load 10.7 to 17.2 at their starts and ends,
  from other work on the host: noisy, so not of record by CLAUDE.md §7.
- **Harness:** the OCI pass (`docker_pass.sh`, with cab0c3b7's fix so each pass builds its own commit), boxes of no
  owner, 10,000 creates a round, two rounds a pass. Out dirs on adacachy2: `~/.cache/hexpass/sock-{1,4}-noowner-A-601513bb`
  and `sock-{2,3}-noowner-B-8ded733f`; every pass recorded errors `{}`.

| Over the socket, p50 in µs, best of 2 a pass | A, passes 1 and 4 | B, passes 2 and 3 |
|---|---|---|
| create | 195.5, 172.6 | 179.6, 182.0 |
| inspect of a running box (`rtt`) | 20.1, 14.7 | 17.6, 17.9 |

- **No change is measurable here.** Each arm's two passes differ by more than the arms do (A's creates 195.5 against
  172.6, its round trips 20.1 against 14.7), so the one wakeup a call the change removes is under this host's noise at
  this load. It is kept: no pass shows harm, and the node's own handoff, the I/O thread's to a worker, is the other
  half (decision 3, "Threads").
