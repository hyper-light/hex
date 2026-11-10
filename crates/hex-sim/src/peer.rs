//! Placement with no plane: the node a box is asked of owns it, and the node that runs it admits it
//! (kiln `crates/kiln-ci/src/place/` at `a63dd98ef`, docs/reach.md §2: Ray's ownership, HTCondor's
//! claiming), under kiln's restart rules (`place/durable_tests.rs`, W05-R25, LB03), adapted to boxes
//! and to the kernel ported in [`crate::lb`]:
//!
//! - The owner writes a box's row before anything the row gates (kiln's commit, then publish): the
//!   box asked before its client is answered, the node it goes to before its start is sent, its end
//!   before its client is told.
//! - It sends a box's start to one node only, and places it again only on that node's word that it
//!   doesn't hold it (refused, or asked and not held), never on its silence, so no box starts twice.
//! - A node admits by its ledger (the kernel's [`lb::Runner`]), writes its record of a box before the
//!   box is created, and keeps the record once the box ends until the owner says it has the end, so
//!   an owner that restarts is told again.
//! - Rows and records are named by the owner, the owner's epoch when the box was asked, and its number
//!   in that epoch, so a restarted owner's numbers never read as an earlier box's (kiln's
//!   `ReusedIdRan`).
//! - A node that restarts reads its records: its epoch one more; each box it owns asked again, or the
//!   node it went to asked whether it holds it; its ledger recovered with the boxes that outlived its
//!   agent, a record whose box its runtime never created dropped, and one whose box ended while it was
//!   down ended (the kernel's `Runner::recover`).
//!
//! Pure (CLAUDE.md §9): rows and records go through [`Records`], a seam whose shells are [`Memory`]
//! (with [`World`]'s crash nemesis) and the node's own files (`hex-node`'s `records`, temp and rename).
//! One node is a laptop: its messages to itself are its own, and there is no plane at all.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::index::{FitIndex, Verdict};
use crate::lb;
use crate::model::{Demand, Kind, NodeId, NodeSpec, Resources, WorkloadId};
use crate::rng::Rng;

#[cfg(test)]
thread_local! {
    /// A planted mistake (kiln place's `ReplaceOnSilence`): an owner whose link came back places its
    /// boxes there again without the node's word that it doesn't hold them.
    static REPLACE_ON_SILENCE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A box's name: its owner, the owner's epoch when it was asked, and its number in that epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key {
    pub owner: u32,
    pub epoch: u64,
    pub seq: u64,
}

impl Key {
    fn name(&self, kind: &str) -> String {
        format!("{kind}-{}-{}-{}", self.owner, self.epoch, self.seq)
    }

    /// The owner's view holds a box under this id: unique per owner and epoch's number.
    fn workload(&self) -> WorkloadId {
        WorkloadId(self.epoch.wrapping_shl(40) ^ self.seq)
    }
}

/// Why a node stopped: its process crashed at a write (the nemesis), or its records can't be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    Crashed,
    Damaged(String),
    Io(String),
}

/// A node's durable records, by name: each written whole or not at all.
pub trait Records {
    fn put(&mut self, name: &str, value: &str) -> Result<(), Stop>;
    fn remove(&mut self, name: &str) -> Result<(), Stop>;
    /// Every record, by name.
    fn all(&mut self) -> Result<Vec<(String, String)>, Stop>;
}

/// What nodes say to each other. A start's word comes back from the node it was sent to only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Msg {
    Start {
        key: Key,
        need: Resources,
    },
    /// A restarted owner, or one whose link came back: do you hold this box?
    Holds {
        key: Key,
    },
    /// The owner has the box's end: its record may go.
    Forget {
        key: Key,
    },
    Started {
        key: Key,
    },
    Refused {
        key: Key,
    },
    Held {
        key: Key,
        holds: bool,
        ended: bool,
    },
    Ended {
        key: Key,
    },
}

/// What a node tells its clients and its runtime, for the shell to act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Told {
    /// To the client: its box runs at a node.
    Started(Key, NodeId),
    /// To the client: its box ended.
    Ended(Key),
    /// To this node's runtime: create the box.
    Create(Key),
}

/// A box as its node's runtime has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boxed {
    Running,
    Exited,
    /// Never created.
    Absent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Asked,
    At(NodeId),
    Done(NodeId),
}

#[derive(Debug, Clone)]
struct Owned {
    need: Resources,
    state: State,
    /// Nodes that refused it since it last waited: in memory only.
    tried: BTreeSet<NodeId>,
}

#[derive(Debug, Clone, Copy)]
struct Ran {
    owner: NodeId,
    need: Resources,
    at: Option<lb::Handle>,
    ended: bool,
}

fn amounts(r: &Resources) -> lb::Amounts {
    [r.cpu_milli, r.mem_bytes, r.disk_bytes, u64::from(r.gpus)]
}

fn demand(need: Resources) -> Demand {
    Demand {
        need,
        kind: Kind::Plain,
        class: crate::model::Class::Oci,
        ask: crate::model::Ask::default(),
    }
}

fn need_text(r: &Resources) -> String {
    format!(
        "{} {} {} {}",
        r.cpu_milli, r.mem_bytes, r.gpus, r.disk_bytes
    )
}

fn damaged(what: &str) -> Stop {
    Stop::Damaged(what.to_owned())
}

