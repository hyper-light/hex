# The waits census: each learned wait's answer distribution

Decision 9 of `docs/design/runtime-architecture.md` and the lead's failure-bound rule (2026-11-06): before a node has
an answer of its own, each wait's bound is twice the slowest answer in that wait's recorded distribution for the host
class. This probe records those distributions. Each wait is a named `hex_core::patience::Patience`; with
`HEX_WAITS_CENSUS` naming a file (and `HEX_WAITS_CENSUS_MAX` its most lines), the node and its client append every
answer each wait records, `name nanos`, from the wait's own timing point (`hex_node::census`), so nothing here
re-implements a wait.

Run on fedora under hex-slot, its timed part under `fedora-timed.lock`:
`[N=10000] [CPUSET=4-15] [LIST_BOXES=N] [LISTS=100] [RESTART_BOXES=N] bash bench/competitive/probes/waits_census.sh <hex> <seam_oci>`.
It runs seam_oci over the node's socket (`HEXBENCH_DRIVER=socket`) in a container with host.sh's OCI-pass flags, in
its arms, each with a census file of its own (the suites arm's on the host, beside it); `waits_census.py` reads each into p50, p99, p99.9, max and n by wait:

| arm | the node's state | owner | phases |
| --- | --- | --- | --- |
| `tmpfs` | the container's tmpfs `/run` | none (the bar's no-owner conditions) | `lifecycle sustained rtt list restart` |
| `disk` | the host's own filesystem, fedora's btrfs (`/opt/disk`) | none | the same |
| `owner` | tmpfs | `HEXBENCH_OWNER=bench`, as host.sh's reuse-on pass | `lifecycle sustained` |
| `debug` | tmpfs, debug builds | none | the tmpfs arm's, on the debug binaries given |
| `suites` | the suites' own (`SUITES=<tree>`, waits_census_suites.sh) | each test's | gate 6's `cargo test --workspace`, every suite through, in a loop beside the probe |

The `suites` arm is the tests' own waits, each named `test.<crate>.<fact>` (`hex_testkit::waits`: a test process with
`HEX_WAITS_CENSUS` set records every answer its waits, and the node's waits in it, get, and disarms none), and the
suites' load the `debug` arm meets, as the gates' is. Each loop's exit status is recorded in `suites.txt`, read before
anything else runs; run 3's launcher logged `date`'s instead and stopped each loop at its first failing suite.

seam_oci's `list` phase makes `LIST_BOXES` boxes and lists them `LISTS` times, so a list's parts wait for their
reader; its `restart` phase makes `RESTART_BOXES` boxes, ends the agent with SIGTERM and starts it again over the
same state, so recovery lists their parts, then deletes them. Each is bounded by the node's own: a box past its
budget is refused, and the arm's error says so.

## What each row seeds, from which arm, and where

A row seeds its wait only with **n of at least 1,000**: the seed is the row's max, and with fewer samples than that the
nearest-rank p99.9 is the max itself, so the tail the seed must cover hasn't been seen. `waits_census.py` writes no
seed for a row under it, and names the row's n.

