# cgroup v2 resource control and machine protection

Research note for CP-T7.1, AC-T7.1.1 ("no build or agent on a Linux runner can starve the machine"),
AC-T7.1.2 (another box's fsync p99 at most 2x idle while one box builds) and AC-T7.1.5 (agent memory per box
at most 1 MiB) (`docs/requirements.toml`). Written and sourced 2026-10-22; every source below was opened and
read that day (accessed 2026-10-22). This note decides nothing: decisions go in `docs/design/` with these
sources cited (CLAUDE.md §5).

**The question.** On 2026-10-22 a production host missed its liveness checks and restarted because a CI build
(a 14-core rustc, load about 30) starved it of memory (task brief; I read no logs, so the incident's mechanism
is not established here). Hex's proposed fix puts every CI job under a parent cgroup with `memory.high` =
machine memory minus max(15%, 8 GiB), gives the runner and agent processes `memory.low`, and gives each job a
fair-share `cpu.weight` and `io.weight` and a `pids.max` cap. This note collects the evidence for and against.

## Conventions

- **Kinds** (as in `docs/research/README.md`): `MEASURED-INDEPENDENT` (a third party measured it),
  `MEASURED-VENDOR` (the project, or the kernel developers on their own patch, measured it), `CLAIMED` (no
  method), `DESIGN` (a fact from code or documentation, or a vendor default that was not measured).
  "(computed here)" marks arithmetic done in this note on cited constants; it takes the kind of its inputs.
