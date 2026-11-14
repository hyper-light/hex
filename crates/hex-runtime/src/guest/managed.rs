//! The borrowed canonical transaction is the native launcher. The API owns only
//! its existing protocol/drain workers; the native receipt owns scope and reaping.

use std::cell::RefCell;

use super::*;
use crate::devices::{ManagedLaunch, Transaction};
use hex_core::patience::{Patience, Piece};

/// One create/attach attempt's budget (decision 9; the review's C-9). It follows its guest: no deadline of its own, each
/// wait in it (native launch, first relay dial, join) waiting within its own W* and past it following its party. A GPU
/// create's alone is finite ([`gpu_create`]); its native launch, report and first relay dial spend its remainder rather
/// than restart it.
#[derive(Clone, Copy)]
pub(super) struct Budget {
    deadline: Option<Instant>,
}

impl Budget {
    /// A budget that follows its guest, with no deadline of its own.
    pub fn following() -> Self {
        Self { deadline: None }
    }

    /// A finite budget, `patience` from now.
    pub fn new(patience: Duration) -> Result<Self, Refusal> {
        if patience.is_zero() {
            return Err(Refusal::Invalid {
                what: "managed guest budget",
                why: "the measured boot bound must be finite and positive".into(),
            });
        }
        Ok(Self {
            deadline: Some(Instant::now().checked_add(patience).ok_or_else(|| {
                Refusal::Invalid {
                    what: "managed guest budget",
                    why: "the measured boot bound exceeds a finite native deadline".into(),
                }
            })?),
        })
    }

    /// What is left of it: none for one that follows; refused once a finite one is spent.
    pub fn remaining(self) -> Result<Option<Duration>, Refusal> {
        self.remaining_at(Instant::now())
    }

    pub fn deadline(self) -> Option<Instant> {
        self.deadline
    }

    fn remaining_at(self, now: Instant) -> Result<Option<Duration>, Refusal> {
        let Some(deadline) = self.deadline else {
            return Ok(None);
        };
        deadline
            .checked_duration_since(now)
            .filter(|remaining| !remaining.is_zero())
            .map(Some)
            .ok_or_else(|| {
                Refusal::os(
                    "spending the original managed guest create budget",
                    std::io::ErrorKind::TimedOut,
                )
            })
    }
}

/// A GPU create's budget: the one place a record still bounds a managed create, owed until Shards' GPU report takes no
/// deadline (the review's C-9; ROOT's ask of Shards, 2026-11-14). Shards binds a VM process's GPU report to a finite
/// absolute create deadline (`PreparedBoot::with_gpu_report`), so a create with devices spends the slowest boot to ready
/// on record for its host class (bench/native/guest/records.tsv's `boot_ready`, [`super::recorded_boot`]), the build's
/// one read of the record. Every other managed create follows its guest ([`Budget::following`]).
pub(super) fn gpu_create() -> Result<Budget, Refusal> {
    Budget::new(super::recorded_boot()?)
}

/// `budget`'s deadline, which a GPU create's report and observer need ([`gpu_create`]): refused by type for one that
/// follows.
pub(super) fn gpu_deadline(budget: Budget) -> Result<Instant, Refusal> {
    budget.deadline().ok_or_else(follows)
}

/// What is left of `budget`, as [`gpu_deadline`] needs it: refused once spent, and by type for one that follows.
pub(super) fn gpu_remaining(budget: Budget) -> Result<Duration, Refusal> {
    budget.remaining()?.ok_or_else(follows)
}

