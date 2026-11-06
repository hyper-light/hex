//! How a pruned create's mount namespace is made (decision 6's update, `docs/research/runtime/goal6-mntns.md`):
//! the seed's pruned namespace copied by the init's clone (goal 5's way, every kernel), a namespace holding only
//! its root made by that clone (`CLONE_EMPTY_MNTNS`, Linux 7.1), or one the agent makes from the rootfs's tree
//! (`OPEN_TREE_NAMESPACE`, Linux 7.0) for the init to join. A node chooses one way at start: the one the record
//! shows fastest, where the kernel has it ([`default_way`]), or, for the bench's A/B and the lab, the one its builder
//! is asked for (`Node::with_mount_namespace`). The node agent reads no environment for it (Ada's 2026-11-06 rule).

use super::OciError;
pub use hex_core::seed::MountNamespace;
use rustix::io::Errno;

/// `open_tree`'s flag that clones the tree into a new mount namespace and returns that namespace
/// (`include/uapi/linux/mount.h:65`, Linux 7.0), which rustix doesn't name.
pub const OPEN_TREE_NAMESPACE: u32 = 1 << 1;
/// `fsmount`'s flag that makes the new mount a new mount namespace's tree (`include/uapi/linux/mount.h:113`,
/// Linux 7.1).
pub const FSMOUNT_NAMESPACE: u32 = 0x2;
/// `clone3`'s flag for a mount namespace holding only a clone of its parent's root mount
/// (`include/uapi/linux/sched.h:42`, Linux 7.1).
pub const CLONE_EMPTY_MNTNS: u64 = 1 << 37;

/// Whether this process may make a mount namespace from a tree, as [`probe`] finds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenTreeNamespace {
    /// The kernel has the call and this process may make one.
    Available,
    /// The kernel has the call, but this process lacks `CAP_SYS_ADMIN` in its user namespace.
    NotPermitted,
    /// The kernel predates it (Linux 7.0).
    Absent,
}

/// What [`probe`]'s `open_tree` answering `result` means. The probe names a path beneath a pipe's
/// descriptor, which is no directory: a kernel without the flag refuses it at its flag check, `EINVAL`
/// (v6.19 `fs/namespace.c:3073-3076`); one with it checks the capability, `EPERM`, then fails the lookup,
/// `ENOTDIR` (`path_init`), before it makes anything (v7.1 `:3219-3232`). Any other answer is a kernel this
/// reading doesn't cover, refused by type.
pub fn classify(result: Result<(), Errno>) -> Result<OpenTreeNamespace, OciError> {
    match result {
        Err(Errno::NOTDIR) => Ok(OpenTreeNamespace::Available),
        Err(Errno::PERM) => Ok(OpenTreeNamespace::NotPermitted),
        Err(Errno::INVAL) => Ok(OpenTreeNamespace::Absent),
        Err(e) => Err(OciError::os(
            "probing open_tree(OPEN_TREE_NAMESPACE)",
            format!("an answer that means neither support nor its absence: {e}"),
        )),
        Ok(()) => Err(OciError::os(
            "probing open_tree(OPEN_TREE_NAMESPACE)",
            "a namespace made from no tree",
        )),
    }
}

/// Asks the kernel whether this process may make a mount namespace from a tree, making nothing and taking
/// no lock ([`classify`] says how).
pub fn probe() -> Result<OpenTreeNamespace, OciError> {
    let (not_a_dir, _w) = rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC)
        .map_err(|e| OciError::os("probing open_tree(OPEN_TREE_NAMESPACE)", e))?;
    let flags = rustix::mount::OpenTreeFlags::from_bits_retain(
        OPEN_TREE_NAMESPACE | rustix::mount::OpenTreeFlags::OPEN_TREE_CLOEXEC.bits(),
    );
    classify(rustix::mount::open_tree(&not_a_dir, "probe", flags).map(drop))
}

/// The `clone3` namespace flags of an init whose mount namespace is made `way`, from the spec's `flags`, which hold
/// `CLONE_NEWNS` (the seed's namespace copied): from its tree, the init is cloned without it and joins the namespace
/// the agent made; empty, `CLONE_EMPTY_MNTNS` takes its place. What a fresh clone takes ([`super::seed`]'s
/// `clone_pruned`), and so what an init parked for that way is cloned with and a claim is held to (decision 8).
pub fn clone_flags(flags: u64, way: MountNamespace) -> u64 {
    let newns = u64::try_from(libc::CLONE_NEWNS).unwrap_or(0);
    match way {
        MountNamespace::FromTree => flags & !newns,
        MountNamespace::Empty => (flags & !newns) | CLONE_EMPTY_MNTNS,
        MountNamespace::Copy => flags,
    }
}

