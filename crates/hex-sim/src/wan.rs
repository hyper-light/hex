//! A modelled WAN, and hyper-swim's coordinates learned over it (`hex-sim.md` H20, scheduler design
//! S4 and A2).
//!
//! **Regions.** Eight Azure regions, their round trips Azure's published monthly P50s between
//! regions, measured by its backbone probes in 1-minute intervals ("Azure network round-trip latency
//! statistics", learn.microsoft.com/azure/networking/azure-network-latency, 2026-07-30). The table
//! gives each direction; the two differ by at most 2 ms, and the larger is kept, so the matrix is
//! symmetric. The table is real data: a few of its triples violate the triangle inequality, as
//! Internet latencies do (Ledlie, Gardner and Seltzer, NSDI 2007).
//!
//! **Zones and nodes.** A region has three zones; Azure's stated bound between zones is "less than
//! approximately 2 milliseconds" ("What are Azure availability zones?", Inter-zone networking), and
//! only that bound is published, so each pair of zones gets a term drawn below it. A node has an access
//! height, the time between it and its zone's network, drawn from [`HEIGHT_NS`]: a modelled range, not
//! a measurement. A true round trip is the two heights plus the zones' term (same region) or the
//! regions' (different regions); a probe sees it stretched by up to [`WanConfig::jitter_per_mille`].
//!
//! **Coordinates.** Every node runs [`CoordinateEngine`], hyper-swim's update, probing
//! `probes` peers a round drawn from the whole membership, as SWIM's probe target is (Das, Gupta and
//! Motivala, DSN 2002), for `rounds` rounds; each probe moves the prober by its sample against the
//! peer's coordinate as it stood. Order within a round is node order, so a seed names the result.

use std::time::Duration;

use crate::error::SimError;
use crate::model::NodeId;
use crate::rng::Rng;
use crate::trace::TraceHash;
use crate::vivaldi::{CoordinateEngine, NetworkCoordinate};

/// The regions, in the order of [`INTER_MS`].
pub const REGIONS: [&str; 8] = [
    "East US",
    "West US",
    "North Europe",
    "West Europe",
    "Southeast Asia",
    "Japan East",
    "Brazil South",
    "Australia East",
];

/// Azure's P50 round trip between regions, milliseconds, the larger of its two directions (module
/// documentation). Row and column follow [`REGIONS`]; the diagonal is the zones' term instead.
pub const INTER_MS: [[u64; 8]; 8] = [
    [0, 69, 73, 85, 224, 162, 118, 202],
    [69, 0, 133, 146, 170, 108, 169, 140],
    [73, 133, 0, 17, 176, 227, 172, 263],
    [85, 146, 17, 0, 169, 234, 185, 265],
    [224, 170, 176, 169, 0, 72, 331, 95],
    [162, 108, 227, 234, 72, 0, 262, 104],
    [118, 169, 172, 185, 331, 262, 0, 295],
    [202, 140, 263, 265, 95, 104, 295, 0],
];

/// Zones per region: each of the eight has three (Azure's list of regions with availability zones).
pub const ZONES: usize = 3;
/// Azure's stated bound on a round trip between zones of one region, nanoseconds.
pub const ZONE_BOUND_NS: u64 = 2_000_000;
/// A node's access height, nanoseconds: modelled, 25 µs to 250 µs, so two nodes of one zone are
/// 0.05-0.5 ms apart, below the zones' bound.
pub const HEIGHT_NS: (u64, u64) = (25_000, 250_000);
/// The most nodes a WAN holds: hex-sim's 10,000-node runs, with room.
pub const MAX_NODES: usize = 1 << 17;
/// The most probes a node makes a round, and rounds a convergence runs: together they bound the
/// updates a convergence makes at `MAX_NODES · MAX_PROBES · MAX_ROUNDS`.
pub const MAX_PROBES: usize = 64;
pub const MAX_ROUNDS: u32 = 1_024;
const NS_PER_MS: u64 = 1_000_000;
const NS_PER_SEC: f64 = 1e9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WanConfig {
    pub nodes: usize,
    /// How many of [`REGIONS`] the nodes spread over, round-robin, from the first.
    pub regions: usize,
    /// Peers each node probes a round.
    pub probes: usize,
    pub rounds: u32,
    /// The most a probe's round trip exceeds the true one, per mille of it.
    pub jitter_per_mille: u64,
    pub seed: u64,
}

