use super::*;
use hex_api::{Bounds, ClassSpec, Network, OciSpec, Proc, Program, Rootfs, Signal, Stdio, Usage};
use hex_sim::model::{Ask, Kind, Resources};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Default)]
struct Counting {
    sim: hex_api::sim::SimRuntime,
    creates: AtomicUsize,
    starts: AtomicUsize,
    deletes: AtomicUsize,
    fail_create: AtomicBool,
    fail_start: AtomicBool,
    fail_delete: AtomicBool,
    fail_record: Mutex<Option<PathBuf>>,
}

impl Runtime for Counting {
    fn classes(&self) -> Vec<hex_api::Class> {
        self.sim.classes()
    }
    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        self.creates.fetch_add(1, Ordering::SeqCst);
        let info = self.sim.create(spec)?;
        if let Some(path) = self.fail_record.lock().unwrap().take() {
            std::fs::create_dir(path).unwrap();
        }
        if self.fail_create.load(Ordering::SeqCst) {
            return Err(Refusal::Os {
                op: "test create",
                why: "after physical creation".into(),
            });
        }
        Ok(info)
    }
    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        if self.fail_start.load(Ordering::SeqCst) {
            return Err(Refusal::Os {
                op: "test start",
                why: "refused".into(),
            });
        }
        self.sim.start(id)
    }
    fn exec(&self, id: &BoxId, p: Program, s: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        self.sim.exec(id, p, s)
    }
    fn run(&self, b: BoxSpec, p: Program, s: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        self.sim.run(b, p, s)
    }
    fn signal(&self, id: &BoxId, s: Signal) -> Result<(), Refusal> {
        self.sim.signal(id, s)
    }
    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        self.sim.inspect(id)
    }
    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        self.sim.usage(id)
    }
    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        self.sim.list()
    }
    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        self.deletes.fetch_add(1, Ordering::SeqCst);
        if self.fail_delete.load(Ordering::SeqCst) {
            return Err(Refusal::Os {
                op: "test delete",
                why: "still present".into(),
            });
        }
        self.sim.delete(id)
    }
}

fn request() -> (Demand, Payload) {
    let need = Resources {
        cpu_milli: 100,
        mem_bytes: hex_api::MIN_MEMORY,
        ..Resources::default()
    };
    let demand = Demand {
        need,
        kind: Kind::Plain,
        class: Class::Oci,
        ask: Ask::default(),
    };
    let payload = Payload {
        spec: BoxSpec {
            id: BoxId::new("caller-name").unwrap(),
            bounds: Bounds {
                cpu_max_milli: Some(100),
                memory_max: Some(need.mem_bytes),
                ..Bounds::default()
            },
            class: ClassSpec::Process,
            owner: None,
        },
        start: false,
    };
    (demand, payload)
}

fn boot(dir: &Path, runtime: Arc<Counting>) -> Driver {
    let (demand, _) = request();
    let nodes = [NodeSpec::new(
        NodeId(0),
        Resources {
            cpu_milli: demand.need.cpu_milli * 2,
            mem_bytes: demand.need.mem_bytes * 2,
            ..Resources::default()
        },
    )];
    Driver::boot(
        NodeId(0),
        &nodes,
        &[],
        Budget { owned: 4, ran: 4 },
        Pick::default(),
        dir,
        Runtimes::new([(Class::Oci, runtime as Arc<dyn Runtime>)]).unwrap(),
    )
    .unwrap()
}

fn local(driver: &Driver, step: Step) -> Step {
    let mut send = std::collections::VecDeque::from(step.send);
    let mut notices = step.notices;
    for _ in 0..128 {
        let Some((to, envelope)) = send.pop_front() else {
            return Step {
                send: Vec::new(),
                notices,
            };
        };
        assert_eq!(to, NodeId(0));
        let next = driver.hear(NodeId(0), envelope).unwrap();
        send.extend(next.send);
        notices.extend(next.notices);
    }
    panic!("local peer did not quiesce")
}