/// A row or record's need: four numbers.
fn need_of<'a>(words: &mut impl Iterator<Item = &'a str>) -> Result<Resources, Stop> {
    let mut n = || -> Result<u64, Stop> {
        words
            .next()
            .and_then(|w| w.parse().ok())
            .ok_or_else(|| damaged("a need"))
    };
    Ok(Resources {
        cpu_milli: n()?,
        mem_bytes: n()?,
        gpus: u32::try_from(n()?).map_err(|_| damaged("a need's GPUs"))?,
        disk_bytes: n()?,
    })
}

/// A record's name read back: its kind and key.
fn parse_name(name: &str) -> Option<(&str, Key)> {
    let mut parts = name.split('-');
    let kind = parts.next()?;
    let owner = parts.next()?.parse().ok()?;
    let epoch = parts.next()?.parse().ok()?;
    let seq = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((kind, Key { owner, epoch, seq }))
}

/// One node: the boxes it owns, and the boxes it runs.
#[derive(Debug)]
pub struct Node {
    id: NodeId,
    epoch: u64,
    next: u64,
    ledger: lb::Runner,
    /// The owner's view: each node's room less what this owner placed there.
    view: FitIndex,
    owned: BTreeMap<Key, Owned>,
    ran: BTreeMap<Key, Ran>,
    /// For the shell: what the node told its clients and its runtime.
    pub told: Vec<Told>,
}

impl Node {
    /// Starts node `id` of `nodes` from its records: its epoch one more, its rows read, and its ledger
    /// recovered with the boxes `runtime` still runs.
    pub fn boot(
        id: NodeId,
        nodes: &[NodeSpec],
        records: &mut impl Records,
        runtime: impl Fn(Key) -> Boxed,
    ) -> Result<Node, Stop> {
        let spec = usize::try_from(id.0)
            .ok()
            .and_then(|at| nodes.get(at))
            .ok_or_else(|| damaged("a node not in the cluster"))?;
        let mut epoch = 0u64;
        let (mut owned, mut ran) = (BTreeMap::new(), BTreeMap::new());
        for (name, value) in records.all()? {
            let mut words = value.split(' ');
            if name == "epoch" {
                epoch = value.parse().map_err(|_| damaged("the epoch"))?;
                continue;
            }
            let (kind, key) = parse_name(&name).ok_or_else(|| damaged("a record's name"))?;
            match kind {
                "own" => {
                    let state = match (words.next(), words.next().map(str::parse::<u32>)) {
                        (Some("asked"), Some(Ok(_))) => State::Asked,
                        (Some("at"), Some(Ok(n))) => State::At(NodeId(n)),
                        (Some("done"), Some(Ok(n))) => State::Done(NodeId(n)),
                        _ => return Err(damaged("an owned box's state")),
                    };
                    let need = need_of(&mut words)?;
                    owned.insert(
                        key,
                        Owned {
                            need,
                            state,
                            tried: BTreeSet::new(),
                        },
                    );
                }
                "ran" => {
                    let owner = words
                        .next()
                        .and_then(|w| w.parse().ok())
                        .ok_or_else(|| damaged("a record's owner"))?;
                    let ended = match words.next() {
                        Some("running") => false,
                        Some("ended") => true,
                        _ => return Err(damaged("a record's end")),
                    };
                    let need = need_of(&mut words)?;
                    ran.insert(
                        key,
                        Ran {
                            owner: NodeId(owner),
                            need,
                            at: None,
                            ended,
                        },
                    );
                }
                _ => return Err(damaged("a record's kind")),
            }
        }
        let epoch = epoch.checked_add(1).ok_or_else(|| damaged("the epoch"))?;
        records.put("epoch", &epoch.to_string())?;
        let mut view = FitIndex::new(nodes).map_err(|e| Stop::Damaged(e.to_string()))?;
        for (key, o) in &owned {
            if let State::At(n) = o.state {
                view.take(n, key.workload(), &demand(o.need))
                    .map_err(|e| Stop::Damaged(e.to_string()))?;
            }
        }
        let mut ledger = lb::Runner::new(lb::RunnerId(id.0), amounts(&spec.capacity), 1);
        let mut node = Node {
            id,
            epoch,
            next: 0,
            ledger: lb::Runner::new(lb::RunnerId(id.0), [0; lb::DIMS], 1),
            view,
            owned,
            ran: BTreeMap::new(),
            told: Vec::new(),
        };
        // Each record against the runtime: running, it is kept (the agent outlived); exited while the
        // agent was down, it ended; never created, its record goes and its owner places it again.
        let mut kept = Vec::new();
        let mut keys = Vec::new();
        for (key, r) in ran {
            let r = match (r.ended, runtime(key)) {
                (false, Boxed::Running) => {
                    kept.push(lb::Kept {
                        owner: lb::OwnerId(r.owner.0),
                        run: lb::Handle::NONE,
                        need: amounts(&r.need),
                    });
                    keys.push(key);
                    r
                }
                (false, Boxed::Exited) => {
                    records.put(&key.name("ran"), &ran_text(&r, true))?;
                    Ran { ended: true, ..r }
                }
                (false, Boxed::Absent) => {
                    records.remove(&key.name("ran"))?;
                    continue;
                }
                (true, _) => r,
            };
            node.ran.insert(key, r);
        }
        for (key, at) in keys.iter().zip(ledger.recover(&kept)) {
            let Some(at) = at else {
                return Err(damaged("a kept box past the node's room"));
            };
            if let Some(r) = node.ran.get_mut(key) {
                r.at = Some(at);
            }
        }
        node.ledger = ledger;
        Ok(node)
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    /// A client asks a box: its row first, then its answer, then its place.
    pub fn submit(
        &mut self,
        need: Resources,
        records: &mut impl Records,
    ) -> Result<(Key, Vec<(NodeId, Msg)>), Stop> {
        let key = Key {
            owner: self.id.0,
            epoch: self.epoch,
            seq: self.next,
        };
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| damaged("a box number"))?;
        records.put(&key.name("own"), &own_text(State::Asked, &need))?;
        self.owned.insert(
            key,
            Owned {
                need,
                state: State::Asked,
                tried: BTreeSet::new(),
            },
        );
        let mut out = Vec::new();
        self.place(key, records, &mut out)?;
        Ok((key, out))
    }

