//! The borrowed canonical transaction is the native launcher. The API owns only
//! its existing protocol/drain workers; the native receipt owns scope and reaping.

use std::cell::RefCell;

use super::*;
use crate::devices::{ManagedLaunch, Transaction};
use hex_core::patience::Patience;

/// One create/attach attempt spends one existing measured boot bound. Native
/// launch and the first relay dial consume its remainder rather than restarting it.
#[derive(Clone, Copy)]
pub(super) struct Budget {
    deadline: Instant,
}

impl Budget {
    pub fn new(patience: Duration) -> Result<Self, Refusal> {
        if patience.is_zero() {
            return Err(Refusal::Invalid {
                what: "managed guest budget",
                why: "the measured boot bound must be finite and positive".into(),
            });
        }
        Ok(Self {
            deadline: Instant::now()
                .checked_add(patience)
                .ok_or_else(|| Refusal::Invalid {
                    what: "managed guest budget",
                    why: "the measured boot bound exceeds a finite native deadline".into(),
                })?,
        })
    }

    pub fn remaining(self) -> Result<Duration, Refusal> {
        self.remaining_at(Instant::now())
    }

    pub fn deadline(self) -> Instant {
        self.deadline
    }

    fn remaining_at(self, now: Instant) -> Result<Duration, Refusal> {
        self.deadline
            .checked_duration_since(now)
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| {
                Refusal::os(
                    "spending the original managed guest create budget",
                    std::io::ErrorKind::TimedOut,
                )
            })
    }
}

struct Launcher<'a> {
    transaction: &'a dyn Transaction,
    budget: Budget,
    refused: RefCell<Option<Refusal>>,
}

impl Launcher<'_> {
    fn remember<T>(&self, result: Result<T, Refusal>) -> Result<T, shards_api::Refusal> {
        result.map_err(|why| {
            match self.refused.try_borrow_mut() {
                Ok(mut refused) => *refused = Some(why),
                Err(_) => return shards_api::Refusal::Launch(shards_api::LaunchError::Observer),
            }
            shards_api::Refusal::Launch(shards_api::LaunchError::NativeRefused)
        })
    }
}

impl shards_api::ManagedLauncher for Launcher<'_> {
    fn launch(
        &mut self,
        plan: &shards_api::PreparedBoot,
        io: shards_api::LaunchIo,
    ) -> Result<shards_api::ManagedReceipt, shards_api::Refusal> {
        self.remember(self.budget.remaining().and_then(|remaining| {
            self.transaction
                .launch_managed(&ManagedLaunch::Shards(plan), io, remaining)
        }))
    }

    fn attach(
        &mut self,
        identity: &shards_api::Identity,
    ) -> Result<shards_api::ManagedReceipt, shards_api::Refusal> {
        self.remember(self.transaction.attach_managed(Some(identity)))
    }
}

fn launch<T>(
    transaction: &dyn Transaction,
    budget: Budget,
    operation: impl FnOnce(&mut dyn shards_api::ManagedLauncher) -> Result<T, shards_api::Refusal>,
) -> Result<T, Refusal> {
    let mut launcher = Launcher {
        transaction,
        budget,
        refused: RefCell::new(None),
    };
    let result = operation(&mut launcher);
    if let Some(why) = launcher.refused.into_inner() {
        return Err(why);
    }
    result.map_err(|why| refusal("managed Shards native scope", &why))
}

pub(super) fn boot(
    transaction: &dyn Transaction,
    plan: shards_api::PreparedBoot,
    budget: Budget,
) -> Result<shards_api::ManagedGuest, Refusal> {
    budget.remaining()?;
    launch(transaction, budget, |launcher| {
        shards_api::boot_managed(plan, launcher)
    })
}

pub(super) fn attach(
    transaction: &dyn Transaction,
    plan: shards_api::PreparedBoot,
    identity: &shards_api::Identity,
    budget: Budget,
) -> Result<shards_api::ManagedGuest, Refusal> {
    budget.remaining()?;
    launch(transaction, budget, |launcher| {
        shards_api::attach_managed(plan, identity, launcher)
    })
}

