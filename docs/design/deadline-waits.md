# Decision (2026-11-12): a condition variable whose timed waits end at their deadlines on macOS too (`hex-wait`)

Serving AC-T7.4.1 (Hex's lead's brief `fix-macos-deadline-waits`). ROOT's macOS gate went red at cb166de1 on
hex-client's queued sender: capped at 25 ms, it was refused at 95.1 and 98.6 ms (the waits census on 75694786), while
every cap bounded by poll(2) answered in 25.1 to 26.1 ms. Status: decided and built in `crates/hex-wait`.

Sources: XNU at `xnu-12377.121.6` and its `bsd/pthread` shims, Libc at `Libc-1752.120.2`, Rust's std (the 1.94.1
and nightly-2026-09-05 sources, which agree and bracket the pinned 1.99.1), and the measurements in
`bench/competitive/probes/deadline_waits.md` (harness `deadline_waits.c` beside it).

## Why std's timed waits are late on macOS

- std's `Condvar::wait_timeout` on Apple is `pthread_cond_timedwait_relative_np`
  (`library/std/src/sys/pal/unix/sync/condvar.rs`). XNU's psynch wait gives it a user timeout urgency
  (`bsd/pthread/pthread_shims.c:239,244`: `TIMEOUT_URGENCY_USER_NORMAL`).
- A user-urgency timer's leeway follows the waiting thread's latency QoS tier (`osfmk/kern/timer_call.c:1859-1915`,
  `timer_compute_leeway`): the interval scaled by the tier, up to the tier's cap. macOS's tiers
  (`osfmk/arm/arm_timer.c:292-294`) are tier 0 a 1/8 of the wait up to 1 ms, tier 1 a 1/4 up to 5 ms, tier 2 a half
  up to 20 ms, and tier 3 four times the wait up to 75 ms. The leeway then grows by up to 100% with the user-idle
  level (`timer_call.c:1937`).
- The QoS classes map to tiers in `osfmk/kern/thread_policy.c:117-123`: user-interactive is tier 0; user-initiated
  and the default are tier 1; utility, background and maintenance are tier 3.
- The kiln runner runs its commands at utility QoS (`thread_qos=0x11` on both Macs below). So a timed wait there
  is late by up to min(4x the wait, 75 ms), and by up to 150 ms while nobody is at the machine. The probe's
  `cond_rel` rows show exactly this.
- Mach semaphores take the same urgency (`osfmk/kern/sync_sema.c:694`). So do `dispatch_semaphore_wait`, which is
  std's `thread::park_timeout` (`library/std/src/sys/sync/thread_parking/darwin.rs`) and so mpsc's `recv_timeout`,
  and `nanosleep` (Libc `gen/nanosleep.c:104`, `__semwait_signal`), which is std's `thread::sleep`.
- A kqueue's own wait, which is kevent's and poll's timeout, sleeps by `assert_wait_deadline`
  (`bsd/kern/kern_event.c:8038`). That is a system urgency (`osfmk/kern/sched_prim.c:1342`), timed with the kernel's
  parameters: an 1/8 of the wait up to 1 ms (`arm_timer.c:283,288`), 2 ms while idle. A ulock, which is
  `os_sync_wait_on_address`, waits the same way (`bsd/kern/sys_ulock.c:755`, `osfmk/kern/waitq.c:2309`).
- An `EVFILT_TIMER` marked `NOTE_CRITICAL` is a critical user timer (`kern_event.c:1640`).
  `timer_compute_leeway` gives it the real-time parameters, a shift of 0 and a cap of 0 (`timer_call.c:1868-1869`,
  `arm_timer.c:281,286`): no leeway at all.
- Linux: std's `Condvar` is a futex wait to an absolute `CLOCK_MONOTONIC` deadline, late by the thread's timer slack
  alone (100 µs on adacachy2's kernel). Windows wasn't measured here.

## What was measured

Each run was the kiln runner's command at utility QoS, n = 100 per cell: 75694786 (Mac17,6, macOS 26.5.2, its user
at it, load 7 to 12) and 2768d0d5 (Mac16,9, macOS 26.2, nobody at it, load 1 to 3). Each cell is the overshoot in
µs, p50 / p99 / max.

