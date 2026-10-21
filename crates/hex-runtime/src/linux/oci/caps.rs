//! Capability and rlimit names from the OCI spec, mapped to what the kernel takes. The capability
//! sets: effective, permitted and inheritable go through `capset(2)` (rustix); the bounding and
//! ambient sets through `prctl(2)` by capability number ([`super::sys`]).

use rustix::thread::{CapabilitySet, CapabilitySets};

use crate::linux::oci::spec::Capabilities;
use crate::linux::oci::{OciError, sys};

/// The highest capability number this maps (`CAP_CHECKPOINT_RESTORE`); the kernel may define more,
/// which Hex treats as unknown names.
pub const LAST_CAP: u32 = 40;

/// The capability number for an OCI `CAP_*` name, or `None` if Hex doesn't know it.
pub fn cap_bit(name: &str) -> Option<u32> {
    let n = match name {
        "CAP_CHOWN" => 0,
        "CAP_DAC_OVERRIDE" => 1,
        "CAP_DAC_READ_SEARCH" => 2,
        "CAP_FOWNER" => 3,
        "CAP_FSETID" => 4,
        "CAP_KILL" => 5,
        "CAP_SETGID" => 6,
        "CAP_SETUID" => 7,
        "CAP_SETPCAP" => 8,
        "CAP_LINUX_IMMUTABLE" => 9,
        "CAP_NET_BIND_SERVICE" => 10,
        "CAP_NET_BROADCAST" => 11,
        "CAP_NET_ADMIN" => 12,
        "CAP_NET_RAW" => 13,
        "CAP_IPC_LOCK" => 14,
        "CAP_IPC_OWNER" => 15,
        "CAP_SYS_MODULE" => 16,
        "CAP_SYS_RAWIO" => 17,
        "CAP_SYS_CHROOT" => 18,
        "CAP_SYS_PTRACE" => 19,
        "CAP_SYS_PACCT" => 20,
        "CAP_SYS_ADMIN" => 21,
        "CAP_SYS_BOOT" => 22,
        "CAP_SYS_NICE" => 23,
        "CAP_SYS_RESOURCE" => 24,
        "CAP_SYS_TIME" => 25,
        "CAP_SYS_TTY_CONFIG" => 26,
        "CAP_MKNOD" => 27,
        "CAP_LEASE" => 28,
        "CAP_AUDIT_WRITE" => 29,
        "CAP_AUDIT_CONTROL" => 30,
        "CAP_SETFCAP" => 31,
        "CAP_MAC_OVERRIDE" => 32,
        "CAP_MAC_ADMIN" => 33,
        "CAP_SYSLOG" => 34,
        "CAP_WAKE_ALARM" => 35,
        "CAP_BLOCK_SUSPEND" => 36,
        "CAP_AUDIT_READ" => 37,
        "CAP_PERFMON" => 38,
        "CAP_BPF" => 39,
        "CAP_CHECKPOINT_RESTORE" => 40,
        _ => return None,
    };
    Some(n)
}

fn set_of(names: &[String]) -> CapabilitySet {
    let mut bits = 0u64;
    for name in names {
        if let Some(bit) = cap_bit(name) {
            bits |= 1u64 << bit;
        }
    }
    CapabilitySet::from_bits_retain(bits)
}

fn has(names: &[String], bit: u32) -> bool {
    names.iter().filter_map(|n| cap_bit(n)).any(|b| b == bit)
}

/// Reduces the bounding set to `caps.bounding` (drops every other capability). This runs while the
/// process still has `CAP_SETPCAP` in its effective set — before any uid change, which would clear
/// the effective set and make `PR_CAPBSET_DROP` fail.
pub fn drop_bounding(caps: &Capabilities) -> Result<(), OciError> {
    for bit in 0..=LAST_CAP {
        if !has(&caps.bounding, bit) {
            sys::capbset_drop(bit)?;
        }
    }
    Ok(())
}

/// Sets the effective/permitted/inheritable sets exactly, then clears and raises the ambient set.
/// This runs after the uid change (with `PR_SET_KEEPCAPS` having preserved the permitted set), so
/// the dropped user still holds the capabilities the spec grants. Ambient is last: a capability
/// must be permitted and inheritable before it can be ambient.
pub fn set_process(caps: &Capabilities) -> Result<(), OciError> {
    let sets = CapabilitySets {
        effective: set_of(&caps.effective),
        permitted: set_of(&caps.permitted),
        inheritable: set_of(&caps.inheritable),
    };
    rustix::thread::set_capabilities(None, sets).map_err(|e| OciError::os("capset", e))?;
    sys::ambient_clear()?;
    for name in &caps.ambient {
        if let Some(bit) = cap_bit(name) {
            sys::ambient_raise(bit)?;
        }
    }
    Ok(())
}

/// The rustix resource for an OCI `RLIMIT_*` name.
pub fn rlimit_resource(name: &str) -> Result<rustix::process::Resource, OciError> {
    use rustix::process::Resource;
    let r = match name {
        "RLIMIT_CPU" => Resource::Cpu,
        "RLIMIT_FSIZE" => Resource::Fsize,
        "RLIMIT_DATA" => Resource::Data,
        "RLIMIT_STACK" => Resource::Stack,
        "RLIMIT_CORE" => Resource::Core,
        "RLIMIT_RSS" => Resource::Rss,
        "RLIMIT_NPROC" => Resource::Nproc,
        "RLIMIT_NOFILE" => Resource::Nofile,
        "RLIMIT_MEMLOCK" => Resource::Memlock,
        "RLIMIT_AS" => Resource::As,
        "RLIMIT_LOCKS" => Resource::Locks,
        "RLIMIT_SIGPENDING" => Resource::Sigpending,
        "RLIMIT_MSGQUEUE" => Resource::Msgqueue,
        "RLIMIT_NICE" => Resource::Nice,
        "RLIMIT_RTPRIO" => Resource::Rtprio,
        "RLIMIT_RTTIME" => Resource::Rttime,
        other => return Err(OciError::Config(format!("unknown rlimit {other:?}"))),
    };
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_cap_names_map_to_their_numbers_and_unknown_ones_dont() {
        assert_eq!(cap_bit("CAP_CHOWN"), Some(0));
        assert_eq!(cap_bit("CAP_NET_BIND_SERVICE"), Some(10));
        assert_eq!(cap_bit("CAP_CHECKPOINT_RESTORE"), Some(LAST_CAP));
        assert_eq!(cap_bit("CAP_MADE_UP"), None);
        assert_eq!(cap_bit("chown"), None);
    }

    #[test]
    fn a_set_is_the_or_of_its_names_bits() {
        let set = set_of(&[
            "CAP_CHOWN".to_owned(),
            "CAP_KILL".to_owned(),
            "CAP_MADE_UP".to_owned(),
        ]);
        assert_eq!(set.bits(), (1 << 0) | (1 << 5));
    }

    #[test]
    fn every_rlimit_name_maps_or_is_refused() {
        assert!(rlimit_resource("RLIMIT_NOFILE").is_ok());
        assert!(rlimit_resource("RLIMIT_AS").is_ok());
        assert!(rlimit_resource("RLIMIT_MADE_UP").is_err());
    }
}
