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

This tree (`cp/t7-native-windows`) carries native step 4's `oci` class (merged from `cp/t7-native-s4` 0862d0e)
beside step 6's feature check, so the lab checks every class there is. An `oci` box is routed by its image: a
Windows image needs Containers and goes to step 4's HCS class; a Linux image (a bundle with no `windows` section)
needs HypervisorPlatform and step 5's Shards guest. While what an image needs is off, it is refused naming the
feature and the installer (`a_windows_image_is_refused_naming_containers_while_it_is_off`,
`a_linux_image_is_refused_naming_what_its_guest_waits_on`, HEX-INST-2).

`bench/install/jupiter.sh` runs it all from kiln on jupiter (Git Bash), a record per phase in `records/`:

| Phase | Who | What it checks |
|---|---|---|
| `jupiter.sh build`, `jupiter.sh gates` | kiln | hex.exe and the test binaries build; fmt, clippy and the workspace's tests pass natively |
| `bench/native/windows/oci_setup.ps1` | kiln, on Ada's yes (step 4's setup) | the image's layers for the oci suite (`layers.txt`) |
| `jupiter.sh before` | kiln | fresh of Hex; the plan saved; each feature the plan turns on reads off, and its image is refused naming it |
| `windows.ps1 install -Binary <hex.exe>` | Ada at jupiter: one UAC prompt, then the reboot | the one admin step |
| `jupiter.sh installed` | kiln | every plan line present; no image refused for a feature; the process class's contract; the oci class's contract and step 4's criteria on the image; a Linux image refused only for the Shards guest |
| `windows.ps1 uninstall` | Ada at jupiter: one UAC prompt, then the reboot | undoing it |
| `jupiter.sh uninstalled` | kiln | none of the plan's lines left |

Windows' part is open until step 5 lands too: the step's "every class working" isn't narrowed, so with step 5's
guest the `installed` phase runs the oci suite in the guest in place of the Linux image's refusal.

## macOS: waits on native plan step 2 (the box users' pool and the privileged helper)