- **Citations** are `[CG-n]` into the numbered References at the end. `l.N` is a line of the file as read.
  Kernel files were read at Linux master, commit `a1c2b1da93f5` (its `Makefile` reads 7.3.0), fetched from
  git.kernel.org on 2026-10-22; `mm/memcontrol.c`, `mm/page_counter.c`, `mm/vmscan.c` and `psi.rst` were
  byte-identical to the copies first read earlier the same day, and `cgroup-v2.rst` differed only in its
  cpuset section (line numbers below are the pinned copy's). Commits are cited by id from their patch pages.
  Section names are given for papers (`§`).
- **Scarcity of independent numbers.** Almost every measurement below is from Meta (the authors of PSI,
  oomd, IOCost, TMO and most of the memory controller) or from kernel developers measuring their own patch.
  The only third-party measurement of `memory.high` behaviour I found is the Kubernetes MemoryQoS benchmark
  [CG-39][CG-40], on single nodes. Treat every `MEASURED-VENDOR` number as a lead, not as a fact Hex designs
  against, until Hex measures the same thing on its own hardware (§9).
- **Percentiles** are quoted as reported. None of the sources here reports a p99.9; the IOCost paper reports
  "P99" once (§4.6) and counts SLO violations; the Kubernetes benchmark reports single runs.
- **Hex's hosts are not described in any source I read.** Kernel version, swap or zswap, filesystem, disk
  type and the cgroup layout the kiln agent runs in are open (§10); Hex's cgroup-delegation facts are in the
  project memory notes, not in a source.

## Summary

1. **`memory.high` on a parent is a throttle, not a guard, and by itself it turns "host dies" into "CI jobs
   hang".** The kernel documents that crossing it "never invokes the OOM killer" and that it "should be used in
   scenarios where an external process monitors the limited cgroup" [CG-1 l.1381-1389]. A 2023 kernel
   documentation commit, acked by Johannes Weiner and Chris Down, says `memory.high` "can result in reclaim
   pressure which simply leads to stalls unless some external component observes and actions on it" and that
   `memory.max` is "the effective way to enforce memory limits" [CG-15]. Kubernetes' benchmark kept an
   aggressor "Throttled indefinitely — still running at 10min, never OOM-killed" in both runs where the gap
   between `memory.high` and `memory.max` was 804 MiB or more (8 GiB and 14 GiB limits) [CG-40 §6]. The
   proposal needs a backstop: a `memory.max` on the parent, a PSI-driven killer, or both (§9.5).
2. **The throttle lands on every task in the limited subtree, not only on the offender.** The charge path
   walks the allocating task's ancestors and flags the task if any ancestor is over `memory.high`; the delay
   is computed from the largest overage in the ancestry [CG-3 l.2807-2838, l.2484-2496; CG-8]. Kubernetes sets
   `memory.high` on containers, not pods, for this reason: a pod-level `memory.high` "will induce throttling in
   other containers" [CG-38 "Proposal"]. So the runner and the agent must sit **outside** the `memory.high`
   subtree (siblings of it), as Meta's `hostcritical.slice` sits beside `workload.slice` [CG-23][CG-20 Fig. 1].
   `memory.low` protects against reclaim, never against this throttle.
3. **`memory.low` is best-effort, is capped by every ancestor, and fails exactly when a swapless host runs out
   of reclaimable memory.** Effective protection is "capped to the parent's effective protection" unless
   `memory_recursiveprot` is mounted [CG-4 l.300-307][CG-1 l.210-218]; `memory.low` is skipped only on the first
   reclaim pass and reclaim "retries if it couldn't find pages anywhere else" rather than OOM [CG-18][CG-5
   l.6632-6639]. The author of `memory.min` wrote that `memory.low` "makes it pretty useless against any sort
   of slow memory leaks ... especially true for swapless systems" [CG-16]. On a host of about 32G, Meta
   protected `hostcritical.slice` with 192M of both `memory.low` and `memory.min` [CG-25] (units as printed
   there).
4. **Meta tried `memory.high` first and abandoned it.** The fbtax2 team put `memory.high` on `system.slice`
   and found that "restricting memory on these system binaries made them more prone to thrashing and OOMs"
   [CG-25]. The shipped design uses `memory.min`/`memory.low` (work-conserving, proportional) plus `io.latency`,
   swap, `cpu.weight` and a pressure-driven killer [CG-29]. Under 10, 50 and 100 MB/s leaks of up to 20 GB, the
   base machine reached "deep thrashing" and, in the 10 and 50 MB/s runs, "never recovers without a reboot";
   the fbtax2 machine lost some requests per second and recovered when oomd killed the leaker [CG-29]
   (`MEASURED-VENDOR`, charts summarised in text). This is the closest published analogue of the incident.
5. **The closest published analogue for CI is Meta's Sandcastle.** About 5% of its hosts rebooted unexpectedly
   per day; after one cgroup per build job and oomd killing whole cgroups under memory pressure the rate fell
   to under 0.5%, post-OOM downtime fell "from tens of minutes to just seconds", and jobs per host rose 35%
   [CG-30] (`MEASURED-VENDOR`).
6. **Memory control alone is not enough, and `io.weight` does nothing unless iocost or BFQ is on.** IOCost's
   authors: "memory limits still resulted in reclaim which interfered with latency-sensitive applications
   through IO. We could achieve comprehensive isolation only by doing both memory and IO controls together"
   [CG-20 §5]. The kernel documents weight-based IO control as provided by iocost (disabled by default) or
   BFQ [CG-1 l.2012-2020, l.2069-2070]; on filesystems without cgroup writeback support "all writeback IOs are
   attributed to the root cgroup" [CG-1 l.2226-2229].
7. **PSI is the supported detection signal and sees the throttle.** The sleep that implements the throttle is
   bracketed by `psi_memstall_enter()`/`leave()` [CG-3 l.2637-2639]; the commit says so on purpose [CG-7]. The
   shipped userspace killers use `full avg10` over 30 s by default (systemd-oomd: 60%) [CG-36], with swap
   strongly recommended because "without swap, the system enters a livelocked state much more quickly and may
   prevent systemd-oomd from responding in a reasonable amount of time" [CG-35].
8. **The throttle is a rate limiter whose curve is relative to `memory.high`.** The sleep per 64-page charge
   batch is 64 x r^2 seconds, r = (usage - high) / high, capped at 2 s [CG-3] (computed here). With Hex's
   formula on a host of 64 GiB or more the cap is reached only when the CI subtree holds about **all** of RAM
   (§2.4), and an allocating task is held to about 4 KiB / r^2 per second (1.6 MB/s at 5% over, per task). It
   slows a runaway; it does not stop one. On kernels before 5.9 it could livelock outright (Kubernetes backed
   out MemoryQoS beta over it) [CG-38].
9. **No source supports the 15% / 8 GiB constant, a CPU wake-up latency number for `cpu.weight` at load 30, or
   any `memory.high` plus zswap measurement.** Those are not published / not found (§10). Meta sizes
   protections from `memory.current` measured under moderate pressure [CG-25]; Senpai sizes by PSI feedback
   [CG-21]; Kubernetes derives from requests [CG-38]; its docs example reserves 10% of a node plus an eviction
   threshold [CG-50].
10. **The kernel will not rescue the host, and placement comes first.** A kernel memory-controller developer
    writes, as "we (kernel MM community)", that they have decided to "keep the kernel oom-killer very
    conservative" and to leave kill policy to userspace killers, which "are prone to reliability issues
    (oom-killer getting stuck in reclaim or not getting enough CPU)" [CG-44]; an operator measured 20-30
    minute brownouts with the OOM path never running, on zram-only swap [CG-44][CG-45]. Borg kills at the
    memory limit and admits prod work by limits; Twine "does not overcommit CPU or memory by default";
    Kubernetes keeps `sum(requests)` within allocatable [CG-48][CG-49][CG-38]. The cgroup caps in the proposal are the second line; the first is placement by memory
    (AC-T7.3.1), and the killer that backs them up must be protected and given CPU.

## 1. Key numbers

Kinds as in Conventions. "r" is `(usage - memory.high) / memory.high`.

| # | Number | Conditions | Kind | Source |
|---|---|---|---|---|
| K1 | Throttle sleep per 64-page charge batch = 64 x r^2 s (computed here from `penalty = overage^2 x HZ >> 20 >> 14`, overage = r x 2^20); clamp 2 s per return to userland; no sleep if the penalty is at most HZ/100 (10 ms); the clamp is reached at r = 17.7% | current `mm/memcontrol.c`; `MEMCG_CHARGE_BATCH` is 64 | DESIGN | [CG-3 l.2418, l.2463-2464, l.2535-2547, l.2608-2617]; [CG-6 l.336] |
| K2 | Code-comment delay table for `memory.high` = 100 MB: 101M 6 ms; 102M 25 ms; 105M 159 ms; 110M 639 ms; 115M 1439 ms; 118M and up 2000 ms | "time to allocate in ms" | DESIGN | [CG-3 l.2437-2461] |
| K3 | Upper bound on one task's allocation rate while throttled: 4 KiB / r^2 per second (computed here from K1, 4 KiB pages): r = 1% 41 MB/s; 5% 1.6 MB/s; 10% 0.41 MB/s; 17.7% and up 0.13 MB/s | per task; tasks sleep independently, so the aggregate scales with the number of allocating tasks | DESIGN | derived from [CG-3] |
| K4 | Hex's formula, high = M - max(0.15 M, 8 GiB) (computed here): M = 64 GiB gives high 54.4 GiB and the 2 s clamp at 64.0 GiB (all of M); M = 128 GiB high 108.8 GiB, clamp at 128.0 GiB; M = 32 GiB high 24 GiB, clamp at 28.2 GiB (88% of M); M = 16 GiB high 8 GiB, clamp at 9.4 GiB. At 0.90 M on a 64 GiB host r = 5.9%, sleep 0.22 s per batch | the proposal in the task brief | DESIGN (arithmetic) | K1 |
| K5 | 2019 demo, no swap, `MemoryHigh=100M`, 1 MB allocations: before the throttle, 1.0 ms per MB up to 100 MB and 1.6-2.0 ms per MB past it, then "System OOM in 2-3 seconds"; with it, 1.0 ms at 100 MB, 84.7 ms at 101, 330.6 ms at 102, 1.02 s at 104, 2.39 s at 106, 16.4 s per MB from 118 MB. The 16.4 s at the clamp is 8 clamped 2 s sleeps per MB, so the charge batch then looks like 128 KiB (inference); today's batch is 64 pages, and today's table K2 gives 6 ms per batch at +1%, about 24 ms per MB against the 84.7 ms measured here. The curve or batch constants changed since; I did not trace which commit | test VM, microbenchmark in the commit message | MEASURED-VENDOR | [CG-7] |
| K6 | A cgroup stuck in allocator throttling: `memory.pressure` some avg10=78.50, avg60=84.99, avg300=84.53 and full avg10=78.50, `io.pressure` about 0, with `inactive_file` 1,370,939,392 B and `active_file` 661,635,072 B of slack cache; cause fixed by retrying reclaim before throttling | Facebook production, one cgroup | MEASURED-VENDOR | [CG-9] |
| K7 | Kubernetes single container (request 256 Mi, limit 512 Mi, high 486 MiB): operation latency about 0.5 ms up to 485 MiB, 27 ms at 490, 119 ms at 495, 448 ms at 500, 336 ms at 505; OOM-killed 54-71 s after start; `memory.events` high 3,596-4,234 | alpha run in a kind single-node cluster (kernel 6.18.5), beta run on a `local-up-cluster.sh` single node (kernel 7.0.9); single runs, three repeat trials 59-65 s | MEASURED-INDEPENDENT | [CG-39 §1, §3, §10][CG-40 §1] |
| K8 | Kubernetes aggressor allocating about 100 MiB/s: high-to-max gap 804 MiB and 1,421 MiB "Throttled indefinitely — still running at 10min, never OOM-killed" (`high` events 770,790 and 628,635); gap 1 MiB OOM-killed after about 44 s; holder pods unaffected | 31.7 GiB node, single node | MEASURED-INDEPENDENT | [CG-40 §5, §6] |
| K9 | Kubernetes node with `sum(memory.min)` = 28 GiB = 90.6% of 30.9 GiB allocatable: stable; BestEffort aggressor OOM-killed at 20,482 MiB after about 212 s; minimum `MemAvailable` 1,118 MiB; kubelet API latency under 50 ms until `MemAvailable` fell below 2 GiB, 502 ms at the minimum | single node | MEASURED-INDEPENDENT | [CG-40 §8] |
| K10 | Kubernetes Guaranteed pod with `memory.min` = `memory.max` writing page cache: OOM-killed in the first iteration (twice); `memory.min` = 0: 19+ iterations; `memory.low` = 512 MiB: 21+ iterations with 725,184 `low` events | 512 Mi pod | MEASURED-INDEPENDENT | [CG-40 §7] |
| K11 | `memory.low` scaling, a workload of about 19 GB: page scans/s test vs control at `memory.low` 21G 0 vs 0; 17G 867 vs 3,799 (23%); 12G 1,203 vs 3,543 (34%); 8G 2,534 vs 3,979 (64%); 4G 3,980 vs 4,147 (96%); 0 3,799 vs 3,980 (95%) | Facebook, one workload, test kernel with proportional reclaim | MEASURED-VENDOR | [CG-17] |
| K12 | fbtax2 host of "around 32G" (units as printed): 19G `memory.low` on `workload.slice`, 192M (0.2G) on `hostcritical.slice` (both `memory.low` and `memory.min`), 6.95G hugepages, "just under 6G up for grabs" | Meta fleet host | MEASURED-VENDOR (configuration) | [CG-25] |
| K13 | fbtax2 leak tests, a leak up to 20 GB in `system.slice` at 10, 50, 100 MB/s: base setup "deep thrashing and never recovers without a reboot" at 10 and 50 MB/s, at 100 MB/s hhvm OOM-killed by the kernel; fbtax2 setup survived with oomd killing the leaker; HDD fleet, SSD preliminary results better | repeated 3 times on fbtax2, once on base; results are charts, text summarised | MEASURED-VENDOR | [CG-29] |
| K14 | `io.latency` targets: `workload.slice` 50 ms, `system.slice` 75 ms; "60ms or 100ms would also likely work fine"; HDD uncontended latency 7-20 ms, ballooning to about 10x under contention | Meta, trial and error | MEASURED-VENDOR | [CG-29][CG-27] |
| K15 | Sandcastle (Meta's build, test and land system): unexpected host reboots about 5% per day to under 0.5%; post-OOM downtime "tens of minutes to just seconds", 10% capacity gain; 35% more build jobs per host | per-job cgroup plus oomd | MEASURED-VENDOR | [CG-30] |
| K16 | Async tier: load shedding on memory pressure "increased throughput by 25%" | Meta | MEASURED-VENDOR | [CG-26] |
| K17 | "In practice at Facebook, we've regularly seen 30 minute host lockups go away entirely" | oomd README | CLAIMED | [CG-32] |
| K18 | IOCost, production web servers with a memory leak started in the system slice: with IOCost the web server "drops to no lower than 80% of its normal throughput"; BFQ shows "an almost total loss of throughput". Separately, in the lab, a memory-stressed benchmark scaled its load up "about 5x faster" with IOCost than with BFQ | production: two SSD generations; lab: single-socket 64 GB server, Linux 5.6 plus the 5.15 IOCost changes | MEASURED-VENDOR | [CG-20 §4, §4.5] |
| K19 | IOCost, ZooKeeper-like service, 12 ensembles on 5 machines, 6 h, 1 s SLO, P99 of the 11 well-behaved ensembles: blk-throttle 78 violations (some tens of seconds); BFQ 13 (2-5 s each; "total system unresponsiveness" forced reruns); IOLatency 31 (longest 7.8 s); IOCost 2 (1.5 s and 1.04 s) | enterprise SSDs | MEASURED-VENDOR | [CG-20 §4.6] |
| K20 | Migrating a region of hundreds of thousands of servers from IOLatency to IOCost: package-fetch failures "roughly 10x fewer"; container-cleanup stalls over 5 s "3x reduction" | production, two months and one month | MEASURED-VENDOR | [CG-20 §4.8] |
| K21 | TMO saves 20-32% of total memory across millions of servers; Senpai: `reclaim_ratio` 0.0005, `PSI_threshold` 0.1% (some), every 6 s, at most 1% of the workload per period, "reaction time to extreme contraction tends to be minutes"; reclaim costs 0.05% of all CPU cycles | Meta fleet | MEASURED-VENDOR | [CG-21 §1, §3.3, §3.4] |
| K22 | PSI triggers: window 500 ms to 10 s; checked 10 times per window (50 ms to 1 s); unprivileged windows a multiple of 2 s; notifications rate-limited to one per window | kernel | DESIGN | [CG-2 l.97-119] |
| K23 | systemd-oomd defaults: memory pressure limit 60% (full avg10) sustained for 30 s; swap used limit 90%; suggested 40% for `user@UID.service`; PSI needs Linux 4.20 | systemd 262 manual | DESIGN (defaults, not measured) | [CG-36][CG-35] |
| K24 | oomd example thresholds: `workload.slice` above 5% and `system.slice` above 40% for 15 s, or `system.slice` above 80% for 30 s | example configuration | DESIGN | [CG-34] |
| K25 | oomd's own protection: `MemoryMin=64M` on `oomd.service` in `hostcritical.slice` "is a reasonable starting point" | vendor guidance | DESIGN | [CG-33] |
| K26 | Weights: `cpu.weight`, `io.weight` range 1-10000, default 100; root-cgroup processes are an implicit IO child of weight 200 and, for CPU, each root thread is a child weighted by its nice level; `io.latency` delay events limited to 1 s | kernel | DESIGN | [CG-1 l.671, l.1184-1189, l.2145-2151, l.2322, l.3149-3168] |
| K27 | Kubernetes `memory.high` = floor((requests + factor x (limit - requests)) / pageSize) x pageSize, factor 0.9 from v1.27; default factor changed to nil (off) in v1.37; kernel 5.9 or later required for the livelock fix | KEP-2570 | DESIGN | [CG-38] |
| K28 | Zram-only-swap hosts "spin in direct reclaim for 20-30 minutes without ever invoking the OOM killer" on 377 GiB machines; over 5 minutes on one node: 141 `should_reclaim_retry` calls all with `no_progress_loops` = 0 and the watermark check passing, reclaimable estimate 4.8M-5.3M pages (19-21 GiB), anon reclaim 52M of 244M scanned (21%), 53% of anon scan events reclaimed zero pages, file reclaim 33M of 42M (78%); a second node 18% anon vs 90% file scan efficiency | production nodes (Cloudflare per [CG-47]), LKML 2026-03-03/04, bpftrace; the brownout itself not captured | MEASURED-INDEPENDENT | [CG-44][CG-45] |
| K29 | `cpu.max` throttling while under quota (highly threaded, non-CPU-bound groups): after the fix "almost 30x performance improvement" in an artificial test, 10 ms quota per 100 ms period on an 80-CPU machine | Indeed, kernel commit 2019-07 | MEASURED-VENDOR (patch author's test) | [CG-54] |
| K30 | Borg scheduling delay: a runnable thread waited more than 5 ms for a CPU only "a few percent of the time", "almost never" longer; modified CFS, Dec 2013, one representative cell | Google production | MEASURED-VENDOR | [CG-48 §6.2, Fig. 13] |
| K31 | Borg: memory is non-compressible (kill, lowest priority first); CPU and disk bandwidth compressible (throttle); the Borglet kills at the memory limit or when an over-committed machine runs out | Google production | DESIGN | [CG-48 §5.5, §6.2] |
| K32 | Kubernetes: default hard eviction `memory.available<100Mi` (Linux), checked every `housekeeping-interval` = 10 s; docs example reserves 10% of a 10 GiB node for system daemons plus a 500 MiB eviction threshold | kubelet defaults and a worked example | DESIGN | [CG-50][CG-51] |
| K33 | Twine: "does not overcommit CPU or memory by default"; fleet machines have one 18-core CPU and 64 GB RAM | Meta | DESIGN (statement) | [CG-49 p.792, p.800] |
| K34 | PSI averages are fed by sampling every 2 s; triggers use their own faster path (K22) | kernel | DESIGN | [CG-41] |

## 2. What the kernel documents and how `memory.high` is enforced

### 2.1 Hierarchy rules that shape Hex's layout

- **No internal processes.** A non-root cgroup can distribute a domain resource to its children "only when
  they don't have any processes of their own"; to control a cgroup's children it must "create children and
  transfer all its processes to the children before enabling controllers" [CG-1 l.507-534]. A cgroup that has
  the runner or agent process directly in it cannot enable `memory`, `cpu` or `io` for the job cgroups below it.
- **Top-down constraint and delegation.** A controller reaches a cgroup only if the parent enabled it
  [CG-1 l.495-505]. Delegation is by write access to the directory and to `cgroup.procs`,
  `cgroup.threads`, `cgroup.subtree_control`; the delegatee must not write the resource files of the
  delegated directory itself, because those "control the distribution of the parent's resources"
  [CG-1 l.543-565]. Processes cannot be moved in or out of a delegated subtree by the delegatee [CG-1
  l.575-611]. Settings are hierarchical: "nothing can escape the resource restrictions imposed by the
  parent" [CG-1 l.562-565].
- **Organize once.** "Migrating a process across cgroups is a relatively expensive operation and stateful
  resources such as memory are not moved together with the process" [CG-1 l.617-631]; memory stays charged to
  the cgroup that instantiated it [CG-1 l.1990-1996]. A job should be placed in its final cgroup at start
  (`CLONE_INTO_CGROUP`, see `oci-runtimes-and-fast-start.md`), not moved after it has grown.
- **Four distribution models** [CG-1 l.661-738]: weights (work-conserving, 1-10000, default 100); limits
  (may be over-committed, default `max`); protections (default 0, hard or best effort, over-commit is split
  among children); allocations (cannot be over-committed).

### 2.2 The memory controls as documented

| Control | What the kernel documentation says | Source |
|---|---|---|
| `memory.min` | "Hard memory protection. ... won't be reclaimed under any conditions. If there is no unprotected reclaimable memory available, OOM killer is invoked." Capped by ancestors; if over-committed each child gets a share proportional to its usage below `memory.min`. "Putting more memory than generally available under this protection is discouraged and may lead to constant OOMs." | [CG-1 l.1331-1352] |
| `memory.low` | "Best-effort memory protection ... won't be reclaimed unless there is no reclaimable memory available in unprotected cgroups." Above the effective boundary pages are reclaimed "proportionally to the overage". Capped by ancestors. | [CG-1 l.1354-1375] |
| `memory.high` | "Memory usage throttle limit. ... processes of the cgroup are throttled and put under heavy reclaim pressure. Going over the high limit never invokes the OOM killer and under extreme conditions the limit may be breached. The high limit should be used in scenarios where an external process monitors the limited cgroup to alleviate heavy reclaim pressure." Opening the file `O_NONBLOCK` skips the synchronous reclaim on write, with the caveat that the cgroup "may take indefinite amount of time to reduce usage below the limit". | [CG-1 l.1377-1401] |
| `memory.max` | "the main mechanism to limit memory usage ... If a cgroup's memory usage reaches this limit and can't be reduced, the OOM killer is invoked in the cgroup." Usage "may go over the limit temporarily". | [CG-1 l.1403-1430] |
| `memory.oom.group` | The cgroup is "an indivisible workload by the OOM killer": all tasks killed together or not at all. "If the OOM killer is invoked in a cgroup, it's not going to kill any tasks outside of this cgroup." | [CG-1 l.1477-1493] |
| `memory.events` | `low` (reclaimed although under `low`: "usually indicates that the low boundary is over-committed"), `high` (throttled and sent to direct reclaim), `max`, `oom`, `oom_kill`, `oom_group_kill`, `sock_throttled`; all hierarchical. | [CG-1 l.1495-1547] |
| `memory.reclaim` | Write-only proactive reclaim, e.g. `echo "1G" > memory.reclaim`; the kernel "can over or under reclaim". | [CG-1 l.1432-1451] |
| `memory.swap.high` / `.max` | `swap.high` is a throttle that "marks a point of no return ... NOT designed to manage the amount of swapping a workload does during regular operation"; its commit says the aim is to "protect the system from potentially buggy tasks consuming a lot of swap" and that "slowing misbehaving tasks down gradually allows user space oom killers or other protection mechanisms to react". `swap.max` stops anonymous memory from being swapped out. | [CG-1 l.1847-1878][CG-14] |
| `memory.zswap.max` / `.writeback` | `zswap.max` caps the zswap pool; `zswap.writeback=0` disables writes to swap devices and, if stores keep failing for incompressible pages, "users can observe reclaim inefficiency". | [CG-1 l.1912-1936] |
| `memory.pressure` | PSI for the cgroup; see §3. | [CG-1 l.1938-1942] |

What is charged: page cache and anonymous memory, kernel data structures such as dentries and inodes, and TCP
socket buffers; "While not completely water-tight, all major memory usages by a given cgroup are tracked ...
to a reasonable extent" [CG-1 l.1297-1314]. `memory.stat` breaks the kernel part into kernel stacks, page
tables, per-CPU data, vmalloc and slab [CG-1 l.1577-1581]. The part that is not tracked is the reason a margin
below physical memory is needed at all; I found no number for its size (§10).

**The kernel documentation disagrees with itself on the role of `memory.high`.** Its "Usage Guidelines" still
say `memory.high` "is the main mechanism to control memory usage" and that "memory pressure monitoring
mechanism isn't implemented yet" [CG-1 l.1948-1966], although `memory.pressure` is documented just above
that section. The entry for `memory.max` says it is "the main mechanism to limit memory usage"
[CG-1 l.1407]. The 2023 commit that rewrote those entries touched only the two entries (10 insertions, 12
deletions) [CG-15]. The systemd manual still says "It is recommended to use MemoryHigh= as the main control
mechanism and use MemoryMax= as the last line of defense" [CG-37]. The newest kernel position (2023, acked by
Johannes Weiner and Chris Down) is that `memory.high` needs an external actor [CG-15].

### 2.3 How the throttle is implemented (from the source, not the documentation)

Read from [CG-3] (`mm/memcontrol.c`):

1. **Charge.** `try_charge_memcg()` charges the allocating task's cgroup. After a successful charge it walks
   from that cgroup **up through every ancestor**: if any has usage above `memory.high` (or swap above
   `memory.swap.high`), it adds the batch to `current->memcg_nr_pages_over_high` and sets `TIF_NOTIFY_RESUME`
   on the current task [l.2807-2838]. In interrupt context it only queues `high_work` [l.2816-2822].
2. **Return to userland** (`__mem_cgroup_handle_over_high`, l.2555-2643). The task first reclaims from its
   cgroup and from each ancestor that is over its high (`reclaim_high`, l.2378-2403). It then computes a
   penalty from the **largest overage over the cgroup and its ancestors** (`mem_find_max_overage`,
   l.2484-2496), scaled by "the task's own contribution to the overage, such that four N-sized allocations are
   throttled approximately the same as one 4N-sized allocation" (l.2539-2547). The penalty is clamped to
   `MEMCG_MAX_HIGH_DELAY_JIFFIES` = 2 s (l.2418, 2608), skipped if it is at most `HZ/100` (10 ms, l.2616), and
   reclaim is retried while it makes progress ("If reclaim is making forward progress but we're still over
   memory.high, we want to encourage that rather than doing allocator throttling", l.2619-2627). Only then
   does the task sleep with `schedule_timeout_killable()`, **inside `psi_memstall_enter()`/`leave()`**
   (l.2637-2639), so the sleep counts as a memory stall.
3. **Synchronously inside the kernel.** If a task has accumulated more than `MEMCG_CHARGE_BATCH` (64 pages,
   [CG-6 l.336]) over-high pages and the allocation may block, the same handler runs before returning
   [CG-3 l.2847-2850], added in 2022 because `mlock()` of a large region "in a single syscall" bypassed the
   limit and could "trigger the oom-killer" [CG-13].
4. **The delay curve** (l.2437-2464, K1-K3): `penalty = overage^2 x HZ >> 20 >> 14` with overage a 20-bit
   fixed-point ratio, so it depends only on r = (usage - high) / high, not on absolute size. The authors chose
   an exponential curve because "temporarily going over memory.high by a small amount may be perfectly
   legitimate" and because it ramps pressure statistics "gradually, which can be useful to work out that you
   have set memory.high too low" [CG-7].
5. **Dying tasks.** Dying tasks no longer enforce `memory.high` since 2024 after Tejun Heo found "zombie
   tasks that had were holding on to a lot of memory, had SIGKILL pending, but were stuck in memory.high
   reclaim" [CG-12; CG-3 l.2577-2578].
6. **Writing `memory.high`** below usage reclaims synchronously in the writer's context, retrying up to
   `MAX_RECLAIM_RETRIES`, unless the file was opened `O_NONBLOCK` [CG-3 l.4838-4889][CG-1 l.1391-1401]. A Hex
   agent that adjusts the parent's `memory.high` dynamically should open it `O_NONBLOCK`.

### 2.4 What the curve means for Hex's `high = M - max(15%, 8 GiB)` (computed here from K1)

The penalty is relative to `high`. For a reserve fraction f = 15% of M (hosts of about 53 GiB and up, where
15% is at least 8 GiB), usage = M gives r = 0.15 / 0.85 = 17.65%, where the 2 s clamp is reached; usage
= 0.95 M gives r = 11.8% and 0.89 s per batch; 0.90 M gives r = 5.9% and 0.22 s; 0.86 M gives r = 1.2% and
9 ms. So the throttle does not bite hard until the subtree holds most of the machine, and a single task is
held to about 4 KiB / r^2 per second: 30 MB/s at 0.86 M, 1.2 MB/s at 0.90 M, 0.30 MB/s at 0.95 M. Tasks
sleep independently, so 14 compiler processes or 100 test processes allocate 14 or 100 times that. On smaller
hosts (the 8 GiB branch) the clamp is reached earlier: 88% of M at 32 GiB. This is an upper bound from the
code, not a measurement of any workload (§9.4 asks for the measurement).

### 2.5 CPU: `cpu.weight` versus `cpu.max`

- **Weight.** `cpu.weight` is 1-10000, default 100, and "proportionally distributes CPU cycles to active
  children"; because only children that can use the resource take part, "this is work-conserving"
  [CG-1 l.664-669, l.679-680, l.1184-1196]. `cpu.idle` makes a cgroup `SCHED_IDLE`, "very low priority relative
  to its peers" [CG-1 l.1280-1288]. Each thread in the **root** cgroup competes as if in its own child of the
  root, weighted by its nice level [CG-1 l.3149-3159], so a process left in the root is not covered by the
  weight given to a runner cgroup.
- **Cap.** `cpu.max` (`$MAX $PERIOD`, default `max 100000`) is a bandwidth limit [CG-1 l.1214-1228]: "Once all
  quota has been assigned any additional requests for quota will result in those threads being throttled.
  Throttled threads will not be able to run again until the next period when the quota is replenished"; quota
  moves to per-CPU run queues in 5 ms slices by default; a group is also throttled when "a parent's quota is
  fully consumed within its period" [CG-53]. `cpu.stat` reports `nr_throttled` and `throttled_usec` for the
  cgroup's own limit only; `cpu.stat.local` includes throttling inherited from ancestors [CG-1 l.1143-1182].
- **Evidence on caps.** A 2019 kernel fix by an operator (Indeed): "highly-threaded, non-cpu-bound applications
  running under cpu.cfs_quota_us constraints can hit a high percentage of periods throttled while
  simultaneously not consuming the allocated amount of quota"; in an artificial test (10 ms of quota per 100 ms
  on an 80-CPU machine) the fix gave "almost 30x performance improvement" [CG-54] (`MEASURED-VENDOR`: the
  patch author's own test). Meta chose `cpu.weight`, "proportional control instead of maximum bandwidth limit"
  [CG-29][CG-28]. Twine's host profiles turn the cgroup2 CPU controller off for single-tenant machines "to
  eliminate its overhead" (no figure given) [CG-49 p.798].
- **Where weights are not enough.** Borg: "shares are insufficient because we have multiple priority levels",
  so the Borglet "dynamically adjusts the resource caps of greedy LS tasks ... selectively applying CFS
  bandwidth control", and Google ran a modified CFS (extended per-cgroup load history, preemption of batch
  tasks by latency-sensitive ones, a smaller quantum) because the standard CFS "required substantial tuning to support
  both low latency and high utilization" [CG-48 §6.2]. Their measurement: a runnable thread waited more than 5
  ms for a CPU only "a few percent of the time" and "almost never" longer, in a representative cell in December
  2013 (Fig. 13) [CG-48 §6.2] (`MEASURED-VENDOR`, a 2013 kernel and scheduler).
- **For load 30 on 14 cores (inference).** Weights give the runner and agent their proportional share; the
  kernel documentation does not promise a bound on how soon a woken task of a heavier cgroup runs, and I found
  no measurement of that bound on a current kernel (§10). `cpu.pressure` is the signal that would show it
  [CG-1 l.1240-1246].

### 2.6 `pids.max`

`pids.max` is a hard limit on tasks in the cgroup and its descendants; "PIDs used in this controller refer to
TIDs", so threads count; `fork()` and `clone()` return `-EAGAIN` at the limit, and migration or lowering the
limit can leave `pids.current` above `pids.max` [CG-1 l.2435-2492]. `pids.events` `max` counts hits
(hierarchical unless `pids_localevents` is mounted) [CG-1 l.249-253, l.2472-2484]; `pids.peak` records the
high-water mark [CG-1 l.2466-2470]. Kubernetes lets operators reserve process IDs for system daemons and warns
that enforcing system-reserved limits "can lead to critical system services being CPU starved, OOM killed, or
unable to fork on the node" [CG-51], so a tight `pids.max` on the protected domain would risk the same. A
cap size for CI jobs (rustc and cargo thread counts) is not in any source I read.

## 3. Pressure stall information (PSI)

- **Definition.** `some` is the share of time in which at least some tasks are stalled; `full` is the share in
  which all non-idle tasks are stalled at once, "actual CPU cycles are going to waste, and a workload that
  spends extended time in this state is considered to be thrashing" [CG-2 l.45-55]. Averages over 10, 60 and
  300 s plus a cumulative `total` in microseconds "to allow detection of latency spikes which wouldn't
  necessarily make a dent in the time averages" [CG-2 l.60-65]. CPU `full` at system level is reported as zero
  [CG-2 l.57-58]. Per-cgroup `cpu.pressure`, `memory.pressure`, `io.pressure` have the same format
  [CG-2 l.178-188]. TMO defines PSI as "the proportion of compute potential that is unproductive due to
  resource stalls" and lists what counts as a memory stall: reclaim on allocation, waiting for IO to refault
  recently evicted file pages, and swap-in; any wait on block IO counts as an IO stall [CG-21 §3.2.1, §3.2.3].
  The introducing commit gives the purpose: "it's hard to tell exactly the impact this has on workload
  productivity, or how close the system is to lockups and OOM kills", so PSI quantifies pressure "without
  sacrificing individual job health or risk complete machine lockups"; the kernel samples every 2 seconds, and
  the 10 s, 1 m and 5 m averages are running averages of those samples [CG-41].
- **Triggers** (`poll()` on an fd after writing e.g. `some 150000 1000000`): window 500 ms to 10 s, evaluated 10
  times per window while in a stall state, monitors stay active at least one window, notifications are
  rate-limited to one per window, one fd per trigger [CG-2 l.67-119][CG-42] (K22). The finest reaction is
  therefore about 50 ms evaluation inside a 500 ms window; the averages are 10 s and longer, refreshed every
  2 s [CG-41].
- **PSI accounting has a cost in deep trees.** `cgroup.pressure` exists because "PSI accounts stalls for each
  cgroup separately and aggregates it at each level of the hierarchy. This may cause non-negligible overhead
  for some workloads when under deep level of the hierarchy" [CG-1 l.1077-1081]. TMO says the cost is
  scheduling latency "since some logic needs to be performed on a context switch ... In real applications in
  our fleet the overhead is negligible" and that PSI is enabled by default on all major distributions
  including Android [CG-21 §3.2.2] (no figure published in that paper). Android's engineer, adding
  `cgroup_disable=pressure` in 2021: "psi_avgs_work has been highly optimized, however on systems with large
  number of cgroups the overhead becomes noticeable" [CG-43] (no figure either). Hex creates a cgroup per job,
  so this scales with job count; relevant to AC-T7.1.5.
- **Who uses it.** oomd and systemd-oomd [CG-31][CG-35]; Senpai [CG-21]; Meta's Async tier for load shedding,
  where `memory.full` spikes make the worker hand jobs back to the dispatcher: "memory overflows" decreased
  and throughput rose 25% [CG-26] (K16). TMO: "long before the kernel's out-of-memory killer triggers,
  applications can be functionally out of memory when the lack of it causes delays that prevent the
  application from meeting its SLO. Userspace out-of-memory killers can monitor full metrics and apply
  killing policies" [CG-21 §3.2.4].
- **The throttle is visible in PSI by design** [CG-7][CG-3 l.2637-2639] (§2.3). Commit b3ff92916af3 shows the
  reverse failure: PSI `io.pressure` near zero and `memory.pressure` at 78.5% while the cgroup was throttled
  despite slack cache [CG-9] (K6), so a monitor must read `memory.pressure` and `memory.events`, not IO
  pressure.

## 4. IOCost (Heo et al., ASPLOS 2022) and the other IO controls

**Design** [CG-20 §3]: an offline per-device linear cost model (base cost for sequential/random x read/write,
plus a per-byte rate) estimates each IO's device occupancy; a per-IO issue path (microsecond scale) throttles
by comparing each cgroup's virtual time with a global clock; a periodic planning path (millisecond scale) adjusts
the clock rate `vrate` against device saturation (a latency-percentile target, e.g. "90th percentile read
completion latency is above 10 ms") and donates unused budget to busy siblings; swap-out and journaling IO is
issued without throttling but charged as debt to the originating cgroup, which is blocked before returning to
userspace if the debt grows, "without causing priority inversions" [CG-20 §3.5]. QoS parameters were tuned with
ResourceControlBench against a memory-leak scenario [CG-20 §3.4].

**Results** (all `MEASURED-VENDOR`, Meta; K18-K20): see §1. Low overhead: on a 750K-IOPS SSD the paper says
BFQ "has severe software overheads" and that the other controllers (blk-throttle, IOLatency, IOCost) "add no
significant overhead" (Fig. 9; no numbers in the text) [CG-20 §4.1]. Proportional control: for a 2:1 weight target BFQ gave the high-priority
workload more than 10:1, IOLatency "roughly 10:1", IOCost 2:1 [CG-20 §4.2].

**Lessons relevant to Hex** [CG-20 §5, §2.1, §4.8]:
- "Memory control alone was insufficient as memory limits still resulted in reclaim which interfered with
  latency-sensitive applications through IO. We could achieve comprehensive isolation only by doing both
  memory and IO controls together."
- Meta's hierarchy (Fig. 1) has `system` (chef and other auxiliary services), `host critical` ("the processes
  required to keep the host running, e.g., sshd and the container management agent") and `workload`. The
  container agent depends on a system service for package fetches, and "communication between the two services
  fails due to the system service getting starved for IO"; failures there "often result in the machine being
  taken out of production entirely" [CG-20 §4.8]. Hex's runner and agent have such dependencies too (fetching,
  logging, kiln's API): the protected set must include what the agent calls (fbtax2 puts such helpers in
  `workload-support.slice` [CG-23]).
- Configuration of the earlier `io.latency` was "nearly unmanageable" for diverse devices and "a complex
  function of both the heterogeneous-device properties and dynamic-application properties" [CG-20 §2.2, §5].
- Device behaviour matters: "SSDs with more consistent behaviors ... could be effectively utilized" [CG-20 §5].

**Kernel interfaces** [CG-1]: `io.weight` is "the relative amount IO time the cgroup can use in relation to its
siblings" (l.2143-2161); its proportional control is the iocost model controller or BFQ's own cgroup support
(l.2012-2020); `io.cost.qos` is "disabled by default and can be enabled by setting" `enable` to 1 (l.2069-2070),
with `ctrl=auto` letting the kernel adjust parameters and `ctrl=user` freezing them (l.2095-2099, l.2118-2121);
"the lower the saturation point, the better the latency QoS at the cost of aggregate bandwidth" (l.2084-2086);
the cost model "isn't expected to be accurate in absolute sense and is scaled to the device behavior
dynamically" (l.2137-2138). `io.latency` throttles peers with a higher target when a group misses its own, by
queue-depth clamping (down to 1 IO) and by delaying swap and metadata IO charged to the offending group, each
delay event capped at 1 s (l.2269-2326). Cgroup-aware writeback exists only on ext2, ext4, btrfs, f2fs and xfs;
elsewhere all writeback is attributed to the root cgroup, and several cgroups dirtying one inode "are not
supported well" (l.2226-2253). Hex needs the filesystem and device types of its runners to know whether
`io.weight` will bind (§10).

## 5. TMO and Senpai (Weiner et al., ASPLOS 2022)

TMO is Meta's memory offloading: PSI plus a userspace agent, Senpai, that keeps `some` memory pressure near a
target by asking the kernel to reclaim [CG-21]. For machine protection its contributions are evidence and a
control pattern, not a protection mechanism:

- **Scale and cost** (K21): 20-32% of total memory saved across millions of servers; Senpai's reclaim is 0.05%
  of all CPU cycles; a single global configuration (`reclaim_ratio` 0.0005, `PSI_threshold` 0.1%, every 6 s,
  at most 1% of the workload per period) works across applications, so "reaction time to extreme contraction
  tends to be minutes" while "adaptation to workload expansion ... is immediate" [CG-21 §3.4].
- **A stateful memory limit is a hazard for growing workloads.** Senpai's first version adjusted the cgroup's
  memory limit; "if a container is under rapid memory growth and its size is actively expanding, it may become
  blocked until Senpai can raise its limit further". They added the stateless `memory.reclaim` file instead
  "without applying any limit, thus avoiding the risk of blocking expanding workloads" [CG-21 §3.3]. A static
  `memory.high` on the CI parent has the same property for a build that legitimately grows (link steps).
- **PSI `some` and `full` have different uses**: TMO uses `some` to keep contention "low but non-zero", and
  `full` for "unacceptable losses of productivity that require immediate remediation" [CG-21 §3.2.4, §3.3].
- **Swap and zswap**: with PSI feedback, swap or zswap backends need no application change; the Web
  application compresses 4x so zswap saved about 13% of its memory while SSD offload saved about 4% because it
  is "sensitive to memory-access slowdown" [CG-21 §4.2]. The kernel doc adds zswap controls (§2.2). No source
  here measures `memory.high` throttling together with zswap (§10).

## 6. Userspace killers: oomd, systemd-oomd, and the CI case study

- **Why a userspace killer.** oomd's overview: kernel OOM handling "is typically a slow and painful process
  because the kernel spends an unbounded amount of time swapping in and out pages and evicting the page cache"
  [CG-31]; Meta lists four failings of the kernel killer [CG-26]: it starts only "after failing at multiple
  attempts to allocate memory"; it picks victims with "primitive heuristics, typically killing whichever one
  frees the most memory"; it "can fail to start at all when the system is thrashing: memory utilization remains
  within normal limits, but workloads don't make progress, and the OOM killer never gets invoked"; and, lacking
  context, it "can even kill vital system processes: When this happens, the system is lost, and the only
  solution is to reboot, losing whatever was running, and taking tens of minutes to restore the host".
- **Sandcastle** [CG-30]: Meta's build/test/land system runs worker tasks, each repeatedly fetching build jobs
  from a queue; the kernel OOM killer "would kill just one of a build job's processes", leaving the job "in an
  undefined state" so "all running jobs must be canceled and retried, taking nearly an hour before the host is
  up again". The fix:
  "The worker contains each build job it runs inside its own sub-cgroup", oomd watches the per-job cgroups and
  "kills the entire cgroup and all its processes at once", a per-job mount namespace and tmpfs frees memory on
  kill. Result (K15): reboots 5% to under 0.5% per day, downtime from tens of minutes to seconds.
  **Not stated** in the case study: the thresholds, whether `memory.high` was used, the host sizes, the
  baseline kernel. The mapping to Hex is direct (per-job cgroup, whole-cgroup kill) and is the strongest
  single piece of evidence in this note for `memory.oom.group` and `cgroup.kill` per job [CG-1 l.1051-1064,
  l.1477-1493].
- **Defaults and examples** (K23-K25). systemd-oomd samples PSI periodically, kills "all processes" of the
  chosen cgroup with `SIGKILL`, considers only descendants of the monitored unit, and only leaf cgroups or
  cgroups with `memory.oom.group` set [CG-35]. Defaults: kill when the monitored cgroup's `full avg10`
  exceeds 60% for 30 s, or when memory and swap are both above 90% used [CG-36]. The manual says processes
  that "can usually ride out slowdowns" (system.slice) tolerate 60%, while an interactive manager "may prefer a
  much lower value like 40%" [CG-35]. oomd's example config uses 5% / 40% for 15 s and 80% for 30 s [CG-34].
  None of these is a measured optimum; the 30 s default alone is long if a host's liveness probe fails sooner.
- **The kernel will not rescue the host, by its memory-management developers' own account.** On LKML on
  2026-03-03, answering a production report, Shakeel Butt wrote: "Over the time we (kernel MM community) have implicitly decided to
  keep the kernel oom-killer very conservative as adding more heuristics in the reclaim/oom path makes the
  kernel more unreliable and punt the aggressiveness of oom-killing to the userspace as a policy. All major
  Linux deployments have started using userspace oom-killers like systemd-oomd, Android's LMKD, fb-oomd or
  some internal alternatives" [CG-44] (`DESIGN`: a memory-controller developer's statement of the community's
  policy). The report he answered:
  "Systems with zram-only swap can spin in direct reclaim for 20-30 minutes without ever invoking the OOM
  killer. We've hit this repeatedly in production on machines with 377 GiB RAM and a 377 GiB zram device"
  [CG-44]; a follow-up trace of one node over 5 minutes shows 141 `should_reclaim_retry` calls, every one with
  `no_progress_loops` = 0 and the watermark check passing, a "reclaimable" estimate of 19-21 GiB, anonymous
  reclaim of 52M of 244M scanned pages (21%) with 53% of scan events reclaiming nothing, and file reclaim of
  33M of 42M (78%) [CG-45] (K28; `MEASURED-INDEPENDENT`, an operator's bpftrace on production nodes; the
  brownout itself was not captured). Chris Down names the reporter as Cloudflare [CG-47]. Hex's hosts may not
  use zram, but the mechanism is the class the incident belongs to: reclaim reports nominal progress while the
  host browns out and the kernel's OOM path never runs. Down: the cycle before the OOM killer "can take seconds
  or even minutes for simple production workloads" [CG-47] (`CLAIMED`, no data).
- **Swap: the sources disagree on its effect on time-to-kill, and agree the kernel killer is too late.**
  systemd-oomd: "highly recommended" to have swap, because "with swap enabled, the system spends enough time
  swapping pages to let systemd-oomd react. Without swap, the system enters a livelocked state much more
  quickly" [CG-35]; oomd's production guide: swap "can also give oomd more time to act" [CG-33]; Meta enabled
  swap everywhere except the main workload, with `memory.swap.max` = 0 there [CG-25][CG-29]. Chris Down (2018,
  updated 2019): "Disabling swap does not prevent disk I/O from becoming a problem under memory contention.
  Instead, it simply shifts the disk I/O thrashing from anonymous pages to file pages"; without swap, "The OOM
  killer is triggered more quickly as anonymous pages are locked into memory ... the time between thrashing
  and OOMing is reduced. Depending on your application, this may be better or worse ... That said, this is
  still too late to be really useful"; and
  "You need to opportunistically handle the situation yourself before ever thinking about the OOM killer"
  [CG-46] (`CLAIMED`: reasoning by a memory-controller author, no measurement). In 2026 he adds that zswap is
  preferable to zram on servers because zram's memory "is not charged to any cgroup, breaking isolation
  semantics", and that `memory.zswap.writeback` = 0 avoids disk swap per cgroup (kernel 6.8 or later, his
  statement) but can make reclaimers loop on incompressible pages [CG-47][CG-1 l.1926-1931]. No source here
  measures any of this for a CI host (§10).
