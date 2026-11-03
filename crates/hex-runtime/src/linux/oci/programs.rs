//! The device programs a node agent holds (goal 5 extended, step 1): decision 2's sharing, with the
//! agent as the holder that outlives its containers. Each policy's program is loaded once and kept open
//! ([`devshare::Programs`]), so a create attaches it with no load (research f60). On a root node the
//! default policy's program is attached once, at the node's start, to the root its oci boxes alone live
//! under, with `BPF_F_ALLOW_OVERRIDE`: a box with the default policy attaches nothing, and a box with
//! another attaches its own, which then decides for it in the root's place (f40-f42). A rootless node
//! loads nothing: loading needs CAP_BPF in the initial user namespace, so its containers are held to
//! their device lists by construction, as before (`run::attach_device_program`).

use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rustix::fs::{Mode, OFlags};

use crate::linux::cgroup::BoxCgroup;
use crate::linux::oci::devshare::{self, Give};
use crate::linux::oci::resources::{self, Insn, Plan};
use crate::linux::oci::{OciError, sys};

/// What a node's device programs have done since it opened them: what tests and the bench read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    /// Programs loaded for boxes; the root's is [`Counts::root_attached`]'s.
    pub loaded: u64,
    /// Programs attached to boxes' cgroups.
    pub attached: u64,
    /// Programs closed past the bound.
    pub closed: u64,
    /// Programs held open now.
    pub held: usize,
    /// Whether the node's start loaded the default policy's program and attached it to the oci boxes'
    /// root: `false` where the root carried it already (an agent restarted over its boxes), and on a
    /// rootless node.
    pub root_attached: bool,
}

/// A node agent's device programs: those it holds open, one per policy, and the policy whose program its
/// oci boxes' root carries.
#[derive(Debug)]
pub struct Programs {
    held: Mutex<devshare::Programs<Arc<OwnedFd>>>,
    /// The default policy's instructions, when the oci boxes' root carries their program.
    root: Option<Vec<Insn>>,
    root_attached: bool,
    loaded: AtomicU64,
    attached: AtomicU64,
    closed: AtomicU64,
}

impl Programs {
    /// A rootless node's: no program at its root, and none loaded.
    pub fn rootless(bound: usize) -> Self {
        Self::holding(bound, None, false)
    }

    /// A root node's: the default policy's program on `root`, the root its oci boxes alone live under,
    /// attached with `BPF_F_ALLOW_OVERRIDE` unless the root carries it already, and at most `bound`
    /// programs held for boxes. Which program the root carries is asked once, here, since the query
    /// takes the kernel's `cgroup_mutex` (research f61). One program attached with OVERRIDE whose
    /// translation is the default's is kept, so an agent restarted over its boxes leaves their root's
    /// program as it was. Anything else is replaced by one attach (include/uapi/linux/bpf.h: an OVERRIDE
    /// attach "will release old program and attach the new one"), or, where the kernel won't (programs
    /// attached with other flags, which Hex never attaches there), refused by name.
    pub fn at_root(root: &Path, bound: usize) -> Result<Self, OciError> {
        let default = resources::default_program()?;
        let dir = rustix::fs::open(
            root,
            OFlags::DIRECTORY | OFlags::RDONLY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| {
            OciError::os(
                "opening the oci boxes' root",
                format!("{}: {e}", root.display()),
            )
        })?;
        let there = sys::attached_device_programs(dir.as_fd())?;
        let kept = there.count == 1
            && there.flags == sys::BPF_F_ALLOW_OVERRIDE
            && there
                .ids
                .first()
                .is_some_and(|id| program_for(*id, &default).is_some());
        if !kept {
            let prog = sys::load_device_program(&default)?;
            sys::attach_device_program(dir.as_fd(), prog.as_fd(), sys::Attach::Override).map_err(
                |e| {
                    OciError::os(
                        "attaching the default device program to the oci boxes' root",
                        format!(
                            "{}: {e} (it holds {} program(s), attached with flags {:#x})",
                            root.display(),
                            there.count,
                            there.flags
                        ),
                    )
                },
            )?;
            // `prog` closes here: the root's attachment holds the program (research f53).
        }
        Ok(Self::holding(bound, Some(default), !kept))
    }

