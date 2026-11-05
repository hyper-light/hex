//! Dangerous workloads (S8): one that trips a runtime security signal is isolated by runtime class.
//! It is stopped where it is, and starts again only in a class that isolates more strongly than the
//! one it tripped in, or is refused, typed: never merely deprioritised.
//!
//! The classes in order of isolation: a process box (host processes in a resource bound), an OCI
//! container (namespaces, cgroups and seccomp over the host's one kernel) and a microVM (its own kernel
//! under a hypervisor). Containers share the host kernel; a microVM does not (Firecracker, NSDI '20;
//! gVisor's per-sandbox kernel makes the same point).
//!
//! What trips: a call the workload's seccomp profile kills for (`SECCOMP_RET_KILL_PROCESS` or
//! `_THREAD`, which seccomp(2) reports to the audit log), an egress attempt no grant names (S3,
//! `AC-T7.7.2`), and an attempt to gain privilege (a capability or set-id change the box refused). A
//! call failed with an errno (`SECCOMP_RET_ERRNO`) does not: profiles fail calls they don't support that
//! way and well-behaved programs fall back, so it says nothing about intent.

/// A runtime class, weakest isolation first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Process,
    Oci,
    MicroVm,
}

/// A runtime security signal from a workload's box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// The seccomp profile killed it for a call.
    SeccompKill,
    /// The seccomp profile failed a call with an errno.
    SeccompErrno,
    /// It tried to reach a destination no grant names.
    EgressRefused,
    /// It tried to gain a capability or change identity, and was refused.
    PrivilegeEscalation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Isolation {
    /// Not a trip: nothing to do.
    None,
    /// Stop it, and start it again only in `to`.
    Move { from: Class, to: Class },
    /// Stop it, and start it nowhere.
    Refuse { class: Class, why: RefuseReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefuseReason {
    /// No node of the cell offers a class stronger than the one it tripped in.
    NoStrongerClass,
}

/// What to do with a workload running in `class` that raised `signal`, where the cell offers the
/// classes in `offered`: the weakest offered class stronger than `class`, or a refusal.
pub fn isolate(class: Class, signal: Signal, offered: &[Class]) -> Isolation {
    if signal == Signal::SeccompErrno {
        return Isolation::None;
    }
    match offered.iter().copied().filter(|c| *c > class).min() {
        Some(to) => Isolation::Move { from: class, to },
        None => Isolation::Refuse {
            class,
            why: RefuseReason::NoStrongerClass,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Class; 3] = [Class::Process, Class::Oci, Class::MicroVm];

    /// S8 dangerous: each trip moves the workload one class up, to the weakest stronger one offered.
    #[test]
    fn a_trip_moves_the_workload_to_a_stronger_class() {
        for s in [
            Signal::SeccompKill,
            Signal::EgressRefused,
            Signal::PrivilegeEscalation,
        ] {
            assert_eq!(
                isolate(Class::Process, s, &ALL),
                Isolation::Move {
                    from: Class::Process,
                    to: Class::Oci
                }
            );
            assert_eq!(
                isolate(Class::Oci, s, &ALL),
                Isolation::Move {
                    from: Class::Oci,
                    to: Class::MicroVm
                }
            );
            assert_eq!(
                isolate(Class::Process, s, &[Class::Process, Class::MicroVm]),
                Isolation::Move {
                    from: Class::Process,
                    to: Class::MicroVm
                }
            );
        }
    }

    /// S8 dangerous: a trip in the strongest class, or where nothing stronger is offered, is refused.
    #[test]
    fn a_trip_with_nothing_stronger_is_refused() {
        let refused = |class| Isolation::Refuse {
            class,
            why: RefuseReason::NoStrongerClass,
        };
        assert_eq!(
            isolate(Class::MicroVm, Signal::SeccompKill, &ALL),
            refused(Class::MicroVm)
        );
        assert_eq!(
            isolate(
                Class::Oci,
                Signal::EgressRefused,
                &[Class::Process, Class::Oci]
            ),
            refused(Class::Oci)
        );
        assert_eq!(
            isolate(Class::Oci, Signal::EgressRefused, &[]),
            refused(Class::Oci)
        );
    }

    /// An errno'd call is not a trip.
    #[test]
    fn an_errno_is_not_a_trip() {
        assert_eq!(
            isolate(Class::Oci, Signal::SeccompErrno, &ALL),
            Isolation::None
        );
    }
}
