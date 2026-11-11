Mac SETEXEC memory-cut evidence, 2026-11-11

Research only. The C fixture at baff74c55cd8f351a11aff9e116143fbfb035f06 is
unchanged. No production, SDK, helper, native API or oracle change is included.

ROOT4cb960811fd7d5dd5b86fc33873ad5451f178845 compiled and ran all four cuts.
NORMAL, missing-image and bad-session each returned relevant1/error0/reaped1.
The memory cut returned SIGKILL9, peak67109272 bytes against explicit64MiB,
reaped1, but relevant0 because its accumulated fflags lacked NOTE_EXIT_MEMORY.
The overall result remains1. The complete5245-byte native output independently
matches fe8d3bdb67002c350a904edeac9f7673a086aca20665f65427ac17746478311e;
binary41ab6968745e19a53d0a8e95e75625b14836d0789878922c9831833946706515.
The log is in the owned coordinator directory
`cohort-4cb96081-mac-setexec-child-cloexec/cli.log`. Controller7169 and children
7170–7173 were reaped; the run reports source unchanged, temp0 and parent limits
unchanged. Its installed SDK is26.5, host macOS26.4 arm64. These are different
versions; the following source pin does not identify the installed kernel bytes.

The fixture sampled the wrong field for memory detail. Its `terminal` collects
only `kevent.fflags`; it discards exit-event `data`, then tests the accumulated
fflags against NOTE_EXIT_MEMORY. Therefore this run does not establish whether
the installed kernel supplied a memory detail bit in data. That value is missing
from the retained native output. SIGKILL and peak alone do not establish cause.

Pinned XNU f6217f891ac0bb64f3d375211650a4c1ff8ca1ea provides the concrete
field contract. In `filt_procevent`, NOTE_EXITSTATUS and NOTE_EXIT_DETAIL are
fflags notification markers. The exit status and the detail bits are collected
in `kn_hook32`. A process with P_LTERM_JETSAM contributes NOTE_EXIT_MEMORY there.
`filt_procprocess` passes that value to `knote_fill_kevent`, which writes it to
`kevent.data`. NOTE_EXIT_MEMORY is0x00020000; NOTE_EXIT_DETAIL is0x02000000.
The observed accumulated fflags0xa6000000 contain the marker and prior NOTE_EXEC,
not the exit detail payload. See [kern_event.c lines1231–1316](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/kern_event.c#L1231),
[data assignment lines999–1017](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/kern_event.c#L999),
and [event.h lines268–293](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/sys/event.h#L268).

The pinned fatal footprint path creates an OS_REASON_JETSAM reason with
JETSAM_REASON_MEMORY_PERPROCESSLIMIT, then kills that process through the
synchronous memorystatus path. `memorystatus_do_kill` sets P_LTERM_JETSAM and,
for this cause, P_JETSAM_PID; `exit_with_reason` retains the reason and flags in
the exiting proc. Thus this source path should supply the memory detail in data;
it does not prove that the installed memory cut took that path. The broad memory
detail also covers other jetsam causes and is not a per-process-limit identifier.
See [fatal callback lines5042–5093](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/kern_memorystatus.c#L5042),
[kill mapping lines2376–2395](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/kern_memorystatus.c#L2376),
and [retention in kern_exit.c lines1623–1630](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/kern_exit.c#L1623).

An independent exact-reason observation is available through the existing public
libproc function `proc_pidinfo`, using the pinned private flavor
PROC_PIDEXITREASONBASICINFO25. Its packed24-byte public struct contains namespace,
code, flags and payload-size scalars, without a user payload pointer. The pinned
kernel permits a retained zombie lookup for this flavor, requires its current
parent or original debugger, returns ENOENT if no reason exists, and copies the
exact reason namespace/code. The source constants are OS_REASON_JETSAM1 and
JETSAM_REASON_MEMORY_PERPROCESSLIMIT7. This is a candidate measurement query,
not an already observed result or permission to add a production interface.
See [proc_info.c lookup lines2109–2116](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/proc_info.c#L2109),
[parent/absence checks lines3867–3927](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/proc_info.c#L3867),
[packed struct lines270–275](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/sys/proc_info.h#L270),
[flavor lines167–171](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/sys/proc_info_private.h#L167),
[namespace](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/sys/reason.h#L113)
and [cause](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/sys/kern_memorystatus.h#L279).

The libproc wrapper returns0 on syscall failure, preserving errno; success must
return the full struct byte count. Any future measurement must record short/error
results instead of fabricating zero-valued reason fields. It must join the SAME
unreaped direct child and exact recorded birth before and after the read, after
WNOWAIT confirms terminality and before the sole waitpid reaps it. No PID scan,
reparenting, extra waiter, task privilege or post-reap PID query is needed. See
[the wrapper](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/libsyscall/wrappers/libproc/libproc.c#L100)
and [exact-size copyout](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/proc_info.c#L2450).

The smaller next control is to retain and print the existing matching NOTE_EXIT
event's flags AND data, validate NOTE_EXIT_DETAIL, and apply the unchanged required
memory-detail predicate to its payload. An explicit memory-only selector can
exercise only cut3 under the unchanged39.507-second budget; the three passed cuts
must not be replayed. A fixed event with the marker but no memory payload must
still fail. No new native function is necessary for that field observation.
The independent namespace/code read is a stronger attribution option requiring
separate review of installed ABI/header availability, not a fallback that turns
an absent event detail into success. No code change or replay is made by this note.

Keep the original relevant0 output and all prior EBADF sources/raw. This research
does not establish a hard aggregate memory bound, GPU-service accounting, closed
fixed-worker membership, actual HVF/Vulkan compatibility, helper survival or final
charge release. The complete2ef/fab helper remains source-only and unintaken.
