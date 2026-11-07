//! What the node socket's Unix event sources share (Linux's and macOS's): making room for a socket at a path,
//! and an exec's stdio from the descriptors its request carried.

use std::io::{PipeReader, PipeWriter};
use std::os::fd::OwnedFd;
use std::os::unix::fs::FileTypeExt;
use std::path::Path;

use hex_api::{Refusal, Stdio};
use hex_core::wire::Streams;
use rustix::fs::{FileType, OFlags};
use rustix::io::Errno;
use rustix::net::{AddressFamily, SocketAddrUnix, SocketType};

/// Makes room for a socket of `kind` at `path`: a socket no agent serves (one that ended) is removed; one an
/// agent serves, or anything that isn't a socket, is refused.
pub fn clear(path: &Path, kind: SocketType) -> Result<(), Refusal> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(Refusal::os("looking at the node agent's socket path", e)),
    };
    if !meta.file_type().is_socket() {
        return Err(Refusal::Invalid {
            what: "socket path",
            why: format!("{} exists and isn't a socket", path.display()),
        });
    }
    let probe = rustix::net::socket(AddressFamily::UNIX, kind, None)
        .map_err(|e| Refusal::os("probing the node agent's socket", e))?;
    let addr = SocketAddrUnix::new(path).map_err(|e| Refusal::Invalid {
        what: "socket path",
        why: format!("{}: {e}", path.display()),
    })?;
    match rustix::net::connect(&probe, &addr) {
        Ok(()) => Err(Refusal::Invalid {
            what: "socket path",
            why: format!("{} is served by a node agent already", path.display()),
        }),
        Err(Errno::CONNREFUSED) => std::fs::remove_file(path)
            .map_err(|e| Refusal::os("removing an ended node agent's socket", e)),
        Err(e) => Err(Refusal::Os {
            op: "probing the node agent's socket",
            why: format!("{}: {e}", path.display()),
        }),
    }
}

/// A program's stdio from the descriptors its exec carried, in the order `streams` names them: each
/// the right end of a pipe, as the seam's `Stdio` holds; anything else is refused by name.
pub fn stdio_of(streams: Streams, fds: Vec<OwnedFd>) -> Result<Stdio, Refusal> {
    let mut fds = fds.into_iter();
    let mut take = |on: bool, name: &str, reads: bool| -> Result<Option<OwnedFd>, Refusal> {
        if !on {
            return Ok(None);
        }
        let fd = fds.next().ok_or_else(|| Refusal::Invalid {
            what: "stdio",
            why: format!("{name}'s descriptor didn't come"),
        })?;
        pipe_end(&fd, name, reads)?;
        Ok(Some(fd))
    };
    let stdin = take(streams.stdin, "stdin", true)?.map(PipeReader::from);
    let stdout = take(streams.stdout, "stdout", false)?.map(PipeWriter::from);
    let stderr = take(streams.stderr, "stderr", false)?.map(PipeWriter::from);
    Ok(Stdio {
        stdin,
        stdout,
        stderr,
    })
}

/// That `fd` is a pipe's end the program can read (`reads`) or write.
fn pipe_end(fd: &OwnedFd, name: &str, reads: bool) -> Result<(), Refusal> {
    let invalid = |why: String| Refusal::Invalid {
        what: "stdio",
        why: format!("{name} {why}"),
    };
    let stat = rustix::fs::fstat(fd).map_err(|e| invalid(format!("can't be looked at: {e}")))?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Fifo {
        return Err(invalid("isn't a pipe".to_owned()));
    }
    let mode = rustix::fs::fcntl_getfl(fd)
        .map_err(|e| invalid(format!("can't be looked at: {e}")))?
        & OFlags::RWMODE;
    let fits = mode == OFlags::RDWR
        || mode
            == if reads {
                OFlags::RDONLY
            } else {
                OFlags::WRONLY
            };
    if fits {
        Ok(())
    } else if reads {
        Err(invalid(
            "is a pipe's write end, which a program can't read".to_owned(),
        ))
    } else {
        Err(invalid(
            "is a pipe's read end, which a program can't write".to_owned(),
        ))
    }
}