    /// Where `key` goes: the node the kernel's walk ranks first among those this owner's view admits
    /// it on and that haven't refused it; its row says so before its start is sent. None: it waits.
    fn place(
        &mut self,
        key: Key,
        records: &mut impl Records,
        out: &mut Vec<(NodeId, Msg)>,
    ) -> Result<(), Stop> {
        let Some(o) = self.owned.get(&key) else {
            return Ok(());
        };
        if o.state != State::Asked {
            return Ok(());
        }
        let d = demand(o.need);
        let mut best: Option<crate::index::Key> = None;
        for n in 0..self.view.len() {
            let n = NodeId(u32::try_from(n).map_err(|_| damaged("a node id"))?);
            if o.tried.contains(&n) || self.view.verdict(n, &d) != Ok(Verdict::Admits) {
                continue;
            }
            let k = self.view.key(n).map_err(|e| Stop::Damaged(e.to_string()))?;
            if best.is_none_or(|b| k < b) {
                best = Some(k);
            }
        }
        let Some(best) = best else {
            return Ok(());
        };
        let (need, to) = (o.need, best.node);
        records.put(&key.name("own"), &own_text(State::At(to), &need))?;
        self.view
            .take(to, key.workload(), &d)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        if let Some(o) = self.owned.get_mut(&key) {
            o.state = State::At(to);
        }
        out.push((to, Msg::Start { key, need }));
        Ok(())
    }

    /// Back to waiting on `from`'s word that it doesn't hold `key`, then placed elsewhere.
    fn not_there(
        &mut self,
        key: Key,
        from: NodeId,
        records: &mut impl Records,
        out: &mut Vec<(NodeId, Msg)>,
    ) -> Result<(), Stop> {
        let Some(o) = self.owned.get(&key) else {
            return Ok(());
        };
        if o.state != State::At(from) {
            return Ok(());
        }
        let need = o.need;
        records.put(&key.name("own"), &own_text(State::Asked, &need))?;
        self.view
            .give(from, key.workload(), &demand(need))
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        if let Some(o) = self.owned.get_mut(&key) {
            o.state = State::Asked;
            o.tried.insert(from);
        }
        self.place(key, records, out)
    }

    /// `key` ended at `from`: its end written, its client told, the node told to forget it, and its
    /// row gone once the client has it.
    fn ended(
        &mut self,
        key: Key,
        from: NodeId,
        records: &mut impl Records,
        out: &mut Vec<(NodeId, Msg)>,
    ) -> Result<(), Stop> {
        let Some(o) = self.owned.get(&key) else {
            out.push((from, Msg::Forget { key }));
            return Ok(());
        };
        let need = o.need;
        match o.state {
            State::At(n) if n == from => {
                records.put(&key.name("own"), &own_text(State::Done(from), &need))?;
                self.view
                    .give(from, key.workload(), &demand(need))
                    .map_err(|e| Stop::Damaged(e.to_string()))?;
            }
            State::Done(n) if n == from => {}
            State::Asked | State::At(_) | State::Done(_) => return Ok(()),
        }
        if let Some(o) = self.owned.get_mut(&key) {
            o.state = State::Done(from);
        }
        self.told.push(Told::Ended(key));
        out.push((from, Msg::Forget { key }));
        records.remove(&key.name("own"))?;
        self.owned.remove(&key);
        Ok(())
    }

    /// A word from node `from`.
    pub fn hear(
        &mut self,
        from: NodeId,
        msg: Msg,
        records: &mut impl Records,
    ) -> Result<Vec<(NodeId, Msg)>, Stop> {
        let mut out = Vec::new();
        match msg {
            Msg::Start { key, need } => self.start(from, key, need, records, &mut out)?,
            Msg::Holds { key } => {
                let r = self.ran.get(&key);
                out.push((
                    from,
                    Msg::Held {
                        key,
                        holds: r.is_some(),
                        ended: r.is_some_and(|r| r.ended),
                    },
                ));
            }
            Msg::Forget { key } => {
                if self.ran.get(&key).is_some_and(|r| r.ended) {
                    records.remove(&key.name("ran"))?;
                    self.ran.remove(&key);
                }
            }
            Msg::Started { key } => {
                if self
                    .owned
                    .get(&key)
                    .is_some_and(|o| o.state == State::At(from))
                {
                    self.told.push(Told::Started(key, from));
                }
            }
            Msg::Refused { key }
            | Msg::Held {
                key, holds: false, ..
            } => self.not_there(key, from, records, &mut out)?,
            Msg::Held {
                key,
                holds: true,
                ended: true,
            }
            | Msg::Ended { key } => self.ended(key, from, records, &mut out)?,
            Msg::Held {
                key,
                holds: true,
                ended: false,
            } => {
                if self
                    .owned
                    .get(&key)
                    .is_some_and(|o| o.state == State::At(from))
                {
                    self.told.push(Told::Started(key, from));
                }
            }
        }
        Ok(out)
    }

