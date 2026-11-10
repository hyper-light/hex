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
use std::fmt::Write;

use crate::index::{FitIndex, Pick, Scorer, Verdict};
use crate::lb;
use crate::model::{
    Arch, Ask, Class, Demand, Kind, ModelId, NodeId, NodeSpec, Os, Resources, WorkloadId,
};
use crate::rng::Rng;
use crate::start_cost::{Rates, StartObservation, Terms};

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
}

/// Why a node stopped: its process crashed at a write (the nemesis), or its records can't be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    Crashed,
    Damaged(String),
    Io(String),
    Full { what: &'static str, bound: usize },
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
        demand: Demand,
    },
    /// Durable owner cancellation; the receiver proves actual deletion before acknowledging it.
    Delete {
        key: Key,
        demand: Demand,
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
        created: bool,
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
    Create(Key, Demand),
    /// Delete/inspect this exact backend, then call end only after actual deletion/absence proof.
    Delete(Key, Demand),
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
    Created(NodeId),
    Deleting(NodeId),
    Done(NodeId),
}

#[derive(Debug, Clone)]
struct Owned {
    demand: Demand,
    workload: WorkloadId,
    /// A persisted Created is reconfirmed against its peer after every owner boot.
    ready: bool,
    state: State,
    /// Nodes that refused it since it last waited: in memory only.
    tried: BTreeSet<NodeId>,
}

#[derive(Debug, Clone, Copy)]
struct Ran {
    owner: NodeId,
    demand: Demand,
    workload: WorkloadId,
    created: bool,
    at: Option<lb::Handle>,
    ended: bool,
}

fn amounts(r: &Resources) -> lb::Amounts {
    [r.cpu_milli, r.mem_bytes, r.disk_bytes, u64::from(r.gpus)]
}

/// The simulator's unconstrained OCI fixture; production callers submit an explicit Demand.
fn plain(need: Resources) -> Demand {
    Demand {
        need,
        kind: Kind::Plain,
        class: Class::Oci,
        ask: Ask::default(),
    }
}

fn damaged(what: &str) -> Stop {
    Stop::Damaged(what.to_owned())
}

fn number<'a, T: std::str::FromStr>(words: &mut impl Iterator<Item = &'a str>) -> Result<T, Stop> {
    words
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| damaged("a demand's number"))
}

fn model_id(name: &str, models: &[String]) -> Result<Option<ModelId>, Stop> {
    if name == "-" {
        return Ok(None);
    }
    models
        .iter()
        .position(|m| m == name)
        .and_then(|at| u32::try_from(at).ok())
        .filter(|&at| at != ModelId::UNNAMED.0)
        .map(|id| Some(ModelId(id)))
        .ok_or_else(|| damaged("a demand's unknown GPU model"))
}

fn check_demand(d: &Demand, models: &[String]) -> Result<(), Stop> {
    if let Some(model) = d.ask.gpu_model
        && (model == ModelId::UNNAMED
            || usize::try_from(model.0)
                .ok()
                .and_then(|at| models.get(at))
                .is_none())
    {
        return Err(damaged("a demand's unknown GPU model"));
    }
    Ok(())
}

// AC-T7.4.1: versioned full-demand rows. Legacy amount-only rows cannot reconstruct a class or
// platform safely and are refused. Models are named durably, so table order never changes an ask.
fn demand_text(d: &Demand, models: &[String]) -> Result<String, Stop> {
    check_demand(d, models)?;
    let mut out = String::new();
    write_demand(&mut out, d, models).map_err(|_| damaged("formatting a demand"))?;
    Ok(out)
}

fn write_demand(out: &mut impl std::fmt::Write, d: &Demand, models: &[String]) -> std::fmt::Result {
    let kind = match d.kind {
        Kind::Plain => "plain",
        Kind::Build => "build",
        Kind::Benchmark => "benchmark",
        Kind::Session => "session",
    };
    let class = match d.class {
        Class::Oci => "oci",
        Class::Firecracker => "firecracker",
        Class::Shards => "shards",
    };
    let os = match d.ask.os {
        None => "-",
        Some(Os::Linux) => "linux",
        Some(Os::Macos) => "macos",
        Some(Os::Windows) => "windows",
        Some(Os::Freebsd) => "freebsd",
    };
    let arch = match d.ask.arch {
        None => "-",
        Some(Arch::X86_64) => "x86_64",
        Some(Arch::Aarch64) => "aarch64",
        Some(Arch::X86) => "x86",
        Some(Arch::Arm) => "arm",
        Some(Arch::Riscv64) => "riscv64",
    };
    let model = d
        .ask
        .gpu_model
        .and_then(|m| usize::try_from(m.0).ok().and_then(|at| models.get(at)))
        .map_or("-", String::as_str);
    write!(
        out,
        "d1 {kind} {class} {} {} {} {} {os} {arch} {model} {} {} {} {} {} {}",
        d.need.cpu_milli,
        d.need.mem_bytes,
        d.need.gpus,
        d.need.disk_bytes,
        d.ask.gpu_memory,
        d.ask.vfio,
        d.ask.block,
        d.ask.block_size,
        d.ask.net,
        u8::from(d.ask.kvm)
    )
}

fn demand_of<'a>(
    words: &mut impl Iterator<Item = &'a str>,
    models: &[String],
) -> Result<Demand, Stop> {
    if words.next() != Some("d1") {
        return Err(damaged("a missing or unsupported full-demand version"));
    }
    let kind = match words.next() {
        Some("plain") => Kind::Plain,
        Some("build") => Kind::Build,
        Some("benchmark") => Kind::Benchmark,
        Some("session") => Kind::Session,
        _ => return Err(damaged("a demand's kind")),
    };
    let class = match words.next() {
        Some("oci") => Class::Oci,
        Some("firecracker") => Class::Firecracker,
        Some("shards") => Class::Shards,
        _ => return Err(damaged("a demand's class")),
    };
    let need = Resources {
        cpu_milli: number(words)?,
        mem_bytes: number(words)?,
        gpus: number(words)?,
        disk_bytes: number(words)?,
    };
    let os = match words.next() {
        Some("-") => None,
        Some(w) => Some(Os::named(w).ok_or_else(|| damaged("a demand's OS"))?),
        None => return Err(damaged("a demand's OS")),
    };
    let arch = match words.next() {
        Some("-") => None,
        Some(w) => Some(Arch::named(w).ok_or_else(|| damaged("a demand's architecture"))?),
        None => return Err(damaged("a demand's architecture")),
    };
    let gpu_model = model_id(
        words
            .next()
            .ok_or_else(|| damaged("a demand's GPU model"))?,
        models,
    )?;
    let ask = Ask {
        os,
        arch,
        gpu_model,
        gpu_memory: number(words)?,
        vfio: number(words)?,
        block: number(words)?,
        block_size: number(words)?,
        net: number(words)?,
        kvm: match words.next() {
            Some("0") => false,
            Some("1") => true,
            _ => return Err(damaged("a demand's KVM flag")),
        },
    };
    if words.next().is_some() {
        return Err(damaged("trailing demand fields"));
    }
    Ok(Demand {
        need,
        kind,
        class,
        ask,
    })
}

fn workload(next: &mut u64) -> Result<WorkloadId, Stop> {
    let id = WorkloadId(*next);
    *next = next
        .checked_add(1)
        .ok_or_else(|| damaged("the node's workload ids"))?;
    Ok(id)
}

/// The full-demand representation used by shell envelopes; GPU models are names.
pub fn encode_demand(demand: &Demand, models: &[String]) -> Result<String, Stop> {
    demand_text(demand, models)
}

/// Decodes exactly one full demand, refusing legacy/trailing/unknown-model fields.
pub fn decode_demand(text: &str, models: &[String]) -> Result<Demand, Stop> {
    demand_of(&mut text.split(' '), models)
}

struct BoundedText {
    text: String,
    max: usize,
}
impl std::fmt::Write for BoundedText {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        let len = self
            .text
            .len()
            .checked_add(text.len())
            .ok_or(std::fmt::Error)?;
        if len > self.max {
            return Err(std::fmt::Error);
        }
        self.text
            .try_reserve_exact(text.len())
            .map_err(|_| std::fmt::Error)?;
        self.text.push_str(text);
        Ok(())
    }
}

fn flag<'a>(words: &mut impl Iterator<Item = &'a str>) -> Result<bool, Stop> {
    match words.next() {
        Some("0") => Ok(false),
        Some("1") => Ok(true),
        _ => Err(damaged("a peer message's boolean")),
    }
}

/// CP-T7.4: the one pure peer-message codec. The caller's common-wire byte budget bounds allocation;
/// device models cross the boundary by name, never as a sender-local ModelId.
pub fn encode_message(msg: &Msg, models: &[String], max_bytes: usize) -> Result<String, Stop> {
    let (tag, key) = match msg {
        Msg::Start { key, demand } | Msg::Delete { key, demand } => {
            check_demand(demand, models)?;
            (
                if matches!(msg, Msg::Start { .. }) {
                    "start"
                } else {
                    "delete"
                },
                key,
            )
        }
        Msg::Holds { key } => ("holds", key),
        Msg::Forget { key } => ("forget", key),
        Msg::Started { key } => ("started", key),
        Msg::Refused { key } => ("refused", key),
        Msg::Held {
            key,
            holds,
            ended,
            created,
        } => {
            if (*created && !*holds) || (*ended && (!*holds || !*created)) {
                return Err(damaged("an inconsistent Held message"));
            }
            ("held", key)
        }
        Msg::Ended { key } => ("ended", key),
    };
    let mut out = BoundedText {
        text: String::new(),
        max: max_bytes,
    };
    let full = || Stop::Full {
        what: "peer message bytes",
        bound: max_bytes,
    };
    write!(&mut out, "p1 {tag} {} {} {}", key.owner, key.epoch, key.seq).map_err(|_| full())?;
    match msg {
        Msg::Start { demand, .. } | Msg::Delete { demand, .. } => {
            out.write_char(' ').map_err(|_| full())?;
            write_demand(&mut out, demand, models).map_err(|_| full())?;
        }
        Msg::Held {
            holds,
            ended,
            created,
            ..
        } => write!(
            &mut out,
            " {} {} {}",
            u8::from(*holds),
            u8::from(*ended),
            u8::from(*created)
        )
        .map_err(|_| full())?,
        Msg::Holds { .. }
        | Msg::Forget { .. }
        | Msg::Started { .. }
        | Msg::Refused { .. }
        | Msg::Ended { .. } => {}
    }
    Ok(out.text)
}

