Measurement-only exit payload correction

This successor addresses the actual ROOT4cb memory-cut fixture failure at the
discarded-field cause documented in macos-jetsam-exit-evidence.md. It changes only
the standalone C fixture. No production, SDK, helper, exit-reason query, native
symbol, jetsam attribute, memory cap or resource allowance is added or changed.
The original baff C and full native5245-byte log remain immutable. NORMAL,
missing-image and bad-session already passed and must not run again for this change.

The pure decoder consumes the SAME already-owned kevents. It accepts only the
exact expected positive PID with EVFILT_PROC; the existing SIGCHLD wakeup and
matching NOTE_EXEC notification provide no exit authority. An exit requires
NOTE_EXIT/NOTE_EXITSTATUS, the existing no-NOTE_REAP EOF/ONESHOT contract, no
EV_ERROR or misplaced payload bits in fflags, and a representable unsigned32-bit
payload. Raw ident/filter/flags/fflags/data are retained and printed. A conflicting
second terminal payload refuses and preserves the first raw fact. Errors never
manufacture a memory detail or native wait result.

The memory predicate requires the actual NOTE_EXIT_DETAIL marker AND the
NOTE_EXIT_MEMORY bit in its data, alongside the unchanged positive SIGKILL wait
status and original peak>=explicit-bound condition. `waitpid` still supplies its
own status variable; kevent data never substitutes for it. A missing event, absent
marker, absent memory payload, malformed native event, normal native exit or
insufficient peak still fails. No private EXITREASON API is necessary to retain
the event payload that the original fixture already received and discarded.

The new pure field-decoder control supplies fixed independent counterexamples:
the same markers plus SIGKILL/peak without a memory payload; a memory bit placed
in fflags; a memory payload paired with normal wait status or insufficient peak;
foreign PID/filter; payload without its marker; EV_ERROR; negative/overwide payload;
conflicting terminal payload; and exec/signal notifications without exit. The
positive event includes the payload bit and retains the raw value. The control
runs before any native setup, PID query, resource query or process effect.

ROOT builds the C using the same recorded installed SDK and compile flags. Run
the two distinct selections against that immutable binary:

```
setexec-control --check-exit-decoder
setexec-control --memory-only --memory-mib 64
```

The first selects one pure control and creates no native actor. The second selects
only existing case3, reports selected1/first_case3/last_case3, and uses the unchanged
39.507-second absolute internal guard plus ROOT's finite external guard and
admitted64MiB input. It must produce exactly one case3 result and no case0–2
results. Default four-cut invocation remains available for original reproducibility;
it is not the authorized follow-up invocation.

All initial held PID/birth/NOTE_EXEC/stop/no-canary checks, final explicit resume,
foreign-image bytes, child-only CLOEXEC sources, prepared file actions, QoS/jetsam,
FD allowlist, parent flags/limits, preexec diagnostic and unexpected returned-child
ownership remain unchanged. The SAME terminal waiter still performs WNOWAIT,
drains its existing bounded queue, reads peak and reaps once. The new decoder is
pure and contains no logging, allocator or native call, because this shared
terminal routine also serves the trusted post-fork cleanup trampoline. Raw-event
printing stays in the parent run-case function, outside that trampoline.

This source is UNCOMPILED/UNRUN. A new memory result must retain the raw exit data,
separate wait status, peak, exact PID/birth, parent-state checks and positive reap.
The old relevant0 is not relabeled. Even a corrected mechanism result does not
prove exact jetsam namespace/code, aggregate hard memory enforcement, HVF/Vulkan
compatibility, fixed-worker closure, helper recovery or final charge release.
The full2ef/fab production helper remains unintaken and production changes held.