    /// A start from `owner`: one the node holds is answered as it stands; else admitted by the ledger,
    /// its record written, then the box created.
    fn start(
        &mut self,
        owner: NodeId,
        key: Key,
        need: Resources,
        records: &mut impl Records,
        out: &mut Vec<(NodeId, Msg)>,
    ) -> Result<(), Stop> {
        if let Some(r) = self.ran.get(&key) {
            out.push((
                owner,
                if r.ended {
                    Msg::Ended { key }
                } else {
                    Msg::Started { key }
                },
            ));
            return Ok(());
        }
        let start = lb::Start {
            owner: lb::OwnerId(owner.0),
            run: lb::Handle::NONE,
            epoch: self.ledger.epoch(),
            need: amounts(&need),
        };
        let lb::ToOwner::Started { at, .. } = self.ledger.start(start) else {
            out.push((owner, Msg::Refused { key }));
            return Ok(());
        };
        let r = Ran {
            owner,
            need,
            at: Some(at),
            ended: false,
        };
        records.put(&key.name("ran"), &ran_text(&r, false))?;
        self.ran.insert(key, r);
        self.told.push(Told::Create(key));
        out.push((owner, Msg::Started { key }));
        Ok(())
    }

    /// The box `key` running here ended (it exited, or was deleted): its room freed, its end written,
    /// its owner told.
    pub fn end(
        &mut self,
        key: Key,
        records: &mut impl Records,
    ) -> Result<Vec<(NodeId, Msg)>, Stop> {
        let Some(r) = self.ran.get(&key).copied().filter(|r| !r.ended) else {
            return Ok(Vec::new());
        };
        if let Some(at) = r.at {
            let mut rout = lb::RunnerOut::new();
            self.ledger
                .end(at, &mut rout)
                .ok_or_else(|| damaged("a box the ledger didn't hold"))?;
        }
        records.put(&key.name("ran"), &ran_text(&r, true))?;
        if let Some(r) = self.ran.get_mut(&key) {
            r.ended = true;
            r.at = None;
        }
        Ok(vec![(r.owner, Msg::Ended { key })])
    }

    /// The link to `peer` is up (again): each box this node sent it is asked after; each end this node
    /// has written of a box that ran there told again to its client, `peer` told to forget it, and its
    /// row gone; each end it holds for `peer` told again; and each waiting box placed again.
    pub fn link_up(
        &mut self,
        peer: NodeId,
        records: &mut impl Records,
    ) -> Result<Vec<(NodeId, Msg)>, Stop> {
        let mut out = Vec::new();
        let mut done = Vec::new();
        #[cfg(test)]
        let mut silent = Vec::new();
        for (key, o) in &self.owned {
            match o.state {
                #[cfg(test)]
                State::At(n) if n == peer && REPLACE_ON_SILENCE.get() => silent.push(*key),
                State::At(n) if n == peer => out.push((peer, Msg::Holds { key: *key })),
                State::Done(n) if n == peer => done.push(*key),
                State::Asked | State::At(_) | State::Done(_) => {}
            }
        }
        #[cfg(test)]
        for key in silent {
            self.not_there(key, peer, records, &mut out)?;
        }
        for key in done {
            self.told.push(Told::Ended(key));
            out.push((peer, Msg::Forget { key }));
            records.remove(&key.name("own"))?;
            self.owned.remove(&key);
        }
        for (key, r) in &self.ran {
            if r.owner == peer && r.ended {
                out.push((peer, Msg::Ended { key: *key }));
            }
        }
        self.retry(records, &mut out)?;
        Ok(out)
    }

    /// Every waiting box placed again, its refusals forgotten: room may have come back.
    pub fn retry(
        &mut self,
        records: &mut impl Records,
        out: &mut Vec<(NodeId, Msg)>,
    ) -> Result<(), Stop> {
        let waiting: Vec<Key> = self
            .owned
            .iter()
            .filter(|(_, o)| o.state == State::Asked)
            .map(|(k, _)| *k)
            .collect();
        for key in waiting {
            if let Some(o) = self.owned.get_mut(&key) {
                o.tried.clear();
            }
            self.place(key, records, out)?;
        }
        Ok(())
    }

    /// What the ledger holds, against what runs: the boxes it admitted.
    pub fn admitted(&self) -> lb::Amounts {
        let mut sum = [0u64; lb::DIMS];
        for (_, _, _, need) in self.ledger.admitted() {
            for (s, n) in sum.iter_mut().zip(need) {
                *s = s.saturating_add(n);
            }
        }
        sum
    }

    /// The boxes this node owns that wait for room, run, or have ended unforgotten.
    pub fn owned(&self) -> usize {
        self.owned.len()
    }

    /// The records of boxes this node runs or ran and hasn't forgotten.
    pub fn records(&self) -> usize {
        self.ran.len()
    }
}