/// AC-T7.4.1: an actual empty process box remains admitted until actual Runtime.delete.
#[test]
fn actual_create_and_delete_hold_empty_boxes_and_keep_caller_names_separate() {
    let dir = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    let driver = boot(dir.path(), rt.clone());
    let (d, p) = request();
    let (key, step) = driver.submit(d, p.clone()).unwrap();
    assert_eq!(rt.creates.load(Ordering::SeqCst), 0);
    assert!(
        !step
            .notices
            .iter()
            .any(|n| matches!(n, Notice::Started { .. }))
    );
    let done = local(&driver, step);
    assert!(done.notices.contains(&Notice::Started {
        key,
        node: NodeId(0)
    }));
    assert_eq!(rt.creates.load(Ordering::SeqCst), 1);
    let info = rt.inspect(&physical_id(key).unwrap()).unwrap();
    assert_eq!(info.state, BoxState::Empty);
    assert_ne!(info.id, p.spec.id);
    let status = driver.poll_status().unwrap();
    assert!(matches!(status.notices.as_slice(), [Notice::Status { .. }]));
    assert!(!driver.status(key).unwrap().unwrap().ended);
    let ended = local(&driver, driver.delete(key).unwrap());
    assert!(ended.notices.contains(&Notice::Ended { key }));
    assert!(rt.list().unwrap().is_empty());
    assert_eq!(rt.deletes.load(Ordering::SeqCst), 1);
    // Unknown keys must never delete an unrelated runtime box.
    assert!(matches!(
        driver.delete(key),
        Err(Error::Runtime(Refusal::NoSuchBox(_)))
    ));
    assert_eq!(rt.deletes.load(Ordering::SeqCst), 1);
}

/// AC-T7.4.1: actual OCI start occurs before Started is emitted, on the registered runtime.
#[test]
fn actual_oci_init_is_started_before_peer_acknowledgement() {
    let dir = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    let driver = boot(dir.path(), rt.clone());
    let (d, mut p) = request();
    p.spec.class = ClassSpec::Oci(Box::new(OciSpec {
        rootfs: Rootfs::Dir("/rootfs".into()),
        init: Program::of(&["wait"]),
        mounts: Vec::new(),
        hostname: None,
        network: Network::Private,
        hosts: Vec::new(),
    }));
    p.start = true;
    let (key, step) = driver.submit(d, p).unwrap();
    let done = local(&driver, step);
    assert!(done.notices.contains(&Notice::Started {
        key,
        node: NodeId(0)
    }));
    assert_eq!(rt.creates.load(Ordering::SeqCst), 1);
    assert_eq!(rt.starts.load(Ordering::SeqCst), 1);
    assert_eq!(
        rt.inspect(&physical_id(key).unwrap()).unwrap().state,
        BoxState::Running
    );
}

/// AC-T7.4.1, commit then publish: failing the ack write after create leaves one recoverable box.
#[test]
fn crash_after_create_before_ack_recovers_the_same_physical_box_once() {
    let dir = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    let driver = boot(dir.path(), rt.clone());
    let (d, p) = request();
    let (key, step) = driver.submit(d, p).unwrap();
    let tmp = dir.path().join(format!("{}.tmp", name("ran", key)));
    *rt.fail_record.lock().unwrap() = Some(tmp.clone());
    let (_, start) = step.send.into_iter().next().unwrap();
    assert!(matches!(
        driver.hear(NodeId(0), start),
        Err(Error::Core(Stop::Io(_)))
    ));
    assert_eq!(rt.creates.load(Ordering::SeqCst), 1);
    assert!(driver.retry().is_err());
    drop(driver);
    std::fs::remove_dir(tmp).unwrap();
    let recovered = boot(dir.path(), rt.clone());
    let step = recovered.take_recovered().unwrap();
    let step = local(&recovered, step);
    assert!(step.notices.contains(&Notice::Started {
        key,
        node: NodeId(0)
    }));
    assert_eq!(rt.creates.load(Ordering::SeqCst), 1);
    assert_eq!(rt.list().unwrap().len(), 1);
}

/// AC-T7.4.1: failed physical creation is acknowledged Refused only after cleanup succeeded.
#[test]
fn failed_create_is_cleaned_and_refused_without_started() {
    let dir = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    rt.fail_create.store(true, Ordering::SeqCst);
    let driver = boot(dir.path(), rt.clone());
    let (d, p) = request();
    let (key, step) = driver.submit(d, p).unwrap();
    let (_, start) = step.send.into_iter().next().unwrap();
    let step = driver.hear(NodeId(0), start).unwrap();
    assert!(
        matches!(step.send.as_slice(),[(_,Envelope{msg:Msg::Refused{key:k},refusal:Some(Refusal::Os{op:"test create",..}),..})] if *k==key)
    );
    assert!(
        !step
            .notices
            .iter()
            .any(|n| matches!(n, Notice::Started { .. }))
    );
    assert!(rt.list().unwrap().is_empty());
    assert_eq!(rt.deletes.load(Ordering::SeqCst), 1);
    assert!(!driver.status(key).unwrap().unwrap().local);
}

