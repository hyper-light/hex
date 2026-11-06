# A guest VM's stop: SIGTERM to the `shards run vm` process's end (native plan step 5, constants)

What bounds a stop that can't wait on a fact alone: scripts/guest/boot_check.py's `STOP_GRACE`, twice (the
margin) the slowest stop here, after which the VM is killed (the lead's rule for timeouts, 2026-11-06).

## HVF (macOS aarch64)

- machine: Adas-Mac-Studio-2 (Mac16,9, M4 Max), launchd one-shot hex.ns5.dstop.20261106T175847Z,
  2026-11-06 17:58Z, under its own co-tenants (load average 6.40 before boots 1-23, 7.89 for 24-40), nothing
  added
- tree: Hex cp/t7-native-s5-consts fffbc27 (boot_check.py printing the stop's time); its guest image
  (initramfs 46b8996cc040ad59…); Shards hex/shards-api debug build at ~/.cache/hex-t7/native-s5/a
- N = 40: each boot to READY, then SIGTERM. Every VM ended by that signal (exit -15).

| | seconds |
|---|---|
| min | 0.001 |
| p50 | 0.002 |
| max (the record's slowest) | 0.013 (stop 1) |

Runs, in order: 0.013 0.002 0.002 0.001 0.002 0.002 0.001 0.002 0.002 0.002 0.002 0.001 0.001 0.002 0.002 0.002 0.002 0.002 0.002 0.002 0.002 0.002 0.002 0.002 0.001 0.002 0.002 0.002 0.002 0.002 0.001 0.002 0.002 0.002 0.001 0.002 0.002 0.002 0.001 0.002.

KVM and WHP: not yet recorded (fedora and jupiter unreachable); they take this record until they are.
