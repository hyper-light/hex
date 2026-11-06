# Native step 5's constants, against Ada's rule (2026-11-06)

Ada's rule: "We do not support arbitrary constants. We support learned and data derived values." Latency and
throughput come before memory. Every constant step 5 adds or relies on ends as one of:

- (a) learned at runtime from what the system sees;
- (b) derived from measured data, with its record;
- (c) a platform or spec limit, cited at its source;
- (d) left, with a written reason, for Ada.

Two more outcomes appear below. **fact**: the clock or bound is gone, and the wait is on the event itself
(CLAUDE.md §4). **input**: a value a test chooses to exercise a bound. It is not tuning, and its test says so.

The audit before conversion is commit 015d32c (Hex `cp/t7-native-s5-route` `2660759`, Shards `hex/shards-api`
`a0cced87`). This revision is the conversion:
- task D's rows at Hex `cp/t7-native-s5-consts`, with line numbers at its tip;
- Shards' `crates/api` at `hex/shards-api-constants` `e72c551b` (local, not pushed, off the pushed `a0cced87`;
  Hex's `vendor/shards-api-draft` carries it on this branch only).

The rows in agent C's files are marked **C** and are merged from C's branch:
- `crates/hex-client/src/guest/stage.rs` and its tests;
- `crates/hex-core/src/stream/files.rs` and its tests;
- `crates/hex-node/src/relay/files.rs` and the files part of `relay/tests.rs`;
- `crates/hex-cli/src/selftest/tree.rs`;
- `crates/hex-cli/tests/guest_tree.rs`.

Relied-on constants are other steps'. Each is listed with its owner, and with whether step 5's use of it is right.

## Totals for task D's rows

Counted from the tables below. A table row may group several audited rows of one kind, as the audit did for
tests.

| final kind | Hex table rows | Shards table rows |
|---|---|---|
| fact | 13 | 1, plus parts of 1 |
| a | 5, plus part of 1 | 1, plus parts of 1 |
| b | 2, plus part of 1 | part of 1 |
| c | 34 | 10, plus parts of 3 |
| input | 5 | 2, plus parts of 2 |
| relied on (another step's) | 10 | — |
| d, left for Ada | 0 | 0 |
| C's (pointers to C's branch) | 7 | — |

Behaviour fixed on the way:
- **HEX-GUEST-32.** stdio connections had a 300 s send bound, which cut the stdin of a program that left it
  unread for five minutes. A local exec never does that (CLAUDE.md §8).
- **HEX-GUEST-33.** Every dial borrowed `ANSWER_PATIENCE` as a clock on the guest's boot.
- **HEX-GUEST-7.** The relay's stdio name read was serial, so one silent connection held up every other exec's
  streams for 5 s.

## hex-cli (src)

