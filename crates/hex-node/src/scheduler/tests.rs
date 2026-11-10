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
