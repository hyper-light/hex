//! The macOS and Windows nodes' charge learner (joint plan step 14, AC-T7.4.1; `docs/design/learned-charge.md`).

use std::cell::RefCell;

use super::*;
use hex_api::{Allocation, BoxId, BoxSpec, ClassSpec, OciSpec};
use hex_core::record::{NativeScope, ProcId};

/// The box a learned holding or end names: this learner holds boxes alone.
fn box_of(holder: &hex_api::LearnedHolder) -> &str {
    match holder {
        hex_api::LearnedHolder::Box(id) => id.as_str(),
        hex_api::LearnedHolder::Node(run) => panic!("a box's learner named a node VM, {run:?}"),
    }
}

/// `records` as the holders a learner reads.
fn holders(records: &[BoxRecord]) -> Vec<Holder<'_>> {
    boxes(records).unwrap()
}

const GIB: u64 = 1 << 30;

fn res(cpu_milli: u64, mem_bytes: u64) -> CapacityResources {
    CapacityResources {
        cpu_milli,
        mem_bytes,
        disk_bytes: 0,
    }
}

/// A Shards VM's profile: 16 CPUs and 8 GiB, its envelope its floor and ceiling.
fn cost() -> RuntimeCost {
    let envelope = res(16_000, 8 * GIB);
    RuntimeCost {
        id: [14; 32],
        floor: envelope,
        ceiling: envelope,
        quantum: CapacityResources {
            cpu_milli: 1,
            mem_bytes: 1,
            disk_bytes: 1,
        },
        guest_overhead: CapacityResources::default(),
        overhead: CapacityResources::default(),
    }
}

/// A running Shards VM's record, booked at 4 CPUs and 4 GiB (its class's prior), its lifetime `n`.
fn vm(name: &str, n: u8) -> BoxRecord {
    let allocation: Allocation = cost()
        .allocate(CapacityResources::default())
        .unwrap()
        .booked_at(res(4_000, 4 * GIB));
    let spec = BoxSpec {
        id: BoxId::new(name).unwrap(),
        bounds: hex_api::Bounds {
            cpu_max_milli: Some(16_000),
            memory_max: Some(8 * GIB),
            ..hex_api::Bounds::default()
        },
        class: ClassSpec::Shards(Box::new(OciSpec {
            rootfs: hex_api::Rootfs::Dir(std::path::PathBuf::from("/images/agent")),
            init: hex_api::Program::of(&["/bin/true"]),
            mounts: Vec::new(),
            hostname: None,
            network: hex_api::Network::Private,
            hosts: Vec::new(),
        })),
        owner: None,
        devices: Vec::new(),
        allocation: Some(allocation),
        route: None,
        band: 0,
    };
    let mut record = BoxRecord::new(spec, Phase::Ready, Vec::new());
    record.native_scope = Some(NativeScope::from_bytes([n; 16]));
    record.init = Some(ProcId {
        pid: u32::from(n),
        start_ticks: 1,
    });
    record
}

/// A cap as the learner told it (D260): the VM, its share, and its VMM's own as read or why not.
type Told = (String, u64, Result<u64, String>);

/// The managed VMs as a record: each one's CPU and memory as the test sets them, every cap set and every VM stopped,
/// in order.
#[derive(Default)]
struct Record {
    usage: RefCell<BTreeMap<String, Usage>>,
    /// Each image VM's host use, as the test sets it, and each room it was told, in order.
    host: RefCell<BTreeMap<String, HostUse>>,
    /// Each image VM's run's rate period, as the test sets it: the platform's window for a VM it sets none for.
    periods: RefCell<BTreeMap<String, Duration>>,
    /// The image VMs whose run's period the test has made unreadable.
    unread_periods: RefCell<Vec<String>>,
    /// Each managed VM's governor's period, as its VM process last answered it (D255), as the test sets it: the test's
    /// window for a VM its governor holds that it sets none for, none for one its platform's cap holds.
    governed: RefCell<BTreeMap<String, Duration>>,
    /// The cycle a platform's cap holds a VM over at any rate (C1), as the test sets it: none where it sets none.
    cycle: std::cell::Cell<Option<Duration>>,
    /// What holds each VM's rate once set (D255), as the test sets it: its governor for a VM it sets none for.
    held: RefCell<BTreeMap<String, hex_api::RateHeld>>,
    rooms: RefCell<Vec<(String, i64)>>,
    /// Each VM's share as it was capped, in order.
    rates: RefCell<Vec<(String, u64)>>,
    /// Each managed VM's guest time as its VM process answers its use (D260), as the test sets it: none apart for a VM
    /// it sets none for.
    guest: RefCell<BTreeMap<String, ManagedGuestTime>>,
    /// Each managed VM's life so far, as the test sets it.
    lives: RefCell<BTreeMap<String, Duration>>,
    /// Each cap as the learner told it (D260), in order.
    caps: RefCell<Vec<Told>>,
    /// Each VM's actuator's least share (C4), as the test sets it: a governor's for a VM it sets none for.
    leasts: RefCell<BTreeMap<String, u64>>,
    refuse_rates: std::cell::Cell<bool>,
    stops: RefCell<Vec<String>>,
    /// The node's records, where the test keeps them: an intent is written there, as the node writes it.
    records: RefCell<Option<Store>>,
    /// Every intent asked and every stop, in order: `intend <box>`, `stop <box>`.
    events: RefCell<Vec<String>>,
}

impl Managed for Record {
    fn usage(&self, holder: Holder<'_>) -> Result<Usage, Refusal> {
        let Holder::Box(record) = holder;
        self.usage
            .borrow()
            .get(record.spec.id.as_str())
            .cloned()
            .ok_or_else(|| Refusal::Broken("no such VM".into()))
    }

    fn guest_time(&self, holder: Holder<'_>) -> Result<ManagedGuestTime, Refusal> {
        let Holder::Box(record) = holder;
        Ok(self
            .guest
            .borrow()
            .get(record.spec.id.as_str())
            .cloned()
            .unwrap_or_else(|| ManagedGuestTime::Unheld("the test tells none".to_owned())))
    }

    fn cap_cycle(&self, _: u64) -> Result<Option<Duration>, Refusal> {
        Ok(self.cycle.get())
    }

    fn least(&self, holder: Holder<'_>) -> u64 {
        let Holder::Box(record) = holder;
        self.leasts
            .borrow()
            .get(record.spec.id.as_str())
            .copied()
            .unwrap_or(GOVERNOR_LEAST.get())
    }

    fn life(&self, holder: Holder<'_>) -> Result<Duration, Refusal> {
        let Holder::Box(record) = holder;
        self.lives
            .borrow()
            .get(record.spec.id.as_str())
            .copied()
            .ok_or_else(|| Refusal::Broken("no life set".into()))
    }

    fn host_use(&self, holder: Holder<'_>) -> Result<HostUse, Refusal> {
        let Holder::Box(record) = holder;
        self.host
            .borrow()
            .get(record.spec.id.as_str())
            .cloned()
            .ok_or_else(|| Refusal::Broken("no such VM".into()))
    }

    fn rate_period(&self, holder: Holder<'_>) -> Result<Option<Duration>, Refusal> {
        let Holder::Box(record) = holder;
        if self
            .unread_periods
            .borrow()
            .iter()
            .any(|name| name == record.spec.id.as_str())
        {
            return Err(Refusal::Broken("its run's period unread".into()));
        }
        if !running_image(record) {
            let name = record.spec.id.as_str();
            if let Some(period) = self.governed.borrow().get(name) {
                return Ok(Some(*period));
            }
            if matches!(
                self.held.borrow().get(name),
                Some(hex_api::RateHeld::Platform(_))
            ) {
                return Ok(None);
            }
            return Ok(Some(WINDOW));
        }
        Ok(Some(
            self.periods
                .borrow()
                .get(record.spec.id.as_str())
                .copied()
                .unwrap_or(WINDOW),
        ))
    }

    fn set_room(&self, holder: Holder<'_>, room: i64) -> Result<(), Refusal> {
        let Holder::Box(record) = holder;
        self.rooms
            .borrow_mut()
            .push((record.spec.id.as_str().to_owned(), room));
        Ok(())
    }

    fn set_rate(&self, holder: Holder<'_>, cap: Cap<'_>) -> Result<hex_api::RateHeld, Refusal> {
        let Holder::Box(record) = holder;
        if self.refuse_rates.get() {
            return Err(Refusal::Broken("the rate was refused".into()));
        }
        let name = record.spec.id.as_str().to_owned();
        self.rates.borrow_mut().push((name.clone(), cap.share));
        let own = match cap.own {
            Own::Read(own) => Ok(own),
            Own::Unheld(why) => Err(why.to_owned()),
        };
        self.caps.borrow_mut().push((name, cap.share, own));
        Ok(self
            .held
            .borrow()
            .get(record.spec.id.as_str())
            .cloned()
            .unwrap_or(hex_api::RateHeld::Governor))
    }

    fn stop(&self, holder: Holder<'_>) -> Result<(), Refusal> {
        let Holder::Box(record) = holder;
        self.stops
            .borrow_mut()
            .push(record.spec.id.as_str().to_owned());
        self.events
            .borrow_mut()
            .push(format!("stop {}", record.spec.id));
        Ok(())
    }

    fn intend(&self, id: &LearnedHolder, scope: Scope, end: &LearnedEnd) -> Result<bool, Refusal> {
        self.events
            .borrow_mut()
            .push(format!("intend {}", box_of(id)));
        match self.records.borrow().as_ref() {
            Some(store) => record_last_resort(store, id, scope, Some(end)),
            None => Ok(false),
        }
    }
}

impl Record {
    /// VM `name` has used `cpu_usec` of CPU in all and holds `memory`.
    fn set(&self, name: &str, cpu_usec: u64, memory: u64) {
        self.usage.borrow_mut().insert(
            name.to_owned(),
            Usage {
                cpu_usec,
                memory_bytes: memory,
                ..Usage::default()
            },
        );
    }
}

const WINDOW: Duration = Duration::from_millis(300);