| name | value | file:line | kind | source or record |
|---|---|---|---|---|
| vsock listen backlog | `i32::MAX` → `net.core.somaxconn` | crates/hex-cli/src/guest.rs:137 | a | listen(2): a larger backlog is truncated to `/proc/sys/net/core/somaxconn` (net/socket.c `__sys_listen_socket`); the guest's own value |
| VMADDR_CID_ANY | `u32::MAX` | guest.rs:133 | c | vsock(7) |
| `MOUNTINFO_BOUND` | gone: read to its end, a line at a time | guest.rs:358 | c | Its length is the kernel's: a line per mount, bounded by `fs.mount-max` (Documentation/admin-guide/sysctl/fs.rst). At this point it lists the initramfs and the `/proc` just mounted. HEX-GUEST-31 |
| `MAX_SHARES` | 17 (x86_64) / 31 (aarch64) | guest.rs:382 → crates/hex-runtime/src/guest/shares.rs:27 | c | One virtio-fs device per share, beside vsock, among Shards' virtio-mmio slots: `VIRTIO_MMIO_MAX` 18 (crates/vmm/src/arch/x86_64/mod.rs:47, the free IOAPIC pins) and 32 (aarch64/mod.rs:33) at Shards a0cced87. Host and init use the same number |
| `CMDLINE_BOUND` | 2048 (x86_64, aarch64) / 512 | guest.rs:397 | c | `COMMAND_LINE_SIZE`: arch/x86/include/asm/setup.h:7, arch/arm64/include/uapi/asm/setup.h, include/uapi/asm-generic/setup.h:5. `/proc/cmdline` prints its terminating zero as a newline (fs/proc/cmdline.c) |
| `LINE_BOUND` | gone: the exact `ready` line's length | guest.rs:473 | c | The child's line is known: the node's `ready <socket>\n` (crates/hex-cli/src/node.rs:84), the relay's `ready\n`. It is read to that length and matched whole |
| tmpfs root `mode=0755` | 0755 | guest.rs:337 | c | Search on `/` for every user (path_resolution(7)) and writes for root alone: ramfs's `RAMFS_DEFAULT_MODE` (fs/ramfs/inode.c), where tmpfs's default 01777 (tmpfs(5) `mode=`) would let any user make names in `/` |
| tree rows (`ENTRIES`, `XATTR_*`, sha256 buffer) | — | crates/hex-cli/src/selftest/tree.rs | C | C's |

## hex-cli (tests)

| name | value | file:line | kind | source or record |
|---|---|---|---|---|
| `HEX_GUEST_CPUS` default | the host's CPUs | crates/hex-cli/tests/guest_real.rs:126 | a | `shards_api::max_vcpus()` (std's `available_parallelism`); the fastest run, as an idle vCPU costs nothing |
| `HEX_GUEST_MEMORY_MIB` default | `min_memory_mib(ARCH)` + 2 × 284 = 632 MiB (aarch64) | crates/hex-cli/tests/guest_real.rs (`PEAK_MIB`) | b | bench/native/guest/peak-rss/RECORD.md: the VM's peak RSS under HEX-GUEST-19, the most of 3 runs on HVF. ×2 is the parent's stated margin, as C's guest_tree takes. x86_64 isn't measured yet (fedora and jupiter down) |
| guest vcpus / memory_mib, walk 600 s, `take(40)`, `ANSWER_PATIENCE` | — | crates/hex-cli/tests/guest_tree.rs | C | C's file. Line 318 was edited here on the parent's word, because the link no longer takes a patience (HEX-GUEST-33) |
| `SUITE` | per class: macOS 2 × 73.75 s, Windows 2 × 75.5 s | crates/hex-cli/tests/guest_validation.rs (`suite()`) | b | Twice (the margin) the slowest HEX-GUEST-20 run on record for the class (bench/native/guest/validation-runs/RECORD.md). Linux (KVM): **record pending**, so the test names it rather than borrow another class's |
| `HEX_GUEST_CPUS` default | the host's CPUs | guest_validation.rs:238 | a | as above |
| `HEX_GUEST_MEMORY_MIB` default | `min_memory_mib(ARCH)` + 2 × 975 = 2014 MiB (aarch64) | guest_validation.rs (`PEAK_MIB`) | b | the same record, under HEX-GUEST-20. At 2014 MiB it passes with the record's 28 of 58 |
| poll interval | gone: re-read after as long as the last read took | crates/hex-cli/tests/guest_validation.rs (`reread`) | a | The lead's call: step 7's learned re-read (cp/t7-native-s7 `hex_api::differential::until`), so reading takes at most half the time, at a read's own cost (HEX-GUEST-35). Why it isn't a fact: `hex_api` has no blocking wait for a box's end. A box's init gets no stdio through the seam, and `inspect` reports `BoxInfo.exit` only after the end. `Runtime::wait(id, timeout)` is noted as a later seam improvement for every class |
| sim guest vcpus / memory | 1 / `min_memory_mib(ARCH)` | crates/hex-cli/tests/node_socket.rs:490 | c | Shards' least guest (`shards_api::check`; min_memory_mib cites shards-vmm vm/{aarch64,x86_64}.rs:28) |
| `wait_for` 10 ms park; fixture `sleep 0.01` | — | node_socket.rs:1012, :790 | relied | goal 1's (0f41abb, 2026-10-27), not step 5's; not in the step-5 audit, so they're listed for goal 1 to convert |

