//! The spec the runtime checked, handed to what starts the container's init (AC-T7.1.1, T-T7.1.4; found
//! 2026-11-04). The runtime reads and parses a bundle's `config.json` and checks it (`spec.rs`: among the rest, a
//! container in a user namespace holding CAP_SYS_ADMIN without a network namespace of its own is refused). A
//! create seed or the shim that then read `config.json` again would make the container from whatever the file held
//! by then: a spec nobody checked, swapped in between. So neither reads the bundle. The runtime puts the exact bytes
//! it parsed in a memfd and seals it against every change (`F_SEAL_WRITE`, `F_SEAL_SHRINK`, `F_SEAL_GROW`, then
//! `F_SEAL_SEAL`: memfd_create(2), fcntl(2) "File Sealing"), and the seed takes the descriptor over `SCM_RIGHTS`, the
//! shim by its `/proc/<pid>/fd` path as it takes its pipes. Each reads it from offset 0 with `pread` (the seed's
//! descriptor shares the runtime's file offset) and refuses one that isn't sealed so, or is over
//! [`MAX_CONFIG_BYTES`]: the bytes it parses are the bytes the runtime checked.

use std::os::fd::{BorrowedFd, OwnedFd};

use rustix::fs::{MemfdFlags, SealFlags};

use crate::linux::oci::OciError;
use crate::linux::oci::spec::MAX_CONFIG_BYTES;

/// The seals a checked spec's memfd carries: nothing can write, shrink or grow it, nor take a seal away.
const SEALS: SealFlags = SealFlags::WRITE
    .union(SealFlags::SHRINK)
    .union(SealFlags::GROW)
    .union(SealFlags::SEAL);

/// `bytes`, the `config.json` the runtime parsed and checked, in a sealed memfd, close-on-exec.
pub fn seal(bytes: &[u8]) -> Result<OwnedFd, OciError> {
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(OciError::Config(format!(
            "config.json is {} bytes, over {MAX_CONFIG_BYTES}",
            bytes.len()
        )));
    }
    let fd = rustix::fs::memfd_create(
        "hex-checked-config",
        MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
    )
    .map_err(|e| OciError::os("making the checked config's memfd", e))?;
    let mut at = 0usize;
    while let Some(rest) = bytes.get(at..).filter(|rest| !rest.is_empty()) {
        let n = rustix::io::write(&fd, rest)
            .map_err(|e| OciError::os("writing the checked config", e))?;
        if n == 0 {
            return Err(OciError::Broken(
                "the checked config's memfd took no bytes".to_owned(),
            ));
        }
        at = at
            .checked_add(n)
            .ok_or_else(|| OciError::Broken("the checked config's length overflowed".to_owned()))?;
    }
    rustix::fs::fcntl_add_seals(&fd, SEALS)
        .map_err(|e| OciError::os("sealing the checked config", e))?;
    Ok(fd)
}

/// The bytes of a checked spec's memfd ([`seal`]), read from offset 0; refused unless it carries every seal of
/// [`SEALS`] and is at most [`MAX_CONFIG_BYTES`].
pub fn read(fd: BorrowedFd<'_>) -> Result<Vec<u8>, OciError> {
    let seals = rustix::fs::fcntl_get_seals(fd)
        .map_err(|e| OciError::os("reading the checked config's seals", e))?;
    if !seals.contains(SEALS) {
        return Err(OciError::Config(
            "the spec handed over isn't sealed against change, so it may not be the one checked"
                .to_owned(),
        ));
    }
    let size = rustix::fs::fstat(fd)
        .map_err(|e| OciError::os("reading the checked config's size", e))?
        .st_size;
    let size = usize::try_from(size)
        .ok()
        .filter(|&n| n <= MAX_CONFIG_BYTES)
        .ok_or_else(|| {
            OciError::Config(format!(
                "the checked config is {size} bytes, over {MAX_CONFIG_BYTES}"
            ))
        })?;
    let mut bytes = vec![0u8; size];
    let mut at = 0usize;
    while at < size {
        let rest = bytes
            .get_mut(at..)
            .ok_or_else(|| OciError::Broken("the checked config's buffer is short".to_owned()))?;
        let offset = u64::try_from(at)
            .map_err(|_| OciError::Broken("the checked config's offset overflowed".to_owned()))?;
        let n = rustix::io::pread(fd, rest, offset)
            .map_err(|e| OciError::os("reading the checked config", e))?;
        if n == 0 {
            return Err(OciError::Broken(format!(
                "the checked config ended at {at} of its {size} bytes"
            )));
        }
        at = at
            .checked_add(n)
            .ok_or_else(|| OciError::Broken("the checked config's length overflowed".to_owned()))?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::os::fd::AsFd;

    use super::*;

    /// The checked spec's bytes come back whole from a sealed memfd, whatever its file offset (CLAUDE.md §6: do X,
    /// expect Y). Do: seal a config, read it twice. Expect: the bytes each time.
    #[test]
    fn a_sealed_config_reads_back_its_bytes_from_offset_zero() {
        let bytes = br#"{"ociVersion":"1.3.0"}"#;
        let fd = seal(bytes).unwrap();
        assert_eq!(read(fd.as_fd()).unwrap(), bytes);
        assert_eq!(read(fd.as_fd()).unwrap(), bytes);
    }

    /// Nothing can change a sealed config (T-T7.1.4). Do: write to it, and truncate it. Expect: both refused.
    #[test]
    fn a_sealed_config_refuses_every_change() {
        let fd = seal(b"{}").unwrap();
        assert!(rustix::io::pwrite(&fd, b"x", 0).is_err(), "written");
        assert!(rustix::fs::ftruncate(&fd, 0).is_err(), "shrunk");
        assert!(rustix::fs::ftruncate(&fd, 64).is_err(), "grown");
    }

    /// A descriptor that isn't sealed isn't taken for the checked spec (T-T7.1.4). Do: read an unsealed memfd.
    /// Expect: refused by type, naming why.
    #[test]
    fn an_unsealed_descriptor_is_refused() {
        let fd = rustix::fs::memfd_create("unsealed", MemfdFlags::CLOEXEC).unwrap();
        rustix::io::write(&fd, b"{}").unwrap();
        assert!(matches!(read(fd.as_fd()), Err(OciError::Config(why)) if why.contains("sealed")));
    }

    /// A config over the runtime's bound is refused before it's sealed (CLAUDE.md §2).
    #[test]
    fn a_config_over_the_bound_is_refused() {
        let big = vec![b' '; MAX_CONFIG_BYTES + 1];
        assert!(matches!(seal(&big), Err(OciError::Config(_))));
    }
}
