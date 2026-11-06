# A guest's boot to ready, per host class (native plan step 5; HEX-GUEST-33's learned first-dial bound)

Boot to ready: from starting `shards run vm` to the relay's control port answering `CONNECT` with `OK` and the
connection's token, as scripts/guest/boot_check.py measures it (init's `hex guest ready` on the console, then
one handshake). The first dial's bound before any boot is seen is twice this record's slowest for the host
class, across every run below, at rest or loaded, on every host of the class (the lead's rule for learned
waits and their seed records, 2026-11-06). Each run is taken under its host's own co-tenants, with nothing
added, and its load is recorded.

## HVF (macOS aarch64)

The class's slowest: **0.265 s** (adas-mac-studio-2.local's second run, its first boot), so the seed is 2 × 0.265 = 0.530 s
(`hex_runtime::guest::RECORDED_BOOT`).

### adas-mac-studio.local-2

- machine: adas-mac-studio.local-2 (Mac15,14, macOS 26.4), launchd one-shot hex.ns5.dboot.20261106T165414Z,
  2026-11-06 16:54Z, load average 9.7-10.3 (noisy, as run)
- tree: Hex cp/t7-native-s5-consts 749823c, initramfs sha256 0200a7741035f631…; Shards a0cced87 debug build
  (77ab4bf4916a7580…); 2 vCPUs, 632 MiB
- N = 40, one boot after another

| | seconds |
|---|---|
| min | 0.135 |
| p50 | 0.167 |
| max (the record's slowest) | 0.177 |

Runs, in order: 0.170 0.162 0.170 0.174 0.172 0.161 0.167 0.169 0.161 0.169 0.156 0.135 0.156 0.143 0.167 0.169 0.171 0.162
0.177 0.170 0.164 0.146 0.171 0.174 0.166 0.172 0.171 0.160 0.153 0.154 0.174 0.161 0.175 0.146 0.174 0.161 0.165
0.171 0.167 0.151.

### adas-mac-studio-2.local

- machine: Adas-Mac-Studio-2 (Mac16,9, M4 Max, macOS 26.2), launchd one-shot hex.ns5.dboot.20261106T172315Z,
  2026-11-06 17:23Z, under its own co-tenants (load average sampled before each boot: 8.43 for boots 1-12,
  13.20 for 13-36, 12.46 for 37-40)
- tree: Hex cp/t7-native-s5-consts 51730eb, guest image built there by scripts/guest/image.sh, initramfs
  sha256 46b8996cc040ad59…; Shards hex/shards-api debug build at ~/.cache/hex-t7/native-s5/a; that host's CPUs,
  632 MiB
- N = 40, one boot after another

| | seconds |
|---|---|
| min | 0.109 |
| p50 | 0.124 |
| max | 0.214 (boot 1, the first after the image was built) |

Runs, in order: 0.214 0.124 0.109 0.117 0.114 0.119 0.132 0.120 0.135 0.130 0.115 0.117 0.124 0.125 0.133 0.128
0.136 0.127 0.123 0.128 0.126 0.133 0.131 0.134 0.131 0.134 0.129 0.121 0.124 0.123 0.120 0.121 0.112 0.131
0.122 0.110 0.123 0.121 0.118 0.117.

### adas-mac-studio-2.local, second run

- launchd one-shot hex.ns5.dstop.20261106T175847Z (the stop record's run, bench/native/guest/stop), 2026-11-06
  17:58Z, load 6.40-7.89 under its own co-tenants; tree fffbc27, the same image and Shards build
- N = 40: min 0.111, p50 0.126, max 0.265 s (boot 1, again a series' first)

Runs, in order: 0.265 0.165 0.177 0.129 0.126 0.114 0.123 0.125 0.128 0.134 0.124 0.135 0.119 0.122 0.120 0.123 0.127 0.114 0.119 0.124 0.123 0.127 0.130 0.127 0.126 0.137 0.111 0.125 0.129 0.119 0.129 0.120 0.126 0.115 0.128 0.130 0.116 0.126 0.129 0.130.

## WHP (Windows x86_64)

The class's slowest: **2.713 s** (boot 17), so the seed is 2 × 2.713 s.

- machine: jupiter (Windows x86_64, WHP), detached scheduled task hex-d-whp-record-20261106T111613 (S4U),
  2026-11-06 11:16-11:17 local, under its own co-tenants (agent C's HEX-GUEST-21 work on the same host; kiln
  reported 16-32 of 32 cores idle around the run), nothing added
- tool: crates/hex-cli/tests/guest_boot_record.rs at Hex 0353a37, release, which boots through shards_api as
  GuestRuntime does: boot to the relay's token on its control port, then `stop` to the guest's end (on a class
  with no stop record, shards_api's stop kills at once, so this is the kill's time)
- image: Shards' vmlinux-6.18.48-x86_64-hyperv, initramfs-x86_64.cpio sha256 f40353b35ec7aeb… (hex c5aa681);
  shards.exe sha256 6bcdb44a2e0e08e… (the Windows agent's debug build); all of the host's vCPUs, 128 + 2 x 284 MiB
- N = 40; the raw lines: bench/native/guest/boot-ready/runs/whp-jupiter-20261106T111613.txt

| | seconds |
|---|---|
| min | 1.237 |
| p50 | 1.450 |
| max | 2.713 |

Runs, in order: 1.401 1.237 1.479 1.445 1.471 1.322 1.419 1.397 1.421 1.466 1.618 1.454 1.753 1.618 1.829 1.840 2.713 1.443 1.470 1.423 1.469 1.420 1.544 1.457 1.470 1.431 1.328 1.417 1.362 1.394 1.392 1.473 1.497 1.450 1.405 1.308 1.467 1.463 1.448 1.450.

## KVM (Linux x86_64)

Not yet recorded: fedora was unreachable for this pass. Shards' own fedora/KVM boot record (p99 114-194 ms)
times Shards' boot, not a Hex relay's first `OK`, so it isn't this measure. Until it is recorded, KVM has no
row in records.tsv and so no seed: a guest there is refused by name.
