//! The learned charge's core (joint plan step 14, AC-T7.4.1; `docs/design/learned-charge.md`): each test does X and
//! expects Y, and pins one number's basis.

use super::*;
use proptest::prelude::*;

const WINDOW: Duration = Duration::from_millis(100);
const GIB: u64 = 1 << 30;

fn res(cpu_milli: u64, mem_bytes: u64, disk_bytes: u64) -> Amounts {
    Amounts {
        cpu_milli,
        mem_bytes,
        disk_bytes,
    }
}

fn class_of(n: u8, cpu: u64, mem: u64) -> ClassKey {
    ClassKey::new(Class::Shards, [n; 32], res(cpu, mem, 0))
}

fn node(budget: Amounts) -> Node<u32> {
    Node::new(
        WINDOW,
        budget,
        Bounds {
            vms: 64,
            classes: 64,
        },
    )
    .unwrap()
}

/// One window's reading: `cpu_milli` of the window's CPU, `mem` held at most.
fn sample(cpu_milli: u64, mem: u64) -> Sample {
    Sample {
        cpu: Cpu::Whole(Duration::from_nanos(cpu_milli * 100_000)),
        span: WINDOW,
        mem_bytes: mem,
        disk_bytes: 0,
    }
}

fn window(node: &mut Node<u32>, readings: &[(u32, u64, u64)]) {
    for &(id, cpu, mem) in readings {
        node.observe(id, &sample(cpu, mem)).unwrap();
    }
    node.settle().unwrap();
}

/// waits_census.py's nearest rank of the p99.9, `ceil(0.999 n)`, in integers: `ceil(999 n / 1000)`. For every n up
/// to SEEDABLE the float product is within an ulp of the exact one, which is whole only at n = 1000 (999), so the two
/// agree.
fn rank(n: usize) -> usize {
    (999 * n).div_ceil(1000)
}

/// The definition: the nearest-rank p99.9 of the last SEEDABLE of `values`, as waits_census.py's `quantile` has it.
fn nearest_rank(values: &[u64]) -> u64 {
    let last = &values[values.len().saturating_sub(SEEDABLE)..];
    let mut sorted = last.to_vec();
    sorted.sort_unstable();
    sorted[rank(sorted.len()).max(1) - 1]
}

/// The same by a scan: the `n / SEEDABLE + 1`-th largest of the last SEEDABLE, equal values counted apart.
fn kth_largest(values: &[u64]) -> u64 {
    let last = &values[values.len().saturating_sub(SEEDABLE)..];
    let k = last.len() / SEEDABLE + 1;
    let first = *last.iter().max().unwrap();
    if k == 1 || last.iter().filter(|v| **v == first).count() >= 2 {
        first
    } else {
        *last.iter().filter(|v| **v < first).max().unwrap()
    }
}

