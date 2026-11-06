//! The node's own disk probe (`AC-T7.10.3`; `docs/design/failure-isolation.md` F3): a thread writes
//! and flushes a small record on the node's state disk every period, and the node judges the
//! outstanding write's age with [`hex_health::probe::DiskProbe`]. A write that blocks blocks only the
//! prober's thread: the judge reads when it began, so a stalled disk is seen while it stalls, not when
//! the write at last returns. At `Drain` the node cordons itself: it takes no new work and its work is
//! moved.
//!
//! Bounds (CLAUDE.md §2): one write outstanding at a time, one fixed-size record, one thread.

use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hex_health::probe::DiskProbe;
use hex_health::{HealthError, Level};

/// The record written each period: one byte, the least write that dirties a block, so the flush
/// must write that block to the device (fdatasync(2)); nothing about the disk is assumed.
pub const RECORD_BYTES: usize = 1;

/// The slowest probe answer recorded for this host class: the seed of the probe's learned wait (Hex's
/// rule for learned waits). macOS: `bench/health/records/2026-11-06-probe-floor-apfs.md`, APFS on an
/// M4 Max, the slowest of 27,655 probes over two runs. A host class with no record is refused, typed:
/// its distribution is recorded first (`examples/fsync_latency`).
pub fn recorded_slowest() -> Result<Duration, ProbeError> {
    if cfg!(target_os = "macos") {
        Ok(Duration::from_micros(147_330))
    } else {
        Err(ProbeError::NoRecord(std::env::consts::OS))
    }
}

/// Every way the probe refuses, typed (CLAUDE.md §3).
#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("the disk probe's record {path}: {source}")]
    Record {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("the disk probe's write and flush: {0}")]
    Write(std::io::Error),
    #[error(transparent)]
    Health(#[from] HealthError),
    #[error("the disk probe's state lock was poisoned by a thread that panicked")]
    Poisoned,
    #[error("the disk probe's thread could not start: {0}")]
    Spawn(std::io::Error),
    #[error("the disk probe's thread ended: {0}")]
    Ended(String),
    #[error("the clock passed u64 nanoseconds")]
    Clock,
    #[error("no recorded disk-probe distribution for {0}: record one first")]
    NoRecord(&'static str),
}

/// What the prober and the judge share.
#[derive(Debug)]
struct Shared {
    probe: Mutex<DiskProbe>,
    /// Set once the judge saw `Drain`; never cleared (a node is uncordoned by a person or the control
    /// plane, not by the disk answering again).
    cordoned: AtomicBool,
    /// Set to stop the prober; `wake` tells it at once rather than at the end of its period.
    stop: Mutex<bool>,
    wake: Condvar,
    /// The prober's last write error, if it ended on one.
    failed: Mutex<Option<String>>,
}

/// A running probe: its thread, and the judge the node agent asks.
#[derive(Debug)]
pub struct DiskProber {
    shared: Arc<Shared>,
    epoch: Instant,
    thread: Option<JoinHandle<()>>,
}

fn lock<T>(m: &Mutex<T>) -> Result<MutexGuard<'_, T>, ProbeError> {
    m.lock().map_err(|_| ProbeError::Poisoned)
}

fn since(epoch: Instant) -> Result<u64, ProbeError> {
    u64::try_from(epoch.elapsed().as_nanos()).map_err(|_| ProbeError::Clock)
}

/// One probe: rewrite the record in place and flush it to the disk.
fn write_record(file: &mut File, record: &[u8]) -> std::io::Result<()> {
    file.seek(SeekFrom::Start(0))?;
    file.write_all(record)?;
    file.sync_data()
}