fn own_text(state: State, need: &Resources) -> String {
    let (word, n) = match state {
        State::Asked => ("asked", 0),
        State::At(n) => ("at", n.0),
        State::Done(n) => ("done", n.0),
    };
    format!("{word} {n} {}", need_text(need))
}

fn ran_text(r: &Ran, ended: bool) -> String {
    let end = if ended { "ended" } else { "running" };
    format!("{} {end} {}", r.owner.0, need_text(&r.need))
}

/// Records in memory, for the simulator: what a node's disk keeps across its crashes.
#[derive(Debug, Clone, Default)]
pub struct Memory {
    rows: BTreeMap<String, String>,
}

impl Records for Memory {
    fn put(&mut self, name: &str, value: &str) -> Result<(), Stop> {
        self.rows.insert(name.to_owned(), value.to_owned());
        Ok(())
    }

    fn remove(&mut self, name: &str) -> Result<(), Stop> {
        self.rows.remove(name);
        Ok(())
    }

    fn all(&mut self) -> Result<Vec<(String, String)>, Stop> {
        Ok(self
            .rows
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }
}

/// When an armed crash strikes a write: before it lands, or after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Before,
    After,
}

/// A node's records with the crash nemesis: an armed crash strikes the next write.
#[derive(Debug)]
pub struct Crashing<R> {
    pub inner: R,
    pub armed: Option<When>,
    /// Writes made, and crashes struck.
    pub writes: u64,
    pub crashes: u64,
}

impl<R: Records> Crashing<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            armed: None,
            writes: 0,
            crashes: 0,
        }
    }

    fn strike(&mut self, write: impl FnOnce(&mut R) -> Result<(), Stop>) -> Result<(), Stop> {
        self.writes = self.writes.saturating_add(1);
        match self.armed.take() {
            None => write(&mut self.inner),
            Some(when) => {
                if when == When::After {
                    write(&mut self.inner)?;
                }
                self.crashes = self.crashes.saturating_add(1);
                Err(Stop::Crashed)
            }
        }
    }
}

impl<R: Records> Records for Crashing<R> {
    fn put(&mut self, name: &str, value: &str) -> Result<(), Stop> {
        self.strike(|r| r.put(name, value))
    }

    fn remove(&mut self, name: &str) -> Result<(), Stop> {
        self.strike(|r| r.remove(name))
    }

    fn all(&mut self) -> Result<Vec<(String, String)>, Stop> {
        self.inner.all()
    }
}

/// What a run of [`World`] found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Report {
    pub asked: u64,
    pub started: u64,
    /// Boxes created twice, at one node or two: must be 0.
    pub double: u64,
    /// Boxes asked and answered whose client was never told their end: must be 0.
    pub lost: u64,
    /// Ends told to a client again after a restart: the same end, which the client takes once
    /// (kiln's rule: every answer that ends a run is the same one).
    pub told_again: u64,
    pub crashes: u64,
    pub refusals: u64,
    pub link_drops: u64,
}

impl Report {
    pub fn lost_or_double(&self) -> u64 {
        self.double.saturating_add(self.lost)
    }
}

#[derive(Debug, Default)]
struct Link {
    up: bool,
    queue: VecDeque<Msg>,
}

/// Nodes placing with no plane, every pair linked, driven by a seeded nemesis: boxes asked at any
/// node, words delivered in any order between links (each link in order), boxes ending, links
/// dropping (what was on them lost) and coming back, and a node's agent crashing at any write, before
/// or after it lands, and starting again from its records while its boxes run on. One node is a
/// laptop.
pub struct World<R> {
    specs: Vec<NodeSpec>,
    nodes: Vec<Option<Node>>,
    records: Vec<Crashing<R>>,
    links: Vec<Link>,
    /// Each box's runtime state, at its node: the boxes outlive their agents.
    boxes: BTreeMap<Key, (NodeId, Boxed)>,
    created: BTreeMap<Key, u64>,
    answered: BTreeSet<Key>,
    told_ended: BTreeMap<Key, u64>,
    rng: Rng,
    pub report: Report,
}

impl<R: Records> World<R> {
    /// The cluster's nodes, each with its records, booted and linked.
    pub fn new(specs: Vec<NodeSpec>, records: Vec<R>, seed: u64) -> Result<Self, Stop> {
        let n = specs.len();
        if records.len() != n || n == 0 {
            return Err(damaged("a node's records each"));
        }
        let mut w = World {
            nodes: (0..n).map(|_| None).collect(),
            records: records.into_iter().map(Crashing::new).collect(),
            links: (0..n.saturating_mul(n)).map(|_| Link::default()).collect(),
            specs,
            boxes: BTreeMap::new(),
            created: BTreeMap::new(),
            answered: BTreeSet::new(),
            told_ended: BTreeMap::new(),
            rng: Rng::new(seed),
            report: Report::default(),
        };
        for i in 0..n {
            w.restart(i)?;
        }
        Ok(w)
    }

    fn n(&self) -> usize {
        self.nodes.len()
    }

    fn link(&mut self, from: usize, to: usize) -> Option<&mut Link> {
        let n = self.n();
        self.links.get_mut(from.checked_mul(n)?.checked_add(to)?)
    }

    fn send(&mut self, from: usize, out: Vec<(NodeId, Msg)>) {
        for (to, m) in out {
            let Ok(to) = usize::try_from(to.0) else {
                continue;
            };
            if let Some(l) = self.link(from, to)
                && l.up
            {
                l.queue.push_back(m);
            }
        }
    }