impl Default for WanConfig {
    /// One probe a protocol period and a round of eight periods; 64 rounds; up to 10% jitter.
    /// Chosen for test runs, not measured.
    fn default() -> Self {
        Self {
            nodes: 1_000,
            regions: REGIONS.len(),
            probes: 8,
            rounds: 64,
            jitter_per_mille: 100,
            seed: 1,
        }
    }
}

/// Where a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Site {
    pub region: usize,
    pub zone: usize,
    pub height_ns: u64,
}

/// The modelled network: each node's site, and each region's zone-pair terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wan {
    sites: Vec<Site>,
    /// Per region, the term between each pair of its zones, nanoseconds; zero within one zone.
    zone_terms: Vec<[[u64; ZONES]; ZONES]>,
    jitter_per_mille: u64,
}

impl Wan {
    /// A WAN of `config.nodes` nodes, drawn from `rng`.
    pub fn generate(config: &WanConfig, rng: &mut Rng) -> Result<Self, SimError> {
        if config.nodes == 0 || config.nodes > MAX_NODES {
            return Err(SimError::Config("a WAN holds 1 to MAX_NODES nodes"));
        }
        if config.regions == 0 || config.regions > REGIONS.len() {
            return Err(SimError::Config("a WAN spans 1 to 8 regions"));
        }
        if config.jitter_per_mille > 1_000 {
            return Err(SimError::Config("jitter is at most the round trip itself"));
        }
        let mut zone_terms = Vec::with_capacity(config.regions);
        for _ in 0..config.regions {
            let mut terms = [[0u64; ZONES]; ZONES];
            for a in 0..ZONES {
                for b in a.saturating_add(1)..ZONES {
                    let term = rng.range(1, ZONE_BOUND_NS);
                    set(&mut terms, a, b, term)?;
                    set(&mut terms, b, a, term)?;
                }
            }
            zone_terms.push(terms);
        }
        let mut sites = Vec::with_capacity(config.nodes);
        for node in 0..config.nodes {
            let region = node
                .checked_rem(config.regions)
                .ok_or(SimError::Config("regions"))?;
            let zone = node
                .checked_div(config.regions)
                .and_then(|n| n.checked_rem(ZONES))
                .ok_or(SimError::Config("zones"))?;
            sites.push(Site {
                region,
                zone,
                height_ns: rng.range(HEIGHT_NS.0, HEIGHT_NS.1),
            });
        }
        Ok(Self {
            sites,
            zone_terms,
            jitter_per_mille: config.jitter_per_mille,
        })
    }

    pub fn len(&self) -> usize {
        self.sites.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sites.is_empty()
    }

    pub fn site(&self, node: NodeId) -> Result<Site, SimError> {
        usize::try_from(node.0)
            .ok()
            .and_then(|i| self.sites.get(i))
            .copied()
            .ok_or(SimError::UnknownNode(node))
    }

    /// The true round trip between two nodes, nanoseconds: zero to itself.
    pub fn rtt_ns(&self, a: NodeId, b: NodeId) -> Result<u64, SimError> {
        if a == b {
            return Ok(0);
        }
        let (x, y) = (self.site(a)?, self.site(b)?);
        let path = if x.region == y.region {
            self.zone_terms
                .get(x.region)
                .and_then(|t| t.get(x.zone))
                .and_then(|row| row.get(y.zone))
                .copied()
                .ok_or(SimError::UnknownNode(b))?
        } else {
            INTER_MS
                .get(x.region)
                .and_then(|row| row.get(y.region))
                .and_then(|ms| ms.checked_mul(NS_PER_MS))
                .ok_or(SimError::UnknownNode(b))?
        };
        x.height_ns
            .checked_add(y.height_ns)
            .and_then(|h| h.checked_add(path))
            .ok_or(SimError::TimeOverflow)
    }

    /// One probe's round trip: the true one stretched by a draw of up to the jitter.
    pub fn sample_ns(&self, a: NodeId, b: NodeId, rng: &mut Rng) -> Result<u64, SimError> {
        let rtt = self.rtt_ns(a, b)?;
        let stretch = rtt
            .checked_mul(rng.range(0, self.jitter_per_mille))
            .and_then(|s| s.checked_div(1_000))
            .ok_or(SimError::TimeOverflow)?;
        rtt.checked_add(stretch).ok_or(SimError::TimeOverflow)
    }
}