- **The killer must itself be protected and must get CPU.** oomd runs in `hostcritical.slice` with `MemoryMin=`
  and `MemoryLow=` (64M suggested) [CG-33]. The third of oomd's three roles in fbtax2 is "Helping when
  kernel resource isolation starts breaking down ... for example, due to priority inversions through certain
  locking constructs" [CG-29]. Butt: "userspace oom-killers are prone to reliability issues (oom-killer getting
  stuck in reclaim or not getting enough CPU)", which is why a BPF OOM killer is being developed [CG-44].

## 7. How large systems use these controls

### 7.1 Meta (fbtax2 and IOCost)

- **Hierarchy** [CG-23][CG-20 Fig. 1]: `system.slice`, `hostcritical.slice` (sshd, oomd, the container agent),
  `workload.slice` with `workload-container.slice` and `workload-support.slice`. The project's goal was "to
  isolate and protect a system's main workload from widely distributed system binaries and other system
  services that run on many Facebook hosts" [CG-22]; "Protecting the main workload from resource conflicts with
  system binaries was a primary goal" [CG-23]. Hex's roles are reversed: the limited side is the workload (CI
  jobs) and the protected side is the infrastructure (runner, agent). The mechanism Meta tried first and
  dropped, `memory.high` on the limited side, is the same as Hex's; the reason Meta gave (restricting memory
  "made them more prone to thrashing and OOMs" and "some of these system.slice processes are critical to the
  main workload") transfers only to the extent a throttled CI job cannot simply be failed and retried.
