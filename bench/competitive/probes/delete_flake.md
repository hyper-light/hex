# The rootless delete flake: `cgroup.kill` misses a leaderless thread group

## The symptom

An `oci` box's `delete` once stayed stuck: 60 s after `cgroup.kill` the box's `cgroup.events` still read
`populated 1`, and `delete` refused its box by type (`crates/hex-node/src/node.rs`, `scrub`'s
`wait_empty` out of patience). It hit twice, rootless, under load: `oci_spec_box`'s `layered` box on
fedora 6.19 (gates at e2bf959, beside Focal's fleet) and on adacachy 7.2.9 at load 35-52 (step 2's gates
at 8b32738). `survivors()` reported "populated, but no process or thread listed" — the box held, but by
the time the 60 s walk ran, nothing was listed.

## The cause, proven on Linux 7.2.9

`delete_flake.c` (run in a delegated cgroup v2 subtree on adacachy, kernel `7.2.9-2-cachyos`) puts a
thread group into a cgroup whose **leader thread exits** (`pthread_exit`) while a non-leader worker
thread lives, then writes `cgroup.kill` and watches `cgroup.events`:

```
--- A: leader exited, non-leader thread alive
  cgroup.events  [populated 1 frozen 0 ]
  cgroup.procs   [1206561 ]
  cgroup.threads [1206562 ]
  after kill +0.0s events [populated 1 frozen 0 ]
  ... (every 0.5 s) ...
  after kill +4.5s events [populated 1 frozen 0 ]
--- A: after cgroup.kill
  cgroup.events  [populated 1 frozen 0 ]   # still populated; the worker 1206562 survives
  cgroup.procs   [1206561 ]
  cgroup.threads [1206562 ]
```

`cgroup.kill` signals thread-group **leaders**; the leader (tid 1206561) has already exited, so the
`SIGKILL` reaches a dead thread and the live non-leader worker (tid 1206562) never gets it. The worker
holds `cgroup.events populated 1` indefinitely — exactly `delete`'s 60 s wedge. The control, a normal
single-thread process, dies at once:

```
--- C: normal process before kill   cgroup.events [populated 1 ...]  procs [1213318] threads [1213318]
--- C: normal process after cgroup.kill   cgroup.events [populated 0 ...]  procs [] threads []
```

The two earlier "nothing listed" sightings are the same cause seen later in the wedge: by the time the
60 s refusal walked the subtree, the leaderless worker had been scheduled out of the listing or had
finally exited, but during the 60 s it held the box populated.

## In the kernel source

