//! SELinux mount labels and AppArmor profiles (goal 6, AC-T7.1.4): `linux.mountLabel` on every filesystem a
//! container's mounts make, `process.apparmorProfile` on the process's `execve`, and a refusal by name where
//! the host's kernel can't apply either, never a container run without it (CLAUDE.md §3). The decision is
//! docs/design/runtime.md, "SELinux mount labels and AppArmor profiles"; its sources are
//! docs/research/runtime/goal6-lsm.md. The rules are pure and tested; the system calls sit beside them.

use std::ffi::CStr;
use std::os::fd::{AsFd, BorrowedFd};

use rustix::fs::{Mode, OFlags, XattrFlags};

use crate::linux::oci::{Failure, OciError};

/// How a filesystem takes the mount label (crun's `get_mount_label_how`, research §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Labelling {
    /// The `context` parameter of its filesystem context.
    Context,
    /// The `security.selinux` xattr on its root: mqueue, whose files the `context` option doesn't reach.
    Xattr,
    /// None: proc and sysfs show the kernel's own objects, and cgroup2 is mounted unlabelled, as crun mounts it.
    None,
}

/// How a filesystem of type `fstype` takes the mount label.
pub(crate) fn labelling(fstype: &str) -> Labelling {
    match fstype {
        "proc" | "sysfs" | "cgroup" | "cgroup2" => Labelling::None,
        "mqueue" => Labelling::Xattr,
        _ => Labelling::Context,
    }
}

/// What the exec attribute is written: `exec <profile>`, or `stack <profile>` where no_new_privs is set and
/// the process is already confined, the one change AppArmor allows it then (research §2).
pub(crate) fn apparmor_command(profile: &str, no_new_privileges: bool, confined: bool) -> Vec<u8> {
    let verb = if no_new_privileges && confined {
        "stack"
    } else {
        "exec"
    };
    format!("{verb} {profile}").into_bytes()
}

/// Whether `/sys/module/apparmor/parameters/enabled`'s contents say AppArmor is on: `Y`, as crun and runc
/// read it (research §3).
pub(crate) fn apparmor_enabled(parameter: &[u8]) -> bool {
    parameter.first() == Some(&b'Y')
}

/// Whether the AppArmor `current` attribute names a profile: anything but exactly `unconfined`, optionally
/// followed by a newline or a space (crun's `is_current_process_confined`, `src/libcrun/utils.c:1068-1105`).
pub(crate) fn confined(current: &[u8]) -> bool {
    match current.strip_prefix(b"unconfined") {
        Some(rest) => !matches!(rest.first(), None | Some(b'\n') | Some(b' ')),
        None => true,
    }
}

/// The AppArmor parameter crun and runc read for whether the module is on.
const APPARMOR_ENABLED: &str = "/sys/module/apparmor/parameters/enabled";

/// The scoped AppArmor attributes, and the legacy ones a kernel without them has (runc's `changeOnExec`).
const SCOPED: &CStr = c"/proc/thread-self/attr/apparmor";
const SCOPED_EXEC: &CStr = c"/proc/thread-self/attr/apparmor/exec";
const SCOPED_CURRENT: &CStr = c"/proc/thread-self/attr/apparmor/current";
const LEGACY_EXEC: &CStr = c"/proc/thread-self/attr/exec";
const LEGACY_CURRENT: &CStr = c"/proc/thread-self/attr/current";

/// The longest `current` attribute read: `unconfined` and one byte after it decide (see [`confined`]).
const CURRENT_BYTES: usize = 11;

/// A process's AppArmor profile, made ready before a clone: the attribute to write and what to write there.
/// [`ProfileOnExec::apply`] then makes system calls alone, so the child of a vfork-style clone may run it.
#[derive(Debug)]
pub(crate) struct ProfileOnExec {
    attr: &'static CStr,
    command: Vec<u8>,
}