/// Joint plan step 14 (AC-T7.4.1), C4 on macOS and Windows: a capping platform's node caps each managed VM at its
/// water-filled share. Do run a window of three Shards VMs of 16-CPU allocations, each booked at 4 CPUs, in a 24-CPU
/// fleet budget, then a window in which the first uses its whole 16-CPU cap, the second 1 CPU and the third 2; then
/// one more with nothing changed. Expect each VM capped first at its booking and an equal share of the 12 CPUs the
/// bookings leave (8 CPUs each); then the second and third at their bookings (they asked less) and the first at its
/// 16-CPU ceiling, its booking risen to the 8 CPUs it used and the 8 the others leave; the caps within the budget;
/// nothing set again when nothing changed; and the memory budgets refused, typed, naming what the platform lacks.
#[test]
fn w14_a_capping_node_caps_each_vm_at_its_water_filled_share() {
    let io = Record::default();
    let vms = vec![vm("vm-1", 1), vm("vm-2", 2), vm("vm-3", 3)];
    for record in &vms {
        io.set(record.spec.id.as_str(), 0, GIB);
    }
    let budget = res(24_000, 64 * GIB);
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    let start = Instant::now();
    learner.window(&io, &holders(&vms), budget, start).unwrap();
    assert_eq!(
        *io.rates.borrow(),
        vec![
            ("vm-1".to_owned(), 8_000),
            ("vm-2".to_owned(), 8_000),
            ("vm-3".to_owned(), 8_000),
        ]
    );
    io.rates.borrow_mut().clear();
    // 300 ms windows: the first used its whole 8-CPU cap (2.4 s), the second 1 CPU, the third 2 CPUs.
    io.set("vm-1", 2_400_000, GIB);
    io.set("vm-2", 300_000, GIB);
    io.set("vm-3", 600_000, GIB);
    learner
        .window(&io, &holders(&vms), budget, start + WINDOW)
        .unwrap();
    assert_eq!(
        *io.rates.borrow(),
        vec![
            ("vm-1".to_owned(), 16_000),
            ("vm-2".to_owned(), 4_000),
            ("vm-3".to_owned(), 4_000),
        ]
    );
    let caps: u64 = vms
        .iter()
        .map(|r| learner.rate(&Scope::of(r).unwrap()).unwrap())
        .sum();
    assert!(caps <= budget.cpu_milli, "{caps} past the budget");
    io.rates.borrow_mut().clear();
    io.set("vm-1", 2_400_000 + 4_800_000, GIB);
    io.set("vm-2", 300_000 + 300_000, GIB);
    io.set("vm-3", 600_000 + 600_000, GIB);
    learner
        .window(&io, &holders(&vms), budget, start + WINDOW * 2)
        .unwrap();
    assert!(io.rates.borrow().is_empty(), "{:?}", io.rates.borrow());
    assert!(matches!(
        learner.unheld(),
        Refusal::Platform {
            what: "memory_high",
            ..
        }
    ));
}

/// Joint plan step 14 (AC-T7.4.1), C4 and C8 on macOS and Windows: a cap the platform refuses fails the window. Do run
/// a window of two VMs whose caps the native owner refuses, then ask for the class's capacity answer, then let a
/// window pass. Expect the window refused with the owner's refusal, typed, no learned use on the answer, and the
/// answer's learned use again once a window passes: the VMs' receipts and holdings.
#[test]
fn w14_a_refused_cap_fails_the_window_typed() {
    let io = Record::default();
    let vms = vec![vm("vm-1", 1), vm("vm-2", 2)];
    for record in &vms {
        io.set(record.spec.id.as_str(), 0, GIB);
    }
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    io.refuse_rates.set(true);
    let start = Instant::now();
    assert_eq!(
        learner.window(&io, &holders(&vms), res(24_000, 64 * GIB), start),
        Err(Refusal::Broken("the rate was refused".into()))
    );
    assert_eq!(
        learner
            .learned(Class::Shards, &[cost()], &holders(&vms))
            .unwrap(),
        None
    );
    io.refuse_rates.set(false);
    learner
        .window(&io, &holders(&vms), res(24_000, 64 * GIB), start + WINDOW)
        .unwrap();
    let learned = learner
        .learned(Class::Shards, &[cost()], &holders(&vms))
        .unwrap()
        .unwrap();
    assert_eq!(learned.used.booked, res(8_000, 8 * GIB));
    assert_eq!(learned.holdings.len(), 2);
}

/// Joint plan step 14 (AC-T7.4.1), C2: a learner carries a cold class at the workload's admission, the census's VMM
/// overhead its ceiling's alone. Do ask a learner that has learned no VM for the capacity answer of a Shards profile
/// whose receipt is its 16-CPU, 8 GiB envelope and a census overhead of half a CPU and 256 MiB. Expect the profile's
/// prior carried at the envelope its VMs are admitted at, its ceiling the whole receipt.
#[test]
fn w14_a_cold_class_is_carried_at_its_admission() {
    let census = RuntimeCost {
        overhead: res(500, 256 << 20),
        ..cost()
    };
    let receipt = census.allocate(CapacityResources::default()).unwrap();
    let learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    let learned = learner
        .learned(Class::Shards, &[census], &[])
        .unwrap()
        .unwrap();
    assert_eq!(
        learned.priors,
        vec![hex_api::LearnedPrior {
            profile: census.id,
            ceiling: receipt.resources,
            booking: res(16_000, 8 * GIB),
        }]
    );
}

/// Joint plan step 14 (AC-T7.4.1): which records are running managed VMs. Do judge a running Shards VM's record, then
/// the same deleting, ended, with no controller, with no lifetime, and a process box's. Expect only the first running.
#[test]
fn w14_only_a_made_live_managed_vm_is_running() {
    let live = vm("vm-1", 1);
    assert!(running(&live));
    let mut deleting = live.clone();
    deleting.phase = Phase::Deleting;
    let mut ended = live.clone();
    ended.exit = Some(hex_api::Exit::Signal(9));
    let mut uncontrolled = live.clone();
    uncontrolled.init = None;
    let mut unscoped = live.clone();
    unscoped.native_scope = None;
    let mut process = live.clone();
    process.spec.class = ClassSpec::Process;
    for record in [deleting, ended, uncontrolled, unscoped, process] {
        assert!(!running(&record), "{record:?}");
    }
}

/// Three running Shards VMs, each booked at 4 GiB (their class's prior), whose fleet's memory budget is 16 GiB.
fn fleet_of_three() -> Vec<BoxRecord> {
    vec![vm("vm-1", 1), vm("vm-2", 2), vm("vm-3", 3)]
}

/// Joint plan step 14 (AC-T7.4.1), C5's last resort on macOS and Windows: a capping node ends a VM through its managed
/// receipt. Do run a window of three Shards VMs booked at 4 GiB in a 16 GiB fleet, then one where they hold 6, 7 and
/// 2 GiB, past the fleet's budget with the growth on record; then another with all three still running; then one with
/// vm-2 gone. Expect vm-2, the VM holding most past its own budget, stopped through its receipt, the class's capacity
/// answer saying why (its 7 GiB past its budget, the fleet's 15 GiB, 15 GiB of growth, its 16 GiB budget, its move
/// awaiting D176); no second stop while vm-2 runs; and none once it's gone, every VM then within its budget.
#[test]
fn w14_a_capping_node_ends_the_vm_most_past_its_budget_through_its_receipt() {
    let io = Record::default();
    let vms = fleet_of_three();
    for record in &vms {
        io.set(record.spec.id.as_str(), 0, 0);
    }
    let budget = res(24_000, 16 * GIB);
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    let start = Instant::now();
    learner.window(&io, &holders(&vms), budget, start).unwrap();
    for (name, held) in [("vm-1", 6 * GIB), ("vm-2", 7 * GIB), ("vm-3", 2 * GIB)] {
        io.set(name, 0, held);
    }
    learner
        .window(&io, &holders(&vms), budget, start + WINDOW)
        .unwrap();
    assert_eq!(*io.stops.borrow(), ["vm-2"]);
    assert!(
        matches!(learner.last.resort(), Resort::Ending { id, .. } if box_of(id) == "vm-2"),
        "{:?}",
        learner.last.resort()
    );
    let learned = learner
        .learned(Class::Shards, &[cost()], &holders(&vms))
        .unwrap()
        .unwrap();
    let ends: Vec<_> = learned
        .ended
        .iter()
        .map(|end| {
            (
                box_of(&end.id),
                end.held,
                end.budget < end.held,
                (end.fleet_held, end.fleet_growth, end.fleet_budget),
                end.unmoved,
            )
        })
        .collect();
    assert_eq!(
        ends,
        [(
            "vm-2",
            7 * GIB,
            true,
            (15 * GIB, 15 * GIB, 16 * GIB),
            Some(hex_api::Upstream::LiveMove)
        )]
    );
    learner
        .window(&io, &holders(&vms), budget, start + WINDOW * 2)
        .unwrap();
    assert_eq!(io.stops.borrow().len(), 1, "one end is never two");
    let left = vec![vm("vm-1", 1), vm("vm-3", 3)];
    learner
        .window(&io, &holders(&left), budget, start + WINDOW * 3)
        .unwrap();
    assert_eq!(io.stops.borrow().len(), 1, "never a VM within its budget");
    assert!(
        matches!(
            learner.last.resort(),
            Resort::Refused(Refusal::Exhausted { .. })
        ),
        "{:?}",
        learner.last.resort()
    );
}

/// A running Shards VM's record (as [`vm`]: booked at 4 GiB, its lifetime `scope`), begun `started`-th on its node.
fn begun(name: &str, scope: u8, started: u64) -> BoxRecord {
    let mut record = vm(name, scope);
    record.started = Some(started);
    record
}

/// Joint plan step 14 (AC-T7.4.1), C3's order on every OS (ROOT, 2026-11-12): of two VMs rising past their bookings
/// into the same room, the one begun later is over and the last resort ends it, whatever their scopes. Do run a window of
/// three Shards VMs booked at 4 GiB in a 16 GiB fleet, begun first, second and third on their node, their scopes in the
/// opposite order (the first's the largest), then one where the first two hold 7 GiB each and the third 2 GiB. Expect the
/// fleet's 4 GiB of room to the first (its budget 7 GiB) and the rest of it, 1 GiB, to the second (its budget 5 GiB), so
/// the second, 2 GiB past its budget, stopped through its receipt and no other VM; and its why on the class's capacity
/// answer naming it: 7 GiB held against its 5 GiB budget, the fleet's 16 GiB held, 16 GiB of growth, its 16 GiB budget.
#[test]
fn w14_of_two_vms_rising_into_one_room_the_later_begun_is_ended() {
    let io = Record::default();
    let vms = vec![
        begun("vm-first", 9, 1),
        begun("vm-second", 5, 2),
        begun("vm-third", 1, 3),
    ];
    for record in &vms {
        io.set(record.spec.id.as_str(), 0, 0);
    }
    let budget = res(24_000, 16 * GIB);
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    let start = Instant::now();
    learner.window(&io, &holders(&vms), budget, start).unwrap();
    for (name, held) in [
        ("vm-first", 7 * GIB),
        ("vm-second", 7 * GIB),
        ("vm-third", 2 * GIB),
    ] {
        io.set(name, 0, held);
    }
    learner
        .window(&io, &holders(&vms), budget, start + WINDOW)
        .unwrap();
    assert_eq!(*io.stops.borrow(), ["vm-second"]);
    let learned = learner
        .learned(Class::Shards, &[cost()], &holders(&vms))
        .unwrap()
        .unwrap();
    assert_eq!(
        learned.ended,
        vec![LearnedEnd {
            id: hex_api::LearnedHolder::Box(BoxId::new("vm-second").unwrap()),
            held: 7 * GIB,
            budget: 5 * GIB,
            band: 0,
            fleet_held: 16 * GIB,
            fleet_growth: 16 * GIB,
            fleet_budget: 16 * GIB,
            unmoved: Some(hex_api::Upstream::LiveMove),
        }]
    );
}

