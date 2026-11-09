# The Windows `process` class: what a job object bounds, enforces and tells

Source notes for the native plan's step 3 (the Windows `process` class), beneath the native-platforms
decision (`docs/design/runtime.md`) and its §2. They say what a job object gives a box natively — the
mechanism Hex's class uses — and what it refuses to tell, with the measurements that settle the design
calls. Sources were read at fixed revisions: Microsoft's Win32 API docs (`MicrosoftDocs/sdk-api` at
`656e9656bfaef34dd29cd7ee9e1146aec17e924c`, `MicrosoftDocs/win32` at
`ca8274decead49403d70ceeed4af8d4ca74eaf11`), the C runtime's command-line docs (`MicrosoftDocs/cpp-docs`
at `f29e25fb8f05bb38cf0246476b5c1fc92bf33876`), and Docker's engine (`moby/moby` at
`aed2b781abc209a639b79d00f6962b629242aa1a`). Each quote is one line of its source, copied exactly;
`path:line` is at that revision. Measurements are jupiter's (AMD Ryzen AI MAX+ 395, 32 logical
processors, Windows 11 Pro 25H2 build 26200), `bench/native/windows/` (`job_events`, `RESULT.md`).

## 1. A box is a job object, its limits hard and the kernel's

- `ProcThread/job-objects.md:9`: "A *job object* allows groups of processes to be managed as a unit."
  Operations on it affect every process in it.
- Children join the job by default (`job-objects.md:32`): "After a process is associated with a job, by
  default any child processes it creates using" `CreateProcess` "are also associated with the job." So a
  box holds a member and everything it forks.
- Memory, the whole job's commit
  (`ns-winnt-jobobject_extended_limit_information.md`, `JobMemoryLimit`): "this member specifies the
  limit for the virtual memory that can be committed for the job." A commit past it is refused
  (`VirtualAlloc` → `ERROR_COMMITMENT_LIMIT`, 1455; measured, `job_events` e1/e16).
- Processes (`ns-winnt-jobobject_basic_limit_information.md:102`): "Establishes a maximum number of
  simultaneously active processes associated with the job."
- CPU, a hard cap (`ns-winnt-jobobject_cpu_rate_control_information.md:101`): "The job's CPU rate is a
  hard limit. After the job reaches its CPU cycle limit for the current scheduling interval, no threads
  associated with the job will run until the next interval." Or a weight (`:88`).
- The whole tree ends with the job (`job-objects.md:80`): "if the job has the
  JOB\_OBJECT\_LIMIT\_KILL\_ON\_JOB\_CLOSE flag specified, closing the last job object handle terminates
  all associated processes" — and for a nested job, "terminates all processes associated with the job
  and its child jobs in the hierarchy." `TerminateJobObject` does the same at once.

So a box needs no watcher to hold its bounds, unlike macOS: the kernel enforces the commit cap, the
process cap, the CPU cap and the tree's end. Hex's class sets them and reads the job back.

## 2. Started suspended, inside the job from creation, nothing else inherited

The plan: "A member is started suspended, assigned to the job and resumed, so nothing runs outside it."
Hex creates the member already in its job, so there is no window at all:

- `nf-processthreadsapi-updateprocthreadattribute.md:255` (`PROC_THREAD_ATTRIBUTE_JOB_LIST`): "The
  lpValue parameter is a pointer to a list of job handles to be assigned to the child process, in the
  order specified." Windows 10 and Server 2016 and newer. The member is a job member from its first
  instruction, and `CREATE_SUSPENDED` keeps it from running until Hex resumes it — stronger than
  create-then-assign, which has a window before the assign.
- Only its standard handles are inherited (`:104`, `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`): "The lpValue
  parameter is a pointer to a list of handles to be inherited by the child process." So a box's member
  doesn't inherit the node's other handles.
- The command line is quoted as the C runtime parses `argv` back
  (`cpp/main-function-command-line-args.md:110-126`: backslashes are literal "unless they immediately
  precede a double quote mark", an even run before a quote halves with the quote a delimiter, an odd run
  escapes it), so a program reads the arguments it was given.
- The environment is the member's whole environment, nothing inherited, as a block Windows requires
  sorted (`ProcThread/changing-environment-variables.md:15`): "All strings in the environment block must
  be sorted alphabetically by name. The sort is case-insensitive, Unicode order, without regard to
  locale."

## 3. Breakaway is refused

A box forbids a member leaving its job: Hex sets neither `JOB_OBJECT_LIMIT_BREAKAWAY_OK` nor
`JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK`. The managing-a-process-tree guidance
(`job-objects.md`, "Prevent breakaways of any kind by setting neither ... limit") then holds: a member
asking for `CREATE_BREAKAWAY_FROM_JOB` is refused. Measured: the child's `CreateProcess` fails with
access denied (error 5) in every pass (`job_events` e8).

## 4. Limit events: the job's accounting, never a message

A job's I/O completion port carries limit events, but most are lossy (`job-objects.md:63`,
`ns-winnt-jobobject_associate_completion_port.md:77`): "except for limits set with the
JobObjectNotificationLimitInformation information class, messages ... delivery to the completion port is
not guaranteed ... Notifications for limits set with JobObjectNotificationLimitInformation are
guaranteed to arrive at the completion port." CLAUDE.md §3 forbids a silent drop, so a box's limit
events come from the job's accounting (`QueryInformationJobObject`), read when they are asked for.

