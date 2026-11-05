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
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hex_health::probe::DiskProbe;
use hex_health::{HealthError, Level};

/// The record written each period: one page, so each probe is one block write and its flush.
pub const RECORD_BYTES: usize = 4096;
/// The clock's granularity G (RFC 6298 §2) the judge sees: `Instant` in nanoseconds, read at a
/// millisecond's resolution by a node agent's timers.
pub const GRANULARITY_NS: u64 = 1_000_000;

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
}

/// What the prober and the judge share.
#[derive(Debug)]
struct Shared {
    probe: Mutex<DiskProbe>,
    /// Set once the judge saw `Drain`; never cleared (a node is uncordoned by a person or the control
    /// plane, not by the disk answering again).
    cordoned: AtomicBool,
    stop: AtomicBool,
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
    pub fn start(state_dir: &Path, period: Duration) -> Result<Self, ProbeError> {
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
            probe: Mutex::new(DiskProbe::new(GRANULARITY_NS)),
            cordoned: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            failed: Mutex::new(None),
        });
        let epoch = Instant::now();
        let ours = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("hex-disk-probe".into())
            .spawn(move || {
                let record = [0u8; RECORD_BYTES];
                if let Err(e) = probe_loop(&ours, epoch, period, || {
                    write_record(&mut file, &record)
                }) {
                    if let Ok(mut f) = ours.failed.lock() {
                        *f = Some(e.to_string());
                    }
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
        let now = since(self.epoch)?;
        let level = lock(&self.shared.probe)?.level(now)?;
        if level == Level::Drain {
            self.shared.cordoned.store(true, Ordering::Release);
        }
        Ok(level)
    }

    /// Whether the node may take new work: false once a judgement saw `Drain`.
    pub fn schedulable(&self) -> bool {
        !self.shared.cordoned.load(Ordering::Acquire)
    }

    /// The probe's current timeout, for a record of the drill.
    pub fn timeout(&self) -> Result<Duration, ProbeError> {
        Ok(Duration::from_nanos(lock(&self.shared.probe)?.timeout_ns()?))
    }

}

impl Drop for DiskProber {
    /// The prober stops after its current write. A write stalled on the disk holds its thread until
    /// the disk answers, so the thread is left to end on its own rather than joined here.
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
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
    while !shared.stop.load(Ordering::Acquire) {
        let began = Instant::now();
        lock(&shared.probe)?.start(since(epoch)?)?;
        write().map_err(ProbeError::Write)?;
        lock(&shared.probe)?.finish(since(epoch)?)?;
        std::thread::sleep(period.saturating_sub(began.elapsed()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn shared() -> Arc<Shared> {
        Arc::new(Shared {
            probe: Mutex::new(DiskProbe::new(GRANULARITY_NS)),
            cordoned: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            failed: Mutex::new(None),
        })
    }

    /// AC-T7.10.3: do a probe whose writes complete, then stall one (the write blocks until released);
    /// expect `Healthy` while it answers, then `Suspect`, then `Drain` while the write is still
    /// blocked, and the node cordoned. Waits on the levels themselves, each by its own deadline.
    #[test]
    fn a_stalled_write_is_judged_while_it_blocks_and_cordons_the_node() {
        let s = shared();
        let epoch = Instant::now();
        let (release, held) = mpsc::channel::<()>();
        let stall_after = 20u32;
        let ours = Arc::clone(&s);
        let thread = std::thread::spawn(move || {
            let mut n = 0u32;
            probe_loop(&ours, epoch, Duration::from_millis(5), || {
                n += 1;
                if n > stall_after {
                    held.recv().ok();
                }
                Ok(())
            })
        });
        let prober = DiskProber {
            shared: Arc::clone(&s),
            epoch,
            thread: None,
        };
        let wait_for = |want: Level, within: Duration| {
            let end = Instant::now() + within;
            loop {
                let level = prober.level().unwrap();
                if level == want {
                    return;
                }
                assert!(Instant::now() < end, "never reached {want:?}, at {level:?}");
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        assert_eq!(prober.level().unwrap(), Level::Healthy);
        assert!(prober.schedulable());
        // One timeout (at least RFC 6298's 1 s) to Suspect, three to Drain, with room.
        wait_for(Level::Suspect, Duration::from_secs(5));
        assert!(prober.schedulable());
        wait_for(Level::Drain, Duration::from_secs(10));
        assert!(!prober.schedulable());
        s.stop.store(true, Ordering::Release);
        release.send(()).unwrap();
        thread.join().unwrap().unwrap();
        // Cordoned stays cordoned once the disk answers again.
        assert!(!prober.schedulable());
    }

    /// AC-T7.10.3, hostile input: do start the probe on a directory that doesn't exist; expect a typed
    /// refusal naming the record, not a panic.
    #[test]
    fn a_missing_state_dir_is_refused_typed() {
        let dir = tempfile::tempdir().unwrap();
        let err = DiskProber::start(&dir.path().join("absent"), Duration::from_millis(100));
        assert!(matches!(err, Err(ProbeError::Record { .. })));
    }

    /// A real disk answers: do probe a temporary directory and judge it across 20 periods; expect
    /// `Healthy` at every judgement and the node schedulable.
    #[test]
    fn a_disk_that_answers_stays_healthy() {
        let dir = tempfile::tempdir().unwrap();
        let p = DiskProber::start(dir.path(), Duration::from_millis(10)).unwrap();
        for _ in 0..20 {
            assert_eq!(p.level().unwrap(), Level::Healthy);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(p.schedulable());
    }
}
