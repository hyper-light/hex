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

Record `records/native-box-macos-abdbfd0-20261105T095826Z.txt`: adas-mac-studio-2 (Apple M4 Max, macOS 26.2),
commit abdbfd0, 2026-11-05 09:58-09:59Z, a one-shot launchd job outside kiln's runs (whose headroom guard
pauses them), load 5.7-7.0.

| ms | Hex | `taskpolicy -m 256` | launchd job (`ResidentSetSize`) | Hex ahead of the best by |
|---|---|---|---|---|
| create p50 | 0.009 (0.009-0.010) | none | none | |
| start p50 | 0.852 (0.852-0.853) | 1.579 (1.579-1.606) | 26.736 (26.736-27.535) | 1.85x; 1.84x counting create |
| start p99 | 0.960 (0.960-1.265) | 1.702 (1.702-2.113) | 36.146 | 1.77x |
| delete p50 | 0.147 (0.147-0.152) | 0.131 (0.131-0.132) | 9.138 | 0.89x (taskpolicy's delete kills one process; Hex's ends a box) |
| bound held | 20/20 per pass, 3 MiB under | 20/20 per pass, 3 MiB under | **0/20: ran to 1 GiB, 768 MiB past** | equal with taskpolicy |
| bound's end p50 | 56.7 (56.7-56.8) | 57.5 (57.5-58.0) | not ended by it | 1.01x |

launchd's `HardResourceLimits` `ResidentSetSize` is advisory (`man launchd.plist`: "the system will prefer to
take memory from processes that are exceeding"), and here it ended none: a user's launchd job is no memory bound. `taskpolicy -m` arms the same fatal jetsam cap Hex arms on each member, so the two hold the bound
alike; Hex's start is quicker because it spawns the program directly rather than through `taskpolicy`'s own
exec.

The same bench at commit 1aa735c (2026-11-05 08:54-08:58Z, raw JSON on the Mac in
`~/.cache/hex-t7/native-s7/bench-20261105T085409Z/`) found a bug: 3 of Hex's 60 bound ends were reported at the
bench's 60 s wait budget, though each program was killed at 253 MiB like the rest. XNU posts the exit event
before the zombie can be reaped; fixed at abdbfd0
(`docs/research/runtime/macos-spawn-jetsam-cap.md`, "The exit event comes before the zombie"). At abdbfd0: 0 of 60.

## Windows: Hex vs Process Governor

Not run yet: jupiter is offline. The bench's `procgov` driver is built and linted for `x86_64-pc-windows-msvc`.
