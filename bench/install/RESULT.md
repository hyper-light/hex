# The installer, per OS: the labs (native plan step 6)

The step's bar: "On a machine fresh of Hex, on each OS (the Mac Studio, jupiter, fedora), the installer's one
admin step leaves every class working with no further step. Uninstall leaves nothing of Hex's behind, checked by
listing what install made." The decision is docs/design/runtime.md, "the installer, per OS".

## Linux: met on a fresh Fedora (fedora)

`bench/install/linux_lab.sh` boots a machine fresh of Hex: Fedora 44's image with the host's `/usr` (systemd 259,
kernel 6.19.10) as a privileged container with a cgroup namespace of its own and systemd its init, Fedora
desktop's uresourced drop-in masked so `user@.service` delegates systemd's own `pids memory cpu`, as a server
edition's does. Its user's manager runs before install (lingering), as a CI runner's does.

Record `records/linux-fb2e5ca-20261106T054108Z.txt`: commit fb2e5ca, 2026-11-06T05:41Z, unit
`hex-native-s6-final-fb2e5ca-20261106T054108Z` on fedora (i9-14900HX), load average 12.52, 8 co-tenant
containers, the lab's container capped at 8 GiB with OOM score 1000. The same unit, tree and target dir passed
the 7 gates first (249 tests).

| Check | Result |
|---|---|
| fresh: `linux.sh check` lists nothing | nothing of Hex's is left |
| HEX-INST-1: before install, a node in a delegated unit of the user's | refused by name: "the io controller isn't delegated to … (it has cpu memory pids); Hex's installer sets the delegation up" |
| the plan (`install --dry-run`) | 13 lines |
| install, the one admin step, as root | `hex-node@hexlab.service` serves `/run/hex/hexlab/node.sock`; all 13 lines present |
| HEX-INST-7: the user's running manager across install | main pid 463 before, 463 after: not restarted |
| HEX-INST-5: the installed node's soft descriptor limit | 1,048,576 >= 594,304 |
| HEX-INST-3: the conformance suite over the installed node, as the user | oci and process boxes: passed |
| HEX-INST-6: a box whose spec names no `RLIMIT_NOFILE` | oci and process boxes each see 1024:524288 |
| HEX-INST-4: uninstall, then `check` of the plan | nothing of Hex's is left |

Red first:
- HEX-INST-5 failed at e9d8842 (`records/linux-e9d8842-20261106T015853Z.txt`), the user service at systemd's
  limits: soft 524,288 < 594,304.
- It failed the same way at aeef3c6 (unit `hex-native-s6-lab-aeef3c6-20261106T015923Z`, fedora's
  `~/.cache/hex-t7/native-s6/run-aeef3c6-20261106T015923Z/`), with the user unit's `LimitNOFILE=1048576` and a
  `user@.service` drop-in raising its hard limit: a running user manager, only re-executed, keeps its old hard
  limit (docs/research/runtime/nofile-limit.md §1.3). Hence the system unit.
- HEX-INST-6 failed at 27a3b90 (unit `hex-native-s6-inst6-27a3b90-20261106T020234Z`): the socket test's oci box
  didn't see the default before aeef3c6.

Fedora's root had run out of inotify instances (`fs.inotify.max_user_instances` 128, nearly all a kind cluster's),
so no lab container could boot systemd. It was raised to 8192 on the lead's order (2026-11-06, on Ada's word);
the command and both values head each record. A reboot restores 128.

Measured earlier, on the user-service route: with `daemon-reload` alone, the already-running user manager
didn't hand `io` on (unit `hex-native-s6-lab-20261104T085256Z`), as systemd's source predicts
(docs/research/runtime/installer-per-os.md §1.4).

## Windows: built; the run of record waits on native plan steps 4 and 5

`bench/install/windows_lab.ps1` runs as a standard user around an administrator's `windows.ps1 install` and
`uninstall` (one UAC prompt and the reboot Windows asks for, each): `before`, `installed`, `uninstalled`; its
`asks` phase (also part of `before`) counts the hex runs of the dry run through a counting `-Binary`. HEX-INST-9 on
jupiter, 2026-11-06 (`records/windows-asks-jupiter-20261106T195224Z.txt`): 3ba6028's `windows.ps1` ran hex twice
for one plan (red, exit 1), 47c94e3's once (green, exit 0). The
Windows `oci` class itself is native plan steps 4 and 5, not built, so after install it is refused for that
alone. Windows' part is open until steps 4 and 5 land: the step's "every class working" isn't narrowed, so the
lab runs again then.

## macOS: waits on native plan step 2 (the box users' pool and the privileged helper)
