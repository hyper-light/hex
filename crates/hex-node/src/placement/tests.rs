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
    inspects: AtomicUsize,
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
        self.inspects.fetch_add(1, Ordering::SeqCst);
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

const TEST_NAMESPACE: &str = "00112233445566778899aabbccddeeff";

fn boot(dir: &Path, runtime: Arc<Counting>) -> Driver {
    try_boot(dir, runtime).unwrap()
}

fn try_boot(dir: &Path, runtime: Arc<Counting>) -> Result<Driver, Error> {
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
}

fn local(driver: &Driver, step: Step) -> Step {
    let mut send = std::collections::VecDeque::from(step.send);
    let mut notices = step.notices;
    for _ in 0..crate::records::MOST {
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
    let info = rt.inspect(&driver.physical_id(key).unwrap()).unwrap();
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
        rt.inspect(&driver.physical_id(key).unwrap()).unwrap().state,
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
    // The aggregate lands before the failed Running write; reboot must not observe it again.
    assert_eq!(
        recovered
            .start_observations()
            .unwrap()
            .as_slice()
            .iter()
            .map(|s| s.count)
            .sum::<u64>(),
        1
    );
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
    assert!(driver.start_observations().unwrap().is_empty());
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
    let ids: std::collections::BTreeSet<_> = keys
        .into_iter()
        .map(|k| physical_id(TEST_NAMESPACE, k).unwrap())
        .collect();
    assert_eq!(ids.len(), keys.len());
    let dir = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    let driver = boot(dir.path(), rt.clone());
    let (d, p) = request();
    let (a, s) = driver.submit(d, p.clone()).unwrap();
    local(&driver, s);
    let (b, s) = driver.submit(d, p).unwrap();
    local(&driver, s);
    assert_ne!(
        driver.physical_id(a).unwrap(),
        driver.physical_id(b).unwrap()
    );
    assert_eq!(rt.list().unwrap().len(), 2);
}

/// AC-T7.4.1: a locally blocked owner needs no backend for a class another peer genuinely runs.
#[test]
fn blocked_owner_without_guest_backend_sends_full_payload_to_capable_peer() {
    use hex_sim::model::{Arch, Classes, Os, Platform};
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    let (mut demand, mut payload) = request();
    demand.class = Class::Shards;
    // The image is Linux; Shards runs it on the macOS host's Hypervisor.framework.
    demand.ask.os = Some(Os::Linux);
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
    let owner = Driver::boot_in(
        Identity {
            namespace: TEST_NAMESPACE,
            node: NodeId(0),
        },
        &nodes,
        &[],
        budget,
        Pick::default(),
        a.path(),
        Runtimes::default(),
    )
    .unwrap();
    let target = Driver::boot_in(
        Identity {
            namespace: TEST_NAMESPACE,
            node: NodeId(1),
        },
        &nodes,
        &[],
        budget,
        Pick::default(),
        b.path(),
        Runtimes::new([(Class::Shards, rt.clone() as Arc<dyn Runtime>)]).unwrap(),
    )
    .unwrap();
    owner
        .block_admission(Refusal::Exhausted {
            what: "test local room",
            bound: 0,
        })
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
    assert_eq!(
        owner.status(key).unwrap().unwrap().physical,
        target.physical_id(key).unwrap()
    );
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
    let (waiting, step) = driver.submit(d, p.clone()).unwrap();
    let rejected = local(&driver, step);
    assert!(
        rejected
            .notices
            .iter()
            .any(|n| matches!(n,Notice::Refused{key,..} if *key==waiting))
    );
    assert_eq!(rt.list().unwrap().len(), 1);
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

/// AC-T7.4.1: real combined launch is observed once; replay and recovery retain the same aggregate.
#[test]
fn combined_launch_sample_is_persisted_once_across_duplicate_start_and_reboot() {
    let dir = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    let driver = boot(dir.path(), rt.clone());
    let (d, p) = request();
    let (_, step) = driver.submit(d, p).unwrap();
    let (_, start) = step.send.into_iter().next().unwrap();
    driver.hear(NodeId(0), start.clone()).unwrap();
    let before = driver.start_observations().unwrap();
    assert_eq!(before.len(), 1);
    let measured = before.first().unwrap();
    assert_eq!(measured.node, NodeId(0));
    assert_eq!(measured.class, Class::Oci);
    assert_eq!(measured.count, 1);
    driver.hear(NodeId(0), start).unwrap();
    assert_eq!(driver.start_observations().unwrap(), before);
    drop(driver);
    let recovered = boot(dir.path(), rt.clone());
    assert_eq!(recovered.start_observations().unwrap(), before);
    assert_eq!(rt.creates.load(Ordering::SeqCst), 1);
}

/// AC-T7.4.1: unavailable admission refuses an unheld Start but cannot hide an existing box.
#[test]
fn typed_probe_refusal_releases_only_unheld_start_and_keeps_control_available() {
    let dir = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    let driver = boot(dir.path(), rt.clone());
    let (d, p) = request();
    let (key, step) = driver.submit(d, p).unwrap();
    let (_, start) = step.send.into_iter().next().unwrap();
    let why = Refusal::Exhausted {
        what: "test probe",
        bound: 0,
    };
    driver.block_admission(why.clone()).unwrap();
    let refused = driver.hear(NodeId(0), start).unwrap();
    assert!(
        matches!(refused.send.as_slice(),[(_,Envelope{msg:Msg::Refused{key:k},refusal:Some(r),..})] if *k==key&&*r==why)
    );
    assert_eq!(rt.creates.load(Ordering::SeqCst), 0);
    local(&driver, driver.delete(key).unwrap());
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
    let (d, p) = request();
    let (_, step) = driver.submit(d, p).unwrap();
    let (_, start) = step.send.into_iter().next().unwrap();
    driver.hear(NodeId(0), start.clone()).unwrap();
    driver.block_admission(why).unwrap();
    let held = driver.hear(NodeId(0), start).unwrap();
    assert!(matches!(
        held.send.as_slice(),
        [(
            _,
            Envelope {
                msg: Msg::Started { .. },
                ..
            }
        )]
    ));
    assert_eq!(rt.creates.load(Ordering::SeqCst), 1);
}

/// AC-T7.4.1: a rejected remote shrink neither blocks nor clears local receiver admission.
#[test]
fn remote_fact_failure_does_not_block_local_start() {
    use hex_sim::model::Os;
    let dir = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    let (d, p) = request();
    let local_node = NodeSpec::new(NodeId(0), d.need);
    let mut remote = NodeSpec::new(NodeId(1), d.need);
    remote.platform.os = Os::Windows;
    let driver = Driver::boot(
        NodeId(0),
        &[local_node, remote],
        &[],
        Budget { owned: 4, ran: 4 },
        Pick::default(),
        dir.path(),
        Runtimes::new([(Class::Oci, rt.clone() as Arc<dyn Runtime>)]).unwrap(),
    )
    .unwrap();
    let mut remote_demand = d;
    remote_demand.ask.os = Some(Os::Windows);
    let (_, remote_start) = driver.submit(remote_demand, p.clone()).unwrap();
    assert_eq!(remote_start.send.first().unwrap().0, NodeId(1));
    let mut shrink = NodeSpec::new(NodeId(1), Resources::default());
    shrink.platform.os = Os::Windows;
    assert!(driver.refresh_node(shrink).is_err());
    let mut local_demand = d;
    local_demand.ask.os = Some(Os::Linux);
    let (key, local_start) = driver.submit(local_demand, p).unwrap();
    let step = local(&driver, local_start);
    assert!(step.notices.contains(&Notice::Started {
        key,
        node: NodeId(0)
    }));
    assert_eq!(rt.creates.load(Ordering::SeqCst), 1);
}

/// AC-T7.4.1/LB03: independent state directories sharing one runtime never adopt/delete each other's
/// equal owner/epoch/sequence keys, and recovery retains exactly the original physical identity.
#[test]
fn independent_state_namespaces_isolate_shared_runtime_boxes_and_survive_restart() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    let first = boot(a.path(), rt.clone());
    let second = boot(b.path(), rt.clone());
    let (d, p) = request();
    let (key_a, step) = first.submit(d, p.clone()).unwrap();
    local(&first, step);
    let (key_b, step) = second.submit(d, p).unwrap();
    local(&second, step);
    assert_eq!(key_a, key_b);
    assert_ne!(first.namespace(), second.namespace());
    let physical_a = first.physical_id(key_a).unwrap();
    let physical_b = second.physical_id(key_b).unwrap();
    assert_ne!(physical_a, physical_b);
    assert_eq!(rt.creates.load(Ordering::SeqCst), 2);
    assert_eq!(rt.list().unwrap().len(), 2);
    let namespace = second.namespace().to_owned();
    local(&first, first.delete(key_a).unwrap());
    assert!(matches!(
        rt.inspect(&physical_a),
        Err(Refusal::NoSuchBox(_))
    ));
    assert_eq!(rt.inspect(&physical_b).unwrap().id, physical_b);
    assert_eq!(rt.list().unwrap().len(), 1);
    drop(second);
    let recovered = boot(b.path(), rt.clone());
    assert_eq!(recovered.namespace(), namespace);
    assert_eq!(
        recovered.status(key_b).unwrap().unwrap().physical,
        physical_b
    );
    assert_eq!(rt.creates.load(Ordering::SeqCst), 2);
    local(&recovered, recovered.delete(key_b).unwrap());
    assert!(rt.list().unwrap().is_empty());
}

/// AC-T7.4.1/LB03: a damaged or unnamespaced existing epoch is refused before any runtime inspect;
/// it must never receive a replacement namespace that strands or adopts physical boxes.
#[test]
fn invalid_or_missing_durable_namespace_is_not_regenerated_before_inspect() {
    let dir = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    let driver = boot(dir.path(), rt.clone());
    let (d, p) = request();
    let (_, step) = driver.submit(d, p).unwrap();
    local(&driver, step);
    drop(driver);
    let path = dir.path().join("epoch.rec");
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let before = rt.inspects.load(Ordering::SeqCst);
    for namespace in [
        None,
        Some(serde_json::json!("ABCDEF0123456789ABCDEF0123456789")),
        Some(serde_json::json!("z".repeat(32))),
        Some(serde_json::json!(false)),
    ] {
        let mut corrupt = original.clone();
        match namespace {
            Some(value) => {
                corrupt["namespace"] = value;
            }
            None => {
                corrupt.as_object_mut().unwrap().remove("namespace");
            }
        }
        std::fs::write(&path, serde_json::to_vec(&corrupt).unwrap()).unwrap();
        assert!(matches!(
            try_boot(dir.path(), rt.clone()),
            Err(Error::Core(Stop::Damaged(_)))
        ));
        assert_eq!(rt.inspects.load(Ordering::SeqCst), before);
        let retained: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(retained, corrupt);
    }
    std::fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(
        try_boot(dir.path(), rt.clone()),
        Err(Error::Core(Stop::Damaged(_)))
    ));
    assert_eq!(rt.inspects.load(Ordering::SeqCst), before);
}

