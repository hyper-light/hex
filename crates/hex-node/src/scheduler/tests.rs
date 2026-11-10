use super::*;
use hex_api::{Bounds, ClassSpec};
use hex_sim::model::{Ask, Demand, Kind, Resources};

/// LB03: a reconnect republishes durable cancellation without killing that new connection.
#[test]
fn replayed_deletion_does_not_interrupt_the_reconnected_peer() {
    let dir = tempfile::tempdir().unwrap();
    let need = Resources {
        cpu_milli: u64::from(hex_api::MIN_CPU_MILLI),
        mem_bytes: hex_api::MIN_MEMORY,
        ..Resources::default()
    };
    let mut local = NodeSpec::new(NodeId(0), Resources::default());
    local.classes = Classes::NONE;
    let mut cluster =
        Cluster::single_owner(vec![local, NodeSpec::new(NodeId(1), need)], Vec::new());
    let driver = Driver::boot(
        NodeId(0),
        &cluster.nodes,
        &cluster.models,
        Budget {
            owned: hex_runtime::MAX_BOXES,
            ran: hex_runtime::MAX_BOXES,
        },
        Pick::default(),
        &dir.path().join("placements"),
        Runtimes::default(),
    )
    .unwrap();
    let (key, _) = driver
        .submit(
            Demand {
                need,
                kind: Kind::Plain,
                class: hex_sim::model::Class::Oci,
                ask: Ask::default(),
            },
            Payload {
                spec: BoxSpec {
                    id: BoxId::new("cancel-me").unwrap(),
                    bounds: Bounds {
                        cpu_max_milli: Some(hex_api::MIN_CPU_MILLI),
                        memory_max: Some(hex_api::MIN_MEMORY),
                        ..Bounds::default()
                    },
                    class: ClassSpec::Process,
                    owner: None,
                },
                start: false,
            },
        )
        .unwrap();
    driver.delete(key).unwrap(); // Durable deletion, with its prior connection lost.
    let stopping = Arc::new(Stopping::new([1]));
    let (child, _input, _output) = stopping::tests::waiting_child();
    stopping.register(1, &child).unwrap();
    let (sender, receiver) = sync_channel(crate::records::MOST);
    let cache = cache::Cache::open(
        &dir.path().join("peers"),
        &std::collections::BTreeSet::from([1]),
        &mut cluster,
    )
    .unwrap();
    let service = Service {
        id: NodeId(0),
        realm: "test".into(),
        command: Mutex::new(()),
        fault: Mutex::new(None),
        cache: Mutex::new(cache),
        _lease: std::fs::File::create(dir.path().join("lease")).unwrap(),
        order: Mutex::new(order::Order::open(&dir.path().join("transport")).unwrap()),
        driver,
        runtimes: Runtimes::default(),
        cluster: Mutex::new(cluster),
        prober: Mutex::new(crate::facts::Prober::open(
            dir.path(),
            crate::facts::Declared::default(),
        )),
        stopping,
        queued: Arc::new(AtomicUsize::new(0)),
        senders: BTreeMap::from([(NodeId(1), sender)]),
        refreshing: BTreeMap::from([(NodeId(1), Arc::new(AtomicBool::new(false)))]),
    };
    service.command(dto::Command::LinkUp { peer: 1 }).unwrap();
    assert!(
        matches!(receiver.try_recv().unwrap().envelope, Some(Envelope { msg: peer::Msg::Delete { key: k, .. }, .. }) if k == key)
    );
    assert!(
        child.lock().unwrap().try_wait().unwrap().is_none(),
        "replayed Delete must keep the fresh connection alive"
    );
    // An explicit cancellation request still interrupts a stalled connection, before queue room.
    service
        .command(dto::Command::Delete { key: external(key) })
        .unwrap();
    assert!(child.lock().unwrap().try_wait().unwrap().is_some());
}