    /// Acts on what node `i` told; a stop is a crash (anything else is the run's error).
    fn after(&mut self, i: usize, r: Result<Vec<(NodeId, Msg)>, Stop>) -> Result<(), Stop> {
        let told = self
            .nodes
            .get_mut(i)
            .and_then(Option::as_mut)
            .map(|n| std::mem::take(&mut n.told))
            .unwrap_or_default();
        let id = NodeId(u32::try_from(i).map_err(|_| damaged("a node id"))?);
        for t in told {
            match t {
                Told::Create(key) => {
                    let c = self.created.entry(key).or_insert(0);
                    *c = c.saturating_add(1);
                    if *c > 1 {
                        self.report.double = self.report.double.saturating_add(1);
                    }
                    self.report.started = self.report.started.saturating_add(1);
                    self.boxes.insert(key, (id, Boxed::Running));
                }
                Told::Ended(key) => {
                    let c = self.told_ended.entry(key).or_insert(0);
                    *c = c.saturating_add(1);
                    if *c > 1 {
                        self.report.told_again = self.report.told_again.saturating_add(1);
                    }
                }
                Told::Started(..) => {}
            }
        }
        match r {
            Ok(out) => {
                self.send(i, out);
                Ok(())
            }
            Err(Stop::Crashed) => {
                self.crash(i);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Node `i`'s agent is gone: its links drop, what was on them lost; its boxes run on.
    fn crash(&mut self, i: usize) {
        if let Some(n) = self.nodes.get_mut(i) {
            *n = None;
        }
        for j in 0..self.n() {
            for (a, b) in [(i, j), (j, i)] {
                if let Some(l) = self.link(a, b) {
                    l.up = false;
                    l.queue.clear();
                }
            }
        }
    }

    /// Node `i` starts again from its records, and its links come back to every node up.
    fn restart(&mut self, i: usize) -> Result<(), Stop> {
        let id = NodeId(u32::try_from(i).map_err(|_| damaged("a node id"))?);
        let boxes = &self.boxes;
        let runtime = |k: Key| match boxes.get(&k) {
            Some(&(at, state)) if at == id => state,
            _ => Boxed::Absent,
        };
        let rec = self.records.get_mut(i).ok_or_else(|| damaged("records"))?;
        match Node::boot(id, &self.specs, rec, runtime) {
            Ok(node) => {
                if let Some(slot) = self.nodes.get_mut(i) {
                    *slot = Some(node);
                }
            }
            Err(Stop::Crashed) => return Ok(()),
            Err(e) => return Err(e),
        }
        for j in 0..self.n() {
            if self.nodes.get(j).is_some_and(Option::is_some) {
                self.relink(i, j)?;
            }
        }
        Ok(())
    }

    /// The link between `i` and `j` comes up, both ends told.
    fn relink(&mut self, i: usize, j: usize) -> Result<(), Stop> {
        for (a, b) in [(i, j), (j, i)] {
            if let Some(l) = self.link(a, b) {
                l.up = true;
            }
        }
        for (a, b) in [(i, j), (j, i)] {
            let peer = NodeId(u32::try_from(b).map_err(|_| damaged("a node id"))?);
            let (Some(Some(node)), Some(rec)) = (self.nodes.get_mut(a), self.records.get_mut(a))
            else {
                continue;
            };
            let r = node.link_up(peer, rec);
            self.after(a, r)?;
            if i == j {
                break;
            }
        }
        Ok(())
    }

    fn up(&self, i: usize) -> bool {
        self.nodes.get(i).is_some_and(Option::is_some)
    }

    fn pick(&mut self, n: usize) -> usize {
        usize::try_from(
            self.rng
                .range(0, u64::try_from(n.saturating_sub(1)).unwrap_or(0)),
        )
        .unwrap_or(0)
    }

    /// A client at an up node asks a box of a size every node holds.
    fn ask(&mut self) -> Result<(), Stop> {
        let i = self.pick(self.n());
        let least = self
            .specs
            .iter()
            .map(|s| s.capacity.cpu_milli)
            .min()
            .unwrap_or(0);
        let need = Resources {
            cpu_milli: self.rng.range(1, least.max(1)),
            mem_bytes: self.rng.range(1, 1 << 20),
            ..Resources::default()
        };
        let (Some(Some(node)), Some(rec)) = (self.nodes.get_mut(i), self.records.get_mut(i)) else {
            return Ok(());
        };
        match node.submit(need, rec) {
            Ok((key, out)) => {
                self.answered.insert(key);
                self.report.asked = self.report.asked.saturating_add(1);
                self.after(i, Ok(out))
            }
            Err(e) => self.after(i, Err(e)),
        }
    }

    /// One word on one up link, delivered.
    fn deliver(&mut self) -> Result<bool, Stop> {
        let n = self.n();
        let busy: Vec<usize> = (0..n.saturating_mul(n))
            .filter(|&k| {
                self.links
                    .get(k)
                    .is_some_and(|l| l.up && !l.queue.is_empty())
            })
            .collect();
        if busy.is_empty() {
            return Ok(false);
        }
        let k = *busy.get(self.pick(busy.len())).unwrap_or(&0);
        let (from, to) = (k.checked_div(n).unwrap_or(0), k.checked_rem(n).unwrap_or(0));
        let Some(msg) = self.links.get_mut(k).and_then(|l| l.queue.pop_front()) else {
            return Ok(false);
        };
        if matches!(msg, Msg::Refused { .. }) {
            self.report.refusals = self.report.refusals.saturating_add(1);
        }
        let from = NodeId(u32::try_from(from).map_err(|_| damaged("a node id"))?);
        let (Some(Some(node)), Some(rec)) = (self.nodes.get_mut(to), self.records.get_mut(to))
        else {
            return Ok(true);
        };
        let r = node.hear(from, msg, rec);
        self.after(to, r)?;
        Ok(true)
    }

    /// A running box exits: its node told if its agent is up, else it learns at its restart.
    fn exit(&mut self) -> Result<(), Stop> {
        let running: Vec<(Key, NodeId)> = self
            .boxes
            .iter()
            .filter(|(_, (_, s))| *s == Boxed::Running)
            .map(|(k, (n, _))| (*k, *n))
            .collect();
        if running.is_empty() {
            return Ok(());
        }
        let Some(&(key, at)) = running.get(self.pick(running.len())) else {
            return Ok(());
        };
        self.boxes.insert(key, (at, Boxed::Exited));
        let i = usize::try_from(at.0).map_err(|_| damaged("a node id"))?;
        let (Some(Some(node)), Some(rec)) = (self.nodes.get_mut(i), self.records.get_mut(i)) else {
            return Ok(());
        };
        let r = node.end(key, rec);
        self.after(i, r)
    }

    /// One step of the nemesis.
    pub fn step(&mut self) -> Result<(), Stop> {
        let n = self.n();
        match self.rng.range(0, 99) {
            0..30 => self.ask(),
            30..70 => self.deliver().map(|_| ()),
            70..82 => self.exit(),
            82..88 if n > 1 => {
                let (i, j) = (self.pick(n), self.pick(n));
                if i != j && self.up(i) && self.up(j) {
                    self.report.link_drops = self.report.link_drops.saturating_add(1);
                    for (a, b) in [(i, j), (j, i)] {
                        if let Some(l) = self.link(a, b) {
                            l.up = false;
                            l.queue.clear();
                        }
                    }
                }
                Ok(())
            }
            82..94 => {
                let (i, j) = (self.pick(n), self.pick(n));
                if self.up(i) && self.up(j) && !self.link(i, j).is_some_and(|l| l.up) {
                    self.relink(i, j)?;
                }
                Ok(())
            }
            94..97 => {
                let i = self.pick(n);
                let when = if self.rng.chance(500) {
                    When::Before
                } else {
                    When::After
                };
                if let Some(r) = self.records.get_mut(i) {
                    r.armed = Some(when);
                }
                Ok(())
            }
            _ => {
                let down: Vec<usize> = (0..n).filter(|&i| !self.up(i)).collect();
                match down.get(self.pick(down.len().max(1))) {
                    Some(&i) => self.restart(i),
                    None => Ok(()),
                }
            }
        }
    }

    /// Everything settles: crashes disarmed, every node up and linked, every word delivered, every
    /// waiting box placed, then every box ended and every end told: each box asked and answered must
    /// end once, told once, created once, and every ledger and record must be empty after.
    pub fn finish(&mut self, budget: u64) -> Result<Report, Stop> {
        for r in &mut self.records {
            r.armed = None;
        }
        for i in 0..self.n() {
            if !self.up(i) {
                self.restart(i)?;
            }
        }
        for i in 0..self.n() {
            for j in 0..self.n() {
                if !self.link(i, j).is_some_and(|l| l.up) {
                    self.relink(i, j)?;
                }
            }
        }
        let mut spent = 0u64;
        loop {
            while self.deliver()? {
                spent = spent.saturating_add(1);
                if spent > budget {
                    return Err(damaged("a run that doesn't settle within its budget"));
                }
            }
            let running: Vec<Key> = self
                .boxes
                .iter()
                .filter(|(_, (_, s))| *s == Boxed::Running)
                .map(|(k, _)| *k)
                .collect();
            let waiting = self.nodes.iter().flatten().any(|n| n.owned() > 0);
            if running.is_empty() && !waiting {
                break;
            }
            for key in running {
                let Some(&(at, _)) = self.boxes.get(&key) else {
                    continue;
                };
                self.boxes.insert(key, (at, Boxed::Exited));
                let i = usize::try_from(at.0).map_err(|_| damaged("a node id"))?;
                if let (Some(Some(node)), Some(rec)) =
                    (self.nodes.get_mut(i), self.records.get_mut(i))
                {
                    let r = node.end(key, rec);
                    self.after(i, r)?;
                }
            }
            for i in 0..self.n() {
                if let (Some(Some(node)), Some(rec)) =
                    (self.nodes.get_mut(i), self.records.get_mut(i))
                {
                    let mut out = Vec::new();
                    let r = node.retry(rec, &mut out).map(|()| out);
                    self.after(i, r)?;
                }
            }
            spent = spent.saturating_add(1);
            if spent > budget {
                return Err(damaged("a run that doesn't settle within its budget"));
            }
        }
        for key in &self.answered {
            if self.told_ended.get(key).copied().unwrap_or(0) == 0 {
                self.report.lost = self.report.lost.saturating_add(1);
            }
        }
        for node in self.nodes.iter().flatten() {
            if node.admitted() != [0; lb::DIMS] || node.records() > 0 || node.owned() > 0 {
                return Err(Stop::Damaged(format!(
                    "node {} keeps room {:?}, {} records and {} boxes after every box ended",
                    node.id().0,
                    node.admitted(),
                    node.records(),
                    node.owned()
                )));
            }
        }
        self.report.crashes = self.records.iter().map(|r| r.crashes).sum();
        Ok(self.report)
    }
}

/// `nodes` nodes of `capacity` each, records in memory: the simulator's cluster with no plane.
pub fn memory(nodes: u32, capacity: Resources, seed: u64) -> Result<World<Memory>, Stop> {
    let specs = (0..nodes)
        .map(|i| NodeSpec::new(NodeId(i), capacity))
        .collect();
    let records = (0..nodes).map(|_| Memory::default()).collect();
    World::new(specs, records, seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP: Resources = Resources {
        cpu_milli: 4_000,
        mem_bytes: 1 << 30,
        gpus: 0,
        disk_bytes: 0,
    };

    fn run(nodes: u32, seed: u64, steps: u32) -> Report {
        let mut w = memory(nodes, CAP, seed).unwrap();
        for _ in 0..steps {
            w.step().unwrap_or_else(|e| panic!("seed {seed}: {e:?}"));
        }
        w.finish(1 << 20)
            .unwrap_or_else(|e| panic!("seed {seed}: {e:?}"))
    }

    /// CP-T7.4, p2p: do run four nodes with no plane for 64 seeds of 600 steps under the nemesis
    /// (agents crashing at any write, before or after it lands; links dropping with what was on them);
    /// expect every box asked and answered created once, ended once and its client told once
    /// (lost-or-double 0), and every ledger and record empty after; and (non-vacuity) crashes struck,
    /// links dropped, nodes refused starts, and boxes ran.
    #[test]
    fn p2p_keeps_every_box_once_through_crashes_and_dropped_links() {
        let mut all = Report::default();
        for seed in 0..64 {
            let r = run(4, seed, 600);
            assert_eq!(r.lost_or_double(), 0, "seed {seed}: {r:?}");
            all.started += r.started;
            all.crashes += r.crashes;
            all.refusals += r.refusals;
            all.link_drops += r.link_drops;
        }
        assert!(
            all.started > 0 && all.crashes > 0 && all.refusals > 0 && all.link_drops > 0,
            "{all:?}"
        );
    }

    /// Non-vacuity: do plant kiln's `ReplaceOnSilence` (an owner places a box again when its node's
    /// link comes back, without asking the node); expect some seed to create a box twice.
    #[test]
    fn replacing_on_silence_is_caught() {
        REPLACE_ON_SILENCE.set(true);
        let caught = (0..64).any(|seed| {
            let mut w = memory(4, CAP, seed).unwrap();
            for _ in 0..600 {
                w.step().unwrap();
            }
            w.finish(1 << 20).map_or(true, |r| r.double > 0)
        });
        REPLACE_ON_SILENCE.set(false);
        assert!(
            caught,
            "no seed created a box twice with the mistake planted"
        );
    }

    /// CP-T7.4, laptop-only: one node, owner and node at once, no plane: do the same; expect the same.
    #[test]
    fn a_laptop_keeps_every_box_once_through_crashes() {
        let mut all = Report::default();
        for seed in 0..64 {
            let r = run(1, seed, 400);
            assert_eq!(r.lost_or_double(), 0, "seed {seed}: {r:?}");
            all.started += r.started;
            all.crashes += r.crashes;
        }
        assert!(all.started > 0 && all.crashes > 0, "{all:?}");
    }

    /// kiln's `ReusedIdRan`: a box's name carries its owner's epoch, so a restarted owner's first box
    /// isn't read as the box its first number named before.
    #[test]
    fn a_restarted_owners_numbers_never_name_an_earlier_box() {
        let specs = vec![NodeSpec::new(NodeId(0), CAP)];
        let mut rec = Memory::default();
        let mut a = Node::boot(NodeId(0), &specs, &mut rec, |_| Boxed::Absent).unwrap();
        let (first, _) = a
            .submit(
                Resources {
                    cpu_milli: 1,
                    ..Resources::default()
                },
                &mut rec,
            )
            .unwrap();
        let mut b = Node::boot(NodeId(0), &specs, &mut rec, |_| Boxed::Absent).unwrap();
        let (again, _) = b
            .submit(
                Resources {
                    cpu_milli: 1,
                    ..Resources::default()
                },
                &mut rec,
            )
            .unwrap();
        assert_eq!(first.seq, again.seq);
        assert_ne!(first, again);
        assert_ne!(first.name("own"), again.name("own"));
    }

    /// CLAUDE.md §6, hostile input: a damaged record stops the node's boot, typed, never guessed.
    #[test]
    fn a_damaged_record_is_refused() {
        let specs = vec![NodeSpec::new(NodeId(0), CAP)];
        for (name, value) in [
            ("own-0-1-0", "maybe 0 1 1 0 0"),
            ("ran-0-1-0", "0 running lots"),
            ("epoch", "x"),
            ("own-0-1", "asked 0 1 1 0 0"),
        ] {
            let mut rec = Memory::default();
            rec.put(name, value).unwrap();
            assert!(
                matches!(
                    Node::boot(NodeId(0), &specs, &mut rec, |_| Boxed::Absent),
                    Err(Stop::Damaged(_))
                ),
                "{name} {value}"
            );
        }
    }
}
