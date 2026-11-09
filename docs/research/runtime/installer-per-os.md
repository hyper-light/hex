# The installer: what each OS needs set up once, how an unprivileged node tells, how uninstall proves it gone

Source notes for the native plan's step 6 (one admin prompt per OS). Hex's installer asks for admin
rights once and sets up what each native class needs: on Linux, the cgroup v2 delegation the `process`
class's boxes live under; on Windows, the Containers and Windows Hypervisor Platform optional features;
on macOS, a pool of box users and a privileged helper run by launchd. A node on a machine missing any
of it refuses that class by name (CLAUDE.md §3), so each piece also needs a way for an **unprivileged**
process to tell it is there. Uninstall removes what install made, checked by listing.

Sources were read at fixed revisions, accessed 2026-11-04. Each quote is one line of its source, copied
exactly; `path:line` is at that revision.

| Source | Revision |
|---|---|
| systemd (`systemd/systemd`) | tag v258 = `781d9d0789379d1ea1f2ecefb804d41e9c8b6c38`; v251 `b622e95f2f59fcb58e23ddafed745eee26a0f52f`, v252 `e8dc52766e1fdb4f8c09c3ab654d1270e1090c8d`, v259 `9ca433482f2281d71718718705ca8cd3bf562ad6` |
| Linux (`torvalds/linux`) | `05441d697f7f9a927264f4058cbbc6a03dfcac9d` |
| Ubuntu 24.04 kernel (`~ubuntu-kernel/.../+git/noble`, git.launchpad.net) | tag Ubuntu-6.8.0-98.98 = `ab62bbff415262dc6e30d6f8c2ce9ef7c155530a` |
| Ubuntu 24.04 apparmor package (`ubuntu/+source/apparmor`) | applied/4.0.1really4.0.1-0ubuntu0.24.04.9 = `835ca4d0785bc87bfc44b46a6124644310a31bfe` |
| shadow-utils (`shadow-maint/shadow`) | `0cee5e6263aee59fda123f4204a0ad1adbcc8bbc` |
| `MicrosoftDocs/windows-powershell-docs` (DISM cmdlets, `docset/winserver2025-ps/Dism/`) | `b7645dd8c687139b28e845119da30ec670526507` |
| `MicrosoftDocs/PowerShell-Docs` (`reference/7.5/`) | `ebaa235784a688ec72ab642186c4c17460fb438b` |
| `MicrosoftDocs/Virtualization-Documentation` | `85e96f5075e548ca24bc252e9f5ac0f39c318ef8` |
| `MicrosoftDocs/win32` | `ca8274decead49403d70ceeed4af8d4ca74eaf11` |
| `MicrosoftDocs/WSL` | `9508d265738ebb07b7c5eb4efb1b841aef61d44d` |
| `MicrosoftDocs/windowsserverdocs` | `2205ce8e55001a9b5e8e0de43981baa8ad782562` |
| `dotnet/docs-maui` (Microsoft Learn's .NET MAUI docs) | `e4fe7f5b5fbb367c030227c6ef0db6a2c8afa21b` |
| `microsoft/Windows-Containers` (Microsoft's install scripts) | `c5593a4e9c06818b757cc965fc50f88dd1e0e0de` |
| `microsoft/hcsshim` | `b1c1f43a3a6fb308cf3b1ba45affa835a693695c` |
| `docker/docs` | `d40e4d2c029e471aa18744e28bebe179d96abb6f` |
| `NixOS/nix` | `e8962d01875f41b781ddd9edeac904fafd285b0b` |
| macOS 26.2 (25C56) man pages and SDK header, read on adas-mac-studio.local | `launchctl.1` sha256 `cf8752dd…42083ae2`; SDK `launchd.plist.5` `1c5f5041…6aaadcb`; `dscl.1` `374fc46e…0e74`; SDK `ServiceManagement.framework/Headers/SMAppService.h` |

URLs are `https://raw.githubusercontent.com/<owner>/<repo>/<revision>/<path>` for the GitHub repos and
`https://git.launchpad.net/<repo>/plain/<path>?id=<revision>` for Launchpad. Numbers are `DESIGN` (how a
thing works, from code or docs) unless marked `MEASURED-OWN` (run here, with the machine named).

## 1. Linux: cgroup v2 delegation under systemd

### 1.1 What the kernel requires of a delegated subtree

- Enabling a controller is per parent: `cgroup-v2.rst:467` "Only controllers which are listed in
  "cgroup.controllers" can be", and top-down (`:500-502`): "all non-root "cgroup.subtree_control" files"
  / "can only contain controllers which are enabled in the parent's".
- No internal processes (`:510-513`): "Non-root cgroups can distribute domain resources to their
  children" / "only when they don't have any processes of their own." So the node agent must not sit in
  the cgroup whose `cgroup.subtree_control` it writes: "the cgroup must create children and transfer all
  its processes to the" (`:532`) "children before enabling controllers in its "cgroup.subtree_control""
  (`:533`).
- Delegation to a user (`:543-545`): "A cgroup can be delegated in two ways. First, to a less privileged"
  / "user by granting write access of the directory and its "cgroup.procs"," / ""cgroup.threads" and
  "cgroup.subtree_control" files to the user." Containment (`:575-576`): "processes" / "can't be moved
  into or out of the sub-hierarchy by the delegatee."

### 1.2 `Delegate=` and `DelegateSubgroup=`

- `man/systemd.resource-control.xml:1394-1395` (v258): "Takes either a boolean argument or a (possibly
  empty) list of control group controller names." / "If true, delegation is turned on, and all supported
  controllers are enabled for the unit". Since v236 (`docs/CGROUP_DELEGATION.md:206`): "boolean.
  However, since v236 it optionally takes a list of controller names". It is only a request (`:208`):
  "specifically. Note that this only encodes a request."
- A list **adds** to what earlier assignments named: `src/core/load-fragment.c:4018`
  "c->delegate_controllers |= mask;" (v258), and the comment at `:3981`: "controllers to the mask to
  delegate." An empty assignment resets (`:3984`): "/* An empty string resets controllers and sets
  Delegate=yes. */". So a drop-in naming `cpu io memory pids` extends user@.service's own list.
- Delegation enables nothing below the unit (`CGROUP_DELEGATION.md:257-260`): "the requested controllers
  available to your service or scope, but won't actually" / "enable them. Currently you have to do that
  manually by writing to" / "`cgroup.subtree_control` within your delegated cgroup".
- A delegated unit gets a `user.delegate` xattr (`:226-227`, `:230-231`): "The service manager sets the
  `user.delegate` extended attribute (readable via" … "newer in combination with systemd 251 and newer."
- `DelegateSubgroup=` arrived in **v254**: `NEWS:4315` "* Service units gained a new setting
  DelegateSubgroup=" under "CHANGES WITH 254:" (`NEWS:4158`); `systemd.resource-control.xml:1446`
  `xpointer="v254"`; `CGROUP_DELEGATION.md:244-245` "Starting with systemd 254 you may" / "also use
  `DelegateSubgroup=` to let the service manager put your initial". It answers the no-internal-processes
  rule (`systemd.resource-control.xml:1442-1444`).

### 1.3 What user@.service delegates by default

- v258 `units/user@.service.in:28` "Delegate=pids memory cpu", `:29` "DelegateSubgroup=init.scope".
  Unchanged in v259 (`:28-29`).
- `cpu` was added in **v252**: v251 `units/user@.service.in:24` "Delegate=pids memory"; v252 `:24`
  "Delegate=pids memory cpu"; `NEWS:5677` "* The cpu controller is delegated to user manager units by
  default, and" (under "CHANGES WITH 252", `NEWS:5596`). `DelegateSubgroup=init.scope` first appears in
  v254's file.
- So **`io` is not delegated to a user's manager by default** in any release read (v244–v259); a rootless
  node wanting `io` (and `cpuset`) needs the installer's drop-in.

### 1.4 The drop-in, and whether a running user manager sees it

- Drop-ins for a template are read for every instance (`man/systemd.unit.xml:203-204`, `:209-210`):
  "Along with a unit file <filename>foo.service</filename>, a "drop-in" directory" … "<literal>.d/</literal>
  subdirectory (e.g. <literal>foo@bar.service.d/</literal>) and read its" / "…followed by the template
  <literal>.d/</literal> subdirectory". So `/etc/systemd/system/user@.service.d/hex-delegate.conf` with
  `[Service]` / `Delegate=cpu cpuset io memory pids` applies to every user. **Gap:** v258's
  `CGROUP_DELEGATION.md` does not itself document a `user@.service` drop-in (searched for "user@" and
  "drop-in": no hits); the mechanism rests on `systemd.unit(5)` above and the additive parse in §1.2.
- `daemon-reload` reloads unit files (`man/systemctl.xml:1483`: "reload all unit files, and recreate
  the entire dependency"), but two facts in v258's source say it is **not enough** for a running user
  manager to use a newly delegated controller:
  1. The user manager reads which controllers it may use once, at startup: `src/core/cgroup.c:3326`
     "r = cg_mask_supported_subtree(m->cgroup_root, &m->cgroup_supported);" inside
     `manager_setup_cgroup`, called from `manager_new` (`src/core/manager.c:874`, call at `:982`), not
     from `manager_reload` (`:3574`).
  2. An existing delegated cgroup keeps its enabled controllers on re-realization:
     `src/core/cgroup.c:2129` "/* For v2 we preserve enabled controllers in delegated units, adjust
     others, */", `:2130` "if (created || !unit_cgroup_delegate(u)) {".
  user@.service re-execs its manager on `reload` (`units/user@.service.in:23` "# Reexecute the manager on
  service reload, instead of reloading."; `:26` "ReloadSignal=RTMIN+25"), and a re-exec runs
  `manager_new` again. So the evidence predicts: `daemon-reload` makes the controller appear in
  `user@UID.service/cgroup.controllers` (the parent slice is not delegated, so it is adjusted), and
  `systemctl reload user@UID.service` (or a fresh login after the last session ends) makes the user
  manager use it. **This is inference from source; it must be measured** (falsifier: after
  `daemon-reload` alone, a user-manager unit with `Delegate=yes` sees `io` in its own
  `cgroup.controllers`; or after `reload user@UID.service` it still does not).

### 1.5 Linger

`man/loginctl.xml:189-193` (v258): "<listitem><para>Enable/disable user lingering for one or more" /
"users. If enabled for a specific user, a user manager is" / "spawned for the user at boot and kept
around after logouts." / "This allows users who are not logged in to run long-running" / "services."
`loginctl enable-linger` creates `/var/lib/systemd/linger/<user>` (`src/login/logind-dbus.c:1663`
"path = strjoina("/var/lib/systemd/linger/", escaped);", then `touch(path)` at `:1666`); logind exposes it
as the user object's `Linger` property (`man/org.freedesktop.login1.xml:1071` "readonly b Linger = ...;").
A rootless node that must outlive its user's sessions needs it; uninstall runs `disable-linger` only if
install turned it on (record the prior state).

### 1.6 Rootless prerequisites on common distros

- **subuid/subgid.** `man/subuid.5.xml:38-39`: "Subuid authorizes a user id to map ranges of user ids
  from its namespace" / "into child namespaces." `newuidmap` honors it even for root
  (`man/newuidmap.1.xml:74-75`: "Note that the root user is not exempted from the requirement for a
  valid" / "<filename>/etc/subuid</filename> entry."). `useradd` allocates a range by default when
  `/etc/subuid` exists (`man/login.defs.d/SUB_UID_COUNT.xml:21` "are respectively 100000, 600100000 and
  65536."); for an existing user `usermod` adds/removes one: `man/usermod.8.xml:430` "<option>-v</option>,
  <option>--add-subuids</option>&nbsp;<replaceable>FIRST</replaceable>-<replaceable>LAST</replaceable>",
  `:448` `-V, --del-subuids`, `:468` `-w, --add-subgids`. Only file-backed (`subuid.5.xml:54-56`: the
  tools "will only create entries in" `/etc/subuid` "if subid delegation is managed via subid" files).
  `getsubids(1)` lists a user's ranges (`man/getsubids.1.xml:46`). (The brief's `--add-subuids-for` is
  not a shadow-utils option; `useradd -F, --add-subids-for-system` is, `useradd.8.xml:294`.)
- **Ubuntu 24.04's AppArmor userns restriction.** The sysctl is Ubuntu's, not mainline's (mainline
  `security/apparmor/lsm.c` has `unprivileged_userns_apparmor_policy` and
  `apparmor_restrict_unprivileged_unconfined`, `:2454`/`:2469`, not this one). Ubuntu's kernel:
  `security/apparmor/lsm.c:2439` ".procname       = "apparmor_restrict_unprivileged_userns",", mode
  `0644` (`:2442`), so any user can read `/proc/sys/kernel/apparmor_restrict_unprivileged_userns`. The
  kernel default is off (`debian.master/config/annotations:618` `CONFIG_SECURITY_APPARMOR_RESTRICT_USERNS
  policy<{'amd64': 'n', 'arm64': 'n', …}>`); the apparmor package turns it on:
  `debian/usr/lib/sysctl.d/10-apparmor.conf:14` "kernel.apparmor_restrict_unprivileged_userns = 1", and
  `:3-6` "Allows to restrict the use of unprivileged user namespaces to applications" / "which have an
  AppArmor profile loaded which specifies the userns" / "permission." The installer should load an
  AppArmor profile for the hex binary granting `userns,` (the file's own route), not flip the sysctl
  (`:11-13` suggests an override file only "If it is desired to disable this restriction").
  **Gap:** the profile syntax was not read from a pinned AppArmor source here.
- **`user.max_user_namespaces`.** `Documentation/admin-guide/sysctl/user.rst:74-77`:
  "max_user_namespaces" / "The maximum number of user namespaces that any user in the current" / "user
  namespace may create." Readable at `/proc/sys/user/max_user_namespaces`; 0 refuses. The installer
  doesn't change it; the node refuses with the value read.

**What Hex does (Linux):** install writes `/etc/systemd/system/user@.service.d/hex-delegate.conf`
(`Delegate=cpu cpuset io memory pids`), runs `daemon-reload` and `reload user@UID.service`, enables
linger if asked, adds subuid/subgid ranges only if the user has none, and loads an AppArmor profile only
where `apparmor_restrict_unprivileged_userns` reads 1. The node unit runs with `Delegate=yes` and
`DelegateSubgroup=` (systemd ≥ 254) so the agent is never an internal process. Unprivileged detection:
read `/proc/self/cgroup`, the delegated cgroup's `cgroup.controllers` (each of cpu, io, memory, pids
present), the `user.delegate` xattr, write access to its `cgroup.subtree_control`; the two sysctls; and
`getsubids`. **Falsified if** a controller listed in the drop-in is missing from the node's
`cgroup.controllers` after install's reload step.

## 2. Windows: the Containers and Windows Hypervisor Platform features

### 2.1 Feature names

- **Containers:** Microsoft's own installer enables it by that name:
  `helpful_tools/Install-DockerCE/install-docker-ce.ps1:310` "Install-Feature -FeatureName Containers"
  (and `Install-ContainerdRuntime/install-containerd-runtime.ps1:282`). `:314-315` notes Hyper-V's two
  names: "Add-WindowsFeature (ServerManager) uses "Hyper-V"; Enable-WindowsOptionalFeature (DISM)" /
  "requires "Microsoft-Hyper-V"."
- **Windows Hypervisor Platform:** `docs/android/emulator/troubleshooting.md:153-154` (Microsoft Learn,
  .NET MAUI): "FeatureName      : HypervisorPlatform" / "DisplayName      : Windows Hypervisor Platform";
  `:165` "Enable-WindowsOptionalFeature -Online -FeatureName HypervisorPlatform -All". **Gap:** no
  Windows reference page (Virtualization-Documentation, windows-powershell-docs) names the feature; this
  how-to is the only Microsoft source found.

### 2.2 Enabling and disabling, one reboot

- `Enable-WindowsOptionalFeature.md:47` "The *All* parameter enables all parent features of the specified
  feature before enabling the specified feature."; `-NoRestart` (`:170`) "Suppresses reboot.";
  `Disable-WindowsOptionalFeature.md:20` "Disable-WindowsOptionalFeature -FeatureName <String[]>
  [-PackageName <String>] [-Remove] [-NoRestart] [-Online]". Both return
  `Microsoft.Dism.Commands.ImageObject` (`Enable-…:317`).
- The restart signal: Microsoft's script reads `RestartNeeded` from that object
  (`install-docker-ce.ps1:232` "$feature = Enable-WindowsOptionalFeature -Online -FeatureName
  $FeatureName -All -NoRestart", `:234` "if ($feature.RestartNeeded -eq "True")"). **Gap:** the cmdlet
  reference at this revision does not document the `RestartNeeded` property.
- DISM: `WSL/install-manual.md:49` "dism.exe /online /enable-feature /featurename:VirtualMachinePlatform
  /all /norestart"; `windowsserverdocs …/hyper-v/get-started/Install-Hyper-V.md:152` "DISM /Online
  /Enable-Feature /All /FeatureName:Microsoft-Hyper-V". The DISM option reference
  (learn.microsoft.com/windows-hardware/manufacture/desktop/dism-operating-system-package-servicing-command-line-options,
  no public repo) has `/Disable-Feature /FeatureName:<name_in_image> [/PackageName:<name_in_image>]
  [/Remove]`. Exit 3010 is `ERROR_SUCCESS_REBOOT_REQUIRED` (`desktop-src/Debug/system-error-codes--1700-3999-.md:2738`
  "3010 (0xBC2)", `:2743` "The requested operation is successful. Changes will not be effective until the
  system is rebooted."). **Gap:** no Microsoft page read states that `dism.exe` itself exits 3010; measure.
- Hex enables both features in one elevated call with `-NoRestart`, ORs their `RestartNeeded`, and asks
  for one reboot.

### 2.3 Asking for elevation once

`about_Requires.md:187-188`: "specifies that the PowerShell session in which you're running the script
must" / "be started with elevated user rights." It refuses an unelevated run; it does not elevate.
Elevation is asked with `Start-Process.md:108` "Start-Process -FilePath "powershell" -Verb RunAs" (the
UAC prompt). Microsoft's script checks the token itself (`install-docker-ce.ps1:522`
"if ($myWindowsPrincipal.IsInRole($adminRole))"). Hex: the unelevated `hex install` re-launches its
installer step once with `-Verb RunAs` (ShellExecute `runas`), the elevated step checks its token and
does every privileged action, so there is one prompt.

### 2.4 Unprivileged detection

- **WMI `Win32_OptionalFeature`.** `desktop-src/CIMWin32Prov/win32-optionalfeature.md:28` "Represents the
  status of the optional features that are present on the operating system."; `InstallState` "Enabled**
  (1)", "Disabled** (2)", "Absent** (3)", "Unknown** (4)" (`:126`, `:133`, `:140`, `:147`); namespace
  `Root\\CIMV2` (`:286`). UAC filtering (`WmiSdk/user-account-control-and-wmi.md:31`): "Some WMI
  operations, such as getting data or executing most methods, do not require that the account run as an
  administrator." `Win32_OptionalFeature` is not on that page's lists of classes that drop instances or
  properties under UAC filtering (`:83-108`). That is consistent with standard-user reads, **not proof:
  measure on jupiter as a standard user**. `Get-WindowsOptionalFeature`, by contrast, is shown run "as
  administrator" (`troubleshooting.md:124`); treat the DISM cmdlets as needing elevation unless measured.
- **Hypervisor platform.** `hypervisor-platform.md:18`: "`WinHvPlatform.dll` (located in the System32
  folder) exports a set of C-style Windows API functions which return HRESULT error codes."
  `funcs/WHvGetCapability.md:470`: "The `WHvCapabilityCodeHypervisorPresent` capability can be used to
  determine whether the Windows Hypervisor is running on a host and the functions of the platform APIs
  can be used to create VM partitions." Hex loads `WinHvPlatform.dll` (absent → feature off) and calls
  `WHvGetCapability(WHvCapabilityCodeHypervisorPresent)` (TRUE → usable now; FALSE after enable → reboot
  pending). **Measure:** that the DLL is absent with the feature off, and that the call needs no admin.
- **Containers / HCS.** hcsshim binds the HCS through `vmcompute.dll`
  (`internal/vmcompute/zsyscall_windows.go:36` "modvmcompute = windows.NewLazySystemDLL("vmcompute.dll")")
  and `computecore.dll` (`internal/computecore/zsyscall_windows.go:36`). Docker's engine "depends on a
  Windows-specific Host Compute Service" (`windowscontainers/troubleshooting.md:155`). **Gap:** no
  Microsoft doc read names the service `vmcompute` or states which feature installs it; measure
  (`sc query vmcompute` / `OpenService` as a standard user, and the DLL's presence, with Containers on
  and off).

### 2.5 What Docker Desktop does

`content/manuals/desktop/setup/install/windows-install.md:91` "- Turn on Hyper-V and Containers Windows
features." (Hyper-V backend prerequisites); `:147` "Choosing all users will prompt for elevation.";
`:293` `--always-run-service` starts `com.docker.service` so later actions avoid UAC. **Gap:** the doc
lists the features as a prerequisite; it does not say the installer enables them.

**What Hex does (Windows):** install (elevated once) enables `Containers` and `HypervisorPlatform` with
`-All -NoRestart`, records which were already on, and asks for one reboot if either reports
`RestartNeeded`. Uninstall disables only the features install turned on (its record), again
`-NoRestart`, one reboot. **Falsified if** a standard user cannot read `Win32_OptionalFeature`'s
`InstallState` (then detection falls back to the DLL/capability probes alone).

## 3. macOS: box users and a privileged helper

### 3.1 How Nix's multi-user installer makes build users

`packaging/scripted-installer/install-darwin-multi-user.sh` (the scripts moved from `scripts/` to
`packaging/scripted-installer/`):

- UIDs: `:8` "# - up to macOS Big Sur we used the same GID/UIDs as Linux (30000:30001-32)"; `:9-11` "we
  changed UID to 301 because Big Sur updates failed into recovery mode" / "we're targeting the 200-400 UID
  range for role users mentioned in the" / "usage note for sysadminctl"; `:12-13` "# - we changed UID to
  351 because Sequoia now uses UIDs 300-304 for its own" / "#   daemon users".
- `:16` `export NIX_FIRST_BUILD_UID="${NIX_FIRST_BUILD_UID:-351}"`, `:17`
  `export NIX_BUILD_GROUP_ID="${NIX_BUILD_GROUP_ID:-350}"`, `:18`
  `export NIX_BUILD_USER_NAME_TEMPLATE="_nixbld%d"`; count `install-multi-user.sh:25`
  `readonly NIX_USER_COUNT=${NIX_USER_COUNT:-32}`, group `:26` `readonly NIX_BUILD_GROUP_NAME="nixbld"`.
  So `_nixbld1`…`_nixbld32` are UIDs 351–382 in group `nixbld` (GID 350).
- Group: `install-darwin-multi-user.sh:134` "/usr/sbin/dseditgroup -o create \", `-i "$NIX_BUILD_GROUP_ID"`
  (`:136`). User: `:231` `/usr/bin/dscl . create "/Users/$username" \` with `UniqueID "${uid}"` (`:232`).
- Attributes, each checked then set (`install-multi-user.sh`): hidden (`:602`, darwin sets `IsHidden 1`,
  `install-darwin-multi-user.sh:169`); `:609` `poly_user_home_set "$username" "/var/empty"`
  (`NFSHomeDirectory`, darwin `:180`); `:620` note "Nix build user $coreid" (`RealName`, darwin `:189`);
  `:627` `poly_user_shell_set "$username" "/sbin/nologin"` (`UserShell`, darwin `:198`; the shell is
  `/sbin/nologin`, not `/usr/bin/false`); group membership via `dseditgroup -o edit -t user -a`
  (darwin `:212-213`); `:641` `PrimaryGroupID` = the group's GID (darwin `:222`). The install is
  idempotent: an existing user with a different UID stops it (`install-multi-user.sh:583-593`).
- `dscl` writes retry on one known error (`install-darwin-multi-user.sh:149` "# workaround a bug in dscl
  where it sometimes fails with eNotYetImplemented:").

**MEASURED-OWN** (adas-mac-studio.local, macOS 26.2 25C56, `dscl . -list /Users UniqueID` as uid 501):
Apple's role users now occupy UIDs 300–308 (`_aonsensed` 300 … `_mds_stores` 308), plus `_oahd` 441, and
groups 395–399 (`com.apple.access_*`). Nix's 351+ is clear on this release, but the range Apple takes
grows; Hex lists before creating and refuses (by name) a collision, never reuses a taken ID.

### 3.2 How Nix installs and removes its daemon

- Plist `/Library/LaunchDaemons/org.nixos.nix-daemon.plist` (`install-darwin-multi-user.sh:20`), installed
  `u=rw,go=r` (`:115`), started with `:118` "launchctl load /Library/LaunchDaemons/org.nixos.nix-daemon.plist"
  then `:121` "launchctl kickstart -k system/org.nixos.nix-daemon". Its keys
  (`misc/launchd/org.nixos.nix-daemon.plist.in:5-10`): `Label`, `KeepAlive` true, `RunAtLoad` true,
  `ProgramArguments` (`:11`).
- Uninstall (`doc/manual/source/installation/uninstall.md:106-107`): "sudo launchctl unload
  /Library/LaunchDaemons/org.nixos.nix-daemon.plist" / "sudo rm /Library/LaunchDaemons/org.nixos.nix-daemon.plist";
  `:117` "sudo dscl . -delete /Groups/nixbld"; `:118` "for u in $(sudo dscl . -list /Users | grep _nixbld);
  do sudo dscl . -delete /Users/$u; done".
- Docker Desktop's (≤ 4.88) helper is the same shape: `mac-permission-requirements.md:85` "$ sudo
  launchctl unload -w /Library/LaunchDaemons/com.docker.vmnetd.plist", `:91` "$ rm
  /Library/LaunchDaemons/com.docker.vmnetd.plist", `:93` "$ rm /Library/PrivilegedHelperTools/com.docker.vmnetd";
  it talks to it over "the Unix domain socket `/var/run/com.docker.vmnetd.sock`" (`:73`).

### 3.3 Apple's launchd and launchctl (macOS 26.2)

- `launchd.plist.5:124-125` "Label <string>" / "This required key uniquely identifies the job to";
  `:135-137` "UserName <string>" / "This optional key specifies the user to run the job as. This key is
  only" / "applicable for services that are loaded into the privileged system domain."; `:197`
  `ProgramArguments`, "required in the absence of the Program key" (`:201`); `:269-273` `KeepAlive`
  "The default is" / "false … The value may be set to true" / "to unconditionally keep the job alive.";
  `:332-336` `RunAtLoad` "The default is false. This key should be avoided, as" / "speculative job launches
  have an adverse effect on system-boot and user-login"; `:835-836` "/Library/LaunchDaemons" "System-wide
  daemons provided by the administrator."
- `launchctl.1:93-94` "Bootstraps or removes domains and services. When service arguments are present," /
  "bootstraps and correspondingly removes their definitions into the domain."; `:462-463` `load | unload`:
  "Recommended alternative subcommands:" / "bootstrap | bootout | enable | disable"; and `:527-528` "subcommands
  will only return a non-zero exit code due to improper usage." / "Otherwise, zero is always returned." So
  Nix's `launchctl load` can't fail visibly; **Hex uses `bootstrap system <plist>` and `bootout
  system/<label>`**, whose exit is meaningful (`:716-718` "will exit with status 0 if the subcommand
  succeeded. Otherwise, it will exit" / "with an error code").
- `launchctl.1:107-109` `disable` persists: "Once a service is" / "disabled, it cannot be loaded in the
  specified domain until it is once again" / "enabled. This state persists across boots".
- `print` (`:263`) "Prints information about the specified service or domain."; its output "is NOT API"
  (`:270-276`, "It may change from release to release without warning."). Use only its exit status.

**MEASURED-OWN** (same Mac, uid 501, unprivileged): `launchctl print system/com.openssh.sshd` exits 0;
`launchctl print system/org.example.none` exits 113, `launchctl error 113` = "Could not find specified
service". So an unprivileged node can tell whether the helper's label is loaded in the system domain from
the exit status alone.

### 3.4 SMAppService versus a plain plist

`SMAppService.h` (macOS 26.2 SDK): `:131` `daemonServiceWithPlistName:` `NS_SWIFT_NAME(daemon(plistName:))
API_AVAILABLE(macos(13.0)…)`; `:121` "The plistName must correspond to a plist in the calling app's
Contents/Library/LaunchDaemons directory"; `:160-161` "If the service corresponds to a LaunchDaemon, the
LaunchDaemon will not be bootstrapped until an" / "admin approves the LaunchDaemon in System Settings.";
`:168` "If the app bundle is not properly code signed, this API will return error kSMErrorInvalidSignature";
`:128-129` the bundle should "live in /Applications". Status values `:31-34` NotRegistered, Enabled,
RequiresApproval, NotFound. A plain `/Library/LaunchDaemons` plist is a "legacy" service whose status
can still be read (`:260` "Valid URLs have the prefix /Library/LaunchDaemons /Library/LaunchAgents or",
`:270` `statusForLegacyURL:`), and `:263-264` shows a user can disable legacy daemons ("still want to know
when a user disables its legacy daemons or agents").

So SMAppService needs a signed `.app` in /Applications and an admin approving it in System Settings (a
second step after install); a script installing a plist under /Library/LaunchDaemons with
`launchctl bootstrap` needs one `sudo` and no bundle, but the user can still turn it off in Login Items.
Hex ships the plain plist (one prompt, CLI-installable, as Nix) and detects a user's disable by the
`print` exit and by connecting to its socket. **Falsified if** macOS refuses to bootstrap an unsigned
or non-bundled `/Library/LaunchDaemons` plist (it doesn't on 26.2 for Nix-shaped installs; re-check per
release).

### 3.5 Unprivileged detection on macOS

`dscl . -list /Users UniqueID` and `-list /Groups PrimaryGroupID` run as a standard user (MEASURED-OWN
above, exit 0, 133 users); `dscl.1:38` "-list path [key]", `:49` "-delete path". The helper: `launchctl
print system/<label>` exit 0 vs 113, then a connect to its socket (the authoritative check that it
answers).

### 3.6 Who may reach the helper's socket

The helper runs as root, so its socket must answer only the users whose nodes may use it.

- A new file's group: `open.2:117` (macOS 26.2 25C56, adas-mac-studio-2) "When a new file is created, it is
  given the group of the directory which" (the BSD rule; a socket `bind(2)` makes is such a file). So a socket
  bound in a folder of group `hexnode` is `hexnode`'s, whatever the daemon's own group.
- Its mode: `launchd.plist.5:293-296` "Umask <integer or string>" / "This optional key specifies what value
  should be passed to umask(2)" / "before running the job. If the value specified is an integer, it must be" /
  "a decimal representation of the desired umask". `Umask` 7 makes the bound socket `0770`.
- `/Library/PrivilegedHelperTools` is `root:wheel 0755` on the same Mac (MEASURED-OWN, `ls -ld`), so a helper
  binary there is replaceable by root alone; `/usr/local/bin` is too on that Mac, but Homebrew on Intel Macs
  makes it the user's, so the helper never runs from there.

Both are checked by the lab (`bench/install/macos_lab.sh`): it records the socket's mode and group, and that
a user outside `hexnode` (`nobody`) can't connect. **Falsified if** the socket comes up another group's or
wider than `0770`, or `nobody` connects.

## What Hex does

| OS | Install makes | Unprivileged detection | Uninstall removes | "Nothing left" check (by listing) |
|---|---|---|---|---|
| Linux | `/etc/systemd/system/user@.service.d/hex-delegate.conf` (`Delegate=cpu cpuset io memory pids`); `daemon-reload` + `reload user@UID.service`; `/var/lib/systemd/linger/<user>` if asked and absent; `/etc/subuid`/`/etc/subgid` lines via `usermod -v/-w` if the user has none; an AppArmor profile for `hex` only where the Ubuntu sysctl reads 1; the node's user unit (`Delegate=yes`, `DelegateSubgroup=`) | own cgroup's `cgroup.controllers` ⊇ {cpu, io, memory, pids}; `user.delegate` xattr; write access to `cgroup.subtree_control`; `/proc/sys/kernel/apparmor_restrict_unprivileged_userns`, `/proc/sys/user/max_user_namespaces`; `getsubids` | the drop-in, the profile, the user unit; `disable-linger` and `usermod -V/-W` only for what install's record says it added; `daemon-reload` + reload | `ls /etc/systemd/system/user@.service.d/`, `ls /var/lib/systemd/linger/`, `grep` of `/etc/subuid`/`/etc/subgid` against the record, `aa-status`/profile dir, `systemctl --user list-unit-files 'hex*'`: none of install's entries remain |
| Windows | `Containers` and `HypervisorPlatform` enabled (`-All -NoRestart`), one reboot if `RestartNeeded`; a record of which were already on | `Win32_OptionalFeature.InstallState` = 1 (measure as standard user); `WinHvPlatform.dll` loads and `WHvCapabilityCodeHypervisorPresent` TRUE; `vmcompute` service/`vmcompute.dll` present (measure) | `Disable-WindowsOptionalFeature -Online -NoRestart` for the features install enabled, one reboot | `Win32_OptionalFeature` lists each recorded feature back at its pre-install `InstallState` |
| macOS | group `hexbox` and users `_hexboxN` (hidden, `/var/empty`, `/sbin/nologin`, `PrimaryGroupID` = the group) in a range checked free first; `/Library/LaunchDaemons/<label>.plist` (root:wheel, 644), helper binary, `launchctl bootstrap system` | `dscl . -list /Users UniqueID` / `/Groups PrimaryGroupID`; `launchctl print system/<label>` exit 0 (113 = absent); connect to the helper's socket | `launchctl bootout system/<label>`; remove plist and binary; `dscl . -delete` each `/Users/_hexboxN` and `/Groups/hexbox` | `dscl . -list /Users` has no `_hexbox*`, `/Groups` no `hexbox`; `launchctl print system/<label>` exits 113; plist and binary paths absent |

Open measurements before the design leans on them: (1) Linux: `daemon-reload` alone vs `reload
user@UID.service` for a newly delegated `io` (§1.4); (2) Windows: standard-user `Win32_OptionalFeature`
read, `WinHvPlatform.dll` presence with the feature off, `vmcompute` service presence vs Containers, and
whether `dism.exe` exits 3010 (§2.2, §2.4); (3) macOS: the UID/GID range on each supported release
before choosing Hex's base.