/// LB03/AC-T7.4.1: the production laptop dispatch drains the same durable messages that a peer
/// transport would send. Then the real common socket/client observes and deletes the actual box.
#[test]
fn laptop_dispatch_and_common_wire_keep_the_box_until_explicit_delete() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(hex_api::sim::SimRuntime::default());
    let need = Resources {
        cpu_milli: u64::from(hex_api::MIN_CPU_MILLI),
        mem_bytes: hex_api::MIN_MEMORY,
        ..Resources::default()
    };
    let cluster = Cluster::single_owner(vec![NodeSpec::new(NodeId(0), need)], Vec::new());
    let driver = Driver::boot(
        NodeId(0),
        &cluster.nodes,
        &cluster.models,
        Budget {
            owned: hex_runtime::MAX_BOXES,
            ran: hex_runtime::MAX_BOXES,
        },
        Pick::default(),
        &dir.path().join("placements"),
        Runtimes::new([(
            hex_sim::model::Class::Oci,
            runtime.clone() as Arc<dyn Runtime>,
        )])
        .unwrap(),
    )
    .unwrap();
    let cache = cache::Cache::open(
        &dir.path().join("peers"),
        &std::collections::BTreeSet::new(),
        &mut cluster.clone(),
    )
    .unwrap();
    let service = Arc::new(Service {
        id: NodeId(0),
        realm: "test".into(),
        command: Mutex::new(()),
        fault: Mutex::new(None),
        cache: Mutex::new(cache),
        _lease: std::fs::File::create(dir.path().join("lease")).unwrap(),
        order: Mutex::new(order::Order::open(&dir.path().join("transport")).unwrap()),
        driver,
        runtimes: Runtimes::default(),
        cluster: Mutex::new(cluster),
        prober: Mutex::new(crate::facts::Prober::open(
            dir.path(),
            crate::facts::Declared::default(),
        )),
        stopping: Arc::new(Stopping::default()),
        queued: Arc::new(AtomicUsize::new(0)),
        senders: BTreeMap::new(),
        refreshing: BTreeMap::new(),
    });
    let demand = Demand {
        need,
        kind: Kind::Plain,
        class: hex_sim::model::Class::Oci,
        ask: Ask::default(),
    };
    let payload = Payload {
        spec: BoxSpec {
            id: BoxId::new("caller-name").unwrap(),
            bounds: Bounds {
                cpu_max_milli: Some(hex_api::MIN_CPU_MILLI),
                memory_max: Some(hex_api::MIN_MEMORY),
                ..Bounds::default()
            },
            class: ClassSpec::Process,
            owner: None,
        },
        start: false,
    };
    let (key, step) = service.driver.submit(demand, payload).unwrap();
    assert!(runtime.list().unwrap().is_empty());
    let dispatch = service.dispatch(step).unwrap();
    assert!(dispatch.refusals.is_empty());
    assert!(
        dispatch
            .notices
            .iter()
            .any(|n| matches!(n, Notice::Started { key: k, .. } if *k == key))
    );
    let physical = service.driver.status(key).unwrap().unwrap().physical;
    assert_eq!(
        runtime.inspect(&physical).unwrap().state,
        hex_api::BoxState::Empty
    );

    #[cfg(target_os = "linux")]
    let platform = crate::sizes::Sizes::now().unwrap().platform();
    #[cfg(target_os = "macos")]
    let platform = crate::serve::macos::platform().unwrap();
    #[cfg(windows)]
    let platform = crate::serve::windows::platform().unwrap();
    let socket = dir.path().join("scheduler.sock");
    let server = crate::serve::Server::bind(
        &socket,
        service.clone() as Arc<dyn Host>,
        crate::serve::limits(&platform),
        platform.threads,
    )
    .unwrap();
    let client = hex_client::Client::connect(&socket).unwrap();
    // A second worker cannot publish Delete ahead of an in-progress command's Start.
    let gate = service.command.lock().unwrap();
    assert!(matches!(
        client.schedule(dto::Command::Delete { key: external(key) }),
        Err(Refusal::Exhausted {
            what: "scheduler command publication",
            ..
        })
    ));
    assert_eq!(runtime.list().unwrap().len(), 1);
    drop(gate);
    let status = client
        .schedule(dto::Command::Status { key: external(key) })
        .unwrap();
    assert!(matches!(
        client.schedule(dto::Command::Describe {
            from: 0,
            cluster: "another-cluster".into()
        }),
        Err(Refusal::Invalid {
            what: "scheduler cluster",
            ..
        })
    ));
    assert!(status.tracked);
    assert!(status.created);
    assert!(!status.ended);
    assert_eq!(status.physical, Some(physical.clone()));
    assert!(matches!(
        client.create(BoxSpec {
            id: BoxId::new("bypass").unwrap(),
            bounds: Bounds::default(),
            class: ClassSpec::Process,
            owner: None
        }),
        Err(Refusal::Invalid {
            what: "scheduler socket",
            ..
        })
    ));
    let deleted = client
        .schedule(dto::Command::Delete { key: external(key) })
        .unwrap();
    assert!(deleted.ended);
    assert!(runtime.list().unwrap().is_empty());
    drop(client);
    drop(server);
}

