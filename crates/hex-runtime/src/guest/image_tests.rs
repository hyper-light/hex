//! The Shards class on Agentfile images over a model of Shards' image boot (D231's `Images`, which has
//! no simulator of its own yet) and a record owner holding the node Store's lifecycle rules: each test
//! does a call and expects the record, the run and the refusal the design names
//! (docs/design/shards-image-class.md; joint plan step 12, AC-T7.4.1).

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::devices::{Books, Records, Transaction};
use hex_api::{Allocation, Bounds, Capacity, CapacityResources, DeviceInfo, Runtime};
use shards_api::image::{Bound, Channel, MemberEnd, MemberInfo, MemberName};

/// A record owner keeping the node Store's rules (hex-node's `Store::update_lifecycle`): a first record
/// is Creating; Creating becomes anything, Ready Ready or Deleting, Deleting Deleting alone; an exit, once
/// written, stays; only a Deleting record is removed.
#[derive(Default)]
struct Owner {
    records: Mutex<Records>,
}

fn broken(why: &str) -> Refusal {
    Refusal::Broken(why.to_owned())
}

impl Transaction for Owner {
    fn records(&self) -> Result<Records, Refusal> {
        Ok(self.records.lock().unwrap().clone())
    }
    fn pool_records(&self) -> Result<crate::devices::PoolRecords, Refusal> {
        Ok(Vec::new())
    }
    fn read_pool(
        &self,
        _: &hex_api::BackingKey,
    ) -> Result<Option<hex_core::record::PoolRecord>, Refusal> {
        Err(broken("an image box reads no pool"))
    }
    fn put_pool(&self, _: &hex_core::record::PoolRecord) -> Result<(), Refusal> {
        Err(broken("an image box writes no pool"))
    }
    fn grant_pool(
        &self,
        _: &hex_api::BackingGrant,
    ) -> Result<hex_core::record::PoolRecord, Refusal> {
        Err(broken("an image box grants no pool"))
    }
    fn remove_pool(&self, _: &hex_api::BackingKey) -> Result<(), Refusal> {
        Err(broken("an image box removes no pool"))
    }
    fn check_unsubmitted_pool(&self) -> Result<(), Refusal> {
        Err(broken("an image box owns no pool scope"))
    }
    fn read(&self, id: &BoxId) -> Result<Option<BoxRecord>, Refusal> {
        Ok(self
            .records
            .lock()
            .unwrap()
            .get(id)
            .map(|row| row.as_ref().unwrap().clone()))
    }
    fn put(&self, record: &BoxRecord) -> Result<(), Refusal> {
        let mut records = self.records.lock().unwrap();
        let allowed = match records.get(&record.spec.id) {
            None => record.phase == Phase::Creating,
            Some(current) => {
                let current = current.as_ref().unwrap();
                let phase = matches!(
                    (current.phase, record.phase),
                    (Phase::Creating, _)
                        | (Phase::Ready, Phase::Ready | Phase::Deleting)
                        | (Phase::Deleting, Phase::Deleting)
                );
                let exit = current.exit.is_none() || current.exit == record.exit;
                phase && exit && current.spec == record.spec
            }
        };
        if !allowed {
            return Err(broken("a write the Store refuses"));
        }
        records.insert(record.spec.id.clone(), Ok(record.clone()));
        Ok(())
    }
    fn remove(&self, id: &BoxId) -> Result<(), Refusal> {
        let mut records = self.records.lock().unwrap();
        match records.get(id) {
            Some(Ok(record)) if record.phase == Phase::Deleting => {
                records.remove(id);
                Ok(())
            }
            _ => Err(broken("a removal of a record that isn't Deleting")),
        }
    }
    fn launch_managed(
        &self,
        _: &crate::devices::ManagedLaunch<'_>,
        _: shards_api::LaunchIo,
        _: Duration,
    ) -> Result<shards_api::ManagedReceipt, Refusal> {
        Err(broken("an image box launches through Shards' image boot"))
    }
    fn attach_managed(
        &self,
        _: Option<&shards_api::Identity>,
    ) -> Result<shards_api::ManagedReceipt, Refusal> {
        Err(broken("an image box attaches nothing"))
    }
    fn finalize_managed(&self, _: Option<&shards_api::Identity>) -> Result<(), Refusal> {
        Err(broken("an image box finalizes no native scope"))
    }
    fn managed_usage(&self) -> Result<Usage, Refusal> {
        Err(broken("an image box's usage is its run's stats"))
    }
}

