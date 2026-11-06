//! `hex selftest-box tree <dir>`: a manifest of everything under `dir`, one line per entry, sorted by
//! path, for HEX-GUEST-21 (docs/design/shards-guest.md §4a): the same walker reads an image's root as
//! containerd unpacks it on Linux and as Hex stages it in a guest, and the two manifests are compared
//! entry by entry. Each line is tab-separated: the path (quoted, relative to `dir`), its kind (`d`, `f`,
//! `l`, `c`, `b`, `p`, `s`), mode (octal, permission and setuid/setgid/sticky bits), uid, gid, mtime
//! (`seconds.nanoseconds`), a link's text (quoted, else `-`), its hard-link group (the first path, in
//! order, of the entries sharing its inode, else `-`), its extended attributes (`name=hex` by name,
//! `,`-joined, else `-`) and a regular file's sha256 (else `-`). atime isn't written: reading a file
//! moves it. `dir` itself isn't an entry: its owner and times are whoever made it.
//!
//! Flat (CLAUDE.md §11): a list of directories to read. Bounded (§2): at most [`ENTRIES`] entries, an
//! entry's xattr names within Linux's `XATTR_LIST_MAX` and each value within `XATTR_SIZE_MAX`; past any,
//! the act fails naming the bound.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The most entries a manifest holds: the staging stream's own bound (`hex_core::stream::files::LIMITS`).
const ENTRIES: usize = 1 << 20;
/// Linux's bounds on an inode's xattr names together and on one value (`XATTR_LIST_MAX`,
/// `XATTR_SIZE_MAX`, include/uapi/linux/limits.h).
const XATTR_LIST_MAX: usize = 1 << 16;
const XATTR_SIZE_MAX: usize = 1 << 16;

/// One entry's line, but its hard-link group, which is known once every entry is.
struct Line {
    dir: bool,
    head: String,
    tail: String,
    inode: Option<(u64, u64)>,
}

/// Writes `dir`'s manifest to stdout; 0 when whole, 1 (with why on stderr) when it can't be.
pub fn tree(dir: &str) -> u8 {
    match manifest(Path::new(dir)) {
        Ok(text) => u8::from(super::write_all(&mut std::io::stdout(), text.as_bytes()).is_err()),
        Err(why) => {
            let _ = std::io::stderr().write_all(format!("tree: {why}\n").as_bytes());
            1
        }
    }
}

fn manifest(root: &Path) -> Result<String, String> {
    let mut lines: BTreeMap<String, Line> = BTreeMap::new();
    let mut dirs: Vec<(PathBuf, String)> = vec![(root.to_owned(), String::new())];
    while let Some((dir, rel)) = dirs.pop() {
        let listing = fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in listing {
            let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
            if lines.len() >= ENTRIES {
                return Err(format!("more than {ENTRIES} entries"));
            }
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| format!("a name under {} isn't UTF-8", dir.display()))?;
            let path = entry.path();
            let rel = if rel.is_empty() {
                name.to_owned()
            } else {
                format!("{rel}/{name}")
            };
            let line = one(&path, &rel)?;
            if line.dir {
                dirs.push((path, rel.clone()));
            }
            lines.insert(rel, line);
        }
    }
    // Each inode shared by more than one entry, named by its first path in order.
    let mut first: BTreeMap<(u64, u64), (&str, usize)> = BTreeMap::new();
    for (path, line) in &lines {
        if let Some(inode) = line.inode {
            let slot = first.entry(inode).or_insert((path.as_str(), 0));
            slot.1 = slot.1.saturating_add(1);
        }
    }
    let mut out = String::new();
    for line in lines.values() {
        let group = match line.inode.and_then(|i| first.get(&i)) {
            Some((name, n)) if *n > 1 => format!("{name:?}"),
            _ => "-".to_owned(),
        };
        out.push_str(&format!("{}\t{group}\t{}\n", line.head, line.tail));
    }
    Ok(out)
}