fn follows() -> Refusal {
    Refusal::Invalid {
        what: "a GPU create's budget",
        why: "Shards binds its GPU report to a finite create deadline, and this budget follows its guest"
            .into(),
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
    stop_scope(
        transaction,
        identity,
        super::books::uncertain(
            id,
            "its managed scope's stop ended with its scope unreleased",
        )?,
    )
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
    if let Some(end) = wait_stopped(&*process, began)
        .map_err(|why| refusal("observing complete Shards scope termination", &why))?
    {
        return Ok(end);
    }
    if let Err(why) = before {
        return Err(refusal("observing original managed native scope", &why));
    }
    Err(uncertain)
}

/// A stopped managed scope's end, waited for as its stop's own wait ([`STOP`]) from `began`, its stop sent: within the
/// slowest stop seen; past it, and before the first, the run-out told and the scope followed by its own party
/// (`ManagedProcess::wait_for(Duration::MAX)`) to its end, or refused once its observer fails. Its end seen released is
/// the stop's answer; one seen unreleased, or none, is `None`, for its caller to refuse. Every stop of a managed scope
/// waits this way, a Firecracker VM's and its observer's too (A-13: they waited the configured `stop_ms`).
pub(crate) fn wait_stopped(
    process: &dyn shards_api::ManagedProcess,
    began: Instant,
) -> Result<Option<shards_api::ManagedEnd>, shards_api::Refusal> {
    let observed = match STOP.deadline() {
        Some(bound) => match process.wait_for(bound)? {
            Some(end) => Some(end),
            None => {
                match process.identity() {
                    Ok(identity) => STOP.ran_out(
                        bound,
                        began.elapsed(),
                        &[
                            Piece::Kind("managed scope pid "),
                            Piece::Pid(i64::from(identity.pid)),
                            Piece::Kind(", by its observer"),
                        ],
                    ),
                    Err(_) => STOP.ran_out(
                        bound,
                        began.elapsed(),
                        &[Piece::Kind(
                            "a managed scope with no identity yet, by its observer",
                        )],
                    ),
                }
                process.wait_for(Duration::MAX)?
            }
        },
        None => process.wait_for(Duration::MAX)?,
    };
    let released = observed.filter(|end| end.scope == shards_api::TerminalScope::Released);
    if released.is_some() {
        STOP.answered(began.elapsed());
    }
    Ok(released)
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

/// The cycle a platform's cap holds a managed VM over at a rate (joint plan step 14, C1; 2c-4b), which its node's charge
/// learner's window takes for each VM that cap holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapCycle {
    /// Its run and its hold at the rate, together.
    Of(Duration),
    /// None: the cap never holds a VM at the rate, its ceiling or past it.
    Never,
    /// Not known yet: its control has measured no stop to derive it from, why told (decision 9's cold start).
    Cold(String),
    /// Its run and its hold at a cold estimate of its control's stop response, the slowest of too few holds to have seen
    /// their p99.9: used, and why it is cold told (the review's A-14).
    ColdOf { cycle: Duration, why: String },
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
        Some(bound) => told_past(&RATE, bound, id, || guest.rate(asked, Duration::MAX))?,
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

/// What a managed VM's VM process answered of its vCPUs' guest time (joint plan step 14, D260).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagedGuestTime {
    /// Each of its vCPUs' guest time since its boot, in nanoseconds, by index (`ns`), as its VM process read them after
    /// `asked` and before `answered` (Shards' `ManagedUse`, D260): the instant they hold at is known within that span
    /// (the review's A-11).
    PerVcpu {
        ns: Vec<u64>,
        asked: Instant,
        answered: Instant,
    },
    /// None apart, why in its words: its microVM not started yet, or a VM process that answers no use (one not booted
    /// `--host-managed`, or a shards before D260).
    Unheld(String),
}

/// A managed VM's use's ask (decision 9; D260), from its ask until its VM process answers: a wait of its own,
/// measuring, as a rate's is ([`RATE`]): Shards follows the VM with no bound of Hex's, and past the slowest answer seen
/// a watcher tells the run-out while the ask goes on, which is refused only at the VM's end.
static USE: Patience = Patience::measuring("managed.use");

/// Joint plan step 14 (D260; ROOT, 2026-11-13): managed guest `guest`'s VM's vCPUs' guest time, box `id`'s, `vcpus` of
/// them, as its VM process answers its use on its socket ([`USE`]): each vCPU's, held to the VM's size as an image run's
/// are ([`super::image::vcpus_ns`]); none apart where its microVM hasn't started or its process answers no use, why
/// told; or the VM's end.
pub(super) fn guest_time(
    guest: &shards_api::ManagedGuest,
    id: &BoxId,
    vcpus: u32,
) -> Result<ManagedGuestTime, Refusal> {
    let began = Instant::now();
    let answer = match USE.deadline() {
        Some(bound) => told_past(&USE, bound, id, || guest.host_use(Duration::MAX))?,
        None => guest.host_use(Duration::MAX),
    };
    let took = began.elapsed();
    match answer {
        Ok(shards_api::ManagedUsed::Running(used)) => {
            USE.answered(took);
            let (asked, answered) = (used.asked, used.answered);
            match used.microvm.guest {
                Ok(times) => super::image::vcpus_ns(
                    &times,
                    vcpus,
                    "reading a managed VM's guest time per vCPU",
                )
                .map(|ns| ManagedGuestTime::PerVcpu {
                    ns,
                    asked,
                    answered,
                }),
                Err(why) => Ok(ManagedGuestTime::Unheld(why)),
            }
        }
        Err(shards_api::Refusal::Unsupported { why, .. }) => Ok(ManagedGuestTime::Unheld(why)),
        // Its end, with what its microVM used in all (Shards' D260), or the end alone where it came between: the box
        // no longer runs.
        Ok(shards_api::ManagedUsed::Ended(_)) | Err(shards_api::Refusal::Ended(_)) => {
            Err(Refusal::NotRunning(id.try_clone()?, "host_use"))
        }
        Err(why) => Err(refusal("asking a managed VM's use", &why)),
    }
}

/// Answers `ask`, box `id`'s VM's, a watcher telling `patience`'s run-out once `bound` passes with no answer while the
/// ask goes on.
fn told_past<T>(
    patience: &'static Patience,
    bound: Duration,
    id: &BoxId,
    ask: impl FnOnce() -> T,
) -> Result<T, Refusal> {
    let (answered, waiting) = hex_wait::mpsc::sync_channel::<()>(1).map_err(|why| match why {
        hex_wait::Failed::Poisoned => {
            Refusal::Broken("a managed VM ask's watch is poisoned".to_owned())
        }
        hex_wait::Failed::Os(why) => Refusal::os("making a managed VM ask's watch", why),
    })?;
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("hex-managed-ask".to_owned())
            .spawn_scoped(scope, move || {
                if matches!(
                    waiting.recv_timeout(bound),
                    Err(hex_wait::mpsc::RecvTimeoutError::Timeout)
                ) {
                    patience.ran_out(
                        bound,
                        bound,
                        &[
                            Piece::Kind("the managed VM process of box "),
                            Piece::Name(id.as_str()),
                            Piece::Kind(", its ask on its vsock socket"),
                        ],
                    );
                }
            })
            .map_err(|why| Refusal::os("starting a managed VM ask's watcher", why))?;
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

    /// Joint plan step 14 (D260; AC-T7.4.1; ROOT, 2026-11-13): a managed VM's vCPUs' guest time is asked of its VM process
    /// through Shards, on its socket, and held to the VM's size. Do ask a managed guest of 2 vCPUs, booted through Shards
    /// with a stand-in VM process at its socket, for its use five times: the process answering both vCPUs' times; its
    /// microVM not started; times of 3 vCPUs; closing the ask unanswered, as a VM process that takes no use does; then,
    /// its process ended, asked again. Expect each ask `USE`; the two vCPUs' times; none apart, in the process's words,
    /// before its microVM starts and from a process that answers no use; the count other than the VM's refused, naming
    /// both; and the VM's end.
    #[cfg(unix)]
    #[test]
    fn w14_d260_a_managed_vms_guest_time_is_asked_of_its_vm_process_and_held_to_its_size() {
        let dir = super::super::tests::socket_fixture();
        let socket = dir.path().join("v");
        let vm = vm_process(
            &socket,
            vec![
                Some(b"USED\ndeparted 0\nvcpu 0 1000\nvcpu 1 2000\nhost 5000 4096 8192\n"),
                Some(b"USED\nguest-unread the microVM has not started\nhost 300 4096 8192\n"),
                Some(b"USED\ndeparted 0\nvcpu 0 1\nvcpu 1 2\nvcpu 2 3\nhost 9 4096 8192\n"),
                None,
            ],
        );
        let (guest, running) = managed_at(&socket);
        let id = BoxId::new("vm-1").unwrap();
        let first = guest_time(&guest, &id, 2);
        assert!(
            matches!(&first, Ok(ManagedGuestTime::PerVcpu { ns, asked, answered }) if *ns == [1_000u64, 2_000] && asked <= answered),
            "{first:?}"
        );
        assert_eq!(
            guest_time(&guest, &id, 2),
            Ok(ManagedGuestTime::Unheld(
                "the microVM has not started".to_owned()
            ))
        );
        let three = guest_time(&guest, &id, 2);
        assert!(
            matches!(&three, Err(Refusal::Os { why, .. }) if why.contains("3 vCPUs") && why.contains("2 vCPUs")),
            "{three:?}"
        );
        let unanswered = guest_time(&guest, &id, 2);
        assert!(
            matches!(&unanswered, Ok(ManagedGuestTime::Unheld(why)) if !why.is_empty()),
            "{unanswered:?}"
        );
        assert_eq!(vm.join().unwrap(), vec![b"USE\n".to_vec(); 4]);
        shards_api::ManagedProcess::stop(running.as_ref()).unwrap();
        shards_api::Guest::wait_for(&guest, Duration::MAX).unwrap();
        assert_eq!(
            guest_time(&guest, &id, 2),
            Err(Refusal::NotRunning(id.try_clone().unwrap(), "host_use"))
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

    /// AC-T7.4.1: no stage restarts a finite create budget after scope publication.
    #[test]
    fn managed_budget_is_one_absolute_finite_deadline() {
        assert!(Budget::new(Duration::ZERO).is_err());
        assert!(Budget::new(Duration::MAX).is_err());
        let start = Instant::now();
        let deadline = start.checked_add(Duration::from_secs(1)).unwrap();
        let budget = Budget {
            deadline: Some(deadline),
        };
        assert_eq!(
            budget.remaining_at(start).unwrap(),
            Some(Duration::from_secs(1))
        );
        assert!(budget.remaining_at(deadline).is_err());
        assert!(
            budget
                .remaining_at(deadline.checked_add(Duration::from_nanos(1)).unwrap())
                .is_err()
        );
    }

    /// The review's C-9, (c2) (decision 9; Shards b4a018090's `Until::Released`): a GPU create's budget follows its
    /// guest as every managed create's does, no deadline and none left, its report taken with no clock. Do take a GPU
    /// create's budget. Expect it to follow, as a following budget does.
    #[test]
    fn c9_c2_a_gpu_creates_budget_follows_its_guest() {
        let gpu = gpu_create().unwrap();
        assert_eq!(gpu.deadline(), None, "a GPU create's budget has a deadline");
        assert_eq!(gpu.remaining().unwrap(), None);
    }

    /// The review's C-9 (decision 9): a managed create's budget that follows its guest has no deadline and none left,
    /// spent never; a GPU create's is finite, the record's, until Shards' GPU report takes none (owed).
    #[test]
    fn c9_a_following_budget_has_no_deadline_and_a_gpu_creates_has_one() {
        let following = Budget::following();
        assert_eq!(following.deadline(), None);
        assert_eq!(following.remaining().unwrap(), None);
        assert!(gpu_deadline(following).is_err());
        let gpu = gpu_create().unwrap();
        let left = gpu.remaining().unwrap().unwrap();
        assert!(left <= crate::guest::recorded_boot().unwrap(), "{left:?}");
        assert_eq!(gpu_deadline(gpu).ok(), gpu.deadline());
    }
}