impl DiskProber {
    /// Start probing the disk under `state_dir` every `period`, its record at `state_dir/.hex-probe`.
    pub fn start(
        state_dir: &Path,
        period: Duration,
        recorded_slowest: Duration,
    ) -> Result<Self, ProbeError> {
        let seed = u64::try_from(recorded_slowest.as_nanos()).map_err(|_| ProbeError::Clock)?;
        let probe = DiskProbe::new(seed)?;
        let path = state_dir.join(".hex-probe");
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|source| ProbeError::Record {
                path: path.clone(),
                source,
            })?;
        let shared = Arc::new(Shared {
            probe: Mutex::new(probe),
            cordoned: AtomicBool::new(false),
            stop: Mutex::new(false),
            wake: Condvar::new(),
            failed: Mutex::new(None),
        });
        let epoch = Instant::now();
        let ours = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("hex-disk-probe".into())
            .spawn(move || {
                let record = [0u8; RECORD_BYTES];
                if let Err(e) =
                    probe_loop(&ours, epoch, period, || write_record(&mut file, &record))
                    && let Ok(mut f) = ours.failed.lock()
                {
                    *f = Some(e.to_string());
                }
            })
            .map_err(ProbeError::Spawn)?;
        Ok(Self {
            shared,
            epoch,
            thread: Some(thread),
        })
    }

    /// Judge the disk now: the outstanding write's age and the completed probes' latencies, and cordon
    /// the node at `Drain`. Refuses once the prober's thread ended on an error.
    pub fn level(&self) -> Result<Level, ProbeError> {
        if let Some(e) = lock(&self.shared.failed)?.as_ref() {
            return Err(ProbeError::Ended(e.clone()));
        }
        // The clock is read under the lock: read before it, the prober could finish a probe stamped
        // later in between, and the judgement's time would run backwards.
        let probe = lock(&self.shared.probe)?;
        let level = probe.level(since(self.epoch)?)?;
        drop(probe);
        if level == Level::Drain {
            self.shared.cordoned.store(true, Ordering::Release);
        }
        Ok(level)
    }

    /// Whether the node may take new work: false once a judgement saw `Drain`.
    /// Whether the node may take new work: false once a judgement saw `Drain`.
    pub fn schedulable(&self) -> bool {
        !self.shared.cordoned.load(Ordering::Acquire)
    }

    /// The probe's current bound, past which an outstanding write drains the node, for a record of
    /// the drill.
    pub fn bound(&self) -> Result<Duration, ProbeError> {
        Ok(Duration::from_nanos(lock(&self.shared.probe)?.bound_ns()?))
    }

    /// Stop the prober after its current write.
    fn stop(&self) {
        if let Ok(mut stop) = self.shared.stop.lock() {
            *stop = true;
        }
        self.shared.wake.notify_all();
    }
}

impl Drop for DiskProber {
    /// The prober stops after its current write. A write stalled on the disk holds its thread until
    /// the disk answers, so the thread is left to end on its own rather than joined here.
    fn drop(&mut self) {
        self.stop();
        drop(self.thread.take());
    }
}

