//! Cells and regions (`docs/design/hex-sim.md` H14-H17): the differential suite on one node, one
//! cell and three regions, the cell-loss drill, and a lost region's work re-placed within the
//! others' capacity.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::disallowed_macros
    )
)]

use hex_sim::federation::{self, CellRun, FedConfig, FedRun, Loss, Moved, Topology};
use hex_sim::generate::{self, Shape};
use hex_sim::index::{FitIndex, Sample};
use hex_sim::model::{Cluster, Kind, WorkloadId};
use hex_sim::rng::Rng;
use hex_sim::sim::{self, Config, Nemesis};
use hex_sim::spec;
use hex_sim::store::State;

const SECOND: u64 = 1_000_000_000;

fn fed(topology: Topology, cell: Config, loss: Loss) -> FedConfig {
    FedConfig {
        topology,
        cell,
        loss,
        region_grace_ns: federation::REGION_GRACE_NS,
    }
}

/// What the specification fixes in every cell of a run (H17): its log one the serial specification
/// makes, nothing lost or started twice, the hard filters held.
fn check_cells(cells: &[CellRun], what: &str) {
    for c in cells {
        let at = format!("{what}, region {} cell {}", c.region, c.cell);
        spec::check_slices(&c.cluster, &c.run).unwrap_or_else(|v| panic!("{at}: {v}"));
        let end = spec::check_end(&c.cluster, &c.run);
        assert_eq!(end.lost_or_double(), 0, "{at}: {end:?}");
        let filters = spec::check_filters(&c.cluster, &c.run).unwrap();
        assert_eq!(filters.violations(), 0, "{at}: {filters:?}");
    }
}

fn suite_seed(seed: u64) -> (Cluster, Config) {
    let mut r = Rng::new(seed ^ 0xce11);
    let shape = Shape {
        nodes: u32::try_from(r.range(6, 30)).unwrap(),
        workloads: r.range(20, 200),
        mean_gap: r.range(0, 40_000),
        mean_run: if r.chance(150) {
            None
        } else {
            Some(r.range(1, 2_000_000))
        },
        gpu_nodes_per_mille: r.range(0, 500),
        max_share_per_mille: r.range(50, 1000),
    };
    let cluster = generate::cluster(seed, &shape).unwrap();
    let config = Config {
        workers: u32::try_from(r.range(1, 8)).unwrap(),
        decide_ns: r.range(0, 200_000),
        commit_ns: r.range(0, 100_000),
        change_window: usize::try_from(r.range(1, 64)).unwrap(),
        restart_ns: r.range(0, 5_000_000),
        ..Config::default()
    };
    (cluster, config)
}

fn crashing(config: &Config, seed: u64, workloads: u64) -> Config {
    let mut r = Rng::new(seed ^ 0xdead);
    Config {
        nemesis: Nemesis::Random {
            seed: r.next_u64(),
            first: r.range(0, workloads.max(2) - 1),
            per_mille: r.range(5, 200),
            max_crashes: u32::try_from(r.range(1, 8)).unwrap(),
        },
        ..*config
    }
}

/// AC-T7.12.3: do 300 generated seeds, each at one node, one cell and three regions of two cells,
/// clean and under the random crash nemesis; expect every cell of every run to keep what the
/// specification fixes (its log the serial specification's, lost-or-double 0, the filters held), one
/// cell through the federation to be `sim::run` exactly, and (non-vacuity) three regions to place
/// work in every region and the nemesis to crash.
#[test]
fn the_differential_suite_passes_on_one_node_one_cell_and_three_regions() {
    let (mut crashes, mut regions_placing, mut runs) = (0u64, 0u64, 0u64);
    for seed in 0..300 {
        let (cluster, base) = suite_seed(seed);
        let n = u64::try_from(cluster.workloads.len()).unwrap();
        for config in [base, crashing(&base, seed, n)] {
            let one_node = Cluster {
                nodes: cluster.nodes[..1].to_vec(),
                workloads: cluster.workloads.clone(),
            };
            let scales = [
                (
                    "one node",
                    &one_node,
                    Topology {
                        regions: 1,
                        cells: 1,
                    },
                ),
                (
                    "one cell",
                    &cluster,
                    Topology {
                        regions: 1,
                        cells: 1,
                    },
                ),
                (
                    "three regions",
                    &cluster,
                    Topology {
                        regions: 3,
                        cells: 2,
                    },
                ),
            ];
            for (what, c, topology) in scales {
                let what = format!("seed {seed}, {what}, {:?}", config.nemesis);
                let out = federation::run(c, &fed(topology, config, Loss::None))
                    .unwrap_or_else(|e| panic!("{what}: {e}"));
                check_cells(&out.cells, &what);
                let placed: u64 = out.cells.iter().map(|x| x.run.report.placements).sum();
                crashes += out.cells.iter().map(|x| x.run.report.crashes).sum::<u64>();
                runs += 1;
                if topology.cells == 1 {
                    // One cell is today's hex-sim, bit for bit (H14).
                    let alone = sim::run(c, &config).unwrap();
                    assert_eq!(out.cells.len(), 1);
                    assert_eq!(out.cells[0].run.report, alone.report, "{what}");
                    assert_eq!(out.cells[0].cluster, *c, "{what}");
                } else if placed > 0 {
                    let every = (0..3).all(|r| {
                        out.cells
                            .iter()
                            .any(|x| x.region == r && x.run.report.placements > 0)
                    });
                    regions_placing += u64::from(every);
                }
            }
        }
    }
    assert_eq!(runs, 1_800);
    assert!(crashes > 500, "{crashes} crashes");
    assert!(
        regions_placing > 300,
        "{regions_placing} three-region runs placed in every region"
    );
}

