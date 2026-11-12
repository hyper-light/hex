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
| `client.answer.<kind>` | a client's call's wait for its answer, by kind; a call a box's class times (a create, a start, a delete, an exec...), by kind and class, `client.answer.<kind>.<class>` from 2026-11-11 (AC-T7.4.1), the rows before it seeding `process` and `oci` | `tmpfs` | each kind's, and class's | `crates/hex-client/src/linux.rs`, `ANSWERS` and `CLASS_ANSWERS` |
| `netcheck.settles`, `netcheck.answers` | a fresh namespace's IPv6 settle; a dump's read | `owner` | the network check's | `crates/hex-runtime/src/linux/oci/netcheck.rs` |
| `netns.make` | the pool's refill thread's make of a network namespace (no node wait; the census's timing point) | `tmpfs` | the netns tests' patience | `crates/hex-node/src/netns/tests.rs`, `MAKE_RECORDED` |
| `lifecycle.exec` (from `seam-<arm>.json`) | a container's exec of `true`, to the program's end | `debug` | a test's own child's end; the conformance suites' Linux seed is the slower first-run record above it (`HostClass::Linux`, 957 ms, merged 2026-11-09) | `crates/hex-testkit/src/waits.rs`, `PROGRAM`; `crates/hex-api/src/conformance.rs` |
| `client.answer.exec` and `lifecycle.exec` | the server's answer to a call over its socket (an exec's the slowest kind the socket's tests ask) and a program's end | `debug`: the larger, since one patience bounds both | the socket's tests' patience | `crates/hex-node/src/serve/tests.rs`, `RECORDED` |
| `seeds.hang` | a seed's answer to a create, which `seeds.wait` and `exec_seeds.answers` share by design | every release arm; `debug` | `CLONE_SLOWEST`, `CLONE_SLOWEST_DEBUG` | `crates/hex-core/src/seeds.rs`, `crates/hex-node/src/seed.rs` |
| `parked.ready` | a claim's wait for its parked init's first word on its sync pipe, from the claim's send: the init's ready on a root node, with no answer before it (the lead's (A) of 2026-11-09) | every release arm; `debug` and `suites` | the claim's patience by build profile, from run 5 | `crates/hex-node/src/createpool.rs`, `CLAIMS` |
| `node.emptying`, `netns.make`, `store.turns`, `node.turns`, `client.answer.create`, `.start`, `.delete`, `.exec`, `node.listing`, `lifecycle.exec` | the facts the tests wait for that these rows record, or that one operation with a row makes within itself | `debug` | the tests' patiences for those facts | `crates/hex-testkit/src/waits.rs`, `crates/hex-api/src/conformance.rs` |
| `test.<crate>.<fact>` | a test's wait for a fact no recorded operation makes (a refill thread's, a checker's, a line delivered by the test's reader) | `suites` | that wait, measuring until its row has 1,000 samples | the test's own `Patience::measuring` (`hex_testkit::waits::until`) |

From run 3 the arm column is run 1's method: a release build's seed is the largest max of every release arm with a
row's 1,000 samples, in runs 1 and 3; a debug build's is the debug arm's (`Patience::by_profile`). From run 5 a debug
build's is the largest answer of its debug-build arms, the `debug` arm and the `suites` (whose waits run armed, under
the gates' load), for every wait seeded from the same record, the tests' among them, and never under a bound one ran
out at: a row seeds once it has 1,000 answers or ran out, and its answers short of 1,000 count as seen. Each is the
census's nanoseconds as recorded, never rounded below the max.

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

**Run 4** (2026-11-09T16:18Z, adacachy2, the record host in fedora's place: AMD Ryzen 9 9950X3D2, 32 CPUs, Linux
7.3.3-1-cachyos; `waits-census-20261109T161828Z`; hex 7f4ae81d68cc7fe8 and seam_oci 7ae5e851836f98d6 built at 50851ad9;
four arms on CPUs 4-15, every wait in the node's arms unarmed, and the suites arm beside them on CPUs 16-31, 79 loops;
load 18.4 at start, 9.8 at end; unit hex-lead-census-20261109T161736Z). The release arms, ms (n where under 1,000):

| row | tmpfs | disk | owner | release seed |
| --- | ---: | ---: | ---: | ---: |
| `client.answer.create` | 43.655 | 41.745 | 17.094 | 43.655 |
| `client.answer.delete` | 20.180 | 35.715 | 18.188 | 35.715 |
| `client.answer.exec` | 16.286 | 17.331 | 17.237 | 17.331 |
| `client.answer.hello` | 0.483 | 1.177 | (n 1) | 1.177 |
| `client.answer.inspect` | 10.914 | 7.297 | - | 10.914 |
| `client.answer.list` | 386.711 | 294.351 | - | 386.711 |
| `client.answer.netns` | 0.715 | 0.306 | (n 4) | 0.715 |
| `client.answer.signal` | 0.583 | 0.058 | - | 0.583 |
| `client.answer.start` | 7.927 | 2.056 | 11.264 | 11.264 |
| `client.answer.usage` | 0.757 | 0.119 | - | 0.757 |
| `netcheck.answers` | - | - | 0.454 | 0.454 |
| `netns.make` | 57.557 | 73.884 | (n 98) | (tests: debug) |
| `node.emptying` | 19.771 | 15.725 | (n 875) | 19.771 |
| `node.listing` (an entry) | 0.001 | 0.007 | (n 65) | 0.007 |
| `node.turns` | 43.627 | 41.704 | 18.168 | 43.627 |
| `seeds.hang` | 29.565 | 17.953 | 18.003 | 29.565 |
| `store.turns` | 3.136 | 1.660 | 1.477 | 3.136 |

- Seeded at this run: the release build's waits, each the largest max of the release arms with a row's 1,000 samples,
  at the census's nanoseconds (the table's 3 decimals hide some: `node.listing`'s 0.007 is 7,043 ns an entry). They
  replace runs 1 and 3's, fedora's. Still MEASURING, short of 1,000: `seeds.wait` (40 to 69 an arm; it shares the
  clone's record by design), `serve.reads` (921 and 931), `netcheck.settles` (4).
- **Its debug rows seed nothing.** The suites arm's waits were armed (the tests' census disarms none), so a wait that
  ran out was no sample and each row's max is cut at its bound: `node_socket`'s create answers ran out at 440 ms,
  twice run 3's debug `client.answer.create`, in 27 of 79 loops, while the suites arm's row has 41.525 ms at most (of
  6,122). Seeded from it, that patience would fall to 83 ms. The debug seeds stay run 3's (fedora's) until run 5, in
  which a wait that runs out for a slow answer tells the census (`Patience::ran_out`, a line `name >nanos`) and a
  row's record is never under the bound it ran out at (`waits_census.py`'s `seed`), so a censored row's patience
  doubles each run until nothing runs out. A seed past its hang bound tells it nothing: a hung seed says nothing of an
  answer's time.