| Mechanism | 75694786, 1 ms | 5 ms | 25 ms | 100 ms | 2768d0d5, 1 ms | 5 ms | 25 ms | 100 ms |
|---|---|---|---|---|---|---|---|---|
| std `Condvar` (`pthread_cond_timedwait_relative_np`) | 2741 / 4055 / 4103 | 10884 / 20025 / 20142 | 62359 / 75005 / 77354 | 63477 / 74824 / 74918 | 5205 / 8022 / 8079 | 29966 / 40024 / 40029 | 56817 / 150014 / 150020 | 67454 / 150017 / 150019 |
| `dispatch_semaphore_wait` (std `park_timeout`) | 3118 / 4762 / 5158 | 11471 / 20016 / 21029 | 55999 / 74981 / 74982 | 62608 / 74474 / 74570 | 5341 / 8017 / 8058 | 30571 / 40019 / 40032 | 47156 / 149302 / 150015 | 85132 / 150015 / 150016 |
| `nanosleep` (std `sleep`) | 2854 / 4050 / 5539 | 12694 / 20059 / 21672 | 58365 / 74802 / 74862 | 63523 / 74979 / 74998 | 4220 / 8019 / 8022 | 29589 / 40013 / 40072 | 25786 / 148799 / 150009 | 50739 / 150013 / 150016 |
| `kevent` timeout, `EVFILT_USER` to wake | 133 / 564 / 621 | 632 / 2630 / 3134 | 911 / 2979 / 4944 | 752 / 1017 / 1018 | 262 / 542 / 599 | 1261 / 1318 / 1850 | 2010 / 2031 / 2145 | 2009 / 2017 / 2030 |
| `poll` on a pipe | 139 / 1010 / 1411 | 637 / 1388 / 3419 | 1015 / 2003 / 2022 | 729 / 1021 / 1031 | 266 / 322 / 755 | 1265 / 1328 / 1471 | 2013 / 2054 / 2336 | 2014 / 2021 / 2021 |
| `os_sync_wait_on_address_with_timeout` | 135 / 562 / 571 | 635 / 2089 / 2631 | 1008 / 1606 / 2230 | 741 / 1033 / 1055 | 263 / 321 / 328 | 1265 / 1335 / 1525 | 2011 / 2105 / 2109 | 2012 / 2018 / 2020 |
| `EVFILT_TIMER` `NOTE_CRITICAL`, `EVFILT_USER` to wake | 18 / 906 / 1255 | 20 / 1803 / 2743 | 22 / 1043 / 1203 | 20 / 47 / 76 | 20 / 80 / 1572 | 20 / 147 / 247 | 24 / 535 / 786 | 25 / 66 / 90 |

- **At 0.1 ms**, `poll` rounds the wait up to a whole millisecond (p50 1035 and 1164 µs), and std's waits come
  406 and 809 µs late. The `NOTE_CRITICAL` timer comes 17 and 18 µs late, `kevent` 19 and 35, `os_sync` 20 and 37.
- **Wake latency**, for a wait woken halfway to its deadline, is the same class for every mechanism. On a quiet
  host at 100 ms, the p50 was 8.6 to 14.8 µs on 75694786 and 11.4 to 13.8 µs on 2768d0d5; `nanosleep`'s signal took
  23.6 to 23.8. Under 75694786's load, every mechanism's p99 rose alike to 0.06 to 2.9 ms: the threads' scheduling,
  not the mechanism.
- **A waiter's own kqueue** (kqueue(2), its user event added, then closed) costs 0.71 µs p50 and 2.33 µs p99 on
  75694786, and 0.50 µs and 0.71 µs on 2768d0d5. A pipe costs 0.88 µs and 0.67 µs p50.
- **Linux** (adacachy2, Ryzen 9 9950X3D2, Linux 7.3.3-cachyos, load 6 to 10): the futex wait is late by 101 to
  106 µs p50 and at most 118 µs at every timeout, its timer slack. The defect isn't there, so Linux keeps std's
  `Condvar`.

## Alternatives