/// AC-T7.4.1: cleanup failure retains the live reservation and never permits reassignment.
#[test]
fn failed_cleanup_keeps_unknown_box_and_reservation_until_explicit_delete() {
    let dir = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    rt.fail_create.store(true, Ordering::SeqCst);
    rt.fail_delete.store(true, Ordering::SeqCst);
    let driver = boot(dir.path(), rt.clone());
    let (d, p) = request();
    let (key, step) = driver.submit(d, p).unwrap();
    let (_, start) = step.send.into_iter().next().unwrap();
    let step = driver.hear(NodeId(0), start).unwrap();
    assert!(step.send.is_empty());
    assert!(matches!(
        step.notices.as_slice(),
        [Notice::Unknown {
            cleanup: Some(_),
            ..
        }]
    ));
    assert!(driver.status(key).unwrap().unwrap().local);
    assert_eq!(rt.list().unwrap().len(), 1);
    rt.fail_delete.store(false, Ordering::SeqCst);
    local(&driver, driver.delete(key).unwrap());
    assert!(rt.list().unwrap().is_empty());
}

/// AC-T7.4.1: finite caps are covered, devices are refused rather than silently omitted.
#[test]
fn underreserved_or_unattachable_specs_are_refused_before_durable_submission() {
    let dir = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    let driver = boot(dir.path(), rt.clone());
    let (mut d, p) = request();
    d.need.mem_bytes -= 1;
    assert!(matches!(
        driver.submit(d, p),
        Err(Error::Runtime(Refusal::Invalid { .. }))
    ));
    let (mut d, p) = request();
    d.need.gpus = 1;
    assert!(driver.submit(d, p).is_err());
    let (d, mut p) = request();
    p.spec.bounds.cpu_max_milli = None;
    assert!(driver.submit(d, p).is_err());
    assert!(driver.owned().unwrap().is_empty());
    assert_eq!(rt.creates.load(Ordering::SeqCst), 0);
}

/// AC-T7.4.1: full peer ownership keys, never caller BoxIds, name physical boxes.
#[test]
fn owner_epoch_sequence_and_repeated_caller_ids_cannot_collide() {
    let keys = [
        Key {
            owner: 0,
            epoch: 1,
            seq: 0,
        },
        Key {
            owner: 1,
            epoch: 1,
            seq: 0,
        },
        Key {
            owner: 0,
            epoch: 2,
            seq: 0,
        },
        Key {
            owner: 0,
            epoch: 1,
            seq: 1,
        },
        Key {
            owner: u32::MAX,
            epoch: u64::MAX,
            seq: u64::MAX,
        },
    ];
    let ids: std::collections::BTreeSet<_> =
        keys.into_iter().map(|k| physical_id(k).unwrap()).collect();
    assert_eq!(ids.len(), keys.len());
    let dir = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    let driver = boot(dir.path(), rt.clone());
    let (d, p) = request();
    let (a, s) = driver.submit(d, p.clone()).unwrap();
    local(&driver, s);
    let (b, s) = driver.submit(d, p).unwrap();
    local(&driver, s);
    assert_ne!(physical_id(a).unwrap(), physical_id(b).unwrap());
    assert_eq!(rt.list().unwrap().len(), 2);
}

