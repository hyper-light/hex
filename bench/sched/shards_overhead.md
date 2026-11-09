# Shards' cost per microVM under KVM: memory per idle guest and boot-to-ready

The recorded workload charges each request its kind's runtime memory beside its own
(`huawei_workload.md`). Until 2026-11-06 Shards' was a 5 MiB stand-in, waiting on Shards' lead, who
hadn't answered. Here it is measured instead, as the user directed: on fedora under KVM, with the build
native step 5 runs on HVF and WHP. **Result: a Shards microVM's runtime costs its host 2.26 MiB beside a
128 MiB guest, rising with the guest's size and vCPUs to 4.77 MiB beside an 8,192 MiB one. Every figure is
under the 5 MiB stand-in. A guest of the trace's sizes is ready 99-169 ms after its process starts
(p50), and within 114-194 ms (p99); the 3,584 MiB guests were re-timed after a load spike (below).**

## What was measured

- **The build.** Native step 5's, fetched and built pinned by SHA-256 by `bench/sched/shards_guest.sh`,
  which is step 5's `m2.sh` made for x86_64 Linux:
  - Shards at `hex/shards-api` `a0cced87`, on `p/internal-local` `b8270370`, a debug build as step 5 ran:
    `shards` `599ea59e…`.
  - Shards' released kernel 6.18.48 for x86_64, `vmlinux-6.18.48-x86_64` `e23c31e8…`, pinned by Shards'
    `crates/shards/src/kernel.rs` (the aarch64 kernel step 5 booted on HVF is `87fbe8c0…`, of the same
    release), and its config `d14e3666…`, both checked against the release's `SHA256SUMS`.
  - Hex's guest image from `cp/t7-native-s5` `bd7c6ee`, by its `scripts/guest/image.sh`, Rust 1.99.1 and
    rust-lld: `hex` `a886421f…`, `initramfs-x86_64.cpio` `7849b599…`. zig 0.16.0, `70e49664…`, from its
    index.
