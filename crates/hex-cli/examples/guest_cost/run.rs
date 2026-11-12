//! The producer's shared half: its plan (SEEDABLE), the census over a driver's guests, the cost they
//! derive, its evidence and the output.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hex_api::{CapacityResources, RuntimeCost};
use sha2::{Digest, Sha256};

use super::seam::{Driver, Host};

/// The census whose rule names the fewest samples a row seeds from (decision 9).
const CENSUS: &str = include_str!("../../../../bench/competitive/probes/waits_census.py");

/// The evidence's first line: what reads it knows it from the sections other producers put before it.
const HEADER: &str = "hex-guest-cost 1";

pub struct Plan {
    /// The census rule's SEEDABLE: the fewest guests a row is taken from.
    pub seedable: usize,
}

impl Plan {
    pub fn derived() -> Result<Self, String> {
        let seedable = CENSUS
            .lines()
            .find_map(|line| line.strip_prefix("SEEDABLE = "))
            .and_then(|n| n.trim().replace('_', "").parse::<usize>().ok())
            .filter(|n| *n > 0)
            .ok_or("bench/competitive/probes/waits_census.py names no SEEDABLE")?;
        Ok(Self { seedable })
    }
}

/// One guest that was ready: how long its boot took (and so its idle hold), its VMM's use over the hold,
/// and what its agent reported free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub ready: Duration,
    pub host: Host,
    pub free: CapacityResources,
}

/// A run's guests: each that was ready, and the bound each one that ran out passed.
pub struct Census {
    pub first: Duration,
    pub samples: Vec<Sample>,
    pub ran_out: Vec<Duration>,
}

/// Boots guests one at a time until `seedable` were ready. Each is booted within the bound, which starts
/// at `first` and doubles at each run-out (decision 9's run-outs, each recorded); a ready one is held idle
/// as long as its boot took, reported on and torn down, whatever its hold or report came to. A run whose
/// run-outs reach `seedable` is refused: its class's guest runs out as often as the census needs answers.
pub fn census<D: Driver>(
    driver: &mut D,
    seedable: usize,
    first: Duration,
) -> Result<Census, String> {
    let mut census = Census {
        first,
        samples: Vec::with_capacity(seedable),
        ran_out: Vec::new(),
    };
    let mut bound = first;
    while census.samples.len() < seedable {
        if census.ran_out.len() >= seedable {
            return Err(format!(
                "{} boots ran out (the last at {bound:?}) before {seedable} were ready",
                census.ran_out.len()
            ));
        }
        let began = Instant::now();
        let Some(guest) = driver.boot(after(began, bound)?)? else {
            census.ran_out.push(bound);
            bound = bound
                .checked_mul(2)
                .ok_or("the doubled boot bound overflows")?;
            continue;
        };
        let ready = began.elapsed();
        let measured = driver
            .hold(&guest, ready)
            .and_then(|host| Ok((host, driver.report(&guest, after(Instant::now(), bound)?)?)));
        let ended = after(Instant::now(), bound).and_then(|at| driver.tear_down(guest, at));
        let (host, free) = measured?;
        ended?;
        census.samples.push(Sample { ready, host, free });
    }
    Ok(census)
}

fn after(from: Instant, span: Duration) -> Result<Instant, String> {
    from.checked_add(span)
        .ok_or_else(|| format!("a wait of {span:?} overflows the clock"))
}

