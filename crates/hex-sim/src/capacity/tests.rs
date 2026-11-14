//! The capacity view under the nemesis (joint plan step 13, `docs/design/capacity-view.md`; AC-T7.4.1).
//! A bounded set of seeds here; ROOT runs the sweeps (`run` over more seeds and runners).

use super::*;

fn total(all: Report, r: Report) -> Report {
    Report {
        steps: all.steps + r.steps,
        hex_started: all.hex_started + r.hex_started,
        hex_refused: all.hex_refused + r.hex_refused,
        runs_granted: all.runs_granted + r.runs_granted,
        runs_refused: all.runs_refused + r.runs_refused,
        runs_unanswered: all.runs_unanswered + r.runs_unanswered,
        link_drops: all.link_drops + r.link_drops,
        keeper_crashes: all.keeper_crashes + r.keeper_crashes,
        crashes_at_write: all.crashes_at_write + r.crashes_at_write,
        runner_crashes: all.runner_crashes + r.runner_crashes,
        released_at_sync: all.released_at_sync + r.released_at_sync,
        adopted_at_sync: all.adopted_at_sync + r.adopted_at_sync,
        forgotten: all.forgotten + r.forgotten,
        wrongly_forgotten: all.wrongly_forgotten + r.wrongly_forgotten,
        samples: all.samples + r.samples,
        moved_in_window: all.moved_in_window + r.moved_in_window,
    }
}

/// The runner counts the suite runs (ROOT, 2026-11-14): one, as the recorded fleet configures each machine's keeper
/// (one kiln runner a machine); and two, the fewest a retirement needs, since it takes a live runner and never the last
/// ([`World::retire`]): a structural edge, no number picked.
const RECORDED: usize = 1;
const RETIRING: usize = 2;

/// Whether the runners' runs outlive their processes: kiln states it for neither, so each is its own case (ROOT,
/// 2026-11-14), never a coin.
const SURVIVE: [bool; 2] = [true, false];

fn seeds(runners: usize, seeds: u64, steps: u64) -> Report {
    SURVIVE.iter().fold(Report::default(), |all, survive| {
        (0..seeds).fold(all, |all, seed| {
            let r = run(seed, runners, steps, *survive, None).unwrap_or_else(|v| {
                panic!("{runners} runners, survive {survive}, seed {seed}: {v:?}")
            });
            total(all, r)
        })
    })
}

/// AC-T7.4.1, joint plan step 13: do run one machine, Hex's own boxes beside its one kiln runner on its keeper, as
/// recorded, its runs outliving its process in one case and ending with it in the other, for 64 seeds of 2,000 steps
/// each under the nemesis (the keeper crashing between messages and at its writes, before and after they land; the
/// runner crashing; links dropped with what was on them, partitioned and healed; waits run out; runs' use moving at
/// any step, a sample's window among them); every charge a recorded one, kiln's legs' and Hex's oci boxes'. Expect the
/// work running never past the room, every running run in the keeper's books and the keeper deciding in no more room
/// than its host measured at every step, and once settled the keeper's books, the runner's and what it runs the same;
/// and (non-vacuity) each fault struck, both sides refused for room, syncs released what ended, a sync adopted work the
/// person had released while its runner was away (CV6), and samples saw use move in their windows.
#[test]
fn no_oversubscription_and_the_books_agree_across_crash_seeds() {
    let all = seeds(RECORDED, 64, 2_000);
    eprintln!("{RECORDED} runner, 64 seeds of 2,000 steps each way: {all:?}");
    assert!(
        all.hex_started > 0
            && all.hex_refused > 0
            && all.runs_granted > 0
            && all.runs_refused > 0
            && all.runs_unanswered > 0
            && all.link_drops > 0
            && all.keeper_crashes > 0
            && all.crashes_at_write > 0
            && all.runner_crashes > 0
            && all.released_at_sync > 0
            && all.wrongly_forgotten > 0
            && all.adopted_at_sync > 0
            && all.samples > 0
            && all.moved_in_window > 0,
        "{all:?}"
    );
}

/// AC-T7.4.1, joint plan step 13 (CV6): do the same with two runners, the fewest a retirement needs ([`RETIRING`]), 32
/// seeds each way; expect the same, and a runner retired and its reservations released by the person.
#[test]
fn a_runner_retired_beside_another_keeps_the_same() {
    let all = seeds(RETIRING, 32, 2_000);
    assert!(
        all.runs_granted > 0 && all.keeper_crashes > 0 && all.link_drops > 0 && all.forgotten > 0,
        "{RETIRING}: {all:?}"
    );
}