/// AC-T7.4.1/LB03: the first remote-created feedback meets publication Busy before any successful
/// outgoing feedback RTT. Release publication by event, not elapsed sleep, then require this same
/// worker to persist/ack creation without an administrator Retry/LinkUp or another queued message.
#[test]
fn cold_remote_creation_feedback_survives_owner_publication_busy() {
    let dir = tempfile::tempdir().unwrap();
    let need = Resources {
        cpu_milli: u64::from(hex_api::MIN_CPU_MILLI),
        mem_bytes: hex_api::MIN_MEMORY,
        ..Resources::default()
    };
    let mut local = NodeSpec::new(NodeId(0), Resources::default());
    local.classes = Classes::NONE;
    let mut cluster =
        Cluster::single_owner(vec![local, NodeSpec::new(NodeId(1), need)], Vec::new());
    let owner_driver = Driver::boot(
        NodeId(0),
        &cluster.nodes,
        &cluster.models,
        Budget {
            owned: hex_runtime::MAX_BOXES,
            ran: hex_runtime::MAX_BOXES,
        },
        Pick::default(),
        &dir.path().join("owner-placements"),
        Runtimes::default(),
    )
    .unwrap();
    let runtime = Arc::new(hex_api::sim::SimRuntime::default());
    let receiver = Driver::boot(
        NodeId(1),
        &cluster.nodes,
        &cluster.models,
        Budget {
            owned: hex_runtime::MAX_BOXES,
            ran: hex_runtime::MAX_BOXES,
        },
        Pick::default(),
        &dir.path().join("receiver-placements"),
        Runtimes::new([(
            hex_sim::model::Class::Oci,
            runtime.clone() as Arc<dyn Runtime>,
        )])
        .unwrap(),
    )
    .unwrap();
    let (owner_send, owner_receive) = sync_channel(crate::records::MOST);
    let owner = Arc::new(Service {
        id: NodeId(0),
        realm: "cold-feedback".into(),
        command: Mutex::new(()),
        fault: Mutex::new(None),
        cache: Mutex::new(
            cache::Cache::open(
                &dir.path().join("peers"),
                &std::collections::BTreeSet::from([1]),
                &mut cluster,
            )
            .unwrap(),
        ),
        _lease: std::fs::File::create(dir.path().join("lease")).unwrap(),
        order: Mutex::new(order::Order::open(&dir.path().join("transport")).unwrap()),
        driver: owner_driver,
        runtimes: Runtimes::default(),
        cluster: Mutex::new(cluster),
        prober: Mutex::new(crate::facts::Prober::open(
            dir.path(),
            crate::facts::Declared::default(),
        )),
        stopping: Arc::new(Stopping::new([1])),
        queued: Arc::new(AtomicUsize::new(0)),
        senders: BTreeMap::from([(NodeId(1), owner_send)]),
        refreshing: BTreeMap::from([(NodeId(1), Arc::new(AtomicBool::new(false)))]),
    });
    let (key, submitted) = owner
        .driver
        .submit(
            Demand {
                need,
                kind: Kind::Plain,
                class: hex_sim::model::Class::Oci,
                ask: Ask::default(),
            },
            Payload {
                spec: BoxSpec {
                    id: BoxId::new("first-cold-feedback").unwrap(),
                    bounds: Bounds {
                        cpu_max_milli: Some(hex_api::MIN_CPU_MILLI),
                        memory_max: Some(hex_api::MIN_MEMORY),
                        ..Bounds::default()
                    },
                    class: ClassSpec::Process,
                    owner: None,
                },
                start: false,
            },
        )
        .unwrap();
    owner.dispatch(submitted).unwrap();
    let start = owner_receive.recv().unwrap().envelope.unwrap();
    assert!(matches!(start.msg, peer::Msg::Start { key: k, .. } if k == key));
    let created = receiver.hear(NodeId(0), start).unwrap();
    assert_eq!(
        runtime.list().unwrap().len(),
        1,
        "receiver physically created its box"
    );
    assert!(receiver.status(key).unwrap().unwrap().created);
    assert!(!owner.driver.status(key).unwrap().unwrap().created);
    let feedback = created
        .send
        .into_iter()
        .find_map(|(to, message)| if to == NodeId(0) { Some(message) } else { None })
        .unwrap();
    assert!(matches!(feedback.msg, peer::Msg::Started { key: k, .. } if k == key));
    assert!(!dir.path().join("transport/peer-1.rec").exists());

    let successes = Arc::new(AtomicUsize::new(0));
    let attempts = Arc::new(AtomicUsize::new(0));
    let (busy, saw_busy) = sync_channel(0);
    let (release, released) = sync_channel(0);
    let publication = owner.command.lock().unwrap();
    let (send, receive) = sync_channel(1);
    let pending = Arc::new(AtomicUsize::new(1));
    send.send(Queued {
        envelope: Some(feedback),
        observations: Vec::new(),
        _permit: Permit(Arc::clone(&pending)),
        _refresh: None,
    })
    .unwrap();
    drop(send); // Exactly one feedback publication; no external retry or link-up is supplied.
    let lane = PeerLane {
        peer: Peer {
            node: 0,
            host: "unused-mock-peer".into(),
            command: "unused-mock-command".into(),
        },
        id: NodeId(1),
        realm: "cold-feedback".into(),
        models: Vec::new(),
        epoch: 1,
        stopping: Arc::new(Stopping::new([0])),
        report: Arc::new(|_, _| {}),
        source: Arc::new(Mutex::new(Weak::new())),
    };
    let worker = {
        let owner = Arc::clone(&owner);
        let successes = Arc::clone(&successes);
        let attempts = Arc::clone(&attempts);
        std::thread::spawn(move || {
            let mut first_busy = Some((busy, released));
            run_peer_with(
                lane,
                receive,
                |_, _| Ok(()),
                |_, command| {
                    let feedback = matches!(command, dto::Command::Hear { .. });
                    let reply = owner.command(command);
                    match &reply {
                        Ok(_) => {
                            successes.fetch_add(1, Ordering::SeqCst);
                        }
                        Err(Refusal::Exhausted {
                            what: "scheduler command publication",
                            ..
                        }) => {
                            assert_eq!(
                                successes.load(Ordering::SeqCst),
                                0,
                                "cold feedback has no prior successful response"
                            );
                            if feedback {
                                attempts.fetch_add(1, Ordering::SeqCst);
                                if let Some((busy, released)) = first_busy.take() {
                                    busy.send(()).unwrap();
                                    released.recv().unwrap();
                                }
                            }
                        }
                        Err(why) => panic!("unexpected mock peer refusal: {why}"),
                    }
                    Ok(reply)
                },
            );
        })
    };
    saw_busy.recv().unwrap();
    assert_eq!(successes.load(Ordering::SeqCst), 0);
    assert!(!owner.driver.status(key).unwrap().unwrap().created);
    assert!(!dir.path().join("transport/peer-1.rec").exists());
    drop(publication);
    release.send(()).unwrap();
    worker.join().unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert_eq!(successes.load(Ordering::SeqCst), 1);
    assert_eq!(pending.load(Ordering::SeqCst), 0);
    assert!(owner.driver.status(key).unwrap().unwrap().created);
    assert!(dir.path().join("transport/peer-1.rec").exists());
    let physical = receiver.status(key).unwrap().unwrap().physical;
    runtime.delete(&physical).unwrap();
}