fn set(terms: &mut [[u64; ZONES]; ZONES], a: usize, b: usize, term: u64) -> Result<(), SimError> {
    let cell = terms
        .get_mut(a)
        .and_then(|row| row.get_mut(b))
        .ok_or(SimError::Config("zones"))?;
    *cell = term;
    Ok(())
}

/// Every node's coordinate engine after a convergence.
pub struct Coordinates {
    engines: Vec<CoordinateEngine>,
    /// Samples taken (the engine's `update` said so), and offered.
    pub taken: u64,
    pub offered: u64,
}

impl Coordinates {
    /// Each node starts at the origin, its engine's stream seeded by the run's seed and its id, and
    /// probes `config.probes` peers a round for `config.rounds` rounds.
    pub fn converge(wan: &Wan, config: &WanConfig, rng: &mut Rng) -> Result<Self, SimError> {
        if config.probes == 0 || config.probes > MAX_PROBES {
            return Err(SimError::Config("a round is 1 to MAX_PROBES probes"));
        }
        if config.rounds > MAX_ROUNDS {
            return Err(SimError::Config(
                "a convergence is at most MAX_ROUNDS rounds",
            ));
        }
        let n = wan.len();
        let last = u64::try_from(n)
            .ok()
            .and_then(|n| n.checked_sub(1))
            .ok_or(SimError::Config("a WAN of no nodes"))?;
        let mut engines = Vec::with_capacity(n);
        for node in 0..n {
            let id = u64::try_from(node).map_err(|_| SimError::Config("node ids"))?;
            engines.push(CoordinateEngine::new(config.seed ^ id.rotate_left(32)));
        }
        let mut coordinates = Self {
            engines,
            taken: 0,
            offered: 0,
        };
        if n < 2 {
            return Ok(coordinates);
        }
        for _ in 0..config.rounds {
            for node in 0..n {
                let me = node_id(node)?;
                for _ in 0..config.probes {
                    let mut peer = rng.range(0, last.saturating_sub(1));
                    if peer >= u64::from(me.0) {
                        peer = peer.saturating_add(1);
                    }
                    let peer = NodeId(u32::try_from(peer).map_err(|_| SimError::Config("ids"))?);
                    let rtt = Duration::from_nanos(wan.sample_ns(me, peer, rng)?);
                    let theirs = *coordinates.coordinate(peer)?;
                    let engine = coordinates
                        .engines
                        .get_mut(node)
                        .ok_or(SimError::UnknownNode(me))?;
                    let took = engine.update(&theirs, rtt, config.probes);
                    coordinates.offered = coordinates.offered.saturating_add(1);
                    if took {
                        coordinates.taken = coordinates.taken.saturating_add(1);
                    }
                }
            }
        }
        Ok(coordinates)
    }

    pub fn coordinate(&self, node: NodeId) -> Result<&NetworkCoordinate, SimError> {
        usize::try_from(node.0)
            .ok()
            .and_then(|i| self.engines.get(i))
            .map(CoordinateEngine::coordinate)
            .ok_or(SimError::UnknownNode(node))
    }

    /// The predicted round trip between two nodes, seconds.
    pub fn predict(&self, a: NodeId, b: NodeId) -> Result<f64, SimError> {
        Ok(CoordinateEngine::estimate_rtt(
            self.coordinate(a)?,
            self.coordinate(b)?,
        ))
    }

    /// A hash of every coordinate's bits, in node order.
    pub fn hash(&self) -> u64 {
        let mut h = TraceHash::default();
        for engine in &self.engines {
            let c = engine.coordinate();
            for x in c.position {
                h.word(x.to_bits());
            }
            h.word(c.height.to_bits());
            h.word(c.error.to_bits());
        }
        h.finish()
    }
}

/// How well the coordinates predict the WAN, over a bounded sample of pairs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Accuracy {
    pub pairs: usize,
    /// The median of `|predicted − true| / true` over the pairs.
    pub median_relative_error: f64,
    /// Of the sampled nodes given one peer in their region and one outside it, how many predicted the
    /// one in their region nearer; of how many.
    pub region_ranked_first: usize,
    pub region_trials: usize,
}