/// The records a node keeps of its boxes, in `dir`, as `Recorded::open` makes them.
fn books(dir: &std::path::Path) -> crate::recorded::Books {
    crate::recorded::Books {
        store: crate::store::Store::open(&dir.join("state"), &dir.join("run")).unwrap(),
        prober: None,
        lifecycle: crate::turns::Turns::new(
            "changing a recorded box's lifecycle",
            "recorded lifecycle methods",
            crate::store::TURNS,
            hex_core::patience::Patience::seeded("recorded.lifecycle", Duration::ZERO),
        ),
        abandoned: crate::abandoned::Abandoned::default(),
        adopted: std::sync::Mutex::new(std::collections::BTreeSet::new()),
        last_boot: None,
        charges: std::sync::OnceLock::new(),
        node_vm_ends: std::sync::Mutex::new(Vec::new()),
    }
}

/// Joint plan step 14 (AC-T7.4.1), C5 on the box itself on macOS and Windows (ROOT, 2026-11-12): a capping node's
/// inspect says why its last resort ended a VM. Do record a Shards VM in a node's store as its create does, write
/// why the last resort ended it into its record as the node does after the window, and count its inspect from its
/// backend's info as the node does. Expect the inspect to say why.
#[test]
fn w14_a_capping_nodes_inspect_says_why_its_last_resort_ended_a_vm() {
    let dir = tempfile::tempdir().unwrap();
    let books = books(dir.path());
    let spec = vm("vm-2", 2).spec;
    let made = BoxRecord::new(spec.clone(), Phase::Creating, Vec::new());
    books.store.begin(&made).unwrap();
    books.store.made(&made).unwrap();
    let end = hex_api::LearnedEnd {
        id: hex_api::LearnedHolder::Box(spec.id.clone()),
        held: 7 * GIB,
        budget: 6 * GIB,
        band: 0,
        fleet_held: 15 * GIB,
        fleet_growth: 15 * GIB,
        fleet_budget: 16 * GIB,
        unmoved: Some(hex_api::Upstream::LiveMove),
    };
    let written = end.clone();
    assert!(
        books
            .store
            .update(&spec.id, move |record| {
                record.last_resort = Some(written);
                true
            })
            .unwrap()
    );
    let info = books
        .counted(hex_api::BoxInfo {
            id: spec.id.clone(),
            class: hex_api::Class::Shards,
            state: hex_api::BoxState::Empty,
            bounds: spec.bounds.clone(),
            allocation: spec.allocation,
            adopted: false,
            membership: None,
            exit: Some(hex_api::Exit::Signal(9)),
            abandoned: 0,
            route: None,
            grants: Vec::new(),
            published: Vec::new(),
            last_resort: None,
            ended: None,
            credentials: Vec::new(),
        })
        .unwrap();
    assert_eq!(info.last_resort, Some(end));
}

/// Joint plan step 14 (AC-T7.4.1), C5's record of why, bounded (CLAUDE.md §2), as every OS's learner keeps it. Do record
/// three ends on a learner for a 20 milli-CPU machine, whose population is two VMs, the third of another class. Expect
/// the class's capacity answer to carry its two latest ends, latest first, and the oldest forgotten once a third came.
#[test]
fn w14_the_last_resorts_ends_are_kept_to_the_machines_population() {
    let mut learner = Learner::new(true, res(20, 64 * GIB)).unwrap();
    let why = |held: u64| charge::Excess {
        held,
        budget: GIB,
        band: 0,
        fleet: charge::Pressure {
            held: 4 * GIB,
            growth: GIB,
            budget: 4 * GIB,
        },
    };
    for (n, class) in [
        (1, Class::Shards),
        (2, Class::Shards),
        (3, Class::Firecracker),
    ] {
        let id = BoxId::new(&format!("vm-{n}")).unwrap();
        learner
            .last
            .record_end(
                class,
                &LearnedHolder::Box(id),
                Scope::Native([u8::try_from(n).unwrap(); 16]),
                why(n * GIB),
            )
            .unwrap();
    }
    let ids = |class| -> Vec<String> {
        learner
            .learned(class, &[], &[])
            .unwrap()
            .unwrap()
            .ended
            .iter()
            .map(|end| box_of(&end.id).to_owned())
            .collect()
    };
    assert_eq!(ids(Class::Shards), ["vm-2"], "vm-1 forgotten past two");
    assert_eq!(ids(Class::Firecracker), ["vm-3"]);
}

/// Three Shards VMs recorded in `store` as a node's create records them, booked at 4 GiB, as the node reads them.
fn recorded_three(store: &Store) -> Vec<BoxRecord> {
    ["vm-1", "vm-2", "vm-3"]
        .iter()
        .zip(1..)
        .map(|(name, n)| {
            let made = BoxRecord::new(vm(name, n).spec, Phase::Creating, Vec::new());
            store.begin(&made).unwrap();
            store.made(&made).unwrap();
            store.get(&made.spec.id).unwrap().unwrap().unwrap()
        })
        .collect()
}

/// Joint plan step 14 (AC-T7.4.1), C5 intent first on macOS and Windows: why survives a node that crashes after a
/// VM's end and before the window's writes. Do record three Shards VMs in a node's store as its create does, run the
/// learner's windows past their fleet's 16 GiB memory budget, vm-2 holding 12 GiB and the others 2 GiB (so the VM past
/// its budget is vm-2 whatever order the store's fresh lifetimes put them in), until it stops vm-2, then crash: leave
/// unmade the writes the window keeps for after it, and reopen the store as the restarted node does. Expect vm-2's
/// record to say why it was ended, its intent written before its stop, and vm-1's and vm-3's records nothing.
#[test]
fn w14_why_survives_a_crash_between_a_capping_nodes_end_and_its_writes() {
    let dir = tempfile::tempdir().unwrap();
    let open = || Store::open(&dir.path().join("state"), &dir.path().join("run")).unwrap();
    let store = open();
    let vms = recorded_three(&store);
    let io = Record::default();
    *io.records.borrow_mut() = Some(store.clone());
    for record in &vms {
        io.set(record.spec.id.as_str(), 0, 0);
    }
    let budget = res(24_000, 16 * GIB);
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    let start = Instant::now();
    learner.window(&io, &holders(&vms), budget, start).unwrap();
    for (name, held) in [("vm-1", 2 * GIB), ("vm-2", 12 * GIB), ("vm-3", 2 * GIB)] {
        io.set(name, 0, held);
    }
    learner
        .window(&io, &holders(&vms), budget, start + WINDOW)
        .unwrap();
    assert_eq!(*io.stops.borrow(), ["vm-2"]);
    let events = io.events.take();
    // The crash: the learner and its store go with the node, the window's writes unmade.
    drop(learner);
    drop(io);
    drop(store);
    let store = open();
    let end = store
        .get(&vms[1].spec.id)
        .unwrap()
        .unwrap()
        .unwrap()
        .last_resort;
    let end = end.expect("vm-2's why, written before its end");
    assert_eq!((box_of(&end.id), end.held), ("vm-2", 12 * GIB));
    for other in [&vms[0], &vms[2]] {
        let record = store.get(&other.spec.id).unwrap().unwrap().unwrap();
        assert_eq!(record.last_resort, None);
    }
    assert_eq!(events, ["intend vm-2", "stop vm-2"]);
}

/// Joint plan step 14 (AC-T7.4.1), C5 intent first on macOS and Windows: a node that crashed between a VM's intent and
/// its end leaves that end to the next. Do record three Shards VMs in a node's store, vm-2's record naming why it is
/// to be ended as a node that crashed right after writing it leaves it, then run a restarted node's learner over the
/// three, each holding 2 GiB of their 16 GiB fleet, within every budget, then another window. Expect vm-2 stopped in
/// the first window for the why its record names, that why on its class's capacity answer, and no other VM stopped.
#[test]
fn w14_a_restarted_capping_node_carries_out_the_end_its_intent_names() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("state"), &dir.path().join("run")).unwrap();
    let made = recorded_three(&store);
    let scope = Scope::Native(*made[1].native_scope.unwrap().bytes());
    let intent = LearnedEnd {
        id: hex_api::LearnedHolder::Box(made[1].spec.id.clone()),
        held: 7 * GIB,
        budget: 6 * GIB,
        band: 0,
        fleet_held: 15 * GIB,
        fleet_growth: 15 * GIB,
        fleet_budget: 16 * GIB,
        unmoved: Some(hex_api::Upstream::LiveMove),
    };
    assert!(
        record_last_resort(
            &store,
            &LearnedHolder::Box(made[1].spec.id.clone()),
            scope,
            Some(&intent)
        )
        .unwrap()
    );
    let vms: Vec<BoxRecord> = made
        .iter()
        .map(|record| store.get(&record.spec.id).unwrap().unwrap().unwrap())
        .collect();
    let io = Record::default();
    for record in &vms {
        io.set(record.spec.id.as_str(), 0, 2 * GIB);
    }
    let budget = res(24_000, 16 * GIB);
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    let start = Instant::now();
    learner.window(&io, &holders(&vms), budget, start).unwrap();
    assert_eq!(*io.stops.borrow(), ["vm-2"]);
    let learned = learner
        .learned(Class::Shards, &[cost()], &holders(&vms))
        .unwrap()
        .unwrap();
    assert_eq!(learned.ended, vec![intent]);
    learner
        .window(&io, &holders(&vms), budget, start + WINDOW)
        .unwrap();
    assert_eq!(io.stops.borrow().len(), 1, "one end is never two");
}

