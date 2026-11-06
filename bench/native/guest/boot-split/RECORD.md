# A guest's boot, split by phase (native plan step 5; the lead's boot-split work)

Each boot is timed host-side (from `boot` to the relay's token on its control port, through shards_api as
GuestRuntime does) and split by the guest's own stamps (`STAMPS_PORT`, HEX-GUEST-37): `CLOCK_BOOTTIME`
nanoseconds at init's first entry (carried across its re-exec), its mounts done, its node ready and its relay
listening. CLOCK_BOOTTIME starts when the kernel starts keeping time, a little after its entry, so (ii) is
from there. (i) the host's spawn to the guest's first instruction and (iv) the relay listening to the host
holding the token share no clock with the guest, so they're given together: the host total less the relay's
stamp. (iv) is at most the dial's last pause (shards_api's sqrt(2cw), about 1-4 ms here).

Tool: crates/hex-cli/tests/guest_boot_record.rs (ignored), N=40 a run, one boot after another.

## HVF (macOS aarch64, adas-mac-studio-2, Mac16,9)

Medians / maxima, milliseconds:

| phase | before: hex with debuginfo, 14.4 MB initramfs | after: hex stripped, 2.2 MB initramfs |
|---|---|---|
| boot to the relay's token, host-side | 152.30 / 277.22 | 91.41 / 190.80 |
| (ii) kernel timekeeping start to init's first entry | 28.35 / 33.27 | 24.65 / 25.40 |
| (iii) init: re-exec off the initramfs, mounts, cgroup | 20.97 / 25.54 | 20.79 / 30.68 |
| (iii) init: the node agent ready | 0.56 / 0.80 | 0.54 / 0.60 |
| (iii) init: the relay listening | 0.14 / 0.32 | 0.12 / 0.18 |
| (i)+(iv) host: spawn, VMM and vCPU, the dial | 104.62 / 232.04 | 44.55 / 140.76 |

- before: launchd hex.ns5.dsplit.20261106T210120Z, 21:07Z, load 9.28-9.33, Hex f581970, initramfs 14,409,736 bytes
  (sha256 34702fe8f6fa114e…); runs/hvf-studio2-20261106T210120Z.txt
- after: launchd hex.ns5.dsplit2.20261106T211043Z, 21:11Z, load 14.51-14.71, Hex f581970 with image.sh building
  `/init` without debug info or symbols, initramfs 2,176,856 bytes (sha256 b5558124454b7f75…);
  runs/hvf-studio2-stripped-20261106T211043Z.txt
- both: Shards a0cced87 debug build, Shards' Image-6.18.48-aarch64, the host's vCPUs, 632 MiB; each under the
  host's own co-tenants (C's timed run had ended), nothing added.

Reading: the strip cut the median boot by 61 ms (40%), most of it on the host side (Shards loading 12 MB less into
the guest) and 3.7 ms of the kernel's unpack. Init's own 21 ms, re-exec to mounts, didn't move: the copy and re-exec
of `/init` aren't what it spends. That is the next look. The node and relay take under 1 ms.

## WHP (Windows x86_64, jupiter)

Pending: jupiter's runner wasn't answering when this was taken.