- **The suites**: 79 loops, every one exit 101. In every loop: `oci_reuse`'s 3 GB burn, waited on with the program-end
  record (a1be8761 gives each program its own fact's wait); `netcheck`'s measuring-dump test, holding the rule before
  d6bfb3f0 (e316cbf6); and `oci_box`'s wait for "the ended orphan to be reaped", 78.26 ms, the program-end record for
  a fact no program's end is. In 78: the shim's bystander, holding the tests' own pipes (7d8bdb1d). In 27:
  `node_socket`'s contract test, "none came in 440 ms".

**Run 5** (2026-11-09T18:40Z, adacachy2 as run 4, Linux 7.3.3-1-cachyos; `waits-census-20261109T184054Z`; hex
d8a0d4967935f6c1 and seam_oci c975f1df17e86d85 built at goal 5's tree ef259dbe, with g6's claim (A) and the censored
run-outs; four arms on CPUs 4-15, every wait in the node's arms unarmed, and the suites arm beside them on CPUs 16-31, 53
loops, its waits armed and each run-out told, `name >nanos`; load 10.1 at start, 13.8 at end). The debug-build arms, ms
(n where under 1,000; "ran out" the count and the largest bound):

| row | debug | suites | debug seed |
| --- | ---: | ---: | ---: |
| `client.answer.create` | 49.798 | 32.842 | 49.798 |
| `client.answer.delete` | 36.988 | 706.091 | 706.091 |
| `client.answer.exec` | 17.891 | 100.781 | 100.781 |
| `client.answer.hello` | 1.540 | 17.232 | 17.232 |
| `client.answer.inspect` | 12.008 | 4.210 | 12.008 |
| `client.answer.list` | 868.709 | 4.884 | 868.709 |
| `client.answer.netns` | 0.294 | 2.734 (n 137) | 2.734 |
| `client.answer.signal` | 0.173 | 3.952 (n 357; ran out 10, at 0.944) | 3.952 |
| `client.answer.start` | 16.121 | 13.277 (ran out 13, at 15.220) | 16.121 |
| `client.answer.usage` | 0.115 | 0.387 (n 447; ran out 2, at 0.325) | 0.387 |
| `lifecycle.exec` | 18.003 | - | 18.262 (`test.oci_box.program`'s, of 272) |
| `netns.make` | 50.825 | - | 50.825 |
| `node.emptying` | 14.875 | 904.705 (ran out 7, at 255.505) | 904.705 |
| `node.listing` (an entry) | 0.001 | 0.036 (ran out 4, at 0.137) | 0.137 |
| `node.turns` | 49.681 | 1719.743 | 1719.743 |
| `parked.ready` | 4.094 | 652.431 | 652.431 |
| `seeds.hang` | 19.132 | 652.938 | 652.938 |
| `store.turns` | 2.180 | 367.733 | 367.733 |

- Seeded at this run, at the census's nanoseconds: every debug build's wait, by run 5's rule above, the suites' answers
  deciding most of them (the debug arm runs one bench at a time; the suites run the gates' load). A seed shared by
  several waits takes theirs too: `seeds.hang` with the line's (`seeds.wait`, ran out 53 at 22.222 in the suites) and the
  exec seeds' (120.691); `lifecycle.exec` with the tests' program waits, `oci_box`'s program run at 18.262 above the
  debug arm's 18.003. `node.listing`'s seed is the bound run 3's ran out at, 137,124 ns an entry, so its patience
  doubles.
- **`parked.ready`** (the claim's wait, g6's (A)) is seeded by build profile: release 12.896 ms, the largest max of the
  release arms (tmpfs 12.896, disk 7.433, owner 3.022), debug 652.431 ms, the suites'.
- The release seeds stay run 4's. Still MEASURING: `serve.reads` (970 to 991 an arm), `netcheck.settles` (4), and
  `netcheck.answers`' debug build (no debug-build row).
- **The suites**: 53 loops, every one exit 101. In all 53: `oci_box`'s `oci_boxes_report_their_inits_exit_through_the_seam`,
  its delete refused "emptying a box: box swap-exec's init still runs 326.645366ms after SIGKILL", `node.emptying`'s
  debug patience under run 3's seed (127.753 ms; 904.705 ms now). In 15, `node_socket`'s contract test; in 12,
  `oci_spec_box`'s conformance, its HEX-RT-2 emptying the same, "256.71229ms after SIGKILL"; in 5, `oci_reuse`'s
  cgroup reuse within its owner; once each, `process_box`'s contract, `node`'s `a_listing_teaches_the_next_its_wait` and
  the init's `a_start_whose_reader_is_gone_is_abandoned_not_a_sigpipe`.
