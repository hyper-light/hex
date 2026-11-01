# A macOS box that can't be escaped or overrun: the box user, the privileged helper, and the kernel's ledger bound

Source notes for the native plan's step 2 (`docs/design/runtime.md`, decision "each OS runs its own boxes
natively"). Step 1 built the rootless `process` class: a box is a process group, and a watcher polls each
member's footprint and ends the box past a bound (`bench/native/macos/RESULT.md`). That bound is an eviction
by a rule, not a kernel cap, and it holds a box of up to one member per core; at twice the cores it overshot
56-587 MiB, and the watcher cost 12-15% of a core (native-platforms.md §3, §7). This note is the evidence for
the kernel-enforced bound and the escape-proof membership that step 2 adds through a privileged helper (Ada's
call, 2026-11-01: "keep investigating rootless options, but use the ledger approach in step 2").

XNU is read at `apple-oss-distributions/xnu` `xnu-12377.1.9` (`f6217f8`), the tree in
`~/.cache/hex-t7/cross-os/xnu-src` on adacachy; `path:line` is at that revision. The macOS arm64 kernel builds
`memorystatus`, `jetsam` and `config_coalitions` (`config/MASTER.arm64.MacOSX:22,46,89`;
`config/MASTER:371,378`). Behaviour claims marked **(measure)** are the lab's to confirm as root on the Mac
Studio (`bench/native/macos/boxuser_probe.c`); reading the source fixes the mechanism, not the timing or the
kernel's exact release-build choices.

## 1. Membership: a user of its own, which a member can't drop

