# A guest's boot to ready, per host class (native plan step 5; HEX-GUEST-33's learned first-dial bound)

Boot to ready: from starting `shards run vm` to the relay's control port answering `CONNECT` with `OK` and the
connection's token, as scripts/guest/boot_check.py measures it (init's `hex guest ready` on the console, then
one handshake). The first dial's bound before any boot is seen is twice this record's slowest for the host
class (the lead's rule for learned waits, 2026-11-06).

## HVF (macOS aarch64)

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

Runs: 0.170 0.162 0.170 0.174 0.172 0.161 0.167 0.169 0.161 0.169 0.156 0.135 0.156 0.143 0.167 0.169 0.171 0.162
0.177 0.170 0.164 0.146 0.171 0.174 0.166 0.172 0.171 0.160 0.153 0.154 0.174 0.161 0.175 0.146 0.174 0.161 0.165
0.171 0.167 0.151.

## KVM (Linux x86_64), WHP (Windows x86_64)

Not yet recorded: fedora and jupiter were unreachable for this pass. Shards' own fedora/KVM boot record
(p99 114-194 ms) times Shards' boot, not a Hex relay's first `OK`, so it isn't this measure. Until each is
recorded, those classes take HVF's record, stated where it is used.
