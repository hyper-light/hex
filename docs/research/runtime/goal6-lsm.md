# SELinux mount labels and AppArmor profiles: how crun and runc apply them (goal 6, AC-T7.1.4)

Sources, read 2026-11-05:
- crun at `~/.cache/hex-research/crun/src` (1.30.1, the tree the earlier goal notes cite).
- runc at `~/.cache/hex-nofile/runc` (`09a3cb038c9673b96945434a88ff5eea982dcb53`, 2026-11-04), with its vendored
  `github.com/opencontainers/selinux`.
- Linux v6.19 from raw.githubusercontent.com/torvalds/linux/v6.19: `fs/fs_context.c`,
  `security/selinux/hooks.c`, `security/apparmor/lsm.c`, `security/apparmor/domain.c`.
- opencontainers/runtime-tools `bd3b0bd` (`~/.cache/hex-rt/runtime-tools`), the suite AC-T7.1.4 names.

## 1. `linux.mountLabel`: which mounts get it, and how

- crun labels a filesystem it makes by the `context` mount option. `add_selinux_mount_label` appends
  `context="<label>"` to the mount's data (`src/libcrun/utils.c:946-963`). The label is quoted because an MLS
  label carries commas (`s0:c715,c811`). On the new mount API crun sets the label as a parameter of its own,
  `fsconfig(FSCONFIG_SET_STRING, "context", label)` (`src/libcrun/linux.c:1081-1086`), so no quoting is needed.
- Which filesystems: `get_mount_label_how` (`src/libcrun/linux.c:3008-3018`). proc and sysfs take no label.
  mqueue takes it as the `security.selinux` xattr on the mount's root (`LABEL_XATTR`; the setxattr is at
  `src/libcrun/linux.c:3529-3533`). Everything else crun makes takes `context=`. cgroup2 is mounted with
  `LABEL_NONE` (`src/libcrun/linux.c:2038`, `:2057`). Bind mounts make no filesystem, so they get no label.
  crun's `/dev` tmpfs is labelled (`src/libcrun/linux.c:6372-6384`).
- runc does the same. `mountPropagate` formats the data with `label.FormatMountLabel`, which gives
  `context="<label>"` (`libcontainer/rootfs_linux.go:1484`;
  `vendor/github.com/opencontainers/selinux/go-selinux/label/label.go:23-48`). mqueue gets
  `label.SetFileLabel` on the mount point (`libcontainer/rootfs_linux.go:651-657`). The tmpfs runc masks a
  directory with is labelled (`libcontainer/rootfs_linux.go:1327`, `:1334`).
- The kernel: every `fsconfig` parameter goes to the LSM before the filesystem sees it
  (`fs/fs_context.c:137-142`). SELinux claims `context` (`security/selinux/hooks.c:2847`, `:2855-2865`). With
  SELinux not active, no LSM claims the key, and the filesystem's own parser refuses it as unknown
  (`fs/fs_context.c:144-148`). So the kernel itself refuses a label it can't apply.

## 2. `process.apparmorProfile`: applied on exec

- runc: `changeOnExec` writes `exec <profile>` to `/proc/thread-self/attr/apparmor/exec`, or to
  `/proc/thread-self/attr/exec` where the scoped file is missing, before `execve`
  (`libcontainer/apparmor/apparmor_linux.go:24-46`). An `ENOENT` from the write means the profile isn't
  loaded (`:42-45`). The first process does this in `standard_init_linux.go:139`, an exec'd one in
  `setns_init_linux.go:120`.
- crun does the same (`set_apparmor_profile` and `set_security_attr`, `src/libcrun/utils.c:1009-1043`,
  `:1107-1129`). It opens through `/proc/thread-self/attr`, checks that the file is on procfs
  (`check_proc_super_magic`, `:994-1007`), and writes `stack <profile>` in place of `exec` when no_new_privs is
  set and the process is already confined (`:1121-1124`; the confinement read is `:1068-1105`). It applies the
  profile after the mounts and before the user changes (`src/libcrun/container.c:1455-1468`).
- The kernel: `exec` and `stack` are the two commands the exec attribute takes
  (`security/apparmor/lsm.c:902-909`). Under no_new_privs, a confined task may only move to a subset of its
  profile, which a stack is (`security/apparmor/domain.c:978-985`, `:1411`).
- The write must happen before the uid changes. A set*id call that changes the credentials makes the task
  non-dumpable, and then its `/proc/<pid>` files belong to root (proc(5), "/proc/pid"), so a non-root
  container user could no longer write its own attribute.

## 3. When the module isn't enabled: crun skips, runc refuses

- crun skips silently. `add_selinux_mount_label` passes the data through unchanged when SELinux is off
  (`src/libcrun/utils.c:954-962`). `set_apparmor_profile` returns 0 when AppArmor is off (`:1111-1128`). crun
  decides SELinux is on from `selinux` appearing in `/proc/mounts` (`:885-906`), and AppArmor is on from
  `/sys/module/apparmor/parameters/enabled` reading `Y` (`:909-935`). So inside a container where selinuxfs
  isn't mounted, crun drops the label even on an enforcing host. That is how crun passed both tests in
  Hex's 2026-10-27 record (`bench/oci-validation/SUMMARY-hex.txt`).
- runc refuses. A process label with SELinux off is a validation error
  (`libcontainer/configs/validate/validator.go:172-178`). `FormatMountLabel` adds `context=` whatever the host
  is (`label.go:38-48`), so the mount fails where SELinux isn't active (§1). The AppArmor write fails where
  AppArmor isn't the LSM handling the attribute. runc's `isEnabled` reads `/sys/kernel/security/apparmor`
  and `/sys/module/apparmor/parameters/enabled` (`apparmor_linux.go:14-21`).

## 4. What the two runtime-tools tests check

- `linux_mount_label` sets the label `system_u:object_r:svirt_sandbox_file_t:s0:c715,c811`. In the container,
  `validateMountLabel` compares that label with the file label of every bind mount's destination
  (`cmd/runtimetest/main.go:1218-1247`). The suite's default spec has no bind mounts, so the test passes
  whenever the container runs.
- `linux_process_apparmor_profile` sets `acme_secure_profile`. `validateApparmorProfile` passes when
  `rootfs/etc/apparmor.d/<profile>` does not exist (`main.go:1206-1216`). So it passes whenever the
  container runs: under crun, when the profile is loaded or AppArmor is off; under runc, only when the
  profile is loaded.
- SELinux and AppArmor are both exclusive LSMs (`LSM_FLAG_EXCLUSIVE`, `security/selinux/hooks.c:7670`,
  `security/apparmor/lsm.c:2559`), so no one kernel runs both. A runtime that refuses a label
  its host can't apply (runc's way) passes each test only on a host where that module is live.