/// Refuses oversized, incomplete, legacy and malformed messages before the state machine.
pub fn decode_message(text: &str, models: &[String], max_bytes: usize) -> Result<Msg, Stop> {
    if text.len() > max_bytes {
        return Err(Stop::Full {
            what: "peer message bytes",
            bound: max_bytes,
        });
    }
    let mut words = text.split(' ');
    if words.next() != Some("p1") {
        return Err(damaged("a missing or unsupported peer message version"));
    }
    let tag = words
        .next()
        .ok_or_else(|| damaged("a peer message's tag"))?;
    let key = Key {
        owner: number(&mut words)?,
        epoch: number(&mut words)?,
        seq: number(&mut words)?,
    };
    let msg = match tag {
        "start" | "delete" => {
            let demand = demand_of(&mut words, models)?;
            return Ok(if tag == "start" {
                Msg::Start { key, demand }
            } else {
                Msg::Delete { key, demand }
            });
        }
        "holds" => Msg::Holds { key },
        "forget" => Msg::Forget { key },
        "started" => Msg::Started { key },
        "refused" => Msg::Refused { key },
        "ended" => Msg::Ended { key },
        "held" => {
            let (holds, ended, created) = (flag(&mut words)?, flag(&mut words)?, flag(&mut words)?);
            if (created && !holds) || (ended && (!holds || !created)) {
                return Err(damaged("an inconsistent Held message"));
            }
            Msg::Held {
                key,
                holds,
                ended,
                created,
            }
        }
        _ => return Err(damaged("an unknown peer message tag")),
    };
    if words.next().is_some() {
        return Err(damaged("trailing peer message fields"));
    }
    Ok(msg)
}

// Combined Runtime.create+start observations have a semantic version independent of numeric epoch.
// One cell per node/class bounds storage; no raw sample log, per-key history, or extra Files rows.
fn epoch_text(epoch: u64, rows: &[StartObservation]) -> Result<String, Stop> {
    let mut text = format!("launch1 {epoch} {}", rows.len());
    for row in rows {
        let class = match row.class {
            Class::Oci => "oci",
            Class::Firecracker => "firecracker",
            Class::Shards => "shards",
        };
        write!(
            &mut text,
            " {} {class} {} {}",
            row.node.0, row.sum_ns, row.count
        )
        .map_err(|_| damaged("formatting combined launch observations"))?;
    }
    Ok(text)
}

fn epoch_of(text: &str, nodes: usize) -> Result<(u64, Vec<StartObservation>), Stop> {
    if let Ok(epoch) = text.parse::<u64>() {
        return Ok((epoch, Vec::new()));
    }
    let mut words = text.split(' ');
    if words.next() != Some("launch1") {
        return Err(damaged("the epoch's combined launch observation version"));
    }
    let epoch = number(&mut words)?;
    let count: usize = number(&mut words)?;
    let classes = usize::try_from(crate::model::Classes::ALL.count())
        .map_err(|_| damaged("the runtime class count"))?;
    let bound = nodes
        .checked_mul(classes)
        .ok_or_else(|| damaged("the combined launch observation bound"))?;
    if count > bound {
        return Err(Stop::Full {
            what: "combined launch observation cells",
            bound,
        });
    }
    let mut rows = Vec::with_capacity(count);
    for _ in 0..count {
        let node = NodeId(number(&mut words)?);
        let class = match words.next() {
            Some("oci") => Class::Oci,
            Some("firecracker") => Class::Firecracker,
            Some("shards") => Class::Shards,
            _ => return Err(damaged("a combined launch observation's class")),
        };
        rows.push(StartObservation {
            node,
            class,
            sum_ns: number(&mut words)?,
            count: number(&mut words)?,
        });
    }
    if words.next().is_some() {
        return Err(damaged("trailing combined launch observation cells"));
    }
    Ok((epoch, rows))
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
    let key = Key { owner, epoch, seq };
    if key.name(kind) == name {
        Some((kind, key))
    } else {
        None
    }
}