- **Memory** [CG-25][CG-29]: first `memory.high` on `system.slice` (abandoned, see Summary 4), then `memory.low`
  for `workload.slice` (19G of about 32G) and a small `memory.low`+`memory.min` for `hostcritical.slice`
  (192M), because "the hard guarantee offered by memory.min is required to protect hostcritical.slice";
  `memory.min`/`memory.low` "are work-conserving ... proportionally regulate memory reclaim ... a lot easier to
  use ... since the configurations have gradual effects and can be ballparked" (the 3 GiB vs 3 GiB example:
  the unprotected peer gets 3x the reclaim pressure) [CG-29]. Working-set sizing: read `memory.current` under
  moderate pressure on many machines and average [CG-25].
- **Swap and oomd** [CG-25][CG-29]: swap everywhere except the main workload, oomd resolving overcommit.
- **IO**: `io.latency` on the top-level slices only (50 ms / 75 ms) for fbtax2; IOCost afterward [CG-27][CG-20].
- **CPU**: `cpu.weight`, "proportional control instead of maximum bandwidth limit (which is set in cpu.max)"
  [CG-29]. For Laser (dozens of colocated instances per host), `cpu.weight` replaced one instance "dominating
  the CPU and starving other threads"; the evidence is a latency chart without numbers [CG-28].