/// The way a node takes unless asked for another: from its tree where this process may make one, else the
/// copy. `Empty` makes the same holds of the mount lock as the copy (`goal6-mntns.md` §4), so it isn't the
/// default until a 7.1 host's record shows it faster.
pub fn default_way(open_tree: OpenTreeNamespace) -> MountNamespace {
    match open_tree {
        OpenTreeNamespace::Available => MountNamespace::FromTree,
        OpenTreeNamespace::NotPermitted | OpenTreeNamespace::Absent => MountNamespace::Copy,
    }
}

/// The way a node takes: `asked` (`copy`, `empty` or `tree`) where given, else [`default_way`]. A namespace
/// from its tree asked of a kernel or process that can't make one is refused by type. Empty asked of a
/// kernel without `CLONE_EMPTY_MNTNS` isn't: the seed learns that from its first clone, makes the copy and
/// says so in its answer, and the node counts the create as copied.
pub fn choose(
    open_tree: OpenTreeNamespace,
    asked: Option<&str>,
) -> Result<MountNamespace, OciError> {
    match asked {
        None => Ok(default_way(open_tree)),
        Some("copy") => Ok(MountNamespace::Copy),
        Some("empty") => Ok(MountNamespace::Empty),
        Some("tree") if open_tree == OpenTreeNamespace::Available => Ok(MountNamespace::FromTree),
        Some("tree") => Err(OciError::Config(format!(
            "the way asked, tree, is a mount namespace made from its tree, which this node can't make \
             ({open_tree:?})"
        ))),
        Some(other) => Err(OciError::Config(format!(
            "the way asked, {other:?}, names no way to make a mount namespace: copy, empty or tree"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HEX-MNTNS-1: do the probe's `open_tree` and get each answer the kernel gives; expect ENOTDIR to
    /// mean the call is there, EPERM that this process may not, EINVAL that the kernel predates it, and
    /// any other answer refused by type rather than read as either.
    #[test]
    fn each_answer_to_the_probe_means_what_the_kernel_says() {
        assert_eq!(
            classify(Err(Errno::NOTDIR)).unwrap(),
            OpenTreeNamespace::Available
        );
        assert_eq!(
            classify(Err(Errno::PERM)).unwrap(),
            OpenTreeNamespace::NotPermitted
        );
        assert_eq!(
            classify(Err(Errno::INVAL)).unwrap(),
            OpenTreeNamespace::Absent
        );
        for other in [Errno::BADF, Errno::NOSYS, Errno::NOENT, Errno::NOMEM] {
            assert!(classify(Err(other)).is_err(), "{other:?} is refused");
        }
        assert!(classify(Ok(())).is_err());
    }

    /// HEX-MNTNS-1: choose with and without a way asked, on each probe's answer; expect from its tree by
    /// default where it can be made and the copy otherwise, each word taken where it can be served, the
    /// tree refused by type where it can't, and an unknown word refused.
    #[test]
    fn a_node_takes_the_way_asked_where_it_can_and_else_its_default() {
        use OpenTreeNamespace::*;
        assert_eq!(choose(Available, None).unwrap(), MountNamespace::FromTree);
        assert_eq!(choose(NotPermitted, None).unwrap(), MountNamespace::Copy);
        assert_eq!(choose(Absent, None).unwrap(), MountNamespace::Copy);
        for probe in [Available, NotPermitted, Absent] {
            assert_eq!(choose(probe, Some("copy")).unwrap(), MountNamespace::Copy);
            assert_eq!(choose(probe, Some("empty")).unwrap(), MountNamespace::Empty);
            assert!(choose(probe, Some("Tree")).is_err());
            assert!(choose(probe, Some("")).is_err());
        }
        assert_eq!(
            choose(Available, Some("tree")).unwrap(),
            MountNamespace::FromTree
        );
        assert!(matches!(
            choose(Absent, Some("tree")),
            Err(OciError::Config(_))
        ));
        assert!(matches!(
            choose(NotPermitted, Some("tree")),
            Err(OciError::Config(_))
        ));
    }
}

#[cfg(test)]
mod probe_tests {
    use super::*;

    /// HEX-MNTNS-1: probe this kernel; expect an answer it classifies (never a refusal), and on a kernel
    /// below 7.0 (fedora's 6.19) that the call is absent, read from its release.
    #[test]
    fn the_probe_reads_this_kernel() {
        let found = probe().unwrap();
        let release = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap();
        let major: u32 = release.split('.').next().unwrap().parse().unwrap();
        if major < 7 {
            assert_eq!(found, OpenTreeNamespace::Absent, "{release}");
        }
        eprintln!("kernel {} probe {found:?}", release.trim());
    }
}