impl Books for Owner {
    fn records(&self) -> Result<Records, Refusal> {
        Transaction::records(self)
    }
    fn pool_records(&self) -> Result<crate::devices::PoolRecords, Refusal> {
        Ok(Vec::new())
    }
    fn prepare_pool(
        &self,
        _: [u8; 32],
        _: hex_api::Execution,
        _: hex_api::RuntimeCost,
        _: &[hex_api::DeviceId],
        _: (
            &[u8; 32],
            &[hex_api::DeviceId],
            Option<&hex_core::record::PoolRecord>,
        ),
        _: &mut crate::devices::PoolPreparationCheck<'_>,
    ) -> Result<hex_core::record::PoolRecord, Refusal> {
        Err(broken("an image box prepares no pool"))
    }
    fn capacity(&self) -> Result<Capacity, Refusal> {
        Err(broken("the image tests read no capacity"))
    }
    fn physical_devices(&self) -> Result<Vec<DeviceInfo>, Refusal> {
        Ok(Vec::new())
    }
    fn with_transaction(
        &self,
        _: &BoxId,
        _: &[hex_api::DeviceId],
        callback: &mut dyn FnMut(&dyn Transaction) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        callback(self)
    }
    fn with_pool_transaction(
        &self,
        _: &hex_api::BackingKey,
        _: &[hex_api::DeviceId],
        _: &mut dyn FnMut(&dyn Transaction) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        Err(broken("an image box enters no pool"))
    }
    fn with_handoff_transaction(
        &self,
        _: &BoxId,
        _: &hex_api::BackingKey,
        _: &[hex_api::DeviceId],
        _: &mut dyn FnMut(&dyn Transaction) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        Err(broken("an image box hands nothing off"))
    }
}

/// A run in the model: its end, which the test or its stop sets, its stops, and its stats.
#[derive(Default)]
struct ModelRun {
    end: Mutex<Option<RunEnd>>,
    stops: AtomicUsize,
    stats: Mutex<Stats>,
}

struct ModelGuest(Arc<ModelRun>);

impl ImageGuest for ModelGuest {
    fn members(&self) -> &[MemberInfo] {
        &[]
    }
    fn published(&self) -> &[Bound] {
        &[]
    }
    fn take_output(&self, _: &MemberName, _: Channel) -> Result<std::io::PipeReader, ImageRefusal> {
        Err(ImageRefusal::Unsupported {
            what: "the model's output",
            why: "it has none".to_owned(),
        })
    }
    fn take_input(&self, _: &MemberName) -> Result<std::io::PipeWriter, ImageRefusal> {
        Err(ImageRefusal::Unsupported {
            what: "the model's input",
            why: "it has none".to_owned(),
        })
    }
    fn wait_member(&self, _: &MemberName, _: Duration) -> Result<Option<MemberEnd>, ImageRefusal> {
        Ok(None)
    }
    fn stats(&self, _: StatsScope, _: Duration) -> Result<Stats, ImageRefusal> {
        match self.0.end.lock().unwrap().clone() {
            Some(end) => Err(ImageRefusal::Ended(end)),
            None => Ok(*self.0.stats.lock().unwrap()),
        }
    }
    fn stop(&self) -> Result<(), ImageRefusal> {
        self.0.stops.fetch_add(1, Ordering::SeqCst);
        self.0.end.lock().unwrap().get_or_insert(RunEnd::Stopped);
        Ok(())
    }
    fn wait_for(&self, _: Duration) -> Result<Option<RunEnd>, ImageRefusal> {
        Ok(self.0.end.lock().unwrap().clone())
    }
}