- **Stated limits of the result** [CG-29]: dips in requests per second remain; "The kernel virtual memory
  management can't know which pages are hot without causing faults"; "cgroup2 controllers have inherent and
  implementation-specific limitations and there still are remaining priority inversion issues which can
  transfer high resource pressure in system.slice to workload.slice"; most dips were "caused by, or greatly
  exaggerated by, the slow performance of hard disks"; SSD runs were better.

### 7.2 Kubernetes MemoryQoS (KEP-2570)

- **Mapping** [CG-38]: container `memory.min` (Guaranteed) or `memory.low` (Burstable) from requests;
  `memory.high` from a throttling factor; `memory.max` from limits; kube-reserved and system-reserved cgroups
  get `memory.min`; the `kubepods` parent gets `memory.min` = sum of children's protection "so the parent covers
  both hard and soft protection for its children".
- **History**: alpha v1.22 with factor 0.8 (early throttling at 80% of the limit), reworked in v1.27 to 0.9 of
  the request-limit gap; beta in v1.28 was cancelled over a livelock: "once the memory.high level is reached, it
  doesn't progress further and stays stuck indefinitely ... the process rapidly consumes the memory reclaimed
  by the kernel"; the fix is kernel commit b3ff92916af3 ("5.9+ (October 2020)") [CG-38][CG-9]. v1.36 adds a
  kernel-version warning and tiered reservation; **v1.37 changes the default of `memoryThrottlingFactor` from
  0.9 to nil, so `memory.high` is off unless the operator opts in** [CG-38 "Beta v1.37"].
- **Known failure modes listed by the KEP** [CG-38]: livelock below 5.9; "memory.min protection ineffective
  ... Verify parent cgroup has memory.min set"; "Node-wide OOM under memoryReservationPolicy: Hard" where
  page cache cannot be reclaimed; `memory.min` = `memory.max` makes page-cache-heavy Guaranteed pods OOM (K10).
- **Benchmarks** (K7-K10, `MEASURED-INDEPENDENT`, single nodes, run by individual GitHub contributors rather
  than a lab):
  a throttled aggressor leaves protected pods untouched (K8); a node with 90.6% `memory.min` stays responsive
  (K9); throttling alone never kills (K8). The authors' reading: "The gap between `memory.high` and `memory.max` ...
  determines the outcome" [CG-40 §6].

### 7.3 Borg, Twine and Kubernetes' node-level reserves

- **Borg (EuroSys 2015)** [CG-48]. Memory is "non-compressible" and CPU and disk bandwidth "compressible": "If a
  machine runs out of non-compressible resources, the Borglet immediately terminates tasks, from lowest to
  highest priority, until the remaining reservations can be met. If the machine runs out of compressible
  resources, the Borglet throttles usage (favoring LS tasks)". A Borglet user-space control loop "assigns
  memory to containers based on predicted future usage (for prod tasks) or on memory pressure (for non-prod
  ones); handles Out-of-Memory (OOM) events from the kernel; and kills tasks when they try to allocate beyond
  their memory limits, or when an over-committed machine actually runs out of memory"; "Linux's eager
  file-caching significantly complicates the implementation because of the need for accurate memory-accounting"
  (§6.2). A task over its memory limit "will be the first to be preempted if resources are needed, regardless
  of its priority" (§5.5). History: early Borglets used post-hoc usage checks and killing, and "it was still
  too easy for rogue tasks to affect the performance of other tasks on the machine"; users inflated requests and
  "in the most extreme cases, users petitioned to use dedicated machines or cells"; once every task ran in a
  cgroup-based container the Borglet gave "much improved control because the OS kernel is in the loop", though
  "occasional low-level resource interference (e.g., memory bandwidth or L3 cache pollution) still happens"
  (§6.2). Placement comes first: prod tasks are admitted by their limits "so they never rely on reclaimed
  resources and aren't exposed to resource oversubscription", and when a machine runs out "we kill or throttle
  nonprod tasks, never prod ones" (§5.5). Reading for Hex: Borg treats memory as kill-not-throttle (the
  opposite of a `memory.high`-first design), keeps the kill loop in userspace beside the kernel's OOM events,
  and protects primarily by admission.
