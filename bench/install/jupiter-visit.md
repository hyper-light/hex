# jupiter: step 4's oci setup and step 6's install, in one visit (native plan steps 4 and 6)

Ada's word (2026-11-06 01:45Z, through the lead): oci_setup.ps1 is approved, and goals 15-17 are to be finished.
This is every admin action left on jupiter, in order, with who does each, every UAC prompt and every reboot.

## What jupiter already is (read 2026-11-05/06 from kiln)

- Windows 11 (NT 10.0.26200.0), AMD Ryzen AI MAX+ 395, 32 logical CPUs; shared with Amphora.
- Containers and HypervisorPlatform are **on** already (Win32_OptionalFeature InstallState 1), so Hex's install
  turns on no feature and Windows asks for **no reboot**. The install plan is 4 lines: `folder C:\Program Files\Hex`,
  `file C:\Program Files\Hex\hex.exe`, `folder C:\ProgramData\Hex`, `file C:\ProgramData\Hex\manifest` (lab
  `before` phase, records windows-before-21cb432-20261105T084555Z).
- **kiln's shell on jupiter is already elevated**: `net session` succeeds and the token is in the Administrators
  role (checked 2026-11-06). So everything below can run from kiln with no UAC prompt at all (option A).
- Staged, nothing to build at the visit: `C:\Users\corph\.cache\hex-t7-windows\stage\b20f1d5\` holds `hex.exe`,
  `windows_process_box.exe`, `windows_oci_box.exe`, `oci_setup.ps1`, `oci_teardown.ps1`,
  `scripts\install\windows.ps1`, `bench\install\windows_lab.ps1` and `SHA256SUMS` (commit b20f1d5, with step 4's fixed scripts from 33d1f17).

## Option A (recommended): no visit

Ada says "yes, run them from kiln". Then, one after the other, from kiln (already elevated; no UAC prompt, no reboot):

| # | Who | What | Admin? |
|---|---|---|---|
| 1 | step 4 | `oci_setup.ps1` (containerd and Docker's static engine as plain processes on their own pipes, nanoserver ltsc2025 pulled, `layers.txt` written under `C:\Users\corph\.cache\hex-native-s4`); then its oci suite and the Docker/containerd comparison | yes: containerd and dockerd need it |
| 2 | Windows agent | `windows.ps1 install -Binary <stage>\hex.exe -NoRestart`, the installer's one admin step | yes |
| 3 | Windows agent | lab `installed`: every plan line present; no image refused for a feature; the process class's contract; step 4's oci suite natively on step 1's layers (a skip fails it); a Linux image refused only for the Shards guest | no |
| 4 | Windows agent | `windows.ps1 uninstall -NoRestart` | yes |
| 5 | Windows agent | lab `uninstalled`: none of the plan's lines left | no |
| 6 | step 4 | `oci_teardown.ps1` (stops its containerd and dockerd by their recorded PIDs) | no |

## Option B: Ada at jupiter, one UAC prompt

1. Open **Windows PowerShell as administrator**: Start, type `powershell`, choose *Run as administrator*.
   **UAC prompt 1 of 1**: "Do you want to allow this app to make changes to your device? Windows PowerShell"
   → **Yes**. Keep this window open to the end; nothing below asks again.
2. Step 4's setup (no further prompt; no reboot):
   ```
   cd C:\Users\corph\.cache\hex-t7-windows\stage\b20f1d5
   powershell -NoProfile -ExecutionPolicy Bypass -File .\oci_setup.ps1
   ```
   It prints `HEX_WINDOWS_LAYERS=...` at the end. Then tell the lead "setup done"; step 4 runs its suite from kiln.
3. Hex's install, once the lead says step 4 is done (no further prompt; **no reboot**, since no feature is
   turned on; if Windows did ask for one, `-NoRestart` prints so and nothing restarts):
   ```
   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install\windows.ps1 install -Binary .\hex.exe -NoRestart
   ```
   It prints `installed: C:\Program Files\Hex\hex.exe`. Tell the lead "installed"; the Windows agent runs the lab's
   `installed` phase from kiln.
4. Hex's uninstall, once the lead says the lab is done (no further prompt, no reboot):
   ```
   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install\windows.ps1 uninstall -NoRestart
   ```
   It ends `nothing of Hex's is left`. Tell the lead "uninstalled"; then the window can close. The agents run the
   lab's `uninstalled` phase and step 4's teardown from kiln.

## Checks before any of it

- `SHA256SUMS` in the stage folder matches its files (the Windows agent checks it before each step).
- The lab's `before` phase passed at 6be64c2 (record windows-before-6be64c2-20261106T015637Z, 2026-11-06T01:56Z): fresh of Hex, the same 4-line plan.
- Nothing of Hex's is on the machine (`windows.ps1 check` lists nothing).