1. **std's `Condvar`** has the defect: rejected on macOS.
2. **A condition variable on std's thread parking** (`dispatch_semaphore_wait`) has the same urgency and the same
   leeway: rejected.
3. **`nanosleep`** is the same, and only a signal wakes it: rejected.
4. **`kevent`'s timeout** honours a deadline to a millisecond (system urgency), with p50s of 0.13 to 2 ms: rejected
   for the `NOTE_CRITICAL` timer's p50, 30 to 100 times better at the same cost.
5. **`poll` on a pipe** is like `kevent`'s timeout, but rounds to whole milliseconds and takes two descriptors.
   Hex waits on sub-millisecond patiences: the testkit's `node.listing` entry is 137 µs. Rejected.
6. **`os_sync_wait_on_address_with_timeout`** has system urgency and needs no descriptor. It came only in macOS
   14.4, so it needs weak linking, and it needs libc's FFI anyway: rejected for the timer's precision.
7. **A one-shot `EVFILT_TIMER` marked `NOTE_CRITICAL`** on the waiter's kqueue, with an `EVFILT_USER` event to wake
   it, gets no leeway. Its p50 is 17 to 25 µs at every timeout; its max, 0.08 to 2.7 ms, is the load's scheduling.
   **Chosen.** It is the timer Hex's macOS box watcher and its guest-cost clock already wait on
   (`hex_runtime::macos::sys::Kqueue::wait`; `docs/research/runtime/native-platforms.md`).
8. **Raising the waiting thread's latency QoS** changes the caller's thread for all its timers and can't escape a
   task's clamp, while hex-client runs on its callers' threads: rejected, not measured.
9. **A timer thread notifying a condition variable at its waiters' deadlines** puts a thread beside every client,
   and its `notify_all` wakes other waiters spuriously: rejected.

## The choice

- **`crates/hex-wait`** holds `Condvar`, `Mutex`, `MutexGuard`, `WaitTimeoutResult` and `Failed`, with std's API
  but for a wait's failure. On Linux and Windows, `Mutex` and `MutexGuard` are std's and `Condvar` is std's with its
  failures typed. On macOS, `Mutex` is std's with a guard that knows it, so a wait can let it go and take it again.
- **The macOS `Condvar`** keeps a queue of its waiters, oldest first, each with a kqueue of its own made for that
  wait. A notification takes a waiter from the queue and triggers its user event; `notify_one` takes the oldest. A
  waiter's deadline is the timer it arms as it blocks. The queue is the one record of who was notified: a waiter
  gone from it when it leaves was chosen. The kqueue is closed with its waiter, and its timer with it. A macOS wait
  never wakes spuriously, and a notification that chose a waiter whose wait failed goes to the next waiter.
- **Why a crate of its own:** hex-api, the lowest of Hex's crates, has a site (its simulator's `Proc` wait). The
  primitive is platform code, not part of the seam. hex-afunix is the precedent of a small platform crate under
  hex-api.
- **Why a kqueue per wait, not per thread:** a kqueue costs 0.5 to 0.7 µs to make, beside a wake's 8 to 12 µs. No
  descriptor is held by a thread that isn't waiting, and nothing outlives a wait: no stale event, and no fork hazard.
  XNU makes every kqueue close-on-exec and close-on-fork (`kern_event.c:3066`).
- **Bounds:** the queue holds at most one entry for each thread in one of its waits, a thread waiting in one at a
  time. A waiting thread holds one kqueue descriptor while it waits. A push reserves its room first, so running out
  of memory is a typed refusal.
- **Failures** are `Failed::Poisoned` (the caller's mutex) and `Failed::Os` (kqueue(2) or kevent(2) refused:
  `EMFILE`, `ENFILE`, `ENOMEM`). Each site maps them to its crate's typed refusal: hex-api's `Refusal::Broken` and
  `Refusal::Os`, hex-quic's `Refused::Ended` and `Refused::Os`, `io::Error`, and the disk probe's
  `ProbeError::Poisoned` and `ProbeError::Wait`.
- **Energy:** a `NOTE_CRITICAL` timer gives up coalescing for its one firing, and it fires only when a deadline is
  reached: a refusal, or a period's end.
