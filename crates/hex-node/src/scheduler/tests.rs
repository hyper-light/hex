use super::*;
use hex_api::{Bounds, ClassSpec};
use hex_sim::model::{Ask, Demand, Kind, Resources};

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
        command: Mutex::new(()),
        fault: Mutex::new(None),
        cache: Mutex::new(cache),
        _lease: std::fs::File::create(dir.path().join("lease")).unwrap(),
        order: Mutex::new(order::Order::open(&dir.path().join("transport")).unwrap()),
        driver,
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