## hex-client

| name | value | file:line | kind | source or record |
|---|---|---|---|---|
| `COPIER_STACK` | gone: std's default stack | crates/hex-client/src/guest.rs:99 | c | std's default thread stack, reserved, not touched. No memory cut at a latency risk |
| reader buffer | `stream::MAX_FRAME` | guest.rs:193 | c | One whole frame at the wire's bound: the unit the deframer takes |
| stdio send timeout = `ANSWER_PATIENCE` | gone | guest.rs:93 | fact | The copier ends with its connection, when the exec does, as a local exec's pipe does (HEX-GUEST-32; this was a bug) |
| control send timeout = `ANSWER_PATIENCE` (relied on) | 300 s | guest.rs:245 | relied | The use is right: it is the local client's own socket bound (crates/hex-client/src/linux.rs:132), so both paths agree (CLAUDE.md §8). `ANSWER_PATIENCE` is the client's (goal 1) and is (d) there |
| dial patience = `ANSWER_PATIENCE` | gone: the first dial `GuestRuntime::first_dial_bound`, later `ZERO` | crates/hex-client/src/guest/shards.rs:54, crates/hex-runtime/src/guest/mod.rs (`recorded_boot()`) | b, then a | The lead's rule for learned waits, keyed by host class at compile time. macOS (HVF): before any boot, 2 × (the margin) the record's slowest boot to ready, 265 ms, taken quiet and under load on both Mac Studios (bench/native/guest/boot-ready/RECORD.md). Then 2 × max(record, this node's slowest), only rising. A hung guest is stopped, counted and booted once more; a second hang is refused by type (HEX-GUEST-33, -34). Linux (KVM) and Windows (WHP): **record pending**. A guest there is refused by name, `Platform`, naming the record, until it is taken |
| stage rows | — | crates/hex-client/src/guest/stage.rs | C | C's |
| `ANSWER_PATIENCE` call wait (relied on) | 300 s | calls.rs:296 | relied | goal 1's. The use is the client's own, unchanged by step 5 |
| `MAX_LIST` (relied on) | 1<<20 | calls.rs:102 | relied | goal 1's. Unused by step 5 |
| `wire::MAX_MESSAGE` (relied on) | 128 KiB | wire | relied | The wire's. Step 5 reads frames at it (`MAX_FRAME`), the same as the local socket |
| negotiated `message_bytes` | from the hello | calls.rs:211 | a | the agent's hello |
| pre-hello limits (relied on) | 1 / 1 / 0 | calls.rs:195-198 | relied | goal 1's |
| test waits 30 s | gone | crates/hex-client/src/guest/tests.rs | fact | `next_request` blocks on the bytes or the end; `wait_for(Duration::MAX)` waits on the exit |
| stand-in hello limits | 1, 4, 4 | guest/tests.rs:75 | input | the stand-in's own, more than any test makes |

## hex-core

| name | value | file:line | kind | source or record |
|---|---|---|---|---|
| `CONTROL_PORT` / `STDIO_PORT` | 0x4845_5800 / 01 | crates/hex-core/src/stream.rs:27 | c | Identifiers. vsock(7): a `u32` above the privileged ports (below 1024 need `CAP_NET_BIND_SERVICE`) and below `VMADDR_PORT_ANY`. Assigned in docs/design/shards-guest.md §3a; HEX-GUEST-30 checks the doc |
| `PREFIX` | `size_of::<u32>()` | stream.rs:31 | c | §3a's frame prefix (a big-endian u32) |
| `TOKEN_BYTES` | `size_of::<u64>()` | stream.rs:39 | c | §3a's token |
| `NAME_BYTES` | the sum of its fields, 24 | stream.rs:44 | c | §3a's stdio-name table, summed in code and checked against the doc (HEX-GUEST-30) |
| `MAX_FRAME` (new) | `MAX_MESSAGE + PREFIX` | stream.rs:35 | c | one frame at the wire's bound |
| `FILES_PORT`, `META_BYTES`, `XATTRS_MAX`, `OPEN/HEAD_BYTES`, `LIMITS.*` | — | crates/hex-core/src/stream/files.rs | C | C's; the port is in §3a's table |
| gather patience | gone: `Gather::forget(conn)` | stream.rs:426 | fact | A set goes when its control connection ends (HEX-GUEST-3) |
| `BoxId::MAX_LEN` (relied on) | 128 | hex-api | relied | goal 1's |
| test bounds 100, 256, 0..512, 4 sets | — | crates/hex-core/src/stream/tests.rs:7 | input | stated in the module doc |
| test gather patience 5 s | gone | — | fact | with the clock |

## hex-node

| name | value | file:line | kind | source or record |
|---|---|---|---|---|
| relay `connections` = `serve::LIMITS.connections` (relied on) | 64 | crates/hex-node/src/relay.rs:67 | relied | The node socket's own (goal 1's, (d) there). The use is right: each host connection holds one of the node's |
| relay `sets` | connections × in_flight | relay.rs:67 | c | From the relied-on `serve::LIMITS`, so a host within the node's bounds never meets it |
| relay `unnamed` (new) | sets × `STREAMS` | relay.rs:67, :77 | c | Every stream of every set naming itself at once. `STREAMS` = 3, §3a. One past it is refused `Exhausted`, logged `hex.guest event=stdio_unnamed` |
| relay `patience` | gone | — | fact | An exec is parked until its set is whole or its host's connection ends (HEX-GUEST-7). Exec start and throughput are measured before and after (bench/native/relay/) |
| `NAME_PATIENCE` | gone | relay.rs:254 | fact | The name is read on the stream's own thread, until it comes or the host ends the connection, bounded by count (`unnamed`), not by time |
| `COPIER_STACK` | gone: std's default | relay.rs | c | as hex-client's |
| host request buffer | `stream::MAX_FRAME` | relay.rs:348 | c | one frame |
| answers buffer `MAX_MESSAGE`, `MAX_FDS` (relied on) | 128 KiB / 3 | relay.rs:553 | relied | the wire's; one message, as the node sends |
| relay/files.rs rows | — | crates/hex-node/src/relay/files.rs | C | C's |
| test patience 300 ms, `readable` 900 ms | gone | relay/tests.rs (HEX-GUEST-7) | fact | The new tests wait on the host's end, the node's connection end, and poll(0) as a check of the fact at that moment |
| test `wait_for` 30 s (HEX-GUEST-6) | 30 s | relay/tests.rs | C | C's row |
| test `connections: 1`, `sets: 1`, `unnamed: 2` | — | relay/tests.rs | input | inputs to bound tests |