pub(super) fn same_init(
    record: &hex_core::record::BoxRecord,
    identity: &shards_api::Identity,
) -> Result<(), Refusal> {
    same_native(hex_core::record::NativeRecord::Box(record), identity)
}

pub(super) fn same_native(
    record: hex_core::record::NativeRecord<'_>,
    identity: &shards_api::Identity,
) -> Result<(), Refusal> {
    if record.scope().is_none()
        || identity.pid == 0
        || identity.started == 0
        || record.init()
            != Some(hex_core::record::ProcId {
                pid: identity.pid,
                start_ticks: identity.started,
            })
    {
        return Err(record.uncertain("its published controller isn't its record's"));
    }
    Ok(())
}

/// A managed scope's stop (decision 9; ROOT, 2026-11-13): from its stop sent until its scope's end is seen, a wait of
/// its own, measuring, never a boot's record. The slowest stop this process has seen bounds it. Before its first, and
/// from a run-out (told) on, it follows the scope by the scope's own party: `ManagedProcess::wait_for` with
/// `Duration::MAX` lasts until the scope's end, or refuses once its observer fails (on macOS the helper that keeps the
/// scope, or the scope's watcher in it), so no wait is given up on a guess while the party that ends it lives.
static STOP: Patience = Patience::measuring("managed.stop");

/// A missing controller never prevents cleanup of its already-owned scope.
/// Only Released, including every helper, permits canonical record removal.
pub(super) fn stop(
    transaction: &dyn Transaction,
    id: &BoxId,
    identity: Option<&shards_api::Identity>,
) -> Result<shards_api::ManagedEnd, Refusal> {
    stop_scope(transaction, identity, super::books::uncertain(id)?)
}

/// Stops the managed scope `identity` names (or the one with none published yet) and waits for its end ([`STOP`]): its
/// end once Released. A stop that wasn't sent follows nothing (nothing would end the scope it waits on): the scope as
/// it is then, or the stop's refusal.
pub(super) fn stop_scope(
    transaction: &dyn Transaction,
    identity: Option<&shards_api::Identity>,
    uncertain: Refusal,
) -> Result<shards_api::ManagedEnd, Refusal> {
    let process = transaction.attach_managed(identity)?;
    let before = process.terminal();
    if let Ok(Some(end)) = &before
        && end.scope == shards_api::TerminalScope::Released
    {
        return Ok(*end);
    }
    let began = Instant::now();
    if let Err(why) = process.stop() {
        return match process.terminal() {
            Ok(Some(end)) if end.scope == shards_api::TerminalScope::Released => Ok(end),
            _ => Err(refusal("stopping managed Shards scope", &why)),
        };
    }
    let wait = |patience| {
        process
            .wait_for(patience)
            .map_err(|why| refusal("observing complete Shards scope termination", &why))
    };
    let observed = match STOP.deadline() {
        Some(bound) => match wait(bound)? {
            Some(end) => Some(end),
            None => {
                STOP.ran_out(bound);
                wait(Duration::MAX)?
            }
        },
        None => wait(Duration::MAX)?,
    };
    if let Some(end) = observed.filter(|end| end.scope == shards_api::TerminalScope::Released) {
        STOP.answered(began.elapsed());
        return Ok(end);
    }
    if let Err(why) = before {
        return Err(refusal("observing original managed native scope", &why));
    }
    Err(uncertain)
}

/// What holds a managed VM's CPU to the rate its node's learner set (joint plan step 14, D255).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagedRate {
    /// Its VM's governor (Shards' D191, D255): the rate it holds the VM's vCPUs to, none where nothing holds them, and
    /// the period it holds it over, as its VM process answered.
    Governor {
        held: Option<std::num::NonZeroU64>,
        period: Option<Duration>,
    },
    /// Its VM process takes no rate: its platform's cap is to hold it; why, in its words.
    Platform(Refusal),
}

