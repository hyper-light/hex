//! The control plane's durable state: one record per workload and each node's used capacity,
//! changed only by compare-and-set writes (`scheduler-architecture.md` S6, Omega's optimistic commit
//! CM[2]). A placement names the record version it read, so a commit made twice, or made from a
//! stale read, is a conflict and never a second start. A write is atomic: the nemesis crashes the
//! control plane before one is applied or after it is applied and before anyone hears of it.
//!
//! The store outlives control-plane crashes, as Amphora does for the real control plane. Its log of
//! applied writes is the history the serial specification checks ([`crate::spec`]).

use std::collections::BTreeMap;

use crate::error::SimError;
use crate::model::{NodeId, NodeSpec, Resources, Time, WorkloadId, WorkloadSpec};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Pending,
    Placed(NodeId),
    Complete(NodeId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    pub spec: WorkloadSpec,
    /// The order the store accepted it in: the serial specification's queue order.
    pub seq: u64,
    pub version: u64,
    pub state: State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Write {
    Submit(WorkloadSpec),
    Place {
        workload: WorkloadId,
        node: NodeId,
        version: u64,
    },
    Complete {
        workload: WorkloadId,
        node: NodeId,
        version: u64,
    },
}

/// An applied write, as the log keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Submit { workload: WorkloadId, seq: u64 },
    Place { workload: WorkloadId, node: NodeId },
    Complete { workload: WorkloadId, node: NodeId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub at: Time,
    pub op: Op,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conflict {
    /// The record changed since the writer read it.
    Version { read: u64, found: u64 },
    /// The record isn't in the state the write moves it from.
    State(State),
    /// The node hasn't the room.
    NodeFull,
    /// No such record.
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Applied {
        op: Op,
        version: u64,
    },
    /// The write was already applied (a client's resubmit, a node's repeated report): nothing changes.
    Duplicate {
        version: u64,
    },
    Conflict(Conflict),
}

#[derive(Debug, Clone)]
pub struct Store {
    capacity: Vec<Resources>,
    used: Vec<Resources>,
    records: BTreeMap<WorkloadId, Record>,
    next_seq: u64,
    log: Vec<Entry>,
    max_records: usize,
    max_log: usize,
}

impl Store {
    /// `max_records` bounds the workloads it keeps and `max_log` its history; past either a write is
    /// refused with [`SimError::Full`].
    pub fn new(nodes: &[NodeSpec], max_records: usize, max_log: usize) -> Result<Self, SimError> {
        for (at, node) in nodes.iter().enumerate() {
            if usize::try_from(node.id.0).ok() != Some(at) {
                return Err(SimError::Config("node ids must be dense from 0, in order"));
            }
        }
        Ok(Self {
            capacity: nodes.iter().map(|n| n.capacity).collect(),
            used: vec![Resources::default(); nodes.len()],
            records: BTreeMap::new(),
            next_seq: 0,
            log: Vec::new(),
            max_records,
            max_log,
        })
    }

    pub fn apply(&mut self, at: Time, write: Write) -> Result<Outcome, SimError> {
        let outcome = match write {
            Write::Submit(spec) => self.submit(spec)?,
            Write::Place {
                workload,
                node,
                version,
            } => self.place(workload, node, version)?,
            Write::Complete {
                workload,
                node,
                version,
            } => self.complete(workload, node, version)?,
        };
        if let Outcome::Applied { op, .. } = outcome {
            if self.log.len() >= self.max_log {
                return Err(SimError::Full {
                    what: "the store's log",
                    bound: self.max_log,
                });
            }
            self.log.push(Entry { at, op });
        }
        Ok(outcome)
    }

    fn submit(&mut self, spec: WorkloadSpec) -> Result<Outcome, SimError> {
        if let Some(record) = self.records.get(&spec.id) {
            return Ok(Outcome::Duplicate {
                version: record.version,
            });
        }
        if self.records.len() >= self.max_records || self.log.len() >= self.max_log {
            return Err(SimError::Full {
                what: "the store's records",
                bound: self.max_records,
            });
        }
        let seq = self.next_seq;
        self.next_seq = seq.checked_add(1).ok_or(SimError::Full {
            what: "submit sequence",
            bound: usize::MAX,
        })?;
        self.records.insert(
            spec.id,
            Record {
                spec,
                seq,
                version: 1,
                state: State::Pending,
            },
        );
        Ok(Outcome::Applied {
            op: Op::Submit {
                workload: spec.id,
                seq,
            },
            version: 1,
        })
    }

    fn place(
        &mut self,
        workload: WorkloadId,
        node: NodeId,
        read: u64,
    ) -> Result<Outcome, SimError> {
        let at = usize::try_from(node.0).map_err(|_| SimError::UnknownNode(node))?;
        let Some(record) = self.records.get_mut(&workload) else {
            return Ok(Outcome::Conflict(Conflict::Missing));
        };
        if record.version != read {
            return Ok(Outcome::Conflict(Conflict::Version {
                read,
                found: record.version,
            }));
        }
        if record.state != State::Pending {
            return Ok(Outcome::Conflict(Conflict::State(record.state)));
        }
        let capacity = self.capacity.get(at).ok_or(SimError::UnknownNode(node))?;
        let used = self.used.get_mut(at).ok_or(SimError::UnknownNode(node))?;
        let after = used
            .checked_add(&record.spec.need)
            .ok_or(SimError::Resources(node))?;
        if !capacity.holds(&after) {
            return Ok(Outcome::Conflict(Conflict::NodeFull));
        }
        let version = bump(record.version)?;
        if self.log.len() >= self.max_log {
            return Err(SimError::Full {
                what: "the store's log",
                bound: self.max_log,
            });
        }
        *used = after;
        record.version = version;
        record.state = State::Placed(node);
        Ok(Outcome::Applied {
            op: Op::Place { workload, node },
            version,
        })
    }

    fn complete(
        &mut self,
        workload: WorkloadId,
        node: NodeId,
        read: u64,
    ) -> Result<Outcome, SimError> {
        let at = usize::try_from(node.0).map_err(|_| SimError::UnknownNode(node))?;
        let Some(record) = self.records.get_mut(&workload) else {
            return Ok(Outcome::Conflict(Conflict::Missing));
        };
        if record.state == State::Complete(node) {
            return Ok(Outcome::Duplicate {
                version: record.version,
            });
        }
        if record.state != State::Placed(node) {
            return Ok(Outcome::Conflict(Conflict::State(record.state)));
        }
        if record.version != read {
            return Ok(Outcome::Conflict(Conflict::Version {
                read,
                found: record.version,
            }));
        }
        let used = self.used.get_mut(at).ok_or(SimError::UnknownNode(node))?;
        let after = used
            .checked_sub(&record.spec.need)
            .ok_or(SimError::Resources(node))?;
        let version = bump(record.version)?;
        if self.log.len() >= self.max_log {
            return Err(SimError::Full {
                what: "the store's log",
                bound: self.max_log,
            });
        }
        *used = after;
        record.version = version;
        record.state = State::Complete(node);
        Ok(Outcome::Applied {
            op: Op::Complete { workload, node },
            version,
        })
    }

    pub fn record(&self, workload: WorkloadId) -> Option<&Record> {
        self.records.get(&workload)
    }

    pub fn records(&self) -> impl Iterator<Item = &Record> {
        self.records.values()
    }

    pub fn log(&self) -> &[Entry] {
        &self.log
    }

    /// The bytes one workload's record takes in the store's map, by its in-memory layout: the key
    /// and the record. The map's node overhead (amortised over its B-tree's entries) is not counted;
    /// `AC-T7.11.4`'s accounting, implementation goal 6, measures the heap.
    pub fn record_bytes() -> usize {
        size_of::<(WorkloadId, Record)>()
    }
}

fn bump(version: u64) -> Result<u64, SimError> {
    version.checked_add(1).ok_or(SimError::Full {
        what: "a record's version",
        bound: usize::MAX,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        let nodes = [NodeSpec {
            id: NodeId(0),
            capacity: Resources {
                cpu_milli: 1000,
                mem_bytes: 10,
                gpus: 0,
            },
            background_milli: 0,
        }];
        Store::new(&nodes, 4, 16).unwrap()
    }

    fn spec(id: u64, cpu: u64) -> WorkloadSpec {
        WorkloadSpec {
            id: WorkloadId(id),
            need: Resources {
                cpu_milli: cpu,
                mem_bytes: 1,
                gpus: 0,
            },
            kind: crate::model::Kind::Plain,
            arrives: 0,
            runs_for: None,
            tenancy: crate::model::Tenancy::default(),
        }
    }

    /// AC-T7.3.3: a placement committed twice from the same read starts once; the second is a conflict.
    #[test]
    fn a_placement_from_a_stale_read_conflicts() {
        let mut s = store();
        assert!(matches!(
            s.apply(0, Write::Submit(spec(1, 100))).unwrap(),
            Outcome::Applied { version: 1, .. }
        ));
        let place = Write::Place {
            workload: WorkloadId(1),
            node: NodeId(0),
            version: 1,
        };
        assert!(matches!(
            s.apply(1, place).unwrap(),
            Outcome::Applied { version: 2, .. }
        ));
        assert_eq!(
            s.apply(2, place).unwrap(),
            Outcome::Conflict(Conflict::Version { read: 1, found: 2 })
        );
        assert_eq!(s.log().len(), 2);
    }

    /// AC-T7.3.3: a resubmit and a repeated completion report change nothing.
    #[test]
    fn resubmits_and_repeated_reports_are_duplicates() {
        let mut s = store();
        s.apply(0, Write::Submit(spec(1, 100))).unwrap();
        assert_eq!(
            s.apply(1, Write::Submit(spec(1, 100))).unwrap(),
            Outcome::Duplicate { version: 1 }
        );
        s.apply(
            2,
            Write::Place {
                workload: WorkloadId(1),
                node: NodeId(0),
                version: 1,
            },
        )
        .unwrap();
        let done = Write::Complete {
            workload: WorkloadId(1),
            node: NodeId(0),
            version: 2,
        };
        assert!(matches!(s.apply(3, done).unwrap(), Outcome::Applied { .. }));
        assert_eq!(s.apply(4, done).unwrap(), Outcome::Duplicate { version: 3 });
        assert_eq!(
            s.record(WorkloadId(1)).unwrap().state,
            State::Complete(NodeId(0))
        );
    }

    #[test]
    fn a_node_is_never_overcommitted() {
        let mut s = store();
        s.apply(0, Write::Submit(spec(1, 700))).unwrap();
        s.apply(0, Write::Submit(spec(2, 700))).unwrap();
        s.apply(
            1,
            Write::Place {
                workload: WorkloadId(1),
                node: NodeId(0),
                version: 1,
            },
        )
        .unwrap();
        let second = s
            .apply(
                2,
                Write::Place {
                    workload: WorkloadId(2),
                    node: NodeId(0),
                    version: 1,
                },
            )
            .unwrap();
        assert_eq!(second, Outcome::Conflict(Conflict::NodeFull));
    }

    /// CLAUDE.md §2: past its bound the store refuses, typed.
    #[test]
    fn records_past_the_bound_are_refused() {
        let mut s = store();
        for id in 0..4 {
            s.apply(0, Write::Submit(spec(id, 1))).unwrap();
        }
        assert!(matches!(
            s.apply(0, Write::Submit(spec(9, 1))),
            Err(SimError::Full { .. })
        ));
    }
}
