# Learned waits: the slowest answer seen, the following wait, and the limits beside them

Source notes for decision 9 of `docs/design/runtime-architecture.md` (Ada's rules of 2026-11-06 and 2026-11-13: no
picked constant; no value measured on our machines baked in per profile, OS or build; "citing a number doesn't count
as derived; an estimator of what's seen does"; "Account for cold starts well in any such approach").

## The bound (2026-11-13)

A wait's bound is the slowest answer its node has seen, with no margin. Before its first answer, and after a run-out,
it follows its party. At the node's local sites it follows the party by its life alone, refused, typed, only at the
party's death or a reset; a stall of the party's measured progress is judged only where that progress is continuous
work toward the answer at a scale the window can see (a VM's boot, below). No record seeds it, and no factor scales
it.

- **Least expected cost.** A bound `T` costs a call `p·T + (1 − p)·P(answer > T)·C`: its wait for an answer that never
  comes, at rate `p`, and a run-out of an answer that would have come, costing `C > 0`. Over an empirical distribution
  `P(answer > T)` is 0 at and past its largest sample and positive under it, so with `p = 0` the least-cost bounds are
  those at or past the largest answer, the least of them the largest answer itself, for any `C`. The census measures
  `p = 0` (every unarmed arm of runs 1 to 5 ended). The trade between how long a dead party holds a caller and how
  often a live one is taken for dead is a failure detector's quality of service: Chen, Toueg and Aguilera, "On the
  Quality of Service of Failure Detectors", IEEE Transactions on Computers 51(5):561-580, 2002.
- **Nothing past the largest answer is distribution-free.** Of `n + 1` exchangeable answers each is as likely to be
  the largest, so the next outruns the largest of `n` with chance at most `1 / (n + 1)`; the largest of `n` is the
  one-sided tolerance limit for any continuous distribution, covering a proportion `β` with confidence `1 − β^n`: S. S.
  Wilks, "Determination of Sample Sizes for Setting Tolerance Limits", Annals of Mathematical Statistics 12(1):91-96,
  1941. A bound past it rests on a model of the unseen tail, which Ada's rule refuses.
- **The following wait.** Where nothing has been seen (the cold start) or the tail ran past what was (a run-out), no
  seen value bounds the wait; what the wait sees as it runs does. The party's liveness (a pidfd, a process handle, a
  connection's state) ends it at the party's death or reset. Its answer comes in uncensored, so the learned bound
  rises to it.
- **A stall judged at the slowest gap seen refuses healthy parties.** The person's rule of 2026-11-13 makes a stall
  of the party's measured progress the typed refusal; the same record argument as the bound's says where such a stall
  can be judged at all. A party's gaps between advances are exchangeable, so the `k`-th gap is the longest yet with
  chance `1 / k`. A wait that refuses once a gap runs past the longest it has measured refuses at the first such
  record: over `n` gaps, with chance `1 − 1 / n` (90% at 10), each time from a party that advances again a moment
  later. Learned across waits, process-wide and only rising as W* is, the chance is still `1 / (N + 1)` a gap over all
  `N` seen, high early, and asking for more gaps before judging is a picked count. Judged over W*, the slowest
  answer, it fails wherever the party advances less often than W* is long, and it did, W* microseconds against
  milliseconds between advances: CPU time judged at µs scale took a killed box for stalled (`node.emptying`, HEX-RT-1:
  304 µs), and the seeds freed took a create in line for stalled (oci_reuse, "Exhausted create seeds"). The hang
  bound, judging no progress at all, killed a seed still answering ("a create seed did not answer within 1.16752ms",
  oci_box).
- **Life alone at the node's local sites.** So at `node.emptying`, the seeds' line (`seeds.wait`), `seeds.hang`, the
  exec seeds (`exec_seeds.answers`), `serve.reads` and `guest.stage`, past W* the wait follows its party's life alone,
  the run-out told. A wedged party that stays alive is ended by its caller's own deadline or its owner's stop, never by
  a guess at how long its next advance may take. Stall judgment stays only where progress is continuous work toward
  the answer at a scale the window can see: Shards' Part B for a VM's boot (no CPU over a whole boot-scale W*), for
  the image boot and the hosted boot once it is vendored. The tests' waits follow life alone too
  (`hex_testkit::waits::follow`: copy::tests' copier was refused at 44 µs of standing CPU time against a 42.9 µs
  window), and `hex_core::patience::Follow`, which judged a stall over W* or the longest gap a wait had measured, is
  removed with `Party::Alive` and `Stopped`: nothing in Hex judges a stall of measured progress.
- **RFC 6298's back-off, considered and dropped.** RFC 6298 §5.5 (sha256 of the rfc-editor.org text
  `f3a6d937c0a7653bd431fdbbdcd7920f4e9d9cdb6134fa9db7a74b90d215edf6`): "(5.5) The host MUST set RTO <- RTO * 2 ("back
  off the timer")." Its factor is a cited number, not an estimate of what is seen (Ada, 2026-11-06, as the Hex lead
  quotes it), and doubling's optimality (online bidding, Chrobak, Kenyon, Noga and Young, Algorithmica 50(4), 2008)
  holds for bids each paid in full, where a following wait pays only for the time its party actually takes.

## The sites the API serves (2026-11-13)

Each production wait is `Patience::measuring`; where it has no bound it follows its party. What each follows, and what
is wired:

| wait | party | liveness, as the shell reads it | progress count | wired |
| --- | --- | --- | --- | --- |
| `seeds.hang` (hex-core `seeds`, hex-node `createpool`) | a seed serving a create | its pidfd, polled with its socket (`createpool::wait_answer`) | none: a local site | liveness alone past W*, never taken for hung (`Seeds::past_hang`) |
| `seeds.wait`, `exec_seeds.answers` (hex-node `seed`) | the busy seeds | their pidfds; the line refused only when none can come | none: a local site | liveness alone past W* |
| `parked.ready` (`createpool::CLAIMS`), `wait_parked` | a parked init | its sync pipe and claim socket ending | its CPU time | liveness; progress owed |
| `node.listing` | the listing | none needed (in process) | entries listed | owed |
| `node.turns`, `store.turns` | a turn's holder | its let-go | its work's progress | owed |
| `node.emptying` | a killed box | its cgroup emptying | none: a local site | liveness alone past W* |
| `serve.reads` | a client reading a list | its connection | none: a local site | liveness alone past W* |
| `guest.stage` | the guest's relay | the connection's end or reset, its timeouts let go past W* | none: a local site | liveness alone past W* |
| `client.answer.*`, `client.guest.answer.*`, `client.quic.answer.*` | the node agent or guest relay | the connection's end or reset | the peer's CPU time or bytes | liveness; progress owed |
| `netcheck.answers`, `netcheck.settles` | the kernel's rtnetlink | the socket | none | owed |
| `hex.image.boot`, `.stats`, `.stop` | the boot or stop thread, D231's run host | the run host's process | none | owed |
| the guest's first boot (`GuestRuntime::first_dial_bound`) | the guest's VM | its process's end | its CPU time | not following: records.tsv's `boot_ready`, a production R03 violation ("Owed", 1) |

**What each local site's life is (2026-11-13).** Past W*, with the run-out told, each waits for its party's life
alone ("Life alone at the node's local sites", above). A seed past its hang bound is waited for until it answers or
its pidfd says it ended. A create in line waits until a seed is handed to it, refused only when no seed is up,
starting or asked for (`Seeds::none_can_come`). An exec waits until a seed frees or a slot opens. A list's part waits
until its client reads or its connection closes, and a stage until its relay answers or the connection ends or
resets.

## Owed (2026-11-13)

Three places seeded a wait from a record, the first in production; the second is done. Each needs code that this
change's one-host checks (macOS, arm64) can't compile, so each is the Hex lead's, with its design:

1. **The guest's first boot reads records.tsv: a production R03 violation, the first to close.**
   `hex_client::guest::shards::RECORDS` takes `bench/native/guest/records.tsv` by `include_str!`; `recorded_boot`
   reads this host class's `boot_ready` row (with none, the slowest of any class's; where no class has one, refused
   "record one first"); and `hex_runtime::guest::first_dial_bound`, the larger of it and the slowest boot seen, makes
   the one finite deadline `managed::Budget::new` spends across a create's native launch and first relay dial (fifteen
   calls in books.rs, dedicated.rs, gpu_pool.rs, shared.rs and mod.rs), as `ShardsGuest::dial_before(port, None)`
   dials until it. The design, HEX-GUEST-33's liveness-sliced dial: before the first boot, and after one that ran out,
   the budget follows the guest's VM (`Budget::following(party)` beside `Budget::new(bound)`, its deadline an
   `Option<Instant>`). The VM's liveness is its process's end, an event on every OS (a pidfd on Linux,
   `EVFILT_PROC`/`NOTE_EXIT` on macOS, a process handle on Windows). A stall of its boot is Shards' Part B's to judge
   (no CPU over a whole boot-scale W*), once vendored, not Hex's. Shards' `connect_once_before(port, at)` takes a
   finite instant, so the dial is cut into slices at the next look the VM's own events give (its exit, its console's
   bytes). The cleaner end is a Shards dial whose wait ends at the guest's answer
   or its VM's end (`Option<Instant>`, `None` for "until either"), with no slices; Shards is ROOT's to change. Then
   `RECORDS`, `recorded_boot`, `seed_in`, the "record one first" gate and `dial_bound` go, and records.tsv is evidence
   alone.