/// Joint plan step 14 (AC-T7.4.1), the learner's bounds (CLAUDE.md §2) on macOS and Windows, as Linux's ([`bounds`]):
/// VMs, its machine's population; classes, as many, the least at which no VM's class is refused. Do run, on a machine
/// of 20 milli-CPU (a population of two), two VMs each of a class of its own; then the first gone and a third of a third
/// class; then a fourth beside those two. Expect the first two held; the third held, the gone VM's class forgotten for
/// it; and the fourth refused, typed, naming the machine's population.
#[test]
fn w14_a_capping_learner_holds_its_machines_population_of_vms_of_as_many_classes() {
    let io = Record::default();
    // Each VM a class of its own: its receipt's profile is its number.
    let own = |name: &str, n: u8| {
        let mut record = vm(name, n);
        if let Some(allocation) = record.spec.allocation.as_mut() {
            allocation.cost.id = [n; 32];
        }
        io.set(name, 0, GIB);
        record
    };
    let (one, two, three, four) = (
        own("vm-1", 1),
        own("vm-2", 2),
        own("vm-3", 3),
        own("vm-4", 4),
    );
    let machine = res(20, 64 * GIB);
    let mut learner = Learner::new(true, machine).unwrap();
    let start = Instant::now();
    learner
        .window(&io, &holders(&[one, two.clone()]), machine, start)
        .unwrap();
    learner
        .window(
            &io,
            &holders(&[two.clone(), three.clone()]),
            machine,
            start + WINDOW,
        )
        .unwrap();
    assert_eq!(
        learner.window(
            &io,
            &holders(&[two, three, four]),
            machine,
            start + WINDOW * 2
        ),
        Err(Refusal::Exhausted {
            what: "a node's VMs",
            bound: 2
        })
    );
}

/// Joint plan step 14 (AC-T7.4.1), C5's queues, bounded (CLAUDE.md §2) at the ends a last resort keeps: the ends a node
/// before this one decided, due, at most one a VM met; the writes into VMs' records not yet made, one a VM lifetime, the
/// oldest forgotten past the bound. Do meet three VMs whose records name intents, and keep an end for each of three
/// VMs, on a last resort keeping two. Expect the third intent refused, typed, naming the bound, and the first VM's write
/// forgotten.
#[test]
fn w14_the_last_resorts_queues_are_kept_to_its_bound() {
    let end = |n: u8| LearnedEnd {
        id: hex_api::LearnedHolder::Box(BoxId::new(&format!("vm-{n}")).unwrap()),
        held: 2 * GIB,
        budget: GIB,
        band: 0,
        fleet_held: 4 * GIB,
        fleet_growth: GIB,
        fleet_budget: 4 * GIB,
        unmoved: Some(Upstream::LiveMove),
    };
    let mut last = LastResort::new(2);
    last.met(Class::Shards, Scope::Native([1; 16]), Some(end(1)))
        .unwrap();
    last.met(Class::Shards, Scope::Native([2; 16]), Some(end(2)))
        .unwrap();
    assert_eq!(
        last.met(Class::Shards, Scope::Native([3; 16]), Some(end(3))),
        Err(Refusal::Exhausted {
            what: "the last resort's ends due",
            bound: 2
        })
    );
    for n in 1..=3 {
        last.write(end(n).id, Scope::Native([n; 16]), Some(end(n)))
            .unwrap();
    }
    let kept: Vec<String> = last
        .unrecorded()
        .into_iter()
        .map(|(id, _, _)| box_of(&id).to_owned())
        .collect();
    assert_eq!(kept, ["vm-2", "vm-3"]);
}

/// A running Shards image VM's record, booked at 4 CPUs and 4 GiB (its class's prior), its lifetime the `started`th its
/// node began: no native scope, its run its node's.
fn image_vm(name: &str, started: u64) -> BoxRecord {
    let allocation: Allocation = cost()
        .allocate(CapacityResources::default())
        .unwrap()
        .booked_at(res(4_000, 4 * GIB));
    let spec = BoxSpec {
        id: BoxId::new(name).unwrap(),
        bounds: hex_api::Bounds::default(),
        class: ClassSpec::ShardsImage(Box::new(hex_api::ShardsImage::new(hex_api::sim::SIM_IMAGE))),
        owner: None,
        devices: Vec::new(),
        allocation: Some(allocation),
        route: None,
        band: 0,
    };
    let mut record = BoxRecord::new(spec, Phase::Ready, Vec::new());
    record.started = Some(started);
    record
}

/// Joint plan step 14 (C1, C4, C5, C7; AC-T7.4.1): a capping node learns a Shards image VM through its run (D241, D247,
/// D188). Do run a window of an image VM of 16 vCPUs, booked at 4 CPUs and 4 GiB, in a 24-CPU fleet budget; then one in
/// which its VM process ran 3.2 s of CPU in 300 ms, 2.4 s of it its vCPUs' guest time, holding 3 GiB; then one whose
/// run host told 15 vCPUs' guest time. Expect it read through its host use, its lifetime its place in the start order;
/// held at its ceiling's 16 CPUs until it has run, told so from its first window with its VMM's own unheld (D260: its
/// run here tells no span to reckon it over), then capped through its rate at the 10.667 CPUs it was charged, its
/// governor's rate leaving out its VMM's own over the window, 2.666 CPUs; its guest told its room, its budget less what
/// it holds, as it moves (64 GiB, then 61 GiB); and the third window refused, typed, its guest time naming a vCPU count
/// other than its reading before's.
#[test]
fn w14_a_capping_node_learns_an_image_vm_through_its_run() {
    let io = Record::default();
    let vm = image_vm("img-1", 1);
    let use_at = |cpu_ns: u64, each_ns: u64, vcpus: u64, held: u64| HostUse {
        cpu_ns,
        footprint_bytes: held,
        resident_bytes: held,
        guest_ns: vec![each_ns; usize::try_from(vcpus).unwrap()],
        window: std::time::Duration::ZERO,
    };
    io.host
        .borrow_mut()
        .insert("img-1".to_owned(), use_at(0, 0, 16, GIB));
    let budget = res(24_000, 64 * GIB);
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    let start = Instant::now();
    learner
        .window(&io, &holders(std::slice::from_ref(&vm)), budget, start)
        .unwrap();
    assert_eq!(
        *io.caps.borrow(),
        vec![(
            "img-1".to_owned(),
            16_000,
            Err("its life so far reads none".to_owned())
        )],
        "held at its 16-CPU ceiling until it has run"
    );
    io.host.borrow_mut().insert(
        "img-1".to_owned(),
        use_at(3_200_000_000, 150_000_000, 16, 3 * GIB),
    );
    learner
        .window(
            &io,
            &holders(std::slice::from_ref(&vm)),
            budget,
            start + WINDOW,
        )
        .unwrap();
    assert_eq!(
        io.caps.borrow().last(),
        Some(&("img-1".to_owned(), 10_667, Ok(2_666)))
    );
    assert_eq!(
        *io.rooms.borrow(),
        vec![
            ("img-1".to_owned(), i64::try_from(64 * GIB).unwrap()),
            ("img-1".to_owned(), i64::try_from(61 * GIB).unwrap()),
        ]
    );
    io.host.borrow_mut().insert(
        "img-1".to_owned(),
        use_at(3_500_000_000, 160_000_000, 15, 3 * GIB),
    );
    assert!(matches!(
        learner.window(&io, &holders(&[vm]), budget, start + WINDOW * 2),
        Err(Refusal::Invalid { why, .. }) if why.contains("vCPU count")
    ));
}

/// Joint plan step 14 (AC-T7.4.1), C1 with no census (ROOT, 2026-11-13; decision 9): a node whose cost carries no VMM
/// overhead books a cold class at its envelope, says its cold start, and learns the overhead from its windows. Do
/// answer for a Shards profile of a 16-CPU, 8 GiB envelope and no census overhead before any window; then run a window
/// of a VM of it booked at its envelope, and one in which its VMM holds 300 MiB past the envelope and runs 1 CPU.
/// Expect the first answer's ceiling the envelope, every dimension of its overhead cold, typed; then the VM charged its
/// 8 GiB and 300 MiB, past the envelope, and the answer's ceiling carrying 300 MiB of memory overhead and no CPU past
/// the envelope, its unread disk still cold.
#[test]
fn w14_a_census_free_class_starts_cold_and_learns_its_vmm_overhead() {
    let mut record = vm("vm-1", 1);
    record.spec.allocation = Some(cost().allocate(CapacityResources::default()).unwrap());
    let records = std::slice::from_ref(&record);
    let io = Record::default();
    io.set("vm-1", 0, GIB);
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    let envelope = res(16_000, 8 * GIB);
    let ceilings = |learner: &Learner| {
        learner
            .learned(Class::Shards, &[cost()], &holders(records))
            .unwrap()
            .unwrap()
            .ceilings
    };
    assert_eq!(
        ceilings(&learner),
        vec![hex_api::LearnedCeiling {
            profile: cost().id,
            envelope,
            overhead: hex_api::LearnedOverhead::default(),
        }]
    );
    let start = Instant::now();
    let budget = res(24_000, 64 * GIB);
    learner
        .window(&io, &holders(records), budget, start)
        .unwrap();
    io.set("vm-1", 300_000, 8 * GIB + (300 << 20));
    learner
        .window(&io, &holders(records), budget, start + WINDOW)
        .unwrap();
    let lifetime = learner.vms.get(&Scope::Native([1; 16])).unwrap().lifetime;
    assert_eq!(
        learner.node.charge(&lifetime).unwrap().mem_bytes,
        8 * GIB + (300 << 20)
    );
    assert_eq!(
        ceilings(&learner),
        vec![hex_api::LearnedCeiling {
            profile: cost().id,
            envelope,
            overhead: hex_api::LearnedOverhead {
                cpu_milli: Some(0),
                mem_bytes: Some(300 << 20),
                disk_bytes: None,
            },
        }]
    );
}

/// Joint plan step 14 (AC-T7.4.1), C8: a class's capacity answer carries the holding of each VM its learner learns, a
/// Shards image VM's as a managed VM's. Do run a window of a managed VM and an image VM, then answer for the class from
/// the node's records. Expect both VMs' holdings on the answer.
#[test]
fn w14_an_answer_carries_each_learned_vms_holding_an_image_vms_too() {
    let io = Record::default();
    io.set("vm-1", 0, GIB);
    io.host.borrow_mut().insert(
        "img-1".to_owned(),
        HostUse {
            cpu_ns: 0,
            footprint_bytes: GIB,
            resident_bytes: GIB,
            guest_ns: vec![0; 16],
            window: std::time::Duration::ZERO,
        },
    );
    let records = vec![vm("vm-1", 1), image_vm("img-1", 2)];
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    learner
        .window(
            &io,
            &holders(&records),
            res(24_000, 64 * GIB),
            Instant::now(),
        )
        .unwrap();
    let mut capacity = hex_api::Capacity {
        own: Default::default(),
        id: "shards".into(),
        total: res(32_000, 64 * GIB),
        free: CapacityResources::default(),
        devices: Vec::new(),
        cost: Some(cost()),
        routes: Vec::new(),
        device_routes: Vec::new(),
        backings: Vec::new(),
        unavailable: Vec::new(),
        learned: None,
        node_vms: Vec::new(),
    };
    learner
        .answer(
            Class::Shards,
            &mut capacity,
            records.iter().map(Holder::Box),
        )
        .unwrap();
    let mut held: Vec<String> = capacity
        .learned
        .unwrap()
        .holdings
        .iter()
        .map(|holding| box_of(&holding.id).to_owned())
        .collect();
    held.sort();
    assert_eq!(held, ["img-1", "vm-1"]);
}

