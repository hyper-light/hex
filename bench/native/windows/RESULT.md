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