- **Each boot.** As Hex's `scripts/guest/boot_check.py` boots: `shards run vm --kernel … --initrd …
  --vsock … --cpus C --memory M --cmdline "console=ttyS0 panic=-1" --no-console`. M is a pool's memory
  and C its CPU in whole vCPUs, for each of the trace's 14 pools. A guest is one process, `shards-vm`.
- **Boot-to-ready.** From starting the process to READY: boot_check's CONNECT to the relay's control port,
  answered OK, then the connection's token, tried every millisecond. Three runs of 100 boots per size, one
  after another, each stopped before the next; p50 and p99 by nearest rank, the best run by p99.
- **Memory per idle guest.** Three runs of five guests per size, each booted to READY, then left idle 3 s.
  - Each guest's process's smaps. Its RAM is the anonymous mappings whose sizes sum to exactly M: on KVM
    x86_64, the main region (split at 3 GiB, the rest above 4 GiB), a 24 MiB one (the kernel, wholly
    resident) and a 16 MiB one. The rest is the VMM's own, its thread stacks among it.
  - The unit's cgroup `memory.stat` before the five and with them idle, divided by five: the kernel's
    memory per guest (`kernel`: KVM's page tables, `sec_pagetables`, the VMM's own page tables, slab and
    stacks).
  - Host memory per idle guest is the guest's PSS: its committed RAM plus the VMM's own.
- **Host.** fedora, `kiln-a094a12d.attlocal.net`, Intel Core i9-14900HX (24 cores, 32 threads), 62.5 GiB,
  Linux 6.19.10-300.fc44. CPUs 4-15 (`AllowedCPUs=4-15`, affinity checked: `4-15`), under
  `flock /home/adalundhe/.cache/hex-t7/fedora-timed.lock`.
- **The run.** Unit hex-sched-shardsmeasure-20261106T131320Z, 2026-11-06, 13:15:21Z to 13:42:04Z, its
  script `bench/sched/shards_overhead.py` `55f3b888…`. Load: noisy, the load average 12.66 before and 15.37
  after, 40.87 over the five minutes before the end. Seven docker containers were up (others' labs and
  builders). Units hex-sched-g4-gates-20261106T130902Z, hex-sched-g4-gatesm-20261106T130902Zm and
  hex-sched-g5-main-final-20261106T130731Z ran beside it. Nothing was done to quiet the host.

## Results

| guest | vCPUs | boot-to-ready p50 / p99, ms | host memory per idle guest, MiB | of it, committed guest RAM | VMM's own PSS, KiB | the kernel's per guest, KiB | overhead, MiB |
|---:|---:|---|---:|---:|---:|---:|---:|
| 128 MiB | 1 | 104.0 / 115.0 | 76.2 | 74.8 | 1,487 | 826 | 2.26 |
| 256 MiB | 1 | 103.2 / 119.4 | 80.4 | 78.9 | 1,483 | 864 | 2.29 |
| 512 MiB | 1 | 104.4 / 138.6 | 86.5 | 85.1 | 1,487 | 880 | 2.31 |
| 768 MiB | 1 | 105.6 / 119.3 | 90.4 | 89.0 | 1,487 | 901 | 2.33 |
| 1,024 MiB | 1 | 106.2 / 120.5 | 95.6 | 94.2 | 1,487 | 908 | 2.34 |
| 1,280 MiB | 2 | 107.0 / 126.0 | 102.8 | 101.3 | 1,531 | 1,142 | 2.61 |
| 1,536 MiB | 2 | 108.0 / 141.1 | 115.1 | 113.6 | 1,527 | 1,172 | 2.64 |
| 1,792 MiB | 2 | 110.1 / 130.7 | 111.3 | 109.8 | 1,531 | 1,146 | 2.61 |
| 2,048 MiB | 2 | 111.0 / 129.9 | 123.0 | 121.5 | 1,527 | 1,222 | 2.68 |
| 2,560 MiB | 3 | 106.5 / 138.0 | 129.6 | 128.1 | 1,575 | 1,522 | 3.02 |
| 3,072 MiB | 3 | 99.1 / 114.2 | 144.3 | 142.8 | 1,575 | 1,483 | 2.99 |
| 3,584 MiB | 3 | 154.6 / 194.0 (re-timed) | 220.6 | 219.0 | 1,575 | 1,871 | 3.37 |
| 4,096 MiB | 4 | 159.0 / 189.9 | 231.3 | 229.7 | 1,619 | 2,100 | 3.63 |
| 8,192 MiB | 7 | 168.5 / 190.3 | 309.6 | 307.9 | 1,739 | 3,150 | 4.77 |

Host memory and committed RAM are medians over 15 guests. The VMM's own PSS is the largest of the 15, and
the kernel's per guest the largest of the three runs. No boot failed.

The 3,584 MiB guests' first timing ran through a load spike: others' CI jobs took the five-minute load
average to 40.87 as the run ended. Their best run read p50 1,411.5 ms and p99 6,088.2 ms, against 99.1 and
114.2 ms for 3,072 MiB just before. The first 4,096 MiB run after them still had a 2,437 ms maximum. So they
were re-timed, with 3,072 and 4,096 MiB as controls in the same run: unit
hex-sched-shardsretime-20261106T134245Z, 13:45:45Z to 13:48:44Z, load average 13.38 at the start and
10.14 at the end. Best of 3 by p99, p50 and p99 in ms:
- 3,584 MiB: 154.6 / 194.0;
- 3,072 MiB: 128.1 / 179.9;
- 4,096 MiB: 154.9 / 175.0.

The table keeps the first run's figures for the controls, and the re-timed ones for 3,584 MiB.

## The figure the trace uses

**Overhead = the VMM's own PSS plus the kernel's memory per guest**, the last column, one `overhead`
row per guest size (`huawei_trace.py`, `SHARDS_OVERHEAD`).
- **Why not the whole host memory per guest.** A request's memory is its guest's whole RAM, reserved as
  a limit. The RAM an idle guest has committed, 75 to 308 MiB here, is within it, so adding it again would
  count it twice.
- **Why it grows with size.** The kernel's share grows with the RAM a guest has touched and its vCPUs:
  KVM's and the VMM's page tables, and slab. The VMM's own PSS grows a little with vCPUs, from 1.45 MiB at
  one to 1.70 MiB at seven.
- **PSS shares what five guests share.** Most of the VMM's PSS is its binary's text, split five ways here.
  A guest alone holds 5.56 MiB of the VMM's own (a probe of 128, 2,048 and 4,096 MiB guests, unit
  hex-sched-shardsprobe-20261106T131122Z). A node with many guests approaches the VMM's private memory,
  0.42-0.66 MiB.

## What it doesn't show

- **Idle guests.** A busy guest commits more of its RAM, within its request, and makes KVM build more page
  tables: up to about 1/512 of the RAM it touches, in 4 KiB pages.
- **A debug build**, as step 5 ran. A release build's text is smaller; its private memory is much the
  same.
- **No network, shares or disks.** A guest with a network has Shards' network process beside it, and one
  with shares its share process: more memory than measured here.
- **One host and kernel.** HVF and WHP aren't measured here.
- **No answer from Shards' lead yet** (asked 2026-11-06 09:18Z). If it answers, its figure goes beside
  these.