/// Joint plan step 14 (AC-T7.4.1): SEEDABLE is decision 9's census rule. Do read waits_census.py's SEEDABLE;
/// expect this core's, and expect its claim: with SEEDABLE readings the nearest-rank p99.9 is the second largest,
/// an observed sample, and with one fewer it is the largest.
#[test]
fn w14_seedable_is_the_waits_census_rule() {
    let census = include_str!("../../../../bench/competitive/probes/waits_census.py");
    let seedable: usize = census
        .lines()
        .find_map(|line| line.strip_prefix("SEEDABLE = "))
        .map(|n| n.trim().replace('_', "").parse().unwrap())
        .unwrap();
    assert_eq!(seedable, SEEDABLE);
    assert_eq!(rank(SEEDABLE), SEEDABLE - 1);
    assert_eq!(rank(SEEDABLE - 1), SEEDABLE - 1);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// Joint plan step 14 (AC-T7.4.1), C1's tail: do push any readings into a tail, one dimension's of a few values
    /// so equal readings come in runs; expect each dimension's p99.9 after every push to be the `n / SEEDABLE +
    /// 1`-th largest of its last SEEDABLE readings, and that, at every 97th push and the last, the nearest-rank
    /// p99.9 as waits_census.py's `quantile` defines it.
    #[test]
    fn w14_the_tail_is_the_nearest_rank_p999_of_the_last_seedable_readings(
        readings in proptest::collection::vec((0u64..2_000, 0u64..1 << 20, 0u64..4), 1..2_600)
    ) {
        let mut tail = Tail::new().unwrap();
        let mut seen: [Vec<u64>; 3] = Default::default();
        let count = readings.len();
        for (at, (cpu, mem, disk)) in readings.into_iter().enumerate() {
            tail.push([cpu, mem, disk]).unwrap();
            seen[0].push(cpu);
            seen[1].push(mem);
            seen[2].push(disk);
            let p = tail.p999().unwrap();
            for d in 0..3 {
                prop_assert_eq!(p[d], kth_largest(&seen[d]), "dimension {} after {} readings", d, at + 1);
                if at % 97 == 0 || at + 1 == count {
                    prop_assert_eq!(p[d], nearest_rank(&seen[d]));
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Joint plan step 14 (AC-T7.4.1), C4's guarantee: do share a capacity no smaller than the weights' sum among
    /// any weights and needs; expect each to be given at least the lesser of its need and its weight and never
    /// more than its need, and the whole capacity or every need given (work-conserving).
    #[test]
    fn w14_every_share_is_at_least_its_need_or_its_weight(
        entries in proptest::collection::vec((1u64..10_000, 0u64..100_000), 1..48),
        spare in 0u64..50_000,
    ) {
        let weights: u64 = entries.iter().map(|(w, _)| w).sum();
        let capacity = weights + spare;
        let given = shares(capacity, &entries).unwrap();
        for ((weight, need), got) in entries.iter().zip(&given) {
            prop_assert!(*got >= (*need).min(*weight), "{got} < min({need}, {weight})");
            prop_assert!(got <= need);
        }
        let needs: u64 = entries.iter().map(|(_, n)| n).sum();
        prop_assert_eq!(given.iter().sum::<u64>(), capacity.min(needs));
    }

    /// Joint plan step 14 (AC-T7.4.1), C3: do run warmed VMs over windows of any use; expect after every window
    /// the bookings within the budget, each at most its charge, and one short of its charge only where the budget
    /// (or, for memory, what VMs hold) left no room.
    #[test]
    fn w14_bookings_stay_within_the_budget_and_rise_only_into_room(
        uses in proptest::collection::vec(proptest::collection::vec((0u64..4_000, 0u64..8 * GIB), 4), 8),
    ) {
        let budget = res(8_000, 16 * GIB, 0);
        let mut n = node(budget);
        for id in 0..4u32 {
            prop_assert_eq!(n.admit(id, class_of(1, 2_000, 4 * GIB), 0, 2).unwrap(), res(2_000, 4 * GIB, 0));
        }
        let ids: Vec<u32> = (0..4).collect();
        let mut rounds = 0;
        for round in &uses {
            for _ in 0..(SEEDABLE / 4) {
                let readings: Vec<(u32, u64, u64)> = ids.iter().zip(round).map(|(id, (c, m))| (*id, *c, *m)).collect();
                window(&mut n, &readings);
                rounds += 1;
                let booked = ids.iter().map(|id| n.booking(id).unwrap()).fold(res(0, 0, 0), |a, b| a.checked_add(b).unwrap());
                prop_assert!(budget.holds(booked), "{booked:?} past {budget:?} after {rounds} windows");
                for id in &ids {
                    let (booking, charge) = (n.booking(id).unwrap(), n.charge(id).unwrap());
                    prop_assert!(charge.holds(booking), "{id}: booking {booking:?} past charge {charge:?}");
                }
                let room = n.room().unwrap();
                for (id, excess) in n.over() {
                    prop_assert!(
                        (excess.cpu_milli == 0 || room.cpu_milli == 0)
                            && (excess.mem_bytes == 0 || room.mem_bytes == 0),
                        "{id} over by {excess:?} with room {room:?}"
                    );
                }
            }
        }
    }
}

/// Joint plan step 14 (AC-T7.4.1), C1's cold rule: do admit a VM with no prior and read it at a tenth of a core;
/// expect it charged its allocation through its first SEEDABLE - 1 windows, then its own tail's p99.9, and its
/// booking down to it at once.
#[test]
fn w14_a_cold_vm_is_charged_its_booking_until_its_tail_is_full() {
    let mut n = node(res(4_000, 8 * GIB, 0));
    let class = class_of(1, 4_000, 4 * GIB);
    assert_eq!(n.admit(1, class, 0, 4).unwrap(), class.ceiling());
    for _ in 0..SEEDABLE - 1 {
        window(&mut n, &[(1, 100, GIB)]);
    }
    assert_eq!(n.warmed(&1), Some(false));
    assert_eq!(n.charge(&1).unwrap(), class.ceiling());
    window(&mut n, &[(1, 100, GIB)]);
    assert_eq!(n.warmed(&1), Some(true));
    assert_eq!(n.charge(&1).unwrap(), res(100, GIB, 0));
    assert_eq!(n.booking(&1).unwrap(), res(100, GIB, 0));
}

/// Joint plan step 14 (AC-T7.4.1), C1's ceiling (R03, learning never raises authority): do read a warmed VM past its
/// allocation in CPU and memory; expect its charge to stop at the allocation.
#[test]
fn w14_the_charge_never_passes_the_allocation() {
    let mut n = node(res(16_000, 64 * GIB, 0));
    let class = class_of(1, 2_000, 4 * GIB);
    n.admit(1, class, 0, 2).unwrap();
    for _ in 0..SEEDABLE {
        window(&mut n, &[(1, 5_000, 9 * GIB)]);
    }
    assert_eq!(n.charge(&1).unwrap(), class.ceiling());
}

/// Joint plan step 14 (AC-T7.4.1), C1's window: do read CPU over twice the window and over half of it; expect the
/// first charged over its span and the second over the whole window (never less), each rounded up.
#[test]
fn w14_a_reading_is_charged_over_its_span_never_less_than_the_window() {
    let mut n = node(res(16_000, 64 * GIB, 0));
    n.admit(1, class_of(1, 16_000, 4 * GIB), 0, 16).unwrap();
    let read = |n: &mut Node<u32>, cpu: Duration, span: Duration| {
        n.observe(
            1,
            &Sample {
                cpu: Cpu::Whole(cpu),
                span,
                mem_bytes: 0,
                disk_bytes: 0,
            },
        )
        .unwrap();
        n.vms[&1].held[0]
    };
    assert_eq!(read(&mut n, WINDOW, WINDOW * 2), 500);
    assert_eq!(read(&mut n, WINDOW / 2, WINDOW / 2), 500);
    assert_eq!(read(&mut n, Duration::from_nanos(1), WINDOW), 1);
    assert_eq!(read(&mut n, WINDOW * 3, WINDOW), 3_000);
}

/// Joint plan step 14 (AC-T7.4.1), C1's sample: do ask a whole reading for its vCPUs; expect the typed upstream
/// absence. Do read a split naming another vCPU count; expect it refused. Do read a split of two vCPUs and the
/// VMM's own; expect the charge their sum.
#[test]
fn w14_per_vcpu_guest_time_awaits_upstream() {
    assert_eq!(
        Cpu::Whole(WINDOW).per_vcpu(),
        Err(ChargeError::Awaits(Upstream::GuestCpuTimePerVcpu))
    );
    let mut n = node(res(16_000, 64 * GIB, 0));
    n.admit(1, class_of(1, 2_000, 4 * GIB), 0, 2).unwrap();
    let split = |guest: Vec<Duration>| Sample {
        cpu: Cpu::Split {
            guest,
            own: WINDOW / 10,
        },
        span: WINDOW,
        mem_bytes: 0,
        disk_bytes: 0,
    };
    assert!(matches!(
        n.observe(1, &split(vec![WINDOW])),
        Err(ChargeError::Invalid { .. })
    ));
    n.observe(1, &split(vec![WINDOW, WINDOW / 2])).unwrap();
    assert_eq!(n.vms[&1].held[0], 1_600);
    assert_eq!(
        split(vec![WINDOW, WINDOW / 2]).cpu.per_vcpu().unwrap(),
        &[WINDOW, WINDOW / 2]
    );
}

/// Joint plan step 14 (AC-T7.4.1), C2: do admit a VM of a class no VM has warmed; expect its allocation. Do warm one
/// whose windows are low but for two high; expect the class's prior the high one (the p99.9 of its last SEEDABLE
/// warmed windows) and a second VM booked at it. Do read SEEDABLE more low windows; expect the prior to fall to the
/// low, as the outlier's windows leave the sample.
#[test]
fn w14_the_prior_is_the_p999_of_the_class_last_warmed_windows_and_falls() {
    let mut n = node(res(16_000, 64 * GIB, 0));
    let class = class_of(1, 8_000, 8 * GIB);
    assert_eq!(n.prior(&class), class.ceiling());
    n.admit(1, class, 0, 8).unwrap();
    for t in 0..SEEDABLE {
        let cpu = if t == 10 || t == 20 { 3_000 } else { 200 };
        window(&mut n, &[(1, cpu, GIB)]);
    }
    assert_eq!(n.prior(&class), res(3_000, GIB, 0));
    assert_eq!(n.admit(2, class, 0, 8).unwrap(), res(3_000, GIB, 0));
    n.end(&2).unwrap();
    for _ in 0..SEEDABLE {
        window(&mut n, &[(1, 200, GIB)]);
    }
    assert_eq!(n.prior(&class), res(200, GIB, 0));
}

/// Joint plan step 14 (AC-T7.4.1), C6: do book a VM at a whole node's CPU and admit another; expect it refused,
/// typed, with its booking and the room (none). Do let a VM hold memory past its booking; expect the room and the
/// reservation to count what it holds.
#[test]
fn w14_admission_refuses_with_the_room_and_counts_memory_held_past_bookings() {
    let mut n = node(res(4_000, 16 * GIB, 0));
    let class = class_of(1, 4_000, 4 * GIB);
    n.admit(1, class, 0, 4).unwrap();
    assert_eq!(
        n.admit(2, class, 0, 4),
        Err(ChargeError::NoRoom {
            booking: class.ceiling(),
            room: res(0, 12 * GIB, 0),
        })
    );
    n.observe(1, &sample(100, 6 * GIB)).unwrap();
    assert_eq!(n.room().unwrap(), res(0, 10 * GIB, 0));
    assert_eq!(n.reservation().unwrap(), res(4_000, 6 * GIB, 0));
}

/// Joint plan step 14 (AC-T7.4.1), C4's weights: do weigh bookings of 4,000, 2,000, 1 and 0 milli-CPU; expect the
/// largest at the kernel's top weight (cgroup-v2.rst: 1 to 10,000), the rest its share rounded up, at least 1.
#[test]
fn w14_weights_put_the_largest_booking_at_the_kernels_top() {
    let mut n = node(res(16_000, 64 * GIB, 0));
    for (id, cpu) in [(1, 4_000), (2, 2_000), (3, 1), (4, 0)] {
        n.admit(id, class_of(u8::try_from(id).unwrap(), cpu, GIB), 0, 4)
            .unwrap();
    }
    assert_eq!(
        n.weights().unwrap(),
        vec![(1, WEIGHT_MAX), (2, 5_000), (3, 3), (4, WEIGHT_MIN)]
    );
}

/// Joint plan step 14 (AC-T7.4.1), C5's budgets: do book VMs at 1, 2 and 3 GiB of a 10 GiB budget, one holding
/// 6 GiB; expect budgets summing to the budget exactly, each its booking and its share of the slack in
/// proportion, and the room its budget less what it holds (negative for the one past it).
#[test]
fn w14_budgets_sum_to_the_fleet_budget_and_cover_every_booking() {
    let mut n = node(res(16_000, 10 * GIB, 0));
    for (id, mem) in [(1, GIB), (2, 2 * GIB), (3, 3 * GIB)] {
        n.admit(id, class_of(u8::try_from(id).unwrap(), 1_000, mem), 0, 1)
            .unwrap();
    }
    n.observe(3, &sample(0, 6 * GIB)).unwrap();
    let budgets = n.budgets().unwrap();
    assert_eq!(
        budgets.iter().map(|(_, b)| b.bytes).sum::<u64>(),
        10 * GIB
    );
    let slack = 4 * GIB;
    for (id, budget) in &budgets {
        let booking = n.booking(id).unwrap().mem_bytes;
        let share = slack * booking / (6 * GIB);
        assert!(budget.bytes >= booking + share && budget.bytes <= booking + share + 1);
        let held = n.vms[id].held[1];
        assert_eq!(
            budget.room,
            i64::try_from(budget.bytes).unwrap() - i64::try_from(held).unwrap()
        );
    }
    assert!(budgets[2].1.room < 0);
}

/// Joint plan step 14 (AC-T7.4.1), C5's last resort: do book VMs at 2 GiB (band 1), 2 and 3 GiB (band 0) of a 10 GiB
/// budget and let the first hold 8 GiB and the second 3, each past its budget (its booking and its share of the
/// slack), the third within its; expect the band-0 VM past its budget ended first, then the band-1 one while the
/// rest still pass the fleet's budget, then none, and never the VM within its budget. With every VM within its
/// budget but the growth on record past the fleet's, expect none: growth is no fact yet.
#[test]
fn w14_the_last_resort_ends_only_a_vm_past_its_budget() {
    let mut n = node(res(16_000, 10 * GIB, 0));
    for (id, band, mem) in [(1, 1, 2 * GIB), (2, 0, 2 * GIB), (3, 0, 3 * GIB)] {
        n.admit(id, class_of(u8::try_from(id).unwrap(), 1_000, mem), band, 1)
            .unwrap();
    }
    for (id, held) in [(1, 8 * GIB), (2, 3 * GIB), (3, 3 * GIB)] {
        n.observe(id, &sample(0, held)).unwrap();
    }
    assert_eq!(n.last_resort().unwrap(), Some(2));
    n.end(&2).unwrap();
    assert_eq!(n.last_resort().unwrap(), Some(1));
    n.end(&1).unwrap();
    assert_eq!(n.last_resort().unwrap(), None);

    let mut within = node(res(16_000, 10 * GIB, 0));
    for id in 1..=2 {
        within
            .admit(id, class_of(u8::try_from(id).unwrap(), 1_000, 5 * GIB), 0, 1)
            .unwrap();
    }
    window(&mut within, &[(1, 0, 5 * GIB), (2, 0, 5 * GIB)]);
    assert_eq!(within.growth(), 10 * GIB);
    assert_eq!(within.last_resort().unwrap(), None);
}

/// Joint plan step 14 (AC-T7.4.1), C5's growth: do grow two VMs' memory by 1 and 2 GiB in one window and by 1 GiB
/// in the next; expect the node's fastest growth on record 3 GiB, and it never falls.
#[test]
fn w14_the_fastest_growth_is_learned_from_the_windows() {
    let mut n = node(res(16_000, 64 * GIB, 0));
    n.admit(1, class_of(1, 1_000, 8 * GIB), 0, 1).unwrap();
    n.admit(2, class_of(2, 1_000, 8 * GIB), 0, 1).unwrap();
    window(&mut n, &[(1, 0, GIB), (2, 0, 2 * GIB)]);
    assert_eq!(n.growth(), 3 * GIB);
    window(&mut n, &[(1, 0, 2 * GIB), (2, 0, 2 * GIB)]);
    assert_eq!(n.growth(), 3 * GIB);
}

/// Joint plan step 14 (AC-T7.4.1), the classes' bound (CLAUDE.md §2): do hold two classes at a bound of two, end
/// the first's VM and admit a third class; expect the first forgotten (its prior the allocation again), the
/// second kept. With both held by live VMs, expect a third refused, typed.
#[test]
fn w14_classes_past_their_bound_forget_the_oldest_without_a_live_vm() {
    let mut n: Node<u32> = Node::new(
        WINDOW,
        res(16_000, 64 * GIB, 0),
        Bounds { vms: 8, classes: 2 },
    )
    .unwrap();
    let (a, b, c) = (
        class_of(1, 1_000, GIB),
        class_of(2, 1_000, GIB),
        class_of(3, 1_000, GIB),
    );
    n.admit(1, a, 0, 1).unwrap();
    n.admit(2, b, 0, 1).unwrap();
    for _ in 0..SEEDABLE {
        window(&mut n, &[(1, 10, GIB / 2), (2, 10, GIB / 2)]);
    }
    assert_eq!(n.prior(&a), res(10, GIB / 2, 0));
    assert!(matches!(
        n.admit(3, c, 0, 1),
        Err(ChargeError::Full { .. })
    ));
    n.end(&1).unwrap();
    n.admit(3, c, 0, 1).unwrap();
    assert_eq!(n.prior(&a), a.ceiling());
    assert_eq!(n.prior(&b), res(10, GIB / 2, 0));
}

/// Joint plan step 14 (AC-T7.4.1), C4 against a planted bug: do warm a VM of three CPUs' allocation down to a booking
/// of one, book two more at one each, and share 3,000 milli-CPU while the first asks for all of it; expect each
/// neighbour its 1,000. Planted, CPU shared by what each asks: expect a neighbour given less than its booking, which
/// the guarantee catches.
#[test]
fn w14_the_guarantee_catches_cpu_shared_by_demand() {
    let mut n = node(res(3_000, 64 * GIB, 0));
    n.admit(1, class_of(1, 3_000, GIB), 0, 3).unwrap();
    for _ in 0..SEEDABLE {
        window(&mut n, &[(1, 1_000, GIB)]);
    }
    assert_eq!(n.booking(&1).unwrap().cpu_milli, 1_000);
    for id in 2..=3 {
        n.admit(id, class_of(2, 1_000, GIB), 0, 1).unwrap();
    }
    let demands = BTreeMap::from([(1, 3_000), (2, 1_000), (3, 1_000)]);
    let held = |given: &[(u32, u64)]| given.iter().all(|(id, got)| *id == 1 || *got >= 1_000);
    assert!(held(&n.shares(3_000, &demands).unwrap()));
    let mutated = with_mutant(Some(Mutant::WeightsByDemand), || {
        n.shares(3_000, &demands).unwrap()
    });
    assert!(!held(&mutated), "{mutated:?}");
}

/// Joint plan step 14 (AC-T7.4.1), C3 against a planted bug: do warm two VMs whose charges together pass the
/// budget; expect the bookings within it. Planted, bookings raised whatever the room: expect them past it.
#[test]
fn w14_bookings_past_the_room_are_caught() {
    let run = || {
        let mut n = node(res(4_000, 64 * GIB, 0));
        for id in 1..=2 {
            n.admit(id, class_of(1, 4_000, GIB), 0, 4).unwrap_or_default();
        }
        for _ in 0..SEEDABLE {
            window(&mut n, &[(1, 100, GIB / 4)]);
        }
        n.admit(2, class_of(1, 4_000, GIB), 0, 4).unwrap();
        for _ in 0..SEEDABLE {
            window(&mut n, &[(1, 3_000, GIB / 4), (2, 3_000, GIB / 4)]);
        }
        n.booking(&1).unwrap().cpu_milli + n.booking(&2).unwrap().cpu_milli
    };
    assert!(run() <= 4_000);
    assert!(with_mutant(Some(Mutant::BookPastRoom), run) > 4_000);
}

/// Joint plan step 14 (AC-T7.4.1), C5 against a planted bug: do let a VM booked at 1 GiB hold 4 GiB and one booked at
/// 6 GiB hold its booking, past a 10 GiB budget; expect the first ended, never the second, within its booking.
/// Planted, the largest holder ended: expect the second chosen, which the rule refuses.
#[test]
fn w14_ending_a_vm_within_its_booking_is_caught() {
    let mut n = node(res(16_000, 10 * GIB, 0));
    for (id, mem) in [(1, GIB), (2, 6 * GIB)] {
        n.admit(id, class_of(u8::try_from(id).unwrap(), 1_000, mem), 0, 1)
            .unwrap();
    }
    for (id, held) in [(1, 4 * GIB + 1), (2, 6 * GIB)] {
        n.observe(id, &sample(0, held)).unwrap();
    }
    assert_eq!(n.last_resort().unwrap(), Some(1));
    let mutated = with_mutant(Some(Mutant::EndLargestHolder), || n.last_resort().unwrap());
    assert_eq!(mutated, Some(2));
}