/// Shards' image boot in the model: what each boot was asked, a refusal for the next where the test
/// sets one, and a gate a boot waits at, open unless the test closes it.
struct ModelImages {
    asked: Mutex<Vec<ImageSpec>>,
    refuse: Mutex<Option<ImageRefusal>>,
    open: Mutex<bool>,
    opened: Condvar,
    run: Arc<ModelRun>,
}

impl ModelImages {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            asked: Mutex::new(Vec::new()),
            refuse: Mutex::new(None),
            open: Mutex::new(true),
            opened: Condvar::new(),
            run: Arc::new(ModelRun::default()),
        })
    }

    fn gate(&self, open: bool) {
        *self.open.lock().unwrap() = open;
        self.opened.notify_all();
    }
}

impl Images for ModelImages {
    fn backend(&self) -> Result<shards_api::Backend, ImageRefusal> {
        Ok(shards_api::Backend::Kvm)
    }
    fn boot(&self, spec: &ImageSpec) -> Result<Box<dyn ImageGuest>, ImageRefusal> {
        self.asked.lock().unwrap().push(spec.clone());
        let open = self.open.lock().unwrap();
        drop(self.opened.wait_while(open, |open| !*open).unwrap());
        if let Some(why) = self.refuse.lock().unwrap().take() {
            return Err(why);
        }
        Ok(Box::new(ModelGuest(Arc::clone(&self.run))))
    }
}

/// The runtime's waits for the model's answers, its boot's, a stats ask's and a stop's: facts no
/// recorded operation makes, so each measures, for its fact alone (hex_testkit::waits; the lead's rule
/// of 2026-11-08), never the program-end record. The model answers each at once while its gate is open.
fn waits() -> Result<ImageWaits, Refusal> {
    Ok(ImageWaits {
        boot: Patience::measuring("test.hex_runtime.image_boot"),
        stats: Patience::measuring("test.hex_runtime.image_stats"),
        stop: Patience::measuring("test.hex_runtime.image_stop"),
    })
}

/// A guest runtime with the model's image boot and record owner (the owner kept alive by the caller).
fn model_runtime(
    images: &Arc<ModelImages>,
    waits: Result<ImageWaits, Refusal>,
) -> (GuestRuntime, Arc<Owner>) {
    let owner = Arc::new(Owner::default());
    let runtime = GuestRuntime::new(
        Arc::new(shards_api::sim::SimGuests::new(|_| {
            shards_api::End::PowerOff
        })),
        super::super::tests::config(),
    )
    .with_images(Ok(Arc::clone(images) as Arc<dyn Images>), waits);
    let books: Arc<dyn Books> = owner.clone();
    runtime.bind_books(Arc::downgrade(&books)).unwrap();
    (runtime, owner)
}

fn allocation() -> Allocation {
    hex_api::sim::sim_vm_allocation().unwrap()
}

fn spec(name: &str, command: Option<Vec<String>>) -> BoxSpec {
    BoxSpec {
        id: BoxId::new(name).unwrap(),
        bounds: Bounds::default(),
        class: ClassSpec::ShardsImage(Box::new(ShardsImage {
            command,
            ..ShardsImage::new(hex_api::sim::SIM_IMAGE)
        })),
        owner: None,
        devices: Vec::new(),
        allocation: Some(allocation()),
        route: None,
    }
}

fn record(owner: &Owner, id: &BoxId) -> Option<BoxRecord> {
    Transaction::read(owner, id).unwrap()
}

/// The tests' waits for a box's state, each its own fact's: a held boot answering once the model's gate
/// opens, shown Running; and a run's own end reaped through its stop, shown Empty. No recorded operation
/// makes either, so each measures, for its fact alone (hex_testkit::waits).
static LATE_BOOT_SHOWN: Patience = Patience::measuring("test.hex_runtime.image_late_boot_shown");
static REAPED_SHOWN: Patience = Patience::measuring("test.hex_runtime.image_reaped_shown");

/// Waits on `wait`, its fact's own patience, for box `id` to show `state`.
fn shown(runtime: &GuestRuntime, id: &BoxId, state: BoxState, wait: &Patience) -> BoxInfo {
    let mut last = None;
    hex_testkit::waits::until(wait, &format!("{id} shown {state:?}"), || {
        let info = runtime.inspect(id).unwrap();
        let reached = info.state == state;
        last = Some(info);
        reached
    })
    .unwrap();
    last.unwrap()
}