impl Accuracy {
    /// Measures `pairs` random pairs (at most [`MAX_NODES`]) and as many region trials.
    pub fn measure(
        wan: &Wan,
        coordinates: &Coordinates,
        pairs: usize,
        rng: &mut Rng,
    ) -> Result<Self, SimError> {
        if pairs == 0 || pairs > MAX_NODES {
            return Err(SimError::Config("accuracy measures 1 to MAX_NODES pairs"));
        }
        let last = u64::try_from(wan.len())
            .ok()
            .and_then(|n| n.checked_sub(1))
            .ok_or(SimError::Config("a WAN of no nodes"))?;
        let draw = |rng: &mut Rng| -> Result<NodeId, SimError> {
            u32::try_from(rng.range(0, last))
                .map(NodeId)
                .map_err(|_| SimError::Config("ids"))
        };
        let mut errors = Vec::with_capacity(pairs);
        let mut region_ranked_first = 0usize;
        let mut region_trials = 0usize;
        for _ in 0..pairs {
            let (a, b) = (draw(rng)?, draw(rng)?);
            if a != b {
                let truth = seconds(wan.rtt_ns(a, b)?);
                let predicted = coordinates.predict(a, b)?;
                errors.push((predicted - truth).abs() / truth);
            }
            // A region trial: the first node drawn in a's region, and the first outside it, within
            // a bounded number of draws.
            let home = wan.site(a)?.region;
            let mut near = None;
            let mut far = None;
            for _ in 0..64 {
                let c = draw(rng)?;
                if c == a {
                    continue;
                }
                if wan.site(c)?.region == home {
                    near = near.or(Some(c));
                } else {
                    far = far.or(Some(c));
                }
                if near.is_some() && far.is_some() {
                    break;
                }
            }
            if let (Some(near), Some(far)) = (near, far) {
                region_trials = region_trials.saturating_add(1);
                if coordinates.predict(a, near)? < coordinates.predict(a, far)? {
                    region_ranked_first = region_ranked_first.saturating_add(1);
                }
            }
        }
        errors.sort_by(f64::total_cmp);
        let median_relative_error = errors
            .get(errors.len().checked_div(2).unwrap_or(0))
            .copied()
            .ok_or(SimError::Config("no pairs of distinct nodes were drawn"))?;
        Ok(Self {
            pairs: errors.len(),
            median_relative_error,
            region_ranked_first,
            region_trials,
        })
    }
}

fn seconds(ns: u64) -> f64 {
    ns as f64 / NS_PER_SEC
}