/// Joint plan step 14 (C4; AC-T7.4.1): a window books off the fleet's budget every box it doesn't learn. Do sort a
/// managed VM, a Shards image VM, a process box capped at 2 CPUs and 1 GiB and an ended image VM, for a learner whose
/// platform has no window for managed VMs, then for one whose platform has one. Expect, with none, the image VM learned
/// (its cap's period its run's own), the managed VM booked whole at its 16-CPU, 8 GiB allocation and the box at its
/// caps, the ended VM nowhere; with one, the managed VM learned too.
#[test]
fn w14_a_window_books_whole_each_vm_it_doesnt_learn() {
    let boxed = BoxRecord::new(
        BoxSpec {
            id: BoxId::new("box-1").unwrap(),
            bounds: hex_api::Bounds {
                cpu_max_milli: Some(2_000),
                memory_max: Some(GIB),
                ..hex_api::Bounds::default()
            },
            class: ClassSpec::Process,
            owner: None,
            devices: Vec::new(),
            allocation: None,
            route: None,
            band: 0,
        },
        Phase::Ready,
        Vec::new(),
    );
    let mut ended = image_vm("img-2", 2);
    ended.exit = Some(hex_api::Exit::Code(0));
    let records = || {
        vec![
            vm("vm-1", 1),
            image_vm("img-1", 1),
            boxed.clone(),
            ended.clone(),
        ]
    };
    let ids = |sorted: &Sorted| -> Vec<String> {
        sorted
            .learned
            .iter()
            .map(|holder| box_of(&holder.id().unwrap()).to_owned())
            .collect()
    };
    let records = records();
    let unmanaged = sort(&records, |_| false).unwrap();
    assert_eq!(ids(&unmanaged), ["img-1"]);
    assert_eq!(unmanaged.whole, res(16_000, 8 * GIB));
    assert_eq!(unmanaged.native, res(2_000, GIB));
    let both = sort(&records, |_| true).unwrap();
    assert_eq!(ids(&both), ["vm-1", "img-1"]);
    assert_eq!(both.whole, CapacityResources::default());
    assert_eq!(both.native, res(2_000, GIB));
}

/// Image VM `name`'s host use with nothing used yet, its 16 vCPUs' guest time none and `held` held.
fn unused(io: &Record, name: &str, held: u64) {
    io.host.borrow_mut().insert(
        name.to_owned(),
        HostUse {
            cpu_ns: 0,
            footprint_bytes: held,
            resident_bytes: held,
            guest_ns: vec![0; 16],
            window: Duration::ZERO,
        },
    );
}

/// Joint plan step 14 (C1; ROOT, 2026-11-13): a capping learner's window is the longest period of the caps it holds,
/// each image VM's its run's own (D247's `ImageGuest::rate_period`), re-read each window, never a number of its own:
/// its platform's cap's cycle counts only for a VM that cap holds. Do run a window of two image VMs whose runs say
/// 100 ms and 500 ms, on a learner whose platform's cap derives cycles; then one after the 500 ms run has ended; then one
/// after the other run says 700 ms (a period Shards derives later reaches Hex as it is). Expect its window after each
/// the longest of those it then holds: 500 ms, 100 ms, then 700 ms.
#[test]
fn w14_a_capping_learners_window_is_the_longest_period_of_the_caps_it_holds_re_read_each_window() {
    let io = Record::default();
    let short = image_vm("img-1", 1);
    let long = image_vm("img-2", 2);
    unused(&io, "img-1", GIB);
    unused(&io, "img-2", GIB);
    io.periods
        .borrow_mut()
        .insert("img-1".to_owned(), Duration::from_millis(100));
    io.periods
        .borrow_mut()
        .insert("img-2".to_owned(), Duration::from_millis(500));
    let budget = res(24_000, 64 * GIB);
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    let start = Instant::now();
    learner
        .window(&io, &holders(&[short.clone(), long]), budget, start)
        .unwrap();
    assert_eq!(
        learner.next_window(),
        Some(Duration::from_millis(500)),
        "the longest run's period"
    );
    learner
        .window(
            &io,
            &holders(std::slice::from_ref(&short)),
            budget,
            start + Duration::from_millis(500),
        )
        .unwrap();
    assert_eq!(
        learner.next_window(),
        Some(Duration::from_millis(100)),
        "the shorter run's, once the longer run has ended: no cap of its platform's holds a VM"
    );
    io.periods
        .borrow_mut()
        .insert("img-1".to_owned(), Duration::from_millis(700));
    learner
        .window(
            &io,
            &holders(std::slice::from_ref(&short)),
            budget,
            start + Duration::from_millis(800),
        )
        .unwrap();
    assert_eq!(
        learner.next_window(),
        Some(Duration::from_millis(700)),
        "re-read each window"
    );
}

/// Joint plan step 14 (C1): the learner's thread waits out its window whatever comes meanwhile, as the window's end
/// reads every VM then held; with no window (a node with no platform window holding no image VM), until a box begins;
/// and ends once its node has gone. Do wait out a 40 ms window with a box's beginning told before it; wait with no
/// window, a box's beginning told; then wait, with no window and with one, once the node's end has dropped. Expect the
/// first to look only once its window has passed, the second to look at once, and the last two gone.
#[test]
fn w14_the_learners_thread_waits_out_its_window_and_with_none_for_a_box_to_begin() {
    let (begun, waits) = hex_wait::mpsc::sync_channel(1).unwrap();
    begun.try_send(()).unwrap();
    let window = Duration::from_millis(40);
    let asked = Instant::now();
    assert_eq!(wait(&waits, Some(window)).unwrap(), Waited::Look);
    assert!(
        asked.elapsed() >= window,
        "a beginning doesn't cut a window short"
    );
    begun.try_send(()).unwrap();
    assert_eq!(wait(&waits, None).unwrap(), Waited::Look);
    drop(begun);
    assert_eq!(wait(&waits, None).unwrap(), Waited::Gone);
    assert_eq!(wait(&waits, Some(window)).unwrap(), Waited::Gone);
}

/// A native owner with no window of its own, as Windows' is (its job's hard cap holds per an undocumented interval),
/// over the simulated runtime: its Shards image VMs' runs each say the period the test sets for its box, answer their
/// rooms and rates as told and read as using nothing, and it counts the learner's reads of a period.
#[derive(Default)]
struct Imaging {
    sim: hex_api::sim::SimRuntime,
    periods: std::sync::Mutex<BTreeMap<String, Duration>>,
    reads: std::sync::atomic::AtomicUsize,
}

#[derive(Clone, Default)]
struct Images(std::sync::Arc<Imaging>);

impl hex_api::Runtime for Images {
    fn classes(&self) -> Vec<hex_api::Class> {
        self.0.sim.classes()
    }
    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        self.0.sim.create(spec)
    }
    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        self.0.sim.start(id)
    }
    fn exec(
        &self,
        id: &BoxId,
        program: hex_api::Program,
        stdio: hex_api::Stdio,
    ) -> Result<Box<dyn hex_api::Proc>, Refusal> {
        self.0.sim.exec(id, program, stdio)
    }
    fn run(
        &self,
        spec: BoxSpec,
        program: hex_api::Program,
        stdio: hex_api::Stdio,
    ) -> Result<Box<dyn hex_api::Proc>, Refusal> {
        self.0.sim.run(spec, program, stdio)
    }
    fn signal(&self, id: &BoxId, signal: hex_api::Signal) -> Result<(), Refusal> {
        self.0.sim.signal(id, signal)
    }
    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        self.0.sim.inspect(id)
    }
    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        self.0.sim.usage(id)
    }
    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        self.0.sim.list()
    }
    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        self.0.sim.delete(id)
    }
    fn room(&self, _: &BoxId, room: Option<i64>) -> Result<Option<u64>, Refusal> {
        Ok(room.and_then(|room| u64::try_from(room).ok()))
    }
    fn rate(
        &self,
        _: &BoxId,
        rate: Option<std::num::NonZeroU64>,
    ) -> Result<Option<std::num::NonZeroU64>, Refusal> {
        Ok(rate)
    }
    fn host_use(&self, _: &BoxId) -> Result<HostUse, Refusal> {
        Ok(HostUse {
            cpu_ns: 0,
            footprint_bytes: GIB,
            resident_bytes: GIB,
            guest_ns: Vec::new(),
            window: Duration::ZERO,
        })
    }
}

impl crate::recorded::Recover for Images {
    /// It keeps no native metadata of its own: nothing to cross-check against the retained cohort.
    fn preflight_retained(
        &self,
        _: &hex_runtime::devices::Records,
        _: &[hex_core::record::PoolRecord],
    ) -> Result<(), Refusal> {
        Ok(())
    }
    fn trace(&self, _: &BoxId) -> Result<Option<ProcId>, Refusal> {
        Ok(None)
    }
    fn adopt(&self, _: &BoxSpec, _: Option<ProcId>) -> Result<(), Refusal> {
        Ok(())
    }
    fn end(&self, _: Option<ProcId>) -> Result<(), Refusal> {
        Ok(())
    }
    fn image_rate_period(&self, record: &BoxRecord) -> Result<Duration, Refusal> {
        self.0
            .reads
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        match self.0.periods.lock().unwrap().get(record.spec.id.as_str()) {
            Some(period) => Ok(*period),
            None => Err(Refusal::NotRunning(
                record.spec.id.try_clone()?,
                "rate_period",
            )),
        }
    }
}

/// The learner's thread woken by an image VM's making: a fact no recorded operation makes, so it measures, for its
/// fact alone (hex_testkit::waits).
static WOKEN: hex_core::patience::Patience =
    hex_core::patience::Patience::measuring("test.hex_node.charges_woken");