/// Joint plan step 12: do create, then start, an image VM of two vCPUs and 256 MiB with no command.
/// Expect: create records it Creating and shows it Created, booting nothing; start writes Ready first
/// and boots once, asking Shards for exactly its pinned image, its admitted size, no command, and none of
/// its image's grants (default deny until goal 15 programs them); then it shows Running.
#[test]
fn a_created_vm_boots_at_start_its_admitted_size_and_no_grant() {
    let images = ModelImages::new();
    let (runtime, owner) = model_runtime(&images, waits());
    let made = runtime.create(spec("image-boot", None)).unwrap();
    let id = made.id.clone();
    assert_eq!(
        (made.class, made.state, made.exit),
        (hex_api::Class::Shards, BoxState::Created, None)
    );
    assert_eq!(record(&owner, &id).unwrap().phase, Phase::Creating);
    assert!(images.asked.lock().unwrap().is_empty());
    runtime.start(&id).unwrap();
    assert_eq!(record(&owner, &id).unwrap().phase, Phase::Ready);
    let asked = images.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 1);
    let boot = &asked[0];
    assert_eq!(
        (
            boot.image.as_str(),
            boot.vcpus,
            boot.memory_mib,
            &boot.command
        ),
        (hex_api::sim::SIM_IMAGE, 2, 256, &None)
    );
    assert_eq!(boot.egress.egress, Keep::Only(Vec::new()));
    assert_eq!(boot.egress.mcp, Keep::Only(Vec::new()));
    assert!(!boot.egress.dns && boot.publish.is_empty() && boot.members.is_empty());
    assert_eq!(
        boot.binary.as_deref(),
        Some(super::super::tests::config().binary.as_path())
    );
    assert_eq!(runtime.inspect(&id).unwrap().state, BoxState::Running);
    assert!(matches!(
        runtime.start(&id),
        Err(Refusal::Invalid { what: "start", .. })
    ));
    assert!(runtime.list().unwrap().iter().any(|info| info.id == id));
}

/// Joint plan step 12, Hex's stop: do signal KILL to a running VM. Expect: its run stopped once, its
/// end recorded as Signal(9) and shown Empty; a second signal NotRunning; delete then removes its record.
#[test]
fn kill_stops_the_run_and_records_signal_9() {
    let images = ModelImages::new();
    let (runtime, owner) = model_runtime(&images, waits());
    let id = runtime.create(spec("image-stop", None)).unwrap().id;
    runtime.start(&id).unwrap();
    runtime.signal(&id, Signal::KILL).unwrap();
    assert_eq!(images.run.stops.load(Ordering::SeqCst), 1);
    assert_eq!(record(&owner, &id).unwrap().exit, Some(Exit::Signal(9)));
    let info = runtime.inspect(&id).unwrap();
    assert_eq!(
        (info.state, info.exit),
        (BoxState::Empty, Some(Exit::Signal(9)))
    );
    assert!(matches!(
        runtime.signal(&id, Signal::KILL),
        Err(Refusal::NotRunning(_, "signal"))
    ));
    runtime.delete(&id).unwrap();
    assert!(record(&owner, &id).is_none());
    assert!(matches!(runtime.inspect(&id), Err(Refusal::NoSuchBox(_))));
}

/// Joint plan step 12: do end a running VM's run on its own, its command exiting 3. Expect: its run
/// host reaped through its stop, its end recorded as Code(3) and shown Empty.
#[test]
fn a_run_that_ends_on_its_own_is_reaped_and_its_status_recorded() {
    let images = ModelImages::new();
    let (runtime, owner) = model_runtime(&images, waits());
    let id = runtime
        .create(spec("image-exit", Some(vec!["exit".into(), "3".into()])))
        .unwrap()
        .id;
    runtime.start(&id).unwrap();
    *images.run.end.lock().unwrap() = Some(RunEnd::Exited(3));
    let info = shown(&runtime, &id, BoxState::Empty, &REAPED_SHOWN);
    assert_eq!(info.exit, Some(Exit::Code(3)));
    assert_eq!(record(&owner, &id).unwrap().exit, Some(Exit::Code(3)));
    assert_eq!(images.run.stops.load(Ordering::SeqCst), 1);
}

