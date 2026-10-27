//! Paths in a container's root, resolved inside it. The container init sets its root up before it
//! pivots into it, while `/` is still the host's, and an image's root may hold a link to anywhere: an
//! ordinary walk of an absolute link lands on the host, where making a missing mount target writes.
//! So every path the init touches in the root is opened with `openat2(RESOLVE_IN_ROOT)`, which walks
//! a link as though the root were `/` (openat2(2)), and what's missing is made relative to the
//! directory it resolved to; a mount goes on the opened file through its `/proc/self/fd` link, as
//! runc mounts (libcontainer/rootfs_linux.go). A path that leads, through a link, to nothing inside
//! the root is refused by name, never followed out (CLAUDE.md §3).

use std::os::fd::{AsRawFd, OwnedFd};
use std::path::Path;

use rustix::fs::{Mode, OFlags, ResolveFlags};
use rustix::io::Errno;

use crate::linux::oci::OciError;

/// What a missing path is made as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Make {
    /// A directory, as a mount of a filesystem or a directory needs.
    Dir,
    /// An empty file, as a bind of a file needs.
    File,
    /// Nothing: a missing path is left missing.
    Nothing,
}

/// `dest`, a path in the container, opened (`O_PATH`) in the root at `rootfs`, with what's missing
/// made first as `make` says, its parents as directories; `None` when it's missing and `make` is
/// [`Make::Nothing`].
pub fn open(rootfs: &Path, dest: &str, make: Make) -> Result<Option<OwnedFd>, OciError> {
    let root = root(rootfs)?;
    let parts: Vec<&str> = dest
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    let mut done = String::new();
    for (i, part) in parts.iter().enumerate() {
        let parent = if done.is_empty() {
            ".".to_owned()
        } else {
            done.clone()
        };
        if !done.is_empty() {
            done.push('/');
        }
        done.push_str(part);
        let last = i.checked_add(1) == Some(parts.len());
        match in_root(&root, &done) {
            Ok(fd) if last => return Ok(Some(fd)),
            Ok(_) => continue,
            Err(Errno::NOENT) => {}
            Err(e) => return Err(failed(dest, &done, e)),
        }
        if make == Make::Nothing {
            return Ok(None);
        }
        let dir = in_root(&root, &parent).map_err(|e| failed(dest, &parent, e))?;
        let made = match if last { make } else { Make::Dir } {
            Make::File => rustix::fs::openat(
                &dir,
                *part,
                OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::WRONLY | OFlags::CLOEXEC,
                Mode::from_bits_truncate(0o644),
            )
            .map(drop),
            Make::Dir | Make::Nothing => {
                rustix::fs::mkdirat(&dir, *part, Mode::from_bits_truncate(0o755))
            }
        };
        match made {
            // Already there: a link, which the next open follows inside the root.
            Ok(()) | Err(Errno::EXIST) => {}
            Err(e) => return Err(failed(dest, &done, e)),
        }
        match in_root(&root, &done) {
            Ok(fd) if last => return Ok(Some(fd)),
            Ok(_) => {}
            Err(Errno::NOENT) => {
                return Err(OciError::Config(format!(
                    "{dest}: {done} in the container's root is a link to nothing inside it"
                )));
            }
            Err(e) => return Err(failed(dest, &done, e)),
        }
    }
    // No component: the root itself.
    in_root(&root, ".")
        .map(Some)
        .map_err(|e| failed(dest, ".", e))
}

/// `dest`'s parent directory opened in the root (made when missing) and `dest`'s last component,
/// for a call that makes that component itself, relative to the directory (`mknodat`, `symlinkat`).
pub fn parent(rootfs: &Path, dest: &str) -> Result<(OwnedFd, String), OciError> {
    let trimmed = dest.trim_end_matches('/');
    let (dir, name) = trimmed.rsplit_once('/').unwrap_or(("", trimmed));
    if name.is_empty() || name == "." || name == ".." {
        return Err(OciError::Config(format!("{dest:?} names no file")));
    }
    let dir = open(rootfs, dir, Make::Dir)?
        .ok_or_else(|| OciError::Config(format!("{dest}: its directory can't be made")))?;
    Ok((dir, name.to_owned()))
}

