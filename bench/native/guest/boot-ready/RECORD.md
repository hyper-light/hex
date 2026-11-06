# A guest's boot to ready, per host class (native plan step 5; HEX-GUEST-33's learned first-dial bound)

Boot to ready: from starting `shards run vm` to the relay's control port answering `CONNECT` with `OK` and the
connection's token, as scripts/guest/boot_check.py measures it (init's `hex guest ready` on the console, then
one handshake). The first dial's bound before any boot is seen is twice this record's slowest for the host
class, across every run below, at rest or loaded, on every host of the class (the lead's rule for learned
waits and their seed records, 2026-11-06). Each run is taken under its host's own co-tenants, with nothing
added, and its load is recorded.

## HVF (macOS aarch64)

The class's slowest: **0.214 s** (adas-mac-studio-2.local's first boot), so the seed is 2 × 0.214 = 0.428 s
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

## KVM (Linux x86_64), WHP (Windows x86_64)

Not yet recorded: fedora and jupiter were unreachable for this pass. Shards' own fedora/KVM boot record
(p99 114-194 ms) times Shards' boot, not a Hex relay's first `OK`, so it isn't this measure. Until each is
recorded, those classes take HVF's record, stated where it is used.
