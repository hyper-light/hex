//! AC-T7.4.1: the file-backed peer core drives the actual selected runtime. Publication follows
//! durable admission, physical creation and durable acknowledgement, in that order. A physical
//! box (even an empty process box) holds its reservation until deletion, never merely until exit.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

use hex_api::{BoxId, BoxInfo, BoxSpec, BoxState, Refusal, Runtime};
use hex_sim::index::Pick;
use hex_sim::model::{Class, Demand, NodeId, NodeSpec};
use hex_sim::peer::{self, Boxed, Budget, Key, Msg, Stop, Told};

mod rows;
use rows::{Context, Rows};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
    pub spec: BoxSpec,
    pub start: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub msg: Msg,
    pub payload: Option<Payload>,
    pub refusal: Option<Refusal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    Started {
        key: Key,
        node: NodeId,
    },
    Ended {
        key: Key,
    },
    Refused {
        key: Key,
        node: NodeId,
        refusal: Refusal,
    },
    /// The box may exist. Admission is retained; an operator may retry deletion or reboot.
    Unknown {
        key: Key,
        refusal: Refusal,
        cleanup: Option<Refusal>,
    },
    Status {
        key: Key,
        info: BoxInfo,
    },
}

#[derive(Debug, Default)]
pub struct Step {
    pub send: Vec<(NodeId, Envelope)>,
    pub notices: Vec<Notice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub key: Key,
    pub node: Option<NodeId>,
    pub local: bool,
    pub created: bool,
    pub ended: bool,
    pub physical: BoxId,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("placement stopped: {0:?}")]
    Core(Stop),
    #[error(transparent)]
    Runtime(#[from] Refusal),
}

impl From<Stop> for Error {
    fn from(stop: Stop) -> Self {
        Self::Core(stop)
    }
}

/// The operator supplies backend truth. A registry has at most the scheduler's three classes;
/// runtimes without a device-attachment seam cannot silently accept scheduler device assignments.
#[derive(Clone, Default)]
pub struct Runtimes {
    by_class: BTreeMap<Class, Arc<dyn Runtime>>,
}

impl Runtimes {
    pub fn new(
        entries: impl IntoIterator<Item = (Class, Arc<dyn Runtime>)>,
    ) -> Result<Self, Error> {
        let mut by_class = BTreeMap::new();
        for (class, runtime) in entries {
            if by_class.insert(class, runtime).is_some() {
                return Err(invalid("duplicate runtime class").into());
            }
        }
        Ok(Self { by_class })
    }

    fn get(&self, class: Class) -> Result<Arc<dyn Runtime>, Error> {
        self.by_class
            .get(&class)
            .cloned()
            .ok_or_else(|| invalid("no registered runtime for placement class").into())
    }
}

/// Bounds derive from the runtime's own maximum box ledger, never a separate picked queue size.
pub fn runtime_budget() -> Budget {
    Budget {
        owned: hex_runtime::MAX_BOXES,
        ran: hex_runtime::MAX_BOXES,
    }
}

pub fn physical_id(key: Key) -> Result<BoxId, Refusal> {
    BoxId::new(&format!("hex-place-{}-{}-{}", key.owner, key.epoch, key.seq))
}

fn invalid(why: impl Into<String>) -> Refusal {
    Refusal::Invalid {
        what: "placement",
        why: why.into(),
    }
}
fn bad(why: impl Into<String>) -> Stop {
    Stop::Damaged(why.into())
}
fn name(kind: &str, key: Key) -> String {
    format!("{kind}-{}-{}-{}", key.owner, key.epoch, key.seq)
}

struct State {
    node: peer::Node,
    rows: Rows,
    stopped: Option<Stop>,
}

pub struct Driver {
    state: Mutex<State>,
    // One operation owns physical effects, with no queued waiters: concurrent mutation is refused.
    // The core/record lock is always released before calling any Runtime method.
    operation: Mutex<()>,
    runtimes: Runtimes,
    peers: Vec<NodeId>,
}

