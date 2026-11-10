//! CP-T7.4/LB03: file-backed production shell around the adapted peer kernel and Runtime driver.
//! The ordinary node socket carries control. Declared peers use authenticated SSH stdio tunnels;
//! no cluster service or listening TCP socket is required, including on a single laptop.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex, Weak};

use hex_api::{
    BoxId, BoxInfo, BoxSpec, Class, Proc, Program, Refusal, Runtime, Signal, Stdio, Usage,
};
use hex_core::{schedule as dto, wire};
use hex_sim::index::Pick;
use hex_sim::model::{Classes, Cluster, NodeId, NodeSpec};
use hex_sim::peer::{self, Budget, Key};
use serde::{Deserialize, Serialize};

use crate::placement::{Driver, Envelope, Error, Notice, Payload, Runtimes, Step};
use crate::serve::Host;

mod offer;
mod order;
mod stopping;
use stopping::Stopping;
mod cache;
pub mod tunnel;

/// CLI-owned reporting: library delivery never chooses an output stream.
pub type Report = Arc<dyn Fn(NodeId, &str) + Send + Sync>;

/// Explicitly initialized runtime endpoints; class names describe the configured backend,
/// while its hello independently establishes which BoxSpec forms it implements.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Backend {
    pub class: String,
    pub socket: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Peer {
    pub node: u32,
    /// A host in the user's SSH configuration, including its identity and pinned host key.
    pub host: String,
    /// One explicitly configured remote command, normally `hex schedule tunnel --socket …`.
    /// SSH's remote command syntax belongs to the administrator, never to a submitted box.
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub node: u32,
    pub state: PathBuf,
    pub socket: PathBuf,
    /// Previously exchanged peer facts. The receiver always checks its own live facts as well.
    #[serde(default)]
    pub nodes: Vec<NodeSpec>,
    #[serde(default)]
    pub models: Vec<String>,
    pub backends: Vec<Backend>,
    #[serde(default)]
    pub peers: Vec<Peer>,
}

fn failed(error: Error) -> Refusal {
    match error {
        Error::Runtime(why) => why,
        Error::Core(peer::Stop::Full { what, bound }) => Refusal::Exhausted { what, bound },
        Error::Core(why) => Refusal::Broken(format!("placement records: {why:?}")),
    }
}

fn key(k: dto::Key) -> Key {
    Key {
        owner: k.owner,
        epoch: k.epoch,
        seq: k.seq,
    }
}
fn external(k: Key) -> dto::Key {
    dto::Key {
        owner: k.owner,
        epoch: k.epoch,
        seq: k.seq,
    }
}

fn encoded_refusal(why: &Refusal) -> Result<Vec<u8>, Refusal> {
    wire::answer(0, &wire::Answer::Refused(why.clone()), wire::MAX_MESSAGE).map_err(Into::into)
}

fn packet(
    envelope: Envelope,
    models: &[String],
    epoch: u64,
    seq: u64,
    observations: Vec<dto::LaunchObservation>,
) -> Result<dto::Packet, Refusal> {
    Ok(dto::Packet {
        epoch,
        seq,
        observations,
        message: peer::encode_message(&envelope.msg, models, wire::MAX_MESSAGE)
            .map_err(|e| Refusal::Broken(format!("encoding placement message: {e:?}")))?,
        payload: envelope.payload.map(|p| dto::Payload {
            spec: p.spec,
            start: p.start,
        }),
        refusal: envelope.refusal.as_ref().map(encoded_refusal).transpose()?,
    })
}

fn envelope(packet: dto::Packet, models: &[String]) -> Result<Envelope, Refusal> {
    let refusal = packet
        .refusal
        .map(|bytes| {
            let received = wire::Received {
                bytes: &bytes,
                size: bytes.len(),
                descriptors: 0,
                descriptors_cut: false,
            };
            match wire::read_answer(&received, wire::MAX_MESSAGE)
                .map_err(Refusal::from)?
                .1
            {
                wire::Answer::Refused(why) => Ok(why),
                _ => Err(Refusal::Invalid {
                    what: "peer refusal",
                    why: "expected a typed Refused answer".into(),
                }),
            }
        })
        .transpose()?;
    Ok(Envelope {
        msg: peer::decode_message(&packet.message, models, wire::MAX_MESSAGE).map_err(|e| {
            Refusal::Invalid {
                what: "placement message",
                why: format!("{e:?}"),
            }
        })?,
        payload: packet.payload.map(|p| Payload {
            spec: p.spec,
            start: p.start,
        }),
        refusal,
    })
}