/// Joint plan step 12, steps 11's boundary: do exec a member's program and send TERM to a running VM.
/// Expect: Awaits(MemberExec) and Awaits(MemberSignal), no stop, and the VM still Running.
#[test]
fn member_exec_and_signals_await_the_agent_control_endpoint() {
    let images = ModelImages::new();
    let (runtime, _owner) = model_runtime(&images, waits());
    let id = runtime.create(spec("image-members", None)).unwrap().id;
    runtime.start(&id).unwrap();
    assert!(matches!(
        runtime.exec(&id, hex_api::Program::of(&["true"]), hex_api::Stdio::null()),
        Err(Refusal::Awaits {
            needs: Upstream::MemberExec
        })
    ));
    assert_eq!(
        runtime.signal(&id, Signal::TERM),
        Err(Refusal::Awaits {
            needs: Upstream::MemberSignal
        })
    );
    assert_eq!(images.run.stops.load(Ordering::SeqCst), 0);
    assert_eq!(runtime.inspect(&id).unwrap().state, BoxState::Running);
}

/// Joint plan steps 12 and 14: do read a running VM's usage, then one whose stats lack a line. Expect:
/// its run's CPU in microseconds, memory, processes and I/O, guest time per vCPU Awaits
/// (GuestCpuTimePerVcpu), never a number; a missing line refused naming it; before its start
/// NotRunning.
#[test]
fn usage_reads_the_runs_stats_and_guest_time_awaits_shards() {
    let images = ModelImages::new();
    *images.run.stats.lock().unwrap() = Stats {
        cpu_ns: Some(7_000_999),
        memory_bytes: Some(64 << 20),
        pids: Some(5),
        read_bytes: Some(11),
        write_bytes: Some(13),
        ..Stats::default()
    };
    let (runtime, _owner) = model_runtime(&images, waits());
    let id = runtime.create(spec("image-usage", None)).unwrap().id;
    assert!(matches!(
        runtime.usage(&id),
        Err(Refusal::NotRunning(_, "usage"))
    ));
    runtime.start(&id).unwrap();
    let usage = runtime.usage(&id).unwrap();
    assert_eq!(
        (
            usage.cpu_usec,
            usage.memory_bytes,
            usage.pids,
            usage.io_read_bytes,
            usage.io_write_bytes
        ),
        (7_000, 64 << 20, 5, 11, 13)
    );
    assert_eq!(
        usage.guest,
        Some(GuestTime::Awaits(Upstream::GuestCpuTimePerVcpu))
    );
    images.run.stats.lock().unwrap().pids = None;
    assert!(matches!(
        runtime.usage(&id),
        Err(Refusal::Os { why, .. }) if why.contains("pids")
    ));
}

/// Joint plan step 12: do start a VM whose image Shards refuses. Expect: the refusal typed (Invalid,
/// naming the image), the box ended with an unknown exit and shown Empty, its record keeping both.
#[test]
fn a_refused_boot_ends_the_box_and_says_why() {
    let images = ModelImages::new();
    *images.refuse.lock().unwrap() = Some(ImageRefusal::Image("no such image".to_owned()));
    let (runtime, owner) = model_runtime(&images, waits());
    let id = runtime.create(spec("image-refused", None)).unwrap().id;
    assert!(matches!(
        runtime.start(&id),
        Err(Refusal::Invalid { what: "shards image", why }) if why.contains("no such image")
    ));
    let kept = record(&owner, &id).unwrap();
    assert_eq!((kept.phase, kept.exit), (Phase::Ready, Some(Exit::Unknown)));
    let info = runtime.inspect(&id).unwrap();
    assert_eq!(
        (info.state, info.exit),
        (BoxState::Empty, Some(Exit::Unknown))
    );
    runtime.delete(&id).unwrap();
}