2. **hex-api's conformance suite** (compiled outside `#[cfg(test)]`), done 2026-11-13 (decision 9's follow-up 1;
   goal 12's macOS checks had failed hex_quic_20, HEX-RT-2 "still running after 939ms", macOS's record). The pure
   core moved down: `hex_api::patience`, with `pub use hex_api::patience;` in hex-core, so every `hex_core::patience`
   path stands, and `test-seeds` with it. `ENDS` is `Patience::measuring("conformance.ends")`, and a case's wait for a
   program's end follows the program to it (`conformance::end_of`: within W*, then by its own events until its end,
   the run-out told). The exit and removal waits read the box while the runtime under test lives
   (`Fixture::party`), and each differential reading follows that party: a double whose answers can't change has
   ended, and fails at once at the step that differs. `still_running()`, the negative watch, is the slowest end seen:
   a check that something doesn't happen needs a learned bound, and that is the one. `PATIENCE`, `seed`, `seed_from`,
   `SLOWEST_RECORDED_END`, `HostClass` and `LearnedWait` went, their users (about 45 calls, in hex-cli's tests and in
   hex-node, hex-client and hex-bench) taking `conformance::end_of`, and hex-bench's checker of the record's uses
   with them.
3. **The tests' own waits**: `Patience::seeded` seeds them from `hex_testkit::waits`' census constants (`PROGRAM` to
   `SEEDS`, adacachy2's debug run 5) under the lead's rule of 2026-11-08, and a dozen tests double a record
   (`node_socket.rs`'s `ANSWERED` and emptying, `oci_box.rs`'s namespaces, program end and creates, `oci_reuse.rs`'s
   kill, `native_firecracker_smoke.rs`'s emptying, `netns/tests.rs`, `serve/linux_tests.rs`, `macos/tests.rs`,
   `macos_process_box.rs`, `helper_daemon/tests.rs`). Made plainly `measuring`, the first answer in a test binary
   would bound every later wait for that fact in it, and a test fails at a run-out where production follows. The
   design: `hex_testkit::waits::until` and `within` take the fact's party (`impl FnMut() -> Party`: a `Proc`'s
   `try_wait`, a cgroup's populated count, a thread's life) and follow it by its life alone before the first answer
   and after a run-out; a run-out is told to the census and never fails a test, which fails only at its party's end.
   Then the constants, the doubles, `seeded` and `test-seeds` go: eighteen seeded statics in six Linux-only files
   (`oci_box.rs`, `oci_reuse.rs`, `node_socket.rs`, `oci_spec_box.rs`, `serve/linux_tests.rs`, `netcheck/tests.rs`)
   and the rest in hex-client, hex-node, hex-runtime and hex-testkit.