/// AC-T7.4.1: the owner needs no local backend for a class another peer genuinely runs.
#[test]
fn owner_without_guest_backend_sends_full_payload_to_capable_peer() {
    use hex_sim::model::{Arch, Classes, Os, Platform};
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    let (mut demand, mut payload) = request();
    demand.class = Class::Shards;
    demand.ask.os = Some(Os::Macos);
    payload.spec.class = ClassSpec::Oci(Box::new(OciSpec {
        rootfs: Rootfs::Dir("/rootfs".into()),
        init: Program::of(&["wait"]),
        mounts: Vec::new(),
        hostname: None,
        network: Network::Private,
        hosts: Vec::new(),
    }));
    payload.start = true;
    let mut owner = NodeSpec::new(NodeId(0), demand.need);
    owner.classes = Classes::NONE.with(Class::Oci);
    let mut target = NodeSpec::new(NodeId(1), demand.need);
    target.classes = Classes::NONE.with(Class::Shards);
    target.platform = Platform {
        os: Os::Macos,
        arch: Arch::Aarch64,
    };
    target.virt.hvf = true;
    let nodes = [owner, target];
    let budget = Budget { owned: 4, ran: 4 };
    let owner = Driver::boot(
        NodeId(0),
        &nodes,
        &[],
        budget,
        Pick::default(),
        a.path(),
        Runtimes::default(),
    )
    .unwrap();
    let target = Driver::boot(
        NodeId(1),
        &nodes,
        &[],
        budget,
        Pick::default(),
        b.path(),
        Runtimes::new([(Class::Shards, rt.clone() as Arc<dyn Runtime>)]).unwrap(),
    )
    .unwrap();
    let (key, step) = owner.submit(demand, payload.clone()).unwrap();
    let (to, start) = step.send.into_iter().next().unwrap();
    assert_eq!(to, NodeId(1));
    assert_eq!(start.payload, Some(payload));
    let step = target.hear(NodeId(0), start).unwrap();
    assert_eq!(rt.starts.load(Ordering::SeqCst), 1);
    let (to, started) = step.send.into_iter().next().unwrap();
    assert_eq!(to, NodeId(0));
    let step = owner.hear(NodeId(1), started).unwrap();
    assert!(step.notices.contains(&Notice::Started {
        key,
        node: NodeId(1)
    }));
    let deleting = owner.delete(key).unwrap();
    assert_eq!(rt.deletes.load(Ordering::SeqCst), 0);
    let (to, message) = deleting.send.into_iter().next().unwrap();
    assert_eq!(to, NodeId(1));
    let ended = target.hear(NodeId(0), message).unwrap();
    assert_eq!(rt.deletes.load(Ordering::SeqCst), 1);
    let (to, message) = ended.send.into_iter().next().unwrap();
    assert_eq!(to, NodeId(0));
    let ended = owner.hear(NodeId(1), message).unwrap();
    assert!(ended.notices.contains(&Notice::Ended { key }));
    assert!(rt.list().unwrap().is_empty());
}

/// AC-T7.4.1: a dropped Start requires the exact backend's absence proof before deletion ends.
#[test]
fn deleting_before_start_delivery_checks_runtime_before_end() {
    let dir = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    let driver = boot(dir.path(), rt.clone());
    let (d, p) = request();
    let (key, dropped) = driver.submit(d, p).unwrap();
    assert_eq!(dropped.send.len(), 1);
    let step = driver.delete(key).unwrap();
    assert_eq!(rt.deletes.load(Ordering::SeqCst), 0);
    let ended = local(&driver, step);
    assert!(ended.notices.contains(&Notice::Ended { key }));
    assert_eq!(rt.creates.load(Ordering::SeqCst), 0);
    assert_eq!(rt.deletes.load(Ordering::SeqCst), 1);
    assert!(driver.status(key).unwrap().is_none());
}

/// AC-T7.4.1: live capacity shrink refuses new admission without disabling physical cleanup.
#[test]
fn failed_atomic_fact_refresh_does_not_poison_delete_or_later_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    let driver = boot(dir.path(), rt.clone());
    let (d, p) = request();
    let (key, step) = driver.submit(d, p.clone()).unwrap();
    local(&driver, step);
    assert!(
        driver
            .refresh_node(NodeSpec::new(NodeId(0), Resources::default()))
            .is_err()
    );
    assert!(driver.submit(d, p.clone()).is_err());
    local(&driver, driver.delete(key).unwrap());
    assert!(rt.list().unwrap().is_empty());
    driver
        .refresh_node(NodeSpec::new(
            NodeId(0),
            Resources {
                cpu_milli: d.need.cpu_milli * 2,
                mem_bytes: d.need.mem_bytes * 2,
                ..Resources::default()
            },
        ))
        .unwrap();
    let (_, step) = driver.submit(d, p).unwrap();
    local(&driver, step);
    assert_eq!(rt.list().unwrap().len(), 1);
}

/// AC-T7.4.1: neither an owned request nor a receiver retry may silently replace its full spec.
#[test]
fn changed_start_payload_is_refused_before_first_create_and_on_receiver_retry() {
    let dir = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    let driver = boot(dir.path(), rt.clone());
    let (d, p) = request();
    let (_, step) = driver.submit(d, p).unwrap();
    let (_, original) = step.send.into_iter().next().unwrap();
    let mut changed = original.clone();
    changed.payload.as_mut().unwrap().spec.id = BoxId::new("changed-caller-id").unwrap();
    assert!(matches!(
        driver.hear(NodeId(0), changed.clone()),
        Err(Error::Runtime(Refusal::Invalid { .. }))
    ));
    assert_eq!(rt.creates.load(Ordering::SeqCst), 0);
    driver.hear(NodeId(0), original).unwrap();
    assert!(matches!(
        driver.hear(NodeId(0), changed),
        Err(Error::Runtime(Refusal::Invalid { .. }))
    ));
    assert_eq!(rt.creates.load(Ordering::SeqCst), 1);
}