## hex-runtime

| name | value | file:line | kind | source or record |
|---|---|---|---|---|
| guest cmdline `panic=-1` | -1 | crates/hex-runtime/src/guest/mod.rs:131 | c | kernel-parameters.txt `panic=`: a negative timeout reboots at once (kernel/panic.c `panic_timeout`) |
| dial/boot patience = `ANSWER_PATIENCE` | gone: learned, as above | mod.rs | b, then a | HEX-GUEST-34 |
| `files::LIMITS` (relied on) | — | mod.rs | C | C's |
| `MAX_CONFIG_BYTES` | `hex_core::bundle::MAX_CONFIG_BYTES` | crates/hex-runtime/src/guest/rewrite.rs:16 | relied | It is now the oci class's own bound, not a copy (goal 1's; (d) there: "a spec is a few kilobytes"). The use is right: the guest's node parses under the same bound |
| `MAX_SHARES` | 17 / 31 | crates/hex-runtime/src/guest/shares.rs:27 | c | as hex-cli's above |
| default shares | macOS 5 dirs; Windows users + temp | shares.rs:118 | c | Docker Desktop's default file-sharing list (research §7): the reference behaviour Hex matches, a list and not a bound |
| `MAX_DESCENDANTS` = `MAX_BOXES` (relied on) | 65,536 | crates/hex-runtime/src/linux/cgroup.rs:30 | relied | goal 1's; unchanged by step 5 |
| GetTempPath2W buffer | 261 | crates/hex-runtime/src/windows/sys.rs:96 | c | MAX_PATH+1, its documentation |
| test config | 1 / `min_memory_mib(ARCH)` | crates/hex-runtime/src/guest/tests.rs:310 | c | Shards' least guest |
| test env 2 / 1024 | — | guest/tests.rs:386 | input | inputs to the parse |
| fixture `sleep 1` | — | guest/tests.rs:114 | input | a box program's argv in a rewrite fixture: data |