/// Joint plan step 14 (C1; ROOT, 2026-11-13): a node whose platform holds no window of its own (Windows': its job's
/// interval undocumented) still learns its Shards image VMs, each over its run's period. Do start the charge learner
/// of such a node; run a window holding no VM; then make an image VM whose run says 70 ms. Expect the learner started,
/// its window none while it holds no VM, the VM's making waking it to read the run's period, and its window then the
/// run's 70 ms.
#[test]
fn w14_a_node_with_no_platform_window_learns_its_image_vms_over_their_runs_periods() {
    let dir = tempfile::tempdir().unwrap();
    let images = Images::default();
    let node =
        std::sync::Arc::new(crate::recorded::Recorded::open(images.clone(), dir.path()).unwrap());
    node.start_charges().unwrap();
    assert_eq!(node.learn_window(), None, "no window while it holds no VM");
    images
        .0
        .periods
        .lock()
        .unwrap()
        .insert("img-1".to_owned(), Duration::from_millis(70));
    let spec = BoxSpec {
        id: BoxId::new("img-1").unwrap(),
        bounds: hex_api::Bounds::default(),
        class: ClassSpec::ShardsImage(Box::new(hex_api::ShardsImage::new(hex_api::sim::SIM_IMAGE))),
        owner: None,
        devices: Vec::new(),
        allocation: Some(hex_api::sim::sim_vm_allocation().unwrap()),
        route: None,
        band: 0,
    };
    hex_api::Runtime::create(node.as_ref(), spec).unwrap();
    hex_testkit::waits::until(&WOKEN, "the learner woken by an image VM's making", || {
        images.0.reads.load(std::sync::atomic::Ordering::SeqCst) > 0
    })
    .unwrap();
    assert_eq!(node.learn_window(), Some(Duration::from_millis(70)));
}

/// Joint plan step 14 (AC-T7.4.1; ROOT, 2026-11-13: the starving case the done-when names): a running VM whose read
/// fails counts whole, at its receipt, against the fleet's budget until a read succeeds, its learner untouched, and
/// the failure is told. Do run a window of three Shards VMs of 16-CPU allocations, each booked at 4 CPUs, in a 24-CPU
/// fleet budget, the third's read failing, and answer the class's capacity; then one in which it reads. Expect the first
/// two capped at their bookings, the 16 CPUs of the third's receipt taken from the budget, the third named among the
/// unread with its refusal and carrying no holding on the answer, so the scheduler counts it at its receipt; then none
/// unread, the third, which has run no window, capped at its 16-CPU ceiling and the two idle at their bookings.
#[test]
fn w14_a_vm_whose_read_fails_counts_whole_against_the_budget_until_it_reads() {
    let io = Record::default();
    let vms = vec![vm("vm-1", 1), vm("vm-2", 2), vm("vm-3", 3)];
    io.set("vm-1", 0, GIB);
    io.set("vm-2", 0, GIB);
    let budget = res(24_000, 64 * GIB);
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    let start = Instant::now();
    learner.window(&io, &holders(&vms), budget, start).unwrap();
    assert_eq!(
        *io.rates.borrow(),
        vec![("vm-1".to_owned(), 4_000), ("vm-2".to_owned(), 4_000)]
    );
    let unread: Vec<&str> = learner.unread().iter().map(|(id, _)| box_of(id)).collect();
    assert_eq!(unread, ["vm-3"]);
    let learned = learner
        .learned(Class::Shards, &[cost()], &holders(&vms))
        .unwrap()
        .unwrap();
    let held: Vec<&str> = learned
        .holdings
        .iter()
        .map(|h| match &h.id {
            hex_api::LearnedHolder::Box(id) => id.as_str(),
            hex_api::LearnedHolder::Node(run) => panic!("a node VM's holding, {run:?}"),
        })
        .collect();
    assert!(!held.contains(&"vm-3"), "{held:?}");
    io.rates.borrow_mut().clear();
    io.set("vm-3", 0, GIB);
    learner
        .window(&io, &holders(&vms), budget, start + WINDOW)
        .unwrap();
    assert!(learner.unread().is_empty());
    let caps: Vec<u64> = vms
        .iter()
        .map(|r| learner.rate(&Scope::of(r).unwrap()).unwrap())
        .collect();
    assert_eq!(caps, [4_000, 4_000, 16_000]);
}

/// Joint plan step 14 (C1; AC-T7.4.1; ROOT, 2026-11-13): an image VM whose run's period can't be read counts whole, at
/// its receipt, against the fleet's budget until it reads, its learner untouched and the failure told, as one whose use
/// can't be read does. Do run a window of three image VMs of 16 vCPUs, each booked at 4 CPUs, in a 24-CPU fleet budget,
/// the third's period unreadable, and answer the class's capacity; then one in which it reads. Expect the first two
/// capped at their bookings, the 16 CPUs of the third's receipt taken from the budget, the third named among the unread
/// with its period's refusal and carrying no holding on the answer; then none unread, the third, which has run no
/// window, at its 16-CPU ceiling and the two idle at their bookings.
#[test]
fn w14_an_image_vm_whose_period_read_fails_counts_whole_against_the_budget_until_it_reads() {
    let io = Record::default();
    let vms = vec![
        image_vm("img-1", 1),
        image_vm("img-2", 2),
        image_vm("img-3", 3),
    ];
    for name in ["img-1", "img-2", "img-3"] {
        unused(&io, name, GIB);
    }
    io.unread_periods.borrow_mut().push("img-3".to_owned());
    let budget = res(24_000, 64 * GIB);
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    let start = Instant::now();
    learner.window(&io, &holders(&vms), budget, start).unwrap();
    assert_eq!(
        *io.rates.borrow(),
        vec![("img-1".to_owned(), 4_000), ("img-2".to_owned(), 4_000)]
    );
    let unread: Vec<(&str, String)> = learner
        .unread()
        .iter()
        .map(|(id, why)| (box_of(id), why.to_string()))
        .collect();
    assert_eq!(unread.len(), 1, "{unread:?}");
    assert!(
        unread
            .iter()
            .all(|(id, why)| *id == "img-3" && why.contains("period")),
        "{unread:?}"
    );
    let learned = learner
        .learned(Class::Shards, &[cost()], &holders(&vms))
        .unwrap()
        .unwrap();
    let held: Vec<&str> = learned
        .holdings
        .iter()
        .map(|h| match &h.id {
            hex_api::LearnedHolder::Box(id) => id.as_str(),
            hex_api::LearnedHolder::Node(run) => panic!("a node VM's holding, {run:?}"),
        })
        .collect();
    assert!(!held.contains(&"img-3"), "{held:?}");
    io.rates.borrow_mut().clear();
    io.unread_periods.borrow_mut().clear();
    learner
        .window(&io, &holders(&vms), budget, start + WINDOW)
        .unwrap();
    assert!(learner.unread().is_empty());
    let caps: Vec<u64> = vms
        .iter()
        .map(|r| learner.rate(&Scope::of(r).unwrap()).unwrap())
        .collect();
    assert_eq!(caps, [4_000, 4_000, 16_000]);
}

/// Joint plan step 14 (D255, C1; ROOT, 2026-11-13): a managed VM's governor holds its rate over its VM process's own
/// period, so the learner's window is the longest of those periods, re-read each window, as an image VM's run's is: its
/// platform's cap's cycle counts only for a VM that cap holds. Do run a window of a managed VM whose governor answered a
/// 500 ms period, on a learner whose platform's cap derives cycles; then one after its process answered 200 ms. Expect
/// the window 500 ms, then 200 ms.
#[test]
fn w14_d255_a_managed_vms_governor_period_is_the_learners_window() {
    let io = Record::default();
    let vms = vec![vm("vm-1", 1)];
    io.set("vm-1", 0, GIB);
    io.governed
        .borrow_mut()
        .insert("vm-1".to_owned(), Duration::from_millis(500));
    let budget = res(24_000, 64 * GIB);
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    let start = Instant::now();
    learner.window(&io, &holders(&vms), budget, start).unwrap();
    assert_eq!(learner.next_window(), Some(Duration::from_millis(500)));
    io.governed
        .borrow_mut()
        .insert("vm-1".to_owned(), Duration::from_millis(200));
    learner
        .window(&io, &holders(&vms), budget, start + WINDOW)
        .unwrap();
    assert_eq!(learner.next_window(), Some(Duration::from_millis(200)));
}

/// Joint plan step 14 (D255; ROOT, 2026-11-13): what holds each managed VM's rate is told on its class's capacity
/// answer, those its governor doesn't hold alone first. Do run a window of three Shards VMs of 16-CPU allocations in a
/// 24-CPU budget, whose caps the learner sets: the first's governor holds it, the second's process takes no rate so its
/// platform's cap holds it, the third's governor holds it under its job; then answer the class's capacity. Expect the
/// rates told: the second's platform and why, the third under its job, then the first's governor.
#[test]
fn w14_d255_what_holds_each_managed_vms_rate_is_told_on_its_classs_capacity_answer() {
    let io = Record::default();
    let vms = vec![vm("vm-1", 1), vm("vm-2", 2), vm("vm-3", 3)];
    for name in ["vm-1", "vm-2", "vm-3"] {
        io.set(name, 0, GIB);
    }
    let unsupported = "its VM process takes no rate".to_owned();
    io.held.borrow_mut().insert(
        "vm-2".to_owned(),
        hex_api::RateHeld::Platform(unsupported.clone()),
    );
    io.held
        .borrow_mut()
        .insert("vm-3".to_owned(), hex_api::RateHeld::GovernorUnderJob);
    let budget = res(24_000, 64 * GIB);
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    learner
        .window(&io, &holders(&vms), budget, Instant::now())
        .unwrap();
    assert_eq!(io.rates.borrow().len(), 3, "{:?}", io.rates.borrow());
    let learned = learner
        .learned(Class::Shards, &[cost()], &holders(&vms))
        .unwrap()
        .unwrap();
    let told: Vec<(&str, &hex_api::RateHeld)> = learned
        .rates
        .iter()
        .map(|rate| match &rate.id {
            hex_api::LearnedHolder::Box(id) => (id.as_str(), &rate.held),
            hex_api::LearnedHolder::Node(run) => panic!("a node VM's rate, {run:?}"),
        })
        .collect();
    assert_eq!(
        told,
        [
            ("vm-2", &hex_api::RateHeld::Platform(unsupported)),
            ("vm-3", &hex_api::RateHeld::GovernorUnderJob),
            ("vm-1", &hex_api::RateHeld::Governor),
        ]
    );
}