/// Decision 9, joint plan step 12: do start a VM whose boot passes its learned patience. Expect: start
/// refused Lifecycle Pending, the box Created while its boot runs, delete Pending; then, the boot
/// answering, the box Running with no second boot.
#[test]
fn a_boot_past_its_patience_is_pending_and_its_answer_lands_after() {
    let images = ModelImages::new();
    images.gate(false);
    // The model holds the boot at its closed gate, which no bound outlasts, so a 1 ns record runs the
    // start's wait out at once. The stats and stop waits measure, as waits() says.
    let held = ImageWaits {
        boot: Patience::seeded("test.hex_runtime.image_boot_held", Duration::from_nanos(1)),
        ..waits().unwrap()
    };
    let (runtime, _owner) = model_runtime(&images, Ok(held));
    let id = runtime.create(spec("image-late", None)).unwrap().id;
    assert!(matches!(
        runtime.start(&id),
        Err(Refusal::Lifecycle {
            cause: LifecycleCause::Pending,
            ..
        })
    ));
    assert_eq!(runtime.inspect(&id).unwrap().state, BoxState::Created);
    assert!(matches!(
        runtime.delete(&id),
        Err(Refusal::Lifecycle {
            cause: LifecycleCause::Pending,
            ..
        })
    ));
    images.gate(true);
    shown(&runtime, &id, BoxState::Running, &LATE_BOOT_SHOWN);
    assert_eq!(images.asked.lock().unwrap().len(), 1);
}

/// Joint plan step 12, the owner's crash (ROOT, 2026-11-12): do give the runtime a Ready record with no
/// exit and no run of this process, as a node restart leaves. Expect: every call and recovery's
/// adoption refused Awaits(AttachAfterCrash), the record and its claim kept; then, the host's boot
/// session bound as another than the record's, the box ended (Exit::Unknown) and deletable.
#[test]
fn an_earlier_node_processes_run_is_held_until_a_reboot_proves_it_ended() {
    let images = ModelImages::new();
    let (runtime, owner) = model_runtime(&images, waits());
    let spec = spec("image-held", None);
    let id = spec.id.clone();
    let mut left = BoxRecord::new(spec.clone(), Phase::Creating, Vec::new());
    Transaction::put(owner.as_ref(), &left).unwrap();
    left.phase = Phase::Ready;
    left.boot = Some(BootSession::new("1111-before").unwrap());
    Transaction::put(owner.as_ref(), &left).unwrap();
    let held = Err(Refusal::Awaits {
        needs: Upstream::AttachAfterCrash,
    });
    assert_eq!(runtime.inspect(&id).map(|_| ()), held);
    assert_eq!(runtime.start(&id), held);
    assert_eq!(runtime.signal(&id, Signal::KILL), held);
    assert_eq!(runtime.delete(&id), held);
    assert_eq!(runtime.validate_binding(&spec), held);
    assert_eq!(runtime.recover_managed(&spec), held);
    assert!(record(&owner, &id).is_some());
    let listed = runtime.list().unwrap();
    assert!(
        listed
            .iter()
            .any(|info| info.id == id && info.state == BoxState::Running)
    );
    runtime
        .bind_boot_session(Some(BootSession::new("2222-after").unwrap()))
        .unwrap();
    runtime.validate_binding(&spec).unwrap();
    let info = runtime.inspect(&id).unwrap();
    assert_eq!(
        (info.state, info.exit),
        (BoxState::Empty, Some(Exit::Unknown))
    );
    runtime.delete(&id).unwrap();
    assert!(record(&owner, &id).is_none());
    assert!(images.asked.lock().unwrap().is_empty());
}

