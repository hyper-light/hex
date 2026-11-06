# The macOS line's constants, by kind (Ada's rule, 2026-11-06)

"We do not support arbitrary constants. We support learned and data derived values." Latency and throughput
come before memory. Every constant `cp/t7-native-macos` adds or relies on: native steps 1 and 2's macOS
process class and box helper, step 6's macOS installer and labs, and step 6's descriptor default.

Kinds:
- **(a)** learned at run time;
- **(b)** derived from a measured record;
- **(c)** a platform or spec limit, cited at its source;
- **(d)** picked, or copied from another system's defaults.

Status says what happens to each (d): converted, to convert (with the lead's call), or kept and why.
Branch tip at audit: 64a0500.

Left out: OS ABI values used as values rather than limits (`NOTE_*`, `EV_*`, kqueue idents, `KERN_SUCCESS`),
wire tags, exit codes, stdio indices, test pids and uids, unit conversions (`1 << 20` as MiB, `/ 1000` for
ns to µs, `NANOS_PER_SECOND`), and the darwin signal table, which is (c): XNU `bsd/sys/signal.h:84-123` and
asm-generic.

## Installer and labs (step 6, macOS)

| Name | Value | Where | Kind | Status |
|---|---|---|---|---|
| Box users' role range | 450-499 | scripts/install/macos.sh:42-43 | (c) Apple's `sysadminctl` usage, macOS 26.4 | Converted, 64a0500. Was 200-400 from Nix's note |
| Box users' first id | first free run of COUNT+2 in 450-499 (450 on .local-2) | macos.sh `free_run` | (a) | Converted, 64a0500. Was FIRST_ID=320 (d). Test macos_ids_test.sh 12/12 |
| Pool size | COUNT=30 | macos.sh:38 | (d) Nix's 32 | Lead: grow on demand, learned, ahead by the node's observed peak of concurrent box-user boxes. Account creation in the root helper is a security question, brought first |
| Box user's process cap | NPROC=1024, and helper `DEFAULT_NPROC`=1024 | macos.sh:44, crates/hex-cli/src/box_helper.rs:15 | (d) step 2's default | Lead: the box's `pids_max` when its spec gives one, else `kern.maxprocperuid` read at run time (c). Linux sets no default either (`pids.max` only from `pids_max`, hex-core cgroup.rs:86) |
| Helper start wait | WAIT_STEPS=300 (30 s, 0.1 s polls) | macos.sh:46,166-171 | (d) | Lead: wait on the fact; deadline derived from the lab's recorded start times. Needs the lab to record them first |
| dscl retries | DSCL_TRIES=3 | macos.sh:49 | (d) Nix's | Lead: retry only eNotYetImplemented, until a read of the record answers, or a deadline from measured transient durations. None measured yet |
| File modes | folder 0750 root:hexnode; Umask, so socket 0770; 0755 binaries; 0644 plist and log | macos.sh:195-237 | (c) access policy, the folder rule cited (research §3.6: open.2:117, launchd.plist.5:293-296) | Kept |
| Lab watchdog | 1800 s | bench/install/macos_lab.sh:54 | (d) | To (b) from the lab's recorded duration |

## Descriptor default (step 6)

| Name | Value | Where | Kind | Status |
|---|---|---|---|---|
| NOFILE soft | 1024 | crates/hex-core/src/limits.rs:7 | (c) select(2)'s FD_SETSIZE; Docker 29's | Kept |
| NOFILE hard | 524288 | limits.rs:9 | (d) systemd's DefaultLimitNOFILE, via Docker 29 | Kept: Ada's call, for compatibility. The lead names it to her under her rule. On macOS the kernel's kern.maxfilesperproc (138,240 measured) binds first |
| Cap at the runner's hard limit | getrlimit at run time | helper_sys.rs:203-205 | (a) | Kept |

## Process class (step 1)

| Name | Value | Where | Kind | Status |
|---|---|---|---|---|
| Watcher floor | 250 µs | crates/hex-core/src/watch.rs:18 | (b) RESULT.md, 9694770 (256 MiB held to 42 MiB at 164 GiB/s) | Kept |
| Watcher ceiling | 100 ms | watch.rs:23 | (b), weakly: RESULT.md says a box read this often costs "next to nothing"; no number fixes 100 | To derive from a measured read-cost curve |
| RT computation budget | 150 µs per 250 µs | crates/hex-runtime/src/macos/process.rs:477 | (b) rtbudget on studio-3, 2026-11-01 | Kept |
| Group drain patience | 10 s | process.rs:43 | (d), a failure bound ("a SIGKILL ends members in milliseconds") | To (b) from measured killpg-to-empty times |
| Listing capacity fallback | 4096 when kern.maxproc can't be read | process.rs:380, 409, 454 | (d) | To a typed refusal: no listing without kern.maxproc |
| Event batch | EVENTS=64 | sys.rs:512 | (d) a buffer; more events wait a call (EV_CLEAR, kern_event.c:1139) | To (b) from measured events per wait, or keep as a batch size with no effect on correctness. Lead's call |
| kevent change chunk | 64 | sys.rs:560 | (d) | Same as EVENTS |
| Background clamp under weight | cpu_weight < 100 | process.rs:750 | (c) 100 is cgroup v2's default cpu.weight (the API's own default) | Kept |
| Jetsam priority band | 40 | sys.rs:298 | (b) jetsam_probe.c, the band the fatal kill was confirmed at | Kept |
| Jetsam cap floor and ceiling | 1 MiB; c_int's max | sys.rs:324-325 | (c) the SPI's MiB unit and int type | Kept |
| Order hash multiplier | 0x9e3779b9 | process.rs:586 | (c) the golden-ratio constant of Fibonacci hashing (Knuth, TAOCP vol. 3 §6.4) | Kept; cite it in the code |