## scripts/guest and bench

| name | value | file:line | kind | source or record |
|---|---|---|---|---|
| `CONTROL_PORT` / `TOKEN_BYTES` copies | 0x48455800 / 8 | scripts/guest/boot_check.py:28 | c | §3a |
| `--deadline` 60 s | gone | boot_check.py | fact | It waits on init's `hex guest ready` on the console, or on the VM's end (init powers off on failure; `panic=-1`) |
| handshake timeout 5 s | gone | boot_check.py | fact | one handshake after ready; the VM's end closes the socket |
| backoff 0.01→0.5 s | gone | boot_check.py | fact | no retries: the console line says the relay listens |
| OK line cap 64 B | `len("OK 4294967295\n")` = 14 | boot_check.py:25 | c | Firecracker docs/vsock.md, "Host-Initiated Connections" |
| VM size | the host's CPUs / least + 2 × 284 MiB | scripts/guest/boot_check.py (`PEAK_MIB`, `guest_memory_mib`) | a / b | `os.cpu_count()`; HEX-GUEST-19's peak record, as a boot is part of that workload |
| terminate grace 10 s | per class: macOS 2 × 0.013 s | boot_check.py (`STOP_GRACES`) | b | Twice (the margin) the slowest stop on record for the host's class: SIGTERM to the VM's end, N=40 on HVF, max 13 ms (bench/native/guest/stop/RECORD.md), then SIGKILL. Linux and Windows: **record pending**. The script refuses there except with `--record`, which waits on the VM's end alone to take the record |
| `panic=-1` copy | -1 | boot_check.py:31 | c | kernel-parameters.txt |
| `BENCH_MEMORY` 8g / `BENCH_OOM` 1000 / `CARGO_BUILD_JOBS` 4 | — | scripts/guest/build.sh:41, :87 | c | The lead's shared fedora limits (hex-slot; the T7 fedora-unit-limits rule): an operator's limit on a shared host |
| `Acquire::Retries` 5 | gone | build.sh | fact | CLAUDE.md §4: no retry. A failed fetch fails the build and is rerun by the operator |
| /dev/console rdev | 5:1 | scripts/guest/initramfs.py | c | devices.txt |
| newc pad, `%08x` | 4 / 8 | initramfs.py | c | buffer-format.rst |
| initramfs modes | 0755 / 0600 / 0755 | initramfs.py:26 | c | devtmpfs's own (drivers/base/devtmpfs.c: its root `mode=0755`, a device node no driver names 0600); `/init` executable by all (execve(2) EACCES), written by root |
| ctr box memory / oom | 8g / 1000 | bench/native/guest-tree/reference.py:35 | c | the operator's shared fedora limits, as above |
| `EXAMPLES` 5, `most_common(10)` | — | bench/native/tarball-census/census.py:13, :83 | input | Report sizes, not bounds: the census counts everything |
| `BENCH_MEMORY` / `BENCH_OOM` | 8g / 1000 | bench/oci-validation/run_hex.sh:5 | c | the operator's, as above |

