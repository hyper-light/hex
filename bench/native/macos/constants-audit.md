# The macOS line's constants, by kind (Ada's rule, 2026-11-06)

"We do not support arbitrary constants. We support learned and data derived values." Latency and throughput
come before memory. This lists every constant `cp/t7-native-macos` adds or relies on: native steps 1 and 2's
macOS process class and box helper (step 2 merged at 84eca3f), and step 6's macOS installer, labs and
descriptor default.

Kinds:
- **(a)** learned at run time;
- **(b)** derived from a measured record;
- **(c)** a platform or spec limit, cited at its source;
- **(d)** picked, or copied from another system's defaults.

Left out: OS ABI values used as values rather than limits, wire tags, exit codes, stdio indices, test pids
and uids, unit conversions, and the darwin signal table, which is (c): XNU `bsd/sys/signal.h:84-123` and
asm-generic.

## Installer and labs (step 6, macOS)

| Name | Value | Where | Kind | How |
|---|---|---|---|---|
| Box users' role range | 450-499 | scripts/install/macos.sh `ROLE_FIRST`/`ROLE_LAST`; hex_core::pool::ROLE_UIDS | (c) | Apple's `sysadminctl` usage, macOS 26.4 (was 200-400, Nix's note). 64a0500, cc74620 |
| Box users' ids | first free run in 450-499 (450 on .local-2) | macos.sh `free_run` | (a) | Learned at install. 95e0557 test, 64a0500 |
| Pool size | grown on demand, one account per box past the pool | hex_core::pool, Daemon::growing, macos/directory.rs | (a) | Was COUNT=30, Nix's 32. The installer makes the groups and one user; the pool stays ahead by the kept peak. Tests 6863a8f, c2dff27; code cc74620, 6895232, a6aa2ba |
| Box user's process cap | the box's `pids_max`, else `kern.maxprocperuid` read at start | helper_daemon.rs, box_helper.rs, sys.rs | (c)/(a) | Was NPROC=1024. Linux sets no default either. dea5772 test, 6bbd791; §12 1c701ed held |
| Helper start wait | success: a connect accepted; failure: launchd's exit report; bound only while running and not accepting: 0.075360 s | macos.sh `wait_serving`, `START_BOUND_S` | (b) | 2 x the max of 20 proxy starts (record agentstart-40454fe-20261106T150233Z). f023f4b, 63e701c. Ada's lab prints the real daemon's time to confirm |
| dscl writes | once; a failure refused naming dscl's error | macos.sh `dscl_write` | none | No retry (was DSCL_TRIES=3, Nix's): eNotYetImplemented never seen. d0d8291, 8ba8ef5 |
| Uninstall's process wait | by the fact (pgrep finds none), bound 2 x (0.883 ms + 0.298 ms x (n-1)) | macos.sh `end_pool_processes`, `drain_bound_s` | (b) | Was WAIT_STEPS 30 s. Record killscale-a84e094-20261106T160339Z. 3af15a2, 925771c |
| File modes | 0750 folder, Umask 7 (socket 0770), 0755, 0644 | macos.sh | (c) | Access policy; research §3.6 cites open.2:117, launchd.plist.5:293-296 |
| Lab watchdog | 1800 s | bench/install/macos_lab.sh | (d) | A hang guard for an admin run; to (b) from Ada's recorded lab duration |

## Descriptor default (step 6)

| Name | Value | Where | Kind | How |
|---|---|---|---|---|
| NOFILE soft | 1024 | hex-core limits.rs | (c) | select(2)'s FD_SETSIZE |
| NOFILE hard | 524288 | limits.rs | (d) | Ada's call (Docker 29's default, for compatibility); the lead names it to her. On macOS kern.maxfilesperproc (138,240 measured) binds first |
| Cap at the helper's hard limit | getrlimit | helper_sys.rs | (a) | |

## Process class (step 1)