/// Work that never ends, all of it arriving in the first virtual second, onto `nodes` nodes, at a
/// tenth of a node's CPU and memory each at most (about 15 a node), so a cell's room is about 15 times
/// its nodes.
fn steady(seed: u64, nodes: u32, workloads: u64) -> Cluster {
    let shape = Shape {
        nodes,
        workloads,
        mean_gap: SECOND / workloads,
        mean_run: None,
        gpu_nodes_per_mille: 0,
        max_share_per_mille: 100,
    };
    let mut cluster = generate::cluster(seed, &shape).unwrap();
    for w in &mut cluster.workloads {
        if w.kind == Kind::Benchmark {
            w.kind = Kind::Plain;
        }
        w.arrives = w.arrives.min(SECOND - 1);
    }
    cluster
}

/// AC-T7.12.2: do the cell-loss drill: two regions of three cells, lose cell 1 of region 0's control
/// plane for good at 2 s; expect every other cell's run (report and trace) the same as without the
/// loss, so other cells' workloads disturbed is exactly 0, and the lost cell's running work kept
/// running; (non-vacuity) the lost cell had work running and waiting when it was lost.
#[test]
fn a_cells_control_plane_loss_disturbs_no_workload_in_another_cell() {
    let cluster = steady(11, 60, 2_000);
    let config = Config {
        workers: 4,
        ..Config::default()
    };
    let topology = Topology {
        regions: 2,
        cells: 3,
    };
    let lost = Loss::Cell {
        region: 0,
        cell: 1,
        at: 2 * SECOND,
    };
    let base = federation::run(&cluster, &fed(topology, config, Loss::None)).unwrap();
    let drill = federation::run(&cluster, &fed(topology, config, lost)).unwrap();
    assert_eq!(base.cells.len(), 6);
    assert_eq!(drill.cells.len(), 6);
    let (mut others, mut disturbed) = (0u64, 0u64);
    for (b, d) in base.cells.iter().zip(&drill.cells) {
        assert_eq!((b.region, b.cell), (d.region, d.cell));
        if (d.region, d.cell) == (0, 1) {
            let kept = spec::check_kept_running(&d.cluster, &d.run);
            assert_eq!(kept.disturbed, 0, "{kept:?}");
            let end = spec::check_end(&d.cluster, &d.run);
            assert!(end.running > 100, "{end:?}");
            assert!(end.waiting > 0, "{end:?}");
            assert_eq!(
                d.run.report.takeovers.len(),
                1,
                "{:?}",
                d.run.report.takeovers
            );
            assert!(
                d.run
                    .report
                    .takeovers
                    .iter()
                    .all(|t| t.takeover_ns.is_none())
            );
            continue;
        }
        let kept = spec::check_kept_running(&d.cluster, &d.run);
        others += kept.kept;
        disturbed += kept.disturbed;
        assert_eq!(
            b.run.report, d.run.report,
            "region {} cell {}",
            d.region, d.cell
        );
        assert_eq!(b.run.ran, d.run.ran, "region {} cell {}", d.region, d.cell);
    }
    check_cells(&drill.cells, "cell-loss drill");
    assert_eq!(disturbed, 0);
    assert!(others > 500, "{others} workloads ran in the other cells");
}

/// Every moved workload placed where the federation sent it, and each refused one held by no
/// surviving node at the end (a brute-force scan of every node, not the federation's own check).
fn check_moved(cluster: &Cluster, out: &FedRun, lost_region: u32) -> (u64, u64) {
    let (mut to, mut refused) = (0u64, 0u64);
    for &(id, moved) in &out.moved {
        match moved {
            Moved::To { region, cell } => {
                assert_ne!(region, lost_region);
                let c = out
                    .cells
                    .iter()
                    .find(|c| (c.region, c.cell) == (region, cell))
                    .unwrap();
                let local = c.ids.iter().position(|&g| g == id).unwrap();
                let state = c
                    .run
                    .store
                    .record(WorkloadId(u64::try_from(local).unwrap()))
                    .map(|r| r.state);
                assert!(
                    matches!(state, Some(State::Placed(_))),
                    "{id:?} moved to region {region} cell {cell} is {state:?}"
                );
                to += 1;
            }
            Moved::NoCapacity => {
                let spec = cluster.workloads[usize::try_from(id.0).unwrap()];
                for c in out.cells.iter().filter(|c| c.region != lost_region) {
                    let mut index = FitIndex::new(&c.cluster.nodes).unwrap();
                    for (at, w) in c.cluster.workloads.iter().enumerate() {
                        if let Some(State::Placed(node)) = c
                            .run
                            .store
                            .record(WorkloadId(u64::try_from(at).unwrap()))
                            .map(|r| r.state)
                        {
                            index.take(node, &w.demand()).unwrap();
                        }
                    }
                    let room = index.sampled_fit(&spec.demand(), &Sample::whole()).best;
                    assert!(
                        room.is_none(),
                        "{id:?} refused while region {} cell {} had room on {:?}",
                        c.region,
                        c.cell,
                        room.map(|k| k.node)
                    );
                }
                refused += 1;
            }
        }
    }
    (to, refused)
}