/// A managed VM's rate's ask (decision 9; D255; ROOT, 2026-11-13), from its ask until its VM process answers: a wait
/// of its own, measuring. Shards follows the VM with no bound of Hex's (`Duration::MAX`: until its process answers or
/// the guest ends). Past the slowest answer seen, a watcher tells the run-out while the ask goes on, which is refused
/// only at the VM's end.
static RATE: Patience = Patience::measuring("managed.rate");

/// Joint plan step 14 (D255; ROOT, 2026-11-13): asks managed guest `guest`'s VM's governor, box `id`'s, to hold its
/// vCPUs to `rate` milli-CPU, through Shards on its VM process's socket ([`RATE`]). Its answer: the governor holds it,
/// over its period; a refusal in the process's words; a process that takes no rate, which its platform's cap is to
/// hold, told why; or the VM's end.
pub(super) fn rate(
    guest: &shards_api::ManagedGuest,
    id: &BoxId,
    rate: u64,
) -> Result<ManagedRate, Refusal> {
    let asked = std::num::NonZeroU64::new(rate);
    let began = Instant::now();
    let answer = match RATE.deadline() {
        Some(bound) => told_past(bound, || guest.rate(asked, Duration::MAX))?,
        None => guest.rate(asked, Duration::MAX),
    };
    let took = began.elapsed();
    match answer {
        Ok(held) => {
            RATE.answered(took);
            Ok(ManagedRate::Governor {
                held,
                period: guest.rate_period(),
            })
        }
        Err(shards_api::Refusal::Rate(why)) => {
            RATE.refused(took);
            Err(Refusal::Invalid {
                what: "a managed VM's rate",
                why,
            })
        }
        Err(shards_api::Refusal::Unsupported { why, .. }) => {
            Ok(ManagedRate::Platform(Refusal::Platform {
                what: "a managed VM's governor",
                platform: std::env::consts::OS,
                why,
            }))
        }
        Err(shards_api::Refusal::Ended(_)) => Err(Refusal::NotRunning(id.try_clone()?, "rate")),
        Err(why) => Err(refusal("asking a managed VM's governor its rate", &why)),
    }
}