/// The path a mount call takes to land on the file `fd` names: its `/proc/self/fd` link, which the
/// kernel follows to exactly that file, with no walk through the root's links again. It names the
/// file only while `fd` is open, so the caller holds `fd` until the call that uses it returns.
pub fn fd_path(fd: &OwnedFd) -> String {
    format!("/proc/self/fd/{}", fd.as_raw_fd())
}

fn root(rootfs: &Path) -> Result<OwnedFd, OciError> {
    rustix::fs::open(
        rootfs,
        OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| OciError::os("opening the container's root", e))
}

/// `path` opened in `root` as though `root` were `/`; magic links (`/proc/<pid>/root`) refused, as
/// openat2(2) advises asking for explicitly.
fn in_root(root: &OwnedFd, path: &str) -> Result<OwnedFd, Errno> {
    rustix::fs::openat2(
        root,
        path,
        OFlags::PATH | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::IN_ROOT | ResolveFlags::NO_MAGICLINKS,
    )
}

fn failed(dest: &str, at: &str, e: Errno) -> OciError {
    OciError::os(
        "resolving a path in the container's root",
        format!("{dest} ({at}): {e}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    /// A root holding links out of it, absolute and relative, and one to a directory inside it.
    fn root_with_links(outside: &Path) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        symlink(outside, root.path().join("etc")).unwrap();
        symlink("../../../../..", root.path().join("up")).unwrap();
        std::fs::create_dir(root.path().join("real-etc")).unwrap();
        symlink("/real-etc", root.path().join("inside")).unwrap();
        root
    }

    /// An image's absolute link to a host directory: the path through it is refused by name, and
    /// nothing is made on the host.
    #[test]
    fn a_link_out_of_the_root_is_never_followed_out() {
        let host = tempfile::tempdir().unwrap();
        let root = root_with_links(host.path());
        let got = open(root.path(), "/etc/hosts", Make::File);
        assert!(
            matches!(&got, Err(OciError::Config(why)) if why.contains("a link to nothing inside it")),
            "{got:?}"
        );
        assert_eq!(std::fs::read_dir(host.path()).unwrap().count(), 0);
    }

    /// A relative link climbing out stops at the root, as `..` does at `/`: what's made lands inside.
    #[test]
    fn a_link_climbing_out_stays_at_the_root() {
        let host = tempfile::tempdir().unwrap();
        let root = root_with_links(host.path());
        open(root.path(), "/up/made/here", Make::Dir).unwrap();
        assert!(root.path().join("made/here").is_dir());
        let above = root.path().parent().unwrap().join("made");
        assert!(!above.exists(), "made above the root");
    }

    /// A link to a directory inside the root is followed there, absolute as it is.
    #[test]
    fn a_link_inside_the_root_is_followed_inside() {
        let host = tempfile::tempdir().unwrap();
        let root = root_with_links(host.path());
        let fd = open(root.path(), "/inside/hosts", Make::File).unwrap();
        assert!(fd.is_some());
        assert!(root.path().join("real-etc/hosts").is_file());
        let (dir, name) = parent(root.path(), "/inside/null").unwrap();
        assert_eq!(name, "null");
        rustix::fs::mkdirat(&dir, "by-parent", Mode::RWXU).unwrap();
        assert!(root.path().join("real-etc/by-parent").is_dir());
    }

    /// Missing and asked to make nothing: nothing is made; the root itself is a path too.
    #[test]
    fn nothing_is_made_unless_asked() {
        let root = tempfile::tempdir().unwrap();
        assert!(
            open(root.path(), "/proc/sys", Make::Nothing)
                .unwrap()
                .is_none()
        );
        assert!(!root.path().join("proc").exists());
        assert!(open(root.path(), "/", Make::Nothing).unwrap().is_some());
        assert!(parent(root.path(), "/").is_err());
    }
}