/// `path`'s line, `rel` its name in the manifest.
fn one(path: &Path, rel: &str) -> Result<Line, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{rel}: {e}"))?;
    let t = meta.file_type();
    let kind = if t.is_dir() {
        "d"
    } else if t.is_file() {
        "f"
    } else if t.is_symlink() {
        "l"
    } else if t.is_char_device() {
        "c"
    } else if t.is_block_device() {
        "b"
    } else if t.is_fifo() {
        "p"
    } else {
        "s"
    };
    let link = if t.is_symlink() {
        let target = fs::read_link(path).map_err(|e| format!("{rel}: {e}"))?;
        format!(
            "{:?}",
            target
                .to_str()
                .ok_or_else(|| format!("{rel}: its link text isn't UTF-8"))?
        )
    } else {
        "-".to_owned()
    };
    let sha = if t.is_file() {
        sha256(path).map_err(|e| format!("{rel}: {e}"))?
    } else {
        "-".to_owned()
    };
    let head = format!(
        "{rel:?}\t{kind}\t{:04o}\t{}\t{}\t{}.{:09}\t{link}",
        meta.mode() & 0o7777,
        meta.uid(),
        meta.gid(),
        meta.mtime(),
        meta.mtime_nsec(),
    );
    Ok(Line {
        dir: t.is_dir(),
        head,
        tail: format!(
            "{}\t{sha}",
            xattrs(path).map_err(|e| format!("{rel}: {e}"))?
        ),
        inode: if t.is_dir() {
            None
        } else {
            Some((meta.dev(), meta.ino()))
        },
    })
}

fn sha256(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        hash.update(buf.get(..n).unwrap_or_default());
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// `path`'s extended attributes, unfollowed, as `name=hex` sorted by name, or `-`.
fn xattrs(path: &Path) -> Result<String, String> {
    let mut names = vec![0u8; XATTR_LIST_MAX];
    let n = match rustix::fs::llistxattr(path, &mut names) {
        Ok(n) => n,
        Err(rustix::io::Errno::NOTSUP) => 0,
        Err(e) => return Err(format!("listing its xattrs: {e}")),
    };
    let mut all: Vec<String> = Vec::new();
    for name in names.get(..n).unwrap_or_default().split(|b| *b == 0) {
        if name.is_empty() {
            continue;
        }
        let name =
            std::str::from_utf8(name).map_err(|_| "an xattr's name isn't UTF-8".to_owned())?;
        let mut value = vec![0u8; XATTR_SIZE_MAX];
        let len = rustix::fs::lgetxattr(path, name, &mut value)
            .map_err(|e| format!("reading xattr {name}: {e}"))?;
        let hex: String = value
            .get(..len)
            .unwrap_or_default()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        all.push(format!("{name}={hex}"));
    }
    if all.is_empty() {
        return Ok("-".to_owned());
    }
    all.sort();
    Ok(all.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HEX-GUEST-21's walker: write a tree holding a directory, a setuid file, a hard link to it, a
    /// symbolic link and a `user.` xattr, then read its manifest: expect one sorted line per entry with
    /// each field as written, the two names of the file one group, and the root itself not an entry.
    #[test]
    fn a_trees_manifest_holds_every_entry_and_field_sorted() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir(root.join("bin")).unwrap();
        fs::write(root.join("bin/tool"), b"abc").unwrap();
        fs::set_permissions(root.join("bin/tool"), fs::Permissions::from_mode(0o4755)).unwrap();
        fs::hard_link(root.join("bin/tool"), root.join("bin/alias")).unwrap();
        std::os::unix::fs::symlink("bin/tool", root.join("link")).unwrap();
        rustix::fs::setxattr(
            root.join("bin/tool"),
            "user.hex",
            b"v",
            rustix::fs::XattrFlags::empty(),
        )
        .unwrap();
        let text = manifest(root).unwrap();
        let lines: Vec<Vec<&str>> = text.lines().map(|l| l.split('\t').collect()).collect();
        let paths: Vec<&str> = lines.iter().map(|l| l[0]).collect();
        assert_eq!(
            paths,
            ["\"bin\"", "\"bin/alias\"", "\"bin/tool\"", "\"link\""]
        );
        let tool = &lines[2];
        assert_eq!((tool[1], tool[2]), ("f", "4755"));
        assert_eq!(tool[7], "\"bin/alias\"", "its group is its first name");
        assert_eq!(lines[1][7], "\"bin/alias\"");
        // An SELinux host labels the file too (security.selinux), as its policy, not this test, says.
        assert!(
            tool[8].split(',').any(|x| x == "user.hex=76"),
            "{}",
            tool[8]
        );
        assert_eq!(
            tool[9],
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!((lines[3][1], lines[3][6]), ("l", "\"bin/tool\""));
        assert_eq!((lines[0][1], lines[0][9]), ("d", "-"));
    }
}