/// AC-T7.4.1/LB03: peers explicitly share identity; changing an existing state's realm is refused.
#[test]
fn explicit_shared_namespace_matches_remote_ids_and_cannot_rebind_existing_state() {
    let dir = tempfile::tempdir().unwrap();
    let rt = Arc::new(Counting::default());
    let (d, p) = request();
    let nodes = [NodeSpec::new(NodeId(0), d.need)];
    let budget = Budget { owned: 4, ran: 4 };
    let runtime = || Runtimes::new([(Class::Oci, rt.clone() as Arc<dyn Runtime>)]).unwrap();
    let driver = Driver::boot_in(
        Identity {
            namespace: TEST_NAMESPACE,
            node: NodeId(0),
        },
        &nodes,
        &[],
        budget,
        Pick::default(),
        dir.path(),
        runtime(),
    )
    .unwrap();
    assert_eq!(driver.namespace(), TEST_NAMESPACE);
    let (key, step) = driver.submit(d, p).unwrap();
    local(&driver, step);
    drop(driver);
    let before = rt.inspects.load(Ordering::SeqCst);
    let different = "ffeeddccbbaa99887766554433221100";
    assert!(matches!(
        Driver::boot_in(
            Identity {
                namespace: different,
                node: NodeId(0)
            },
            &nodes,
            &[],
            budget,
            Pick::default(),
            dir.path(),
            runtime()
        ),
        Err(Error::Core(Stop::Damaged(_)))
    ));
    assert_eq!(rt.inspects.load(Ordering::SeqCst), before);
    let driver = Driver::boot_in(
        Identity {
            namespace: TEST_NAMESPACE,
            node: NodeId(0),
        },
        &nodes,
        &[],
        budget,
        Pick::default(),
        dir.path(),
        runtime(),
    )
    .unwrap();
    assert_eq!(driver.namespace(), TEST_NAMESPACE);
    assert_eq!(
        driver.status(key).unwrap().unwrap().physical,
        physical_id(TEST_NAMESPACE, key).unwrap()
    );
    let longest = physical_id(
        &"f".repeat(64),
        Key {
            owner: u32::MAX,
            epoch: u64::MAX,
            seq: u64::MAX,
        },
    )
    .unwrap();
    assert!(longest.as_str().len() <= BoxId::MAX_LEN);
    assert_eq!(longest.as_str().len(), 127);
}