| Name | Value | Where | Kind | How |
|---|---|---|---|---|
| Watcher floor | 250 µs | hex-core watch.rs | (b) | RESULT.md, 9694770 |
| Watcher ceiling | 100 ms | watch.rs | (b), weak | RESULT.md says a box read this often costs "next to nothing", but no measurement fixes 100. Open: needs a read-cost curve (reads per second against CPU) to derive the ceiling as the slowest pace whose cost is under the stated budget. Step 1's code; the lead's call whether this line measures it |
| RT computation budget | 150 µs per 250 µs | process.rs | (b) | rtbudget on studio-3 |
| Group drain | 2 x (0.883 ms + 0.298 ms x (n-1)) | hex_core::drain, process.rs | (b) | Was DRAIN_PATIENCE 10 s. Record killscale-a84e094. ffcec11 test, 9c72dac |
| Listing size | kern.maxproc, or refused naming it | sys::listing | (c) | Was a 4096 fallback at five sites. 54e8ad0 test, 4f41305 |
| Event batch | EVENTS = 64, also the registration chunk | sys.rs | (d) | A batch size that changes no outcome: more events wait one more call (EV_CLEAR coalesces a process's events, kern_event.c:1139). The second 64 merged into it (c376bc2). Open: derive it from measured events per wait under the largest box, or accept it as a batch size; the lead's call |
| Background clamp | cpu_weight < 100 | process.rs | (c) | 100 is cgroup v2's default cpu.weight |
| Jetsam band | 40 | sys.rs | (b) | jetsam_probe.c |
| Jetsam cap floor and ceiling | 1 MiB; c_int's max | sys.rs | (c) | The SPI's unit and type |
| Order hash multiplier | 0x9e3779b9 | process.rs | (c) | Fibonacci hashing, Knuth TAOCP vol. 3 §6.4 (cited, 622902f) |

## Box helper (step 2)

| Name | Value | Where | Kind | How |
|---|---|---|---|---|
| Ledger reserve | 16 MiB, capped at bound/4 | macos/mod.rs | (d) | Step 2 derives it from Ada's root lab record (armlat); its SHA comes before the lab is re-pinned |
| Sweep pace | none | helper | none | Was pump(500). Event-driven: crossings and member forks and ends wake it (step 2's b05d3a0, merged) |
| End drain | hex_core::drain::bound | helper_daemon.rs | (b) | As the group drain. Step 2 checks Ada's box-user drain record against it |
| Reap wait | none | helper_daemon.rs | none | Was REAP_PATIENCE 100 ms. Wait answers at once, try_wait (step 2's 469b093) |
| Listen backlog | SOMAXCONN | helper_daemon.rs | (c) | `bsd/sys/socket.h:597` (step 2) |
| Message cap | from ARG_MAX, PATH_MAX, NGROUPS_MAX | hex-core helper.rs | (c) | Step 2's 469b093 |
| Mach constants | mach headers; trailer 68 B = MAX_TRAILER_SIZE; reply id + 100 | helper_sys.rs | (c) | message.h:563-564 (622902f); ipc_kobject.c:402 (step 2) |
| Probe waits | 30 s, usleep 300 ms | boxuser_probe.c | (d) | Step 2 derives them from Ada's record |

## Tests

| Name | Value | Where | Kind | How |
|---|---|---|---|---|
| Fact waits | HANG_GUARD 39.507 s | macos_process_box.rs, macos/tests.rs, helper_daemon/tests.rs | (b) | 2 x the slowest recorded run of the tests that wait (record test-waits-macos.txt, step 2's 4f09e8f). A guard only ends a hang. This line's per-act 2 x 30-sample waits (e436b28) flaked as latency assertions and were replaced at 84eca3f |
| A bound kill's lateness | ENDED_AT_BOUND 930.938 ms, budget HANG_GUARD | macos_process_box.rs | (b) | 2 x the max of 30 hog kills (record execexit-a82ce7b-20261106T161405Z); the one test that claims latency |
| Property-test cases | derived from the rarest case's share | watch, ledger, boxuser, helper tests | (b) | Step 2's f262faf |
| Pool property test | proptest's default (256 cases) | hex-core pool.rs | (d) | Copied default. Its rare branch (a full range) is pinned by its own deterministic test; to derive the count from the share of pools that hold a pool name |
| Measurement hang guard | 600 s a sample | macos_process_box.rs measure_exec_to_exit | (d) | A measurement tool's guard under its launchd limit; not a check |
| Test inputs | 256 MiB bounds, 2 GiB hogs, 60 programs, rates | tests | inputs | 60 is (b), the bench's 3 of 60; the rates are RESULT.md's |

## CLAUDE.md §12 lines on this branch

| Commit | Line | State |
|---|---|---|
| caa6e9f | helper_sys.rs: the box helper's child sets `RLIMIT_NOFILE` beside `RLIMIT_NPROC` | Approved by Ada, 2026-11-06, as written (git note on caa6e9f) |
| d4252f3, a960732 | helper_sys.rs (step 2): the file, its fork, user drop, ledger and exception arming; the mach_msg crossing receive | Approved by Ada, 2026-11-06 (step 2's cd46d31, merged) |
| 1c701ed | sys.rs: `sysctlbyname` also reads `kern.maxprocperuid` | HELD FOR ADA; step 7's 08d52a0 adds `kern.argmax` to the same line |
| b0cfaae | sys.rs fork for a plain box's descriptor limit | Never landed: reverted at 1c8309f |
