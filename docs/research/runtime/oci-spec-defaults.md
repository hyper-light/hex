# A container from a structured spec: what the reference engines give it, and what the kernel allows

The sources behind `docs/design/runtime.md`, "Decision: a box from an OciSpec runs as `docker run` runs
its image" (runtime goal 3, plan step 5). Accessed 2026-10-27. Code is quoted from
raw.githubusercontent.com at the revision fetched that day (`main` or `master` unless a tag is named),
with its lines; the kernel at tag `v6.19`, fedora's; man pages from man7.org. Every number is `DESIGN`
(a fact from code or docs) unless marked `MEASURED`.

## 1. What `docker run` gives a container that names nothing else (moby)

- **The default spec**: `daemon/pkg/oci/defaults.go`, `DefaultLinuxSpec` (lines 53-189):
  - capabilities `caps.DefaultCapabilities()` as the bounding, permitted and effective sets (57-61), no
    inheritable or ambient; `daemon/pkg/oci/caps/defaults.go` (4-21) lists them: CAP_CHOWN,
    CAP_DAC_OVERRIDE, CAP_FSETID, CAP_FOWNER, CAP_MKNOD, CAP_NET_RAW, CAP_SETGID, CAP_SETUID, CAP_SETFCAP,
    CAP_SETPCAP, CAP_NET_BIND_SERVICE, CAP_SYS_CHROOT, CAP_KILL, CAP_AUDIT_WRITE;
  - mounts (64-107): `/proc` proc `nosuid,noexec,nodev`; `/dev` tmpfs `nosuid,strictatime,mode=755,
    size=65536k`; `/dev/pts` devpts `nosuid,noexec,newinstance,ptmxmode=0666,mode=0620,gid=5`; `/sys`
    sysfs `nosuid,noexec,nodev,ro`; `/sys/fs/cgroup` cgroup `ro,nosuid,noexec,nodev`; `/dev/mqueue`
    mqueue `nosuid,noexec,nodev`; `/dev/shm` tmpfs `nosuid,noexec,nodev,mode=1777`;
  - read-only paths `/proc/bus`, `/proc/fs`, `/proc/irq`, `/proc/sys`, `/proc/sysrq-trigger` (110-116);
  - namespaces mount, network, time, uts, pid, ipc (117-124);
  - device rules: deny all, then allow 1:5, 1:3, 1:9, 1:8, 5:0 and 5:1, and deny 10:229 (130-185);
  - no `noNewPrivileges` and no `rlimits`: a container inherits the engine's limits.
- **Masked paths**, `defaultLinuxMaskedPaths` (194-219): `/proc/acpi`, `/proc/asound`, `/proc/interrupts`
  ("https://github.com/moby/moby/security/advisories/GHSA-6fw5-f8r9-fgfm"), `/proc/kcore`, `/proc/keys`,
  `/proc/latency_stats`, `/proc/sched_debug`, `/proc/scsi`, `/proc/timer_list`, `/proc/timer_stats`,
  `/sys/devices/virtual/powercap` (GHSA-jq35-85cj-fj4p), `/sys/firmware`, then, for each possible CPU,
  `/sys/devices/system/cpu/cpu%d/thermal_throttle` where it exists (GHSA-6fw5-f8r9-fgfm again).
- **`/etc/hosts`**, `daemon/libnetwork/etchosts/etchosts.go`: `defaultContentIPv4` is `127.0.0.1
  localhost` and `defaultContentIPv6` is `::1 localhost ip6-localhost ip6-loopback`, `fe00:: ip6-localnet`,
  `ff00:: ip6-mcastprefix`, `ff02::1 ip6-allnodes`, `ff02::2 ip6-allrouters` (29-39); a record is written
  `"%s\t%s\n"`, the address then the names (23-26); `Build` writes the defaults and then the extra records,
  the `--add-host` lines (55-57, 70-84).

## 2. What containerd's default spec gives one

- `pkg/oci/spec.go`, `populateDefaultUnixSpec` (163-217): `NoNewPrivileges: true`; one rlimit,
  `RLIMIT_NOFILE` 1024 hard and soft; the same fourteen capabilities (`defaultUnixCaps`, 124-141); the same
  read-only paths; devices deny all; namespaces pid, ipc, uts, mount, network (`defaultUnixNamespaces`,
  143-161). Masked paths (`cachedDefaultLinuxMaskedPaths`, 262-279) are moby's list, the thermal-throttle
  paths appended by `appendCPUThrottlePaths` (`pkg/oci/spec_linux.go`, 34-42).