struct Permit(Arc<AtomicUsize>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
struct Queued {
    envelope: Option<Envelope>,
    observations: Vec<dto::LaunchObservation>,
    _permit: Permit,
    _refresh: Option<RefreshPermit>,
}

struct RefreshPermit(Arc<AtomicBool>);
impl Drop for RefreshPermit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

struct PeerLane {
    peer: Peer,
    id: NodeId,
    models: Vec<String>,
    epoch: u64,
    stopping: Arc<Stopping>,
    report: Report,
    source: Arc<Mutex<Weak<Service>>>,
}

fn run_peer(lane: PeerLane, receive: Receiver<Queued>) {
    let PeerLane {
        peer,
        id,
        models,
        epoch,
        stopping,
        report,
        source,
    } = lane;
    let mut tunnel = None;
    let mut seq = 0u64;
    while let Ok(mut queued) = receive.recv() {
        let mut pace = std::time::Duration::ZERO;
        loop {
            if stopping.is_stopped() {
                return;
            }
            let began = std::time::Instant::now();
            let fresh = tunnel.is_none();
            if fresh {
                match tunnel::Connection::open(&peer, &stopping) {
                    Ok(connection) => tunnel = Some(connection),
                    Err(why) => {
                        report(NodeId(peer.node), &why.to_string());
                        pace = pace.max(began.elapsed());
                        if stopping.wait_retry(pace).unwrap_or(true) {
                            return;
                        }
                        // Core records, including a newer Delete, are authoritative after reconnect.
                        for old in receive.try_iter() {
                            drop(old);
                        }
                        queued.envelope = None;
                        continue;
                    }
                }
            }
            let Some(connection) = tunnel.as_mut() else {
                return;
            };
            if fresh || queued.envelope.is_none() {
                match connection.call(dto::Command::Describe { from: id.0 }) {
                    Ok(Ok(reply)) => {
                        if let Some(offer) = reply.offer {
                            loop {
                                let result = {
                                    let Some(service) =
                                        source.lock().ok().and_then(|s| s.upgrade())
                                    else {
                                        return;
                                    };
                                    service.command(dto::Command::Advertise {
                                        from: peer.node,
                                        offer: offer.clone(),
                                        observations: reply.observations.clone(),
                                    })
                                };
                                match result {
                                    Ok(_) => break,
                                    Err(Refusal::Exhausted {
                                        what: "scheduler command publication",
                                        ..
                                    }) => {
                                        pace = pace.max(began.elapsed());
                                        if stopping.wait_retry(pace).unwrap_or(true) {
                                            return;
                                        }
                                    }
                                    Err(why) => {
                                        report(NodeId(peer.node), &why.to_string());
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Ok(Err(why)) => report(NodeId(peer.node), &why.to_string()),
                    Err(why) => {
                        report(NodeId(peer.node), &why.to_string());
                        tunnel = None;
                        pace = pace.max(began.elapsed());
                        if stopping.wait_retry(pace).unwrap_or(true) {
                            return;
                        }
                        for old in receive.try_iter() {
                            drop(old);
                        }
                        queued.envelope = None;
                        continue;
                    }
                }
            }
            let Some(envelope) = queued.envelope.take() else {
                break;
            };
            let Some(next) = seq.checked_add(1) else {
                report(NodeId(peer.node), "peer sequence exhausted");
                return;
            };
            seq = next;
            let packet = match packet(envelope, &models, epoch, seq, queued.observations.clone()) {
                Ok(packet) => packet,
                Err(why) => {
                    report(NodeId(peer.node), &why.to_string());
                    break;
                }
            };
            let Some(connection) = tunnel.as_mut() else {
                return;
            };
            match connection.call(dto::Command::Hear { from: id.0, packet }) {
                Ok(Ok(_)) => break,
                Ok(Err(why)) => {
                    // A typed application refusal is not a transport retry. Keep ownership and let
                    // later Delete/Holds progress; it alone is not proof that the box is absent.
                    report(NodeId(peer.node), &why.to_string());
                    break;
                }
                Err(why) => {
                    report(NodeId(peer.node), &why.to_string());
                    tunnel = None;
                    pace = pace.max(began.elapsed());
                    if stopping.wait_retry(pace).unwrap_or(true) {
                        return;
                    }
                    for old in receive.try_iter() {
                        drop(old);
                    }
                    // The next Describe feedback reconstructs intents from durable state, rather
                    // than replaying a stale Start ahead of a committed deletion.
                    continue;
                }
            }
        }
    }
}

#[derive(Default)]
struct Dispatch {
    refusals: Vec<Vec<u8>>,
    notices: Vec<Notice>,
}

/// A scheduler socket serves control only. Box execution continues through its configured Runtime
/// endpoints, so a caller cannot bypass placement admission by calling Create on this socket.
pub struct Service {
    id: NodeId,
    command: Mutex<()>,
    fault: Mutex<Option<Refusal>>,
    cache: Mutex<cache::Cache>,
    _lease: std::fs::File,
    order: Mutex<order::Order>,
    driver: Driver,
    cluster: Mutex<Cluster>,
    prober: Mutex<crate::facts::Prober>,
    stopping: Arc<Stopping>,
    queued: Arc<AtomicUsize>,
    senders: BTreeMap<NodeId, SyncSender<Queued>>,
    refreshing: BTreeMap<NodeId, Arc<AtomicBool>>,
}

impl Service {
    pub fn open(config: &Config, report: Report) -> Result<Arc<Self>, Refusal> {
        std::fs::create_dir_all(&config.state)
            .map_err(|e| Refusal::os("creating scheduler state", e))?;
        let lease = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(config.state.join("scheduler.lock"))
            .map_err(|e| Refusal::os("opening scheduler state lease", e))?;
        lease.try_lock().map_err(|e| {
            Refusal::Broken(format!(
                "scheduler state is already held or cannot be locked: {e}"
            ))
        })?;
        let order = order::Order::open(&config.state.join("transport"))?;
        let epoch = order.epoch;
        let mut cluster = Cluster::single_owner(config.nodes.clone(), Vec::new());
        cluster.models = config.models.clone();
        let id = NodeId(config.node);
        if cluster.nodes.is_empty() {
            let ids: std::collections::BTreeSet<_> =
                config.peers.iter().map(|p| p.node).chain([id.0]).collect();
            let count = u32::try_from(ids.len()).map_err(|e| Refusal::Invalid {
                what: "scheduler nodes",
                why: e.to_string(),
            })?;
            if ids.len() != config.peers.len().saturating_add(1) || ids.iter().copied().ne(0..count)
            {
                return Err(Refusal::Invalid {
                    what: "scheduler nodes",
                    why: "node IDs must be dense from zero, with one route per remote node".into(),
                });
            }
            cluster.nodes = ids
                .into_iter()
                .map(|id| {
                    let mut spec = NodeSpec::new(NodeId(id), hex_sim::model::Resources::default());
                    spec.classes = Classes::NONE;
                    spec
                })
                .collect();
        }
        if !cluster.nodes.iter().any(|node| node.id == id) {
            return Err(Refusal::Invalid {
                what: "scheduler node",
                why: "own ID is absent from the configured peer facts".into(),
            });
        }
        let remote: std::collections::BTreeSet<_> = cluster
            .nodes
            .iter()
            .filter(|n| n.id != id)
            .map(|n| n.id.0)
            .collect();
        let routes: std::collections::BTreeSet<_> = config.peers.iter().map(|p| p.node).collect();
        if routes != remote || routes.len() != config.peers.len() {
            return Err(Refusal::Invalid {
                what: "scheduler peers",
                why: "configure exactly one authenticated route for every remote node".into(),
            });
        }
        if routes.len() >= crate::records::MOST {
            return Err(Refusal::Exhausted {
                what: "configured peer records",
                bound: crate::records::MOST.saturating_sub(1),
            });
        }
        let cache = cache::Cache::open(&config.state.join("peers"), &routes, &mut cluster)?;
        let mut classes = Classes::NONE;
        let mut backends = Vec::new();
        for backend in &config.backends {
            let class = match backend.class.as_str() {
                "oci" => hex_sim::model::Class::Oci,
                "shards" => hex_sim::model::Class::Shards,
                "firecracker" => hex_sim::model::Class::Firecracker,
                other => {
                    return Err(Refusal::Invalid {
                        what: "runtime class",
                        why: other.into(),
                    });
                }
            };
            let runtime =
                Arc::new(hex_client::Client::connect(&backend.socket)?) as Arc<dyn Runtime>;
            classes = classes.with(class);
            backends.push((class, runtime));
        }
        std::fs::create_dir_all(&config.state)
            .map_err(|e| Refusal::os("creating scheduler state", e))?;
        let mut prober =
            crate::facts::Prober::open(&config.state, crate::facts::Declared::default())
                .with_classes(classes);
        let facts = prober.probe()?;
        let local = facts
            .node(id, &mut cluster)
            .map_err(|e| Refusal::Broken(e.to_string()))?;
        // The generic Runtime seam has no device-attachment call. Its driver refuses device asks,
        // and therefore this endpoint offers none until an attachment-capable backend is installed.
        let mut local = local;
        local.capacity.gpus = 0;
        local.devices.clear();
        if let Some(slot) = cluster.nodes.iter_mut().find(|node| node.id == id) {
            *slot = local;
        }
        let driver = Driver::boot(
            id,
            &cluster.nodes,
            &cluster.models,
            Budget {
                owned: hex_runtime::MAX_BOXES,
                ran: hex_runtime::MAX_BOXES,
            },
            Pick::default(),
            &config.state.join("placements"),
            Runtimes::new(backends).map_err(failed)?,
        )
        .map_err(failed)?;
        let mut senders = BTreeMap::new();
        let mut refreshing = BTreeMap::new();
        let queued = Arc::new(AtomicUsize::new(0));
        let stopping = Arc::new(Stopping::new(config.peers.iter().map(|p| p.node)));
        let source = Arc::new(Mutex::new(Weak::<Service>::new()));
        for peer in &config.peers {
            let node = NodeId(peer.node);
            if node == id
                || !cluster.nodes.iter().any(|n| n.id == node)
                || senders.contains_key(&node)
            {
                return Err(Refusal::Invalid {
                    what: "scheduler peer",
                    why: format!("invalid or duplicate node {}", peer.node),
                });
            }
            let (send, receive) = sync_channel::<Queued>(crate::records::MOST);
            let peer = peer.clone();
            let models = cluster.models.clone();
            let stopping = Arc::clone(&stopping);
            let report = Arc::clone(&report);
            let source = Arc::clone(&source);
            std::thread::Builder::new()
                .name(format!("hex-peer-{}", peer.node))
                .spawn(move || {
                    run_peer(
                        PeerLane {
                            peer,
                            id,
                            models,
                            epoch,
                            stopping,
                            report,
                            source,
                        },
                        receive,
                    )
                })
                .map_err(|e| Refusal::os("starting authenticated peer delivery", e))?;
            refreshing.insert(node, Arc::new(AtomicBool::new(false)));
            senders.insert(node, send);
        }
        let service = Arc::new(Self {
            id,
            command: Mutex::new(()),
            fault: Mutex::new(None),
            cache: Mutex::new(cache),
            _lease: lease,
            order: Mutex::new(order),
            driver,
            cluster: Mutex::new(cluster),
            prober: Mutex::new(prober),
            stopping,
            queued,
            senders,
            refreshing,
        });
        *source
            .lock()
            .map_err(|_| Refusal::Broken("peer feedback lock poisoned".into()))? =
            Arc::downgrade(&service);
        service.refresh_peers()?;
        let recovered = service.driver.take_recovered().map_err(failed)?;
        service.dispatch(recovered)?;
        Ok(service)
    }

    /// Cancels and reaps only this service's authenticated peer children.
    pub fn shutdown(&self) -> Result<(), Refusal> {
        self.stopping
            .stop()
            .map_err(|e| Refusal::os("stopping scheduler peer connections", e))
    }

    fn observations(&self) -> Result<Vec<dto::LaunchObservation>, Refusal> {
        Ok(self
            .driver
            .start_observations()
            .map_err(failed)?
            .into_iter()
            .filter(|row| row.node == self.id)
            .map(|row| dto::LaunchObservation {
                class: match row.class {
                    hex_sim::model::Class::Oci => "oci",
                    hex_sim::model::Class::Firecracker => "firecracker",
                    hex_sim::model::Class::Shards => "shards",
                }
                .into(),
                sum_ns: row.sum_ns.to_string(),
                count: row.count,
            })
            .collect())
    }

    fn merge_observations(
        &self,
        from: u32,
        rows: &[dto::LaunchObservation],
    ) -> Result<(), Refusal> {
        let classes =
            usize::try_from(Classes::ALL.count()).map_err(|e| Refusal::Broken(e.to_string()))?;
        if rows.len() > classes {
            return Err(Refusal::Exhausted {
                what: "peer launch classes",
                bound: classes,
            });
        }
        let mut seen = std::collections::BTreeSet::new();
        let observations = rows
            .iter()
            .map(|row| {
                let class = match row.class.as_str() {
                    "oci" => hex_sim::model::Class::Oci,
                    "firecracker" => hex_sim::model::Class::Firecracker,
                    "shards" => hex_sim::model::Class::Shards,
                    _ => {
                        return Err(Refusal::Invalid {
                            what: "peer launch class",
                            why: row.class.clone(),
                        });
                    }
                };
                let sum_ns = row.sum_ns.parse::<u128>().map_err(|e| Refusal::Invalid {
                    what: "peer launch sum",
                    why: e.to_string(),
                })?;
                if !seen.insert(class)
                    || row.count == 0
                    || sum_ns > u128::from(u64::MAX).saturating_mul(u128::from(row.count))
                {
                    return Err(Refusal::Invalid {
                        what: "peer launch aggregate",
                        why: "duplicate class or inconsistent aggregate".into(),
                    });
                }
                Ok(hex_sim::start_cost::StartObservation {
                    node: NodeId(from),
                    class,
                    sum_ns,
                    count: row.count,
                })
            })
            .collect::<Result<Vec<_>, Refusal>>()?;
        self.driver
            .merge_peer_start_observations(NodeId(from), &observations)
            .map_err(failed)
    }

    fn refresh_peers(&self) -> Result<(), Refusal> {
        for (node, sender) in &self.senders {
            let refreshing = self
                .refreshing
                .get(node)
                .ok_or_else(|| Refusal::Broken("peer refresh gate absent".into()))?;
            if refreshing
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                continue;
            }
            let refresh = RefreshPermit(Arc::clone(refreshing));
            self.queued
                .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                    n.checked_add(1).filter(|n| *n <= crate::records::MOST)
                })
                .map_err(|_| Refusal::Exhausted {
                    what: "queued peer messages",
                    bound: crate::records::MOST,
                })?;
            let queued = Queued {
                envelope: None,
                observations: Vec::new(),
                _permit: Permit(Arc::clone(&self.queued)),
                _refresh: Some(refresh),
            };
            sender.try_send(queued).map_err(|_| Refusal::Exhausted {
                what: "queued peer messages",
                bound: crate::records::MOST,
            })?;
        }
        Ok(())
    }

    fn dispatch(&self, step: Step) -> Result<Dispatch, Refusal> {
        let mut result = Dispatch::default();
        let mut work = VecDeque::from([step]);
        while let Some(step) = work.pop_front() {
            for notice in step.notices {
                match &notice {
                    Notice::Refused { refusal, .. } => {
                        result.refusals.push(encoded_refusal(refusal)?)
                    }
                    Notice::Unknown {
                        refusal, cleanup, ..
                    } => {
                        result.refusals.push(encoded_refusal(refusal)?);
                        if let Some(cleanup) = cleanup {
                            result.refusals.push(encoded_refusal(cleanup)?);
                        }
                    }
                    _ => {}
                }
                result.notices.push(notice);
            }
            for (node, envelope) in step.send {
                if node == self.id {
                    if work.len() >= crate::records::MOST {
                        return Err(Refusal::Exhausted {
                            what: "local placement transitions",
                            bound: crate::records::MOST,
                        });
                    }
                    work.push_back(self.driver.hear(self.id, envelope).map_err(failed)?);
                    continue;
                }
                let delivery = self
                    .senders
                    .get(&node)
                    .ok_or_else(|| Refusal::Invalid {
                        what: "scheduler peer",
                        why: format!("no authenticated route to node {}", node.0),
                    })
                    .and_then(|sender| {
                        if matches!(envelope.msg, peer::Msg::Delete { .. }) {
                            self.stopping.interrupt_peer(node.0).map_err(|e| {
                                Refusal::os("interrupting a peer for deletion control", e)
                            })?;
                        }
                        self.queued
                            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                                n.checked_add(1).filter(|n| *n <= crate::records::MOST)
                            })
                            .map_err(|_| Refusal::Exhausted {
                                what: "queued peer messages",
                                bound: crate::records::MOST,
                            })?;
                        let permit = Permit(Arc::clone(&self.queued));
                        let observations = self.observations()?;
                        let queued = Queued {
                            envelope: Some(envelope),
                            observations,
                            _permit: permit,
                            _refresh: None,
                        };
                        sender.try_send(queued).map_err(|_| Refusal::Exhausted {
                            what: "queued peer messages",
                            bound: crate::records::MOST,
                        })?;
                        Ok(())
                    });
                if let Err(why) = delivery {
                    result.refusals.push(encoded_refusal(&why)?);
                }
            }
        }
        Ok(result)
    }

    fn refresh(&self) -> Result<(), Refusal> {
        let owned = self.driver.measured_owned_use().map_err(failed)?;
        let facts = self
            .prober
            .lock()
            .map_err(|_| Refusal::Broken("scheduler probe lock poisoned".into()))?
            .probe()?;
        let mut cluster = self
            .cluster
            .lock()
            .map_err(|_| Refusal::Broken("scheduler model lock poisoned".into()))?;
        let mut local = facts
            .node_with_owned(self.id, &mut cluster, owned)
            .map_err(|e| Refusal::Broken(e.to_string()))?;
        local.capacity.gpus = 0;
        local.devices.clear();
        self.driver.refresh_node(local.clone()).map_err(failed)?;
        if let Some(node) = cluster.nodes.iter_mut().find(|n| n.id == self.id) {
            *node = local;
        }
        Ok(())
    }

    pub fn command(&self, command: dto::Command) -> Result<dto::Reply, Refusal> {
        // LB03: commit order must remain publication order through local effects and enqueue.
        // A delete cannot overtake a committed Start whose caller has not published it yet.
        let _command = self.command.try_lock().map_err(|why| match why {
            std::sync::TryLockError::WouldBlock => Refusal::Exhausted {
                what: "scheduler command publication",
                bound: 1,
            },
            std::sync::TryLockError::Poisoned(_) => {
                Refusal::Broken("scheduler command lock poisoned".into())
            }
        })?;
        if let Some(why) = self
            .fault
            .lock()
            .map_err(|_| Refusal::Broken("scheduler fault lock poisoned".into()))?
            .clone()
        {
            return Err(why);
        }

        // Current facts gate new admission. Refresh errors stop admission; an old, larger offer is
        // never used merely because the host can no longer establish its current capacity.
        if matches!(&command, dto::Command::Submit { .. } | dto::Command::Retry)
            && let Err(why) = self.refresh()
        {
            self.driver.block_admission(why).map_err(failed)?;
        }

        let mut requested = None;
        let step = match command {
            dto::Command::Describe { from } => {
                if !self.senders.contains_key(&NodeId(from)) {
                    return Err(Refusal::Invalid {
                        what: "peer description",
                        why: "sender is not configured".into(),
                    });
                }
                self.refresh()?;
                let cluster = self
                    .cluster
                    .lock()
                    .map_err(|_| Refusal::Broken("scheduler model lock poisoned".into()))?;
                let spec = cluster
                    .nodes
                    .iter()
                    .find(|n| n.id == self.id)
                    .ok_or_else(|| Refusal::Broken("own facts absent".into()))?;
                return Ok(dto::Reply {
                    offer: Some(offer::from_local(spec, &cluster.models)?),
                    observations: self.observations()?,
                    ..dto::Reply::default()
                });
            }
            dto::Command::Advertise {
                from,
                offer: incoming,
                observations,
            } => {
                if !self.senders.contains_key(&NodeId(from)) {
                    return Err(Refusal::Invalid {
                        what: "peer advertisement",
                        why: "sender is not configured".into(),
                    });
                }
                let mut cluster = self
                    .cluster
                    .lock()
                    .map_err(|_| Refusal::Broken("scheduler model lock poisoned".into()))?;
                let spec = offer::decode(&incoming, NodeId(from), &mut cluster)?;
                self.driver.refresh_node(spec.clone()).map_err(failed)?;
                if let Err(why) = self
                    .cache
                    .lock()
                    .map_err(|_| Refusal::Broken("peer cache lock poisoned".into()))?
                    .put(NodeId(from), &incoming)
                {
                    *self
                        .fault
                        .lock()
                        .map_err(|_| Refusal::Broken("scheduler fault lock poisoned".into()))? =
                        Some(why.clone());
                    return Err(why);
                }
                if let Some(slot) = cluster.nodes.iter_mut().find(|n| n.id == NodeId(from)) {
                    *slot = spec;
                }
                drop(cluster);
                self.merge_observations(from, &observations)?;
                let mut step = self.driver.link_up(NodeId(from)).map_err(failed)?;
                let retry = self.driver.retry().map_err(failed)?;
                step.send.extend(retry.send);
                step.notices.extend(retry.notices);
                step
            }
            dto::Command::Submit { demand, payload } => {
                let mut cluster = self
                    .cluster
                    .lock()
                    .map_err(|_| Refusal::Broken("scheduler model lock poisoned".into()))?;
                let needs = hex_sim::needs::parse(&demand).map_err(|why| Refusal::Invalid {
                    what: "placement demand",
                    why,
                })?;
                if needs
                    .gpu_model
                    .as_ref()
                    .is_some_and(|name| !cluster.models.contains(name))
                {
                    return Err(Refusal::Invalid {
                        what: "GPU model",
                        why: "no configured peer reported that model".into(),
                    });
                }
                let class = needs.class.ok_or_else(|| Refusal::Invalid {
                    what: "runtime class",
                    why: "name class:oci, class:shards or class:firecracker explicitly".into(),
                })?;
                let ask = needs.ask(&mut cluster).map_err(|e| Refusal::Invalid {
                    what: "placement demand",
                    why: e.to_string(),
                })?;
                let demand = hex_sim::model::Demand {
                    need: needs.need,
                    kind: hex_sim::model::Kind::Plain,
                    class,
                    ask,
                };
                drop(cluster);
                let (key, step) = self
                    .driver
                    .submit(
                        demand,
                        Payload {
                            spec: payload.spec,
                            start: payload.start,
                        },
                    )
                    .map_err(failed)?;
                requested = Some(key);
                step
            }
            dto::Command::Hear { from, packet } => {
                if from != self.id.0 && !self.senders.contains_key(&NodeId(from)) {
                    return Err(Refusal::Invalid {
                        what: "scheduler sender",
                        why: format!("unconfigured peer {from}"),
                    });
                }
                let mut order = self
                    .order
                    .lock()
                    .map_err(|_| Refusal::Broken("peer order lock poisoned".into()))?;
                use sha2::Digest;
                let encoded = serde_json::to_vec(&packet).map_err(|e| Refusal::Invalid {
                    what: "peer packet",
                    why: e.to_string(),
                })?;
                let fingerprint: [u8; 32] = sha2::Sha256::digest(&encoded).into();
                if !order.accept(from, packet.epoch, packet.seq, fingerprint)? {
                    return Ok(dto::Reply {
                        key: None,
                        at: None,
                        physical: None,
                        created: false,
                        ended: false,
                        refusals: Vec::new(),
                        ..dto::Reply::default()
                    });
                }
                self.merge_observations(from, &packet.observations)?;
                let cluster = self
                    .cluster
                    .lock()
                    .map_err(|_| Refusal::Broken("scheduler model lock poisoned".into()))?;
                let message = envelope(packet, &cluster.models)?;
                drop(cluster);
                if matches!(message.msg, peer::Msg::Start { .. })
                    && let Err(why) = self.refresh()
                {
                    self.driver.block_admission(why).map_err(failed)?;
                }
                let step = self.driver.hear(NodeId(from), message).map_err(failed)?;
                drop(order);
                step
            }
            dto::Command::LinkUp { peer } => self.driver.link_up(NodeId(peer)).map_err(failed)?,
            dto::Command::Retry => {
                self.refresh_peers()?;
                self.driver.retry().map_err(failed)?
            }
            dto::Command::Delete { key: k } => {
                requested = Some(key(k));
                self.driver.delete(key(k)).map_err(failed)?
            }
            dto::Command::Status { key: k } => {
                self.refresh_peers()?;
                requested = Some(key(k));
                self.driver.poll_status().map_err(failed)?
            }
        };
        let dispatch = self.dispatch(step)?;
        let status = requested
            .map(|key| self.driver.status(key).map_err(failed))
            .transpose()?
            .flatten();
        Ok(dto::Reply {
            key: requested.map(external),
            tracked: status.is_some(),
            at: status.as_ref().and_then(|s| s.node).map(|n| n.0),
            physical: status.as_ref().map(|s| s.physical.clone()),
            created: status.as_ref().is_some_and(|s| s.created)
                || dispatch
                    .notices
                    .iter()
                    .any(|n| matches!(n, Notice::Started { key, .. } if Some(*key) == requested)),
            ended: status.as_ref().is_some_and(|s| s.ended)
                || dispatch
                    .notices
                    .iter()
                    .any(|n| matches!(n, Notice::Ended { key } if Some(*key) == requested)),
            refusals: dispatch.refusals,
            ..dto::Reply::default()
        })
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.stopping.stop();
    }
}

fn control_only() -> Refusal {
    Refusal::Invalid {
        what: "scheduler socket",
        why: "use scheduler control here and the configured runtime socket for box operations"
            .into(),
    }
}

impl Runtime for Service {
    fn classes(&self) -> Vec<Class> {
        Vec::new()
    }
    fn create(&self, _: BoxSpec) -> Result<BoxInfo, Refusal> {
        Err(control_only())
    }
    fn start(&self, _: &BoxId) -> Result<(), Refusal> {
        Err(control_only())
    }
    fn exec(&self, _: &BoxId, _: Program, _: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        Err(control_only())
    }
    fn run(&self, _: BoxSpec, _: Program, _: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        Err(control_only())
    }
    fn signal(&self, _: &BoxId, _: Signal) -> Result<(), Refusal> {
        Err(control_only())
    }
    fn inspect(&self, _: &BoxId) -> Result<BoxInfo, Refusal> {
        Err(control_only())
    }
    fn usage(&self, _: &BoxId) -> Result<Usage, Refusal> {
        Err(control_only())
    }
    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        Err(control_only())
    }
    fn delete(&self, _: &BoxId) -> Result<(), Refusal> {
        Err(control_only())
    }
}