/// Joint plan step 14 (D255; ROOT, 2026-11-13): on a platform with no window (Windows today), the learner's first sight
/// of a managed VM asks its governor at the VM's ceiling, which holds nothing below its vCPUs: one whose governor answers
/// a period is learned over it and capped at its learned rate; one whose process takes no rate stays booked whole, told.
/// Do, on a learner with no platform window, see two Shards VMs of 16-CPU allocations, the first's governor answering a
/// 100 ms period and the second's process taking no rate; sort them, run a window of what it learns in a 6-CPU budget,
/// and answer the class's capacity. Expect both asked at their 16-CPU ceilings; the first learned and the second booked
/// whole; the first capped below its ceiling; its 100 ms the window; and the rates told: the second's platform and why,
/// then the first's governor.
#[test]
fn w14_d255_with_no_platform_window_a_managed_vm_is_learned_over_its_governors_period() {
    let io = Record::default();
    let vms = vec![vm("vm-1", 1), vm("vm-2", 2)];
    for name in ["vm-1", "vm-2"] {
        io.set(name, 0, GIB);
    }
    io.governed
        .borrow_mut()
        .insert("vm-1".to_owned(), Duration::from_millis(100));
    let unsupported = "its VM process takes no rate".to_owned();
    io.held.borrow_mut().insert(
        "vm-2".to_owned(),
        hex_api::RateHeld::Platform(unsupported.clone()),
    );
    let mut learner = Learner::new(false, res(32_000, 64 * GIB)).unwrap();
    learner.govern(&io, &holders(&vms)).unwrap();
    assert_eq!(
        *io.rates.borrow(),
        vec![("vm-1".to_owned(), 16_000), ("vm-2".to_owned(), 16_000)]
    );
    let sorted = sort(&vms, |holder| learner.learns(holder)).unwrap();
    let learned: Vec<String> = sorted
        .learned
        .iter()
        .map(|holder| box_of(&holder.id().unwrap()).to_owned())
        .collect();
    assert_eq!(learned, ["vm-1"]);
    assert_eq!(sorted.whole.cpu_milli, 16_000, "the second's receipt");
    io.rates.borrow_mut().clear();
    learner
        .window(&io, &sorted.learned, res(6_000, 64 * GIB), Instant::now())
        .unwrap();
    let rates = io.rates.borrow().clone();
    assert!(
        matches!(rates.as_slice(), [(id, rate)] if id == "vm-1" && *rate < 16_000),
        "{rates:?}"
    );
    assert_eq!(learner.next_window(), Some(Duration::from_millis(100)));
    let answer = learner
        .learned(Class::Shards, &[cost()], &holders(&vms))
        .unwrap()
        .unwrap();
    let told: Vec<(&str, &hex_api::RateHeld)> = answer
        .rates
        .iter()
        .map(|rate| match &rate.id {
            hex_api::LearnedHolder::Box(id) => (id.as_str(), &rate.held),
            hex_api::LearnedHolder::Node(run) => panic!("a node VM's rate, {run:?}"),
        })
        .collect();
    assert_eq!(
        told,
        [
            ("vm-2", &hex_api::RateHeld::Platform(unsupported)),
            ("vm-1", &hex_api::RateHeld::Governor),
        ]
    );
}

/// A managed Shards VM's guest time as its VM process answers its use (D260): each of its 16 vCPUs at `each_ns`.
fn sixteen(each_ns: u64) -> ManagedGuestTime {
    ManagedGuestTime::PerVcpu(vec![each_ns; 16])
}

/// Joint plan step 14 (D260; AC-T7.4.1; ROOT, 2026-11-13): a managed VM is held split from its VM process's first use
/// answer, its governor at its share less its VMM's own: over its life so far at that first answer, then over each
/// window. Do run a capping learner's first window over a managed Shards VM of 16 vCPUs, booked at 4 CPUs in a 24-CPU
/// budget, whose VM process answers its first use as the window reads it: its 16 vCPUs ran 2.5 s of its guest and its
/// scope 3 s of CPU in the 10 s it has run; then a window 300 ms on, in which its scope ran 360 ms of CPU, 300 ms of it
/// its vCPUs'. Expect it capped in its first window at its 16-CPU share with its VMM's own over its life, 50 milli-CPU,
/// left out of its governor's rate; then at its 4-CPU booking with its VMM's own over the window, 200 milli-CPU.
#[test]
fn w14_d260_a_managed_vm_is_held_split_from_its_first_use_answer() {
    let io = Record::default();
    let vm = vm("vm-1", 1);
    io.set("vm-1", 3_000_000, GIB);
    io.guest
        .borrow_mut()
        .insert("vm-1".to_owned(), sixteen(156_250_000));
    io.lives
        .borrow_mut()
        .insert("vm-1".to_owned(), Duration::from_secs(10));
    let budget = res(24_000, 64 * GIB);
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    let start = Instant::now();
    learner
        .window(&io, &holders(std::slice::from_ref(&vm)), budget, start)
        .unwrap();
    assert_eq!(*io.caps.borrow(), vec![("vm-1".to_owned(), 16_000, Ok(50))]);
    io.set("vm-1", 3_360_000, GIB);
    io.guest
        .borrow_mut()
        .insert("vm-1".to_owned(), sixteen(175_000_000));
    learner
        .window(
            &io,
            &holders(std::slice::from_ref(&vm)),
            budget,
            start + WINDOW,
        )
        .unwrap();
    assert_eq!(
        io.caps.borrow().last(),
        Some(&("vm-1".to_owned(), 4_000, Ok(200)))
    );
}

/// Joint plan step 14 (D260; AC-T7.4.1; ROOT, 2026-11-13): until its VM process's first use answer a managed VM's VMM's
/// own isn't read apart, and its cap says why; its first answer, whenever it comes, splits it over its life so far. Do
/// run a window over a managed Shards VM of 16 vCPUs, booked at 4 CPUs, whose process answers its microVM hasn't
/// started, its scope having run 2.94 s of CPU; then one 300 ms on whose answer tells its 16 vCPUs ran 2.5 s of its
/// guest in the 10 s it has run, its scope 3 s. Expect its first cap at its 16-CPU share, its VMM's own unheld in its
/// process's words; its second at its 4-CPU booking, its VMM's own over its life, 50 milli-CPU.
#[test]
fn w14_d260_until_its_first_use_answer_a_vms_own_is_unheld_and_told() {
    let io = Record::default();
    let vm = vm("vm-1", 1);
    io.set("vm-1", 2_940_000, GIB);
    io.guest.borrow_mut().insert(
        "vm-1".to_owned(),
        ManagedGuestTime::Unheld("the microVM has not started".to_owned()),
    );
    let budget = res(24_000, 64 * GIB);
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    let start = Instant::now();
    learner
        .window(&io, &holders(std::slice::from_ref(&vm)), budget, start)
        .unwrap();
    assert_eq!(
        *io.caps.borrow(),
        vec![(
            "vm-1".to_owned(),
            16_000,
            Err("the microVM has not started".to_owned())
        )]
    );
    io.set("vm-1", 3_000_000, GIB);
    io.guest
        .borrow_mut()
        .insert("vm-1".to_owned(), sixteen(156_250_000));
    io.lives
        .borrow_mut()
        .insert("vm-1".to_owned(), Duration::from_secs(10));
    learner
        .window(
            &io,
            &holders(std::slice::from_ref(&vm)),
            budget,
            start + WINDOW,
        )
        .unwrap();
    assert_eq!(
        io.caps.borrow().last(),
        Some(&("vm-1".to_owned(), 4_000, Ok(50)))
    );
}

/// Joint plan step 14 (D260; AC-T7.4.1; ROOT, 2026-11-13): a VM's governor is told its share less its VMM's own, never
/// none, and what holds it says so. Do take its governor's rate and what holds it, on a platform whose job holds its
/// share and on one with none, for a 4-CPU share whose VMM's own is 300 milli-CPU; is the whole share; is past it; and
/// isn't read. Expect 3,700, held by its governor; the least rate a governor takes, 1 milli-CPU (`RATE`'s unit, Shards'
/// D247 and D255 taking milli-CPU and none letting the vCPUs be), told as its VMM's own taking its share on either
/// platform, a finding; and while unread its whole share, under its job where it has one, else its VMM's own unheld,
/// why told.
#[test]
fn w14_d260_a_vms_governor_is_its_share_less_its_vmms_own_never_none_and_says_so() {
    assert_eq!(GOVERNOR_LEAST, NonZeroU64::MIN);
    let cap = |own| Cap { share: 4_000, own };
    let read = cap(Own::Read(300));
    assert_eq!(read.governor().get(), 3_700);
    for job in [true, false] {
        assert_eq!(read.held(job), hex_api::RateHeld::Governor);
    }
    for own in [4_000, 5_000] {
        let eaten = cap(Own::Read(own));
        assert_eq!(eaten.governor(), GOVERNOR_LEAST);
        for job in [true, false] {
            assert_eq!(
                eaten.held(job),
                hex_api::RateHeld::OwnTakesShare { own, share: 4_000 }
            );
        }
    }
    let unheld = cap(Own::Unheld("the microVM has not started"));
    assert_eq!(unheld.governor().get(), 4_000);
    assert_eq!(unheld.held(true), hex_api::RateHeld::GovernorUnderJob);
    assert_eq!(
        unheld.held(false),
        hex_api::RateHeld::GovernorOwnUnheld("the microVM has not started".to_owned())
    );
}

/// Joint plan step 14 (D260; AC-T7.4.1; ROOT, 2026-11-13): an image VM's governor takes its share less its VMM's own
/// too, split from its run's first answer: Σown past the fleet's budget is what D260 closes. Do run a window of an image
/// VM of 16 vCPUs whose run host says its VM process ran 3 s of CPU, 2.5 s of it its vCPUs' guest time, in the 10 s since
/// its run started; then one 300 ms on, in which it ran 360 ms, 300 ms of it its vCPUs'. Expect its first cap at its
/// 16-CPU share, its VMM's own over its run, 50 milli-CPU; its second at its 4-CPU booking, its VMM's own over the
/// window, 200 milli-CPU.
#[test]
fn w14_d260_an_image_vms_governor_takes_its_share_less_its_vmms_own() {
    let io = Record::default();
    let vm = image_vm("img-1", 1);
    let use_at = |cpu_ns: u64, each_ns: u64, window: Duration| HostUse {
        cpu_ns,
        footprint_bytes: GIB,
        resident_bytes: GIB,
        guest_ns: vec![each_ns; 16],
        window,
    };
    io.host.borrow_mut().insert(
        "img-1".to_owned(),
        use_at(3_000_000_000, 156_250_000, Duration::from_secs(10)),
    );
    let budget = res(24_000, 64 * GIB);
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    let start = Instant::now();
    learner
        .window(&io, &holders(std::slice::from_ref(&vm)), budget, start)
        .unwrap();
    assert_eq!(
        *io.caps.borrow(),
        vec![("img-1".to_owned(), 16_000, Ok(50))]
    );
    io.host.borrow_mut().insert(
        "img-1".to_owned(),
        use_at(3_360_000_000, 175_000_000, Duration::from_millis(10_300)),
    );
    learner
        .window(
            &io,
            &holders(std::slice::from_ref(&vm)),
            budget,
            start + WINDOW,
        )
        .unwrap();
    assert_eq!(
        io.caps.borrow().last(),
        Some(&("img-1".to_owned(), 4_000, Ok(200)))
    );
}

