# What a native Windows box bounds with a job object: measured

`job_probe.ps1` on jupiter (AMD Ryzen AI MAX+ 395, 32 CPUs, Windows 11 Pro 25H2, build 26200.9711), Windows PowerShell
5.1 with its C# compiled by Add-Type, 2026-11-01. These are the native-platforms research's §7 measurements
(`docs/research/runtime/native-platforms.md` §2). Amphora's chain4 pass ran beside them throughout, by Ada's rule that
work runs side by side. Records:

- `records/job-probe-124f1c0-20261101T001006Z.txt`: commit 124f1c0, one pass, 00:10:06-00:10:29Z.
- `records/job-probe-69c2df6-20261101T001102Z.txt`: commit 69c2df6, kiln run r42, passes 1 and 2 (00:11:02Z at 25%
  CPU, 00:19:06Z at 65%). Pass 3's output was lost when kiln restarted mid-run.
- `records/job-probe-80fb38b-20261101T075113Z.txt`: commit 80fb38b, three whole passes, 07:51:13-08:39:31Z, at 3-11%
  CPU, after jupiter's reboot that enabled Containers and HypervisorPlatform. The record of the three below.

## A job costs nothing measurable over the program's own start

A box, against a bare start of the same program (`cmd.exe /c exit 0`), 200 each, alternating. The box is a job with
its limits (memory, active processes, kill-on-close, a hard CPU cap), the program started suspended into it, resumed,
run to its end, and the job's handle closed.

| Record | box p50 | bare p50 | box p99 / max | bare p99 / max |
|---|---|---|---|---|
| 124f1c0 | 43.1 ms | 42.6 ms | 66.7 / 66.8 ms | 88.1 / 2,679 ms |
| 69c2df6, pass 1 | 42.9 ms | 42.8 ms | 23,243 / 27,428 ms | 27,396 / 27,547 ms |
| 69c2df6, pass 2 | 71.5 ms | 72.5 ms | 11,258 / 17,500 ms | 11,250 / 27,720 ms |
| 80fb38b, pass 1 (3% CPU) | 20.6 ms | 20.4 ms | 22,876 / 23,190 ms | 22,797 / 23,046 ms |
| 80fb38b, pass 2 (4% CPU) | 20.3 ms | 20.3 ms | 21,546 / 21,862 ms | 21,662 / 21,738 ms |
| 80fb38b, pass 3 (11% CPU) | 20.8 ms | 20.3 ms | 21,852 / 22,030 ms | 21,502 / 21,612 ms |

The tails are Windows' own process start, a box's and a bare start's alike, and not the host's load: at 3-11% CPU
about 1 start in 100 still took 21.5-23.2 s. A stall that near a fixed length looks like a timeout inside
Windows' process start (a security or reputation check, to be found). The Windows `process` class names its
cause before it claims a tail.

## The memory bound is the kernel's, to within a dozen pages

A member committing 1 MiB at a time into a job capped at 268,435,456 bytes (256 MiB):

- Its commits are refused at the cap: `VirtualAlloc` fails with error 1455 (`ERROR_COMMITMENT_LIMIT`) after 235 MiB
  of its own, the rest of the job's commit being the .NET runtime's and cmd.exe's.
- The job's peak passed the cap by 32,768-49,152 bytes (8-12 pages) in the five passes that print bytes: the kernel
  holds the cap at commit time, to within a few pages.

## The tree ends with the job, and a member can't leave it

- Closing the job's last handle (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`) ended the member and its grandchild, in every
  pass.
- `CREATE_BREAKAWAY_FROM_JOB` from inside a job that doesn't allow it was refused with error 5 (access denied), in
  every pass.

## What this says

- A native Windows box gets from the kernel what a Linux cgroup gives: a commit cap for the whole job, a process
  count, a CPU cap, and its whole tree ended at once, with no way out for a member.
- The job adds about 0.5 ms or less to a program's start. The start itself is the cost to beat: about 43 ms for
  `cmd.exe`, with multi-second tails under load.

## The `process` class on job objects: measured (native plan step 3)

`job_events` (native Rust, `windows-sys`; its own workspace) measures what a job object enforces and
tells for Hex's class, and `crates/hex-cli/tests/windows_process_box.rs` runs HEX-RT-1..9 and the
step's scenarios against the real class. Both on jupiter, 2026-11-03; record
`records/job-events-20261103T074519Z.txt`, sources `docs/research/runtime/windows-job-objects.md`.

### A commit that would cross the commit limit is refused whole

A member committing at several step sizes into a 256 MiB job, born in the job (`JOB_LIST`), e16:

| commit step | job's peak over the limit |
|---|---|
| 64 KiB | 28-49 KiB |
| 1 MiB | 92-104 KiB |
| 4 MiB | 1.10-1.11 MiB |
| 16 MiB | 1.09-1.11 MiB |
| 64 MiB | 1.09-1.11 MiB |
| 256 MiB | 1.09-1.11 MiB |

