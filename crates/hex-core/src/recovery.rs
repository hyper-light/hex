//! After the node agent restarts: which boxes to take back and which to remove (T-T7.1.2). A box is
//! taken back only when its record says it was made and the machine still holds it; everything else
//! the agent finds of a box (a record mid-create or mid-delete, a record it can't read, a cgroup or
//! directory without a record, a container whose init is gone) is removed, so nothing is leaked.

use std::collections::BTreeMap;

use hex_api::{BoxId, Class};

use crate::record::{BoxRecord, Phase};

/// What the node agent found of one box.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seen {
    /// Its record: `Err` holds why the file there couldn't be read as one.
    pub record: Option<Result<BoxRecord, String>>,
    /// Its cgroup, if one exists.
    pub cgroup: bool,
    /// For a record with an init: whether that very process still runs.
    pub init_alive: Option<bool>,
}

/// What to do with a box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Take it back as it is.
    Adopt,
    /// End what runs in it and remove it, with what's left of it.
    Remove(Why),
}

/// Why a box is removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// The agent stopped while making it.
    MidCreate,
    /// The agent stopped while removing it.
    MidDelete,
    /// Its container's init has ended: the container is gone.
    InitGone,
    /// Its record exists but its cgroup doesn't: something removed it (its unit was stopped).
    NoCgroup,
    /// Its cgroup exists but no record of it does.
    NoRecord,
    /// Its record can't be read, for this reason.
    Unreadable(String),
}

/// The decision for one box.
pub fn decide(seen: &Seen) -> Decision {
    match &seen.record {
        None => Decision::Remove(Why::NoRecord),
        Some(Err(why)) => Decision::Remove(Why::Unreadable(why.clone())),
        Some(Ok(r)) => match r.phase {
            Phase::Creating => Decision::Remove(Why::MidCreate),
            Phase::Deleting => Decision::Remove(Why::MidDelete),
            Phase::Ready if !seen.cgroup => Decision::Remove(Why::NoCgroup),
            Phase::Ready if r.spec.class.class() == Class::Oci && seen.init_alive != Some(true) => {
                Decision::Remove(Why::InitGone)
            }
            Phase::Ready => Decision::Adopt,
        },
    }
}

/// Decisions for every box found, in id order.
pub fn plan(seen: &BTreeMap<BoxId, Seen>) -> Vec<(BoxId, Decision)> {
    seen.iter().map(|(id, s)| (id.clone(), decide(s))).collect()
}

#[cfg(test)]
mod tests {
    //! A model of the machine (records, cgroups, directories, inits) with the node agent's steps
    //! applied to it, crashed after every step of every generated history, then recovered: crash
    //! injection at every write (CLAUDE.md §6). After recovery every box left is whole and taken
    //! back, every box that was whole and not being deleted survives, and nothing else is left.

    use std::collections::{BTreeMap, BTreeSet};

    use hex_api::{Bounds, BoxSpec, ClassSpec, Network, OciSpec, Program, Rootfs};
    use proptest::prelude::*;

    use super::*;
    use crate::record::{DELETE_STEPS, ProcId, REMOVE_STEPS, Step, create_steps};

    #[derive(Debug, Clone, Default)]
    struct Machine {
        records: BTreeMap<BoxId, BoxRecord>,
        cgroups: BTreeSet<BoxId>,
        dirs: BTreeSet<BoxId>,
        inits: BTreeSet<BoxId>,
        steps: usize,
    }

    #[derive(Debug, Clone, Copy)]
    enum Op {
        Create(u8, Class),
        Delete(u8),
        /// A container's init exits on its own (the box stays until deleted).
        InitExits(u8),
    }

    fn id(n: u8) -> BoxId {
        BoxId::new(&format!("b{n}")).unwrap()
    }

    fn spec(id: &BoxId, class: Class) -> BoxSpec {
        let class = match class {
            Class::Process => ClassSpec::Process,
            Class::Oci => ClassSpec::Oci(Box::new(OciSpec {
                rootfs: Rootfs::Layers(vec![]),
                init: Program::of(&["sleep", "infinity"]),
                mounts: vec![],
                hostname: None,
                network: Network::Host,
                hosts: vec![],
            })),
        };
        BoxSpec {
            id: id.clone(),
            bounds: Bounds::default(),
            class,
        }
    }

    /// Applies one step; `false` when the crash budget ran out first.
    fn apply(m: &mut Machine, budget: &mut usize, id: &BoxId, spec: &BoxSpec, step: Step) -> bool {
        if *budget == 0 {
            return false;
        }
        *budget -= 1;
        m.steps += 1;
        match step {
            Step::MarkCreating => {
                m.records.insert(
                    id.clone(),
                    BoxRecord::new(spec.clone(), Phase::Creating, vec![]),
                );
            }
            Step::MakeCgroup => {
                m.cgroups.insert(id.clone());
            }
            Step::MakeDirs => {
                m.dirs.insert(id.clone());
            }
            Step::StartInit => {
                m.inits.insert(id.clone());
            }
            Step::MarkReady => {
                if let Some(r) = m.records.get_mut(id) {
                    r.phase = Phase::Ready;
                    if m.inits.contains(id) {
                        r.init = Some(ProcId {
                            pid: 1,
                            start_ticks: 1,
                        });
                    }
                }
            }
            Step::MarkDeleting => {
                if let Some(r) = m.records.get_mut(id) {
                    r.phase = Phase::Deleting;
                }
            }
            Step::KillAll => {
                m.inits.remove(id);
            }
            Step::RemoveCgroup => {
                m.cgroups.remove(id);
            }
            Step::RemoveDirs => {
                m.dirs.remove(id);
            }
            Step::DropRecord => {
                m.records.remove(id);
            }
        }
        true
    }