A finding beside them, the lead's to weigh: where a learned bound ended a call (a client's call with no caller's
deadline, `ReplyDeadline::combine`: "the learned wait is the call's bound"; a listing, a seed's line, `serve.reads`),
the call was refused at the slowest answer seen. With no seed and no margin, the `n`-th answer is the slowest yet with
chance `1 / n` where answers are exchangeable, so a fresh process's second call of a kind was refused whenever its
answer was slower than the first's: half the time. The cost above, with `p = 0` measured, is least with no bound at
all. Since 2026-11-13 (decision 9's (c), the person's ruling) none ends at it: each records its run-out there and goes
on following its party, as `wait_past` does under a caller's deadline. `combine` gives a call with no caller's deadline
no budget, and it follows its agent's connection; a listing goes on, in process; `serve.reads` follows its client by
the messages it takes; and the create seeds' line (`Seeds::claim`) and the exec seeds' (`SeedPool::acquire`) follow the
seeds freed. Each is refused only at its party's end, a reset, or a whole W* window with no advance. Where that progress
is no finer than the answer itself (a client taking one waiting part, a create first in line), the stall is judged one
whole window past the run-out.

The turns (`node.turns`, `store.turns`) do so since 2026-11-13: a hold past the slowest seen is told to the census and
the wait follows the holder, this process's own thread, to its let-go. Refused there, the store failed HEX-SOCK-6
(`store::tests::updates_from_many_threads_all_land`: eight threads changing one box's record 25 times each) on
adas-mac-studio-3.local (macOS arm64, load 14.6 on 14 cores) at 5038d770, `box a's change before this one was still
under way after 905.625µs`: with eight threads contending, each new slowest hold refused a waiter. A disk that stops
answering then holds its waiters as long as its writer, until the hold's progress is followed (the sites above).

Doubled bounds of the 2026-11-06 rule outside `Patience`, each a record and a factor in production code:
`hex_core::setup::FEATURE_QUERY_PATIENCE` (jupiter's 2,421 ms twice, `hex_runtime::windows::features`' bound),
`hex_core::watch`'s lateness seeds (an unrecorded model at twice the highest record), `hex_node::serve::windows`'
`close_wait` (twice the larger of `PEER_CLOSE_RECORD` and what it has seen), and, on the lead's line,
`precise_patience` (hex-client) and the charge driver's `guard` (hex-cli).

## The failure-bound rule (the lead, 2026-11-06, superseded 2026-11-13)

Before any sample on a node, a wait's bound is 2 × the slowest answer in that wait's recorded distribution for the
host class, from a named record; after, 2 × max(the record's slowest, the node's slowest), only rising. A wait that
ran out is no sample, nor a dead or hung seed. No caller failure where an alternative exists. Where no record
exists, the wait is MEASURING with the record to come named, and no value is picked. RFC 6298's estimator (the
first draft) and calibration reads (the second) were ruled out: the RFC's floor and constants are TCP's clocks', and
a calibration read leaves the first wait unbounded. Only its factor of two is gone; the rest stands.