- **Unsafe:** `crates/hex-wait/src/macos/sys.rs`, named in CLAUDE.md §12.

## The channel and the timer (second step, 2026-11-12)

std's other timed waits take the same leeway on macOS: an mpsc receive with a timeout parks its thread
(`thread::park_timeout`, a dispatch semaphore, `darwin.rs`), and `thread::sleep` is `nanosleep`. Both wait on Mach
semaphores with user urgency (`sync_sema.c:694`; `gen/nanosleep.c:104`). Their rows in the table above are
`dispatch_semaphore_wait` and `nanosleep`. The model is the condition variable's.

- **`hex_wait::mpsc`** is a bounded channel, std's `sync_channel` API but for its failures. It is one implementation
  on every platform: a queue under hex-wait's `Mutex`, whose receiver waits on hex-wait's `Condvar`. So it is std's
  condition variable on Linux and Windows, and the waiter's kqueue on macOS. Its room is reserved when it is made,
  so a send never allocates; making one may be refused (`Failed::Os`, out of memory). A receive's wait may fail as
  hex-wait's waits may: `RecvTimeoutError::Failed` and `RecvError::Failed`. A bound of 0 is a rendezvous, taking a
  message only while its receiver waits. What the receiver hadn't taken is dropped once it goes, outside the lock.
- **`hex_wait::rest_until`** waits to a deadline. On macOS it uses a kqueue of its own and the same `NOTE_CRITICAL`
  timer, with nothing to wake it. Elsewhere it is std's `park_timeout`, looped to the deadline. It serves the waits
  that have no event to wait on: the conformance cases poll a box's state, resting as long as the last read took.
- **Bounds:** a channel holds at most its bound, one receiver and its counted senders. A rest holds one kqueue
  descriptor while it waits.

## Sites

- **Moved (12):** hex-api `sim.rs` (a simulated program's end); hex-client `calls.rs` (the send lock, and a
  `Proc`'s end); hex-quic `conn.rs` (a connection's change, its wait call alone) and `driver.rs` (an accepted
  connection); hex-node `serve.rs` (a connection's drain, and a stopping server's remote ends), `turns.rs`,
  `scheduler/stopping.rs` and `disk_probe.rs`; hex-runtime `run.rs` (macOS and Windows) and `guest/image.rs`.
- **Left (4):** the Linux-only hex-node `createpool.rs`, `seed.rs` and `supervise/run.rs`, and the Windows-only
  hex-runtime `windows/wmi.rs`.

The second step, std's channel receives and timed parks:

- **Moved:**
  - hex-client `calls.rs`: a call's answer under its caller's cap, on its mailbox;
  - hex-quic `driver.rs` and `lib.rs`: a dial's answer by its deadline;
  - hex-node `recorded.rs`: the charge learner's window, a wait it ends early when the node goes;
  - hex-api `conformance.rs`: `rest`, the pause of `exit_of`'s, `gone_within`'s and differential `until`'s reads of a
    fact, each under its patience;
  - two tests that bound a wait for a fact: hex-node `store.rs`'s 31 writes, and `store/pool/family_preparation.rs`'s
    callback.
- **Left:**
  - the Linux-only hex-node `createpool.rs:1603`;
  - the Windows-only hex-runtime `windows/oci/launch.rs:150` and `windows/mod.rs:3109,3270`;
  - hex-cli `selftest.rs:368,410`: a box member's holds, of its memory until a bound's kill lands, and while a
    watcher is measured. They are least times, not deadlines: a late wake only holds longer.
  - two tests' poll pauses, each under a bound the clock checks every round: hex-node `disk_probe.rs:280` (a 5 ms
    pause between level reads) and hex-runtime `macos/managed/remote/controls.rs:265` (a 10 ms pause in a test's
    nemesis).

## The test

- **The mechanism.** hex-wait's `macos::tests::ac_t7_4_1_a_deadline_wait_arms_a_critical_one_shot_timer_at_its_deadline`
  reads back from the kernel what a deadline wait armed while it waits (libproc's `PROC_PIDFDKQUEUE_EXTINFO`, as
  `lskq(1)` reads a kqueue): one `EVFILT_TIMER`, `EV_ONESHOT`, its saved flags `NOTE_NSECONDS | NOTE_CRITICAL`,
  armed for what was left of the wait. std's condition variable arms none: the test was red against it, and holds it
  so beside hex-wait's.