impl Driver {
    pub fn boot(
        id: NodeId,
        nodes: &[NodeSpec],
        models: &[String],
        budget: Budget,
        pick: Pick,
        dir: &Path,
        runtimes: Runtimes,
    ) -> Result<Self, Error> {
        if budget.owned > hex_runtime::MAX_BOXES || budget.ran > hex_runtime::MAX_BOXES {
            return Err(invalid("placement budget exceeds runtime box bound").into());
        }
        let mut rows = Rows::open(dir, budget)?;
        let mut actual = BTreeMap::new();
        let mut start = Vec::new();
        for (key, payload) in rows.running()? {
            // Core demand is durable beside this payload; decode it through core boot below.
            // Select its runtime using the shared model codec, without a second decision format.
            let (demand, ended) = durable_demand(&rows, key, models)?;
            let runtime = runtimes.get(demand.class)?;
            validate(&demand, &payload)?;
            validate_backend(&payload, &runtime)?;
            let physical = physical_id(key)?;
            let boxed = match runtime.inspect(&physical) {
                Ok(info) => {
                    if !ended && payload.start && info.state == BoxState::Created {
                        start.push((runtime, physical));
                    }
                    Boxed::Running
                }
                Err(Refusal::NoSuchBox(_)) => Boxed::Absent,
                Err(why) => return Err(why.into()),
            };
            actual.insert(key, boxed);
        }
        let node = peer::Node::boot(id, nodes, models, budget, pick, &mut rows, |key| {
            actual.get(&key).copied().unwrap_or(Boxed::Absent)
        })?;
        // Only validated, recovered records may resume an init; no acknowledgement has escaped.
        for (runtime, id) in start {
            runtime.start(&id)?;
        }
        Ok(Self {
            state: Mutex::new(State {
                node,
                rows,
                stopped: None,
            }),
            operation: Mutex::new(()),
            runtimes,
            peers: nodes.iter().map(|n| n.id).collect(),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, State>, Error> {
        let state = self
            .state
            .lock()
            .map_err(|_| Refusal::Broken("placement mutex poisoned".into()))?;
        if let Some(stop) = &state.stopped {
            return Err(Error::Core(stop.clone()));
        }
        Ok(state)
    }

    fn operation(&self) -> Result<MutexGuard<'_, ()>, Error> {
        self.operation.try_lock().map_err(|e| {
            match e {
                TryLockError::WouldBlock => Refusal::Exhausted {
                    what: "placement operation",
                    bound: 1,
                },
                TryLockError::Poisoned(_) => {
                    Refusal::Broken("placement operation mutex poisoned".into())
                }
            }
            .into()
        })
    }

    pub fn submit(&self, demand: Demand, payload: Payload) -> Result<(Key, Step), Error> {
        let _operation = self.operation()?;
        validate(&demand, &payload)?;
        let mut state = self.lock()?;
        state.rows.context = Context::NextOwned(payload);
        let State { node, rows, .. } = &mut *state;
        let result = node.submit(demand, rows);
        state.rows.context = Context::None;
        let (key, out) = finish(&mut state, result, true)?;
        let (step, create) = collect(&mut state, out)?;
        drop(state);
        Ok((key, self.drive(step, create)?))
    }

    pub fn hear(&self, from: NodeId, envelope: Envelope) -> Result<Step, Error> {
        let _operation = self.operation()?;
        let mut state = self.lock()?;
        if let Msg::Start { key, demand } = envelope.msg {
            if key.owner != from.0 || !self.peers.contains(&from) {
                return Err(invalid("start owner is not its known sender").into());
            }
            if state.node.demand(key).is_some_and(|d| d != demand) {
                return Err(invalid("start changed its durable demand").into());
            }
            let payload = envelope
                .payload
                .clone()
                .ok_or_else(|| invalid("start has no payload"))?;
            // Runtime::classes is a backend call too: validate after dropping the core lock.
            let prior = state.node.ran_keys().any(|(k, _, _)| k == key);
            if prior && state.rows.payload("ran", key)? != payload {
                return Err(invalid("repeated start changed its payload").into());
            }
            drop(state);
            let checked = validate(&demand, &payload).and_then(|()| {
                let runtime = self.runtimes.get(demand.class)?;
                validate_backend(&payload, &runtime)
            });
            state = self.lock()?;
            match checked {
                Ok(()) => {}
                Err(Error::Runtime(why)) if !prior => {
                    return Ok(Step {
                        send: vec![(
                            from,
                            Envelope {
                                msg: Msg::Refused { key },
                                payload: None,
                                refusal: Some(why),
                            },
                        )],
                        notices: Vec::new(),
                    });
                }
                Err(why) => return Err(why),
            }
            state.rows.context = Context::Start(key, payload);
        } else if envelope.payload.is_some() {
            return Err(invalid("payload outside start").into());
        }
        let State { node, rows, .. } = &mut *state;
        let result = node.hear(from, envelope.msg, rows);
        state.rows.context = Context::None;
        let out = finish(&mut state, result, false)?;
        let (mut step, create) = collect(&mut state, out)?;
        if let (Msg::Refused { key }, Some(refusal)) = (envelope.msg, envelope.refusal) {
            step.notices.push(Notice::Refused {
                key,
                node: from,
                refusal,
            });
        }
        drop(state);
        self.drive(step, create)
    }

    pub fn link_up(&self, peer: NodeId) -> Result<Step, Error> {
        self.change(|node, rows| node.link_up(peer, rows))
    }

    /// Recovery uses the same link-up protocol, including the laptop's self-link. Messages stay
    /// external so a local workload never recurses through the runtime driver.
    pub fn take_recovered(&self) -> Result<Step, Error> {
        self.change(|node, rows| {
            let mut out = Vec::new();
            for peer in &self.peers {
                out.extend(node.link_up(*peer, rows)?);
            }
            Ok(out)
        })
    }

    pub fn refresh_node(&self, spec: NodeSpec) -> Result<(), Error> {
        let _operation = self.operation()?;
        let mut state = self.lock()?;
        let result = state.node.refresh_node(spec);
        finish(&mut state, result, false)
    }

    pub fn set_pick(&self, pick: Pick) -> Result<(), Error> {
        let _operation = self.operation()?;
        self.lock()?.node.set_pick(pick);
        Ok(())
    }

    pub fn set_gpu_load(
        &self,
        node: NodeId,
        device: usize,
        load: hex_sim::model::GpuLoad,
    ) -> Result<(), Error> {
        let _operation = self.operation()?;
        let mut state = self.lock()?;
        let result = state.node.set_gpu_load(node, device, load);
        finish(&mut state, result, false)
    }

    pub fn retry(&self) -> Result<Step, Error> {
        self.change(|node, rows| {
            let mut out = Vec::new();
            node.retry(rows, &mut out)?;
            Ok(out)
        })
    }

    fn change(
        &self,
        change: impl FnOnce(&mut peer::Node, &mut Rows) -> Result<Vec<(NodeId, Msg)>, Stop>,
    ) -> Result<Step, Error> {
        let _operation = self.operation()?;
        let mut state = self.lock()?;
        let State { node, rows, .. } = &mut *state;
        let result = change(node, rows);
        let out = finish(&mut state, result, false)?;
        let (step, create) = collect(&mut state, out)?;
        drop(state);
        self.drive(step, create)
    }

    fn drive(&self, mut step: Step, create: Vec<(Key, Demand, Payload)>) -> Result<Step, Error> {
        for (key, demand, payload) in create {
            let runtime = self.runtimes.get(demand.class)?;
            let id = physical_id(key)?;
            let mut spec = payload.spec.clone();
            spec.id = id.clone();
            let made = match runtime.inspect(&id) {
                Ok(info) => Ok(info),
                Err(Refusal::NoSuchBox(_)) => runtime.create(spec),
                Err(why) => {
                    step.notices.push(Notice::Unknown {
                        key,
                        refusal: why,
                        cleanup: None,
                    });
                    continue;
                }
            };
            let made = made.and_then(|info| {
                if payload.start && info.state == BoxState::Created {
                    runtime.start(&id)?;
                }
                Ok(())
            });
            match made {
                Ok(()) => self.completed(key, None, &mut step)?,
                Err(why @ Refusal::Exists(_)) => step.notices.push(Notice::Unknown {
                    key,
                    refusal: why,
                    cleanup: None,
                }),
                Err(why) => match runtime.delete(&id) {
                    Ok(()) | Err(Refusal::NoSuchBox(_)) => {
                        self.completed(key, Some(why), &mut step)?
                    }
                    Err(cleanup) => step.notices.push(Notice::Unknown {
                        key,
                        refusal: why,
                        cleanup: Some(cleanup),
                    }),
                },
            }
        }
        Ok(step)
    }

    fn completed(&self, key: Key, refusal: Option<Refusal>, step: &mut Step) -> Result<(), Error> {
        let mut state = self.lock()?;
        let State { node, rows, .. } = &mut *state;
        let result = if refusal.is_some() {
            node.create_refused(key, rows)
        } else {
            node.created(key, rows)
        };
        let out = finish(&mut state, result, false)?;
        let (mut next, create) = collect(&mut state, out)?;
        if !create.is_empty() {
            state.stopped = Some(bad("create recursively requested creation"));
            return Err(bad("unexpected nested creation").into());
        }
        if let Some(why) = refusal {
            for (_, message) in &mut next.send {
                if matches!(message.msg,Msg::Refused{key:k} if k==key) {
                    message.refusal = Some(why.clone());
                }
            }
            next.notices.push(Notice::Refused {
                key,
                node: state.node.id(),
                refusal: why,
            });
        }
        step.send.extend(next.send);
        step.notices.extend(next.notices);
        Ok(())
    }

    pub fn owned(&self) -> Result<Vec<(Key, Option<NodeId>)>, Error> {
        Ok(self.lock()?.node.owned_keys().collect())
    }

    pub fn status(&self, key: Key) -> Result<Option<Status>, Error> {
        let state = self.lock()?;
        let own = state.node.owned_keys().find(|(k, _)| *k == key);
        let ran = state.node.ran_keys().find(|(k, _, _)| *k == key);
        if own.is_none() && ran.is_none() {
            return Ok(None);
        }
        Ok(Some(Status {
            key,
            node: ran
                .map(|_| state.node.id())
                .or_else(|| own.and_then(|(_, n)| n)),
            local: ran.is_some(),
            created: ran.is_some_and(|(_, c, _)| c),
            ended: ran.is_some_and(|(_, _, e)| e),
            physical: physical_id(key)?,
        }))
    }

    pub fn delete(&self, key: Key) -> Result<Step, Error> {
        let _operation = self.operation()?;
        let state = self.lock()?;
        if !state.node.ran_keys().any(|(k, _, _)| k == key) {
            return Err(Refusal::NoSuchBox(physical_id(key)?).into());
        }
        let demand = state
            .node
            .demand(key)
            .ok_or_else(|| bad("ran demand missing"))?;
        drop(state);
        match self.runtimes.get(demand.class)?.delete(&physical_id(key)?) {
            Ok(()) | Err(Refusal::NoSuchBox(_)) => self.ended(key),
            Err(why) => Err(why.into()),
        }
    }

    fn ended(&self, key: Key) -> Result<Step, Error> {
        let mut state = self.lock()?;
        let State { node, rows, .. } = &mut *state;
        let result = node.end(key, rows);
        let out = finish(&mut state, result, false)?;
        let (step, create) = collect(&mut state, out)?;
        if !create.is_empty() {
            return Err(bad("end requested creation").into());
        }
        Ok(step)
    }

    pub fn poll_status(&self) -> Result<Step, Error> {
        let _operation = self.operation()?;
        let state = self.lock()?;
        let keys: Vec<_> = state
            .node
            .ran_keys()
            .filter(|(_, _, ended)| !ended)
            .map(|(key, _, _)| {
                Ok((
                    key,
                    state
                        .node
                        .demand(key)
                        .ok_or_else(|| bad("ran demand missing"))?,
                ))
            })
            .collect::<Result<_, Stop>>()?;
        drop(state);
        let mut step = Step::default();
        for (key, demand) in keys {
            match self.runtimes.get(demand.class)?.inspect(&physical_id(key)?) {
                Ok(info) => step.notices.push(Notice::Status { key, info }),
                Err(Refusal::NoSuchBox(_)) => {
                    let next = self.ended(key)?;
                    step.send.extend(next.send);
                    step.notices.extend(next.notices);
                }
                Err(why) => step.notices.push(Notice::Unknown {
                    key,
                    refusal: why,
                    cleanup: None,
                }),
            }
        }
        Ok(step)
    }

    /// Actual memory held by this driver's physical boxes. CPU/disk are not invented from caps.
    pub fn measured_owned_use(&self) -> Result<hex_sim::model::Resources, Error> {
        let _operation = self.operation()?;
        let state = self.lock()?;
        let keys: Vec<_> = state
            .node
            .ran_keys()
            .filter(|(_, _, ended)| !ended)
            .map(|(key, _, _)| {
                Ok((
                    key,
                    state
                        .node
                        .demand(key)
                        .ok_or_else(|| bad("ran demand missing"))?,
                ))
            })
            .collect::<Result<_, Stop>>()?;
        drop(state);
        let mut resources = hex_sim::model::Resources::default();
        for (key, demand) in keys {
            match self.runtimes.get(demand.class)?.usage(&physical_id(key)?) {
                Ok(usage) => {
                    resources.mem_bytes = resources
                        .mem_bytes
                        .checked_add(usage.memory_bytes)
                        .ok_or_else(|| invalid("physical memory usage overflow"))?
                }
                Err(Refusal::NoSuchBox(_)) => {}
                Err(why) => return Err(why.into()),
            }
        }
        Ok(resources)
    }
}

fn validate(demand: &Demand, payload: &Payload) -> Result<(), Error> {
    payload.spec.check()?;
    let cpu = payload
        .spec
        .bounds
        .cpu_max_milli
        .ok_or_else(|| invalid("CPU cap required for a reserved box"))?;
    let mem = payload
        .spec
        .bounds
        .memory_max
        .ok_or_else(|| invalid("memory cap required for a reserved box"))?;
    if u64::from(cpu) > demand.need.cpu_milli || mem > demand.need.mem_bytes {
        return Err(invalid("box limits exceed placement reservation").into());
    }
    if demand.need.gpus > 0
        || demand.ask.devices()
        || demand.ask.gpu_model.is_some()
        || demand.ask.gpu_memory > 0
        || demand.ask.block_size > 0
    {
        return Err(invalid("runtime seam cannot attach assigned scheduler devices").into());
    }
    if payload.start && payload.spec.class.class() != hex_api::Class::Oci {
        return Err(invalid("only OCI boxes have an init to start").into());
    }
    if demand.class != Class::Oci && payload.spec.class.class() != hex_api::Class::Oci {
        return Err(invalid("microVM placement requires an OCI guest payload").into());
    }
    let value = rows::payload_value(payload)?;
    let text = serde_json::to_string(&value).map_err(|e| bad(e.to_string()))?;
    if text.len() > hex_core::wire::MAX_MESSAGE {
        return Err(invalid("placement payload exceeds wire bound").into());
    }
    Ok(())
}

fn validate_backend(payload: &Payload, runtime: &Arc<dyn Runtime>) -> Result<(), Error> {
    if !runtime.classes().contains(&payload.spec.class.class()) {
        return Err(invalid("registered runtime does not support box spec class").into());
    }
    Ok(())
}

fn finish<T>(state: &mut State, result: Result<T, Stop>, submit: bool) -> Result<T, Error> {
    result.map_err(|stop| {
        if !(submit && matches!(stop, Stop::Full { .. })) {
            state.stopped = Some(stop.clone());
        }
        Error::Core(stop)
    })
}

fn collect(
    state: &mut State,
    out: Vec<(NodeId, Msg)>,
) -> Result<(Step, Vec<(Key, Demand, Payload)>), Error> {
    let mut step = Step::default();
    let mut create = Vec::new();
    for (node, msg) in out {
        let payload = match msg {
            Msg::Start { key, .. } => Some(state.rows.payload("own", key)?),
            _ => None,
        };
        step.send.push((
            node,
            Envelope {
                msg,
                payload,
                refusal: None,
            },
        ));
    }
    for told in std::mem::take(&mut state.node.told) {
        match told {
            Told::Started(key, node) => step.notices.push(Notice::Started { key, node }),
            Told::Ended(key) => step.notices.push(Notice::Ended { key }),
            Told::Create(key, demand) => {
                create.push((key, demand, state.rows.payload("ran", key)?))
            }
        }
    }
    Ok((step, create))
}

fn durable_demand(rows: &Rows, key: Key, models: &[String]) -> Result<(Demand, bool), Stop> {
    let row = rows.row(key)?;
    // The existing core durable grammar prefixes a ran demand with owner and lifecycle state.
    let (_, rest) = row.split_once(' ').ok_or_else(|| bad("ran owner"))?;
    let (state, demand) = rest.split_once(' ').ok_or_else(|| bad("ran state"))?;
    Ok((peer::decode_demand(demand, models)?, state == "ended"))
}

#[cfg(test)]
mod tests;