- `pkg/oci/mounts.go`, `defaultMounts` (25-71): moby's mounts without the cgroup one, `/dev/shm` sized
  `size=65536k` (the 64 MiB Docker's `--shm-size` defaults to), and a `/run` tmpfs.

## 3. What runc changes for a rootless container

`libcontainer/specconv/example.go`, `ToRootless` (161-220): it drops the network namespace and adds a
user namespace mapping the caller's euid and egid, alone, to 0 (164-189); "Replace the /sys mount with an
rbind" of the host's `/sys`, options `rbind,nosuid,noexec,nodev,ro` (194-202); "Remove all gid= and
uid= mappings" from mount options (205-211); and it removes `linux.resources` (218-219).

## 4. The kernel

- **overlayfs** (`Documentation/filesystems/overlayfs.rst`): "The specified lower directories will be
  stacked beginning from the rightmost one and going left", lower1 the top (361-363); colons in a lower
  directory's name are escaped with a backslash (366-368); "The "-o userxattr" mount option forces
  overlayfs to use the "user.overlay." xattr namespace instead of "trusted.overlay.". This is useful for
  unprivileged mounting of overlayfs." (817-819). `fs/overlayfs/params.h:20`: `#define OVL_MAX_STACK 500`,
  enforced at `fs/overlayfs/params.c:539-540` ("too many lower directories, limit is %d"). The work
  directory overlayfs makes is mode 0: `fs/overlayfs/super.c`, `ovl_workdir_create` (311), `.ia_mode =
  S_IFDIR | 0` (326).
- **mount(2)'s options** are one page: `fs/namespace.c`, `copy_mount_options` (3892-3904), `copy =
  kmalloc(PAGE_SIZE, GFP_KERNEL)` and `copy_from_user(copy, data, PAGE_SIZE)`.
- **The same filesystem over its own root is refused**: `fs/namespace.c:3614-3617`, "Refuse the same
  filesystem on the same mount point", `return -EBUSY` when the new mount's superblock and the mount point's
  are one and the point is that mount's root. This is the legacy mount(2) path; util-linux's `mount`
  uses the new mount API by default, which doesn't pass through it (so a `mount -t cgroup2` by hand
  succeeds where Hex's call fails; `LIBMOUNT_FORCE_MOUNT2=always` reproduces Hex's).
- **Loopback** is interface 1 in every network namespace: `include/net/flow.h:25`, `#define
  LOOPBACK_IFINDEX 1`; `drivers/net/loopback.c:222`, `BUG_ON(dev->ifindex != LOOPBACK_IFINDEX);`.
- **overlayfs's magic number**: `include/uapi/linux/magic.h:38`, `OVERLAYFS_SUPER_MAGIC 0x794c7630`.
- **openat2(2)**, `RESOLVE_IN_ROOT`: "Treat the directory referred to by dirfd as the root directory
  while resolving path. Absolute symbolic links are interpreted relative to dirfd. If a prefix
  component of path equates to dirfd, then an immediately following .. component likewise equates to
  dirfd (just as /.. is traditionally equivalent to /)." The same page advises asking for
  `RESOLVE_NO_MAGICLINKS` explicitly rather than relying on `RESOLVE_IN_ROOT` to imply it.

## 5. How runc mounts in an image's root

`libcontainer/rootfs_linux.go` opens each mount destination inside the root (`pathrs.OpenInRoot`, 538;
`pathrs.MkdirAllInRoot`, 577, 636) and mounts on the descriptor: `libcontainer/mount_linux.go`,
`mountViaFds` (146-200), "The dstFd argument, if non-empty, is expected to be in the form of a path to an
opened file descriptor on procfs (i.e. "/proc/thread-self/fd/NN")", passed to `unix.Mount` as the target.

## 6. Measured on adacachy (Linux 7.2.9-2-cachyos, 2026-10-27), rootless

- `MEASURED`: a fresh network namespace's `lo` has flags `0x8` (LOOPBACK, not UP), and a connect to
  127.0.0.1 fails with ENETUNREACH (`unshare -Urn` and python3's `SIOCGIFFLAGS`).
- `MEASURED`: in a user namespace of one's own, overlayfs with `userxattr`, sysfs (with a network
  namespace of one's own), cgroup2 (with a cgroup namespace), devpts without `gid=5`, and mqueue (with an
  IPC namespace) all mount (`unshare -Urmnpfi --mount-proc --cgroup`); a removed lower file leaves a
  `0, 0` character device in the upper layer, and `work/work` is mode 0.
- `MEASURED`: in a user namespace, a plain (non-recursive) bind of the host's `/sys` is refused (EINVAL):
  it has locked mounts beneath it. An `rbind` succeeds, and carries the host's cgroup2 mount.
- `MEASURED`: over that `rbind`, `mount(2)` of cgroup2 at `/sys/fs/cgroup` is refused (EBUSY,
  `LIBMOUNT_FORCE_MOUNT2=always`); with a read-only tmpfs mounted there first, cgroup2 mounts, read-only,
  and shows the namespace's own cgroup.