- **The client's deadline tests** (macOS): `ac_t7_4_1_a_queued_sender_is_refused_by_its_deadline` queues a delete
  behind the held sender, and `ac_t7_4_1_a_call_waiting_for_its_answer_is_refused_by_its_deadline` holds the
  reader's turn as another call's would be, so a delete waits for its answer on its mailbox. Each expects its refusal
  the absolute deadline's, while the sender or the turn is still held, so the deadline is its one cause, and never
  before the deadline.
- **No bound on lateness** (the waits audit, ROOT's ruling of 2026-11-13). Each refusal's lateness, and a poll(2)
  wait's beside it on the same thread (none of hex-wait's, the kernel's own leeway whatever the QoS), go to the waits
  census as `test.hex_client.budget.queued_delete_late`, `mailbox_late` and `precise_late`, held to nothing. Under
  load a refusal carries its thread's wait for a core, which no bound tells from the wait's own: ROOT saw 5 of 15 runs
  come 6.23 to 9.00 ms late at load 18.5 on 18 cores at af9131d4. Until 2026-11-13 the tests held each refusal to
  twice the slowest precise wait recorded or seen, seeded from the timer's idle record of 2.743 ms: a chosen multiple
  and a record seed, which failed a healthy run whenever a refusal came later than every precise wait beside it
  (6.34 ms against 4.57 ms at 902c90a2 on the Mac Studio, goal 12's r298).
- What shows hex-wait's waits come on time is the probe's record above, and the client tests' refusals measured on
  std's: its `Condvar`'s 70.07 ms late and its mpsc's 44.00 ms late (at 36864a83 on 75694786), and with std's
  condition variable and channel back in their places (a mutation) 24 to 150 ms late at load 32.6 to 35.0, where
  hex-wait's came at most 8.68 ms late at load 33.8 to 39.5 beside 18 spinners at nice 0.
- hex-wait's own tests hold its semantics on every platform: the condition variable's, the channel's (order,
  fullness, a timeout, disconnection, a rendezvous, what a gone receiver left) and the rest's.

## What would falsify it

- A macOS whose std condition variable waits come within the platform's timer precision at utility QoS: the
  probe's `cond_rel` rows would say so, and hex-wait's macOS path would be std's again.
- A `NOTE_CRITICAL` timer that is coalesced: the probe's `ktimer` rows would show it, and the census's rows of the
  client's deadline tests would show their refusals later than the precise waits beside them.
- A kqueue that costs as much to make as a wake takes (now 0.5 to 0.7 µs against 8 to 12 µs).

## Open

- **Windows wasn't measured here.** Its waits are std's, and the two client tests run on macOS alone until it is.
  This is ROOT's plan for jupiter:
  1. Port `deadline_waits.c` to Windows with these mechanisms:
     - std's `Condvar` (`SleepConditionVariableSRW`);
     - std's `park_timeout` and `sleep`;
     - `WaitForSingleObject` on an event;
     - a high-resolution waitable timer (`CreateWaitableTimerExW` with `CREATE_WAITABLE_TIMER_HIGH_RESOLUTION`),
       waited with `WaitForMultipleObjects` beside an event that wakes it.

     Use the same timeouts (0.1, 1, 5, 25 and 100 ms), n = 100 and the two arms. Print the system timer's
     resolution before and after.
  2. Run it as the kiln runner's command on jupiter (x86_64), as on the Macs, and on an arm64 Windows host if there
     is one.
  3. If std's waits come within the high-resolution timer's precision, turn both client tests on for Windows. If
     they come late by the system timer's resolution, choose a Windows wait by this record's method before moving
     Windows's sites, including the three left above.
- **hex-runtime's macOS `Kqueue`** arms the same timer, so one could be built on the other.
- **QoS tiers other than utility** were read from XNU's source, not measured: the probe ran as the runner's command.
