# What a native Windows box bounds with a job object: measured

`job_probe.ps1` on jupiter (AMD Ryzen AI MAX+ 395, 32 CPUs, Windows 11 Pro 25H2, build 26200.9711), Windows PowerShell
5.1 with its C# compiled by Add-Type, 2026-11-01. These are the native-platforms research's §7 measurements
(`docs/research/runtime/native-platforms.md` §2). Amphora's chain4 pass ran beside them throughout, by Ada's rule that
work runs side by side. Records:

- `records/job-probe-124f1c0-20261101T001006Z.txt`: commit 124f1c0, one pass, 00:10:06-00:10:29Z.
- `records/job-probe-69c2df6-20261101T001102Z.txt`: commit 69c2df6, kiln run r42, passes 1 and 2 (00:11:02Z at 25%
  CPU, 00:19:06Z at 65%). Pass 3's output was lost when kiln restarted mid-run. Rerun for three whole passes before
  any of this is of record.

## A job costs nothing measurable over the program's own start

A box, against a bare start of the same program (`cmd.exe /c exit 0`), 200 each, alternating. The box is a job with
its limits (memory, active processes, kill-on-close, a hard CPU cap), the program started suspended into it, resumed,
run to its end, and the job's handle closed.

| Record | box p50 | bare p50 | box p99 / max | bare p99 / max |
|---|---|---|---|---|
| 124f1c0 | 43.1 ms | 42.6 ms | 66.7 / 66.8 ms | 88.1 / 2,679 ms |
| 69c2df6, pass 1 | 42.9 ms | 42.8 ms | 23,243 / 27,428 ms | 27,396 / 27,547 ms |
| 69c2df6, pass 2 | 71.5 ms | 72.5 ms | 11,258 / 17,500 ms | 11,250 / 27,720 ms |

The tails are Windows' process start under the host's load, a box's and a bare start's alike: up to 27 s for a
`cmd.exe` at 25-65% CPU. That's for the Windows `process` class to measure and name before it claims a tail.

## The memory bound is the kernel's, to within ten pages

A member committing 1 MiB at a time into a job capped at 268,435,456 bytes (256 MiB):

- Its commits are refused at the cap: `VirtualAlloc` fails with error 1455 (`ERROR_COMMITMENT_LIMIT`) after 235 MiB
  of its own, the rest of the job's commit being the .NET runtime's and cmd.exe's.
- The job's peak passed the cap by 32,768 and 40,960 bytes (8 and 10 pages) in the two passes that print bytes: the
  kernel holds the cap at commit time, to within a few pages.

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