/// Runs 64 seeds each way with `mutant` planted, its runner's runs outliving it and not: the first violation.
fn caught(mutant: Mutant) -> Option<Violation> {
    let found = SURVIVE.iter().find_map(|survive| {
        (0..64).find_map(|seed| {
            run(seed, RECORDED, 2_000, *survive, Some(mutant))
                .err()
                .map(|v| (seed, *survive, v))
        })
    });
    eprintln!("{mutant:?}: {found:?}");
    found.map(|(_, _, v)| v)
}

/// Non-vacuity, CV5: do plant a keeper's shell that releases a runner's reservations when its link
/// drops; expect some seed to run a run the keeper's books don't hold.
#[test]
fn releasing_on_silence_is_caught() {
    let found = caught(Mutant::ReleaseOnSilence);
    assert!(
        matches!(
            found,
            Some(Violation::Unbooked { .. } | Violation::Oversubscribed { .. })
        ),
        "{found:?}"
    );
}

/// Non-vacuity, CV5 (a holder's first obligation): do plant a runner that starts a run when it asks;
/// expect some seed to run an unbooked run or past the room.
#[test]
fn starting_before_the_grant_is_caught() {
    let found = caught(Mutant::StartBeforeGrant);
    assert!(
        matches!(
            found,
            Some(Violation::Unbooked { .. } | Violation::Oversubscribed { .. })
        ),
        "{found:?}"
    );
}

/// Non-vacuity, CV5 (a holder's third obligation): do plant a runner whose next process lists none of
/// the runs that outlived it; expect some seed to run a run the keeper released.
#[test]
fn forgetting_survivors_is_caught() {
    let found = caught(Mutant::ForgetSurvivors);
    assert!(
        matches!(
            found,
            Some(Violation::Unbooked { .. } | Violation::Oversubscribed { .. })
        ),
        "{found:?}"
    );
}

/// Non-vacuity, CV2: do plant a keeper that answers before its write lands, its crash losing the write;
/// expect some seed to run a run the restarted keeper doesn't hold.
#[test]
fn answering_before_the_write_is_caught() {
    let found = caught(Mutant::AnswerBeforeWrite);
    assert!(found.is_some(), "no seed caught it");
}

/// Non-vacuity, CV3: do plant Hex admitting against the room less its own boxes alone; expect some seed
/// past the room.
#[test]
fn hex_ignoring_the_holders_is_caught() {
    let found = caught(Mutant::LocalIgnoresForeign);
    assert!(
        matches!(found, Some(Violation::Oversubscribed { .. })),
        "{found:?}"
    );
}

/// Non-vacuity, CV5: do plant a keeper's shell that drops from a sync what the keeper doesn't hold, which
/// its sync would adopt after the person's wrong release; expect some seed to run a run the keeper's books
/// lack once its runner has synced, or that runner told the counts disagree.
#[test]
fn dropping_what_the_keeper_lacks_at_a_sync_is_caught() {
    let found = caught(Mutant::DropUnknownAtSync);
    assert!(
        matches!(
            found,
            Some(Violation::Unbooked { .. } | Violation::SyncDisagreed { .. })
        ),
        "{found:?}"
    );
}

/// Non-vacuity, CV12 (withdrawn): do plant a host that credits the room each joined runner's work, the smaller of two
/// reads on either side of its read of free memory, up to its reservations (the withdrawn rule, for any credit); expect
/// some seed whose keeper decides in more room than its host measured.
#[test]
fn crediting_any_use_is_caught() {
    let found = caught(Mutant::Credit);
    assert!(
        matches!(
            found,
            Some(Violation::Overcredited { .. } | Violation::Oversubscribed { .. })
        ),
        "{found:?}"
    );
}

/// AC-T7.4.1, joint plan step 13 (CLAUDE.md §2: a loop's bound derived, never picked): do have one runner, synced, ask
/// for one run more than its reservation bound and let each one's wait run out before its answer, so two words each
/// wait on its link to the keeper, more than twice the bound, then settle; expect it to settle with the books agreeing:
/// settling is bounded by the protocol's own count of what its links carry, not by a multiple of a runner's
/// reservations.
#[test]
fn a_backlog_on_a_link_settles_within_the_protocols_own_bound() {
    let mut world = World::new(0, RECORDED, true, None).unwrap();
    world.connect(Some(0));
    // Its hello, the welcome, its sync and the answer.
    for _ in 0..4 {
        world.deliver_any().unwrap();
    }
    assert!(world.runners[0].holder.as_ref().unwrap().is_up());
    let asks = world.budget.reservations + 1;
    for _ in 0..asks {
        world.ask(Some(0));
        world.ran_out(Some(0)).unwrap();
    }
    assert_eq!(world.runners[0].link.to_keeper.len(), 2 * asks);
    let settled = world.settle();
    assert!(settled.is_ok(), "{settled:?}");
}

