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

### Init's 21 ms, found (scratch builds, not committed; adas-mac-studio-2, launchd, 2026-11-06 21:13-21:15Z, load 13-34)

Finer stamps in a scratch build (each of init's three stamps moved; hex.ns5.dfine.20261106T211315Z and
dfine2.20261106T211351Z, N=20 and N=12):

- re-exec off the initramfs (copy and exec of the stripped `/init`): under 1 ms (init's first entry 25.1-27.1 ms;
  after the re-exec 25.0-26.6 ms of boottime, its first stamp being the first entry carried across).
- proc and sysfs: under 1 ms.
- **cgroup2's mount: about 20 ms** (stamp after sysfs 25.3-28.6 ms, after cgroup2 45.1-51.1 ms), each ending
  just past a whole millisecond: it waits out RCU grace periods, timer tick by timer tick.
- devtmpfs, /run, /tmp: under 0.05 ms together.

Tests on the kernel's command line (scratch, N=12 each): `cgroup_no_v1=all` leaves it at 20 ms
(hex.ns5.dfine3.20261106T211502Z); `rcupdate.rcu_expedited=1` brings it to 0.03-0.05 ms, the relay listening at
26.6-27.8 ms instead of 45-56 (hex.ns5.dfine4.20261106T211529Z). The kernel expedites RCU itself only until it starts
init (`rcu_end_inkernel_boot`), and cgroup2's mount comes after.

Cut 2 (HEX-GUEST-38): init turns `/sys/kernel/rcu_expedited` on for cgroup2's mount alone and off after it, as
the kernel does for its own boot. The guest's run otherwise keeps RCU as the kernel leaves it. Whether to leave it
on, trading an interrupt to every CPU per grace period for faster cgroup and namespace teardown in the boxes, is a
separate measured question. Its 40-boot record with the committed tree is pending: studio-2 stopped answering.

### Cut 2's record (the committed tree)

launchd hex.ns5.dsplit3.20261107T003310Z, 2026-11-07 00:43Z, adas-mac-studio-2, load 2.92-4.40 (C's run had ended),
Hex 7cbe9f5 (cut 1 + cut 2), initramfs 2,177,528 bytes (sha256 531813f58da81d2d…), N=40;
runs/hvf-studio2-rcu-20261107T003310Z.txt. Medians / maxima, ms:

| phase | after cut 1 | after cut 2 |
|---|---|---|
| total, host-side | 91.41 / 190.80 | 91.78 / 248.58 |
| (ii) kernel to init | 24.65 / 25.40 | 23.70 / 30.33 |
| (iii) init: re-exec, mounts, cgroup | 20.79 / 30.68 | **0.75 / 0.87** |
| (iii) node ready | 0.54 / 0.60 | 0.44 / 0.66 |
| (iii) relay listening | 0.12 / 0.18 | 0.11 / 0.13 |
| (i)+(iv) host | 44.55 / 140.76 | 66.42 / 222.85 |

Reading: init's phase fell from 20.8 ms to 0.75 ms; the relay now listens at about 25 ms of the guest's boot.
The host's total didn't move (p50 91.8 ms): the host-side share rose by about what the guest saved, so the
guest is now ready before the host's first successful CONNECT. The critical path is Shards' (the VMM's start,
and when its vsock muxer accepts a CONNECT), (i)+(iv), about 66 ms, which goes to Shards' lead. The
first-boot maxima (238-249 ms) are a series' first boots, as in every HVF record.

## WHP (Windows x86_64, jupiter)

Detached S4U task hex-d-whp-split-20261106T164636, 2026-11-06 16:48-17:07 PST, N=40 each, under HEAVY co-tenant
load: other agents' builds held the host (7 cargo and 2 cargo-clippy processes, kiln reporting 0-15 of 32 cores
idle); no other guest was booting. The same shards.exe as the WHP records (debug build, AWS-LC without assembly),
Shards' vmlinux-6.18.48-x86_64-hyperv. Images built there from each tree (x86_64-unknown-linux-musl, rust-lld; the
cpio by a Rust port of initramfs.py, jupiter having no Python): before = f581970 (hex with debug info, initramfs
14,018,216 bytes, sha256 71788884…), after = b3c994e (cut 1 + cut 2, initramfs 2,617,392 bytes, sha256 dc0f8f11…).
Raw log on jupiter: %USERPROFILE%\.cache\hex-t7\native-s5\d\whp-split-20261106T164636.log (its per-boot phases in
whp-split.tsv beside it).

Medians / maxima, ms:

| phase | before | after cuts 1 + 2 |
|---|---|---|
| total, host-side | 12,294 / 31,042 | 13,383 / 22,672 |
| (ii) kernel to init | 1,832 / 5,125 | 1,714 / 3,919 |
| (iii) init: re-exec, mounts, cgroup | **75.5 / 382** | **9.4 / 117** |
| (iii) node ready | 2.8 / 19.8 | 3.1 / 18.3 |
| (iii) relay listening | 0.8 / 3.9 | 1.1 / 22.5 |
| (i)+(iv) host | 10,625 / 25,848 | 11,970 / 19,696 |

Reading: cut 2 is larger on WHP (init 75 -> 9 ms: the cgroup2 mount's RCU wait is longer there). The totals, 10-20x
the morning's WHP record, are the host side's under this load: the guest is listening within ~2 s, and the host's
paced dial (sqrt(2cw), its pause growing with the wait) lands long after; readiness as an event removes that wait. A
re-take at the host's ordinary load, before and after, goes with that change.
