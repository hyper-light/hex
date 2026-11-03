# Counting a fork refused by a Windows job object's process limit: what others do

The question (native plan step 3, HEX-RT-6). A Windows job object's `ActiveProcessLimit` refuses an
over-limit `CreateProcess` **synchronously in the forking process** (`ERROR_NOT_ENOUGH_QUOTA`), the
limit hard and the kernel's. But the job's owner has no reliable, synchronous count of such refusals:
the job's accounting counters don't increment on a refusal (measured, `bench/native/windows/RESULT.md`,
`job_events` e14 — 25 refusals, every counter flat), and the one job-side signal, the completion-port
message `JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT`, Microsoft documents as not guaranteed to be delivered
(`docs/research/runtime/windows-job-objects.md` §4). Hex measured that this message is also posted
*asynchronously*, after the refusing process may have exited, so a `usage()` read taken right after
races it (HEX-RT-6 passes under load, fails an idle isolated read). Ada's call (2026-11-03): research
what competing and alternative implementations do before settling HEX-RT-6's Windows semantics.

Sources were read at fixed revisions (a commit SHA, or a release tag; a Microsoft Learn page by URL with
its access date, since Learn isn't versioned by SHA). Each quote is one line of its source, `path:line`
at that revision.

## 1. The container ecosystem does not count it — and mostly does not even set the limit

### hcsshim (microsoft/hcsshim, the Windows containerd shim), `b1c1f43a3a6fb308cf3b1ba45affa835a693695c`

hcsshim's job-object layer **never arms** the active-process limit: `JobLimits`
(`internal/jobobject/jobobject.go:42`) carries only CPU, weight, affinity, memory and IO fields, and
`SetResourceLimits` (`internal/jobobject/limits.go:25`) writes only those. The Win32 field exists in the
bindings but is never set (`internal/winapi/jobobject.go:72`, `ActiveProcessLimit uint32` inside
`JOBOBJECT_BASIC_LIMIT_INFORMATION`).

Its IOCP loop (`internal/jobobject/iocp.go:36`, `func pollIOCP`) dispatches messages in `parseMessage`
(`:81`). It acts only on `JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO` (`:86` → `MsgAllProcessesExited`); the
`JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT` case (`:92`) — with the new-process, exit and memory cases — has an
**empty body**, falling through to `MsgUnimplemented`, documented `iocp.go:31`:
"MsgUnimplemented represents a message that we are aware of, but that isn't implemented currently." Its
accounting query (`jobobject.go:433`, `QueryProcessorStats` → `JOBOBJECT_BASIC_ACCOUNTING_INFORMATION`)
exposes only `TotalProcesses`/`ActiveProcesses`/`TotalTerminateProcesses` (`internal/winapi/jobobject.go:133-135`)
— no refusal count. **Recognized, then discarded; never counted.**

### Docker / moby (moby/moby), `0c86697d0b643694d4ce4b106278853c2a07cbe4`

Docker **refuses `--pids-limit` on Windows outright**, synchronously at container-resource validation
(`daemon/daemon_windows.go:161-162`):
"if resources.PidsLimit != nil && *resources.PidsLimit != 0 {" →
`return warnings, fmt.Errorf("invalid option: Windows does not support PidsLimit")`. The Windows OCI
resource builder (`daemon/oci_windows.go:422`, `setResourcesInSpec`) sets only CPU, memory and storage —
no process cap. So moby never sets the job limit and has nothing to count: it is a typed *config*
refusal, not a per-fork runtime signal.

### Kubernetes (kubernetes/kubernetes `v1.37.2`, kubernetes/website `8f6d9762bfd9e130d63f678c2e0f9b048bc3373e`)

Per-pod/node PID limiting is **Linux-only**. `podPidsLimit` is applied only in the Linux pod-cgroup path
(`pkg/kubelet/cm/pod_container_manager_linux.go:99-100`); Windows wires a no-op stub
(`pkg/kubelet/cm/container_manager_windows.go:273-274`, `NewPodContainerManager` → `&podContainerManagerStub{}`),
and the non-Linux resource config is nil (`pkg/kubelet/cm/helpers_unsupported.go:52-53`,
`ResourceConfigForPod` → `return nil`). Node PID stats for `PIDPressure` eviction are Linux-only
(`pkg/kubelet/stats/pidlimit/pidlimit_unsupported.go` `//go:build !linux` → `Stats` returns `nil, nil`).
The Windows docs confirm it (`content/en/docs/concepts/windows/intro.md:157`: "The `PIDPressure`
Condition is not implemented"). Enforcement rides on the Linux cgroup `pids` controller; on Windows the
limit is never applied, so there is nothing to count.

## 2. The reliable counters all observe the synchronous failure in a layer they control

The refusal is reliable and synchronous in exactly one place: the return of `CreateProcess` in the
forking process (`winnt.h` `JOBOBJECT_BASIC_LIMIT_INFORMATION.ActiveProcessLimit`, Microsoft Learn,
accessed 2026-11-03: "If you try to associate a process with a job, and this causes the active process
count to exceed this limit, the process is terminated and the association fails."). Every system that
counts child-creation attempts reliably moves the count to a layer that sees that synchronous failure.

### Chromium sandbox (chromium/chromium `b80155a7735fafe92487726c87668b9bfca78efc`), `sandbox/win/src/`

Chromium **prevents** rather than counts: a locked-down target's job caps processes at one
(`job.cc:47` sets `JOB_OBJECT_LIMIT_ACTIVE_PROCESS`, `:48` `ActiveProcessLimit = 1`), so the target can
create no child at all. The **broker** is the sole creator of each target
(`broker_services.cc:262` "Creates the target (child) process suspended and assigns it to the job",
`:284` `::CreateProcessAsUserW(`), so the launches it performs are counted by construction; a target's
own `CreateProcess` is never brokered (the IPC dispatcher brokers thread/token calls only,
`process_thread_dispatcher.cc` — no create-process tag). Tellingly, Chromium *does* consume the same
completion-port message Hex does, but only to keep its live-target accounting balanced, and comments on
its unreliability: `broker_services.cc:189` `case JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT:`, `:190`
"A child process attempted and failed to create a child process.", `:194` "Windows does not reveal the
process id." So the one peer that reads this message treats it as a best-effort accounting artifact,
exactly as Hex's measurement found.

### BuildXL (microsoft/BuildXL `64be5f9fae1b204b809abd084129ac1898bb1df3`), Detours

BuildXL hooks `CreateProcess*` **in the forking process itself** (`DetouredFunctions.cpp:2959`
`Detoured_CreateProcessW`, shared body `:2736` `Detoured_CreateProcessCommonW`) and reports both outcomes
with the real error: `:2902` success → `:2906` `ReportIfNeeded(..., ERROR_SUCCESS)`, `:2911` failure →
`:2913` "Process creation failure is something normally visible to the caller. Preserve last error
information." → `:2916` `ReportIfNeeded(..., GetLastError())`. A job `ERROR_NOT_ENOUGH_QUOTA` refusal
lands in the failure path, counted synchronously in the caller. The detour is injected into every child
(`DetouredProcessInjector.h`), riding the whole tree — reliable where the DLL loads, evadable by
breakaway or a raw `NtCreateUserProcess`, at the cost of injecting a DLL into every box member.

### ETW `Microsoft-Windows-Kernel-Process`

Success-only: the provider's creation event is `ProcessStart` (ID 1), emitted when a process actually
starts, keyed by the new PID (community mapping, Velociraptor `windows.etw.kernelprocess`, accessed
2026-11-03; I could not reach a Microsoft-primary per-event manifest from this host — flagged). A refused
`CreateProcess` starts no process, so it emits no event; there is no "creation-refused" event in the
provider (nor in Security audit 4688). A job-active-process-limit refusal produces no ETW event.

### Kernel driver `PsSetCreateProcessNotifyRoutineEx`/`Ex2` (Microsoft Learn, doc rev `5ea3c2eb…`, accessed 2026-11-03)

The most authoritative hook: it "runs in the context of the thread that created the new process",
"just after the first thread in the process has been created", and can veto via
`PS_CREATE_NOTIFY_INFO.CreationStatus` ("Drivers can change this value to an error code to prevent the
process from being created."). So a driver can *impose and count its own* limit deterministically. But
it is a **signed kernel driver**, and it is not documented to fire for a creation a *job* limit rejected
before first-thread (the "created and deleted without a thread ever being created" note suggests such an
attempt may surface only as a deletion, or not at all) — so it does not cleanly count a *job's* refusal.

## Summary: each approach

| Mechanism | Reliable for the job owner? | Synchronous? | Cost | Counts a job-limit refusal? |
|---|---|---|---|---|
| `CreateProcess` return in the forking process | Yes, but only the forking process sees it | Yes | none | Yes (to the caller) |
| Job completion-port `JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT` | No — delivery "not guaranteed", no PID | Async (posts after the fact) | none | A best-effort hint only; Chromium uses it as an accounting offset. Hex does NOT use it (§3 forbids a lossy source) |
| Job accounting (`JOBOBJECT_BASIC_ACCOUNTING_INFORMATION`) | — | — | none | No — measured flat across 25 refusals (e14) |
| Chromium broker (broker calls `CreateProcess`; target limit 1) | Yes, for brokered launches | Yes | broker mediates every creation; in-box spawning prevented | Counts by construction; doesn't count a member's own forks (prevents them) |
| BuildXL Detours (`CreateProcess` hook in the child) | Yes where injected (evadable) | Yes | a DLL injected into every box member | Yes — sees the synchronous `ERROR_NOT_ENOUGH_QUOTA` |
| ETW Kernel-Process | — | near-real-time | an ETW consumer | No — success-only, no refusal event |
| Kernel driver `Ex2` veto | Yes for its own limit | Yes (can veto) | a signed kernel driver | Counts its own vetoes; a job's refusal not documented to fire it |

## Recommendation for HEX-RT-6 on Windows, and what would falsify it

The research confirms the premise and finds no cheap reliable alternative: **no competitor counts
job-refused forks on Windows** (hcsshim discards the message, Docker refuses the option, Kubernetes is
Linux-only), and the mechanisms that *can* count reliably all relocate the limit-and-count into a layer
that sees the synchronous failure — a broker, a per-child Detours DLL, or a signed kernel driver — each
disproportionate to the `process` class's purpose. The one peer that reads the same completion-port
message (Chromium) treats it as an unreliable best-effort offset, matching Hex's measurement.

So, for the `process` class:
1. **Enforce with the job's hard `ActiveProcessLimit`** — kernel-enforced, synchronous, reliable; a box
   never exceeds its process bound. (Unchanged; this is the behaviour HEX-RT-6 must prove.)
2. **Count the refusals Hex observes synchronously** — the §3-clean "implement the counter ourselves"
   (Ada): the node's own `exec` into a full box returns `ERROR_NOT_ENOUGH_QUOTA` synchronously, so Hex
   counts it exactly in `Usage::pids_refused` and surfaces it as `Exhausted` (the broker insight —
   Hex is the broker for the members it starts). This never reads a lossy message.
3. **Do not count from the lossy completion-port message.** The step forbids a source that may be lost
   (CLAUDE.md §3), so Hex does *not* tally the `JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT` notifications (the
   way an earlier draft did, and the way Chromium uses the message only as a throwaway accounting
   offset). A member's *own* internal fork, refused by the kernel in that member, is therefore enforced
   but not counted by the box — there is no §3-clean job-side source for it.
4. **RECOMMENDED — HEX-RT-6 on Windows checks the refusal's deterministic effect** rather than the exact
   member-internal count: the blocked program exits non-zero (the bound stopped a fork it needed), which
   is deterministic on every platform, with the exact-count assertion kept where a kernel counts it
   (Linux's `pids.events`). This keeps the contract — the bound blocks forks and is enforced — without a
   count Windows can't give §3-cleanly. **This is a recommendation, not applied here**: changing a
   plan test's evidence on a platform is a plan decision for Ada and the lead (who owns the shared
   conformance suite). As the suite stands, HEX-RT-6 asserts the exact count unconditionally, so it does
   not pass on Windows for a member's own internal spawn — the open item this note is written to settle.

**What would falsify this** (and move Hex to an exact Windows count):
- A Windows release that adds the active-process limit to the guaranteed `JobObjectNotificationLimitInformation`
  class, or a job accounting field that counts refused creations — then Hex counts from that
  deterministically and HEX-RT-6 asserts the exact count on Windows too. (Re-run `job_events` e14 on the
  new build to check.)
- A requirement for an exact in-box fork-refusal count (e.g. billing) that justifies a broker or a signed
  driver — then Hex adopts that layer and counts by construction, as Chromium/BuildXL do.