/// The cost's measured parts: the maximum over the census's guests (its rule's recorded maximum), each
/// dimension apart. Overhead, outside the envelope: the VMM's CPU over its hold as milli-CPUs (rounded
/// up), its peak memory beyond the envelope's, and the bytes it wrote. Guest overhead, inside: the envelope
/// less what the guest's agent reported free.
pub fn derive(
    envelope: CapacityResources,
    census: &Census,
    seedable: usize,
) -> Result<(CapacityResources, CapacityResources), String> {
    if census.samples.len() < seedable {
        return Err(format!(
            "{} guests measured, fewer than SEEDABLE {seedable}",
            census.samples.len()
        ));
    }
    let mut overhead = CapacityResources::default();
    let mut inside = CapacityResources::default();
    for sample in &census.samples {
        overhead.cpu_milli = overhead
            .cpu_milli
            .max(milli(sample.host.cpu, sample.ready)?);
        overhead.mem_bytes = overhead
            .mem_bytes
            .max(sample.host.peak_bytes.saturating_sub(envelope.mem_bytes));
        overhead.disk_bytes = overhead.disk_bytes.max(sample.host.written_bytes);
        inside.cpu_milli = inside
            .cpu_milli
            .max(envelope.cpu_milli.saturating_sub(sample.free.cpu_milli));
        inside.mem_bytes = inside
            .mem_bytes
            .max(envelope.mem_bytes.saturating_sub(sample.free.mem_bytes));
        inside.disk_bytes = inside
            .disk_bytes
            .max(envelope.disk_bytes.saturating_sub(sample.free.disk_bytes));
    }
    Ok((overhead, inside))
}