/// Node VM `n`'s run.
fn node_run(n: u8) -> hex_api::NodeRun {
    hex_api::NodeRun::from_bytes([n; 16])
}

/// Node VM `n`'s row in `phase`, sized at the Shards profile's admission, an Ended one with its exit.
fn node_row(n: u8, phase: hex_api::NodeVmPhase) -> hex_core::record::NodeVmRecord {
    hex_core::record::NodeVmRecord {
        version: hex_core::record::RECORD_VERSION,
        run: node_run(n),
        image: "hex-node:rev@sha256:00".to_owned(),
        allocation: cost().allocate(CapacityResources::default()).unwrap(),
        phase,
        diagnostics: None,
        exit: if phase == hex_api::NodeVmPhase::Ended {
            Some(hex_api::Exit::Signal(9))
        } else {
            None
        },
        last_resort: None,
    }
}

/// Why a last resort ended node VM `n`: 3 GiB held of a 2 GiB budget in band 0, its fleet past its budget.
fn node_end(n: u8) -> LearnedEnd {
    LearnedEnd {
        id: hex_api::LearnedHolder::Node(node_run(n)),
        held: 3 * GIB,
        budget: 2 * GIB,
        band: 0,
        fleet_held: 9 * GIB,
        fleet_growth: GIB,
        fleet_budget: 8 * GIB,
        unmoved: Some(Upstream::LiveMove),
    }
}

/// `row` as an earlier node process left it in the node state `dir`: its bytes in its place, its lock held by none.
fn leave_node_row(dir: &std::path::Path, row: &hex_core::record::NodeVmRecord) {
    let rows = dir.join(".node-vms");
    std::fs::create_dir_all(&rows).unwrap();
    let name: String = row.run.bytes().iter().map(|b| format!("{b:02x}")).collect();
    std::fs::write(rows.join(format!("{name}.json")), row.encode().unwrap()).unwrap();
}

/// Joint plan step 39, place 1 (C5, intent first; ROOT, 2026-11-13: a node VM's last-resort end lives in its row): the
/// last resort's intent for a node VM goes into its row, as a box's goes into its record. Do write why it is ending node
/// VM 7 into its Ready row through the write the learner makes; withdraw it; write it under another lifetime's scope;
/// and write one for a run with no row. Expect the end written and read back from the row, then gone; the other
/// lifetime's and the rowless run's not written.
#[test]
fn w39_the_last_resorts_intent_for_a_node_vm_is_written_into_its_row() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::store::Store::open(dir.path(), &dir.path().join("@records")).unwrap();
    store
        .put_node_vm(&node_row(7, hex_api::NodeVmPhase::Booting))
        .unwrap();
    store
        .put_node_vm(&node_row(7, hex_api::NodeVmPhase::Ready))
        .unwrap();
    let holder = hex_api::LearnedHolder::Node(node_run(7));
    let scope = Scope::Run(node_run(7));
    let row = |store: &crate::store::Store| store.node_vm(&node_run(7)).unwrap().unwrap();
    assert!(record_last_resort(&store, &holder, scope, Some(&node_end(7))).unwrap());
    assert_eq!(row(&store).last_resort, Some(node_end(7)));
    assert!(record_last_resort(&store, &holder, scope, None).unwrap());
    assert_eq!(row(&store).last_resort, None);
    assert!(
        !record_last_resort(&store, &holder, Scope::Run(node_run(8)), Some(&node_end(7))).unwrap()
    );
    assert_eq!(row(&store).last_resort, None);
    let rowless = hex_api::LearnedHolder::Node(node_run(9));
    assert!(
        !record_last_resort(
            &store,
            &rowless,
            Scope::Run(node_run(9)),
            Some(&node_end(9))
        )
        .unwrap()
    );
}

/// Joint plan step 39, place 1 (AC-T7.4.1; ROOT, 2026-11-13): a node VM's last-resort end survives its node's restart in
/// its row, and the next node's learner carries it, so its class's answers tell it as they told it before. Do leave
/// node VM 4's row Ended and node VM 5's Ready with no process holding its lock, each with why the last resort ended
/// it, as an earlier node would; then open a node over them, recover it and start its charge learner. Expect the
/// recovery to say both ends on record, and the learner's Shards ends to name both node VMs, by their runs.
#[test]
fn w39_a_node_vms_last_resort_end_is_read_back_at_start_into_the_learners_ends() {
    let dir = tempfile::tempdir().unwrap();
    let mut ended = node_row(4, hex_api::NodeVmPhase::Ended);
    ended.last_resort = Some(node_end(4));
    let mut unowned = node_row(5, hex_api::NodeVmPhase::Ready);
    unowned.last_resort = Some(node_end(5));
    leave_node_row(dir.path(), &ended);
    leave_node_row(dir.path(), &unowned);
    let node = std::sync::Arc::new(
        crate::recorded::Recorded::open(Images::default(), dir.path()).unwrap(),
    );
    let recovered = node.recover().unwrap();
    assert_eq!(
        recovered.node_vm_ends,
        crate::store::NodeVmEnds::Carried(vec![node_end(4), node_end(5)])
    );
    node.start_charges().unwrap();
    let (learner, _) = node.books.charges.get().unwrap();
    let learned = learner
        .lock()
        .unwrap()
        .learned(Class::Shards, &[cost()], &[])
        .unwrap()
        .unwrap();
    let mut ended: Vec<LearnedEnd> = learned.ended;
    ended.sort_by_key(|end| match &end.id {
        hex_api::LearnedHolder::Node(run) => *run,
        hex_api::LearnedHolder::Box(id) => panic!("a box's end, {id}"),
    });
    assert_eq!(ended, [node_end(4), node_end(5)]);
}

/// Joint plan step 39, place 1 (ROOT, 2026-11-13: a cold start is told, never covered by a guess): do open and recover
/// a node over a state no node VM row lies in. Expect the recovery to say no node VM's last-resort end is on record.
#[test]
fn w39_a_node_with_no_node_vm_end_on_record_says_so_at_start() {
    let dir = tempfile::tempdir().unwrap();
    let node = crate::recorded::Recorded::open(Images::default(), dir.path()).unwrap();
    let recovered = node.recover().unwrap();
    assert_eq!(
        recovered.node_vm_ends,
        crate::store::NodeVmEnds::NoneOnRecord
    );
    assert_eq!(
        recovered.node_vm_ends.to_string(),
        "node VM: no last-resort end on record"
    );
}

/// Joint plan step 14 (C4; ROOT, 2026-11-13; AC-T7.4.1): each VM's share is held at least at its own actuator's least,
/// never Hex's cgroup least: an image VM's governor's, a thousandth of a CPU (`GOVERNOR_LEAST`); a managed VM's its
/// platform's cap's, here a Windows job's on 128 processors, 13 thousandths (`hex_core::job::least_milli`). Do run a
/// window of an image VM and a managed VM, each booked no CPU, in a fleet budget of a thousandth of a CPU. Expect the
/// image VM's share 1 and the managed VM's 13.
#[test]
fn w14_each_vms_share_is_held_at_its_own_actuators_least() {
    let io = Record::default();
    let mut image = image_vm("img-1", 1);
    let mut managed = vm("vm-1", 2);
    for record in [&mut image, &mut managed] {
        record.spec.allocation = Some(
            cost()
                .allocate(CapacityResources::default())
                .unwrap()
                .booked_at(res(0, 4 * GIB)),
        );
    }
    unused(&io, "img-1", GIB);
    io.set("vm-1", 0, GIB);
    io.leasts.borrow_mut().insert("vm-1".to_owned(), 13);
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    learner
        .window(
            &io,
            &holders(&[image, managed]),
            res(1, 64 * GIB),
            Instant::now(),
        )
        .unwrap();
    let mut shares = io.rates.borrow().clone();
    shares.sort();
    assert_eq!(
        shares,
        vec![("img-1".to_owned(), 1), ("vm-1".to_owned(), 13)]
    );
}

/// Joint plan step 14 (C1; ROOT, 2026-11-13): a platform cap's cycle sets the learner's window only for a VM that cap
/// holds, at that VM's own share, and where it does, the class's capacity answer names that VM and the window: every
/// VM's read waits on it, a slowdown told, never silent. Do run a window of a managed VM its governor holds over a
/// 100 ms period, on a learner whose platform's cap holds a VM over 2 s at any rate; then a window with a second managed
/// VM whose process takes no rate, so that cap holds it; then answer the class's capacity. Expect the first window's
/// next window 100 ms, told with no VM named; then 2 s, told naming the second VM.
#[test]
fn w14_a_platform_caps_cycle_sets_the_window_only_for_the_vms_it_holds_and_is_told() {
    let io = Record::default();
    let governed = vm("vm-1", 1);
    let held = vm("vm-2", 2);
    io.set("vm-1", 0, GIB);
    io.set("vm-2", 0, GIB);
    io.governed
        .borrow_mut()
        .insert("vm-1".to_owned(), Duration::from_millis(100));
    io.held.borrow_mut().insert(
        "vm-2".to_owned(),
        hex_api::RateHeld::Platform("its VM process takes no rate".into()),
    );
    io.cycle.set(Some(Duration::from_secs(2)));
    let budget = res(24_000, 64 * GIB);
    let mut learner = Learner::new(true, res(32_000, 64 * GIB)).unwrap();
    let start = Instant::now();
    learner
        .window(
            &io,
            &holders(std::slice::from_ref(&governed)),
            budget,
            start,
        )
        .unwrap();
    assert_eq!(learner.next_window(), Some(Duration::from_millis(100)));
    let told = |learner: &Learner| {
        learner
            .learned(
                Class::Shards,
                &[cost()],
                &holders(&[governed.clone(), held.clone()]),
            )
            .unwrap()
            .unwrap()
            .window
    };
    assert_eq!(
        told(&learner),
        Some(hex_api::LearnedWindow {
            window_ns: 100_000_000,
            slowed_by: None,
        })
    );
    learner
        .window(
            &io,
            &holders(&[governed.clone(), held.clone()]),
            budget,
            start + Duration::from_millis(100),
        )
        .unwrap();
    assert_eq!(learner.next_window(), Some(Duration::from_secs(2)));
    assert_eq!(
        told(&learner),
        Some(hex_api::LearnedWindow {
            window_ns: 2_000_000_000,
            slowed_by: Some(LearnedHolder::Box(BoxId::new("vm-2").unwrap())),
        })
    );
}