/// The prober: start a probe, write, finish it, wait out the period; until asked to stop.
fn probe_loop(
    shared: &Shared,
    epoch: Instant,
    period: Duration,
    mut write: impl FnMut() -> std::io::Result<()>,
) -> Result<(), ProbeError> {
    loop {
        let began = Instant::now();
        {
            let mut probe = lock(&shared.probe)?;
            probe.start(since(epoch)?)?;
        }
        write().map_err(ProbeError::Write)?;
        {
            let mut probe = lock(&shared.probe)?;
            probe.finish(since(epoch)?)?;
        }
        let stop = lock(&shared.stop)?;
        let (stop, _) = shared
            .wake
            .wait_timeout_while(stop, period.saturating_sub(began.elapsed()), |s| !*s)
            .map_err(|_| ProbeError::Poisoned)?;
        if *stop {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// The macOS/APFS record's slowest answer, the seed every test runs on, whatever its host: the
    /// mechanics under test don't depend on the disk.
    const SEED_NS: u64 = 147_330_000;

    /// Wait `d` between judgements: on a channel nothing sends on, as `thread::sleep` is disallowed.
    fn pause(d: Duration) {
        let (_keep, rx) = mpsc::channel::<()>();
        let _ = rx.recv_timeout(d);
    }

    fn shared() -> Arc<Shared> {
        Arc::new(Shared {
            probe: Mutex::new(DiskProbe::new(SEED_NS).unwrap()),
            cordoned: AtomicBool::new(false),
            stop: Mutex::new(false),
            wake: Condvar::new(),
            failed: Mutex::new(None),
        })
    }

    /// Run the prober over `write`, stalling its write number `stall_at` (0 the first) until released;
    /// returns the judge, the release and the thread.
    fn stalling(
        stall_at: u32,
    ) -> (
        DiskProber,
        mpsc::Sender<()>,
        JoinHandle<Result<(), ProbeError>>,
    ) {
        let s = shared();
        let epoch = Instant::now();
        let (release, held) = mpsc::channel::<()>();
        let ours = Arc::clone(&s);
        let thread = std::thread::spawn(move || {
            let mut n = 0u32;
            probe_loop(&ours, epoch, Duration::from_millis(5), || {
                if n == stall_at {
                    held.recv().ok();
                }
                n += 1;
                Ok(())
            })
        });
        let prober = DiskProber {
            shared: s,
            epoch,
            thread: None,
        };
        (prober, release, thread)
    }

    /// Wait until `prober` judges `want`, failing past `within`.
    fn wait_for(prober: &DiskProber, want: Level, within: Duration) {
        let end = Instant::now() + within;
        loop {
            let level = prober.level().unwrap();
            if level == want {
                return;
            }
            assert!(Instant::now() < end, "never reached {want:?}, at {level:?}");
            pause(Duration::from_millis(5));
        }
    }

    /// AC-T7.10.3: do a probe whose writes answer, then stall one (it blocks until released); expect
    /// `Suspect` and then `Drain` while it is still blocked, and the node cordoned, and cordoned still
    /// once the disk answers again.
    #[test]
    fn a_stalled_write_is_judged_while_it_blocks_and_cordons_the_node() {
        let (prober, release, thread) = stalling(20);
        assert!(prober.schedulable());
        // Suspect past the record's 147 ms, Drain past its 295 ms bound, with room.
        wait_for(&prober, Level::Suspect, Duration::from_secs(5));
        assert!(prober.schedulable());
        wait_for(&prober, Level::Drain, Duration::from_secs(10));
        assert!(!prober.schedulable());
        prober.stop();
        release.send(()).unwrap();
        thread.join().unwrap().unwrap();
        assert!(!prober.schedulable());
    }

    /// AC-T7.10.3, Hex's rule for learned waits, before any sample: do stall the very first write;
    /// expect it judged against the record's bound (no calibration, no unbounded first wait): drained
    /// past twice the record's slowest, the node cordoned.
    #[test]
    fn the_first_write_is_bounded_by_the_record() {
        let (prober, release, thread) = stalling(0);
        assert_eq!(prober.bound().unwrap(), Duration::from_nanos(2 * SEED_NS));
        wait_for(&prober, Level::Drain, Duration::from_secs(10));
        assert!(!prober.schedulable());
        prober.stop();
        release.send(()).unwrap();
        thread.join().unwrap().unwrap();
    }

    /// AC-T7.10.3, hostile input: do start the probe on a directory that doesn't exist; expect a typed
    /// refusal naming the record, not a panic.
    #[test]
    fn a_missing_state_dir_is_refused_typed() {
        let dir = tempfile::tempdir().unwrap();
        let err = DiskProber::start(
            &dir.path().join("absent"),
            Duration::from_millis(100),
            Duration::from_nanos(SEED_NS),
        );
        assert!(matches!(err, Err(ProbeError::Record { .. })));
    }

    /// A real disk answers: do probe a temporary directory at this host class's recorded seed across
    /// 20 periods; expect it never drained, the node schedulable, and the record one byte. A host class
    /// with no record is refused, typed, naming it (record its distribution first).
    #[test]
    fn a_disk_that_answers_is_never_drained() {
        let seed = match recorded_slowest() {
            Ok(seed) => seed,
            Err(ProbeError::NoRecord(os)) => {
                assert_eq!(os, std::env::consts::OS);
                return;
            }
            Err(other) => panic!("{other}"),
        };
        let dir = tempfile::tempdir().unwrap();
        let p = DiskProber::start(dir.path(), Duration::from_millis(10), seed).unwrap();
        for _ in 0..20 {
            assert_ne!(p.level().unwrap(), Level::Drain);
            pause(Duration::from_millis(10));
        }
        assert!(p.schedulable());
        let len = std::fs::metadata(dir.path().join(".hex-probe"))
            .unwrap()
            .len();
        assert_eq!(len, 1);
    }
}