Karn's point is from Karn and Partridge ("Improving Round-Trip Time Estimates in Reliable Transport Protocols",
SIGCOMM '87; RFC 6298 §3 states it for TCP): a sample is taken only from an answer whose wait is unambiguous.

## The records the seeds come from, checked in the files

- `bench/competitive/probes/seed_path.md`, run 3 (fedora, 2026-10-30T06:06:13Z, load 25.5 to 29.6): a seed's
  clone of an init, request to the init's first run, p50 0.234, p90 0.278, p99 1.050, max 1.918 ms, n=320. Run 2:
  request to child max 0.989 ms. Run 1's max of 15.778 ms is its first create's, timed from the seed's own start
  (the record says so), not a clone. `seeds::CLONE_SLOWEST` is run 3's 1.918 ms, the slowest clone recorded.
- `bench/competitive/probes/levers.md`, runs 1 and 2: a 400-byte record write and rename p99 1,037.7 µs on btrfs,
  893.8 on overlayfs, 42.1 on tmpfs; no fsync, no max. Not the store's durable write's distribution, so the store's
  wait is MEASURING.
- `bench/competitive/probes/input_census.md` (adacachy, 2026-11-06): sizes of `/proc/net` files and netlink dumps,
  no times. The network check's waits are MEASURING.
- No record times a box's emptying from its kill, a listing per entry, a turn's hold, a list part's drain or a
  socket call's answer by kind: each is MEASURING, its record to come named at the wait.

## Linux: pid_max, /proc/self/fd, RLIMIT_NOFILE

Documentation/admin-guide/sysctl/kernel.rst, "pid_max": the PID allocation wrap value; no task holds a pid at or
past it, so the threads a box's cgroups list are at most `pid_max` (read at runtime from
`/proc/sys/kernel/pid_max`). proc(5): `/proc/self/fd` holds one entry per open descriptor of the process.
getrlimit(2): `RLIMIT_NOFILE`, one more than the largest descriptor number a process may open.

## systemd: DefaultLimitNOFILE=

systemd-system.conf(5), "DefaultLimitNOFILE=": the default descriptor limit for services, 1024 soft and 524288 hard
since systemd v240 (the soft limit kept at 1024 for select(2)'s sake). The netns tests' "systemd's default" is
that soft half.

## Balls in bins: Dubhashi and Ranjan, "Balls and Bins: A Study in Negative Dependence"

Random Structures & Algorithms 13(2), 1998. The indicators that each bin is empty after independent throws are
negatively associated, so Chernoff-Hoeffding bounds apply to their sum. The store's shard test uses it: 256 ids
over 64 shards miss mu = 64·(63/64)^256 ≈ 1.14 shards on average, and P(misses ≥ 14) ≤ 2^-32 for a uniform hash.