    fn holding(bound: usize, root: Option<Vec<Insn>>, root_attached: bool) -> Self {
        Self {
            held: Mutex::new(devshare::Programs::new(bound)),
            root,
            root_attached,
            loaded: AtomicU64::new(0),
            attached: AtomicU64::new(0),
            closed: AtomicU64::new(0),
        }
    }

    /// The program `plan` asks a box's own cgroup to carry on a create through the seam
    /// ([`devshare::give`]): `None` for the root's policy (the root's program decides) or, with no root
    /// program, for a policy that allows every device; else the policy's program. A container in a user
    /// namespace of its own is held to its device list by construction and refused an allow-by-default
    /// list with denials, as before ([`Plan::check_in_userns`]); under the root's program it carries the
    /// allow-all program, so the root's default doesn't decide for it in construction's place. What a
    /// cgroup handed on must carry already for a box to be given it (goal 5 extended, step 6).
    pub fn wants(&self, plan: &Plan, in_userns: bool) -> Result<Option<Vec<Insn>>, OciError> {
        let policy = if in_userns {
            plan.check_in_userns()?;
            None
        } else {
            plan.devices.as_deref()
        };
        Ok(match devshare::give(policy, self.root.as_deref()) {
            Give::Nothing => None,
            Give::Program(insns) => Some(insns),
        })
    }

    /// Gives box cgroup `cg` the device program `plan` asks for ([`Programs::wants`]): the program held
    /// for the policy, loaded first only when none is.
    pub fn attach(&self, cg: &BoxCgroup, plan: &Plan, in_userns: bool) -> Result<(), OciError> {
        match self.wants(plan, in_userns)? {
            Some(insns) => self.attach_program(cg, insns),
            None => Ok(()),
        }
    }

    /// Attaches the program held for `insns` to box cgroup `cg`, loading it first only when none is.
    pub fn attach_program(&self, cg: &BoxCgroup, insns: Vec<Insn>) -> Result<(), OciError> {
        let prog = self.program(insns)?;
        let dir = cg
            .open_dir()
            .map_err(|e| OciError::os("opening a box's cgroup", e))?;
        sys::attach_device_program(dir.as_fd(), prog.as_fd(), sys::Attach::Multi)?;
        self.attached.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// The program held for `insns`, loaded and held now if none is. It's loaded with the lock held, so a
    /// policy many creates ask for at once is loaded once; a policy's first create is the only one that
    /// waits on the verifier, and the others wait for it only that once.
    fn program(&self, insns: Vec<Insn>) -> Result<Arc<OwnedFd>, OciError> {
        let mut held = self.held.lock().map_err(|_| {
            OciError::Broken("the node's device programs' lock was poisoned".to_owned())
        })?;
        if let Some(prog) = held.get(&insns) {
            return Ok(Arc::clone(prog));
        }
        let prog = Arc::new(sys::load_device_program(&insns)?);
        self.loaded.fetch_add(1, Ordering::Relaxed);
        // What the bound puts out closes as it drops, once no create still attaching holds it; a program
        // attached to a box lives on there (research f53).
        for _ in held.put(insns, Arc::clone(&prog)) {
            self.closed.fetch_add(1, Ordering::Relaxed);
        }
        Ok(prog)
    }

    pub fn counts(&self) -> Result<Counts, OciError> {
        let held = self
            .held
            .lock()
            .map_err(|_| {
                OciError::Broken("the node's device programs' lock was poisoned".to_owned())
            })?
            .len();
        Ok(Counts {
            loaded: self.loaded.load(Ordering::Relaxed),
            attached: self.attached.load(Ordering::Relaxed),
            closed: self.closed.load(Ordering::Relaxed),
            held,
            root_attached: self.root_attached,
        })
    }
}

/// Loaded program `id`, open, if it's the program for `insns`: a cgroup device program whose translated
/// instructions are `insns` byte for byte ([`devshare::is_policy`]). The open descriptor holds it from
/// that check to its attach, so an id the kernel gives another program meanwhile (ids are reused,
/// research f53) can't be what's attached. One gone, or one that can't be opened or read, is `None`.
pub fn program_for(id: u32, insns: &[Insn]) -> Option<OwnedFd> {
    let prog = sys::program_by_id(id).ok()??;
    let identity = sys::program_identity(prog.as_fd()).ok()?;
    let translation = sys::program_instructions(prog.as_fd(), identity.xlated_len).ok()?;
    if devshare::is_policy(&identity, translation.as_deref(), insns) {
        Some(prog)
    } else {
        None
    }
}