- **Twine (OSDI 2020)** [CG-49]. "Twine does not overcommit CPU or memory by default, although a job owner can
  explicitly configure their job to do so" (p.800). Its fleet uses "millions of small machines", each with "one
  18-core CPU and 64GB RAM" (p.792). Host profiles can set kernel versions, sysctls and "cgroupv2 (e.g., CPU
  controller)" (p.792); for services that fully use a machine and do not stack, Twine disables the cgroup2 CPU
  controller "to eliminate its overhead" (p.798, no figure). One lesson "based on lessons from past incidents":
  `net.ipv4.tcp_mem` tuned "to alleviate TCP's memory pressure under high loads in order to prevent cascading
  failures" (p.796). The paper gives no `memory.low` or `memory.high` settings: not found.
- **Kubernetes node-level reserves** [CG-50][CG-51][CG-52]. "Memory pressure at the node level leads to System
  OOMs which affects the entire node and all pods running on it. Nodes can go offline temporarily until memory
  has been reclaimed" [CG-51], the incident's class. The kubelet's default hard eviction is
  `memory.available<100Mi` on Linux, evaluated every `housekeeping-interval` (default 10 s); "If memory usage
  increases within that window rapidly, the kubelet may not observe MemoryPressure fast enough, and the OOM
  killer will still be invoked", with `--kernel-memcg-notification` for immediate notification and
  `--kube-reserved`/`--system-reserved` as the workaround [CG-50] (K32). The documented worked example reserves
  10% of a 10 GiB node for system daemons plus the eviction threshold (`--system-reserved=memory=1.5Gi` with
  `--eviction-hard=memory.available<500Mi`) [CG-50] (an example, not a measurement). `systemReserved` "should
  reserve memory for the kernel too since kernel memory is not accounted to pods in Kubernetes at this time"
  [CG-51]. The kubelet counts `active_file` as non-reclaimable and "is liable to observe this as high resource
  use and taint the node as experiencing memory pressure" [CG-50]: a monitor that reads usage including page
  cache over-reports, which PSI avoids. Enforcing the reserved cgroups is discouraged without profiling [CG-51]
  (§2.6). The Pod QoS page confirms memory throttling and protection are off by default in v1.37 and that
  "Without this ancestor coverage, the per-Pod and per-container protection would be ineffective" [CG-52].

## 8. Failure-mode catalogue

| # | Failure mode | Mechanism and evidence | Kind | Mitigation shown |
|---|---|---|---|---|
| F1 | Throttle without kill: jobs hang, host lives | `memory.high` never OOMs; K8 shows aggressors throttled for 10 min with 8-14 GiB limits; [CG-15] "simply leads to stalls" | DESIGN; MEASURED-INDEPENDENT | `memory.max` on the parent, or PSI-driven kill of the job cgroup [CG-30][CG-36] |
| F2 | Protected tasks inside the limited subtree are throttled | charge path walks ancestors; max overage over the ancestry [CG-3 l.2807-2838, l.2484-2496][CG-8]; KEP puts `memory.high` on containers, not pods [CG-38] | DESIGN | runner and agent outside the subtree [CG-23] |
| F3 | Throttled although the cgroup has slack file cache | one reclaim walk per return to userland did not keep up with growing cache; fixed by retrying reclaim (K6) [CG-9]; livelock on kernels before 5.9 [CG-38] | MEASURED-VENDOR | kernel 5.9 or later; check `memory.events` `high` rate |
| F4 | Priority inversion and deadlock from reclaim while holding locks | 2015: "a low-prio task ... might perform over-high reclaim with a bunch of locks held. If a higher prio task needed any of these locks, it would have to wait" [CG-10]; 2023: btrfs lock-order deadlock when the synchronous path ignored `gfp_mask` [CG-11]; 2024: zombies stuck in `memory.high` reclaim [CG-12]; Meta: "priority inversions through certain locking constructs" [CG-29] | DESIGN (commit messages) | newer kernels; a killer that does not depend on throttled tasks |
| F5 | Large single allocations jump the limit | `mlock()` of a large chunk in one syscall bypassed `memory.high` and could invoke the OOM killer (before 2022) [CG-13] | DESIGN | a kernel with c9afe31ec443 (2022) and its follow-up 9ea9cb00a82b (2023) [CG-11][CG-13]; the first release carrying them is not established here |
| F6 | `memory.low` does not protect when nothing else is reclaimable | skipped only while "there is an unprotected supply of reclaimable memory" [CG-5 l.6203-6209]; on failure, `memcg_low_reclaim` retries over protected cgroups [CG-5 l.6632-6639]; "useless against ... slow memory leaks ... especially true for swapless systems" [CG-16] | DESIGN | `memory.min` for the small critical set [CG-25][CG-16] |
| F7 | `memory.low` / `memory.min` ineffective because an ancestor is 0 | effective protection capped by the parent's [CG-4 l.300-307]; KEP failure mode "memory.min protection ineffective" [CG-38]; systemd: "set a corresponding allocation on all ancestors" [CG-37] | DESIGN | protection on every ancestor, or `memory_recursiveprot` [CG-1 l.210-218][CG-19] |
| F8 | Proportional low-protection reclaim caused OOMs | "occasional OOM killing when memory.low settings are in effect" (2021) [CG-18]; the earlier cliff at the boundary, fixed 2019 [CG-17] | MEASURED-VENDOR (report) | kernels with both fixes |
| F9 | `memory.min` over-commit or `min` = `max` | "may lead to constant OOMs" [CG-1 l.1351-1352]; K10 (OOM in the first iteration) | MEASURED-INDEPENDENT | keep `memory.min` small and below `memory.max` headroom [CG-25] |
| F10 | Memory pressure becomes IO pressure and starves the protected set | refaults and swap IO [CG-24][CG-20 §5]; agent starved of IO while fetching [CG-20 §4.8] | MEASURED-VENDOR | IO control with memory control together (K18-K20) |
| F11 | No swap shortens the time a killer gets | systemd-oomd manual [CG-35]; Facebook 2019, before the `memory.high` throttle existed: "containment can frequently fail completely and cause OOM situations outside of the cgroup ... especially with swap space -- either when none is configured, or swap is full" [CG-7] | DESIGN; MEASURED-VENDOR | swap or zswap, decided by measurement (§10) |
| F12 | Kernel OOM kills one process of a job | Sandcastle [CG-30] | MEASURED-VENDOR | `memory.oom.group` per job; `cgroup.kill` |
| F13 | Writeback not attributed to the job | non-supporting filesystems write back as root [CG-1 l.2226-2229] | DESIGN | check filesystem; `io.weight` cannot bind to that IO |
| F14 | Unaccounted kernel memory | "not completely water-tight" [CG-1 l.1303-1306] | DESIGN | reserve below physical memory (size unknown) |
| F15 | PSI cost at depth | `cgroup.pressure` rationale [CG-1 l.1077-1081] | DESIGN | disable PSI in non-leaf cgroups if measured to cost |
| F16 | The adjusting agent blocks in reclaim | `memory.high` write reclaims synchronously unless `O_NONBLOCK` [CG-3 l.4838-4889][CG-1 l.1391-1401] | DESIGN | open `O_NONBLOCK` |
| F17 | `pids.max` counts threads and cannot stop already-attached tasks | "PIDs used in this controller refer to TIDs"; limit may be set below current; migration can exceed it; fork/clone return `-EAGAIN` [CG-1 l.2447-2448, l.2486-2492] | DESIGN | size by observed `pids.peak` |
| F18 | The kernel OOM killer does not run while the host browns out | 20-30 minute brownouts with `no_progress_loops` = 0 on every retry (K28) [CG-44][CG-45]; kernel MM developers keep the kernel killer "very conservative" and leave policy to userspace [CG-44]; Meta: the kernel killer "can fail to start at all when the system is thrashing" [CG-26] | MEASURED-INDEPENDENT; DESIGN | a PSI-driven userspace killer in the protected domain (§9.5) |
| F19 | The userspace killer is itself starved | "prone to reliability issues (oom-killer getting stuck in reclaim or not getting enough CPU)" [CG-44]; oomd is run with `MemoryMin=` in `hostcritical.slice` [CG-33] | DESIGN | protect its memory and give its cgroup CPU weight; measure its reaction time |
| F20 | `cpu.max` throttles without using the quota | highly threaded, non-CPU-bound groups hit throttling while under quota (fixed 2019, 30x on an artificial test) [CG-54]; throttling inherited from a parent [CG-53] | MEASURED-VENDOR | use `cpu.weight` for fair share; reserve `cpu.max` for hard caps and watch `nr_throttled` |
| F21 | A usage-based monitor over-reports page cache | the kubelet counts `active_file` as non-reclaimable and can taint the node [CG-50]; Borg: "Linux's eager file-caching significantly complicates ... memory-accounting" [CG-48 §6.2] | DESIGN | drive kills from PSI, not from `memory.current` |

## 9. Implications for Hex's machine-protection design

Everything under this heading that is not a citation is an inference from the cited evidence ("DESIGN" in the
sense of a proposal, to be falsified in §9.4); none has been measured on Hex.

### 9.1 What supports the proposed design

1. **Separating the CI domain from the host-critical domain, with the critical processes outside the limited
   subtree**, is how both Meta and Kubernetes lay out their trees [CG-23][CG-20 Fig. 1][CG-38].
2. **A ceiling below physical memory plus the throttle composes with monitors**: the throttle does not kill
   (appropriate for slow-but-legitimate builds), its delays are visible as PSI [CG-7][CG-3 l.2637-2639], and
   the kernel reclaims first [CG-3 l.2619-2627]. In the Kubernetes benchmark a throttled aggressor left the
   protected pods untouched for 10 minutes (K8).
3. **`memory.low` / `memory.min` for the runner and agent** is Meta's host-critical practice (192M of
   each on a host of about 32G, K12), oomd's (64M, K25), and Kubernetes' for kube-reserved/system-reserved [CG-38].
   The Kubernetes node test shows a node stays responsive with 90.6% of memory under `memory.min` while an
   unprotected aggressor is killed (K9).
4. **Proportional, work-conserving CPU and IO** (`cpu.weight`, `io.weight`) are what Meta chose over caps
   [CG-29][CG-20 §3] and what the kernel documents as work-conserving [CG-1 l.664-669]: idle share is not
   wasted, which matters for AC-T7.1.2-style noisy-neighbour targets.
5. **`pids.max` per job** targets a failure no other controller prevents: "a fork bomb is likely to exhaust the
   number of tasks before hitting memory restrictions" [CG-1 l.2442-2445].
6. **Per-job cgroups with whole-cgroup kill** is the Sandcastle result (K15), the nearest CI evidence.

### 9.2 What argues against it, or against relying on it alone

1. **`memory.high` is not a protection by itself** (Summary 1, F1): the 2023 kernel documentation change, the
   Kubernetes benchmark (K8), and the kernel doc's own "external process monitors" sentence. The Kubernetes
   project then made `memory.high` opt-in by default in v1.37 [CG-38]; Meta did not ship it for `system.slice`
   [CG-25].
2. **A parent-level `memory.high` throttles innocent jobs and the agent if they share the subtree** (Summary 2,
   F2). Between jobs: one runaway job pushes the parent over `high`; reclaim then runs in whichever task
   allocates next, over the whole subtree, and every allocating task in it gets a sleep proportional to its own
   recent allocation [CG-3 l.2539-2547]. Per-job `memory.low` inside the parent shapes which job reclaim takes
   from, because the protection is evaluated relative to the reclaim target [CG-1 l.1971-1988]. Nothing I
   read measures between-job fairness under a parent `memory.high`.
