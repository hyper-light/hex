//! The node agent's charge learner (joint plan step 14, AC-T7.4.1; `docs/design/learned-charge.md`).

use std::cell::RefCell;

use super::*;
use hex_sim::charge::SEEDABLE;

const GIB: u64 = 1 << 30;

/// A Shards VM's profile: half of a 32-CPU host and 8 GiB, its envelope its floor and ceiling.
fn cost() -> RuntimeCost {
    let envelope = CapacityResources {
        cpu_milli: 16_000,
        mem_bytes: 8 * GIB,
        disk_bytes: 0,
    };
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

fn res(cpu_milli: u64, mem_bytes: u64) -> CapacityResources {
    CapacityResources {
        cpu_milli,
        mem_bytes,
        disk_bytes: 0,
    }
}

fn machine() -> CapacityResources {
    res(32_000, 64 * GIB)
}

/// The cgroups as a record: each VM's use as the test sets it, the fleets under `.vm`, and every write by path.
#[derive(Default)]
struct Record {
    usage: RefCell<BTreeMap<PathBuf, Usage>>,
    fleets: RefCell<Vec<(PathBuf, Option<String>)>>,
    writes: RefCell<Vec<(PathBuf, &'static str, String)>>,
}

impl Cgroups for Record {
    fn usage(&self, cgroup: &BoxCgroup) -> Result<Usage, Refusal> {
        self.usage
            .borrow()
            .get(cgroup.dir())
            .cloned()
            .ok_or_else(|| Refusal::Broken("no such cgroup".into()))
    }

    fn write(&self, cgroup: &BoxCgroup, write: &Write) -> Result<(), Refusal> {
        self.writes.borrow_mut().push((
            cgroup.dir().to_path_buf(),
            write.file,
            write.value.clone(),
        ));
        Ok(())
    }

    fn fleets(&self, _: &Path) -> Result<Vec<(PathBuf, Option<String>)>, Refusal> {
        Ok(self.fleets.borrow().clone())
    }
}

impl Record {
    fn set(&self, dir: &Path, cpu_usec: u64, memory_bytes: u64) {
        self.usage.borrow_mut().insert(
            dir.to_path_buf(),
            Usage {
                cpu_usec,
                memory_bytes,
                ..Usage::default()
            },
        );
    }

    fn take(&self) -> Vec<(PathBuf, &'static str, String)> {
        std::mem::take(&mut *self.writes.borrow_mut())
    }
}

fn root() -> PathBuf {
    PathBuf::from("/sys/fs/cgroup/runner/hex")
}

fn fleet() -> PathBuf {
    root().join(".vm").join("node-a")
}

fn running(ids: &[(&str, u8)]) -> Vec<(BoxId, Scope, BoxCgroup)> {
    ids.iter()
        .map(|(id, n)| {
            (
                BoxId::new(id).unwrap(),
                [*n; 16],
                BoxCgroup::at(fleet().join(id)),
            )
        })
        .collect()
}

fn whole() -> Allocation {
    cost().allocate(CapacityResources::default()).unwrap()
}

fn shards(_: &BoxId) -> Result<Option<(Class, Allocation)>, Refusal> {
    Ok(Some((Class::Shards, whole())))
}

/// Joint plan step 14 (AC-T7.4.1), C4 and C5 in the node: the placement's cgroup paths and weights. Do run a window
/// of two cold Shards VMs, each booked at its whole 16-CPU, 8 GiB receipt, on a 32-CPU machine whose fleet budget is
/// half of it, beside another node's fleet budgeted 8 CPUs; then a second window with nothing changed. Expect each
/// VM's own cgroup given its weight by booking and `memory.high` its budget held at its 8 GiB ceiling (its `cpu.max`
/// and `memory.max` left as its create wrote them); `.vm/node-a` capped at 16 CPUs and weighing half the machine;
/// `.vm` weighing both fleets' 24 CPUs of 32; and the second window writing nothing.
#[test]
fn w14_the_learner_places_each_vm_its_fleet_and_vm_by_path_and_weight() {
    let io = Record::default();
    io.fleets.borrow_mut().extend([
        (fleet(), None),
        (
            root().join(".vm").join("node-b"),
            Some("800000 100000".into()),
        ),
    ]);
    let vms = running(&[("vm-1", 1), ("vm-2", 2)]);
    for (_, _, cgroup) in &vms {
        io.set(cgroup.dir(), 0, GIB);
    }
    let mut learner = Learner::new(&fleet(), machine()).unwrap();
    let start = Instant::now();
    learner
        .window(&io, vms, shards, res(16_000, 64 * GIB), start)
        .unwrap();
    let vm = |id: &str, file: &'static str, value: &str| (fleet().join(id), file, value.to_owned());
    assert_eq!(
        io.take(),
        vec![
            vm("vm-1", "cpu.weight", "10000"),
            vm("vm-1", "memory.high", &(8 * GIB).to_string()),
            vm("vm-2", "cpu.weight", "10000"),
            vm("vm-2", "memory.high", &(8 * GIB).to_string()),
            (fleet(), "cpu.weight", "5000".to_owned()),
            (fleet(), "cpu.max", "1600000 100000".to_owned()),
            (root().join(".vm"), "cpu.weight", "7500".to_owned()),
        ]
    );
    learner
        .window(
            &io,
            running(&[("vm-1", 1), ("vm-2", 2)]),
            shards,
            res(16_000, 64 * GIB),
            start + WINDOW,
        )
        .unwrap();
    assert_eq!(io.take(), Vec::new(), "nothing changed, nothing written");
}

/// Joint plan step 14 (AC-T7.4.1), C7: a Shards guest's room can't be told yet. Do run a window of a Shards VM and a
/// Firecracker VM; expect the Shards guest's room refused as awaiting `GuestRoom`, the Firecracker one's not.
#[test]
fn w14_a_shards_guests_room_awaits_guest_room() {
    let io = Record::default();
    let vms = running(&[("vm-1", 1), ("vm-2", 2)]);
    for (_, _, cgroup) in &vms {
        io.set(cgroup.dir(), 0, GIB);
    }
    let mut learner = Learner::new(&fleet(), machine()).unwrap();
    let record = |id: &BoxId| -> Result<Option<(Class, Allocation)>, Refusal> {
        let class = if id.as_str() == "vm-1" {
            Class::Shards
        } else {
            Class::Firecracker
        };
        Ok(Some((class, whole())))
    };
    learner
        .window(&io, vms, record, machine(), Instant::now())
        .unwrap();
    let vm1 = BoxId::new("vm-1").unwrap();
    assert_eq!(learner.awaits(), vec![(&vm1, Upstream::GuestRoom)]);
}

/// Joint plan step 14 (AC-T7.4.1), C2 and C8 in the node: what a capacity answer carries. Do warm one VM, booked at
/// its whole receipt, at a tenth of a CPU and 1 GiB for SEEDABLE windows, and admit a second at the class's prior
/// that then holds 2 GiB; then end the second; then fail a window. Expect the prior its learned tail; the second
/// VM's holding first, past its receipt by a GiB; the use summing receipts and holdings; the ended VM gone from
/// them; and nothing learned carried after the failed window, until a window passes.
#[test]
fn w14_a_capacity_answer_carries_priors_use_and_holdings_most_past_first() {
    let io = Record::default();
    let first = running(&[("vm-1", 1)]);
    let dir = first[0].2.dir().to_path_buf();
    let mut learner = Learner::new(&fleet(), machine()).unwrap();
    let start = Instant::now();
    for t in 0..=u64::try_from(SEEDABLE).unwrap() {
        io.set(&dir, t * 10_000, GIB);
        let at = start + WINDOW * u32::try_from(t).unwrap();
        learner
            .window(&io, running(&[("vm-1", 1)]), shards, machine(), at)
            .unwrap();
    }
    let learned = learner.learned(Class::Shards, &[cost()]).unwrap().unwrap();
    let prior = res(100, GIB);
    assert_eq!(
        learned.priors,
        vec![LearnedPrior {
            profile: [14; 32],
            ceiling: res(16_000, 8 * GIB),
            booking: prior,
        }]
    );
    let booked = whole().booked_at(prior);
    assert_eq!(booked.booking(), prior);
    let both = running(&[("vm-1", 1), ("vm-2", 2)]);
    io.set(both[1].2.dir(), 0, 2 * GIB);
    let record = move |id: &BoxId| -> Result<Option<(Class, Allocation)>, Refusal> {
        let allocation = if id.as_str() == "vm-2" {
            booked
        } else {
            whole()
        };
        Ok(Some((Class::Shards, allocation)))
    };
    let later = start + WINDOW * 2_000;
    learner.window(&io, both, record, machine(), later).unwrap();
    learner
        .window(
            &io,
            running(&[("vm-1", 1), ("vm-2", 2)]),
            record,
            machine(),
            later + WINDOW,
        )
        .unwrap();
    let learned = learner.learned(Class::Shards, &[cost()]).unwrap().unwrap();
    let ids: Vec<&str> = learned.holdings.iter().map(|h| h.id.as_str()).collect();
    assert_eq!(ids, ["vm-2", "vm-1"], "most past its receipt first");
    assert_eq!(learned.holdings[0].holding.mem_bytes, 2 * GIB);
    assert_eq!(
        learned.used,
        LearnedUse {
            booked: res(16_000 + 100, 8 * GIB + GIB),
            reservation: res(100 + 100, GIB + 2 * GIB),
        }
    );
    learner
        .window(
            &io,
            running(&[("vm-1", 1)]),
            record,
            machine(),
            later + WINDOW * 2,
        )
        .unwrap();
    let learned = learner.learned(Class::Shards, &[cost()]).unwrap().unwrap();
    assert_eq!(learned.holdings.len(), 1);
    assert_eq!(learned.used.booked, res(16_000, 8 * GIB));
    learner.failed(Refusal::Broken("a read failed".into()));
    assert_eq!(learner.learned(Class::Shards, &[cost()]).unwrap(), None);
    learner
        .window(
            &io,
            running(&[("vm-1", 1)]),
            record,
            machine(),
            later + WINDOW * 3,
        )
        .unwrap();
    assert!(learner.learned(Class::Shards, &[cost()]).unwrap().is_some());
}
