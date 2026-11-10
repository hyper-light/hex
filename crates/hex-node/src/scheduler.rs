//! CP-T7.4/LB03: file-backed production shell around the adapted peer kernel and Runtime driver.
//! The ordinary node socket carries control. Declared peers use authenticated SSH stdio tunnels;
//! no cluster service or listening TCP socket is required, including on a single laptop.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex};

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

pub mod tunnel;

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

fn packet(envelope: Envelope, models: &[String]) -> Result<dto::Packet, Refusal> {
    Ok(dto::Packet {
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

/// A scheduler socket serves control only. Box execution continues through its configured Runtime
/// endpoints, so a caller cannot bypass placement admission by calling Create on this socket.
pub struct Service {
    id: NodeId,
    driver: Driver,
    cluster: Mutex<Cluster>,
    prober: Mutex<crate::facts::Prober>,
    senders: BTreeMap<NodeId, SyncSender<Envelope>>,
}

impl Service {
    pub fn open(config: &Config) -> Result<Arc<Self>, Refusal> {
        let mut cluster = Cluster::single_owner(config.nodes.clone(), Vec::new());
        cluster.models = config.models.clone();
        let id = NodeId(config.node);
        if cluster.nodes.is_empty() && id == NodeId(0) {
            cluster
                .nodes
                .push(NodeSpec::new(id, hex_sim::model::Resources::default()));
        }
        if !cluster.nodes.iter().any(|node| node.id == id) {
            return Err(Refusal::Invalid {
                what: "scheduler node",
                why: "own ID is absent from the configured peer facts".into(),
            });
        }
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
            &config.state,
            Runtimes::new(backends).map_err(failed)?,
        )
        .map_err(failed)?;
        let mut senders = BTreeMap::new();
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
            let (send, receive) = sync_channel(crate::records::MOST);
            let peer = peer.clone();
            let models = cluster.models.clone();
            std::thread::Builder::new()
                .name(format!("hex-peer-{}", peer.node))
                .spawn(move || {
                    let mut tunnel = None;
                    for message in receive {
                        let delivery = packet(message, &models).and_then(|packet| {
                            if tunnel.is_none() {
                                tunnel = Some(tunnel::Connection::open(&peer)?);
                            }
                            let conn = tunnel
                                .as_mut()
                                .ok_or_else(|| Refusal::Broken("peer tunnel absent".into()))?;
                            conn.call(dto::Command::Hear { from: id.0, packet })
                                .map(|_| ())
                        });
                        if let Err(why) = delivery {
                            tunnel = None;
                            // The durable peer core retains ownership. Silence never releases it; LinkUp/Retry
                            // resends from its records, rather than making an unsafe second placement.
                            eprintln!("hex scheduler peer {}: {why}", peer.node);
                        }
                    }
                })
                .map_err(|e| Refusal::os("starting authenticated peer delivery", e))?;
            senders.insert(node, send);
        }
        let service = Arc::new(Self {
            id,
            driver,
            cluster: Mutex::new(cluster),
            prober: Mutex::new(prober),
            senders,
        });
        let recovered = service.driver.take_recovered().map_err(failed)?;
        service.dispatch(recovered)?;
        Ok(service)
    }

    fn dispatch(&self, step: Step) -> Result<Vec<Vec<u8>>, Refusal> {
        let mut refusals = Vec::new();
        for notice in step.notices {
            match notice {
                Notice::Refused { refusal, .. } => refusals.push(encoded_refusal(&refusal)?),
                Notice::Unknown {
                    refusal, cleanup, ..
                } => {
                    refusals.push(encoded_refusal(&refusal)?);
                    if let Some(cleanup) = cleanup {
                        refusals.push(encoded_refusal(&cleanup)?);
                    }
                }
                _ => {}
            }
        }
        for (node, envelope) in step.send {
            if let Some(why) = &envelope.refusal {
                refusals.push(encoded_refusal(why)?);
            }
            let result = self
                .senders
                .get(&node)
                .ok_or_else(|| Refusal::Invalid {
                    what: "scheduler peer",
                    why: format!("no authenticated route to node {}", node.0),
                })
                .and_then(|sender| {
                    sender.try_send(envelope).map_err(|_| Refusal::Exhausted {
                        what: "queued peer messages",
                        bound: crate::records::MOST,
                    })
                });
            if let Err(why) = result {
                refusals.push(encoded_refusal(&why)?);
            }
        }
        Ok(refusals)
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
        // Current facts gate new admission. Refresh errors stop admission; an old, larger offer is
        // never used merely because the host can no longer establish its current capacity.
        if matches!(
            &command,
            dto::Command::Submit { .. } | dto::Command::Hear { .. } | dto::Command::Retry
        ) {
            self.refresh()?;
        }

        let mut requested = None;
        let step = match command {
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
                let cluster = self
                    .cluster
                    .lock()
                    .map_err(|_| Refusal::Broken("scheduler model lock poisoned".into()))?;
                let message = envelope(packet, &cluster.models)?;
                drop(cluster);
                self.driver.hear(NodeId(from), message).map_err(failed)?
            }
            dto::Command::LinkUp { peer } => self.driver.link_up(NodeId(peer)).map_err(failed)?,
            dto::Command::Retry => self.driver.retry().map_err(failed)?,
            dto::Command::Delete { key: k } => {
                requested = Some(key(k));
                self.driver.delete(key(k)).map_err(failed)?
            }
            dto::Command::Status { key: k } => {
                requested = Some(key(k));
                self.driver.poll_status().map_err(failed)?
            }
        };
        let refusals = self.dispatch(step)?;
        let status = requested
            .map(|key| self.driver.status(key).map_err(failed))
            .transpose()?
            .flatten();
        Ok(dto::Reply {
            key: requested.map(external),
            at: status.as_ref().and_then(|s| s.node).map(|n| n.0),
            created: status.as_ref().is_some_and(|s| s.created),
            ended: status.as_ref().is_some_and(|s| s.ended),
            refusals,
        })
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