/// Answers `ask`, a watcher telling [`RATE`]'s run-out once `bound` passes with no answer while the ask goes on.
fn told_past<T>(bound: Duration, ask: impl FnOnce() -> T) -> Result<T, Refusal> {
    let (answered, waiting) = hex_wait::mpsc::sync_channel::<()>(1).map_err(|why| match why {
        hex_wait::Failed::Poisoned => {
            Refusal::Broken("a managed VM rate's watch is poisoned".to_owned())
        }
        hex_wait::Failed::Os(why) => Refusal::os("making a managed VM rate's watch", why),
    })?;
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("hex-managed-rate".to_owned())
            .spawn_scoped(scope, move || {
                if matches!(
                    waiting.recv_timeout(bound),
                    Err(hex_wait::mpsc::RecvTimeoutError::Timeout)
                ) {
                    RATE.ran_out(bound);
                }
            })
            .map_err(|why| Refusal::os("starting a managed VM rate's watcher", why))?;
        let answer = ask();
        drop(answered);
        Ok(answer)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_core::record::{BoxRecord, Phase, ProcId};
    #[cfg(unix)]
    use std::sync::Condvar;

    #[cfg(unix)]
    /// A managed VM process's receipt as its launcher keeps it (D255): its identity, running until it is stopped.
    struct Running {
        identity: shards_api::Identity,
        stopped: Mutex<bool>,
        changed: Condvar,
    }

    #[cfg(unix)]
    impl shards_api::ManagedProcess for Running {
        fn identity(&self) -> Result<&shards_api::Identity, shards_api::Refusal> {
            Ok(&self.identity)
        }
        fn wait_for(
            &self,
            patience: Duration,
        ) -> Result<Option<shards_api::ManagedEnd>, shards_api::Refusal> {
            let stopped = self.stopped.lock().unwrap();
            let stopped = *if patience == Duration::MAX {
                self.changed
                    .wait_while(stopped, |stopped| !*stopped)
                    .unwrap()
            } else {
                self.changed
                    .wait_timeout_while(stopped, patience, |stopped| !*stopped)
                    .unwrap()
                    .0
            };
            Ok(if stopped {
                Some(shards_api::ManagedEnd {
                    status: shards_api::ManagedStatus::Unknown,
                    scope: shards_api::TerminalScope::Released,
                })
            } else {
                None
            })
        }
        fn stop(&self) -> Result<(), shards_api::Refusal> {
            *self.stopped.lock().unwrap() = true;
            self.changed.notify_all();
            Ok(())
        }
        fn terminal(&self) -> Result<Option<shards_api::ManagedEnd>, shards_api::Refusal> {
            self.wait_for(Duration::ZERO)
        }
    }

    #[cfg(unix)]
    /// Launches `running` as its managed VM process.
    struct Launches(triomphe::Arc<Running>);

    #[cfg(unix)]
    impl shards_api::ManagedLauncher for Launches {
        fn launch(
            &mut self,
            _: &shards_api::PreparedBoot,
            _: shards_api::LaunchIo,
        ) -> Result<shards_api::ManagedReceipt, shards_api::Refusal> {
            Ok(shards_api::managed_receipt(triomphe::Arc::clone(&self.0)))
        }
        fn attach(
            &mut self,
            _: &shards_api::Identity,
        ) -> Result<shards_api::ManagedReceipt, shards_api::Refusal> {
            Ok(shards_api::managed_receipt(triomphe::Arc::clone(&self.0)))
        }
    }

    #[cfg(unix)]
    /// A managed guest booted through Shards (`shards_api::boot_managed`) whose VM process's socket is `socket`, where a
    /// test's stand-in VM process listens; and its receipt.
    fn managed_at(socket: &std::path::Path) -> (shards_api::ManagedGuest, triomphe::Arc<Running>) {
        let spec = shards_api::GuestSpec {
            kernel: "/k/Image".into(),
            init: shards_api::Init::Executable("/k/init".into()),
            cmdline: "console=ttyS0 quiet".into(),
            vcpus: 2,
            memory_mib: 256,
            gpu: false,
            gpu_devices: Vec::new(),
            disks: Vec::new(),
            shares: Vec::new(),
            ports: vec![1024],
            binary: None,
        };
        let plan =
            shards_api::prepare_boot(&spec, std::path::Path::new("/never/shards"), socket, 0)
                .unwrap();
        let running = triomphe::Arc::try_new(Running {
            identity: shards_api::Identity {
                pid: 42,
                started: 7,
                vsock: socket.to_owned(),
            },
            stopped: Mutex::new(false),
            changed: Condvar::new(),
        })
        .unwrap();
        let guest =
            shards_api::boot_managed(plan, &mut Launches(triomphe::Arc::clone(&running))).unwrap();
        (guest, running)
    }

    /// A stand-in managed VM process at `path` (Shards' D255 protocol, as its own test's stand-in): for each of
    /// `answers` in turn, takes one dial, reads its ask's line and answers it, or closes it unanswered (none); gives
    /// back the asks' lines.
    #[cfg(unix)]
    fn vm_process(
        path: &std::path::Path,
        answers: Vec<Option<&'static [u8]>>,
    ) -> std::thread::JoinHandle<Vec<Vec<u8>>> {
        use std::io::{Read as _, Write as _};
        let listener = std::os::unix::net::UnixListener::bind(path).unwrap();
        std::thread::spawn(move || {
            let mut asks = Vec::new();
            for answer in answers {
                let (mut dial, _) = listener.accept().unwrap();
                let mut line = Vec::new();
                let mut byte = [0u8; 1];
                while line.last() != Some(&b'\n') {
                    assert_eq!(dial.read(&mut byte).unwrap(), 1, "an ask's whole line");
                    line.push(byte[0]);
                }
                asks.push(line);
                if let Some(answer) = answer {
                    dial.write_all(answer).unwrap();
                }
            }
            asks
        })
    }

    /// Joint plan step 14 (D255; AC-T7.4.1; ROOT, 2026-11-13): a managed VM's rate is asked of its governor through
    /// Shards, on its VM process's socket, and held as that process answers. Do ask a managed guest, booted through
    /// Shards with a stand-in VM process at its socket, for 4 CPUs four times: the process answering it held over a 100
    /// ms period; refusing it, the microVM not started; closing the ask unanswered, as a VM process that takes no rate
    /// does; then, its process ended, asked again. Expect each ask `RATE 4000`; the governor holding 4000 milli-CPU over
    /// 100 ms; a refusal in the process's words; the platform's cap to hold it, told why; and the VM's end.
    #[cfg(unix)]
    #[test]
    fn w14_d255_a_managed_vms_rate_is_asked_of_its_governor_and_held_as_it_answers() {
        let dir = super::super::tests::socket_fixture();
        let socket = dir.path().join("v");
        let vm = vm_process(
            &socket,
            vec![
                Some(b"RATED 4000 100000000\n"),
                Some(b"REFUSED the microVM has not started\n"),
                None,
            ],
        );
        let (guest, running) = managed_at(&socket);
        let id = BoxId::new("vm-1").unwrap();
        assert_eq!(
            rate(&guest, &id, 4_000),
            Ok(ManagedRate::Governor {
                held: std::num::NonZeroU64::new(4_000),
                period: Some(Duration::from_millis(100)),
            })
        );
        assert_eq!(
            rate(&guest, &id, 4_000),
            Err(Refusal::Invalid {
                what: "a managed VM's rate",
                why: "the microVM has not started".to_owned(),
            })
        );
        let platform = rate(&guest, &id, 4_000);
        assert!(
            matches!(&platform, Ok(ManagedRate::Platform(Refusal::Platform { why, .. })) if !why.is_empty()),
            "{platform:?}"
        );
        assert_eq!(vm.join().unwrap(), vec![b"RATE 4000\n".to_vec(); 3]);
        shards_api::ManagedProcess::stop(running.as_ref()).unwrap();
        shards_api::Guest::wait_for(&guest, Duration::MAX).unwrap();
        assert_eq!(
            rate(&guest, &id, 4_000),
            Err(Refusal::NotRunning(id.try_clone().unwrap(), "rate"))
        );
    }

    /// AC-T7.4.1: a VM becomes usable only after the native launcher's
    /// before-resume publication matches its actual PID and birth exactly.
    #[test]
    fn canonical_native_publication_must_match_the_managed_controller() {
        let spec = super::super::tests::oci(hex_api::Rootfs::Dir("/rootfs".into()), Vec::new());
        let mut record = BoxRecord::new(spec, Phase::Creating, Vec::new());
        record.native_scope = Some(hex_core::record::NativeScope::from_bytes([1; 16]));
        let identity = shards_api::Identity {
            pid: 42,
            started: 7,
            vsock: "/scope/v".into(),
        };
        assert!(same_init(&record, &identity).is_err());
        record.init = Some(ProcId {
            pid: 42,
            start_ticks: 7,
        });
        same_init(&record, &identity).unwrap();
        record.init = Some(ProcId {
            pid: 42,
            start_ticks: 8,
        });
        assert!(same_init(&record, &identity).is_err());
    }

    /// AC-T7.4.1: no stage restarts a create budget after scope publication.
    #[test]
    fn managed_budget_is_one_absolute_finite_deadline() {
        assert!(Budget::new(Duration::ZERO).is_err());
        assert!(Budget::new(Duration::MAX).is_err());
        let start = Instant::now();
        let deadline = start.checked_add(Duration::from_secs(1)).unwrap();
        let budget = Budget { deadline };
        assert_eq!(budget.remaining_at(start).unwrap(), Duration::from_secs(1));
        assert!(budget.remaining_at(deadline).is_err());
        assert!(
            budget
                .remaining_at(deadline.checked_add(Duration::from_nanos(1)).unwrap())
                .is_err()
        );
    }
}
