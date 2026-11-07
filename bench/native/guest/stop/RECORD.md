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

## WHP (Windows x86_64)

- machine: jupiter (Windows x86_64, WHP), detached scheduled task hex-d-whp-record-20261106T111613 (S4U),
  2026-11-06 11:16-11:17 local, under its own co-tenants (agent C's HEX-GUEST-21 work on the same host; kiln
  reported 16-32 of 32 cores idle around the run), nothing added
- tool: crates/hex-cli/tests/guest_boot_record.rs at Hex 0353a37, release, which boots through shards_api as
  GuestRuntime does: boot to the relay's token on its control port, then `stop` to the guest's end (on a class
  with no stop record, shards_api's stop kills at once, so this is the kill's time)
- image: Shards' vmlinux-6.18.48-x86_64-hyperv, initramfs-x86_64.cpio sha256 f40353b35ec7aeb… (hex c5aa681);
  shards.exe sha256 6bcdb44a2e0e08e…: debug build, AWS-LC without assembly (AWS_LC_SYS_NO_ASM, the Windows agent's; Shards' lead, 2026-11-06: a boot doesn't run AWS-LC's crypto); all of the host's vCPUs, 128 + 2 x 284 MiB
- N = 40; the raw lines: bench/native/guest/boot-ready/runs/whp-jupiter-20261106T111613.txt

| | seconds |
|---|---|
| min | 0.0024 |
| p50 | 0.0031 |
| max (the class's slowest) | 0.0044 (stop 13) |

Runs, in order: 0.0035 0.0036 0.0032 0.0033 0.0029 0.0029 0.0034 0.0029 0.0030 0.0030 0.0037 0.0031 0.0044 0.0030 0.0034 0.0030 0.0033 0.0036 0.0033 0.0030 0.0032 0.0032 0.0027 0.0031 0.0031 0.0035 0.0035 0.0032 0.0031 0.0026 0.0031 0.0037 0.0029 0.0026 0.0024 0.0033 0.0031 0.0032 0.0030 0.0028.

KVM: not yet recorded (fedora unreachable); it has no row, so it has no seed.