/// Decision 9, joint plan step 12: do create with no recorded waits, and with a VM size Shards can't
/// boot (255 MiB, odd). Expect: each refused before any record, the first with the waits' own refusal,
/// the second naming the allocation.
#[test]
fn a_create_is_refused_before_any_record_without_its_waits_or_a_bootable_size() {
    let images = ModelImages::new();
    let unrecorded = Refusal::Platform {
        what: "the oci class's guest",
        platform: "test",
        why: "no image_boot record for any host class yet".to_owned(),
    };
    let (runtime, owner) = model_runtime(&images, Err(unrecorded.clone()));
    assert_eq!(
        runtime.create(spec("image-unrecorded", None)).map(|_| ()),
        Err(unrecorded)
    );
    assert!(Transaction::records(owner.as_ref()).unwrap().is_empty());
    let (runtime, owner) = model_runtime(&images, waits());
    let odd_vm = CapacityResources {
        cpu_milli: 1_000,
        mem_bytes: 255 << 20,
        disk_bytes: 0,
    };
    let cost = hex_api::RuntimeCost {
        id: [3; 32],
        floor: CapacityResources::default(),
        ceiling: odd_vm,
        quantum: CapacityResources {
            cpu_milli: 1_000,
            mem_bytes: 1 << 20,
            disk_bytes: 1,
        },
        guest_overhead: CapacityResources::default(),
        overhead: CapacityResources::default(),
    };
    let mut unbootable = spec("image-odd", None);
    unbootable.allocation = Some(cost.allocate(odd_vm).unwrap());
    assert!(matches!(
        runtime.create(unbootable),
        Err(Refusal::Invalid { what: "shards image", why }) if why.contains("allocation")
    ));
    assert!(Transaction::records(owner.as_ref()).unwrap().is_empty());
    assert!(images.asked.lock().unwrap().is_empty());
}

/// Joint plan step 12: do create where the host has no image boot, as macOS's is until D231's run host
/// is started holding only its stdio. Expect: refused naming that, before any record.
#[test]
fn a_create_is_refused_before_any_record_on_a_host_without_an_image_boot() {
    let owner = Arc::new(Owner::default());
    let refused = Refusal::Awaits {
        needs: Upstream::RunHostStdioOnly,
    };
    let runtime = GuestRuntime::new(
        Arc::new(shards_api::sim::SimGuests::new(|_| {
            shards_api::End::PowerOff
        })),
        super::super::tests::config(),
    )
    .with_images(Err(refused.clone()), waits());
    let books: Arc<dyn Books> = owner.clone();
    runtime.bind_books(Arc::downgrade(&books)).unwrap();
    assert_eq!(
        runtime.create(spec("image-no-host", None)).map(|_| ()),
        Err(refused)
    );
    assert!(Transaction::records(owner.as_ref()).unwrap().is_empty());
}

/// Joint plan step 12, CLAUDE.md §12: do ask for this host's image boot. Expect: Shards' on Linux; on
/// Windows the refusal naming Shards' owed Windows run host; on macOS the one naming a run host started
/// holding only its stdio, since D231 starts it through std's spawn, which hands it every descriptor
/// another thread holds without close-on-exec.
#[test]
fn this_hosts_image_boot_is_shards_on_linux_and_names_what_d231_owes_elsewhere() {
    let host = host_images().map(|_| ());
    let expected = if cfg!(windows) {
        Err(Refusal::Awaits {
            needs: Upstream::WindowsRunHost,
        })
    } else if cfg!(target_os = "macos") {
        Err(Refusal::Awaits {
            needs: Upstream::RunHostStdioOnly,
        })
    } else {
        Ok(())
    };
    assert_eq!(host, expected);
}

/// Joint plan step 12: do signal KILL to a VM never started. Expect: no boot, its end recorded
/// Signal(9) and shown Empty, and its start then NotRunning.
#[test]
fn kill_before_start_ends_the_vm_without_a_boot() {
    let images = ModelImages::new();
    let (runtime, owner) = model_runtime(&images, waits());
    let id = runtime.create(spec("image-unstarted", None)).unwrap().id;
    runtime.signal(&id, Signal::KILL).unwrap();
    let kept = record(&owner, &id).unwrap();
    assert_eq!(
        (kept.phase, kept.exit),
        (Phase::Ready, Some(Exit::Signal(9)))
    );
    assert!(matches!(
        runtime.start(&id),
        Err(Refusal::NotRunning(_, "start"))
    ));
    assert!(images.asked.lock().unwrap().is_empty());
}