fn advertise_timing_service(
    id: NodeId,
    dir: &Path,
    mut cluster: Cluster,
    runtimes: Runtimes,
    sender: SyncSender<Queued>,
) -> Arc<Service> {
    std::fs::create_dir_all(dir).unwrap();
    let other = cluster.nodes.iter().find(|node| node.id != id).unwrap().id;
    let driver = Driver::boot(
        id,
        &cluster.nodes,
        &cluster.models,
        Budget {
            owned: hex_runtime::MAX_BOXES,
            ran: hex_runtime::MAX_BOXES,
        },
        Pick::default(),
        &dir.join("placements"),
        runtimes.clone(),
    )
    .unwrap();
    Arc::new(Service {
        id,
        realm: "advertise-time".into(),
        command: Mutex::new(()),
        fault: Mutex::new(None),
        cache: Mutex::new(
            cache::Cache::open(
                &dir.join("peers"),
                &std::collections::BTreeSet::from([other.0]),
                &mut cluster,
            )
            .unwrap(),
        ),
        _lease: std::fs::File::create(dir.join("lease")).unwrap(),
        order: Mutex::new(order::Order::open(&dir.join("transport")).unwrap()),
        driver,
        runtimes,
        cluster: Mutex::new(cluster),
        prober: Mutex::new(crate::facts::Prober::open(
            dir,
            crate::facts::Declared::default(),
        )),
        stopping: Arc::new(Stopping::new([other.0])),
        queued: Arc::new(AtomicUsize::new(0)),
        senders: BTreeMap::from([(other, sender)]),
        refreshing: BTreeMap::from([(other, Arc::new(AtomicBool::new(false)))]),
    })
}