This was first read as a crossing commit admitted past the limit by a bounded ≈1.1 MiB, and Hex set the
job's commit limit a margin under the box's bound for it. e20 (2026-11-07) shows otherwise. Each member
first commits to within half a commit of the limit, then asks for one commit (64 MiB down to 64 KiB) that
would cross it. **Every such commit is refused whole** (`ERROR_COMMITMENT_LIMIT`, the job's commit
unchanged), and the job's `PeakJobMemoryUsed` still rises by the refused ask: 64 MiB asked from
33,521,664 bytes short of the limit leaves the peak 33,718,272 bytes past it. e16's flat ≈1.1 MiB is that ask landing at the
limit plus the member's own base commit, since each of its steps divides 256 MiB. So the job never commits
past its limit, and its commit limit is the box's bound itself (`hex_core::job`, no margin). The peak
counts a refused ask, so the class reports the box's committed peak as that peak at most its bound
(`job::committed_peak`), and counts the box reaching its bound when the peak is past the limit
(`job::reached_bound`): only a refused ask takes it there. That is the job's own accounting, so the class
keeps no completion port and no message queue (CLAUDE.md §2, §3). It first read the
`JobObjectNotificationLimitInformation` violation, set 4 MiB under the cap, which e5 and e17 saw trip on
2026-11-03. On 2026-11-07 the same probes saw it in none of their passes, and e18 at Hex's own numbers
found it set late (up to 26 s) or never, so a box that ended at its bound wasn't counted
(bench/install/records/windows-jupiter-20261105.txt entries 21 and 22).

### Processes, CPU, breakaway, delete

- The active-process limit refuses the over-limit fork in the forking process
  (`ERROR_NOT_ENOUGH_QUOTA`). Across 25 refused creations the job's accounting never moved, and failed
  assignments didn't increment it either: Windows 26200 exposes no guaranteed per-job count of refused
  forks (like Linux's `pids.events`), and the only job-side signal — the completion-port
  `JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT` message — Microsoft documents as not guaranteed, a lossy message
  §3 forbids as a source. So `pids_refused` counts only the refusals Hex observes synchronously and
  reliably: the node's own `exec` into a full box, whose `CreateProcess` returns the refusal (counted
  exactly, surfaced as `Exhausted`). A member's own internal fork, refused by the kernel in that member,
  is enforced but not counted by the box (no §3-clean job-side source;
  `docs/research/runtime/windows-pids-refused.md`).
- The CPU hard cap is precise: asked 1, 2.5 and 4 CPUs of 32, a box used 0.92, 2.61 and 3.94. The weight
  (1-9) only biases a shared core, weakly and noisily (even 9:1 gave ≈1.17×): the hard cap is the real
  CPU bound.
- A member asking to break away (`CREATE_BREAKAWAY_FROM_JOB`) is refused (error 5). `TerminateJobObject`
  / closing the job's last handle (kill-on-close) ends the box's grandchild in about a millisecond, and
  a nested job's close leaves its parent's other members.
- A member born in the box via `JOB_LIST` is a member of the box's job and the node's; whether a member
  runs is read from the job's live process-id list, not its accounting `ActiveProcesses` (which lingers
  while a terminated member's handle is held).

### HEX-RT on jupiter

On the final merge tree (cp/t7-native-s3 1b7a92f), HEX-RT-1..9 pass and the step's scenarios pass (record
`records/hexrt-jupiter-20261103-1b7a92f.txt`, definitive plain kiln run r1128; the 7 workspace gates pass
on the same tip, kiln-a094a12d runs r904/r1127, exit 0): a member past its 256 MiB bound is refused at the bound
with the job's peak at or below it (oom counted), a member asking to break away is refused, delete ends a
box's grandchildren, an under-bound member runs and reports its exit, a node-started process past the
process bound is refused (`Exhausted`) and counted, and TERM ends the box reading as `Signal(15)`.
HEX-RT-6 on Windows checks the process bound's **deterministic effect** — the member that can't start all
its children exits non-zero — rather than the exact `pids_refused` count, because a member's own internal
fork-refusal has no §3-clean count on Windows (the research below). The exact
count assertion stays where a kernel keeps it (Linux's `pids.events`), via `Fixture::refused_forks_counted`;
`pids_refused` on Windows still carries the node-mediated refusals Hex sees synchronously. This is Ada's
decision (2026-11-03), in that order: she directed "check blocking happened + keep the tally" and
"implement the counter ourselves"; asked for research first; then, with the fix validated 7/7 on jupiter,
confirmed "we already solved this."

### Architectures

The class is architecture-neutral (no intrinsics). It builds and lints clean on `x86_64-pc-windows-msvc`
(`cargo clippy --all-targets -- -D warnings`, in `scripts/gates.sh`), and HEX-RT and the `job_events`
measurements ran on jupiter (amd64). **Windows arm64 (`aarch64-pc-windows-msvc`) is skipped for now**
(Ada's call, 2026-11-03): it is not in the gates and not built or run here.