pub(crate) fn node_id(node: usize) -> Result<NodeId, SimError> {
    u32::try_from(node)
        .map(NodeId)
        .map_err(|_| SimError::Config("node ids fit u32"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn converged(config: &WanConfig) -> (Wan, Coordinates) {
        let mut rng = Rng::new(config.seed);
        let wan = Wan::generate(config, &mut rng).unwrap();
        let coordinates = Coordinates::converge(&wan, config, &mut rng).unwrap();
        (wan, coordinates)
    }

    /// The matrix is Azure's table as the module says: symmetric, zero on the diagonal, and every
    /// cross-region round trip far above the zones' bound.
    #[test]
    fn the_region_matrix_is_symmetric_and_wide_area() {
        for (i, row) in INTER_MS.iter().enumerate() {
            for (j, &ms) in row.iter().enumerate() {
                assert_eq!(ms, INTER_MS[j][i], "{} / {}", REGIONS[i], REGIONS[j]);
                if i == j {
                    assert_eq!(ms, 0);
                } else {
                    assert!(ms * NS_PER_MS > 8 * ZONE_BOUND_NS, "{i} {j}");
                }
            }
        }
    }

    /// Do: build a 1,000-node, 8-region WAN. Expect every true round trip symmetric, inside a region
    /// below the zones' bound plus two heights, and across regions at least the table's.
    #[test]
    fn true_round_trips_follow_the_model() {
        let config = WanConfig::default();
        let mut rng = Rng::new(3);
        let wan = Wan::generate(&config, &mut rng).unwrap();
        for a in (0..1_000u32).step_by(37) {
            for b in (0..1_000u32).step_by(41) {
                let (x, y) = (NodeId(a), NodeId(b));
                let rtt = wan.rtt_ns(x, y).unwrap();
                assert_eq!(rtt, wan.rtt_ns(y, x).unwrap());
                let (sx, sy) = (wan.site(x).unwrap(), wan.site(y).unwrap());
                if a == b {
                    assert_eq!(rtt, 0);
                } else if sx.region == sy.region {
                    assert!(rtt < ZONE_BOUND_NS + 2 * HEIGHT_NS.1);
                } else {
                    assert!(rtt >= INTER_MS[sx.region][sy.region] * NS_PER_MS);
                }
                let sample = wan.sample_ns(x, y, &mut rng).unwrap();
                assert!(sample >= rtt && sample <= rtt + rtt / 10);
            }
        }
        assert_eq!(
            wan.rtt_ns(NodeId(0), NodeId(1_000)),
            Err(SimError::UnknownNode(NodeId(1_000)))
        );
    }

    /// S4, A2 (G3 step 1): one seed names one set of coordinates, bit for bit; another seed another.
    #[test]
    fn a_seed_names_one_set_of_coordinates() {
        let config = WanConfig {
            rounds: 16,
            ..WanConfig::default()
        };
        let (_, a) = converged(&config);
        let (_, b) = converged(&config);
        let (_, c) = converged(&WanConfig { seed: 2, ..config });
        assert_eq!(a.hash(), b.hash());
        assert_ne!(a.hash(), c.hash());
        assert_eq!(a.offered, 1_000 * 8 * 16);
        assert_eq!(
            a.taken, a.offered,
            "every probe of a distinct live peer is a sample"
        );
    }

    /// S4, A2 (G3 step 1): do converge hyper-swim's coordinates on a 1,000-node 8-region WAN;
    /// expect the median relative error at most 25% and a node's own region predicted nearer than
    /// another region's in at least 95% of trials (chance is 50%).
    #[test]
    fn coordinates_learn_the_wan() {
        let config = WanConfig::default();
        let (wan, coordinates) = converged(&config);
        let mut rng = Rng::new(99);
        let accuracy = Accuracy::measure(&wan, &coordinates, 20_000, &mut rng).unwrap();
        eprintln!("G3 coordinates: {accuracy:?}");
        assert!(accuracy.pairs > 19_000);
        assert!(
            accuracy.median_relative_error <= 0.25,
            "median relative error {:.3}",
            accuracy.median_relative_error
        );
        assert!(accuracy.region_trials > 19_000);
        assert!(
            accuracy.region_ranked_first * 100 >= accuracy.region_trials * 95,
            "{} of {}",
            accuracy.region_ranked_first,
            accuracy.region_trials
        );
    }

    /// Non-vacuity for the test above: with no rounds every coordinate is the origin, and the same
    /// measure finds them useless (relative error 100% at the median, regions no better than chance).
    #[test]
    fn unconverged_coordinates_fail_the_same_measure() {
        let config = WanConfig {
            rounds: 0,
            ..WanConfig::default()
        };
        let (wan, coordinates) = converged(&config);
        let mut rng = Rng::new(99);
        let accuracy = Accuracy::measure(&wan, &coordinates, 2_000, &mut rng).unwrap();
        assert!(accuracy.median_relative_error > 0.99);
        assert_eq!(accuracy.region_ranked_first, 0);
    }

    /// Hostile configurations are refused, typed.
    #[test]
    fn out_of_bounds_configurations_are_refused() {
        let mut rng = Rng::new(1);
        for bad in [
            WanConfig {
                nodes: 0,
                ..WanConfig::default()
            },
            WanConfig {
                nodes: MAX_NODES + 1,
                ..WanConfig::default()
            },
            WanConfig {
                regions: 9,
                ..WanConfig::default()
            },
            WanConfig {
                jitter_per_mille: 1_001,
                ..WanConfig::default()
            },
        ] {
            assert!(matches!(
                Wan::generate(&bad, &mut rng),
                Err(SimError::Config(_))
            ));
        }
        let wan = Wan::generate(&WanConfig::default(), &mut rng).unwrap();
        for bad in [
            WanConfig {
                probes: 0,
                ..WanConfig::default()
            },
            WanConfig {
                rounds: MAX_ROUNDS + 1,
                ..WanConfig::default()
            },
        ] {
            assert!(matches!(
                Coordinates::converge(&wan, &bad, &mut rng),
                Err(SimError::Config(_))
            ));
        }
    }
}