The probe is the evidence on the running kernel; the mechanism is in the source, below at mainline
Linux `v6.12` (the running `7.2.9-2-cachyos` ships no `cgroup.c`/`signal.c` to cite in place, and this
path has been stable for years — the probe confirms `v6.12`'s behavior holds on 7.2.9).

- `kernel/cgroup/cgroup.c:4008` `__cgroup_kill` walks the cgroup with `CSS_TASK_ITER_PROCS |
  CSS_TASK_ITER_THREADED` (`:4019`) and does `send_sig(SIGKILL, task, 0)` (`:4029`) on each task the
  walk yields. The walk (`css_task_iter_advance`, `:4944`) under `CSS_TASK_ITER_PROCS` **skips every
  task that isn't a thread-group leader** (`:4946`, `if (!thread_group_leader(task)) goto repeat;`)
  and yields a *dying* leader only while its group still has a live member (`:4950`,
  `!atomic_read(&task->signal->live)`). So for our leaderless group the live non-leader worker is
  never visited — only the already-dead leader is signalled.
- `kernel/signal.c:1660` `send_sig` → `send_sig_info` (`:1643`) → `do_send_sig_info(sig, info, p,
  PIDTYPE_PID)` (`:1652`): the signal is aimed at that one task, not its thread group.
- `kernel/signal.c:994` `complete_signal`: the dead leader fails `wants_signal` (`:982`, `PF_EXITING →
  false`), and because the type is `PIDTYPE_PID` the function returns at `:1006`-`:1011` **without**
  the `next_thread` search (`:1012`-`:1028`) that would otherwise find the live worker. So the
  group-fatal path (`:1034`-`:1056`, `__for_each_thread(signal, t) { sigaddset(&t->pending.signal,
  SIGKILL); signal_wake_up(t, 1); }`) never runs, and the worker lives — exactly the probe.

This also shows why a per-thread kill works: a `SIGKILL` aimed at the *live worker* passes
`wants_signal` (`:985`, `if (sig == SIGKILL) return true;`), so `complete_signal` reaches the
group-fatal path at `:1034` and takes the whole thread group down at once.

## The fix, verified: a process-directed SIGKILL through a process pidfd

`cgroup.kill` misses the leaderless group because its `SIGKILL` is *thread-directed* at each leader
(`PIDTYPE_PID`), and a dead leader's send returns before the group search that would reach the worker
(above). A **process-directed** `SIGKILL` — the kind `kill(tgid)` and a plain process `pidfd` send
(`PIDTYPE_TGID`) — does run that search and reaches the live worker. The `delete_flake_point4.c` probe
proves it on both supported kernels (Linux 7.2.9 and 6.19.10): after `cgroup.kill` leaves the group
`populated 1`, both `kill(tgid, SIGKILL)` and `pidfd_open(tgid)` + `pidfd_send_signal(SIGKILL, NULL)`
take it to `populated 0`, and `pidfd_open` succeeds on the zombie-leader group. So the fix needs no
per-thread (`PIDFD_THREAD`/`tgkill`) signalling at all.

`BoxCgroup::kill` (`crates/hex-runtime/src/linux/cgroup.rs`) writes `cgroup.kill` (the atomic bulk
kill); then, only if the box is still populated:

- **sweep `cgroup.procs` across the subtree, bounded** (`sweep_targets`): at most `MAX_SWEEP_CGROUPS`
  cgroups (its pending queue included) and `MAX_SIGNALLED` processes, refusing (`Refusal::Full`) past
  either rather than truncating silently (§2, §3); a cgroup gone mid-walk is a benign skip, any other
  read failure is returned. `sweep_targets` is pure over a `CgroupTree` seam, so a mock proves these
  bounds and typed refusals (`cgroup.rs` unit tests).
- **a process pidfd per id** (`sigkill_box_process`): `pidfd_open` pins exactly the process that holds
  the id now; the cgroup is then re-read and the id signalled only while it is still listed, so an id
  that named one of the box's processes when the sweep read it but has since exited — and may now name
  an unrelated process — is not signalled (the identity-and-lifetime check the delete race turned on,
  the lead's point 1). No freeze is taken, so a post-`cgroup.kill` zombie — an exited init awaiting its
  reap, which no freeze would bring to `frozen 1` — is handled, not waited on.
- **process-directed `SIGKILL`** (`pidfd_send_signal` on a process pidfd, so `PIDTYPE_TGID`): reaches
  the leaderless group's live worker; a process gone since the sweep (`ESRCH`, or no longer listed) is
  already as dead as the signal would make it.

The reproducer is `crates/hex-cli/tests/delete_flake.rs` (T-T7.1.2): a box holding a leaderless thread
group must empty on `delete` (without the fix `wait_empty` times out; with it the box empties), and a
second test confirms a box's kill spares a process outside it — the sweep signals only an id its cgroup
still lists on the re-read.

## What this does not explain

This proves one cause — `cgroup.kill` missing a leaderless group — and fixes it. It does **not** prove
the same cause is behind the separate 60-second `populated`-with-empty-`cgroup.procs`-and-`threads`
stalls STATUS.md also noted; that observation is unexplained and needs its own evidence (an upstream
report, Tejun Heo 2026-03-23 "cgroup: Wait for dying tasks to leave on rmdir", is a lead, not proof).
The leaderless-group diagnostic above is retained for that work.

## How it ran

`delete_flake.c`, compiled with `cc -O2 -pthread`, run inside `systemd-run --user --scope -p
Delegate=yes` so it owns a cgroup v2 subtree. Not of record (a mechanism probe, not a timing figure);
the kernel and load are printed with each run. Two runs on adacachy 7.2.9 on 2026-11-02 agreed.

## The fail→pass run of record (both supported kernels)

`cargo test -p hex-cli --test delete_flake` under good-citizen caps (`nice -n 10`, `-j 4`), the test's
own leaderless-thread reproducer — it waits on the leader becoming a zombie, so the leaderless state is
a fact, not a timing; a 10 s `wait_empty` bound stands in for the 60 s wedge — at the test commit
(`cb9375a`, the base kill) then the fix (`10bc30a`).

adacachy 7.2.9-2-cachyos, load 35.69:
- `cb9375a` **FAILS** — `the box must empty after cgroup.kill, but a leaderless thread held it: [tid 2020616: S …]`, 10.13 s.
- `10bc30a` **PASSES** — 0.12 s.

fedora 6.19.10-300.fc44, load 8.18:
- `cb9375a` **FAILS** — the same leaderless-worker wedge (`[tid 212928: S …]`), 10.11 s.
- `10bc30a` **PASSES** — 0.14 s.

At `10bc30a` on fedora the full `scripts/gates.sh` passes (exit 0), `oci_box`'s
`oci_boxes_report_their_inits_exit_through_the_seam` included: an earlier freeze-based fix wedged it 5 s
(an OCI box's init becomes a zombie after `cgroup.kill`, and a cgroup holding a zombie never reaches
`frozen 1`), which is why the fix signals through a pidfd with a membership re-read and takes no freeze.
