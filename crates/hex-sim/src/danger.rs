//! Where a dangerous workload may run again (S8; F7 in `docs/design/failure-isolation.md`, `hex-sim.md`
//! H42): by the kinds Hex places, G2's [`Class`]. A workload that trips a security signal
//! ([`hex_health::danger::trips`]) is stopped, and starts again only in a stronger kind on its node, or
//! is refused, typed.
//!
//! - An OCI container shares its host's kernel; a Firecracker or Shards microVM runs its own under a
//!   hypervisor (Agache et al., NSDI '20, FI[19]). So a tripped OCI container moves to a microVM: the
//!   first of [`STRONGER`] its node runs and that will hold it, its own need plus that kind's overhead
//!   in what the node has free once the container's share is given back. A GPU workload fits no
//!   microVM: neither takes one ([`Class::takes_gpus`]).
//! - Firecracker before Shards: Firecracker's isolation is published and measured (a minimal device
//!   model, NSDI '20, `docs/research/runtime/substrates-and-sandboxes.md` [SUB-11]); Shards' device
//!   model isn't public in the sources read (`docs/research/scheduler/agentic-workloads.md`, the
//!   device table). Their overheads don't decide it: Shards' is a 5 MiB stand-in for Firecracker's.
//! - A trip inside a microVM is refused: no kind Hex places isolates more strongly.

use hex_health::danger::{self, Signal};

use crate::model::{Class, Classes, Overheads, Resources};

/// The kinds a tripped OCI container may move to, in the order it tries them.
pub const STRONGER: [Class; 2] = [Class::Firecracker, Class::Shards];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Isolation {
    /// Not a trip: nothing to do.
    None,
    /// Stop it, and start it again only as `to`, on its node.
    Move { from: Class, to: Class },
    /// Stop it, and start it nowhere.
    Refuse { from: Class, why: Refusal },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// It tripped in a microVM: no kind isolates more strongly.
    InMicroVm,
    /// Its node runs no microVM.
    NoMicroVmOnNode,
    /// Its node runs a microVM that won't hold it: too little memory or CPU free, or GPUs it can't
    /// take.
    DoesNotFit,
}

/// Whether `node`'s room holds `need` run as `class`, its overhead counted.
fn holds(room: &Resources, need: &Resources, class: Class, overheads: Overheads) -> bool {
    if need.gpus > 0 && !class.takes_gpus() {
        return false;
    }
    let with = Resources {
        mem_bytes: overheads.of(class),
        ..Resources::default()
    };
    need.checked_add(&with).is_some_and(|n| room.holds(&n))
}