/// T-T7.4.1's determinism: one seed, one run.
#[test]
fn a_seed_names_one_run() {
    for survive in SURVIVE {
        assert_eq!(
            run(7, RECORDED, 2_000, survive, None),
            run(7, RECORDED, 2_000, survive, None)
        );
    }
}

/// A sweep's size from the environment, for the lead's record runs: `name`'s count, else the bounded suite's.
fn sized(name: &str, bounded: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(bounded)
}

/// AC-T7.4.1, joint plan step 13: the record sweep, the lead's, in release: `HEX_CAPACITY_SEEDS` seeds (the bounded
/// suite's 64 unless set) of `HEX_CAPACITY_STEPS` steps (2,000) each way ([`SURVIVE`]) with the recorded runner and
/// with retirement's two, then each planted mistake over the same seeds with the recorded runner, each way. Expect every
/// seed to hold, and each mistake caught, with how many seeds of how many caught it and the first.
#[test]
#[ignore = "the record sweep, sized by HEX_CAPACITY_SEEDS and HEX_CAPACITY_STEPS, in release (the lead's)"]
fn the_record_sweep() {
    let (n, steps) = (
        sized("HEX_CAPACITY_SEEDS", 64),
        sized("HEX_CAPACITY_STEPS", 2_000),
    );
    for runners in [RECORDED, RETIRING] {
        let all = seeds(runners, n, steps);
        eprintln!("{runners} runners, {n} seeds of {steps} steps each way: {all:?}");
    }
    for mutant in [
        Mutant::ReleaseOnSilence,
        Mutant::StartBeforeGrant,
        Mutant::ForgetSurvivors,
        Mutant::AnswerBeforeWrite,
        Mutant::LocalIgnoresForeign,
        Mutant::DropUnknownAtSync,
        Mutant::Credit,
    ] {
        for survive in SURVIVE {
            let caught: Vec<(u64, Option<u64>)> = (0..n)
                .filter_map(|seed| {
                    run(seed, RECORDED, steps, survive, Some(mutant))
                        .err()
                        .map(|v| (seed, step_of(&v)))
                })
                .collect();
            eprintln!(
                "SWEEP mutant={mutant:?} survive={survive} seeds={n} steps={steps} caught={} first_seed={:?} first_step={:?}",
                caught.len(),
                caught.first().map(|(seed, _)| seed),
                caught.first().and_then(|(_, step)| *step)
            );
        }
    }
}

/// The step a violation was found at, where it names one: settling's findings come once every step has run.
fn step_of(v: &Violation) -> Option<u64> {
    match v {
        Violation::Oversubscribed { step, .. }
        | Violation::Unreported { step, .. }
        | Violation::Unbooked { step, .. }
        | Violation::SyncDisagreed { step, .. }
        | Violation::Overcredited { step, .. } => Some(*step),
        Violation::Disagree { .. } | Violation::Stopped(_) | Violation::Unsettled => None,
    }
}

/// AC-T7.4.1, joint plan step 13 (H10; R03: a machine's room recorded, never picked): the machine under the nemesis is the
/// recorded fleet's (`bench/sched/huawei-r5-day30.fleet.trace`, from kiln's runner facts of 2026-11-05) smallest by memory
/// of the hosts where Hex's own boxes and kiln's runs share one room, those that run Hex's `oci` class: where the two
/// contend for the least room. Do read the fleet's node rows; expect the room to be that host's CPUs, memory and GPUs.
#[test]
fn the_machine_is_the_recorded_fleets_smallest_shared_host() {
    let fleet = crate::replay::parse(
        include_str!("../../../../bench/sched/huawei-r5-day30.fleet.trace"),
        &crate::replay::Limits::default(),
    )
    .unwrap();
    let shared = fleet
        .nodes
        .iter()
        .filter(|n| n.classes.has(crate::model::Class::Oci))
        .map(|n| n.capacity)
        .min_by_key(|c| (c.mem_bytes, c.cpu_milli))
        .unwrap();
    assert_eq!(
        (ROOM.cpu_milli, ROOM.mem_bytes, ROOM.gpus),
        (shared.cpu_milli, shared.mem_bytes, u64::from(shared.gpus))
    );
}

