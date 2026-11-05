# Each OS's `process` class against its named competitors (native plan step 7)

`crates/hex-bench/benches/native_box.rs` drives Hex's `process` class through the hex-api `Runtime` seam and, on the
same machine, the tools each OS offers for the same job (the choice of competitors, class for class:
`docs/research/runtime/native-process-competitors.md`). Every box is bounded to 256 MiB. Each pass times 200
lifecycles after 20 warmup: create, start (to the program's "up" line, `hex selftest-box serve`) and delete (to
the program's end), each alone; a competitor with no separate create times its create and start together as
start. Then 20 programs climb to 1 GiB a MiB at a time, each reporting the MiB it touched (`climb`): the last
one reported is how far it got, and the time from its start to its end is how fast the bound ended it. Three
passes per driver, interleaved; the figure given is the best pass's, with the three passes' range beside it.
The `oci` class's create, start and delete against runc, crun, youki and gVisor are in
`bench/competitive/RESULT.md`.

## Linux: Hex vs a systemd transient scope

Record `records/native-box-linux-1aa735c-20261105T090107Z.txt`: fedora (i9-14900HX, Linux 6.19.10), commit
1aa735c (abdbfd0 changed only macOS code), 2026-11-05 09:01-09:02Z, CPUs 4-15 under the timed lock, load 18-35
with 7 containers running (noisy, as recorded).

| ms | Hex | `systemd-run --user --scope` | Hex ahead by |
|---|---|---|---|
| create p50 | 0.160 (0.160-0.169) | none (its start is its create) | |
| start p50 | 0.397 (0.397-0.412) | 4.395 (4.395-18.315) | 11.1x; 7.9x counting Hex's create |
| start p99 | 0.691 (0.691-1.409) | 26.622 (26.622-30.014) | 38.5x |
| delete p50 | 0.696 (0.696-0.701) | 10.264 (10.264-10.632) | 14.7x |
| bound held | 20/20 per pass, 2 MiB under | 20/20 per pass, 2 MiB under | equal |
| bound's end p50 | 99.5 (99.5-106.2) | 117.5 (117.5-123.2) | 1.18x |

## macOS: Hex vs `taskpolicy -m` and a launchd job

Record `records/native-box-macos-b1ddb89-20261105T110227Z.txt`: adas-mac-studio-2 (Apple M4 Max, macOS 26.2),
commit b1ddb89, 2026-11-05 11:02-11:03Z, a one-shot launchd job outside kiln's runs, load 2.2-4.7. kiln's headroom
guard there had learned a 0.0068-core budget, but it stopped only kiln-run pids: the bench's processes, sampled
every 0.5 s, were never stopped.

| ms | Hex | `taskpolicy -m 256` | launchd job (`ResidentSetSize`) | Hex ahead of the best by |
|---|---|---|---|---|
| create p50 | 0.010 (0.010) | none | none | |
| start p50 | 0.850 (0.850-0.854) | 1.591 (1.591-1.626) | 24.522 (24.522-25.648) | 1.87x; 1.85x counting create |
| start p99 | 0.924 (0.924-1.249) | 1.722 (1.722-2.098) | 34.942 | 1.86x |
| delete p50 | 0.147 (0.147-0.151) | 0.132 (0.132-0.135) | 5.590 | 0.90x (taskpolicy's delete kills one process; Hex's ends a box) |
| bound held | 20/20 per pass, 3 MiB under | 20/20 per pass, 3 MiB under | **0/20: ran to 1 GiB, 768 MiB past** | equal with taskpolicy |
| bound's end p50 | 56.2 (56.2-56.4) | 57.1 (57.1-59.0) | not ended by it | 1.02x |

launchd's `HardResourceLimits` `ResidentSetSize` is advisory (`man launchd.plist`: "the system will prefer to
take memory from processes that are exceeding"), and here it ended none: a user's launchd job is no memory bound. `taskpolicy -m` arms the same fatal jetsam cap Hex arms on each member, so the two hold the bound
alike; Hex's start is quicker because it spawns the program directly rather than through `taskpolicy`'s own
exec.

The same bench at commit 1aa735c (2026-11-05 08:54-08:58Z, raw JSON on the Mac in
`~/.cache/hex-t7/native-s7/bench-20261105T085409Z/`) found a bug: 3 of Hex's 60 bound ends were reported at the
bench's 60 s wait budget, though each program was killed at 253 MiB like the rest. XNU posts the exit event
before the zombie can be reaped; fixed at abdbfd0
(`docs/research/runtime/macos-spawn-jetsam-cap.md`, "The exit event comes before the zombie"). Since: 0 of 60 at
abdbfd0 and 0 of 60 at b1ddb89.

## Windows: Hex vs Process Governor

Record: entry 8 of `bench/install/records/windows-jupiter-20261105.txt` on `cp/t7-native-windows` (a905168), the
Windows agent's run on jupiter (AMD Ryzen AI MAX+ 395, 32 logical CPUs, Windows NT 10.0.26200), built from a
detached worktree of commit b1ddb89, 2026-11-05 11:04:00-11:09:25Z. Load 11-77% CPU with 3-12 co-tenant
cargo/rustc. Process Governor 4.1.26190.22 (`procgov -q --maxjobmem 256M`, the GitHub release, sha256 recorded).
Args 200 20 20, interleaved, 3 passes. Raw JSON on jupiter: `C:\Users\corph\.cache\hex-t7-windows\records\native_box-b1ddb89-20261105T110322Z\`.
The suite on jupiter: `windows_process_box` 9 of 9 at 1a2e1eb and at e322d7b (entry 7).

| ms, p50 | Hex | Process Governor | Hex ahead by |
|---|---|---|---|
| create | 0.028 (0.028-0.042) | none (its start is its create) | |
| start | 21.9 (21.9-43.5) | 52.0 (52.0-58.5) | 2.4x |
| delete | 0.866 (0.866-0.984) | 221.6 (221.6-268.9) | 256x |
| bound held | 20/20 per pass, 6 MiB under | 20/20 per pass, 3 MiB under | equal |
| bound's end | 186.8 (186.8-197.6) | 406.6 (406.6-438.4; one 6,239 ms outlier in pass 1) | 2.2x |

Each climb under either ended `0xC0000409`: the program aborting on the commit its job refused. Hex's start fell
from 43.5 to 21.9 ms as the load dropped from 77% to 33% CPU, so read the Windows start as load-sensitive. An
earlier run at e322d7b (entry 7, n 500) gave Hex start 52.2 ms and delete 1.49 ms under 58-82% CPU; its procgov
passes failed on procgov's banner, fixed with `-q` at b1ddb89.