impl Host for Service {
    fn schedule(&self, command: dto::Command) -> Result<dto::Reply, Refusal> {
        self.command(command)
    }
    fn abandoned(&self, _: &BoxId) -> Result<(), Refusal> {
        Err(control_only())
    }
    fn netns(&self) -> Result<hex_core::netns::Counts, Refusal> {
        Err(control_only())
    }
}

/// A configuration is a wire-sized input, refused before allocation past that bound.
pub fn read_config(path: &Path) -> Result<Config, Refusal> {
    use std::io::Read;
    let file =
        std::fs::File::open(path).map_err(|e| Refusal::os("opening scheduler configuration", e))?;
    let mut bytes = Vec::new();
    file.take(
        u64::try_from(wire::MAX_MESSAGE)
            .unwrap_or(u64::MAX)
            .saturating_add(1),
    )
    .read_to_end(&mut bytes)
    .map_err(|e| Refusal::os("reading scheduler configuration", e))?;
    if bytes.len() > wire::MAX_MESSAGE {
        return Err(Refusal::Exhausted {
            what: "scheduler configuration bytes",
            bound: wire::MAX_MESSAGE,
        });
    }
    serde_json::from_slice(&bytes).map_err(|e| Refusal::Invalid {
        what: "scheduler configuration",
        why: e.to_string(),
    })
}

#[cfg(test)]
mod tests;