/// AC-T7.4.1, CV8 (R03: a bound derived, never picked): the keeper holds a holder's name to a bound, which on this machine
/// is the longest name its runners say in their hellos. Do make a world of the most runners the suite runs
/// ([`RETIRING`]); expect its keeper's name bound to be its longest runner's name.
#[test]
fn the_keepers_name_bound_is_its_runners_longest_name() {
    let world = World::new(0, RETIRING, true, None).unwrap();
    let longest = world.runners.iter().map(|r| r.name.len()).max().unwrap();
    assert_eq!(world.budget.name_bytes, longest);
}

/// AC-T7.4.1, joint plan step 13, CV8 (ROOT, 2026-11-14: the protocol's own bound wins): a runner's reservation bound is
/// as many runs of the smallest recorded charge as the room holds in its fewest dimension, but never more than one sync
/// carries. Do read the room and the smallest recorded charge; expect the bound the smaller of the two, here the sync's.
#[test]
fn a_runners_reservation_bound_is_the_protocols_own() {
    let smallest = run_charges().iter().copied().reduce(Amounts::min).unwrap();
    let fits = (ROOM.cpu_milli / smallest.cpu_milli).min(ROOM.mem_bytes / smallest.mem_bytes);
    let most = hex_core::capacity::most_held().unwrap();
    assert_eq!(
        reservations().unwrap(),
        usize::try_from(fits).unwrap().min(most)
    );
    assert_eq!(
        reservations().unwrap(),
        most,
        "the room holds fewer: {fits}"
    );
}

/// AC-T7.4.1, joint plan step 13 (ROOT, 2026-11-14: a leg below kiln's resolution is that unit, never 0, never dropped):
/// a recorded leg's need is measured to a resolution, a milli-core (the trace's unit) and a 4 KiB page, of which every
/// peak RSS kiln measured is a whole number. Do read kiln's export and every leg of the trace made from it; expect each
/// of the export's 3,750 measured peak RSSs whole pages, the trace's legs below the resolution counted, and every charge
/// the runners draw at least one unit in each dimension, one for each leg. (A leg kiln didn't measure takes its job's
/// median, between two measured ones for an even count: `kiln_workload.md`.) A charge of nothing is the keeper's own
/// edge case, refused Why::Empty (hex-api's capacity tests), which no recorded charge asks.
#[test]
fn a_recorded_legs_need_is_whole_units_of_its_resolution() {
    let export: serde_json::Value = serde_json::from_reader(flate2::read::GzDecoder::new(
        std::fs::File::open(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../bench/sched/kiln-ci-20261022-20261105.export.json.gz"
        ))
        .unwrap(),
    ))
    .unwrap();
    let measured: Vec<u64> = export["runs"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|run| run["jobs"].as_array().unwrap())
        .filter(|job| job["started_at"].is_u64() && job["finished_at"].is_u64())
        .filter_map(|job| job["peak_rss"].as_u64())
        .collect();
    assert_eq!(measured.len(), 3_750);
    assert!(measured.iter().all(|m| m % RESOLUTION.mem_bytes == 0));
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../bench/sched/kiln-ci-20261022-20261105.trace"
    ))
    .unwrap();
    let legs: Vec<Amounts> = text.lines().filter_map(need).collect();
    assert_eq!(legs.len(), 4_655);
    let below = |f: fn(&Amounts) -> u64| legs.iter().filter(|l| f(l) == 0).count();
    assert_eq!((below(|l| l.cpu_milli), below(|l| l.mem_bytes)), (631, 58));
    assert_eq!(run_charges().len(), legs.len());
    assert!(run_charges().iter().all(|c| RESOLUTION.fits(*c)));
}

/// AC-T7.4.1, joint plan step 13 (ROOT, 2026-11-14): Hex's own boxes are the recorded fleet's `oci` legs. Do read them;
/// expect each drawn as recorded, 0.3 to 3.4 cores and 128 MiB to 4 GiB.
#[test]
fn hex_boxes_are_the_recorded_fleets_oci_legs() {
    let boxes = box_charges();
    assert_eq!(boxes.len(), 8_432);
    let low = boxes.iter().copied().reduce(Amounts::min).unwrap();
    let high = boxes.iter().copied().reduce(Amounts::max).unwrap();
    assert_eq!((low.cpu_milli, low.mem_bytes), (300, 128 << 20));
    assert_eq!((high.cpu_milli, high.mem_bytes), (3_400, 4 << 30));
}