impl ProfileOnExec {
    /// `profile` made ready, or a refusal by name where AppArmor isn't on here.
    pub(crate) fn of(profile: &str, no_new_privileges: bool) -> Result<Self, OciError> {
        let on = match std::fs::read(APPARMOR_ENABLED) {
            Ok(bytes) => apparmor_enabled(&bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(OciError::os("reading the AppArmor parameter", e)),
        };
        if !on {
            return Err(OciError::Config(format!(
                "process.apparmorProfile is set ({profile:?}) but AppArmor isn't enabled on this host \
                 ({APPARMOR_ENABLED} doesn't read Y): Hex refuses rather than run the container without it"
            )));
        }
        let scoped = rustix::fs::access(SCOPED, rustix::fs::Access::EXISTS).is_ok();
        let (attr, current) = if scoped {
            (SCOPED_EXEC, SCOPED_CURRENT)
        } else {
            (LEGACY_EXEC, LEGACY_CURRENT)
        };
        let fd = open_proc(current, OFlags::RDONLY)
            .map_err(|e| OciError::os("opening the AppArmor current attribute", e))?;
        let mut buf = [0u8; CURRENT_BYTES];
        let n = rustix::io::read(&fd, &mut buf)
            .map_err(|e| OciError::os("reading the AppArmor current attribute", e))?;
        let read = buf.get(..n).unwrap_or_default();
        Ok(Self {
            attr,
            command: apparmor_command(profile, no_new_privileges, confined(read)),
        })
    }

    /// Writes the exec attribute, so the profile goes on at the process's next `execve`. Its caller does
    /// this before the uid changes, while the task still owns its `/proc` files (research §2).
    pub(crate) fn apply(&self) -> Result<(), Failure> {
        let fd = open_proc(self.attr, OFlags::WRONLY)
            .map_err(Failure::of("opening the AppArmor exec attribute"))?;
        let written = rustix::io::write(&fd, &self.command).map_err(|e| match e {
            // AppArmor's answer for a profile it doesn't have (runc apparmor_linux.go:42-45).
            rustix::io::Errno::NOENT => Failure {
                what: "applying the AppArmor profile: it isn't loaded",
                errno: e,
            },
            e => Failure {
                what: "writing the AppArmor exec attribute",
                errno: e,
            },
        })?;
        if written != self.command.len() {
            return Err(Failure {
                what: "writing the AppArmor exec attribute: a short write",
                errno: rustix::io::Errno::IO,
            });
        }
        Ok(())
    }
}

/// Opens a `/proc` file and checks it is on procfs, so a container's own file at that path can't stand in
/// (crun's `check_proc_super_magic`, research §2).
fn open_proc(path: &CStr, flags: OFlags) -> rustix::io::Result<rustix::fd::OwnedFd> {
    let fd = rustix::fs::open(path, flags | OFlags::CLOEXEC, Mode::empty())?;
    if rustix::fs::fstatfs(&fd)?.f_type != rustix::fs::PROC_SUPER_MAGIC {
        return Err(rustix::io::Errno::XDEV);
    }
    Ok(fd)
}

/// Asks the kernel for a tmpfs labelled `label` and drops it: a refusal by name where SELinux isn't active
/// (no module claims the `context` key, `fs/fs_context.c:137-148`) or the label isn't one it knows. Create
/// runs this before it makes anything, so a spec with a mount label is never run unlabelled.
pub(crate) fn check_mount_label(label: &str) -> Result<(), OciError> {
    let refused = |step: &str, e: rustix::io::Errno| {
        OciError::Config(format!(
            "linux.mountLabel is set ({label:?}) but the kernel refused it as a tmpfs's context ({step}: \
             {e}): SELinux isn't active here or the label isn't valid, and Hex refuses rather than run the \
             container without it"
        ))
    };
    let fs = rustix::mount::fsopen("tmpfs", rustix::mount::FsOpenFlags::FSOPEN_CLOEXEC)
        .map_err(|e| OciError::os("fsopen", e))?;
    rustix::mount::fsconfig_set_string(fs.as_fd(), "context", label)
        .map_err(|e| refused("the context key", e))?;
    rustix::mount::fsconfig_create(fs.as_fd()).map_err(|e| refused("the filesystem", e))?;
    Ok(())
}

/// Labels a filesystem by the `security.selinux` xattr on its root ([`Labelling::Xattr`]): `mount` is its
/// detached mount, its root opened to set the xattr, which a path-only descriptor can't.
pub(crate) fn label_root(mount: BorrowedFd<'_>, label: &str) -> Result<(), OciError> {
    let root = rustix::fs::openat(
        mount,
        c".",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| OciError::os("opening a filesystem's root to label it", e))?;
    rustix::fs::fsetxattr(
        &root,
        "security.selinux",
        label.as_bytes(),
        XattrFlags::empty(),
    )
    .map_err(|e| OciError::os("labelling a filesystem's root (security.selinux)", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    // AC-T7.1.4 (linux_mount_label): ask how each filesystem takes the label, expect crun's answer
    // (src/libcrun/linux.c:3008-3018, :2038): proc, sysfs and cgroup none, mqueue by xattr, the rest by context.
    #[test]
    fn each_filesystem_takes_the_mount_label_as_crun_gives_it() {
        for fstype in ["proc", "sysfs", "cgroup", "cgroup2"] {
            assert_eq!(labelling(fstype), Labelling::None, "{fstype}");
        }
        assert_eq!(labelling("mqueue"), Labelling::Xattr);
        for fstype in ["tmpfs", "devpts", "overlay", "ramfs"] {
            assert_eq!(labelling(fstype), Labelling::Context, "{fstype}");
        }
    }

    // AC-T7.1.4 (linux_process_apparmor_profile): make the exec attribute's text, expect `exec <profile>`, and
    // `stack <profile>` only where no_new_privs is set on a confined process (src/libcrun/utils.c:1121-1124).
    #[test]
    fn the_profile_is_stacked_only_under_no_new_privs_when_confined() {
        assert_eq!(apparmor_command("acme", false, false), b"exec acme");
        assert_eq!(apparmor_command("acme", false, true), b"exec acme");
        assert_eq!(apparmor_command("acme", true, false), b"exec acme");
        assert_eq!(apparmor_command("acme", true, true), b"stack acme");
    }

    // AC-T7.1.4 and CLAUDE.md §3: read the AppArmor parameter's contents, expect on only for `Y`, so a host
    // that reads anything else refuses the profile rather than drop it.
    #[test]
    fn apparmor_is_on_only_where_its_parameter_reads_y() {
        assert!(apparmor_enabled(b"Y\n"));
        assert!(!apparmor_enabled(b"N\n"));
        assert!(!apparmor_enabled(b""));
    }

    // AC-T7.1.4: read a `current` attribute, expect unconfined only for exactly `unconfined` (then nothing, a
    // newline or a space), so a profile named `unconfined_foo` or a short read counts as confined.
    #[test]
    fn only_the_unconfined_token_reads_unconfined() {
        assert!(!confined(b"unconfined"));
        assert!(!confined(b"unconfined\n"));
        assert!(!confined(b"unconfined "));
        assert!(confined(b"unconfined_"));
        assert!(confined(b"docker-def"));
        assert!(confined(b"unconf"));
    }
}