- **Memory, from the peak.** The job refuses whole a commit that would cross its commit limit, and counts
  the refused ask in its peak (`PeakJobMemoryUsed`; `job_events` e20, bench/install/records/windows-jupiter-20261105.txt entry 22), so the
  job's commit never passes the limit and its peak does exactly when a member was refused at the bound.
  Hex reads that
  (`hex_core::job::reached_bound`) and keeps no completion port, so no message queue to bound
  (CLAUDE.md §2). The "guaranteed" notification class doesn't do: a notification limit set under the cap
  records its violation late or not at all. On jupiter (build 26200.9711, 2026-11-07), a member refused
  at the cap and ended at once left it unset in 339 of 340 passes, a member held past the line saw it
  set after 1 ms, 25,895 ms, or not within 30 s, and neither its completion-port message nor the
  violation came with no hard cap at all; e17 and e5, which saw it in every pass on 2026-11-03, see it
  in none now (`job_events` e5, e17, e18; bench/install/records/windows-jupiter-20261105.txt entry 21).
- **The process count has no guaranteed event.** The violation class covers only memory, I/O bytes, job
  time and CPU-rate (`ns-winnt-jobobject_limit_violation_information.md`); there is no active-process
  notification limit, and `JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT` is one of the lossy completion messages.
  So a fork refused at the active-process limit has no §3-clean, job-side count (see §6).

## 5. The CPU hard cap maps as Docker's does

`CpuRate` is a fraction of the whole machine
(`ns-winnt-jobobject_cpu_rate_control_information.md:137`): "the number of cycles per 10,000 cycles",
and (`:139`) "Set CpuRate to a percentage times 100." Docker's Windows runtime maps `--cpus` the same
way (`moby/moby` `daemon/oci_windows.go:438`):
`cpuMaximum = max(uint16(c.HostConfig.NanoCPUs/int64(runtime.NumCPU())/(1e9/10000)), 1)`, i.e.
`cpus × 10000 / NumCPU`. Hex maps `cpu_max_milli` (thousandths of a CPU) to `milli × 10 / processors`,
over `GetActiveProcessorCount(ALL_PROCESSOR_GROUPS)` (every group, not Docker's one-group `NumCPU`),
rounded down so the cap never exceeds what was asked, and refuses by name a bound that maps below 1 or
above 10,000 (where Docker floors a tiny request to 1). The hard cap is precise: asked 1, 2.5 and 4 CPUs
of 32, a box used 0.92, 2.61 and 3.94 (`job_events` e12). The weight (1 to 9) only biases a shared core,
weakly and noisily (`job_events` e11): the hard cap is the real CPU bound.

## 6. What this settles, and the open plan calls

| A box needs | how, on Windows | §3-clean source |
|---|---|---|
| memory bound | job commit limit, refused past it | peak (accounting) |
| reached the bound | the peak at or past the commit limit | `PeakJobMemoryUsed` (accounting) |
| CPU | weight, or a hard cap of the whole machine | set only |
| processes | active-process limit (hard) | — refusals have no guaranteed count (below) |
| membership, no escape | the job, breakaway refused | the job's process-id list (accounting) |
| end the tree | kill-on-close / `TerminateJobObject` | — |

Two measured facts drive design calls the lead takes to the owner:

- **A crossing commit is refused whole; the peak counts its ask.** A member committed to within half a
  commit of a 256 MiB limit, asking for one commit of 64 MiB down to 64 KiB, was refused every time with
  the job's commit unchanged, while `PeakJobMemoryUsed` rose by the ask (`job_events` e20). e16's
  sweep, read on 2026-11-03 as a bounded ≈1.1 MiB overshoot admitted past the limit, is that ask landing
  at the limit plus the member's base commit (each step divides 256 MiB). So Hex sets the job's commit
  limit at the box's bound itself, with no margin (`hex_core::job::limits`), reports the committed peak
  as the job's peak at most the bound, and counts the bound reached when the peak is past it.
  **Falsifier:** a job whose commit (not its peak) passes its limit, or a peak past the limit with no
  refused commit. Keep e16 and e20 in the record so this stays checkable on new hardware or Windows
  builds.
- **A refused fork is enforced but not counted.** The active-process limit refuses a fork synchronously
  in the forking process (`CreateProcess` → `ERROR_NOT_ENOUGH_QUOTA`, 1816), the limit hard and the
  kernel's. But the job's accounting never moved across 25 refusals, and failed assignments didn't
  increment it either (`job_events` e14): Windows 26200 exposes no guaranteed per-job count of refused
  creations, like Linux's `pids.events`, and the only job-side signal is the lossy completion-port
  message §3 forbids. So `pids_refused` counts only what Hex observes synchronously — the node's own
  `exec` into a full box (surfaced as `Exhausted`) — and a member's own internal fork is enforced but
  not counted. The full survey of how others handle this, and the recommendation, is
  `docs/research/runtime/windows-pids-refused.md`.