/// AC-T7.4.1/r423: successful SSH Describe is followed by local source Advertise Busy. Its own
/// pauses must not become learned operation cost. Virtual time has one fixed Describe cost; only
/// requested waits advance it thereafter. Three Busy retries are gated before publication opens.
#[test]
fn local_advertise_retry_pace_does_not_learn_its_own_pauses() {
    use std::time::{Duration, Instant};
    let dir = tempfile::tempdir().unwrap();
    let need = Resources {
        cpu_milli: u64::from(hex_api::MIN_CPU_MILLI),
        mem_bytes: hex_api::MIN_MEMORY,
        ..Resources::default()
    };
    let mut local = NodeSpec::new(NodeId(0), Resources::default());
    local.classes = Classes::NONE;
    let cluster = Cluster::single_owner(vec![local, NodeSpec::new(NodeId(1), need)], Vec::new());
    let (owner_send, owner_receive) = sync_channel(crate::records::MOST);
    let owner = advertise_timing_service(
        NodeId(0),
        &dir.path().join("owner"),
        cluster.clone(),
        Runtimes::default(),
        owner_send,
    );
    let runtime = Arc::new(hex_api::sim::SimRuntime::default());
    let (source_send, source_receive) = sync_channel(crate::records::MOST);
    let source = advertise_timing_service(
        NodeId(1),
        &dir.path().join("source"),
        cluster,
        Runtimes::new([(
            hex_sim::model::Class::Oci,
            runtime.clone() as Arc<dyn Runtime>,
        )])
        .unwrap(),
        source_send,
    );
    assert_ne!(source.id, owner.id);
    let (key, submitted) = owner
        .driver
        .submit(
            Demand {
                need,
                kind: Kind::Plain,
                class: hex_sim::model::Class::Oci,
                ask: Ask::default(),
            },
            Payload {
                spec: BoxSpec {
                    id: BoxId::new("local-advertise-timing").unwrap(),
                    bounds: Bounds {
                        cpu_max_milli: Some(hex_api::MIN_CPU_MILLI),
                        memory_max: Some(hex_api::MIN_MEMORY),
                        ..Bounds::default()
                    },
                    class: ClassSpec::Process,
                    owner: None,
                },
                start: false,
            },
        )
        .unwrap();
    owner.dispatch(submitted).unwrap();
    let start = owner_receive.recv().unwrap().envelope.unwrap();
    let created = source.driver.hear(NodeId(0), start).unwrap();
    source.dispatch(created).unwrap();
    assert_eq!(runtime.list().unwrap().len(), 1);
    assert!(!owner.driver.status(key).unwrap().unwrap().created);
    let publication = source.command.lock().unwrap();
    let (waited, waits) = sync_channel(0);
    let (release, released) = sync_channel(0);
    let (applied, was_applied) = sync_channel(0);
    let described = Arc::new(AtomicBool::new(false));
    let pauses = Arc::new(Mutex::new(Duration::ZERO));
    // One virtual nanosecond is the test's fixed work unit, never a production wait/seed.
    let work = Duration::from_nanos(1);
    let began = Instant::now();
    let lane = PeerLane {
        peer: Peer {
            node: 0,
            host: "unused-mock-peer".into(),
            command: "unused-mock-command".into(),
        },
        id: source.id,
        realm: source.realm.clone(),
        models: Vec::new(),
        epoch: 1,
        stopping: Arc::clone(&source.stopping),
        report: Arc::new(|_, why| panic!("unexpected peer failure: {why}")),
        source: Arc::new(Mutex::new(Arc::downgrade(&source))),
    };
    let worker = {
        let owner = Arc::clone(&owner);
        let described_for_call = Arc::clone(&described);
        let pauses_for_wait = Arc::clone(&pauses);
        std::thread::spawn(move || {
            let mut applied = Some(applied);
            let mut released = Some(released);
            let mut attempts = 0;
            run_peer_with_time(
                lane,
                source_receive,
                |_, _| Ok(()),
                |_, command| {
                    let describe = matches!(&command, dto::Command::Describe { .. });
                    let result = owner.command(command);
                    if describe {
                        assert!(
                            result.as_ref().unwrap().offer.is_some(),
                            "first Describe succeeds with an actual owner offer"
                        );
                        described_for_call.store(true, Ordering::SeqCst);
                    } else if result.is_ok()
                        && owner.driver.status(key).unwrap().unwrap().created
                        && let Some(applied) = applied.take()
                    {
                        applied.send(()).unwrap();
                    }
                    Ok(result)
                },
                || {
                    began
                        .checked_add(
                            *pauses.lock().unwrap()
                                + if described.load(Ordering::SeqCst) {
                                    work
                                } else {
                                    Duration::ZERO
                                },
                        )
                        .unwrap()
                },
                |_, pace| {
                    assert!(described_for_call.load(Ordering::SeqCst));
                    attempts += 1;
                    waited.send(pace).unwrap();
                    *pauses_for_wait.lock().unwrap() += pace;
                    if attempts == 3 {
                        released.take().unwrap().recv().unwrap();
                    }
                    Ok(false)
                },
            );
        })
    };
    let observed = [
        waits.recv().unwrap(),
        waits.recv().unwrap(),
        waits.recv().unwrap(),
    ];
    drop(publication);
    release.send(()).unwrap();
    was_applied.recv().unwrap();
    assert!(owner.driver.status(key).unwrap().unwrap().created);
    let physical = source.driver.status(key).unwrap().unwrap().physical;
    drop(source); // Close the source queue after its publication/feedback complete.
    worker.join().unwrap();
    runtime.delete(&physical).unwrap();
    // Clean up before the RED assertion so failing timing evidence never strands a worker.
    assert_eq!(
        observed, [work; 3],
        "only fixed operation work may determine retry pace"
    );
}