## Shards' `crates/api` (hex/shards-api-constants e72c551b)

Every recorded bound is keyed by host class at compile time (`shards_api::record`). macOS-HVF uses its record. Linux-KVM and Windows-WHP get `record::Pending`, which names the record to take: `stop()` is refused by name there, and the clocked tests fail by name. PM M462/M463 are in Shards' docs/research/platform-measurements.md.


| name | value | file:line | kind | source or record |
|---|---|---|---|---|
| `TAIL` | 59 + 4096×6 + 1 = 24,636 | crates/api/src/client.rs:50 | c | The longest report shards writes as it ends: a reset's console page (`serial.rs:57`) escaped at most 6× (`vm_run.rs:2730-2737`) |
| `STOP_GRACE` | HVF 2 × 13 ms; KVM/WHP pending | lib.rs:155 | b | Twice (the margin) the slowest stop on record: Hex bench/native/guest/stop/RECORD.md; shards-api's own stop() in PM M463 peaked at 6.6 ms |
| `DRAIN_GRACE` | gone | — | fact | an end mark written after shards exits; the drain reads to it |
| `FIRST_BACKOFF` / `LAST_BACKOFF` | gone: pause √(2·c·w) | client.rs | a | c = the last refused dial's cost, w = the wait so far. It paces retries; the deadline is the caller's. Whether the lead's timeout rule covers a pacing rule is with the lead |
| `ANSWER` | 14 | client.rs:57 | c | Firecracker docs/vsock.md; Shards' conn.rs:372 |
| `SOCKET_PATH` | 103 | client.rs:180 | c | sun_path 104 (xnu bsd/sys/un.h) / 108 (unix(7)) |
| PrivateDir 64 tries | gone | client.rs:204 | c | The name now carries a per-process random value, so an existing name is refused, never retried |
| drain buffer | `TAIL` | client.rs:307 | c | no read returns more than the tail keeps |
| handshake 1 ms floor; stream `.max(1)` | gone | client.rs:594; stream.rs:119 | c | poll(2) takes ms, rounded up there |
| `min_memory_mib`, `max_vcpus`, `MAX_PORTS`, memory even | — | lib.rs:199, :207, :215, :236 | c / a / c / c | shards-vmm, cited at file:line |
| conformance payload | 64 KiB + 1 | conformance.rs:158 | input | one byte past the device's per-connection buffer (conn.rs:21) |
| conformance `take(4096)` | `MAX_PORTS + 1` | conformance.rs:215 | c | pigeonhole |
| unserved patience / slack | 200 ms / `OVERSHOOT` HVF 2 × 150.9 ms; KVM/WHP pending | conformance.rs:259 | input / b | PM M462: the slowest overshoot on record, ×2 the margin |
| sim tests | `PATIENCE` HVF 2 × 1.715 s, `REFUSED_AFTER_END` HVF 2 × 2.89 ms; 100 ms, 4096 inputs | tests/sim.rs:31, :158 | b / input | PM M463 (harness `the_waits_of_these_tests`); KVM/WHP pending |
| real test | `PATIENCE` HVF 2 × 207.6 ms; max(min, 256) / 1 vCPU | tests/real.rs:54, :39 | b / c | PM M463: the slowest wait a real HVF guest takes, its boot to an answering port; KVM/WHP pending |
| test ports 1234 / 1236 | — | sim.rs, real.rs | c | identifiers: the test guest's `ECHO_PORT` |
| unit-test specs | — | client.rs, lib.rs, stream.rs | input | stated in each test |

## The (d) rows left, and why

None of step 5's own. What remains is other steps':

1. **The relied-on (d)s** (`ANSWER_PATIENCE`, `MAX_LIST`, `serve::LIMITS`, `MAX_CONFIG_BYTES`, `MAX_BOXES`, the
   pre-hello limits) are other steps'. Step 5 now uses each one where it is correct, or no longer uses it.