/// Lose region 1 of three, two cells each, at 2 s; run until done; return the run and the counts.
fn lose_region(cluster: &Cluster) -> (FedRun, u64, u64) {
    let config = Config {
        workers: 4,
        max_events: 1 << 24,
        ..Config::default()
    };
    let topology = Topology {
        regions: 3,
        cells: 2,
    };
    let loss = Loss::Region {
        region: 1,
        at: 2 * SECOND,
    };
    let out = federation::run(cluster, &fed(topology, config, loss)).unwrap();
    let surviving: Vec<CellRun> = out
        .cells
        .iter()
        .filter(|c| c.region != 1)
        .cloned()
        .collect();
    check_cells(&surviving, "region lost");
    let (to, refused) = check_moved(cluster, &out, 1);
    (out, to, refused)
}

/// T-T7.12.1, with room: do lose one of three regions of 100 nodes while each is a third full; expect every
/// workload of the lost region re-placed in another region (none refused), each where the federation
/// sent it, every surviving cell's log the serial specification's and nothing lost or doubled.
#[test]
fn a_lost_regions_work_is_re_placed_in_the_others_when_they_have_room() {
    let cluster = steady(21, 300, 1_500);
    let (out, to, refused) = lose_region(&cluster);
    assert_eq!(refused, 0);
    assert!(to > 400, "{to} moved");
    assert_eq!(u64::try_from(out.moved.len()).unwrap(), to);
}

/// T-T7.12.1, tight: do lose one of three regions of 100 nodes while each is near full; expect the moved
/// work placed only within the others' capacity: each moved workload placed where the federation
/// sent it, each refused one held by no surviving node (brute force), no node over its capacity (the
/// specification's replay), and (non-vacuity) some placed and some refused.
#[test]
fn a_lost_regions_work_is_re_placed_only_within_the_others_capacity() {
    let cluster = steady(22, 300, 4_000);
    let (_, to, refused) = lose_region(&cluster);
    assert!(to > 100, "{to} moved");
    assert!(refused > 100, "{refused} refused");
}

/// H16's fence: do lose a region; expect every workload the lost region held and hadn't finished by
/// the loss moved exactly once, and no workload of the lost region started in two surviving cells.
#[test]
fn a_lost_regions_work_moves_once() {
    let cluster = steady(23, 300, 1_500);
    let (out, _, _) = lose_region(&cluster);
    let held: u64 = out
        .cells
        .iter()
        .filter(|c| c.region == 1)
        .map(|c| u64::try_from(c.ids.len()).unwrap())
        .sum();
    assert_eq!(u64::try_from(out.moved.len()).unwrap(), held);
    let mut seen = std::collections::HashSet::new();
    for c in out.cells.iter().filter(|c| c.region != 1) {
        for (at, &g) in c.ids.iter().enumerate() {
            if c.run.starts[at] > 0 {
                assert!(seen.insert(g), "{g:?} started in two cells");
            }
        }
    }
}

/// T-T7.12.1 and H16's "assume and deduct": do lose one of three regions a third full, with 3,000
/// more workloads arriving in the very instant the federation acts, so the surviving regions have
/// them queued and not yet placed; expect the moved work placed only in room those queued workloads
/// leave (each moved one placed where it was sent, each refused one held by no surviving node), and
/// (non-vacuity) over 1,000 queued in the surviving regions then, some moved and some refused.
#[test]
fn a_lost_regions_work_waits_behind_what_the_others_have_queued() {
    let when = 2 * SECOND + federation::REGION_GRACE_NS;
    let mut cluster = steady(24, 300, 1_500);
    let burst = steady(25, 300, 3_000);
    let first = cluster.workloads.len();
    for (k, w) in burst.workloads.iter().enumerate() {
        cluster.workloads.push(hex_sim::model::WorkloadSpec {
            id: WorkloadId(u64::try_from(first + k).unwrap()),
            arrives: when,
            ..*w
        });
    }
    let (out, to, refused) = lose_region(&cluster);
    let moved: std::collections::HashSet<WorkloadId> = out.moved.iter().map(|m| m.0).collect();
    let queued: usize = out
        .cells
        .iter()
        .filter(|c| c.region != 1)
        .map(|c| {
            c.ids
                .iter()
                .filter(|g| usize::try_from(g.0).unwrap() >= first && !moved.contains(g))
                .count()
        })
        .sum();
    assert!(queued > 1_000, "{queued} queued in the surviving regions");
    assert!(to > 50, "{to} moved");
    assert!(refused > 50, "{refused} refused");
}