/// What to do with a workload running as `from` that raised `signal`, on a node running `classes`
/// with `free` unplaced, the workload's own need `need` (its kind's overhead not counted) and the
/// kinds' `overheads`.
pub fn isolate(
    from: Class,
    signal: Signal,
    classes: Classes,
    free: Resources,
    need: Resources,
    overheads: Overheads,
) -> Isolation {
    if !danger::trips(signal) {
        return Isolation::None;
    }
    if from != Class::Oci {
        return Isolation::Refuse {
            from,
            why: Refusal::InMicroVm,
        };
    }
    if !STRONGER.iter().any(|c| classes.has(*c)) {
        return Isolation::Refuse {
            from,
            why: Refusal::NoMicroVmOnNode,
        };
    }
    // Stopped, the container gives back its need and its own overhead.
    let own = Resources {
        mem_bytes: overheads.of(from),
        ..Resources::default()
    };
    let room = need
        .checked_add(&own)
        .and_then(|back| free.checked_add(&back));
    let to = room.and_then(|room| {
        STRONGER
            .into_iter()
            .find(|c| classes.has(*c) && holds(&room, &need, *c, overheads))
    });
    match to {
        Some(to) => Isolation::Move { from, to },
        None => Isolation::Refuse {
            from,
            why: Refusal::DoesNotFit,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1 << 20;

    fn overheads() -> Overheads {
        let mut o = Overheads::default();
        o.set(Class::Oci, 440_402);
        o.set(Class::Firecracker, 5 * MIB);
        o.set(Class::Shards, 5 * MIB);
        o
    }

    fn res(cpu_milli: u64, mem_mib: u64, gpus: u32) -> Resources {
        Resources {
            cpu_milli,
            mem_bytes: mem_mib * MIB,
            gpus,
        }
    }

    fn only(classes: &[Class]) -> Classes {
        classes.iter().fold(Classes::NONE, |c, k| c.with(*k))
    }

    /// S8 dangerous (F7): a tripped OCI container moves to Firecracker where its node runs it, to
    /// Shards where the node runs Shards alone; never to a kind its node doesn't run.
    #[test]
    fn an_oci_trip_moves_to_the_first_microvm_its_node_runs() {
        let need = res(500, 512, 0);
        for s in [
            Signal::SeccompKill,
            Signal::EgressRefused,
            Signal::PrivilegeEscalation,
        ] {
            let iso = |classes| {
                isolate(
                    Class::Oci,
                    s,
                    classes,
                    res(4000, 4096, 0),
                    need,
                    overheads(),
                )
            };
            assert_eq!(
                iso(Classes::ALL),
                Isolation::Move {
                    from: Class::Oci,
                    to: Class::Firecracker
                }
            );
            assert_eq!(
                iso(only(&[Class::Oci, Class::Shards])),
                Isolation::Move {
                    from: Class::Oci,
                    to: Class::Shards
                }
            );
        }
    }

    /// The move counts the container's share given back and the microVM's overhead: with exactly its
    /// need plus 5 MiB less the 0.42 MiB it gives back free, it fits; a byte less, it doesn't.
    #[test]
    fn the_move_counts_both_overheads() {
        let need = res(500, 512, 0);
        let exact = Resources {
            cpu_milli: 0,
            mem_bytes: 5 * MIB - 440_402,
            gpus: 0,
        };
        let iso = |free| {
            isolate(
                Class::Oci,
                Signal::SeccompKill,
                Classes::ALL,
                free,
                need,
                overheads(),
            )
        };
        assert_eq!(
            iso(exact),
            Isolation::Move {
                from: Class::Oci,
                to: Class::Firecracker
            }
        );
        let short = Resources {
            mem_bytes: exact.mem_bytes - 1,
            ..exact
        };
        assert_eq!(
            iso(short),
            Isolation::Refuse {
                from: Class::Oci,
                why: Refusal::DoesNotFit
            }
        );
    }

    /// Refusals, typed: a trip in either microVM; a node running no microVM; a GPU workload, which no
    /// microVM takes.
    #[test]
    fn a_trip_with_no_stronger_kind_that_holds_it_is_refused() {
        let free = res(4000, 4096, 1);
        let need = res(500, 512, 0);
        for from in [Class::Firecracker, Class::Shards] {
            assert_eq!(
                isolate(
                    from,
                    Signal::SeccompKill,
                    Classes::ALL,
                    free,
                    need,
                    overheads()
                ),
                Isolation::Refuse {
                    from,
                    why: Refusal::InMicroVm
                }
            );
        }
        assert_eq!(
            isolate(
                Class::Oci,
                Signal::EgressRefused,
                only(&[Class::Oci]),
                free,
                need,
                overheads()
            ),
            Isolation::Refuse {
                from: Class::Oci,
                why: Refusal::NoMicroVmOnNode
            }
        );
        assert_eq!(
            isolate(
                Class::Oci,
                Signal::PrivilegeEscalation,
                Classes::ALL,
                free,
                res(500, 512, 1),
                overheads()
            ),
            Isolation::Refuse {
                from: Class::Oci,
                why: Refusal::DoesNotFit
            }
        );
    }

    /// An errno'd call is not a trip, in any kind.
    #[test]
    fn an_errno_is_not_a_trip() {
        for from in [Class::Oci, Class::Firecracker, Class::Shards] {
            assert_eq!(
                isolate(
                    from,
                    Signal::SeccompErrno,
                    Classes::ALL,
                    res(0, 0, 0),
                    res(1, 1, 0),
                    overheads()
                ),
                Isolation::None
            );
        }
    }
}
