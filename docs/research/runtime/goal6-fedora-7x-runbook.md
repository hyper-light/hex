# Fedora to Linux 7.2: Ada's runbook (goal 6, 2026-11-06)

Goal 6's benchmarks of record need a bench host on Linux 7.1 or later (`goal6-mntns.md` §7, §14). Fedora is that host
(i9-14900HX, CPUs 4-15 for timed parts). A reboot restarts fedora's kiln runner, so the move is Ada's own act. Every
fact below was read on fedora on 2026-11-06 at about 01:55Z, without changing anything.

## Fedora today

- Fedora 44 on `6.19.10-300.fc44.x86_64`; UEFI, GRUB with BLS entries, `/boot` on its own partition (2.0G, 1.4G free).
- `/etc/sysconfig/kernel`: `UPDATEDEFAULT=yes`, `DEFAULTKERNEL=kernel-core`. A new kernel becomes GRUB's default when
  installed, and the old one stays installed (installonly).
- No out-of-tree modules are loaded: no nvidia, zfs or vbox, and no dkms or akmods. Nothing needs rebuilding for a new
  kernel.
- `/` (and `/home`) is at 100%, with about 21G free. The kernel needs about 300M in `/var/cache` and
  `/usr/lib/modules`, and about 70M in `/boot`.
- The kiln runner is the user unit `kiln.service`, enabled, with lingering on for `adalundhe`, so it starts at boot
  with no login. Docker is enabled (29.8.2).

## The package

`kernel-7.2.15-200.fc44` from Fedora's own `updates` repository, signed by Fedora's key: `kernel`, `kernel-core`,
`kernel-modules`, `kernel-modules-core`, all at 7.2.15-200.fc44, all available there. It's the same build the 7.2 guest
lab boots, where goal 6's tests and measurements ran (`goal6-mntns.md` §9-§15). `kernel-vanilla` isn't needed.

## Steps

1. **Pick a window, and drain.** Tell the agents with work on fedora (the lead passes it on). Wait until no `hex-*`
   unit and no kiln CI job is running:
   ```
   systemctl --user list-units 'hex-*' 'kiln-*' --state=active --no-legend
   ```
2. **Check space** (at least 2G free on `/`, 200M on `/boot`):
   ```
   df -h / /boot
   ```
3. **Install** the kernel. 6.19.10 stays installed beside it:
   ```
   sudo dnf install kernel-7.2.15-200.fc44
   ```
4. **Keep 6.19 the default, and boot 7.2 once.** If 7.2 doesn't come up, the next boot (a power cycle) is 6.19 again:
   ```
   sudo grubby --set-default /boot/vmlinuz-6.19.10-300.fc44.x86_64
   ID=$(sudo grubby --info=/boot/vmlinuz-7.2.15-200.fc44.x86_64 | sed -n 's/^id="\(.*\)"$/\1/p'); echo "$ID"
   sudo grub2-reboot "$ID"             # BLS entries are kept by id; an index can shift
   sudo grubby --default-kernel        # should still print the 6.19.10 kernel
   sudo systemctl reboot
   ```
5. **Check it came up**, as `adalundhe`:
   ```
   uname -r                                   # 7.2.15-200.fc44.x86_64
   systemctl --user is-active kiln.service    # active (fedora back as a kiln runner)
   systemctl is-active docker                 # active
   ```
   - In kiln, `kiln-a094a12d.attlocal.net` shows online.
   - The lead or the goal 6 agent then confirms Hex sees the new calls, from the goal 6 checkout:
     `bash bench/competitive/probes/mntns_record.sh probe` should print `OPEN_TREE_NAMESPACE supported` and
     `CLONE_EMPTY_MNTNS supported`.
6. **Make 7.2 the default**, once step 5 passes:
   ```
   sudo grubby --set-default /boot/vmlinuz-7.2.15-200.fc44.x86_64
   ```
   6.19.10 stays installed and in GRUB's menu.

## Rollback

- **If 7.2 doesn't boot at step 4:** power-cycle. The one-time entry is used up, so GRUB boots 6.19.10, still the
  default.
- **Back to 6.19 at any later point:**
  ```
  sudo grubby --set-default /boot/vmlinuz-6.19.10-300.fc44.x86_64
  sudo systemctl reboot
  ```
  Or, at boot, pick `6.19.10-300.fc44` in GRUB's menu (hold Shift or press Esc while it starts, to show the menu).
- The 6.19.10 kernel isn't removed by this. Fedora keeps three kernels by default (`installonly_limit`), so a later
  update can remove it; remove 7.2 instead with `sudo dnf remove kernel-core-7.2.15-200.fc44` while booted on 6.19.

## What else it affects

- **Every agent's fedora units end at the reboot.** Detached `hex-*` units, other projects' units and kiln CI jobs
  all stop. That's why step 1 drains first.
- **Kernel-dependent records.** 6.19 measurements stay 6.19's; goal 6 and goal 5's tail records name their kernel.
  After the move, the baselines a 7.2 comparison needs are re-measured on 7.2. Goal 6 runs its own with one command
  (`bash bench/competitive/probes/mntns_record.sh host`).
- **Known kernel fixes 7.2.y carries.** The CLONE_INTO_CGROUP kill fix (8e3599202166, tagged for stable) is in 7.2.y,
  so Hex's guard for it stays harmless there.