## Box helper (step 2)

| Name | Value | Where | Kind | Status |
|---|---|---|---|---|
| Ledger reserve | 16 MiB, capped at bound/4 | crates/hex-runtime/src/macos/mod.rs:36,150 | (d), its comment says "a constant until measured" (boxlab.sh) | To (b) from the arming-window allocation boxlab measures (Ada's root lab) |
| Crossing pump timeout | 500 ms | box_helper.rs:74 | (d) | The daemon's sweep pace (layer three). To (a), the watch rule's own wait for the boxes it holds (watch.rs), as rootless mode does |
| User drain patience | 10 s | helper_daemon.rs:36 | (d), a failure bound | To (b) as for the group drain |
| Reap patience | 100 ms | helper_daemon.rs:42 | (d), a bound on a wait asked of a running program | To (b) from measured NOTE_EXIT-to-zombie times (kern_exit.c:2594-2664), or remove: the node asks only after NOTE_EXIT |
| Listen backlog | 16 | helper_daemon.rs:649 | (d) | The helper serves one node connection at a time. To (c), the socket's own: SOMAXCONN, or 1 |
| Programs per box | nproc | helper_daemon.rs:268 | follows #NPROC | Converts with NPROC |
| Ended boxes kept | pool size | helper_daemon.rs:269 | follows the pool | Converts with the pool |
| Message cap | MAX_MESSAGE=64 KiB | crates/hex-core/src/helper.rs:35 | (d), argued ("holds any a caller passes in practice") | To (c): the program's own ARG_MAX (sysctl kern.argmax) plus the header, read at run time |
| Mach constants, message layout | EXC_MASK_RESOURCE 1<<11 … MACH_RCV_TIMED_OUT; trailer 68 B; reply id+100 | helper_sys.rs:84-140, 299 | (c) mach headers; trailer 68 and id+100 uncited | Cite: the trailer is mach_msg_max_trailer_t's size; +100 is MIG's reply-id rule |

## Tests (steps 1, 2, 6)

| Name | Value | Where | Kind | Status |
|---|---|---|---|---|
| Waits on a program's end | 10 s, 30 s, 20 s budget; lateness budget/2 | macos_process_box.rs:132,171,207,240,269,310,337,345; hex-runtime macos/tests.rs:193,303,308 | (d) | To (b) from the suite's measured exec-to-exit distributions (launchd runs, outside kiln's pacing) |
| Bench and test rates | 256 GiB/s, 1,000,000 procs/s | macos_process_box.rs:68-69; watch.rs:298 | (b) RESULT.md's allocation rate | Kept |
| Overshoot bar | 64 MiB | macos_process_box.rs:178-181 | (b) the step's bench bar (criterion 2) | Kept |
| Loop and case counts | 60 programs; proptest 512/2048/4096 cases; spin budget 100,000 | macos_process_box.rs:339; watch/ledger/boxuser/helper tests; helper_daemon/tests.rs:380 | (d) | 60 is (b): the bench's 3-of-60 failure. Proptest case counts: to the count giving a stated miss probability. The spin budget: to a wait on the fact |
| Test bounds and footprints | 256 MiB, 128 MiB, 2 GiB hogs, nproc 64/2 | tests across the line | Test inputs, not limits | Kept |
| launchd one-shot limits | 2 h, 1 h | outside the repo (run scripts) | (d) | To (b) from recorded run times |