Step 1 and native-platforms.md §3 establish it: macOS cannot follow a fork (`NOTE_TRACK` was removed in 10.5,
`bsd/sys/event.h:364`), so a member that calls `setsid()` leaves its process group, session and parent, as
build daemons do (Gradle's, Bazel's, sccache's). The one marker a member inherits and can't drop unprivileged
is its user id, and processes can be listed by it:

- `proc_listpids(PROC_UID_ONLY, uid, …)` lists every process of a user (`bsd/sys/proc_info.h:54`,
  `PROC_UID_ONLY` is 4). A daemonized member keeps its uid, so this listing keeps it **(measure: the `escape`
  member of `box_probe.c`, run inside a box, is found by the box user's listing)**.
- `RLIMIT_NPROC` counts a user's processes and the kernel refuses a fork past it
  (`bsd/kern/kern_fork.c:269-270`: it reads the parent's `RLIMIT_NPROC` and calls `chgproccnt(uid, 1)` on each
  fork). So a box's process count is capped in the kernel by the box user's `RLIMIT_NPROC`, where step 1's
  rootless box could only count the group and end it.
- Changing a user id takes privilege (`setuid` to another uid is root's). So a box that runs as a user of its
  own needs something privileged to start it there: the helper.

A pool of box users is made once, at install, with admin rights, as Nix's build users are
(`_nixbld1…`). This note and the design take the pool as given; the installer is the plan's step 6.

## 2. The privileged helper: a launchd daemon that does one thing

Starting a program as another user, arming a per-task kernel memory limit (§3), and routing another task's
Mach exceptions (§4) are all privileged. So the node talks to a small privileged helper — a launchd daemon
installed with admin rights — over a typed, bounded protocol, and the helper does exactly one thing: start a
box's leader as a given pool user, in a new process group and session, armed. It refuses every other request
by name (CLAUDE.md §3), and a hostile-input test exercises its parser (CLAUDE.md §6). The node keeps every
later duty that needs no privilege: listing the box user's processes, reading footprints, deciding the
apportionment, and ending the box (a box user's processes are the node's to kill, since the node runs as the
user that owns the pool, or the helper kills on request — **(measure: which)**).

The helper is the macOS analog of what `clone3(CLONE_INTO_CGROUP)` and the cgroup delegation give the Linux
`process` class for free: a kernel-enforced boundary a member is born inside and can't leave.

## 3. The ledger bound: a per-task physical-footprint limit, apportioned

macOS caps a single task's physical footprint through its ledger. `task_set_phys_footprint_limit(task,
new_limit_mb, …)` sets it (`osfmk/kern/task.c:7583`), and it is root-gated:

- It calls `proc_check_footprint_priv()` first and returns `KERN_NO_ACCESS` on failure
  (`osfmk/kern/task.c:7593`). `proc_check_footprint_priv` is `priv_check_cred(…, PRIV_VM_FOOTPRINT_LIMIT, 0)`
  (`bsd/kern/kern_priv.c:131-133`), `PRIV_VM_FOOTPRINT_LIMIT` is 6002 (`bsd/sys/priv.h:104`), and
  `priv_check_cred` grants a uid-0 credential every privilege (`bsd/kern/kern_priv.c:96-99`). So root sets the
  limit and an unprivileged process gets `KERN_NO_ACCESS` — which is what step 1 measured rootless
  (`mem_bound_probe.c`). **(measure: root sets it on a spawned child and reads it back.)**
- The other door to the same ledger limit is `memorystatus_control(MEMORYSTATUS_CMD_SET_MEMLIMIT_PROPERTIES,
  …)`, which needs root or the `com.apple.private.memorystatus` entitlement, else `EPERM`
  (`bsd/kern/kern_memorystatus.c:9191-9193`). `posix_spawn` sets the same thing atomically at spawn through its
  jetsam attribute (`posix_spawnattr_setjetsam_ext`, applied at exec, `bsd/kern/kern_exec.c:4642-4678`), which
  is how the helper arms a box's leader **before its first instruction** (§5).

The limit is per task. A box's bound is apportioned across its members so their limits sum to the bound less a
reserve (`hex_core::boxuser::apportion`): by pigeonhole the armed members' footprints then sum to at most
`bound − reserve`, so the whole tree stays under the bound even while a just-forked member is still unarmed
(§5). A member that reaches its share is re-apportioned with its new footprint; if headroom remains its share
grows, else the box is ended. The arithmetic is `hex_core::boxuser`, tested against an oracle; the kernel turns
each per-task limit into the wall.

## 4. What a crossing does — fatal versus the high-watermark, and the exception's delivery

This is the crux step 1 left to step 2, and the source fixes it. When a task's footprint ledger crosses its
limit, the ledger callback `task_footprint_exceeded` runs `task_process_crossed_limit_no_diag`
(`osfmk/kern/task.c:7442,7517`), and there are two routes, by whether the limit is **fatal**:

- **Fatal** (`memlimit_is_fatal`): the exception is sent **synchronously** on the suspended task through
  `exception_triage(EXC_RESOURCE, …)` (`osfmk/kern/task.c:7406-7417`), so a handler registered on the task's
  `EXC_MASK_RESOURCE` port receives it at once; then `memorystatus_on_ledger_footprint_exceeded` kills the
  task (`memstat_kill_process_sync`, `bsd/kern/kern_memorystatus.c:5076-5091`). So a fatal per-task limit is a
  hard kernel cap: the member cannot exceed it; it is killed at it. **(measure: a fatal limit's overshoot —
  how far past the limit a fast allocator's footprint reaches before the kill.)**
- **High-watermark** (not fatal): the exception is **not** sent on the live task; a corpse of the task is
  forked and the exception is queued on it (`task_enqueue_exception_with_corpse(task, EXC_RESOURCE, …)`,
  `osfmk/kern/task.c:7419-7427`, `osfmk/corpses/corpse.c:572`), delivered asynchronously by the
  `thread_exception_queue` daemon to the (inherited) `EXC_MASK_RESOURCE` port (`osfmk/kern/thread.c:904-933`).
  The task is **not** killed and keeps running past the limit. Two costs: it fires **once per task per limit**,
  gated by an exception bit (`task_set_exc_resource_bit`, `osfmk/kern/task.c:7262-7277,7529`), re-armed only by
  `MEMORYSTATUS_CMD_REARM_MEMLIMIT` (root/entitlement, `bsd/kern/kern_memorystatus.c:8805-8817`); and corpses
  have a small system-wide budget, `DEFAULT_TOTAL_CORPSES_ALLOWED` = 5 on macOS
  (`osfmk/corpses/task_corpse.h:62`, `osfmk/corpses/corpse.c:163,239`). **(measure: the high-watermark
  notification's latency, that it fires once, that a root re-arm makes it fire again, and whether a fork storm
  exhausts the corpse budget.)**

The consequence for the design, which step 1's hand-off did not have: Ada's "a member crossing its share
raises `EXC_RESOURCE` to the helper, which rebalances or ends the box, event-driven with no polling" maps onto
the **high-watermark** route (notify without killing, then re-apportion and re-arm as root), with the fatal
route as a **backstop** the tree truly cannot pass. The clean event-driven rebalance is real but carries the
once-per-limit bit, the root re-arm, and the corpse budget; the measurements decide how much of the bound
leans on the event and how much on a fatal backstop plus a cheap periodic re-apportion. The exception is caught
by `task_set_exception_ports(task, EXC_MASK_RESOURCE, helper_port, …)` set by the helper (privileged, §2).

## 5. Fork and exec: the arming window, and the reserve

A new member starts unarmed, and the source says why exactly:

- **Fork** resets the child's memorystatus limits to none — `child_proc->p_memstat_memlimit = 0`,
  `_memlimit_active = 0`, `_memlimit_inactive = 0`, priority to default (`bsd/kern/kern_fork.c:1199-1209`) — and
  a new task is given a fresh ledger carrying only the system-wide `max_task_footprint` limit
  (`osfmk/kern/task.c:1555,1684-1686`), which is 0 (no limit) on a default macOS kernel
  (`max_task_footprint_mb` tunable, `osfmk/kern/task.c:468`). So a forked member is unbounded until the helper
  arms it.
- **Exec** copies almost nothing of the task across (`task_copy_fields_for_exec` copies only `vtimers`,
  `osfmk/kern/task.c:9352-9355`); the limit is set at exec only when `posix_spawn` carried a jetsam attribute
  (`bsd/kern/kern_exec.c:4631-4678`), and exception ports only when the spawn asked
  (`exec_handle_exception_port_actions`, `kern_exec.c:2908`). So `posix_spawn` can arm the **leader** atomically
  at its first instruction, and the helper does that; a member that the box **forks** afterwards is armed by the
  helper at the fork's notice.

So the window is: a member forks → the helper learns (a `kqueue` `EVFILT_PROC`/`NOTE_FORK` on the known
members wakes it, then it lists `PROC_UID_ONLY` and arms the new pid; or it polls the listing) → it arms the
child. The reserve (`hex_core::boxuser::Bound::reserve`) is the headroom kept out of apportionment to cover
what a member allocates in that window; it is sized against the measurement. **(measure: the window per
member, and how much a child allocates in it, steady and under a 28-member fork burst; the reserve and the
fork-burst overshoot ≤ 64 MiB follow.)**

## 6. Coalitions: considered, set aside for the cap

A forked (non-corpse) task inherits all its parent's coalitions (`osfmk/kern/task.c:1936-1943`), and a
resource coalition sums its members' instantaneous `phys_footprint` (`coalition_info_resource_usage`,
`bsd/sys/coalition.h:49`; `osfmk/mach/coalition.h`). That is a cheap aggregate **read** of a whole tree's
footprint in one call, which the helper could use in place of summing each member's rusage. But the only
resource a coalition can be **limited** on is logical I/O writes, not footprint (`osfmk/kern/coalition.h`,
`bsd/sys/coalition.h`), so a coalition cannot be the memory cap; the per-task ledger limit, apportioned (§3),
is. Coalitions stay a possible optimisation for the read, noted, not the mechanism.

## 7. Precedents

- **FreeBSD `rctl`/RACCT** caps a process's, user's, loginclass's or jail's `memoryuse` (RSS) in the kernel
  with a deny action (needs `kern.racct.enable`); its manual warns that denying `memoryuse` can thrash. It is
  the closest kernel analog to a per-task footprint cap; XNU has no RACCT/RCTL, only the ledger.
- **Solaris `rcapd`** caps a project's or zone's RSS from **userspace** by sampling and paging out the excess
  (default 5 s sample, OmniOS `rcapd(8)`: a collection's RSS can exceed its cap until the next sample). That is
  step 1's rootless model, at seconds; step 2's kernel ledger is the hard-cap version.
- **systemd-oomd** and **Android `lmkd`** watch PSI from userspace and `SIGKILL` a whole cgroup / a process by
  priority before the kernel OOMs; the helper's "end the box when no headroom remains" is the same policy,
  applied to a box rather than the machine.
- **Resource Containers** (Banga, Druschel, Mogul, OSDI 1999) separate the resource principal from the
  protection domain; XNU coalitions (§6) are that idea, and a box apportioning one bound across many tasks is
  too.

## 8. What would falsify the design

- A public, unprivileged macOS interface that caps a process tree's footprint (then the helper isn't needed
  for memory). Step 1's `mem_bound_probe.c` found none.
- A fatal per-task ledger limit whose overshoot before the kill is large and unbounded (then the fatal backstop
  doesn't hold a tight bound, and the design must lean on the high-watermark notify, or on a smaller per-task
  share). **(measure.)**
- A high-watermark `EXC_RESOURCE` that a root process can't re-arm, or whose corpse delivery a fork storm
  starves past use (then the event-driven rebalance isn't dependable and the periodic re-apportion carries it).
  **(measure.)**
- A forked member whose arming window is wider than a reserve can cover without wasting most of the bound (then
  the box can't be both tight and fork-heavy, and the rootless watcher stays the fork-storm bound). **(measure.)**

## 9. To measure, as root on the Mac Studio (`boxuser_probe.c`)

Root needs Ada's admin password; ask when the lab is ready, and never store or print it (plan step 2). Three
passes of each, recorded in `bench/native/macos/`:

- `priv` — root arms `task_set_phys_footprint_limit` on a spawned child and reads it back; the rootless
  `KERN_NO_ACCESS` becomes success.
- `cap fatal|soft MB` — a child allocates at full speed under a fatal, then a high-watermark, per-task limit:
  overshoot and the kill (fatal); the notification's latency, once-per-limit, and re-arm (soft).
- `spawnarm fatal MB` — the leader armed at spawn (`posix_spawnattr_setjetsam_ext`): the cap holds from the
  first instruction.
- `forkwin MB` — a leader forks a hog; the window from fork to arm and the child's footprint at arm.
- `uid UID N` — N members under a pool uid, one daemonized: `proc_listpids(PROC_UID_ONLY)` finds all,
  `RLIMIT_NPROC` caps the forks, a kill-all empties the uid.
- `box B N steady|burst` — per-task limits summing to B across N = 1,4,8,14,28 members allocating at full
  speed: the box's total overshoot (target ≤ 8 MiB steady; ≤ 64 MiB for a 28-member fork burst) and the
  helper's CPU (target ≤ 0.1% of a core at 95% of the bound).