3. **The throttle is a brake whose strength is relative to `high`, so it is weak just above `high` and
   strongest only near all of RAM on large hosts** (§2.4, K3, K4). Computed here on a 64 GiB host (high 54.4
   GiB): from 0.85 M to 0.88 M each task is still allowed more than 3 MB/s (so 100 tasks add 300 MB/s); between
   0.90 M and 0.95 M (3.2 GiB) each task is held to 1.2 to 0.3 MB/s, so 100 allocating tasks need about 68 s
   and 14 tasks about 8 minutes to cross it (integrating the per-task rate bound, T = high x (r2^3 - r1^3) /
   (3 x N x 4 KiB)). That is time for a monitor to act, if one exists; with none, a persistent allocator still
   reaches the reserve. The effective reserve against a fast allocator is therefore nearer 10% than 15%
   (inference from the code constants; not measured).
4. **The 15% / 8 GiB constant has no evidence.** Meta sizes by measured working set [CG-25]; Senpai by PSI
   [CG-21]; Kubernetes by requests [CG-38]. A fixed fraction does not distinguish reclaimable page cache from
   anonymous memory, though both count toward `memory.current` [CG-1 l.1297-1314], and kernel memory is only
   partly tracked [CG-1 l.1303-1306]. Not found: any published reserve for a CI host.
5. **`memory.low` fails when the host has no reclaimable memory elsewhere** (F6): a CI parent dominated by
   anonymous memory (rustc, linkers) on a host without swap is such a regime; whether the incident was one is
   unknown (no logs read).
6. **A static limit is wrong for a growing workload** (TMO's reason for `memory.reclaim` over limits [CG-21
   §3.3]); builds have link-time spikes.
7. **Priority-inversion residue**: even Meta's full design reports remaining inversions [CG-29]; a throttled
   job holding a lock the agent needs (a build-directory lock, a shared filesystem lock) can stall the agent
   outside the subtree. Whether the agent's own paths take locks that throttled jobs hold is unmeasured
   (hypothesis, test in §9.4).
8. **IO is a second channel through which memory pressure reaches the host** (F10): a memory-only fix leaves
   refault and writeback IO storms that can starve the agent's page-ins; Meta's IO conclusion (K18-K20)
   applies, and `io.weight` is inert unless iocost or BFQ is enabled for the device [CG-1 l.2012-2020,
   l.2069-2070].

### 9.3 Failure modes of the proposal as stated, in order of likelihood from the evidence

1. Jobs stall at `memory.high` and the CI queue backs up while the host stays up (F1): needs a kill policy and
   an alarm on `memory.events` `high`.
2. The runner and agent are throttled or reclaimed because they share an ancestor with `memory.high` or an
   ancestor has no `memory.low` (F2, F7).
3. The host still thrashes because the swapless anonymous working set leaves nothing for reclaim to take but
   the protected set's executable pages (F6, F10).
4. A kernel older than 5.9 livelocks at `memory.high` (F3).
5. `io.weight` set but not applied (iocost disabled, unsupported filesystem) (F13).
6. Kill chooses one process of a job instead of the job (F12).

### 9.4 What to measure on Hex's own hardware (CLAUDE.md §4, §7)

On fedora or jupiter, not adacachy (production); one slot on kiln's bench calendar; load average under 2 and
nothing else building before each run; best of N recorded in `bench/`. Extend `bench noisy_neighbor`
(AC-T7.1.2) with a memory axis. Compare, on the same kernel and disks:
- configs: (a) no cgroups; (b) the proposal as stated; (c) (b) plus `memory.max` above `high`; (d) (c) plus
  `memory.oom.group` per job; (e) (d) plus `memory.min` on the protected set with ancestor protection; (f)
  (e) plus iocost; (g) (e) plus a PSI-driven killer; each with swap off, zram/zswap, and disk swap;
- loads: a kiln build at `-j14` (the incident); the same plus a 10, 50 and 100 MB/s anonymous leak up to the
  host's size (the fbtax2 tests, K13); a fork storm for `pids.max`; a large-file writeback storm for
  `io.weight`;
- outputs: liveness-probe latency p50, p99, p99.9 and max of a probe served by the agent, the runner's
  heartbeat gap, time to recovery, jobs killed and jobs completed per hour, `memory.events` (`low`, `high`,
  `max`, `oom_kill`), `memory.pressure` and `io.pressure` `full`/`some` on the CI parent and system-wide,
  `cpu.stat` `nr_throttled`, `pids.peak`, `memory.current` and `memory.stat` (anon, file, kernel, slab,
  pagetables) per job, and the agent's memory per box (AC-T7.1.5) with PSI accounting on and off at the depth
  Hex uses;
- environment facts to record: kernel version and config (`PSI`, iocost, `memory_recursiveprot`), swap/zswap,
  filesystem types of the build and cache directories, device type and scheduler.
Falsifiers: if configuration (b) keeps the probe's p99.9 within the AC-T7.1.2 bound under every load above and
recovers without a kill, the backstops are unnecessary for that load; if (b) shows any run where jobs stall
longer than the CI timeout or the probe's maximum exceeds the liveness window, §9.2 item 1 stands.

### 9.5 Candidate shape the evidence points to (a proposal, to be tested, not a decision)

- Two sibling domains under a Hex root: **host-critical** (runner, agent, killer, sshd-class helpers and what
  the agent calls) and **CI** (the parent with per-job leaves). Protections on every ancestor of the critical
  set, or `memory_recursiveprot` mounted [CG-1 l.210-218][CG-4 l.300-307]; a small `memory.min` plus a larger
  `memory.low` (Meta: 192M, oomd: 64M; to be sized by measurement, K12, K25).
- On the CI parent: `memory.high` as proposed or from PSI feedback, with a **`memory.max` above it** so that
  OOM, if it comes, is local ("not going to kill any tasks outside of this cgroup" [CG-1 l.1491-1493]);
  `cpu.weight` and `io.weight` below the critical set's; iocost enabled and tuned on the data devices, or
  `io.latency` where iocost is unavailable [CG-20][CG-29].
- Per job: its own leaf with `memory.oom.group` = 1, `pids.max`, equal weights; the whole job killed together
  [CG-30][CG-1 l.1477-1486].
- A **PSI-driven killer in the critical domain**, using `cgroup.kill` on the job cgroup chosen by recent
  reclaim activity (systemd-oomd's `kill-by-pgscan` policy [CG-36]), triggered well before the liveness
  window; thresholds measured, not copied from K23-K24. The kernel will not do this (F18) and the killer can
  starve (F19), so it gets `memory.min`, CPU weight and a bounded work loop (CLAUDE.md §2, §11), and its
  reaction time is a measured acceptance number. Whether to use a separate process (oomd, systemd-oomd) or
  an in-agent loop is open: no source compares them.
- **Swap policy decided by measurement**, not by default: swap-less (fast kill, more thrash on file pages),
  disk swap, or zswap with disk-backed swap (Chris Down's recommendation; zram is not charged to a cgroup)
  [CG-46][CG-47][CG-35] (F11).
- **Placement first**: the scheduler admits CI legs by memory (AC-T7.3.1) so that the sum of requested memory
  stays within what the machine can give after the protected domain and kernel overhead, as Borg, Twine and
  Kubernetes do [CG-48][CG-49][CG-38]; the cgroup caps catch what admission misses.
- Keep the agent's own writes to `memory.high` `O_NONBLOCK` (F16), and require at least kernel 5.9 (the
  Kubernetes floor [CG-38]) plus the 2022-2024 `memory.high` fixes [CG-11][CG-12][CG-13], whose first releases
  are to be looked up.

## 10. Open questions and gaps

- **Not published / not found:** any evidence for the 15% / 8 GiB constant or any reserve for a CI host (the
  nearest are Meta's 192M host-critical protection [CG-25] and Kubernetes' 10% example [CG-50]); any
  `cpu.weight` wake-up latency measurement for a protected cgroup beside a loaded cgroup (load 30 on 14 cores)
  on a current kernel (Borg's 2013 delay histogram is for a modified CFS [CG-48]); any `memory.high` plus
  zswap measurement; any independent measurement of oomd or systemd-oomd reaction time; any number for PSI's
  runtime overhead (TMO says "negligible", the kernel doc and the 2021 commit say "non-negligible" and
  "noticeable" at depth or with many cgroups, none gives a figure [CG-1][CG-21][CG-43]); the size of
  unaccounted kernel memory; between-job fairness under a parent `memory.high`; rustc and cargo thread counts
  for sizing `pids.max`; Twine's cgroup memory settings (the paper has none [CG-49]); any comparison of an
  in-agent killer with oomd or systemd-oomd.
- **Facts about Hex's hosts that decide the design:** kernel version and config on the kiln runners (the
  environment's own kernel is not evidence for them); whether swap or zswap exists; filesystems and devices
  (cgroup writeback support, iocost applicability); how the kiln agent is placed in the cgroup tree (the memory
  notes say it shares a service cgroup on fedora and adacachy, so `memory.low` ancestors are not yet set); what
  the host's liveness check measures and its window; the incident's kernel logs (OOM, hung-task, PSI).
- **Single-source or unmeasured claims to re-check:** the 2019 curve in K5 versus the current table K2; the
  Kubernetes benchmark's 10-minute observation (a longer run might OOM); the Sandcastle case study gives no
  thresholds; Meta's results are charts summarised in prose; the KEP says kernel 5.9 fixed the livelock, but
  related `memory.high` paths were fixed later (2022 [CG-13], 2023 [CG-11], 2024 [CG-12]), so "5.9 or later"
  is Kubernetes' minimum and not necessarily the right floor for Hex; the release that first carries each
  later fix is not established here.
- **Read but thin:** Twine says little about memory protection (§7.3); Borg's isolation evidence is from
  2013-14 on a modified kernel; the Kubernetes benchmarks are single-node, small-container runs by GitHub
  users; Chris Down's swap and zswap articles are reasoning by a kernel author, not measurements; the Cloudflare
  data is on zram-only swap, which Hex may not use.
- **Not read:** the Linux scheduler's wake-up latency papers and EEVDF documentation (no `cpu.weight` latency
  claim is made here); Heracles, PARTIES and CPI2 on shared-resource interference (cache and memory
  bandwidth) that cgroup weights do not isolate (Borg names it: "occasional low-level resource interference (e.g., memory bandwidth or L3 cache
  pollution) still happens" [CG-48]); Borg's successors Omega and Autopilot (Twine names Autopilot as adjusting each task's CPU
  and memory allocation [CG-49 p.800]); LWN coverage of `memory.high`; the BPF OOM killer, mentioned as in
  development in [CG-44]; the Senpai repository itself (Senpai is described through the TMO paper only).

## References

All accessed 2026-10-22.

**Kernel documentation and source** (Linux master at commit `a1c2b1da93f5`, `Makefile` 7.3.0)

- [CG-1] T. Heo et al., "Control Group v2", `Documentation/admin-guide/cgroup-v2.rst` (header date October
  2015; contents as of the commit above).
  https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/plain/Documentation/admin-guide/cgroup-v2.rst
- [CG-2] J. Weiner, "PSI - Pressure Stall Information", `Documentation/accounting/psi.rst` (April 2018; current
  contents). https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/plain/Documentation/accounting/psi.rst
- [CG-3] Linux kernel, `mm/memcontrol.c`.
  https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/plain/mm/memcontrol.c