| census row | the wait | arm that seeds it | seeds | where |
| --- | --- | --- | --- | --- |
| `store.turns` | a record change's wait for the same box's: a turn's hold, its write and fsyncs | `tmpfs` and `disk`: the slower arm's | `WRITE_PATIENCE` | `crates/hex-node/src/store.rs`, `Store::open` |
| `node.turns` | a create or delete's wait for the same box's | `tmpfs` | `TURN_PATIENCE` | `crates/hex-node/src/node.rs`, the node's `Turns` |
| `node.emptying` | a killed box's emptying, from its kill to `populated 0` | `tmpfs` | `EMPTY_PATIENCE`, serve's `STOP_PATIENCE` | `node.rs`, `EMPTYING` |
| `node.listing` | a recovery's listing, per entry | `tmpfs` and `disk` (`restart`): the slower | `LIST_PATIENCE` | `node.rs`, `LISTING` |
| `serve.reads` | a list's worker's wait for its client to read a part | `tmpfs` (`list`) | `READ_PATIENCE` | `crates/hex-node/src/serve.rs`, `READS` |
| `client.answer.<kind>` | a client's call's wait for its answer, by kind | `tmpfs` | each kind's | `crates/hex-client/src/linux.rs`, `ANSWERS` |
| `netcheck.settles`, `netcheck.answers` | a fresh namespace's IPv6 settle; a dump's read | `owner` | the network check's | `crates/hex-runtime/src/linux/oci/netcheck.rs` |
| `netns.make` | the pool's refill thread's make of a network namespace (no node wait; the census's timing point) | `tmpfs` | the netns tests' patience | `crates/hex-node/src/netns/tests.rs`, `MAKE_RECORDED` |
| `lifecycle.exec` (from `seam-<arm>.json`) | a container's exec of `true`, to the program's end | `debug` | the conformance suites' patience, and every test that takes it | `crates/hex-api/src/conformance.rs`, `PROGRAM_END` |
| `client.answer.exec` and `lifecycle.exec` | the server's answer to a call over its socket (an exec's the slowest kind the socket's tests ask) and a program's end | `debug`: the larger, since one patience bounds both | the socket's tests' patience | `crates/hex-node/src/serve/tests.rs`, `RECORDED` |
| `seeds.hang` | a seed's answer to a create, which `seeds.wait` and `exec_seeds.answers` share by design | every release arm; `debug` | `CLONE_SLOWEST`, `CLONE_SLOWEST_DEBUG` | `crates/hex-core/src/seeds.rs`, `crates/hex-node/src/seed.rs` |
| `parked.ready` | a claim's wait for its parked init's first word on its sync pipe, from the claim's send: the init's ready on a root node, with no answer before it (the lead's (A) of 2026-11-09) | every release arm; `debug` | the claim's patience by build profile, MEASURING until a run with it in records it | `crates/hex-node/src/createpool.rs`, `CLAIMS` |
| `node.emptying`, `netns.make`, `store.turns`, `node.turns`, `client.answer.create`, `.start`, `.delete`, `.exec`, `node.listing`, `lifecycle.exec` | the facts the tests wait for that these rows record, or that one operation with a row makes within itself | `debug` | the tests' patiences for those facts | `crates/hex-testkit/src/waits.rs`, `crates/hex-api/src/conformance.rs` |
| `test.<crate>.<fact>` | a test's wait for a fact no recorded operation makes (a refill thread's, a checker's, a line delivered by the test's reader) | `suites` | that wait, measuring until its row has 1,000 samples | the test's own `Patience::measuring` (`hex_testkit::waits::until`) |

From run 3 the arm column is run 1's method: a release build's seed is the largest max of every release arm with a
row's 1,000 samples, in runs 1 and 3; a debug build's is the debug arm's (`Patience::by_profile`). Each is the census's
nanoseconds as recorded, never rounded below the max.

Until a run is recorded here each of those waits stays MEASURING in the code, naming this file's row. Where a row's n
is short of 1,000 (`client.answer.hello` is one a connection; `node.listing` a restart's entries; `serve.reads` only
the parts that waited), the run says so and the arm's count is what to raise.

## Runs

**Run 1** (2026-11-08T02:33Z, fedora (fedora-2), Linux 6.19.10, `waits-census-20261108T023331Z`; hex 9d627203806d51b4
and seam_oci 1d4679357a517ffe built at 91abf3a; N=10,000, CPUs 4-15 under the timed lock, no owner but the `owner`
arm; load 7.6 at start, 11.8 at end; unit hex-g5rec-census-20261108T023230Z). The waits measuring through it: none
armed (91abf3a: a measuring wait is never armed). Each seed is the row's slowest arm's max, so a node on disk or with
an owner is covered as well as the bar's tmpfs one. ms:

| wait | tmpfs max | disk max | owner max | n (most) | p99.9 (slowest arm) | seed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `client.answer.create` | 57.205 | 63.058 | 55.554 | 40,021 | 43.285 | 63.058 |
| `client.answer.delete` | 77.759 | 67.438 | 20.635 | 40,021 | 43.830 | 77.759 |
| `client.answer.exec` | 17.378 | 14.544 | 24.421 | 10,020 | 2.800 | 24.421 |
| `client.answer.inspect` | 15.403 | 26.205 | - | 10,020 | 0.166 | 26.205 |
| `client.answer.start` | 5.746 | 8.402 | 2.190 | 20,021 | 4.806 | 8.402 |
| `lifecycle.exec` | 17.588 | 14.718 | 24.550 | 10,000 | 3.347 | 24.550 |
| `netns.make` | 109.307 | 38.633 | 19.603 | 40,202 | 7.113 | 109.307 |
| `node.emptying` | 42.798 | 32.101 | (n 17) | 10,276 | 18.198 | 42.798 |
| `node.turns` | 77.426 | 67.195 | 53.981 | 80,042 | 41.320 | 77.426 |
| `seeds.hang` | 26.630 | 36.925 | 5.233 | 40,028 | 11.712 | 36.925 |
| `store.turns` | 5.935 | 7.825 | 2.373 | 120,645 | 0.573 | 7.825 |

- `seeds.hang` replaces `CLONE_SLOWEST`'s seed_path.md run 3 (1.918 ms, a clone timed alone): under the bar's creates
  the seeds' answers reach 36.9 ms, p99 about 4 ms, so a 3.8 ms bound would have taken about 1% of healthy seeds for
  hung. `seeds.wait` and the exec seeds' answers take the same record (their rows here: n 67, none).
- **Short, so still MEASURING** (n under 1,000, the count's knob to raise, not the threshold): `client.answer.hello`
  (n 1-2, one a connection), `client.answer.list` (100: `HEXBENCH_LISTS`), `client.answer.netns` (3-8),
  `client.answer.signal` and `.usage` (none: no phase calls them), `node.listing` (65-130: one restart a run),
  `serve.reads` (73-88: only parts that waited), `seeds.wait` (30-67). `netcheck.settles` and `.answers` have no row:
  the `owner` arm checked no kept namespace. The next run drives each.

**Run 2** (2026-11-08T02:45Z, `waits-census-20261108T024522Z`, at 9c9a15b): armed at run 1's seeds, it mostly failed,
and so taught the census's method. Three arms' nodes failed at start, an emptying of a slot holding a leftover `hex`
past its 85.6 ms bound (117 to 145 ms), and the tmpfs arm's deletes after a 10,000-box list past 155.5 ms; and none of
those answers was recorded, since a wait that runs out is no sample (Karn). So from run 3 the census measures every
wait unarmed (`patience::measure_all`, 946e06f), and a wait's seed is its build profile's (`Patience::by_profile`).
Its tmpfs arm's rows that were measuring then stand: `client.answer.hello` 1.023 ms (n 10,002), `.list` 329.722
(1,000), `.netns` 0.997 (10,009), `.signal` 1.180 (10,000), `.usage` 0.803 (10,000).

**Run 3** (2026-11-08T02:58Z, `waits-census-20261108T025856Z`, at 946e06f; every wait unarmed; four arms, the debug
one on debug builds beside the suites (gate 6, `cargo test --workspace`, looping on CPUs 16-31); load 27.9 at start,
the host shared). Its maxima, ms (n of the arm's max in brackets where it matters):

| wait | tmpfs | disk | owner | release seed (runs 1 and 3) | debug arm (seed) |
| --- | ---: | ---: | ---: | ---: | ---: |
| `client.answer.create` | 271.127 | 74.524 | 45.594 | 271.127 | 220.155 |
| `client.answer.delete` | 263.637 | 87.993 | 59.883 | 263.637 | 306.052 |
| `client.answer.exec` | 24.616 | 28.487 | 38.938 | 38.938 | 38.754 |
| `client.answer.hello` | 0.645 | 12.326 | (n 1) | 12.326 | 10.396 |
| `client.answer.inspect` | 18.316 | 26.047 | - | 26.205 (run 1) | 22.024 |
| `client.answer.list` | 348.309 | 446.346 | - | 446.346 | 1,439.697 |
| `client.answer.netns` | 4.511 | 1.401 | (n 4) | 4.511 | 8.048 |
| `client.answer.signal` | 1.423 | 2.996 | - | 2.996 | 0.273 |
| `client.answer.start` | 21.493 | 7.631 | 19.405 | 21.493 | 7.610 |
| `client.answer.usage` | 0.374 | 2.089 | - | 2.089 | 0.162 |
| `lifecycle.exec` | 24.636 | 28.750 | 39.172 | (tests: debug) | 39.131 |
| `netns.make` | 23.925 | 40.782 | (n 104) | (tests: debug) | 120.814 |
| `node.emptying` | 29.620 | 24.913 | (n 327) | 42.798 (run 1) | 127.753 |
| `node.listing` (an entry) | 0.001 | 0.008 | (n 65) | 0.008 | 0.069 |
| `node.turns` | 270.803 | 87.847 | 59.851 | 270.803 | 305.809 |
| `seeds.hang` | 187.044 | 26.252 | 59.552 | 187.044 | 100.815 |
| `store.turns` | 13.220 | 4.324 | 15.209 | 15.209 | 54.401 |
| `netcheck.answers` | - | - | 2.093 (160,192) | 2.093 | none (the debug arm keeps no namespaces) |

- Seeded at this run (by build profile; the tests' patiences from the debug arm), each at the census's nanoseconds:
  the table's 3 decimals hide some, `node.listing`'s 0.008 being 8,268 ns an entry and its debug 0.069, 68,562 ns.
  Run 2's measuring rows stand under run 3's, so they raise no max. Still MEASURING, short of 1,000: `seeds.wait` (45
  to 63 an arm: a create rarely waits for a seed; it shares the clone's record by design), `serve.reads` (307 to 623:
  only parts that waited), `netcheck.settles` (4: one a checker thread, a process's first).
- **The suites beside the debug arm didn't load it as meant.** Every loop of `cargo test --workspace` (646 of them)
  ended within about 2 s at `node_socket`'s HEX-RT-11 or 12, "no exit reported after" `Duration::MAX`: the
  conformance suites' `exit_of` takes a measuring patience's absent deadline for one already passed, so with
  `PROGRAM_END` measuring the case failed at its first look; and the launcher logged `date`'s exit status, not
  cargo's, so its "suites exit 0" says nothing. The debug arm ran beside cargo starting a dozen test binaries every
  2 s, not beside the whole suites, so its rows are what a lighter load gives; the next run beside the suites should
  run them through (`--no-fail-fast`), each loop's own exit recorded.
- **Convergence** (the lead, 2026-11-08): the census stops when a whole run sets no new max for any wait; each row
  says the run it converged at. Run 3 set a new max for most rows (run 1's were armed), so none has converged yet:
  the next run follows at these seeds' tree.
