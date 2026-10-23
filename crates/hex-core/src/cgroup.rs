//! A box's bounds as cgroup v2 file writes, and its usage read back from cgroup v2 files. File names,
//! formats and units are the kernel's (Documentation/admin-guide/cgroup-v2.rst).

use hex_api::{Bounds, Refusal, Usage};

/// `cpu.max`'s period, in microseconds: the kernel's default (100 ms).
pub const CPU_PERIOD_US: u64 = 100_000;
/// `cpu.max` quota for each thousandth of a CPU: a thousandth of [`CPU_PERIOD_US`].
const QUOTA_US_PER_MILLI: u64 = 100;

/// The controllers a box's parent enables for it: those [`Bounds`] writes to.
pub const CONTROLLERS: [&str; 4] = ["cpu", "io", "memory", "pids"];

/// A block device, by number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Dev {
    pub major: u32,
    pub minor: u32,
}

/// One write to a file in the box's cgroup directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    pub file: &'static str,
    pub value: String,
}

fn w(file: &'static str, value: String) -> Write {
    Write { file, value }
}

/// What to write so the box holds `bounds`: `disks` are the devices `io.latency` and `io.max` apply
/// to (the disks the node keeps boxes' files on). Writes come in the order the kernel needs: a box's
/// `memory.high` is never set above a `memory.max` written after it.
pub fn writes(bounds: &Bounds, disks: &[Dev]) -> Result<Vec<Write>, Refusal> {
    bounds.check()?;
    let mut out = Vec::new();
    if let Some(weight) = bounds.cpu_weight {
        out.push(w("cpu.weight", weight.to_string()));
    }
    if let Some(milli) = bounds.cpu_max_milli {
        let quota = u64::from(milli)
            .checked_mul(QUOTA_US_PER_MILLI)
            .ok_or(Refusal::Invalid {
                what: "cpu_max_milli",
                why: format!("{milli} overflows cpu.max"),
            })?;
        out.push(w("cpu.max", format!("{quota} {CPU_PERIOD_US}")));
    }
    if let Some(max) = bounds.memory_max {
        out.push(w("memory.max", max.to_string()));
        // The bound covers all of the box's memory: with swap open, a box past memory.max swaps
        // instead of being killed, so on a host with swap it is no bound (a 512 MiB hog finished
        // in a 64 MiB box on zram, 2026-10-23). memory.swap.max 0 keeps its anonymous memory in RAM.
        out.push(w("memory.swap.max", "0".to_owned()));
    }
    if let Some(high) = bounds.memory_high {
        out.push(w("memory.high", high.to_string()));
    }
    if let Some(weight) = bounds.io_weight {
        out.push(w("io.weight", format!("default {weight}")));
    }
    for d in disks {
        if let Some(target) = bounds.io_latency_us {
            out.push(w(
                "io.latency",
                format!("{}:{} target={target}", d.major, d.minor),
            ));
        }
        if let Some(r) = bounds.io_max {
            let rate = |v: Option<u64>| v.map_or_else(|| "max".to_owned(), |v| v.to_string());
            out.push(w(
                "io.max",
                format!(
                    "{}:{} rbps={} wbps={} riops={} wiops={}",
                    d.major,
                    d.minor,
                    rate(r.read_bps),
                    rate(r.write_bps),
                    rate(r.read_iops),
                    rate(r.write_iops)
                ),
            ));
        }
    }
    if let Some(max) = bounds.pids_max {
        out.push(w("pids.max", max.to_string()));
    }
    Ok(out)
}

/// The cgroup files [`usage`] reads, by name; a file the kernel doesn't have reads as empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageFiles {
    pub memory_current: String,
    pub memory_peak: String,
    pub memory_events: String,
    pub pids_current: String,
    pub pids_events: String,
    pub cpu_stat: String,
    pub io_stat: String,
}

/// What the files say the box used. A field the kernel didn't report reads 0; a value that isn't a
/// number is skipped, never trusted.
pub fn usage(f: &UsageFiles) -> Usage {
    let num = |s: &str| s.trim().parse::<u64>().unwrap_or(0);
    let mut io_read_bytes = 0u64;
    let mut io_write_bytes = 0u64;
    for line in f.io_stat.lines() {
        io_read_bytes = io_read_bytes.saturating_add(keyed(line, "rbytes=").unwrap_or(0));
        io_write_bytes = io_write_bytes.saturating_add(keyed(line, "wbytes=").unwrap_or(0));
    }
    Usage {
        memory_bytes: num(&f.memory_current),
        memory_peak_bytes: num(&f.memory_peak),
        oom_kills: flat(&f.memory_events, "oom_kill").unwrap_or(0),
        pids: num(&f.pids_current),
        pids_refused: flat(&f.pids_events, "max").unwrap_or(0),
        cpu_usec: flat(&f.cpu_stat, "usage_usec").unwrap_or(0),
        io_read_bytes,
        io_write_bytes,
    }
}