- [CG-4] Linux kernel, `mm/page_counter.c`.
  https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/plain/mm/page_counter.c
- [CG-5] Linux kernel, `mm/vmscan.c`.
  https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/plain/mm/vmscan.c
- [CG-6] Linux kernel, `include/linux/memcontrol.h` (`MEMCG_CHARGE_BATCH` 64U at l.336).
  https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/plain/include/linux/memcontrol.h

**Kernel commits** (patch pages; `https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/patch/?id=<id>`)

- [CG-7] C. Down, "mm, memcg: throttle allocators when failing reclaim over memory.high", `0e4b01df8659`,
  2019-09-23. https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/patch/?id=0e4b01df8659
- [CG-8] C. Down, "mm, memcg: throttle allocators based on ancestral memory.high", `e26733e0d0ec`, 2020-03-21.
  https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/patch/?id=e26733e0d0ec
- [CG-9] C. Down, "mm, memcg: reclaim more aggressively before high allocator throttling", `b3ff92916af3`,
  2020-08-06. https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/patch/?id=b3ff92916af3
- [CG-10] T. Heo, "memcg: punt high overage reclaim to return-to-userland path", `b23afb93d317`, 2015-11-05.
  https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/patch/?id=b23afb93d317
- [CG-11] J. Weiner, "mm: memcontrol: fix GFP_NOFS recursion in memory.high enforcement", `9ea9cb00a82b`,
  2023-09-14. https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/patch/?id=9ea9cb00a82b
- [CG-12] J. Weiner, "mm: memcontrol: don't throttle dying tasks on memory.high", `63fd327016fd`, 2024-01-11.
  https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/patch/?id=63fd327016fd
- [CG-13] S. Butt, "memcg: synchronously enforce memory.high for large overcharges", `c9afe31ec443`,
  2022-03-22. https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/patch/?id=c9afe31ec443
- [CG-14] J. Kicinski, "mm/memcg: automatically penalize tasks with high swap use", `4b82ab4f2883`, 2020-06-01
  (read; not cited above except as background for `memory.swap.high`).
  https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/patch/?id=4b82ab4f2883
- [CG-15] D. Schatzberg, "cgroup: Documentation: Clarify usage of memory limits", `5647e53f7856`, 2023-06-01
  (acked by J. Weiner and C. Down). https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/patch/?id=5647e53f7856
- [CG-16] R. Gushchin, "memcg: introduce memory.min", `bf8d5d52ffe8`, 2018-06-07.
  https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/patch/?id=bf8d5d52ffe8
- [CG-17] C. Down, "mm, memcg: proportional memory.{low,min} reclaim", `9783aa9917f8`, 2019-10-06.
  https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/patch/?id=9783aa9917f8
- [CG-18] J. Weiner, "mm: memcontrol: fix occasional OOMs due to proportional memory.low reclaim",
  `f56ce412a59d`, 2021-08-19. https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/patch/?id=f56ce412a59d
- [CG-19] J. Weiner, "mm: memcontrol: recursive memory.low protection", `8a931f801340`, 2020-04-01.
  https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/patch/?id=8a931f801340

**Papers**

- [CG-20] T. Heo, D. Schatzberg, A. Newell, S. Liu, S. Dhakshinamurthy, I. Narayanan, J. Bacik, C. Mason,
  C. Tang, D. Skarlatos, "IOCost: Block IO Control for Containers in Datacenters", ASPLOS '22, 2022-02/03,
  doi 10.1145/3503222.3507727. https://www.cs.cmu.edu/~dskarlat/publications/iocost_asplos22.pdf
- [CG-21] J. Weiner, N. Agarwal, D. Schatzberg, L. Yang, H. Wang, B. Sanouillet, B. Sharma, T. Heo, M. Jain,
  C. Tang, D. Skarlatos, "TMO: Transparent Memory Offloading in Datacenters", ASPLOS '22, 2022-02/03,
  doi 10.1145/3503222.3507731. https://www.cs.cmu.edu/~dskarlat/publications/tmo_asplos22.pdf

**Meta documentation** (Facebook Inc.)

- [CG-22] "Maximizing Resource Utilization with cgroup2" (overview), (c) 2019.
  https://facebookmicrosites.github.io/cgroup2/docs/overview.html
- [CG-23] "Creating and organizing cgroups" (incl. the fbtax2 cgroup hierarchy), (c) 2019.
  https://facebookmicrosites.github.io/cgroup2/docs/create-cgroups.html
- [CG-24] "Using PSI pressure metrics", (c) 2019. https://facebookmicrosites.github.io/cgroup2/docs/pressure-metrics.html
- [CG-25] "Memory controller" (incl. the fbtax2 memory configuration), (c) 2019.
  https://facebookmicrosites.github.io/cgroup2/docs/memory-controller.html
- [CG-26] "Memory controller strategies and tools" (overcommit, load shedding, oomd), (c) 2019.
  https://facebookmicrosites.github.io/cgroup2/docs/memory-strategies.html
- [CG-27] "IO controller" (io.latency, fbtax2 IO), (c) 2019. https://facebookmicrosites.github.io/cgroup2/docs/io-controller.html
- [CG-28] "CPU controller" (cpu.weight, Laser case study), (c) 2019.
  https://facebookmicrosites.github.io/cgroup2/docs/cpu-controller.html
- [CG-29] "fbtax2: Putting it all together" (strategy summary and test results), (c) 2019.
  https://facebookmicrosites.github.io/cgroup2/docs/fbtax-results.html
- [CG-30] "oomd at Facebook" (Sandcastle case study), (c) 2018. https://facebookincubator.github.io/oomd/docs/oomd-casestudy.html
- [CG-31] "Overview of oomd", (c) 2018. https://facebookincubator.github.io/oomd/docs/overview.html
- [CG-32] facebookincubator/oomd, `README.md`. https://raw.githubusercontent.com/facebookincubator/oomd/main/README.md
- [CG-33] facebookincubator/oomd, `docs/production_setup.md`.
  https://raw.githubusercontent.com/facebookincubator/oomd/main/docs/production_setup.md
- [CG-34] facebookincubator/oomd, `docs/configuration.md`.
  https://raw.githubusercontent.com/facebookincubator/oomd/main/docs/configuration.md

**systemd** (manual pages served as "systemd 262")

- [CG-35] "systemd-oomd.service(8)". https://www.freedesktop.org/software/systemd/man/latest/systemd-oomd.service.html
- [CG-36] "oomd.conf(5)". https://www.freedesktop.org/software/systemd/man/latest/oomd.conf.html
- [CG-37] "systemd.resource-control(5)". https://www.freedesktop.org/software/systemd/man/latest/systemd.resource-control.html

**Kubernetes**

- [CG-38] Kubernetes SIG Node, "KEP-2570: Support Memory QoS with cgroups v2" (README; implementation history
  to 2026-06-01). https://raw.githubusercontent.com/kubernetes/enhancements/master/keps/sig-node/2570-memory-qos/README.md
- [CG-39] GitHub user `sohankunkerkar`, "KEP-2570: MemoryQoS Benchmark Results" (alpha; kernel 6.18.5, kind,
  kubelet v1.36.0-alpha.2; undated). https://raw.githubusercontent.com/sohankunkerkar/kep-2570-memoryqos-benchmarks/main/README.md
- [CG-40] GitHub user `QiWang19`, "KEP-2570: MemoryQoS Benchmark Results (Beta)" (kernel 7.0.9, single-node
  `local-up-cluster.sh`, 30.9 GiB allocatable; includes changes merged through 2026-06-01; undated).
  https://raw.githubusercontent.com/QiWang19/kep-2570-memoryqos-benchmarks/rollback-validation/README-rerun.md

**More kernel commits and documentation** (same Linux tree; patch pages as above)

- [CG-41] J. Weiner, "psi: pressure stall information for CPU, memory, and IO", `eb414681d5a0`, 2018-10-26.
  https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/patch/?id=eb414681d5a0
- [CG-42] S. Baghdasaryan, "psi: introduce psi monitor", `0e94682b73bf`, 2019-05-14.
  https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/patch/?id=0e94682b73bf
- [CG-43] S. Baghdasaryan, "cgroup: make per-cgroup pressure stall tracking configurable", `3958e2d0c34e`,
  2021-05-24. https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/patch/?id=3958e2d0c34e

**Mailing list and practitioner writing**

- [CG-44] S. Butt, reply "Re: [RFC PATCH 0/1] mm: Reduce direct reclaim stalls with RAM-backed swap", LKML,
  2026-03-03 (quotes M. Fleming's cover letter). https://lkml.iu.edu/hypermail/linux/kernel/2603.0/04407.html
  (the lore.kernel.org original returned HTTP 403 to my fetches; this is the lkml.iu.edu archive copy).
- [CG-45] M. Fleming, follow-up on the same thread with bpftrace data, LKML, 2026-03-04.
  https://lkml.iu.edu/hypermail/linux/kernel/2603.0/06910.html (J. Weiner's reply, read for context:
  https://lkml.iu.edu/hypermail/linux/kernel/2603.0/04939.html).
- [CG-46] C. Down, "In defence of swap: common misconceptions", 2018-01-02 (updated 2019-07).
  https://chrisdown.name/2018/01/02/in-defence-of-swap.html
- [CG-47] C. Down, "Debunking zswap and zram myths", 2026-03-24 (URL slug `zswap-vs-zram-when-to-use-what`).
  https://chrisdown.name/2026/03/24/zswap-vs-zram-when-to-use-what.html

**Cluster managers and Kubernetes documentation**

- [CG-48] A. Verma, L. Pedrosa, M. Korupolu, D. Oppenheimer, E. Tune, J. Wilkes, "Large-scale cluster
  management at Google with Borg", EuroSys '15, 2015-04, doi 10.1145/2741948.2741964.
  https://static.googleusercontent.com/media/research.google.com/en//pubs/archive/43438.pdf
- [CG-49] C. Tang et al., "Twine: A Unified Cluster Management System for Shared Infrastructure", OSDI '20,
  2020-11 (printed pages 787-803). https://www.usenix.org/system/files/osdi20-tang.pdf
- [CG-50] Kubernetes documentation, "Node-pressure Eviction".
  https://kubernetes.io/docs/concepts/scheduling-eviction/node-pressure-eviction/
- [CG-51] Kubernetes documentation, "Reserve Compute Resources for System Daemons".
  https://kubernetes.io/docs/tasks/administer-cluster/reserve-compute-resources/
- [CG-52] Kubernetes documentation, "Pod Quality of Service Classes" (section "Memory QoS with cgroup v2").
  https://kubernetes.io/docs/concepts/workloads/pods/pod-qos/

**CPU bandwidth control** (Linux tree)

- [CG-53] "CFS Bandwidth Control", `Documentation/scheduler/sched-bwc.rst` (read at commit `a1c2b1da93f5`).
  https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/plain/Documentation/scheduler/sched-bwc.rst?id=a1c2b1da93f5f612aa7a33190c4900c98e51f6da
- [CG-54] D. Chiluk (Indeed), "sched/fair: Fix low cpu usage with high throttling by removing expiration of
  cpu-local slices", `de53fd7aedb1`, 2019-07-23.
  https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/patch/?id=de53fd7aedb1

Status of this note: complete for the scope set by the task (kernel cgroup v2 documentation and source, PSI,
IOCost, TMO, oomd and systemd-oomd, Meta's fbtax2, Borg, Twine, Kubernetes MemoryQoS and node reserves,
failure modes); the "Not read" list in §10 names what a second pass should add.
