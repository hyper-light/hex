# Learned waits: the slowest answer seen, the following wait, and the limits beside them

Source notes for decision 9 of `docs/design/runtime-architecture.md` (Ada's rules of 2026-11-06 and 2026-11-13: no
picked constant; no value measured on our machines baked in per profile, OS or build; "citing a number doesn't count
as derived; an estimator of what's seen does"; "Account for cold starts well in any such approach").

## The bound (2026-11-13)

A wait's bound is the slowest answer its node has seen, with no margin. Before its first answer, and after a run-out,
it follows its party: it lasts while the party is alive and advancing, refused, typed, at its death, a reset or a
stall of its own measured progress (`hex_core::patience::Follow`). No record seeds it, and no factor scales it.

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
  connection's state) ends it at the party's death or reset; its progress (a count that only rises while it works)
  ends it at a stall longer than any gap between advances this wait has measured, the same estimator, the largest
  seen, over the wait's own observations. Its answer comes in uncensored, so the learned bound rises to it.
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
| `seeds.hang` (hex-core `seeds`, hex-node `createpool`) | a seed serving a create | its pidfd (`Seeds::died`) | its CPU time | liveness; progress owed |
| `seeds.wait`, `exec_seeds.answers` (hex-node `seed`) | the busy seeds | their pidfds | a seed freed | yes |
| `parked.ready` (`createpool::CLAIMS`), `wait_parked` | a parked init | its sync pipe and claim socket ending | its CPU time | liveness; progress owed |
| `node.listing` | the listing | none needed (in process) | entries listed | owed |
| `node.turns`, `store.turns` | a turn's holder | its let-go | its work's progress | owed |
| `node.emptying` | a killed box | its cgroup | members gone | owed (`Duration::MAX` meanwhile) |
| `serve.reads` | a client reading a list | its connection | bytes read | liveness; progress owed |
| `client.answer.*`, `client.guest.answer.*`, `client.quic.answer.*`, `guest.stage` | the node agent or guest relay | the connection's end or reset | the peer's CPU time or bytes | liveness; progress owed |
| `netcheck.answers`, `netcheck.settles` | the kernel's rtnetlink | the socket | none | owed |
| `hex.image.boot`, `.stats`, `.stop` | the boot or stop thread, D231's run host | the run host's process | none | owed |
| the guest's first boot (`GuestRuntime::first_dial_bound`) | the guest's VM | its process's end | its CPU time | not following: records.tsv's `boot_ready`, a production R03 violation ("Owed", 1) |

## Owed (2026-11-13)

Three places still seed a wait from a record, the first in production. Each needs code that this change's one-host
checks (macOS, arm64) can't compile, so each is the Hex lead's, with its design:

1. **The guest's first boot reads records.tsv: a production R03 violation, the first to close.**
   `hex_client::guest::shards::RECORDS` takes `bench/native/guest/records.tsv` by `include_str!`; `recorded_boot`
   reads this host class's `boot_ready` row (with none, the slowest of any class's; where no class has one, refused
   "record one first"); and `hex_runtime::guest::first_dial_bound`, the larger of it and the slowest boot seen, makes
   the one finite deadline `managed::Budget::new` spends across a create's native launch and first relay dial (fifteen
   calls in books.rs, dedicated.rs, gpu_pool.rs, shared.rs and mod.rs), as `ShardsGuest::dial_before(port, None)`
   dials until it. The design, HEX-GUEST-33's liveness-sliced dial: before the first boot, and after one that ran out,
   the budget follows the guest's VM (`Budget::following(party)` beside `Budget::new(bound)`, its deadline an
   `Option<Instant>`). The VM's liveness is its process's end, an event on every OS (a pidfd on Linux,
   `EVFILT_PROC`/`NOTE_EXIT` on macOS, a process handle on Windows); its progress is its CPU time
   (`/proc/<pid>/stat`, `proc_pid_rusage`, `GetProcessTimes`), given to `Follow::look`. Shards'
   `connect_once_before(port, at)` takes a finite instant, so the dial is cut into slices at the follower's own
   instants: `Follow::stalls_after()` once the VM has shown a gap between advances, before that the next look its own
   events give (its exit, its console's bytes). The cleaner end is a Shards dial whose wait ends at the guest's answer
   or its VM's end (`Option<Instant>`, `None` for "until either"), with no slices; Shards is ROOT's to change. Then
   `RECORDS`, `recorded_boot`, `seed_in`, the "record one first" gate and `dial_bound` go, and records.tsv is evidence
   alone.
2. **hex-api's conformance suite** (compiled outside `#[cfg(test)]`): `conformance::PATIENCE`, the program-end record
   of this host's class (`HostClass::slowest_recorded_end`: Linux 957 ms, macOS 939 ms, Windows 6,239 ms), and `ENDS`,
   a `LearnedWait` seeded from it. hex-api can't name `hex_core::patience` (hex-core depends on hex-api), so the design
   moves the pure core down: `crates/hex-core/src/patience.rs` and its tests to `hex_api::patience`, with
   `pub use hex_api::patience;` in hex-core, so every `hex_core::patience` path stands, and `test-seeds` with it.
   `ENDS` becomes `Patience::measuring("conformance.ends")`: a case's wait for a program it expects to end follows the
   program until the first end (`Proc::try_wait` looked at on its own events, the end the answer), then is bounded at
   the slowest end seen; `still_running()`, the negative watch, is the slowest end seen, or a typed refusal where none
   has been (the suite runs its ending cases before it). `PATIENCE`, `seed`, `seed_from`, `SLOWEST_RECORDED_END`,
   `slowest_recorded_end` and `LearnedWait` go, and their users outside the suite (about 45 calls, in hex-cli's Linux
   and Windows tests and in hex-node, hex-client and hex-bench) take a helper, `conformance::end_of(&mut proc)`,
   that waits as the suite does.
3. **The tests' own waits**: `Patience::seeded` seeds them from `hex_testkit::waits`' census constants (`PROGRAM` to
   `SEEDS`, adacachy2's debug run 5) under the lead's rule of 2026-11-08, and a dozen tests double a record
   (`node_socket.rs`'s `ANSWERED` and emptying, `oci_box.rs`'s namespaces, program end and creates, `oci_reuse.rs`'s
   kill, `native_firecracker_smoke.rs`'s emptying, `netns/tests.rs`, `serve/linux_tests.rs`, `macos/tests.rs`,
   `macos_process_box.rs`, `helper_daemon/tests.rs`). Made plainly `measuring`, the first answer in a test binary
   would bound every later wait for that fact in it, and a test fails at a run-out where production follows. The
   design: `hex_testkit::waits::until` and `within` take the fact's party (`impl FnMut() -> Party`: a `Proc`'s
   `try_wait`, a cgroup's populated count, a pid's CPU time) and follow it with `Follow` before the first answer and
   after a run-out; a run-out is told to the census and never fails a test, which fails at its party's end or stall.
   Then the constants, the doubles, `seeded` and `test-seeds` go: eighteen seeded statics in six Linux-only files
   (`oci_box.rs`, `oci_reuse.rs`, `node_socket.rs`, `oci_spec_box.rs`, `serve/linux_tests.rs`, `netcheck/tests.rs`)
   and the rest in hex-client, hex-node, hex-runtime and hex-testkit.

A finding beside them, the lead's to weigh: where a learned bound ends a call (a client's call with no caller's
deadline, `ReplyDeadline::combine`: "the learned wait is the call's bound"; a listing, a seed's line, `serve.reads`),
the call is refused at the slowest answer seen. With no seed and no margin, the `n`-th answer is the slowest yet with
chance `1 / n` where answers are exchangeable, so a fresh process's second call of a kind is refused whenever its
answer is slower than the first's: half the time. The cost above, with `p = 0` measured, is least with no bound at
all: at the learned bound the wait would record its run-out and go on following its party, as `wait_past` already
does under a caller's deadline, refused only at the party's end, reset or stall.

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