    /// Runs `ops` until the budget runs out; returns the boxes that were whole and not being
    /// deleted when it did.
    fn run(m: &mut Machine, ops: &[Op], mut budget: usize) {
        for op in ops {
            match *op {
                Op::Create(n, class) => {
                    let id = id(n);
                    if m.records.contains_key(&id) {
                        continue;
                    }
                    let spec = spec(&id, class);
                    for s in create_steps(class) {
                        if !apply(m, &mut budget, &id, &spec, *s) {
                            return;
                        }
                    }
                }
                Op::Delete(n) => {
                    let id = id(n);
                    let Some(r) = m.records.get(&id) else {
                        continue;
                    };
                    let spec = r.spec.clone();
                    for s in DELETE_STEPS {
                        if !apply(m, &mut budget, &id, &spec, *s) {
                            return;
                        }
                    }
                }
                Op::InitExits(n) => {
                    m.inits.remove(&id(n));
                }
            }
        }
    }

    fn whole(m: &Machine) -> BTreeSet<BoxId> {
        m.records
            .iter()
            .filter(|(id, r)| {
                r.phase == Phase::Ready
                    && m.cgroups.contains(*id)
                    && (r.spec.class.class() == Class::Process
                        || (m.inits.contains(*id) && m.dirs.contains(*id)))
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    fn recover(m: &mut Machine) {
        let mut seen: BTreeMap<BoxId, Seen> = BTreeMap::new();
        for (id, r) in &m.records {
            let init_alive = r.init.map(|_| m.inits.contains(id));
            seen.insert(
                id.clone(),
                Seen {
                    record: Some(Ok(r.clone())),
                    cgroup: m.cgroups.contains(id),
                    init_alive,
                },
            );
        }
        for id in m.cgroups.iter().chain(&m.dirs).chain(&m.inits) {
            seen.entry(id.clone()).or_insert(Seen {
                record: None,
                cgroup: m.cgroups.contains(id),
                init_alive: None,
            });
        }
        for (id, d) in plan(&seen) {
            if let Decision::Remove(_) = d {
                let spec = spec(&id, Class::Process);
                let mut unlimited = usize::MAX;
                for s in REMOVE_STEPS {
                    apply(m, &mut unlimited, &id, &spec, *s);
                }
            }
        }
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            (0u8..4, prop_oneof![Just(Class::Process), Just(Class::Oci)])
                .prop_map(|(n, c)| Op::Create(n, c)),
            (0u8..4).prop_map(Op::Delete),
            (0u8..4).prop_map(Op::InitExits),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

        #[test]
        fn a_crash_after_any_step_leaves_only_whole_boxes_and_keeps_every_whole_one(ops in proptest::collection::vec(op(), 1..12)) {
            let mut full = Machine::default();
            run(&mut full, &ops, usize::MAX);
            let total = full.steps;
            // Non-vacuity: the histories do make and remove boxes.
            prop_assume!(total > 0);
            for crash_after in 0..=total {
                let mut m = Machine::default();
                run(&mut m, &ops, crash_after);
                let survivors = whole(&m);
                recover(&mut m);
                let left: BTreeSet<BoxId> = m.records.keys().cloned().collect();
                prop_assert_eq!(&left, &survivors, "crash after step {}", crash_after);
                prop_assert_eq!(whole(&m), survivors.clone());
                let leaked: Vec<&BoxId> = m.cgroups.iter().chain(&m.dirs).chain(&m.inits).filter(|id| !survivors.contains(*id)).collect();
                prop_assert!(leaked.is_empty(), "leaked {:?} after a crash at step {}", leaked, crash_after);
            }
        }
    }

    #[test]
    fn each_finding_gets_its_decision() {
        let r = |phase, class| {
            let mut r = BoxRecord::new(spec(&id(0), class), phase, vec![]);
            r.init = match class {
                Class::Oci => Some(ProcId {
                    pid: 7,
                    start_ticks: 9,
                }),
                Class::Process => None,
            };
            Some(Ok(r))
        };
        let cases = [
            (
                Seen {
                    record: None,
                    cgroup: true,
                    init_alive: None,
                },
                Decision::Remove(Why::NoRecord),
            ),
            (
                Seen {
                    record: Some(Err("bad".into())),
                    cgroup: true,
                    init_alive: None,
                },
                Decision::Remove(Why::Unreadable("bad".into())),
            ),
            (
                Seen {
                    record: r(Phase::Creating, Class::Process),
                    cgroup: true,
                    init_alive: None,
                },
                Decision::Remove(Why::MidCreate),
            ),
            (
                Seen {
                    record: r(Phase::Deleting, Class::Process),
                    cgroup: true,
                    init_alive: None,
                },
                Decision::Remove(Why::MidDelete),
            ),
            (
                Seen {
                    record: r(Phase::Ready, Class::Process),
                    cgroup: false,
                    init_alive: None,
                },
                Decision::Remove(Why::NoCgroup),
            ),
            (
                Seen {
                    record: r(Phase::Ready, Class::Oci),
                    cgroup: true,
                    init_alive: Some(false),
                },
                Decision::Remove(Why::InitGone),
            ),
            (
                Seen {
                    record: r(Phase::Ready, Class::Oci),
                    cgroup: true,
                    init_alive: Some(true),
                },
                Decision::Adopt,
            ),
            (
                Seen {
                    record: r(Phase::Ready, Class::Process),
                    cgroup: true,
                    init_alive: None,
                },
                Decision::Adopt,
            ),
        ];
        for (seen, want) in cases {
            assert_eq!(decide(&seen), want, "{seen:?}");
        }
    }
}
