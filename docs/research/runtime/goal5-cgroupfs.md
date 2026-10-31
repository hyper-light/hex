# A rootless box's cgroup filesystem: the host tree it carries, and keeping it read-only

Source notes for goal 5's added security guard (Ada's call, 2026-10-31; kiln research
`01a190c51f137042a9697949c8fe2c7a`, findings f85-f87, graded from these sources). A research agent
flagged that a rootless `oci` box on the host's network mounts the host's cgroup filesystem writable
beneath the box's own overmounts (`crates/hex-core/src/bundle.rs`, `default_mounts`), and asked whether
a box holding `CAP_SYS_ADMIN` in its own user namespace could unmount those overmounts and reach other
boxes' cgroup files. It can — and before this fix could write them. This note records what was found
and why the fix holds.

Sources were read at fixed revisions: Linux `cgroup-v2.rst` at v6.19
(`raw.githubusercontent.com/torvalds/linux/v6.19/Documentation/admin-guide/cgroup-v2.rst`); the man
pages `mount_namespaces(7)`, `mount_setattr(2)` and `cgroup_namespaces(7)` at man-pages-5.13
(`raw.githubusercontent.com/mkerrisk/man-pages/man-pages-5.13/`), downloaded into
`~/.cache/hex-t7/goal5-cgfs-src/`. The behaviour was measured on fedora (kiln-a094a12d, Linux
6.19.10) by `crates/hex-cli/tests/oci_cgroupfs_box.rs`, run with the fix off and on.

## 1. The layout under scrutiny

A rootless container in a user namespace of its own can't mount a `sysfs` while it shares the host's
network namespace (mounting one takes `CAP_SYS_ADMIN` in the user namespace that owns the mounter's
network namespace, which the box's doesn't hold), so Hex binds the host's `/sys` instead, as runc's
rootless containers do. The bind must be recursive — a user namespace refuses a plain bind that would
cut off the locked submounts beneath `/sys` — so it carries the host's `cgroup2` filesystem. Over it
Hex mounts a read-only `tmpfs` and then the box's own `cgroup2` (in the box's cgroup namespace, so it
shows only the box's subtree). `crates/hex-core/src/bundle.rs`, `default_mounts` (rootless, host
network):

- `&["rbind", "nosuid", "noexec", "nodev", "ro"],` — the host `/sys` bind (`bundle.rs:442`).
- `"cgroup2",` with `&["nosuid", "noexec", "nodev", "ro"]` — the box's own cgroup filesystem over it
  (`bundle.rs:484-486`).

The box is in its own cgroup namespace, whose root is its own cgroup, so what it sees of
`/sys/fs/cgroup` through its own `cgroup2` is only its subtree:

- `cgroup_namespaces(7):40`: "Each cgroup namespace has its own set of cgroup root directories."
- `cgroup-v2.rst:3034` (v6.19): "The 'cgroupns root' for a cgroup namespace is the cgroup in which the
  process calling unshare(2) is running." (The lab reads `self_cgroup=0::/`.)

## 2. Why it was a hole (f85)

The overmounts (the box's `cgroup2` and the `tmpfs`) are the box's own mounts, made in its own user
namespace, so a box holding `CAP_SYS_ADMIN` can unmount them. Beneath lies the host's `cgroup2` bind,
which the recursive bind left **writable** (its read-only remount covered only the bind's top; see §3).
Two things that would otherwise stand in the way do not:

- **nsdelegate.** fedora mounts `cgroup2` `nsdelegate`, but it guards only WRITES to a cgroup namespace
  root's files from inside that namespace — not reads, and not a mount owned by the init namespace, which
  the host `/sys` bind is.
  - `cgroup-v2.rst:188`: "Consider cgroup namespaces as delegation boundaries."
  - `cgroup-v2.rst:554-555`: "... the kernel rejects writes to all / files on a namespace root from
    inside the cgroup namespace, except for ..." (the delegatable files).
- **DAC.** A rootless box's processes run as the node's uid, which owns every box's cgroup files, so
  file permissions allow it.

Measured with the fix off (`oci_cgroupfs_box.rs`, fedora 6.19.10): a box holding `CAP_SYS_ADMIN`
unmounted its overmounts and wrote a second box's cgroup files —
`write:victim.cgroup.kill=OK[]`, `write:victim.memory.max=OK[]`, `write:parent.memory.max=OK[]` — i.e.
it could kill, reconfigure and reach the host tree.

## 3. The fix, and why it holds (f86)

A `mount(2)` remount changes one mount; recursing an attribute change over a whole tree is what
`mount_setattr`'s `AT_RECURSIVE` is for, so remounting only the bind's top left the submounts the
recursive bind carried with their source's flags:

- `mount_setattr(2):132-133`: "AT_RECURSIVE / Change the mount properties of the entire mount tree."

So the runtime now remounts each cgroup filesystem the recursive read-only bind carries read-only too
(`crates/hex-runtime/src/linux/oci/init.rs`, `do_mount` → `remount_cgroupfs_readonly`, a flat bounded
pass over `/proc/self/mountinfo`, `init.rs:693-801`). Measured with the fix on (same lab): the revealed
host cgroup filesystem is read-only (`cgfs_top_opts=ro,nosuid,nodev,noexec,relatime`) and every write
is refused with `Read-only file system` — the box can no longer kill, reconfigure or migrate.

## 4. The read residual (f87)

The box can still READ the host cgroup tree it reaches. This is inherent to a host-network rootless box:

- The host `/sys` bind is read-only, and a read-only mount permits reads.
- The host `/sys/fs/cgroup` submount is locked in the box's user namespace, so it can't be removed,
  only made read-only: `mount_namespaces(7):1076`: "are locked so that they may not be individually
  unmounted."
- A box WITHOUT `CAP_SYS_ADMIN` can't reach it at all: it can't unmount its own cgroup mounts, so it
  sees only its own subtree through its own `cgroup2` in its own cgroup namespace (§1). The lab confirms
  reads still succeed with the fix on (`read:victim.memory.max=OK[max]`).

Closing the read residual too would mean refusing the rootless + host-network + `CAP_SYS_ADMIN`
combination with a typed error (CLAUDE.md §3), since Hex can't isolate such a box's view of the host
cgroup tree. That is a policy choice left to the owner.

## 5. What would falsify this

- A kernel where a read-only recursive bind already covers its submounts (then the top-only remount
  wouldn't matter): refuted by §2's measurement (writes succeeded with the fix off) and `AT_RECURSIVE`'s
  existence (§3).
- nsdelegate denying writes to a sibling box's files through the host (init-namespace) bind: refuted by
  §2's measurement (writes succeeded). It guards only the namespace root's files from inside the
  namespace (`cgroup-v2.rst:554-555`).
- A way to remove or empty the host `cgroup2` beneath the box (then reads could be closed by mounts):
  refuted by the submount's lock (`mount_namespaces(7):1076`).