/// Caller-derived record budgets: the shell uses its runtime/host admission bound. Pending and
/// terminal rows count too, until the owner acknowledges them; overflow is an explicit refusal.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub owned: usize,
    pub ran: usize,
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
    /// Actual receiver admissions, including device ownership, independent of owner guesses.
    admission: FitIndex,
    models: Vec<String>,
    next_workload: u64,
    budget: Budget,
    pick: Pick,
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
        models: &[String],
        budget: Budget,
        pick: Pick,
        records: &mut impl Records,
        runtime: impl Fn(Key) -> Boxed,
    ) -> Result<Node, Stop> {
        let spec = usize::try_from(id.0)
            .ok()
            .and_then(|at| nodes.get(at))
            .filter(|n| n.id == id)
            .ok_or_else(|| damaged("a node not in the cluster"))?;
        let mut names = BTreeSet::new();
        for name in models {
            if name.is_empty()
                || name == "-"
                || crate::model::model_name(name) != *name
                || !names.insert(name)
            {
                return Err(damaged("the shared device model table"));
            }
        }
        for device in nodes.iter().flat_map(|n| &n.devices) {
            if device.model != ModelId::UNNAMED
                && usize::try_from(device.model.0)
                    .ok()
                    .and_then(|at| models.get(at))
                    .is_none()
            {
                return Err(damaged("a node device's unknown model"));
            }
        }
        let mut view = FitIndex::new(nodes).map_err(|e| Stop::Damaged(e.to_string()))?;
        let mut admission = view.clone();
        let (mut epoch, mut next_workload) = (0u64, 0u64);
        let mut epoch_seen = false;
        let mut start_rows = Vec::new();
        let (mut owned, mut ran) = (BTreeMap::new(), BTreeMap::new());
        let known = |n: NodeId| {
            usize::try_from(n.0)
                .ok()
                .and_then(|at| nodes.get(at))
                .is_some_and(|s| s.id == n)
        };
        for (name, value) in records.all()? {
            if name == "epoch" {
                if epoch_seen {
                    return Err(damaged("a repeated epoch row"));
                }
                epoch_seen = true;
                (epoch, start_rows) = epoch_of(&value, nodes.len())?;
                continue;
            }
            let mut words = value.split(' ');
            let (kind, key) = parse_name(&name).ok_or_else(|| damaged("a record's name"))?;
            if !known(NodeId(key.owner)) {
                return Err(damaged("a record's unknown owner"));
            }
            match kind {
                "own" => {
                    if owned.contains_key(&key) {
                        return Err(damaged("a repeated owned row"));
                    }
                    if key.owner != id.0 {
                        return Err(damaged("a foreign owned row"));
                    }
                    let state = match (words.next(), words.next().map(str::parse::<u32>)) {
                        (Some("asked"), Some(Ok(0))) => State::Asked,
                        (Some("at"), Some(Ok(n))) if known(NodeId(n)) => State::At(NodeId(n)),
                        (Some("created"), Some(Ok(n))) if known(NodeId(n)) => {
                            State::Created(NodeId(n))
                        }
                        (Some("done"), Some(Ok(n))) if known(NodeId(n)) => State::Done(NodeId(n)),
                        (Some("deleting"), Some(Ok(n))) if known(NodeId(n)) => {
                            State::Deleting(NodeId(n))
                        }
                        _ => return Err(damaged("an owned box's state")),
                    };
                    let demand = demand_of(&mut words, models)?;
                    let workload = workload(&mut next_workload)?;
                    if let State::At(n) | State::Created(n) | State::Deleting(n) = state {
                        if view.verdict(n, &demand) != Ok(Verdict::Admits) {
                            return Err(damaged("an owned box past its node's admission"));
                        }
                        view.take(n, workload, &demand)
                            .map_err(|e| Stop::Damaged(e.to_string()))?;
                    }
                    if owned.len() >= budget.owned {
                        return Err(Stop::Full {
                            what: "owned records",
                            bound: budget.owned,
                        });
                    }
                    owned.insert(
                        key,
                        Owned {
                            demand,
                            workload,
                            ready: false,
                            state,
                            tried: BTreeSet::new(),
                        },
                    );
                }
                "ran" => {
                    if ran.contains_key(&key) {
                        return Err(damaged("a repeated runner row"));
                    }
                    let owner = NodeId(number(&mut words)?);
                    if owner.0 != key.owner || !known(owner) {
                        return Err(damaged("a record's owner"));
                    }
                    let (created, ended) = match words.next() {
                        Some("creating") => (false, false),
                        Some("running") => (true, false),
                        Some("ended") => (true, true),
                        _ => return Err(damaged("a record's end")),
                    };
                    let demand = demand_of(&mut words, models)?;
                    if ran.len() >= budget.ran {
                        return Err(Stop::Full {
                            what: "runner records",
                            bound: budget.ran,
                        });
                    }
                    ran.insert(
                        key,
                        Ran {
                            owner,
                            demand,
                            workload: workload(&mut next_workload)?,
                            created,
                            ended,
                            at: None,
                        },
                    );
                }
                _ => return Err(damaged("a record's kind")),
            }
        }
        if ran
            .iter()
            .any(|(key, r)| owned.get(key).is_some_and(|o| o.demand != r.demand))
        {
            return Err(damaged("owned and runner rows disagree on the demand"));
        }
        view.restore_start_observations(&start_rows)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        admission
            .restore_start_observations(&start_rows)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        if owned.keys().any(|k| k.epoch > epoch) {
            return Err(damaged("an owned row ahead of its epoch"));
        }
        let epoch = epoch.checked_add(1).ok_or_else(|| damaged("the epoch"))?;
        let mut ledger = lb::Runner::new(lb::RunnerId(id.0), amounts(&spec.capacity), 1);
        let (mut kept, mut keys, mut recovered) = (Vec::new(), Vec::new(), BTreeMap::new());
        for (key, mut r) in ran {
            match (r.ended, runtime(key)) {
                (true, Boxed::Running) => {
                    return Err(damaged("an ended record whose runtime still runs"));
                }
                (false, Boxed::Running) => {
                    if admission.verdict(id, &r.demand) != Ok(Verdict::Admits) {
                        return Err(damaged("a kept box past the node's admission"));
                    }
                    admission
                        .take(id, r.workload, &r.demand)
                        .map_err(|e| Stop::Damaged(e.to_string()))?;
                    kept.push(lb::Kept {
                        owner: lb::OwnerId(r.owner.0),
                        run: lb::Handle::NONE,
                        need: amounts(&r.demand.need),
                    });
                    keys.push(key);
                    if !r.created {
                        r.created = true;
                        records.put(&key.name("ran"), &ran_text(&r, models)?)?;
                    }
                }
                (false, Boxed::Exited) => {
                    r.created = true;
                    r.ended = true;
                    records.put(&key.name("ran"), &ran_text(&r, models)?)?;
                }
                (false, Boxed::Absent) => {
                    records.remove(&key.name("ran"))?;
                    continue;
                }
                (true, _) => {}
            }
            recovered.insert(key, r);
        }
        for (key, at) in keys.iter().zip(ledger.recover(&kept)) {
            let at = at.ok_or_else(|| damaged("a kept box past the node's room"))?;
            if let Some(r) = recovered.get_mut(key) {
                r.at = Some(at);
            }
        }
        records.put("epoch", &epoch_text(epoch, &start_rows)?)?;
        Ok(Node {
            id,
            epoch,
            next: 0,
            ledger,
            view,
            admission,
            models: models.to_vec(),
            next_workload,
            budget,
            pick,
            owned,
            ran: recovered,
            told: Vec::new(),
        })
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    /// Live measured rates feed the same kiln scorer; unobserved terms stay uniform.
    pub fn set_rates(&mut self, node: NodeId, rates: Rates) -> Result<(), Stop> {
        self.view
            .set_rates(node, rates)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        self.admission
            .set_rates(node, rates)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        Ok(())
    }

    /// In-memory measured combined launch update. Actual physical creation uses created_with_start
    /// so the observation and acknowledgment are persisted before publication.
    pub fn observe_start(
        &mut self,
        node: NodeId,
        class: Class,
        elapsed_ns: u64,
    ) -> Result<(), Stop> {
        self.view
            .observe_start(node, class, elapsed_ns)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        self.admission
            .observe_start(node, class, elapsed_ns)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        Ok(())
    }

    /// At most nodes × initialized classes exact aggregate rows, including measured zero duration.
    pub fn start_observations(&self) -> Vec<StartObservation> {
        self.view.start_observations()
    }

    /// Restore the caller's checked aggregate snapshot and persist it in the existing epoch row.
    pub fn restore_start_observations(
        &mut self,
        rows: &[StartObservation],
        records: &mut impl Records,
    ) -> Result<(), Stop> {
        self.view
            .restore_start_observations(rows)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        self.admission
            .restore_start_observations(rows)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        self.persist_starts(records)
    }

    /// Sender-owned facts replace only that peer's cells; retries never add a sample. The local
    /// node is authoritative for its own measurements, so loopback snapshots cannot regress them.
    pub fn merge_peer_start_observations(
        &mut self,
        peer: NodeId,
        rows: &[StartObservation],
        records: &mut impl Records,
    ) -> Result<(), Stop> {
        if usize::try_from(peer.0)
            .ok()
            .is_none_or(|n| n >= self.view.len())
        {
            return Err(damaged("an observation's unknown peer"));
        }
        let bound = usize::try_from(crate::model::Classes::ALL.count())
            .map_err(|_| damaged("the runtime class count"))?;
        if rows.len() > bound {
            return Err(Stop::Full {
                what: "sender-owned combined launch cells",
                bound,
            });
        }
        let mut classes = BTreeSet::new();
        for row in rows {
            if row.node != peer
                || row.count == 0
                || !classes.insert(row.class)
                || row.sum_ns
                    > u128::from(row.count)
                        .checked_mul(u128::from(u64::MAX))
                        .ok_or_else(|| damaged("a combined launch sum"))?
            {
                return Err(damaged("invalid sender-owned combined launch cells"));
            }
        }
        if peer == self.id {
            return Ok(());
        }
        let mut merged = self.start_observations();
        merged.retain(|row| row.node != peer);
        merged.extend_from_slice(rows);
        self.restore_start_observations(&merged, records)
    }

    fn persist_starts(&self, records: &mut impl Records) -> Result<(), Stop> {
        records.put(
            "epoch",
            &epoch_text(self.epoch, &self.start_observations())?,
        )
    }

    /// Feeds the existing scorer/admission with a fresh GPU sample, retaining reservations.
    pub fn set_gpu_load(
        &mut self,
        node: NodeId,
        device: usize,
        load: crate::model::GpuLoad,
    ) -> Result<(), Stop> {
        self.view
            .set_gpu_load(node, device, load)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        self.admission
            .set_gpu_load(node, device, load)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        Ok(())
    }

    /// Updates the same picker from caller-learned policy inputs.
    pub fn set_pick(&mut self, pick: Pick) {
        self.pick = pick;
    }

    /// Refreshes live facts without forgetting reservations. Unsafe capacity/device shrink fails
    /// atomically; the shell must refuse further admission until it resolves incompatible facts.
    pub fn refresh_node(&mut self, spec: NodeSpec) -> Result<(), Stop> {
        for device in &spec.devices {
            if device.model != ModelId::UNNAMED
                && usize::try_from(device.model.0)
                    .ok()
                    .and_then(|at| self.models.get(at))
                    .is_none()
            {
                return Err(damaged("a refreshed device's model"));
            }
        }
        // Both preflights are pure; this node cannot change between them and the updates below.
        self.view
            .check_refresh_node(&spec)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        self.admission
            .check_refresh_node(&spec)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        self.view
            .refresh_node(&spec)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        self.admission
            .refresh_node(&spec)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        if spec.id == self.id {
            self.ledger
                .set_budget(amounts(&spec.capacity), &mut lb::RunnerOut::new());
        }
        Ok(())
    }

    /// The full durable request, for recovery of the shell's enveloped runtime payload.
    pub fn demand(&self, key: Key) -> Option<Demand> {
        self.owned
            .get(&key)
            .map(|o| o.demand)
            .or_else(|| self.ran.get(&key).map(|r| r.demand))
    }

    /// Actual readiness confirmed by the selected peer. Persisted creation is queried again on boot;
    /// admission alone, pending deletion, and ended boxes are never reported as ready.
    /// An unpublished or interrupted Start must not overtake durable deletion intent.
    pub fn is_deleting(&self, key: Key) -> bool {
        self.owned
            .get(&key)
            .is_some_and(|own| matches!(own.state, State::Deleting(_)))
    }

    pub fn is_created(&self, key: Key) -> bool {
        if self.is_ended(key) {
            return false;
        }
        if let Some(o) = self.owned.get(&key) {
            return o.ready && matches!(o.state, State::Created(_));
        }
        self.ran.get(&key).is_some_and(|r| r.created && !r.ended)
    }

    /// Known terminal proof; unknown/forgotten keys return false without retaining new tombstones.
    pub fn is_ended(&self, key: Key) -> bool {
        self.owned
            .get(&key)
            .is_some_and(|o| matches!(o.state, State::Done(_)))
            || self.ran.get(&key).is_some_and(|r| r.ended)
    }

    /// At most Budget::owned entries. None means waiting; Some names the selected peer.
    pub fn owned_keys(&self) -> impl Iterator<Item = (Key, Option<NodeId>)> + '_ {
        self.owned.iter().map(|(&key, o)| {
            (
                key,
                match o.state {
                    State::Asked => None,
                    State::At(n) | State::Created(n) | State::Deleting(n) | State::Done(n) => {
                        Some(n)
                    }
                },
            )
        })
    }

    /// At most Budget::ran entries: (key, actually created, ended), including pending admissions.
    pub fn ran_keys(&self) -> impl Iterator<Item = (Key, bool, bool)> + '_ {
        self.ran.iter().map(|(&key, r)| (key, r.created, r.ended))
    }

    /// A client asks a box: its row first, then its answer, then its place.
    pub fn submit(
        &mut self,
        demand: Demand,
        records: &mut impl Records,
    ) -> Result<(Key, Vec<(NodeId, Msg)>), Stop> {
        check_demand(&demand, &self.models)?;
        if self.owned.len() >= self.budget.owned {
            return Err(Stop::Full {
                what: "owned records",
                bound: self.budget.owned,
            });
        }
        let workload = workload(&mut self.next_workload)?;
        let key = Key {
            owner: self.id.0,
            epoch: self.epoch,
            seq: self.next,
        };
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| damaged("a box number"))?;
        records.put(
            &key.name("own"),
            &own_text(State::Asked, &demand, &self.models)?,
        )?;
        self.owned.insert(
            key,
            Owned {
                demand,
                workload,
                ready: false,
                state: State::Asked,
                tried: BTreeSet::new(),
            },
        );
        let mut out = Vec::new();
        self.place(key, records, &mut out)?;
        Ok((key, out))
    }

    /// CP-T7.4: cancel at its single selected peer. Intent is durable before Delete, and neither
    /// reservation nor ownership is released until that peer proves actual deletion/absence.
    pub fn delete(
        &mut self,
        key: Key,
        records: &mut impl Records,
    ) -> Result<Vec<(NodeId, Msg)>, Stop> {
        if key.owner != self.id.0 {
            return Err(damaged("deletion by a different owner"));
        }
        let Some(o) = self.owned.get(&key) else {
            return Err(damaged("deletion of a box this node does not own"));
        };
        let (state, demand) = (o.state, o.demand);
        let mut out = Vec::new();
        match state {
            State::Asked => {
                records.put(
                    &key.name("own"),
                    &own_text(State::Done(self.id), &demand, &self.models)?,
                )?;
                if let Some(o) = self.owned.get_mut(&key) {
                    o.state = State::Done(self.id);
                }
                self.ended(key, self.id, records, &mut out)?;
            }
            State::At(peer) | State::Created(peer) => {
                records.put(
                    &key.name("own"),
                    &own_text(State::Deleting(peer), &demand, &self.models)?,
                )?;
                if let Some(o) = self.owned.get_mut(&key) {
                    o.state = State::Deleting(peer);
                }
                out.push((peer, Msg::Delete { key, demand }));
            }
            State::Deleting(peer) => out.push((peer, Msg::Delete { key, demand })),
            State::Done(peer) => self.ended(key, peer, records, &mut out)?,
        }
        Ok(out)
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
        let d = o.demand;
        let fit = self
            .view
            .fit_with_terms_excluding(&d, &self.pick, &Terms::default(), &o.tried)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        let Some(best) = fit.best else {
            return Ok(());
        };
        let (demand, to, workload) = (o.demand, best.node, o.workload);
        records.put(
            &key.name("own"),
            &own_text(State::At(to), &demand, &self.models)?,
        )?;
        self.view
            .take(to, workload, &d)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        if let Some(o) = self.owned.get_mut(&key) {
            o.state = State::At(to);
        }
        out.push((to, Msg::Start { key, demand }));
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
        if o.state == State::Deleting(from) {
            // A missing core row (Held false) is not runtime absence proof for deletion.
            out.push((
                from,
                Msg::Delete {
                    key,
                    demand: o.demand,
                },
            ));
            return Ok(());
        }
        if !matches!(o.state, State::At(n) | State::Created(n) if n == from) {
            return Ok(());
        }
        let (demand, workload) = (o.demand, o.workload);
        records.put(
            &key.name("own"),
            &own_text(State::Asked, &demand, &self.models)?,
        )?;
        self.view
            .give(from, workload, &demand)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        if let Some(o) = self.owned.get_mut(&key) {
            o.state = State::Asked;
            o.ready = false;
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
        let (demand, workload) = (o.demand, o.workload);
        match o.state {
            State::At(n) | State::Created(n) | State::Deleting(n) if n == from => {
                records.put(
                    &key.name("own"),
                    &own_text(State::Done(from), &demand, &self.models)?,
                )?;
                self.view
                    .give(from, workload, &demand)
                    .map_err(|e| Stop::Damaged(e.to_string()))?;
            }
            State::Done(n) if n == from => {}
            State::Asked
            | State::At(_)
            | State::Created(_)
            | State::Deleting(_)
            | State::Done(_) => return Ok(()),
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

    fn started(&mut self, key: Key, from: NodeId, records: &mut impl Records) -> Result<(), Stop> {
        let Some(o) = self.owned.get(&key) else {
            return Ok(());
        };
        match o.state {
            State::At(peer) if peer == from => records.put(
                &key.name("own"),
                &own_text(State::Created(from), &o.demand, &self.models)?,
            )?,
            State::Created(peer) if peer == from && !o.ready => {}
            State::Asked
            | State::At(_)
            | State::Created(_)
            | State::Deleting(_)
            | State::Done(_) => return Ok(()),
        }
        if let Some(o) = self.owned.get_mut(&key) {
            o.state = State::Created(from);
            o.ready = true;
        }
        self.told.push(Told::Started(key, from));
        Ok(())
    }

    /// A word from node `from`.
    pub fn hear(
        &mut self,
        from: NodeId,
        msg: Msg,
        records: &mut impl Records,
    ) -> Result<Vec<(NodeId, Msg)>, Stop> {
        if usize::try_from(from.0)
            .ok()
            .is_none_or(|n| n >= self.view.len())
        {
            return Err(damaged("a message's unknown peer"));
        }
        let (key, owner_message) = match msg {
            Msg::Start { key, .. }
            | Msg::Delete { key, .. }
            | Msg::Holds { key }
            | Msg::Forget { key } => (key, true),
            Msg::Started { key }
            | Msg::Refused { key }
            | Msg::Held { key, .. }
            | Msg::Ended { key } => (key, false),
        };
        if key.owner != if owner_message { from.0 } else { self.id.0 } {
            return Err(damaged("a message's box owner"));
        }
        let mut out = Vec::new();
        match msg {
            Msg::Start { key, demand } => self.start(from, key, demand, records, &mut out)?,
            Msg::Delete { key, demand } => {
                check_demand(&demand, &self.models)?;
                if self
                    .ran
                    .get(&key)
                    .is_some_and(|r| r.demand != demand || r.owner != from)
                {
                    return Err(damaged("Delete disagrees with its admitted demand"));
                }
                if self.ran.get(&key).is_some_and(|r| r.ended) {
                    out.push((from, Msg::Ended { key }));
                } else {
                    self.told.retain(
                        |t| !matches!(t, Told::Create(k, _) | Told::Delete(k, _) if *k == key),
                    );
                    self.told.push(Told::Delete(key, demand));
                }
            }
            Msg::Holds { key } => {
                let r = self.ran.get(&key);
                out.push((
                    from,
                    Msg::Held {
                        key,
                        holds: r.is_some(),
                        ended: r.is_some_and(|r| r.ended),
                        created: r.is_some_and(|r| r.created),
                    },
                ));
            }
            Msg::Forget { key } => {
                if self.ran.get(&key).is_some_and(|r| r.ended) {
                    records.remove(&key.name("ran"))?;
                    self.ran.remove(&key);
                }
            }
            Msg::Started { key } => self.started(key, from, records)?,
            Msg::Refused { key }
            | Msg::Held {
                key, holds: false, ..
            } => self.not_there(key, from, records, &mut out)?,
            Msg::Held {
                key,
                holds: true,
                ended: true,
                ..
            }
            | Msg::Ended { key } => self.ended(key, from, records, &mut out)?,
            Msg::Held {
                key,
                holds: true,
                ended: false,
                created: true,
            } => self.started(key, from, records)?,
            Msg::Held {
                holds: true,
                ended: false,
                created: false,
                ..
            } => {}
        }
        Ok(out)
    }

    /// A start from `owner`: one the node holds is answered as it stands; else admitted by the ledger,
    /// its record written, then the box created.
    fn start(
        &mut self,
        owner: NodeId,
        key: Key,
        demand: Demand,
        records: &mut impl Records,
        out: &mut Vec<(NodeId, Msg)>,
    ) -> Result<(), Stop> {
        if owner.0 != key.owner
            || usize::try_from(owner.0)
                .ok()
                .is_none_or(|n| n >= self.view.len())
        {
            return Err(damaged("a start's owner"));
        }
        check_demand(&demand, &self.models)?;
        if self.owned.get(&key).is_some_and(|o| o.demand != demand) {
            return Err(damaged("a start disagrees with its owned demand"));
        }
        if let Some(r) = self.ran.get(&key) {
            if r.owner != owner || r.demand != demand {
                return Err(damaged("a start reuses a key with a different demand"));
            }
            out.push((
                owner,
                if r.ended {
                    Msg::Ended { key }
                } else if r.created {
                    Msg::Started { key }
                } else {
                    Msg::Held {
                        key,
                        holds: true,
                        ended: false,
                        created: false,
                    }
                },
            ));
            return Ok(());
        }
        if self.ran.len() >= self.budget.ran
            || self.admission.verdict(self.id, &demand) != Ok(Verdict::Admits)
        {
            out.push((owner, Msg::Refused { key }));
            return Ok(());
        }
        let start = lb::Start {
            owner: lb::OwnerId(owner.0),
            run: lb::Handle::NONE,
            epoch: self.ledger.epoch(),
            need: amounts(&demand.need),
        };
        let lb::ToOwner::Started { at, .. } = self.ledger.start(start) else {
            out.push((owner, Msg::Refused { key }));
            return Ok(());
        };
        let workload = workload(&mut self.next_workload)?;
        self.admission
            .take(self.id, workload, &demand)
            .map_err(|e| Stop::Damaged(e.to_string()))?;
        let r = Ran {
            owner,
            demand,
            workload,
            created: false,
            at: Some(at),
            ended: false,
        };
        records.put(&key.name("ran"), &ran_text(&r, &self.models)?)?;
        self.ran.insert(key, r);
        self.told.push(Told::Create(key, demand));
        Ok(())
    }

    /// Actual whole opaque Runtime.create+start elapsed time (including any staging). One full key
    /// is sampled at most once: durable aggregate first, then Running, then Started. After a crash
    /// between writes, boot proves runtime ownership and never re-observes a recovered box.
    pub fn created_with_start(
        &mut self,
        key: Key,
        elapsed_ns: u64,
        records: &mut impl Records,
    ) -> Result<Vec<(NodeId, Msg)>, Stop> {
        let Some(r) = self.ran.get(&key).copied() else {
            return Err(damaged("creation without an admission"));
        };
        if !r.created && !r.ended {
            self.observe_start(self.id, r.demand.class, elapsed_ns)?;
            self.persist_starts(records)?;
        }
        self.created(key, records)
    }

    /// Runtime feedback, after actual successful creation. Admission alone never publishes Started.
    pub fn created(
        &mut self,
        key: Key,
        records: &mut impl Records,
    ) -> Result<Vec<(NodeId, Msg)>, Stop> {
        let Some(mut r) = self.ran.get(&key).copied() else {
            return Err(damaged("creation without an admission"));
        };
        if r.ended {
            return Ok(vec![(r.owner, Msg::Ended { key })]);
        }
        if !r.created {
            r.created = true;
            records.put(&key.name("ran"), &ran_text(&r, &self.models)?)?;
            self.ran.insert(key, r);
        }
        Ok(vec![(r.owner, Msg::Started { key })])
    }

    /// Runtime refused before creating anything: durable removal permits this owner's retry.
    pub fn create_refused(
        &mut self,
        key: Key,
        records: &mut impl Records,
    ) -> Result<Vec<(NodeId, Msg)>, Stop> {
        let Some(r) = self.ran.get(&key).copied() else {
            return Ok(Vec::new());
        };
        if r.created || r.ended {
            return Err(damaged("creation refusal after creation"));
        }
        records.remove(&key.name("ran"))?;
        self.release(&r)?;
        self.ran.remove(&key);
        Ok(vec![(r.owner, Msg::Refused { key })])
    }

    fn release(&mut self, r: &Ran) -> Result<(), Stop> {
        if let Some(at) = r.at {
            let mut out = lb::RunnerOut::new();
            self.ledger
                .end(at, &mut out)
                .ok_or_else(|| damaged("a box the ledger didn't hold"))?;
            self.admission
                .give(self.id, r.workload, &r.demand)
                .map_err(|e| Stop::Damaged(e.to_string()))?;
        }
        Ok(())
    }

    /// The box `key` running here ended (it exited, or was deleted): its room freed, its end written,
    /// its owner told.
    pub fn end(
        &mut self,
        key: Key,
        records: &mut impl Records,
    ) -> Result<Vec<(NodeId, Msg)>, Stop> {
        if usize::try_from(key.owner)
            .ok()
            .is_none_or(|n| n >= self.view.len())
        {
            return Err(damaged("an end's unknown owner"));
        }
        let Some(r) = self.ran.get(&key).copied() else {
            // Only the runtime shell calls end: no record alone is not absence proof.
            return Ok(vec![(NodeId(key.owner), Msg::Ended { key })]);
        };
        if r.ended {
            return Ok(vec![(r.owner, Msg::Ended { key })]);
        }
        let ended = Ran {
            created: true,
            ended: true,
            ..r
        };
        records.put(&key.name("ran"), &ran_text(&ended, &self.models)?)?;
        self.release(&r)?;
        if let Some(r) = self.ran.get_mut(&key) {
            r.created = true;
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
                State::At(n) | State::Created(n) if n == peer => {
                    out.push((peer, Msg::Holds { key: *key }))
                }
                State::Done(n) if n == peer => done.push(*key),
                State::Deleting(n) if n == peer => out.push((
                    peer,
                    Msg::Delete {
                        key: *key,
                        demand: o.demand,
                    },
                )),
                State::Asked
                | State::At(_)
                | State::Created(_)
                | State::Deleting(_)
                | State::Done(_) => {}
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

fn own_text(state: State, demand: &Demand, models: &[String]) -> Result<String, Stop> {
    let (word, n) = match state {
        State::Asked => ("asked", 0),
        State::At(n) => ("at", n.0),
        State::Created(n) => ("created", n.0),
        State::Deleting(n) => ("deleting", n.0),
        State::Done(n) => ("done", n.0),
    };
    Ok(format!("{word} {n} {}", demand_text(demand, models)?))
}

fn ran_text(r: &Ran, models: &[String]) -> Result<String, Stop> {
    let state = if r.ended {
        "ended"
    } else if r.created {
        "running"
    } else {
        "creating"
    };
    Ok(format!(
        "{} {state} {}",
        r.owner.0,
        demand_text(&r.demand, models)?
    ))
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
    models: Vec<String>,
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
        Self::with_models(specs, Vec::new(), records, seed)
    }

    pub fn with_models(
        specs: Vec<NodeSpec>,
        models: Vec<String>,
        records: Vec<R>,
        seed: u64,
    ) -> Result<Self, Stop> {
        let n = specs.len();
        if records.len() != n || n == 0 {
            return Err(damaged("a node's records each"));
        }
        let mut w = World {
            nodes: (0..n).map(|_| None).collect(),
            records: records.into_iter().map(Crashing::new).collect(),
            links: (0..n.saturating_mul(n)).map(|_| Link::default()).collect(),
            specs,
            models,
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
                Told::Create(key, _) => {
                    let c = self.created.entry(key).or_insert(0);
                    *c = c.saturating_add(1);
                    if *c > 1 {
                        self.report.double = self.report.double.saturating_add(1);
                    }
                    self.report.started = self.report.started.saturating_add(1);
                    self.boxes.insert(key, (id, Boxed::Running));
                    let feedback = match (
                        self.nodes.get_mut(i).and_then(Option::as_mut),
                        self.records.get_mut(i),
                    ) {
                        (Some(node), Some(rec)) => node.created(key, rec),
                        _ => return Err(damaged("runtime feedback to a missing node")),
                    };
                    match feedback {
                        Ok(out) => self.send(i, out),
                        Err(Stop::Crashed) => {
                            self.crash(i);
                            return Ok(());
                        }
                        Err(e) => return Err(e),
                    }
                }
                Told::Delete(key, _) => {
                    if self.boxes.get(&key).is_some_and(|(at, _)| *at == id) {
                        self.boxes.insert(key, (id, Boxed::Exited));
                    }
                    let feedback = match (
                        self.nodes.get_mut(i).and_then(Option::as_mut),
                        self.records.get_mut(i),
                    ) {
                        (Some(node), Some(rec)) => node.end(key, rec),
                        _ => return Err(damaged("deletion feedback to a missing node")),
                    };
                    match feedback {
                        Ok(out) => self.send(i, out),
                        Err(Stop::Crashed) => {
                            self.crash(i);
                            return Ok(());
                        }
                        Err(e) => return Err(e),
                    }
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
        let owned = self
            .specs
            .iter()
            .try_fold(0u64, |sum, n| sum.checked_add(n.capacity.cpu_milli))
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| damaged("the world's record budget"))?;
        let ran = self
            .specs
            .get(i)
            .and_then(|n| usize::try_from(n.capacity.cpu_milli).ok())
            .ok_or_else(|| damaged("the world's runner budget"))?;
        let pick = Pick {
            scorer: Scorer::HEX,
            ..Pick::default()
        };
        match Node::boot(
            id,
            &self.specs,
            &self.models,
            Budget { owned, ran },
            pick,
            rec,
            runtime,
        ) {
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
        match node.submit(plain(need), rec) {
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

    fn test_budget() -> Budget {
        let rows = usize::try_from(CAP.cpu_milli).unwrap();
        Budget {
            owned: rows,
            ran: rows,
        }
    }

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

    /// AC-T7.4.1: receiver admission must enforce the runtime class, independently of the owner.
    #[test]
    fn a_receiver_refuses_oci_on_a_shards_only_macos_laptop() {
        let mut spec = NodeSpec::new(NodeId(0), CAP);
        spec.platform = crate::model::Platform {
            os: crate::model::Os::Macos,
            arch: crate::model::Arch::Aarch64,
        };
        spec.classes = crate::model::Classes::NONE.with(crate::model::Class::Shards);
        let mut records = Memory::default();
        let mut node = Node::boot(
            NodeId(0),
            &[spec],
            &[],
            test_budget(),
            Pick::default(),
            &mut records,
            |_| Boxed::Absent,
        )
        .unwrap();
        let key = Key {
            owner: 0,
            epoch: 1,
            seq: 0,
        };
        let out = node
            .hear(
                NodeId(0),
                Msg::Start {
                    key,
                    demand: plain(Resources {
                        cpu_milli: 1,
                        ..Resources::default()
                    }),
                },
                &mut records,
            )
            .unwrap();
        assert_eq!(out, vec![(NodeId(0), Msg::Refused { key })]);
        assert!(node.told.is_empty());
        assert_eq!(node.admitted(), [0; lb::DIMS]);
    }

    fn boot_test(specs: &[NodeSpec], models: &[String], records: &mut Memory) -> Node {
        Node::boot(
            NodeId(0),
            specs,
            models,
            test_budget(),
            Pick {
                scorer: Scorer::HEX,
                ..Pick::default()
            },
            records,
            |_| Boxed::Absent,
        )
        .unwrap()
    }

    fn requested() -> Demand {
        plain(Resources {
            cpu_milli: 1,
            ..Resources::default()
        })
    }
    fn other_key(seq: u64) -> Key {
        Key {
            owner: 0,
            epoch: 1,
            seq,
        }
    }
    fn start(
        node: &mut Node,
        key: Key,
        demand: Demand,
        records: &mut Memory,
    ) -> Vec<(NodeId, Msg)> {
        node.hear(NodeId(0), Msg::Start { key, demand }, records)
            .unwrap()
    }

    /// AC-T7.4.1: a real macOS/arm64 Shards-only laptop uses the same peer path, and admission
    /// publishes no Started until the runtime confirms successful creation.
    #[test]
    fn full_demand_macos_shards_laptop_waits_for_runtime_creation() {
        let mut spec = NodeSpec::new(NodeId(0), CAP);
        spec.platform = crate::model::Platform {
            os: Os::Macos,
            arch: Arch::Aarch64,
        };
        spec.virt = crate::model::Virt {
            hvf: true,
            ..crate::model::Virt::default()
        };
        spec.classes = spec.platform.os.classes(spec.virt);
        let mut records = Memory::default();
        let mut node = boot_test(&[spec], &[], &mut records);
        let demand = Demand {
            kind: Kind::Session,
            class: Class::Shards,
            ask: Ask {
                os: Some(Os::Linux),
                arch: Some(Arch::Aarch64),
                ..Ask::default()
            },
            ..requested()
        };
        let (key, out) = node.submit(demand, &mut records).unwrap();
        assert_eq!(out, vec![(NodeId(0), Msg::Start { key, demand })]);
        assert!(start(&mut node, key, demand, &mut records).is_empty());
        assert_eq!(
            std::mem::take(&mut node.told),
            vec![Told::Create(key, demand)]
        );
        let held = node
            .hear(NodeId(0), Msg::Holds { key }, &mut records)
            .unwrap();
        assert_eq!(
            held,
            vec![(
                NodeId(0),
                Msg::Held {
                    key,
                    holds: true,
                    ended: false,
                    created: false
                }
            )]
        );
        node.hear(NodeId(0), held[0].1, &mut records).unwrap();
        assert!(node.told.is_empty());
        let out = node.created(key, &mut records).unwrap();
        assert_eq!(out, vec![(NodeId(0), Msg::Started { key })]);
        node.hear(NodeId(0), out[0].1, &mut records).unwrap();
        assert_eq!(
            std::mem::take(&mut node.told),
            vec![Told::Started(key, NodeId(0))]
        );
        assert_eq!(node.demand(key), Some(demand));
    }

    /// AC-T7.4.1: receiving nodes recheck hard filters even when a stale/malicious owner sends Start.
    #[test]
    fn full_demand_receiver_rechecks_os_arch_and_class() {
        let mut spec = NodeSpec::new(NodeId(0), CAP);
        spec.classes = crate::model::Classes::NONE.with(Class::Oci);
        let mut records = Memory::default();
        let mut node = boot_test(&[spec], &[], &mut records);
        for (seq, demand) in [
            Demand {
                ask: Ask {
                    os: Some(Os::Windows),
                    ..Ask::default()
                },
                ..requested()
            },
            Demand {
                ask: Ask {
                    arch: Some(Arch::Aarch64),
                    ..Ask::default()
                },
                ..requested()
            },
            Demand {
                class: Class::Shards,
                ..requested()
            },
        ]
        .into_iter()
        .enumerate()
        {
            let key = other_key(u64::try_from(seq).unwrap());
            assert_eq!(
                start(&mut node, key, demand, &mut records),
                vec![(NodeId(0), Msg::Refused { key })]
            );
        }
        assert!(node.told.is_empty());
        assert_eq!(node.records(), 0);
        assert_eq!(node.admitted(), [0; lb::DIMS]);
    }

    /// AC-T7.4.1: VFIO functions are exclusive even though the amounts ledger has no VFIO dimension.
    /// Keys that collided under epoch<<40 XOR seq must release only their own device.
    #[test]
    fn full_demand_receiver_reserves_devices_and_uses_lossless_local_ids() {
        let mut spec = NodeSpec::new(NodeId(0), CAP);
        spec.devices = vec![
            crate::model::Device {
                kind: crate::model::DeviceKind::Vfio,
                model: ModelId::UNNAMED,
                size: 0,
                load: crate::model::GpuLoad::default()
            };
            2
        ];
        let mut records = Memory::default();
        let mut node = boot_test(&[spec], &[], &mut records);
        let demand = Demand {
            ask: Ask {
                vfio: 1,
                ..Ask::default()
            },
            ..requested()
        };
        let a = Key {
            owner: 0,
            epoch: 1,
            seq: 0,
        };
        let b = Key {
            owner: 0,
            epoch: 0,
            seq: 1 << 40,
        };
        assert!(start(&mut node, a, demand, &mut records).is_empty());
        assert!(start(&mut node, b, demand, &mut records).is_empty());
        node.created(a, &mut records).unwrap();
        node.created(b, &mut records).unwrap();
        let c = other_key(2);
        assert_eq!(
            start(&mut node, c, demand, &mut records),
            vec![(NodeId(0), Msg::Refused { key: c })]
        );
        node.end(a, &mut records).unwrap();
        assert!(start(&mut node, c, demand, &mut records).is_empty());
        assert_eq!(
            start(&mut node, other_key(3), demand, &mut records),
            vec![(NodeId(0), Msg::Refused { key: other_key(3) })]
        );
        node.end(b, &mut records).unwrap();
        node.create_refused(c, &mut records).unwrap();
        assert_eq!(node.admitted(), [0; lb::DIMS]);
        assert!(start(&mut node, other_key(4), demand, &mut records).is_empty());
    }

    /// AC-T7.4.1: all fields survive own/ran records and reboot, including a remapped shared model
    /// table. A recovered runtime hold still owns its GPU/VFIO/block/network devices.
    #[test]
    fn full_demand_reboot_preserves_every_constraint_and_device_hold() {
        use crate::model::{Device, DeviceKind, GpuLoad};
        let models = vec!["rtx-5060-ti".to_owned(), "other".to_owned()];
        let mut spec = NodeSpec::new(NodeId(0), Resources { gpus: 2, ..CAP });
        spec.devices = vec![
            Device::gpu(ModelId(0), 16 << 30),
            Device::gpu(ModelId(0), 16 << 30),
        ];
        spec.devices.extend(
            [DeviceKind::Vfio, DeviceKind::Block, DeviceKind::Net].map(|kind| Device {
                kind,
                model: ModelId::UNNAMED,
                size: 1 << 30,
                load: GpuLoad::default(),
            }),
        );
        let demand = Demand {
            kind: Kind::Session,
            need: Resources {
                gpus: 1,
                ..requested().need
            },
            ask: Ask {
                os: Some(Os::Linux),
                arch: Some(Arch::X86_64),
                gpu_model: Some(ModelId(0)),
                gpu_memory: 8 << 30,
                vfio: 1,
                block: 1,
                block_size: 1 << 20,
                net: 1,
                kvm: true,
            },
            ..requested()
        };
        let mut records = Memory::default();
        let mut node = boot_test(&[spec.clone()], &models, &mut records);
        let (key, _) = node.submit(demand, &mut records).unwrap();
        assert!(start(&mut node, key, demand, &mut records).is_empty());
        let reordered = vec![models[1].clone(), models[0].clone()];
        for d in &mut spec.devices {
            if d.model == ModelId(0) {
                d.model = ModelId(1);
            }
        }
        let mut expected = demand;
        expected.ask.gpu_model = Some(ModelId(1));
        let mut node = Node::boot(
            NodeId(0),
            &[spec],
            &reordered,
            test_budget(),
            Pick::default(),
            &mut records,
            |k| {
                if k == key {
                    Boxed::Running
                } else {
                    Boxed::Absent
                }
            },
        )
        .unwrap();
        assert_eq!(node.demand(key), Some(expected));
        assert_eq!(
            node.ran_keys().collect::<Vec<_>>(),
            vec![(key, true, false)]
        );
        assert_eq!(
            start(&mut node, other_key(1), expected, &mut records),
            vec![(NodeId(0), Msg::Refused { key: other_key(1) })]
        );
        let ended = node.end(key, &mut records).unwrap();
        node.hear(NodeId(0), ended[0].1, &mut records).unwrap();
        assert_eq!(node.owned(), 0);
        assert_eq!(node.admitted(), [0; lb::DIMS]);
        assert!(start(&mut node, other_key(2), expected, &mut records).is_empty());
    }

    /// AC-T7.4.1: a runtime refusal frees the actual devices and retains the owner's complete ask
    /// while moving it to another peer. Silence/pending admission never permits replacement.
    #[test]
    fn full_demand_runtime_refusal_retries_without_erasing_constraints() {
        let specs = vec![NodeSpec::new(NodeId(0), CAP), NodeSpec::new(NodeId(1), CAP)];
        let mut own_records = Memory::default();
        let mut run_records = Memory::default();
        let mut owner = boot_test(&specs, &[], &mut own_records);
        let mut receiver = boot_test(&specs, &[], &mut run_records);
        let demand = Demand {
            kind: Kind::Build,
            ask: Ask {
                os: Some(Os::Linux),
                arch: Some(Arch::X86_64),
                kvm: true,
                ..Ask::default()
            },
            ..requested()
        };
        let (key, first) = owner.submit(demand, &mut own_records).unwrap();
        assert_eq!(first[0].0, NodeId(0));
        assert!(
            receiver
                .hear(NodeId(0), first[0].1, &mut run_records)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            owner.link_up(NodeId(0), &mut own_records).unwrap(),
            vec![(NodeId(0), Msg::Holds { key })]
        );
        let refused = receiver.create_refused(key, &mut run_records).unwrap();
        assert_eq!(receiver.admitted(), [0; lb::DIMS]);
        let retry = owner
            .hear(NodeId(0), refused[0].1, &mut own_records)
            .unwrap();
        assert_eq!(retry, vec![(NodeId(1), Msg::Start { key, demand })]);
        assert_eq!(owner.demand(key), Some(demand));
    }

    /// AC-T7.4.1: peer placement uses the existing GPU-load scorer, not the lowest raw capacity key.
    #[test]
    fn full_demand_gpu_load_selects_the_idle_peer() {
        let mut specs = vec![
            NodeSpec::new(NodeId(0), Resources { gpus: 1, ..CAP }),
            NodeSpec::new(NodeId(1), Resources { gpus: 1, ..CAP }),
        ];
        specs[0].devices[0].load.busy_per_mille = 1000;
        let mut records = Memory::default();
        let mut node = boot_test(&specs, &[], &mut records);
        let demand = Demand {
            need: Resources {
                gpus: 1,
                ..requested().need
            },
            ..requested()
        };
        let (key, out) = node.submit(demand, &mut records).unwrap();
        assert_eq!(out[0].0, NodeId(1));
        node.hear(NodeId(1), Msg::Ended { key }, &mut records)
            .unwrap();
        node.set_gpu_load(NodeId(0), 0, crate::model::GpuLoad::default())
            .unwrap();
        node.set_gpu_load(
            NodeId(1),
            0,
            crate::model::GpuLoad {
                busy_per_mille: 1000,
                ..crate::model::GpuLoad::default()
            },
        )
        .unwrap();
        node.set_pick(Pick::default());
        assert_eq!(node.submit(demand, &mut records).unwrap().1[0].0, NodeId(0));
    }

    /// AC-T7.4.1: full-demand rows reject legacy/trailing/unknown-model fields rather than silently
    /// converting an incomplete request into default OCI. Caller budgets count pending rows.
    #[test]
    fn full_demand_codec_and_record_budgets_refuse_invalid_input() {
        let demand = requested();
        let text = demand_text(&demand, &[]).unwrap();
        assert_eq!(demand_of(&mut text.split(' '), &[]).unwrap(), demand);
        for text in [
            "1 1 0 0".to_owned(),
            format!("{text} trailing"),
            text.replace(" - 0 0", " unknown-model 0 0"),
        ] {
            assert!(matches!(
                demand_of(&mut text.split(' '), &[]),
                Err(Stop::Damaged(_))
            ));
        }
        let specs = vec![NodeSpec::new(NodeId(0), CAP)];
        let mut records = Memory::default();
        let mut node = Node::boot(
            NodeId(0),
            &specs,
            &[],
            Budget { owned: 1, ran: 1 },
            Pick::default(),
            &mut records,
            |_| Boxed::Absent,
        )
        .unwrap();
        let (key, _) = node.submit(demand, &mut records).unwrap();
        assert!(matches!(
            node.submit(demand, &mut records),
            Err(Stop::Full {
                what: "owned records",
                bound: 1
            })
        ));
        assert!(start(&mut node, key, demand, &mut records).is_empty());
        assert_eq!(
            start(&mut node, other_key(1), demand, &mut records),
            vec![(NodeId(0), Msg::Refused { key: other_key(1) })]
        );
    }

    /// CP-T7.4: crash before/after runtime-created publication; recovery never creates twice.
    #[test]
    fn full_demand_runtime_feedback_survives_each_created_write_crash() {
        let specs = vec![NodeSpec::new(NodeId(0), CAP)];
        for when in [When::Before, When::After] {
            let mut rec = Crashing::new(Memory::default());
            let mut node = Node::boot(
                NodeId(0),
                &specs,
                &[],
                test_budget(),
                Pick::default(),
                &mut rec,
                |_| Boxed::Absent,
            )
            .unwrap();
            let (key, _) = node.submit(requested(), &mut rec).unwrap();
            assert!(
                node.hear(
                    NodeId(0),
                    Msg::Start {
                        key,
                        demand: requested()
                    },
                    &mut rec
                )
                .unwrap()
                .is_empty()
            );
            assert_eq!(
                std::mem::take(&mut node.told),
                vec![Told::Create(key, requested())]
            );
            rec.armed = Some(when);
            assert_eq!(node.created(key, &mut rec), Err(Stop::Crashed));
            let mut node = Node::boot(
                NodeId(0),
                &specs,
                &[],
                test_budget(),
                Pick::default(),
                &mut rec,
                |_| Boxed::Running,
            )
            .unwrap();
            assert!(
                node.told.is_empty(),
                "recovery re-created a runtime-owned box: {when:?}"
            );
            assert_eq!(
                node.ran_keys().collect::<Vec<_>>(),
                vec![(key, true, false)]
            );
            let held = node.hear(NodeId(0), Msg::Holds { key }, &mut rec).unwrap();
            node.hear(NodeId(0), held[0].1, &mut rec).unwrap();
            assert_eq!(
                std::mem::take(&mut node.told),
                vec![Told::Started(key, NodeId(0))]
            );
            let end = node.end(key, &mut rec).unwrap();
            node.hear(NodeId(0), end[0].1, &mut rec).unwrap();
            assert_eq!(node.admitted(), [0; lb::DIMS]);
            assert_eq!(node.owned(), 0);
            assert_eq!(rec.crashes, 1);
        }
    }

    /// CP-T7.4: wire variants roundtrip, names remap, full-width ids and exact byte bounds survive.
    #[test]
    fn full_demand_peer_codec_is_bounded_complete_and_name_based() {
        let models = vec!["gpu-a".to_owned(), "gpu-b".to_owned()];
        let demand = Demand {
            ask: Ask {
                gpu_model: Some(ModelId(0)),
                ..Ask::default()
            },
            ..requested()
        };
        let key = Key {
            owner: u32::MAX,
            epoch: u64::MAX,
            seq: u64::MAX,
        };
        let budget = format!(
            "p1 delete {} {} {} {}",
            key.owner,
            key.epoch,
            key.seq,
            encode_demand(&demand, &models).unwrap()
        )
        .len();
        for msg in [
            Msg::Start { key, demand },
            Msg::Delete { key, demand },
            Msg::Holds { key },
            Msg::Forget { key },
            Msg::Started { key },
            Msg::Refused { key },
            Msg::Held {
                key,
                holds: true,
                ended: false,
                created: false,
            },
            Msg::Held {
                key,
                holds: true,
                ended: true,
                created: true,
            },
            Msg::Ended { key },
        ] {
            let text = encode_message(&msg, &models, budget).unwrap();
            assert_eq!(decode_message(&text, &models, text.len()).unwrap(), msg);
            assert!(matches!(
                encode_message(&msg, &models, text.len() - 1),
                Err(Stop::Full { .. })
            ));
            assert!(matches!(
                decode_message(&text, &models, text.len() - 1),
                Err(Stop::Full { .. })
            ));
            assert!(matches!(
                decode_message(
                    &format!("{text} extra"),
                    &models,
                    text.len() + " extra".len()
                ),
                Err(Stop::Damaged(_))
            ));
        }
        let text = encode_message(&Msg::Start { key, demand }, &models, budget).unwrap();
        let swapped = vec![models[1].clone(), models[0].clone()];
        let mut remapped = demand;
        remapped.ask.gpu_model = Some(ModelId(1));
        assert_eq!(
            decode_message(&text, &swapped, text.len()).unwrap(),
            Msg::Start {
                key,
                demand: remapped
            }
        );
        for bad in [
            "p1 held 0 1 0 0 1 1",
            "p1 held 0 1 0 1 0 2",
            "p1 holds 4294967296 1 0",
            "p1 holds 0 18446744073709551616 0",
            "p1 whatever 0 1 0",
            "holds 0 1 0",
        ] {
            assert!(
                matches!(
                    decode_message(bad, &models, bad.len()),
                    Err(Stop::Damaged(_))
                ),
                "{bad}"
            );
        }
    }

    /// AC-T7.4.1: refresh preserves both reservations and their exclusive devices; unsafe shrink is
    /// atomic, while changed classes/platform filter subsequent admissions through the same core.
    #[test]
    fn full_demand_live_facts_refresh_preserves_reservations_and_filters() {
        use crate::model::{Device, DeviceKind, GpuLoad};
        let mut spec = NodeSpec::new(NodeId(0), CAP);
        spec.devices = vec![Device {
            kind: DeviceKind::Vfio,
            model: ModelId::UNNAMED,
            size: 0,
            load: GpuLoad::default(),
        }];
        let mut records = Memory::default();
        let mut node = boot_test(&[spec.clone()], &[], &mut records);
        let demand = Demand {
            ask: Ask {
                vfio: 1,
                ..Ask::default()
            },
            ..requested()
        };
        let (key, _) = node.submit(demand, &mut records).unwrap();
        start(&mut node, key, demand, &mut records);
        let before = node.admitted();
        let mut shrink = spec.clone();
        shrink.capacity.cpu_milli = 0;
        assert!(matches!(node.refresh_node(shrink), Err(Stop::Damaged(_))));
        let mut remove = spec.clone();
        remove.devices.clear();
        assert!(matches!(node.refresh_node(remove), Err(Stop::Damaged(_))));
        assert_eq!(node.admitted(), before);
        assert_eq!(
            start(&mut node, other_key(1), demand, &mut records),
            vec![(NodeId(0), Msg::Refused { key: other_key(1) })]
        );
        spec.classes = crate::model::Classes::NONE;
        node.refresh_node(spec.clone()).unwrap();
        assert_eq!(
            start(&mut node, other_key(2), requested(), &mut records),
            vec![(NodeId(0), Msg::Refused { key: other_key(2) })]
        );
        node.end(key, &mut records).unwrap();
        spec.classes = crate::model::Classes::NONE.with(Class::Shards);
        spec.platform = crate::model::Platform {
            os: Os::Macos,
            arch: Arch::Aarch64,
        };
        node.refresh_node(spec).unwrap();
        let shards = Demand {
            class: Class::Shards,
            ask: Ask {
                os: Some(Os::Linux),
                arch: Some(Arch::Aarch64),
                ..Ask::default()
            },
            ..requested()
        };
        assert!(start(&mut node, other_key(3), shards, &mut records).is_empty());
        assert_eq!(node.admitted(), amounts(&shards.need));
    }

    fn delete_specs() -> Vec<NodeSpec> {
        vec![
            NodeSpec::new(NodeId(0), Resources::default()),
            NodeSpec::new(NodeId(1), requested().need),
        ]
    }

    fn receiver(specs: &[NodeSpec], records: &mut Memory) -> Node {
        Node::boot(
            NodeId(1),
            specs,
            &[],
            test_budget(),
            Pick::default(),
            records,
            |_| Boxed::Absent,
        )
        .unwrap()
    }

    /// AC-T7.4.1: durable remote deletion survives owner/receiver restart, retains reservations on
    /// failed or delayed runtime deletion, and never replaces the workload on another peer.
    #[test]
    fn remote_delete_keeps_reservations_until_actual_delete_and_recovers_intent() {
        let specs = delete_specs();
        let (mut owned_records, mut ran_records) = (Memory::default(), Memory::default());
        let mut owner = boot_test(&specs, &[], &mut owned_records);
        let mut runner = receiver(&specs, &mut ran_records);
        let demand = requested();
        let (key, start) = owner.submit(demand, &mut owned_records).unwrap();
        assert_eq!(start, vec![(NodeId(1), Msg::Start { key, demand })]);
        runner
            .hear(NodeId(0), start[0].1, &mut ran_records)
            .unwrap();
        let started = runner.created(key, &mut ran_records).unwrap();
        owner
            .hear(NodeId(1), started[0].1, &mut owned_records)
            .unwrap();
        owner.told.clear();
        runner.told.clear();
        let deletion = owner.delete(key, &mut owned_records).unwrap();
        assert_eq!(deletion, vec![(NodeId(1), Msg::Delete { key, demand })]);
        assert!(owner.told.is_empty());
        assert_eq!(owner.view.verdict(NodeId(1), &demand), Ok(Verdict::NoRoom));
        assert!(
            runner
                .hear(NodeId(0), deletion[0].1, &mut ran_records)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            std::mem::take(&mut runner.told),
            vec![Told::Delete(key, demand)]
        );
        assert_eq!(runner.admitted(), amounts(&demand.need));
        let mut owner = Node::boot(
            NodeId(0),
            &specs,
            &[],
            test_budget(),
            Pick::default(),
            &mut owned_records,
            |_| Boxed::Absent,
        )
        .unwrap();
        let mut runner = Node::boot(
            NodeId(1),
            &specs,
            &[],
            test_budget(),
            Pick::default(),
            &mut ran_records,
            |_| Boxed::Running,
        )
        .unwrap();
        assert_eq!(
            owner.link_up(NodeId(1), &mut owned_records).unwrap(),
            deletion
        );
        let mut retry = Vec::new();
        owner.retry(&mut owned_records, &mut retry).unwrap();
        assert!(retry.is_empty());
        assert_eq!(runner.admitted(), amounts(&demand.need));
        assert_eq!(owner.owned(), 1);
        assert!(owner.told.is_empty());
        // The shell now proves the actual backend no longer holds this key.
        let ended = runner.end(key, &mut ran_records).unwrap();
        let forget = owner
            .hear(NodeId(1), ended[0].1, &mut owned_records)
            .unwrap();
        assert_eq!(std::mem::take(&mut owner.told), vec![Told::Ended(key)]);
        assert_eq!(owner.owned(), 0);
        assert_eq!(runner.admitted(), [0; lb::DIMS]);
        assert_eq!(owner.view.verdict(NodeId(1), &demand), Ok(Verdict::Admits));
        assert_eq!(
            runner
                .hear(NodeId(0), deletion[0].1, &mut ran_records)
                .unwrap(),
            ended
        );
        assert!(runner.told.is_empty());
        runner
            .hear(NodeId(0), forget[0].1, &mut ran_records)
            .unwrap();
        assert!(
            runner
                .hear(NodeId(0), deletion[0].1, &mut ran_records)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            std::mem::take(&mut runner.told),
            vec![Told::Delete(key, demand)]
        );
        assert_eq!(runner.end(key, &mut ran_records).unwrap(), ended);
    }

    /// AC-T7.4.1: cancelling an admitted-but-uncreated box supersedes its pending Create action,
    /// but the receiver still requires actual backend absence proof before releasing admission.
    #[test]
    fn remote_delete_pending_create_waits_for_backend_absence_proof() {
        let specs = delete_specs();
        let (mut owned_records, mut ran_records) = (Memory::default(), Memory::default());
        let mut owner = boot_test(&specs, &[], &mut owned_records);
        let mut runner = receiver(&specs, &mut ran_records);
        let (key, start) = owner.submit(requested(), &mut owned_records).unwrap();
        runner
            .hear(NodeId(0), start[0].1, &mut ran_records)
            .unwrap();
        let deletion = owner.delete(key, &mut owned_records).unwrap();
        assert!(
            runner
                .hear(NodeId(0), deletion[0].1, &mut ran_records)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            std::mem::take(&mut runner.told),
            vec![Told::Delete(key, requested())]
        );
        assert_eq!(runner.admitted(), amounts(&requested().need));
        assert_eq!(owner.owned(), 1);
        let ended = runner.end(key, &mut ran_records).unwrap();
        owner
            .hear(NodeId(1), ended[0].1, &mut owned_records)
            .unwrap();
        assert_eq!(owner.told, vec![Told::Ended(key)]);
        assert_eq!(runner.admitted(), [0; lb::DIMS]);
    }

    /// AC-T7.4.1: an owner cancellation committed before a crash is replayed as Delete, never Start.
    #[test]
    fn remote_delete_committed_intent_is_not_lost_at_its_write() {
        let specs = delete_specs();
        let mut rec = Crashing::new(Memory::default());
        let mut owner = Node::boot(
            NodeId(0),
            &specs,
            &[],
            test_budget(),
            Pick::default(),
            &mut rec,
            |_| Boxed::Absent,
        )
        .unwrap();
        let (key, _) = owner.submit(requested(), &mut rec).unwrap();
        rec.armed = Some(When::After);
        assert_eq!(owner.delete(key, &mut rec), Err(Stop::Crashed));
        let mut owner = Node::boot(
            NodeId(0),
            &specs,
            &[],
            test_budget(),
            Pick::default(),
            &mut rec,
            |_| Boxed::Absent,
        )
        .unwrap();
        assert_eq!(
            owner.link_up(NodeId(1), &mut rec).unwrap(),
            vec![(
                NodeId(1),
                Msg::Delete {
                    key,
                    demand: requested()
                }
            )]
        );
        assert_eq!(owner.owned(), 1);
        assert!(owner.told.is_empty());
        assert_eq!(
            owner.delete(Key { owner: 1, ..key }, &mut rec),
            Err(damaged("deletion by a different owner"))
        );
    }

    /// AC-T7.4.1: owner readiness is a durable peer acknowledgment, not merely At/admitted. A crash
    /// around acknowledgment never publishes early; reboot queries the peer before reporting ready.
    #[test]
    fn remote_readiness_is_durable_and_reconfirmed_after_owner_restart() {
        let specs = delete_specs();
        for when in [When::Before, When::After] {
            let mut rec = Crashing::new(Memory::default());
            let mut owner = Node::boot(
                NodeId(0),
                &specs,
                &[],
                test_budget(),
                Pick::default(),
                &mut rec,
                |_| Boxed::Absent,
            )
            .unwrap();
            let (key, _) = owner.submit(requested(), &mut rec).unwrap();
            assert!(!owner.is_created(key));
            rec.armed = Some(when);
            assert_eq!(
                owner.hear(NodeId(1), Msg::Started { key }, &mut rec),
                Err(Stop::Crashed)
            );
            assert!(owner.told.is_empty());
            let mut owner = Node::boot(
                NodeId(0),
                &specs,
                &[],
                test_budget(),
                Pick::default(),
                &mut rec,
                |_| Boxed::Absent,
            )
            .unwrap();
            assert!(!owner.is_created(key));
            assert_eq!(
                owner.link_up(NodeId(1), &mut rec).unwrap(),
                vec![(NodeId(1), Msg::Holds { key })]
            );
            owner
                .hear(
                    NodeId(1),
                    Msg::Held {
                        key,
                        holds: true,
                        ended: false,
                        created: false,
                    },
                    &mut rec,
                )
                .unwrap();
            assert!(!owner.is_created(key));
            owner
                .hear(
                    NodeId(1),
                    Msg::Held {
                        key,
                        holds: true,
                        ended: false,
                        created: true,
                    },
                    &mut rec,
                )
                .unwrap();
            assert!(owner.is_created(key));
            assert_eq!(
                std::mem::take(&mut owner.told),
                vec![Told::Started(key, NodeId(1))]
            );
            let row = rec.inner.rows.get(&key.name("own")).unwrap();
            assert!(row.starts_with("created 1 d1 "), "{row}");
            owner
                .hear(NodeId(1), Msg::Started { key }, &mut rec)
                .unwrap();
            assert!(
                owner.told.is_empty(),
                "a duplicate acknowledgment republished Started"
            );
            owner.delete(key, &mut rec).unwrap();
            assert!(!owner.is_created(key));
            owner
                .hear(NodeId(1), Msg::Started { key }, &mut rec)
                .unwrap();
            assert!(!owner.is_created(key));
            assert!(
                owner.told.is_empty(),
                "late Started reversed deletion intent"
            );
        }
    }

    struct LaunchCrash {
        records: Crashing<Memory>,
        after: Option<(usize, When)>,
    }
    impl LaunchCrash {
        fn before_write(&mut self) {
            if let Some((skip, when)) = self.after {
                if skip == 0 {
                    self.records.armed = Some(when);
                    self.after = None;
                } else {
                    self.after = Some((skip - 1, when));
                }
            }
        }
    }
    impl Records for LaunchCrash {
        fn put(&mut self, name: &str, value: &str) -> Result<(), Stop> {
            self.before_write();
            self.records.put(name, value)
        }
        fn remove(&mut self, name: &str) -> Result<(), Stop> {
            self.before_write();
            self.records.remove(name)
        }
        fn all(&mut self) -> Result<Vec<(String, String)>, Stop> {
            self.records.all()
        }
    }

    /// AC-T7.4.1/B4: whole launch samples are durable before Started, deduplicated by the existing
    /// full-key Created transition, and never fabricated for recovered/preexisting runtimes.
    #[test]
    fn combined_launch_survives_each_write_crash_without_resampling() {
        let specs = vec![NodeSpec::new(NodeId(0), CAP)];
        for write in 0..2 {
            for when in [When::Before, When::After] {
                let mut rec = LaunchCrash {
                    records: Crashing::new(Memory::default()),
                    after: None,
                };
                let mut node = Node::boot(
                    NodeId(0),
                    &specs,
                    &[],
                    test_budget(),
                    Pick::default(),
                    &mut rec,
                    |_| Boxed::Absent,
                )
                .unwrap();
                let (key, _) = node.submit(requested(), &mut rec).unwrap();
                node.hear(
                    NodeId(0),
                    Msg::Start {
                        key,
                        demand: requested(),
                    },
                    &mut rec,
                )
                .unwrap();
                node.told.clear();
                rec.after = Some((write, when));
                assert_eq!(
                    node.created_with_start(key, 0, &mut rec),
                    Err(Stop::Crashed)
                );
                assert!(node.told.is_empty());
                let mut node = Node::boot(
                    NodeId(0),
                    &specs,
                    &[],
                    test_budget(),
                    Pick::default(),
                    &mut rec,
                    |_| Boxed::Running,
                )
                .unwrap();
                let expected = if write == 0 && when == When::Before {
                    vec![]
                } else {
                    vec![StartObservation {
                        node: NodeId(0),
                        class: Class::Oci,
                        sum_ns: 0,
                        count: 1,
                    }]
                };
                assert_eq!(
                    node.start_observations(),
                    expected,
                    "write {write}, {when:?}"
                );
                assert!(node.told.is_empty(), "boot must not Create again");
                let started = node.created_with_start(key, 999, &mut rec).unwrap();
                node.hear(NodeId(0), started[0].1, &mut rec).unwrap();
                node.hear(
                    NodeId(0),
                    Msg::Held {
                        key,
                        holds: true,
                        ended: false,
                        created: true,
                    },
                    &mut rec,
                )
                .unwrap();
                assert_eq!(
                    node.start_observations(),
                    expected,
                    "replay sampled an existing runtime"
                );
                assert!(node.is_created(key));
                let ended = node.end(key, &mut rec).unwrap();
                assert!(node.is_ended(key));
                assert!(!node.is_created(key));
                node.hear(NodeId(0), ended[0].1, &mut rec).unwrap();
            }
        }
    }

    /// AC-T7.4.1/B4: production submit actually uses the shared learned scorer; with no observations
    /// it keeps the old uniform packing choice, then picks the faster class peer and preserves it
    /// across reboot without counting a remote aggregate twice.
    #[test]
    fn combined_launch_changes_real_submit_and_persists_remote_aggregates() {
        let specs = vec![NodeSpec::new(NodeId(0), CAP), NodeSpec::new(NodeId(1), CAP)];
        let mut rec = Memory::default();
        let mut node = boot_test(&specs, &[], &mut rec);
        node.set_rates(NodeId(0), Rates::default()).unwrap();
        let (first, out) = node.submit(requested(), &mut rec).unwrap();
        assert_eq!(out[0].0, NodeId(0));
        node.hear(NodeId(0), out[0].1, &mut rec).unwrap();
        node.created_with_start(first, 800, &mut rec).unwrap();
        let peer = [StartObservation {
            node: NodeId(1),
            class: Class::Oci,
            sum_ns: 100,
            count: 1,
        }];
        node.merge_peer_start_observations(NodeId(1), &peer, &mut rec)
            .unwrap();
        let snapshot = node.start_observations();
        node.merge_peer_start_observations(NodeId(1), &peer, &mut rec)
            .unwrap();
        assert_eq!(node.start_observations(), snapshot);
        assert_eq!(
            node.submit(requested(), &mut rec).unwrap().1[0].0,
            NodeId(1)
        );
        assert!(rec.rows.get("epoch").unwrap().starts_with("launch1 "));
        let mut node = Node::boot(
            NodeId(0),
            &specs,
            &[],
            test_budget(),
            Pick::default(),
            &mut rec,
            |key| {
                if key == first {
                    Boxed::Running
                } else {
                    Boxed::Absent
                }
            },
        )
        .unwrap();
        assert_eq!(node.start_observations(), snapshot);
        assert_eq!(
            node.submit(requested(), &mut rec).unwrap().1[0].0,
            NodeId(1)
        );
    }

    /// AC-T7.4.1/B4: aggregate authority is the authenticated sender, and loopback retries cannot
    /// replace a newly measured local sample with an older immutable outgoing snapshot.
    #[test]
    fn combined_launch_merge_is_bounded_authoritative_and_idempotent() {
        let specs = vec![NodeSpec::new(NodeId(0), CAP), NodeSpec::new(NodeId(1), CAP)];
        let mut rec = Memory::default();
        let mut node = boot_test(&specs, &[], &mut rec);
        let old = [StartObservation {
            node: NodeId(0),
            class: Class::Oci,
            sum_ns: 0,
            count: 1,
        }];
        node.restore_start_observations(&old, &mut rec).unwrap();
        node.observe_start(NodeId(0), Class::Oci, 100).unwrap();
        let snapshot = node.start_observations();
        node.merge_peer_start_observations(NodeId(0), &old, &mut rec)
            .unwrap();
        assert_eq!(node.start_observations(), snapshot);
        let foreign = StartObservation {
            node: NodeId(0),
            class: Class::Shards,
            sum_ns: 1,
            count: 1,
        };
        assert!(matches!(
            node.merge_peer_start_observations(NodeId(1), &[foreign], &mut rec),
            Err(Stop::Damaged(_))
        ));
        let peer = StartObservation {
            node: NodeId(1),
            ..foreign
        };
        assert!(matches!(
            node.merge_peer_start_observations(NodeId(1), &[peer, peer], &mut rec),
            Err(Stop::Damaged(_))
        ));
        assert_eq!(node.start_observations(), snapshot);
        node.merge_peer_start_observations(NodeId(1), &[peer], &mut rec)
            .unwrap();
        assert_eq!(
            node.start_observations()
                .iter()
                .find(|r| r.node == NodeId(0)),
            snapshot.first()
        );
    }

    /// CLAUDE.md §6/B4: hostile aggregate metadata cannot erase requirements or allocate more than
    /// the node/class-derived cell bound; legacy numeric epochs remain readable without samples.
    #[test]
    fn combined_launch_epoch_refuses_invalid_or_incompatible_observations() {
        let specs = vec![NodeSpec::new(NodeId(0), CAP)];
        for text in [
            "poststage1 1 0",
            "launch1 1 4",
            "launch1 1 1 1 oci 1 1",
            "launch1 1 1 0 oci 0 0",
            "launch1 1 2 0 oci 1 1 0 oci 1 1",
            "launch1 1 0 extra",
        ] {
            let mut rec = Memory::default();
            rec.put("epoch", text).unwrap();
            assert!(
                Node::boot(
                    NodeId(0),
                    &specs,
                    &[],
                    test_budget(),
                    Pick::default(),
                    &mut rec,
                    |_| Boxed::Absent
                )
                .is_err(),
                "{text}"
            );
        }
        let mut rec = Memory::default();
        rec.put("epoch", "1").unwrap();
        let node = boot_test(&specs, &[], &mut rec);
        assert!(node.start_observations().is_empty());
        assert!(rec.rows.get("epoch").unwrap().starts_with("launch1 2 0"));
    }

    /// kiln's `ReusedIdRan`: a box's name carries its owner's epoch, so a restarted owner's first box
    /// isn't read as the box its first number named before.
    #[test]
    fn a_restarted_owners_numbers_never_name_an_earlier_box() {
        let specs = vec![NodeSpec::new(NodeId(0), CAP)];
        let mut rec = Memory::default();
        let mut a = Node::boot(
            NodeId(0),
            &specs,
            &[],
            test_budget(),
            Pick::default(),
            &mut rec,
            |_| Boxed::Absent,
        )
        .unwrap();
        let (first, _) = a
            .submit(
                plain(Resources {
                    cpu_milli: 1,
                    ..Resources::default()
                }),
                &mut rec,
            )
            .unwrap();
        let mut b = Node::boot(
            NodeId(0),
            &specs,
            &[],
            test_budget(),
            Pick::default(),
            &mut rec,
            |_| Boxed::Absent,
        )
        .unwrap();
        let (again, _) = b
            .submit(
                plain(Resources {
                    cpu_milli: 1,
                    ..Resources::default()
                }),
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
                    Node::boot(
                        NodeId(0),
                        &specs,
                        &[],
                        test_budget(),
                        Pick::default(),
                        &mut rec,
                        |_| Boxed::Absent
                    ),
                    Err(Stop::Damaged(_))
                ),
                "{name} {value}"
            );
        }
    }
}