/// `cpu` used over `span`, as milli-CPUs, rounded up.
fn milli(cpu: Duration, span: Duration) -> Result<u64, String> {
    if span.is_zero() {
        return Err("a guest was ready in no time, so its hold measures nothing".to_owned());
    }
    let milli = cpu
        .as_nanos()
        .checked_mul(1000)
        .map(|n| n.div_ceil(span.as_nanos()))
        .ok_or("a VMM's CPU overflows its rate")?;
    u64::try_from(milli).map_err(|_| "a VMM's CPU rate overflows".to_owned())
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What a run wrote.
pub struct Made {
    pub summary: String,
    pub cost_file: PathBuf,
    pub cost_sha256: [u8; 32],
    pub evidence_file: PathBuf,
}

/// The run on `driver`: its census, the cost and evidence derived from it, written under `out` as
/// `cost.json` and `evidence`, the cost's id the evidence's SHA-256.
pub fn produce<D: Driver>(driver: &mut D, plan: &Plan, out: &Path) -> Result<Made, String> {
    let class = driver.class();
    let described = driver.describe()?;
    let envelope = driver.envelope()?;
    let patience = driver.patience()?;
    let page = rustix::param::page_size();
    let block = rustix::fs::statvfs(driver.runtime())
        .map_err(|e| format!("reading {}'s filesystem: {e}", driver.runtime().display()))?
        .f_frsize;
    let quantum = CapacityResources {
        cpu_milli: 1,
        mem_bytes: u64::try_from(page).map_err(|e| e.to_string())?,
        disk_bytes: block,
    };
    let census = census(driver, plan.seedable, patience.bound)?;
    let (overhead, inside) = derive(envelope.resources, &census, plan.seedable)?;

    let mut evidence = String::new();
    let mut line = |text: String| {
        evidence.push_str(&text);
        evidence.push('\n');
    };
    line(HEADER.to_owned());
    line(format!(
        "class {class} on {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    for (name, value) in described {
        line(format!("config {name} {value}"));
    }
    let resources = |r: CapacityResources| {
        format!(
            "cpu_milli={} mem_bytes={} disk_bytes={}",
            r.cpu_milli, r.mem_bytes, r.disk_bytes
        )
    };
    line(format!(
        "envelope {} ({}): floor and ceiling",
        resources(envelope.resources),
        envelope.source
    ));
    line(format!(
        "quantum {} (cpu: the API's milli-CPU; mem: this host's page size; disk: statvfs f_frsize of {})",
        resources(quantum),
        driver.runtime().display()
    ));
    line(format!(
        "census seedable={} (SEEDABLE, bench/competitive/probes/waits_census.py, decision 9) ready={} ran_out={} first_bound_ns={} ({}), doubled at each run-out; each ready guest held idle as long as its boot took",
        plan.seedable,
        census.samples.len(),
        census.ran_out.len(),
        census.first.as_nanos(),
        patience.source
    ));
    line(format!(
        "overhead {} (the maximum over the ready guests: the VMM's CPU over its hold, rounded up; its peak memory beyond the envelope's; the bytes it wrote; {})",
        resources(overhead),
        driver.host_source()
    ));
    line(format!(
        "guest_overhead {} (the maximum over the ready guests of the envelope less what the guest's agent reported free)",
        resources(inside)
    ));
    line("samples ready_ns cpu_ns peak_bytes written_bytes free_cpu_milli free_mem_bytes free_disk_bytes".to_owned());
    for (n, s) in census.samples.iter().enumerate() {
        line(format!(
            "ready {n} {} {} {} {} {} {} {}",
            s.ready.as_nanos(),
            s.host.cpu.as_nanos(),
            s.host.peak_bytes,
            s.host.written_bytes,
            s.free.cpu_milli,
            s.free.mem_bytes,
            s.free.disk_bytes
        ));
    }
    for (n, bound) in census.ran_out.iter().enumerate() {
        line(format!("ran_out {n} {}", bound.as_nanos()));
    }
    let cost = RuntimeCost {
        id: Sha256::digest(evidence.as_bytes()).into(),
        floor: envelope.resources,
        ceiling: envelope.resources,
        quantum,
        guest_overhead: inside,
        overhead,
    };
    cost.check().map_err(|e| e.to_string())?;
    cost.allocate(CapacityResources::default())
        .map_err(|e| format!("the derived cost admits no guest: {e}"))?;
    let cost_bytes = serde_json::to_vec(&cost).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let made = Made {
        summary: String::new(),
        cost_file: out.join("cost.json"),
        cost_sha256: Sha256::digest(&cost_bytes).into(),
        evidence_file: out.join("evidence"),
    };
    std::fs::write(&made.evidence_file, evidence.as_bytes()).map_err(|e| e.to_string())?;
    std::fs::write(&made.cost_file, &cost_bytes).map_err(|e| e.to_string())?;
    let mut summary = String::new();
    let _ = write!(
        summary,
        "{class}: {} guests ready, {} ran out; overhead {}; guest_overhead {}",
        census.samples.len(),
        census.ran_out.len(),
        resources(overhead),
        resources(inside)
    );
    Ok(Made { summary, ..made })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seam::{Envelope, Lines, Patience};

    /// A scripted class: each boot either runs out or is ready after a set time, its VMM's use and its
    /// agent's report as scripted. No VM runs.
    struct Scripted {
        runtime: PathBuf,
        boots: Vec<Option<Sample>>,
        booted: usize,
        torn_down: usize,
        bounds: Vec<Duration>,
    }

    impl Driver for Scripted {
        type Guest = Sample;
        fn class(&self) -> String {
            "scripted".to_owned()
        }
        fn describe(&self) -> Result<Lines, String> {
            Ok(vec![("image".to_owned(), "none".to_owned())])
        }
        fn envelope(&self) -> Result<Envelope, String> {
            Ok(Envelope {
                resources: envelope(),
                source: "scripted".to_owned(),
            })
        }
        fn patience(&self) -> Result<Patience, String> {
            Ok(Patience {
                bound: Duration::from_secs(1),
                source: "scripted".to_owned(),
            })
        }
        fn runtime(&self) -> &Path {
            &self.runtime
        }
        fn host_source(&self) -> String {
            "scripted".to_owned()
        }
        fn boot(&mut self, deadline: Instant) -> Result<Option<Sample>, String> {
            self.bounds
                .push(deadline.saturating_duration_since(Instant::now()));
            let next = self.boots.get(self.booted).copied().ok_or("no boot left")?;
            self.booted += 1;
            Ok(next)
        }
        fn hold(&mut self, guest: &Sample, _: Duration) -> Result<Host, String> {
            Ok(guest.host)
        }
        fn report(&mut self, guest: &Sample, _: Instant) -> Result<CapacityResources, String> {
            Ok(guest.free)
        }
        fn tear_down(&mut self, _: Sample, _: Instant) -> Result<(), String> {
            self.torn_down += 1;
            Ok(())
        }
    }

    fn envelope() -> CapacityResources {
        CapacityResources {
            cpu_milli: 2000,
            mem_bytes: 512 << 20,
            disk_bytes: 0,
        }
    }

    fn sample(cpu_ms: u64, peak: u64, written: u64, free_mem: u64) -> Sample {
        Sample {
            ready: Duration::from_millis(100),
            host: Host {
                cpu: Duration::from_millis(cpu_ms),
                peak_bytes: peak,
                written_bytes: written,
            },
            free: CapacityResources {
                cpu_milli: 2000,
                mem_bytes: free_mem,
                disk_bytes: 64 << 20,
            },
        }
    }

    /// AC-T7.4.1: the plan's SEEDABLE is the census's own.
    #[test]
    fn the_plan_takes_the_census_rules_seedable() {
        assert_eq!(Plan::derived().unwrap().seedable, 1_000);
    }

    /// AC-T7.4.1: a census of three ready guests and two run-outs between them: expect each run-out's
    /// bound recorded, the bound doubled after each, every ready guest torn down, and each dimension the
    /// maximum over the ready guests (the VMM's CPU per its hold rounded up; its peak beyond the envelope's
    /// memory, none for one under it; its writes; the envelope less the agent's free, saturating).
    #[test]
    fn the_census_doubles_at_run_outs_and_takes_each_maximum() {
        let dir = tempfile::tempdir().unwrap();
        let mut driver = Scripted {
            runtime: dir.path().to_owned(),
            boots: vec![
                Some(sample(10, 600 << 20, 4096, 400 << 20)),
                None,
                Some(sample(25, 100 << 20, 8192, 450 << 20)),
                None,
                Some(sample(5, 520 << 20, 0, 300 << 20)),
            ],
            booted: 0,
            torn_down: 0,
            bounds: Vec::new(),
        };
        let census = census(&mut driver, 3, Duration::from_secs(1)).unwrap();
        assert_eq!(census.samples.len(), 3);
        assert_eq!(
            census.ran_out,
            vec![Duration::from_secs(1), Duration::from_secs(2)]
        );
        assert!(
            driver.bounds[4] > Duration::from_millis(3_900),
            "{:?}",
            driver.bounds
        );
        assert_eq!(driver.torn_down, 3);
        let (overhead, inside) = derive(envelope(), &census, 3).unwrap();
        // Each guest's CPU over its hold, which lasts as long as its boot took here.
        let rate = |s: &Sample| {
            u64::try_from((s.host.cpu.as_nanos() * 1000).div_ceil(s.ready.as_nanos())).unwrap()
        };
        assert_eq!(
            overhead.cpu_milli,
            census.samples.iter().map(rate).max().unwrap()
        );
        assert_eq!(overhead.mem_bytes, (600 << 20) - (512 << 20));
        assert_eq!(overhead.disk_bytes, 8192);
        assert_eq!(
            inside,
            CapacityResources {
                cpu_milli: 0,
                mem_bytes: (512 << 20) - (300 << 20),
                disk_bytes: 0,
            }
        );
        assert!(derive(envelope(), &census, 4).is_err());
    }

    /// AC-T7.4.1 (ROOT, CPU per window; original red): two guests ready at once whose VMMs used 1 ms of CPU
    /// before their first boundary: expect the CPU figure that millisecond over the node's control window
    /// (no less than 50 ms: at most 20 milli-CPU), never over the boot's own microseconds.
    #[test]
    fn a_short_boot_does_not_set_the_cpu_figure_from_noise() {
        let dir = tempfile::tempdir().unwrap();
        let mut driver = Scripted {
            runtime: dir.path().to_owned(),
            boots: vec![Some(sample(1, 64 << 20, 0, 400 << 20)); 2],
            booted: 0,
            torn_down: 0,
            bounds: Vec::new(),
        };
        let census = census(&mut driver, 2, Duration::from_secs(1)).unwrap();
        let (overhead, _) = derive(envelope(), &census, 2).unwrap();
        assert!(overhead.cpu_milli <= 20, "{overhead:?}");
    }

    /// AC-T7.4.1 (ROOT, VMM memory; original red): a guest that touched 30 MiB of its 512 MiB, its VMM's
    /// footprint 90 MiB: expect the VMM's own memory as the overhead, never nothing because the footprint
    /// stayed under the envelope's.
    #[test]
    fn a_partial_touch_guest_still_shows_vmm_memory() {
        let dir = tempfile::tempdir().unwrap();
        let mut driver = Scripted {
            runtime: dir.path().to_owned(),
            boots: vec![Some(sample(1, 90 << 20, 0, 400 << 20)); 2],
            booted: 0,
            torn_down: 0,
            bounds: Vec::new(),
        };
        let census = census(&mut driver, 2, Duration::from_secs(1)).unwrap();
        let (overhead, _) = derive(envelope(), &census, 2).unwrap();
        assert!(overhead.mem_bytes > 0, "{overhead:?}");
    }

    /// AC-T7.4.1 (ROOT, disk per VM; original red): the first guest's VMM writes the host's carried 2 MiB
    /// binary into the cache, the second writes nothing, and neither holds a byte of its own: expect no disk
    /// overhead, the cache being the host's.
    #[test]
    fn a_first_boot_cache_write_is_not_disk_overhead() {
        let dir = tempfile::tempdir().unwrap();
        let mut driver = Scripted {
            runtime: dir.path().to_owned(),
            boots: vec![
                Some(sample(1, 64 << 20, 2 << 20, 400 << 20)),
                Some(sample(1, 64 << 20, 0, 400 << 20)),
            ],
            booted: 0,
            torn_down: 0,
            bounds: Vec::new(),
        };
        let census = census(&mut driver, 2, Duration::from_secs(1)).unwrap();
        let (overhead, _) = derive(envelope(), &census, 2).unwrap();
        assert_eq!(overhead.disk_bytes, 0, "{overhead:?}");
    }

    /// AC-T7.4.1: a class whose guests run out as often as the census needs answers is refused, not
    /// waited on forever.
    #[test]
    fn as_many_run_outs_as_samples_refuse_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let mut driver = Scripted {
            runtime: dir.path().to_owned(),
            boots: vec![None, None, None],
            booted: 0,
            torn_down: 0,
            bounds: Vec::new(),
        };
        assert!(census(&mut driver, 2, Duration::from_secs(1)).is_err());
        assert_eq!(driver.booted, 2);
    }

    /// AC-T7.4.1: produce over a scripted class: expect the cost and evidence written, the cost's id the
    /// evidence's SHA-256, the pair loaded as a node loads it, and no CPU-response section yet (the Mac
    /// producer adds its row to this same pair).
    #[test]
    fn the_pair_loads_as_the_node_loads_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut driver = Scripted {
            runtime: dir.path().to_owned(),
            boots: vec![Some(sample(10, 600 << 20, 4096, 400 << 20)); 2],
            booted: 0,
            torn_down: 0,
            bounds: Vec::new(),
        };
        let out = dir.path().join("out");
        let made = produce(&mut driver, &Plan { seedable: 2 }, &out).unwrap();
        let (cost, _, rows) = hex_runtime::guest::load_runtime_evidence(
            &made.cost_file,
            made.cost_sha256,
            &made.evidence_file,
        )
        .unwrap();
        assert!(rows.is_empty());
        let evidence = std::fs::read(&made.evidence_file).unwrap();
        let digest: [u8; 32] = Sha256::digest(&evidence).into();
        assert_eq!(cost.id, digest);
        assert!(evidence.starts_with(b"hex-guest-cost 1\n"));
        assert_eq!((cost.floor, cost.ceiling), (envelope(), envelope()));
        assert_eq!(cost.quantum.cpu_milli, 1);
        assert_eq!(
            cost.quantum.mem_bytes,
            u64::try_from(rustix::param::page_size()).unwrap()
        );
        let text = String::from_utf8(evidence).unwrap();
        assert_eq!(text.lines().filter(|l| l.starts_with("ready ")).count(), 2);
    }
}