/// The value of `key` in a flat-keyed file ("key value" lines).
pub fn flat(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let mut parts = line.split_ascii_whitespace();
        (parts.next() == Some(key))
            .then(|| parts.next().and_then(|v| v.parse().ok()))
            .flatten()
    })
}

/// The value of a `key=value` field in a nested-keyed line ("8:0 rbytes=1 wbytes=2").
fn keyed(line: &str, key: &str) -> Option<u64> {
    line.split_ascii_whitespace()
        .find_map(|f| f.strip_prefix(key).and_then(|v| v.parse().ok()))
}

/// Whether a cgroup's `cgroup.events` says processes run in it or below it.
pub fn populated(events: &str) -> Option<bool> {
    flat(events, "populated").map(|n| n != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_api::IoRates;
    use proptest::prelude::*;

    #[test]
    fn every_bound_becomes_its_kernel_file_in_kernel_units() {
        let b = Bounds {
            cpu_weight: Some(50),
            cpu_max_milli: Some(1500),
            memory_max: Some(2 << 30),
            memory_high: Some(1 << 30),
            io_weight: Some(25),
            io_latency_us: Some(5000),
            io_max: Some(IoRates {
                write_bps: Some(100 << 20),
                ..IoRates::default()
            }),
            pids_max: Some(4096),
        };
        let disks = [
            Dev {
                major: 259,
                minor: 0,
            },
            Dev {
                major: 8,
                minor: 16,
            },
        ];
        let got: Vec<(&str, String)> = writes(&b, &disks)
            .unwrap()
            .into_iter()
            .map(|w| (w.file, w.value))
            .collect();
        let want: Vec<(&str, String)> = vec![
            ("cpu.weight", "50".into()),
            ("cpu.max", "150000 100000".into()),
            ("memory.max", "2147483648".into()),
            ("memory.swap.max", "0".into()),
            ("memory.high", "1073741824".into()),
            ("io.weight", "default 25".into()),
            ("io.latency", "259:0 target=5000".into()),
            (
                "io.max",
                "259:0 rbps=max wbps=104857600 riops=max wiops=max".into(),
            ),
            ("io.latency", "8:16 target=5000".into()),
            (
                "io.max",
                "8:16 rbps=max wbps=104857600 riops=max wiops=max".into(),
            ),
            ("pids.max", "4096".into()),
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn no_bounds_write_nothing_and_an_invalid_bound_writes_nothing_either() {
        assert_eq!(
            writes(&Bounds::default(), &[Dev { major: 8, minor: 0 }]).unwrap(),
            vec![]
        );
        assert!(
            writes(
                &Bounds {
                    pids_max: Some(0),
                    ..Bounds::default()
                },
                &[]
            )
            .is_err()
        );
    }

    #[test]
    fn usage_reads_the_kernels_files() {
        let f = UsageFiles {
            memory_current: "1048576\n".into(),
            memory_peak: "4194304\n".into(),
            memory_events: "low 0\nhigh 3\nmax 7\noom 2\noom_kill 2\noom_group_kill 0\n".into(),
            pids_current: "5\n".into(),
            pids_events: "max 9\n".into(),
            cpu_stat: "usage_usec 123456\nuser_usec 100000\nsystem_usec 23456\n".into(),
            io_stat: "259:0 rbytes=100 wbytes=200 rios=1 wios=2 dbytes=0 dios=0\n8:16 rbytes=1 wbytes=2 rios=1 wios=1 dbytes=0 dios=0\n".into(),
        };
        let u = usage(&f);
        assert_eq!(
            u,
            Usage {
                memory_bytes: 1 << 20,
                memory_peak_bytes: 4 << 20,
                oom_kills: 2,
                pids: 5,
                pids_refused: 9,
                cpu_usec: 123_456,
                io_read_bytes: 101,
                io_write_bytes: 202,
            }
        );
        assert_eq!(usage(&UsageFiles::default()), Usage::default());
        assert_eq!(populated("populated 1\nfrozen 0\n"), Some(true));
        assert_eq!(populated("populated 0\nfrozen 0\n"), Some(false));
        assert_eq!(populated("garbage"), None);
    }

    proptest! {
        // Hostile input (CLAUDE.md §6): whatever the files hold, reading them returns a value.
        #[test]
        fn usage_of_any_bytes_is_a_value_not_a_panic(a in ".*", b in ".*", c in ".*", d in "\\PC*") {
            let f = UsageFiles { memory_current: a.clone(), memory_peak: b, memory_events: c.clone(), pids_current: a, pids_events: c.clone(), cpu_stat: c, io_stat: d.clone() };
            let _ = usage(&f);
            let _ = populated(&d);
        }

        #[test]
        fn cpu_max_holds_the_asked_share_of_each_period(milli in 10u32..=u32::MAX) {
            let ws = writes(&Bounds { cpu_max_milli: Some(milli), ..Bounds::default() }, &[]).unwrap();
            let v = &ws[0].value;
            let (quota, period) = v.split_once(' ').unwrap();
            prop_assert_eq!(period, "100000");
            prop_assert_eq!(quota.parse::<u64>().unwrap(), u64::from(milli) * 100);
        }
    }
}
