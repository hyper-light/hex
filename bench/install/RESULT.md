# The installer, per OS: the labs (native plan step 6)

The step's bar: "On a machine fresh of Hex, on each OS (the Mac Studio, jupiter, fedora), the installer's one
admin step leaves every class working with no further step. Uninstall leaves nothing of Hex's behind, checked by
listing what install made." The decision is docs/design/runtime.md, "the installer, per OS".

## Linux: met on a fresh Fedora (fedora)

`bench/install/linux_lab.sh` boots a machine fresh of Hex: Fedora 44's image with the host's `/usr` (systemd 259,
kernel 6.19.10) as a privileged container with a cgroup namespace of its own and systemd its init, Fedora
desktop's uresourced drop-in masked so `user@.service` delegates systemd's own `pids memory cpu`, as a server
edition's does. Its user's manager runs before install (lingering), as a CI runner's does.

Record `records/linux-12b2e92-20261104T085736Z.txt`: commit 12b2e92, 2026-11-04T08:57Z, unit
`hex-native-s6-gates-20261104T085736Z` on fedora (i9-14900HX), load average 9.85, 104 co-tenant containers (docker ps, in the unit's log),
the lab's container capped at 8 GiB with OOM score 1000.

| Check | Result |
|---|---|
| fresh: `linux.sh check` lists nothing | nothing of Hex's is left |
| HEX-INST-1: before install, a node in a delegated unit of the user's | refused by name: "the io controller isn't delegated to … (it has cpu memory pids); Hex's installer sets the delegation up" |
| the plan (`install --dry-run`) | 13 lines |
| install, the one admin step, as root | the user's node serves its socket; `user@1000.service` delegates `cpu io memory pids`; all 13 lines present |
| HEX-INST-3: the conformance suite over the installed node, as the user | oci and process boxes: passed |
| HEX-INST-4: uninstall, then `check` of the plan | nothing of Hex's is left |

Measured on the way: with `daemon-reload` alone, the already-running user manager didn't hand `io` on (the
installed node was refused `io`, unit `hex-native-s6-lab-20261104T085256Z`), as systemd's source predicts
(docs/research/runtime/installer-per-os.md §1.4); install now re-executes each running user manager.

## Windows: built, not yet run (jupiter offline)

`bench/install/windows_lab.ps1` runs as a standard user around an administrator's `windows.ps1 install` and
`uninstall` (one UAC prompt and the reboot Windows asks for, each): `before`, `installed`, `uninstalled`. The
Windows `oci` class itself is native plan steps 4 and 5, not built, so after install it is refused for that
alone. Windows' part is open until steps 4 and 5 land: the step's "every class working" isn't narrowed, so the
lab runs again then.

## macOS: waits on native plan step 2 (the box users' pool and the privileged helper)
