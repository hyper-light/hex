//! Joint plan step 14 on macOS and Windows (AC-T7.4.1; `docs/design/learned-charge.md` C1 to C5, C8): the charge
//! learner of a node whose platform caps a microVM's CPU rather than weighs it. Once a window, the longest any cap it
//! holds runs over, it reads each running VM it learns, learns its charge and booking (`hex_sim::charge`), and caps it
//! at its water-filled share (`charge::Node::rates`): a managed VM through its native owner, macOS's managed control's
//! rate or Windows' job's hard cap ([`crate::recorded::Recover::set_managed_rate`], over the platform's window,
//! [`crate::recorded::Recover::charge_window`]); a Shards image VM through its rate (`Runtime::rate`, Shards' D247,
//! over its run's own period, read each window, [`crate::recorded::Recover::image_rate_period`]), its cost read from
//! its run host (`Runtime::host_use`, D241) and its guest told its room, its budget less what it holds (`Runtime::room`,
//! D188). A managed VM on a platform with no window is booked whole off the fleet's budget, as a native box books its
//! caps ([`sort`]); with no window of its own and no image VM held, the learner waits for a box to begin ([`wait`]).
//! Every VM it learns so keeps at least its booking and the caps fit the fleet's CPU budget, as Linux's weights hold
//! them (C4).
//! Neither platform can hold a managed VM's memory at a budget below its ceiling: macOS ends a process past its
//! footprint limit and Windows refuses a commit past a job's, and neither throttles at a level as `memory.high` does,
//! so a managed VM's budget is refused, typed, naming that (C5); an image VM's guest holds itself within its room. Its
//! last resort ends the VM most past its own budget (a managed VM through its receipt,
//! [`crate::recorded::Recover::stop_managed`]; an image VM's run stopped), by the state every OS's learner shares
//! ([`LastResort`]), as Linux's kills its cgroup. Its capacity answers carry what it learned, as Linux's do (C8).

use std::collections::{BTreeMap, VecDeque};
use std::num::NonZeroU64;
use std::time::{Duration, Instant};

use hex_api::{
    BoxInfo, CapacityResources, HostUse, Learned, LearnedEnd, LearnedHolder, LearnedUse,
    MAX_LEARNED_HOLDINGS, Refusal, RuntimeCost, Upstream, Usage,
};
use hex_core::record::{BoxRecord, Phase};
use hex_runtime::guest::ManagedGuestTime;
use hex_sim::charge::{self, ChargeError, ClassKey, Cpu, Excess, Sample};
use hex_sim::model::Class;

use crate::store::Store;

/// One VM's lifetime, as its record names it (C3): a managed VM's native scope, its random bytes, or a Shards image VM's
/// place in its node's start order (`BoxRecord::started`), which its node gives each lifetime once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Scope {
    Native([u8; 16]),
    Started(u64),
}

impl Scope {
    /// The lifetime `record` names: its native scope; else, a Shards image VM's, its place in its node's start order.
    pub(crate) fn of(record: &BoxRecord) -> Option<Self> {
        if let Some(scope) = &record.native_scope {
            return Some(Self::Native(*scope.bytes()));
        }
        match (&record.spec.class, record.started) {
            (hex_api::ClassSpec::ShardsImage(_), Some(started)) => Some(Self::Started(started)),
            _ => None,
        }
    }
}

/// A VM as a learner's core holds it (C3): its lifetime's place in its node's start order, then its scope, so every
/// order the rule takes among VMs is the order the node began them, kept in their records across the node's restarts,
/// never their scopes' random bytes: the room, of a band, to the earliest begun; a budget's spare bytes and a CPU share's
/// spare milli-CPU to the first; of VMs equally past their budgets, the end to the latest. A record that holds no place
/// (an earlier boot's, which its store leaves as that boot wrote it) comes first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Lifetime {
    pub(crate) started: u64,
    pub(crate) scope: Scope,
}

impl Lifetime {
    /// The lifetime of `scope`, its record's place `started` in its node's start order.
    pub(crate) fn of(scope: Scope, started: Option<u64>) -> Self {
        Self {
            started: started.unwrap_or(0),
            scope,
        }
    }
}

/// What a learner holds a VM by (joint plan steps 14 and 39; ROOT, 2026-11-13): a box's record. A node VM's row, a VM
/// the node runs for itself and a holder of its own in the node's ledger, joins it (step 39).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Holder<'a> {
    Box(&'a BoxRecord),
}

impl<'a> Holder<'a> {
    /// Its holder in the node's ledger and on its class's capacity answers.
    pub(crate) fn id(self) -> Result<LearnedHolder, Refusal> {
        match self {
            Self::Box(record) => Ok(LearnedHolder::Box(record.spec.id.try_clone()?)),
        }
    }

    /// Whether it is holder `id`.
    pub(crate) fn is(self, id: &LearnedHolder) -> bool {
        match (self, id) {
            (Self::Box(record), LearnedHolder::Box(id)) => record.spec.id == *id,
            (Self::Box(_), LearnedHolder::Node(_)) => false,
        }
    }

    /// The lifetime it names ([`Scope::of`]).
    pub(crate) fn scope(self) -> Option<Scope> {
        match self {
            Self::Box(record) => Scope::of(record),
        }
    }

    /// Its receipt.
    pub(crate) fn allocation(self) -> Option<hex_api::Allocation> {
        match self {
            Self::Box(record) => record.spec.allocation,
        }
    }

    /// Its class.
    pub(crate) fn class(self) -> hex_api::Class {
        match self {
            Self::Box(record) => record.spec.class.class(),
        }
    }

    /// Its priority band.
    pub(crate) fn band(self) -> u8 {
        match self {
            Self::Box(record) => record.spec.band,
        }
    }

    /// Its place in its node's start order.
    pub(crate) fn started(self) -> Option<u64> {
        match self {
            Self::Box(record) => record.started,
        }
    }

    /// Why a node before this one was ending it, where its record says so (C5).
    pub(crate) fn intent(self) -> Option<&'a LearnedEnd> {
        match self {
            Self::Box(record) => record.last_resort.as_ref(),
        }
    }

    /// Whether it is a managed microVM that runs now ([`running`]).
    pub(crate) fn managed(self) -> bool {
        match self {
            Self::Box(record) => running(record),
        }
    }

    /// Whether it is a Shards image VM made and not ended ([`running_image`]).
    pub(crate) fn image(self) -> bool {
        match self {
            Self::Box(record) => running_image(record),
        }
    }
}

/// `records`, a node's boxes' records, as the holders a learner reads, each a box's.
pub(crate) fn boxes(records: &[BoxRecord]) -> Result<Vec<Holder<'_>>, Refusal> {
    let mut holders = Vec::new();
    holders
        .try_reserve_exact(records.len())
        .map_err(|_| Refusal::Exhausted {
            what: "a window's VMs",
            bound: records.len(),
        })?;
    holders.extend(records.iter().map(Holder::Box));
    Ok(holders)
}

/// One CPU in milli-CPU, the unit a vCPU and the machine's CPU are counted in.
const CPU_MILLI: u64 = 1_000;

/// What the learner reads and writes of a node's VMs: their native owners' and runs' in the node, a record's in tests.
pub(crate) trait Managed {
    /// A managed VM's use, through its native owner: its scope's CPU so far and the memory it holds, its VM process's
    /// and every process beside it in its scope (the platform's cap holds them all).
    fn usage(&self, holder: Holder<'_>) -> Result<Usage, Refusal>;
    /// A managed VM's vCPUs' guest time since its boot, as its VM process answers its use (D260), asked before its
    /// [`Managed::usage`] so its VMM's own, the scope's CPU less its vCPUs', is never less than none: none apart where
    /// its microVM hasn't started or its process answers no use, why told.
    fn guest_time(&self, holder: Holder<'_>) -> Result<ManagedGuestTime, Refusal>;
    /// How long a managed VM has run (D260): from its VM process's start to now, the span its first split's counters
    /// cover.
    fn life(&self, holder: Holder<'_>) -> Result<Duration, Refusal>;
    /// What a Shards image VM costs its host, as its run host reads its VM process (`Runtime::host_use`, D241).
    fn host_use(&self, holder: Holder<'_>) -> Result<HostUse, Refusal>;
    /// The period the VM's rate holds over: a Shards image VM's its run's own (D247's `ImageGuest::rate_period`); a
    /// managed VM's its governor's, as its VM process last answered (D255), none before its first answer or while its
    /// platform's cap holds it.
    fn rate_period(&self, holder: Holder<'_>) -> Result<Option<Duration>, Refusal>;
    /// Caps the VM at `cap` (C4, D260), and says what holds it there ([`Cap::held`]): its governor holds its vCPUs to
    /// [`Cap::governor`], an image VM's through its rate (D247), a managed VM's through its VM process (D255), on
    /// Windows its job at the share beside; or its platform's cap at the share where its VM process takes no rate.
    fn set_rate(&self, holder: Holder<'_>, cap: Cap<'_>) -> Result<hex_api::RateHeld, Refusal>;
    /// Tells an image VM's guest its room, `room` bytes past what it holds (`Runtime::room`, D188).
    fn set_room(&self, holder: Holder<'_>, room: i64) -> Result<(), Refusal>;
    /// Ends the VM: a managed VM through its receipt, an image VM's run stopped (the last resort, C5).
    fn stop(&self, holder: Holder<'_>) -> Result<(), Refusal>;
    /// Writes `end`, why the last resort is ending the VM of `scope`, into box `id`'s record where it is still that VM's
    /// lifetime, before its end (C5, intent first): whether it was written.
    fn intend(&self, id: &LearnedHolder, scope: Scope, end: &LearnedEnd) -> Result<bool, Refusal>;
}

/// A VM's VMM's own CPU as its learner last read it apart from its vCPUs' (D260).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Own<'a> {
    /// Over its last window, in milli-CPU; at its first split, over its life so far.
    Read(u64),
    /// Not read apart, why: from its boot to its VM process's first use answer, or a process that answers none.
    Unheld(&'a str),
}

/// What a learner caps a VM at for its next window (C4, D260): its share, its whole CPU (its VMM's own and its vCPUs'
/// together), and its VMM's own as last read ([`Own`]), which its governor's rate leaves out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cap<'a> {
    pub(crate) share: u64,
    pub(crate) own: Own<'a>,
}

/// The least rate a VM's governor holds its vCPUs to: 1 milli-CPU, its rate's unit and least nonzero value (Shards'
/// D247 and D255: `RATE` takes milli-CPU, 0 letting the vCPUs be), so a governor is never told none.
pub(crate) const GOVERNOR_LEAST: NonZeroU64 = NonZeroU64::MIN;

impl Cap<'_> {
    /// The rate its governor holds its vCPUs to (D260: the rate less `vmm_ns` over the window): its share less its
    /// VMM's own where read, its whole share while not; [`GOVERNOR_LEAST`] where its VMM's own took the whole share,
    /// which [`Cap::held`] tells.
    pub(crate) fn governor(self) -> NonZeroU64 {
        let _ = self.own;
        NonZeroU64::new(self.share).unwrap_or(GOVERNOR_LEAST)
    }

    /// What holds the VM at its share, its governor holding its vCPUs to [`Cap::governor`], on a platform whose `job`
    /// holds its whole CPU at the share beside (Windows) or none: its governor, its VMM's own read and left out; its
    /// VMM's own taking the whole share, a finding; and while its VMM's own isn't read apart, its governor at the whole
    /// share under its job, or with nothing holding its VMM's own, why told.
    pub(crate) fn held(self, job: bool) -> hex_api::RateHeld {
        if job {
            hex_api::RateHeld::GovernorUnderJob
        } else {
            hex_api::RateHeld::Governor
        }
    }
}

fn refused(why: ChargeError) -> Refusal {
    match why {
        ChargeError::Full { what, bound } => Refusal::Exhausted { what, bound },
        ChargeError::Awaits(needs) => Refusal::Awaits { needs },
        other => Refusal::Invalid {
            what: "a microVM's learned charge",
            why: other.to_string(),
        },
    }
}

/// Whether `record` is a managed microVM that runs now: made, with its native lifetime, its receipt and its controller,
/// and not ended.
pub(crate) fn running(record: &BoxRecord) -> bool {
    record.phase == Phase::Ready
        && record.native_scope.is_some()
        && record.spec.allocation.is_some()
        && record.init.is_some()
        && record.exit.is_none()
        && matches!(
            record.spec.class.class(),
            hex_api::Class::Shards | hex_api::Class::Firecracker
        )
}

/// Whether `record` is a Shards image VM made and not ended, whose run its node reads and holds (D241, D247): its record
/// names its lifetime's place in its node's start order and its receipt.
pub(crate) fn running_image(record: &BoxRecord) -> bool {
    record.phase == Phase::Ready
        && matches!(record.spec.class, hex_api::ClassSpec::ShardsImage(_))
        && record.spec.allocation.is_some()
        && record.started.is_some()
        && record.exit.is_none()
}

/// A window's view of the node's records (C3, C4): the VMs it learns; what each VM it doesn't books, whole, its
/// receipt's allocation, as no cap holds it below; and what the native boxes book, their caps.
#[derive(Debug, Default)]
pub(crate) struct Sorted<'a> {
    pub(crate) learned: Vec<Holder<'a>>,
    pub(crate) whole: CapacityResources,
    pub(crate) native: CapacityResources,
}

/// Sorts `records` for a window of a learner ([`Sorted`]): it learns each VM whose cap's period it reads over (C1), a
/// managed VM where `learns` says so ([`Learner::learns`]) and every image VM, its cap's period its run's own
/// ([`crate::recorded::Recover::image_rate_period`]).
pub(crate) fn sort<'a>(
    records: impl IntoIterator<Item = &'a BoxRecord>,
    learns: impl Fn(Holder<'_>) -> bool,
) -> Result<Sorted<'a>, Refusal> {
    let overflow = || Refusal::Invalid {
        what: "a node's boxes' bookings",
        why: "they overflow".into(),
    };
    let mut sorted = Sorted::default();
    for record in records {
        let kind = if running(record) {
            Some(learns(Holder::Box(record)))
        } else if running_image(record) {
            Some(true)
        } else {
            None
        };
        match (kind, record.spec.allocation) {
            (Some(true), _) => sorted.learned.push(Holder::Box(record)),
            (Some(false), Some(allocation)) => {
                sorted.whole = sorted
                    .whole
                    .checked_add(allocation.resources)
                    .ok_or_else(overflow)?;
            }
            _ if record.phase == Phase::Ready
                && matches!(
                    record.spec.class.class(),
                    hex_api::Class::Process | hex_api::Class::Oci
                ) =>
            {
                let caps = CapacityResources {
                    cpu_milli: record.spec.bounds.cpu_max_milli.map_or(0, u64::from),
                    mem_bytes: record.spec.bounds.memory_max.unwrap_or(0),
                    disk_bytes: 0,
                };
                sorted.native = sorted.native.checked_add(caps).ok_or_else(overflow)?;
            }
            _ => {}
        }
    }
    Ok(sorted)
}

/// What the learner's thread waited for between windows (C1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Waited {
    /// It runs a window.
    Look,
    /// Its node has gone: it ends.
    Gone,
}

/// Waits for the learner's next window (C1): out its window where it has one, a box's beginning told on `ended`
/// meanwhile changing nothing, as the window's end reads every VM it then holds; with none (a node with no platform
/// window holding no image VM), until a box begins. Gone once its node has gone, `ended`'s sender dropped with it.
/// Refused, typed, where it can't wait.
pub(crate) fn wait(
    ended: &hex_wait::mpsc::Receiver<()>,
    window: Option<Duration>,
) -> Result<Waited, Refusal> {
    let failed = |why| Refusal::os("waiting out the charge learner's window", why);
    let Some(window) = window else {
        return match ended.recv() {
            Ok(()) => Ok(Waited::Look),
            Err(hex_wait::mpsc::RecvError::Disconnected) => Ok(Waited::Gone),
            Err(hex_wait::mpsc::RecvError::Failed(why)) => Err(failed(why)),
        };
    };
    // A window the clock can't add to now bounds nothing: its wait is for its node's end alone.
    let end = Instant::now().checked_add(window);
    loop {
        let left = end.map_or(window, |end| end.saturating_duration_since(Instant::now()));
        match ended.recv_timeout(left) {
            Ok(()) => {}
            Err(hex_wait::mpsc::RecvTimeoutError::Timeout) => return Ok(Waited::Look),
            Err(hex_wait::mpsc::RecvTimeoutError::Disconnected) => return Ok(Waited::Gone),
            Err(hex_wait::mpsc::RecvTimeoutError::Failed(why)) => return Err(failed(why)),
        }
    }
}

/// One reading of a VM (C1): its VMM's whole CPU so far, in nanoseconds, the memory it holds now, and each vCPU's guest
/// time so far, or why none apart: an image VM's host use (D241), a managed VM's guest time as its VM process answers
/// its use (D260) and its scope's use through its native owner.
struct Read {
    cpu_ns: u64,
    memory_bytes: u64,
    guest_ns: Result<Vec<u64>, String>,
    /// The span its counters cover, from the VM's start, where read: an image VM's run's (D241's window), a managed VM's
    /// life where its learner splits it first ([`Read::of`]).
    life: Option<Duration>,
}

impl Read {
    /// `holder`'s reading, its life read too where `first`, its learner holding no split of it yet.
    fn of(io: &impl Managed, holder: Holder<'_>, first: bool) -> Result<Self, Refusal> {
        if holder.image() {
            let host = io.host_use(holder)?;
            return Ok(Self {
                cpu_ns: host.cpu_ns,
                memory_bytes: host.footprint_bytes,
                guest_ns: Ok(host.guest_ns),
                life: Some(host.window),
            });
        }
        let _ = first;
        let usage = io.usage(holder)?;
        Ok(Self {
            cpu_ns: usage.cpu_usec.saturating_mul(1_000),
            memory_bytes: usage.memory_bytes,
            guest_ns: Err("its use isn't asked".to_owned()),
            life: None,
        })
    }

    /// Its VMM's own CPU (D260) over its life so far, the VM's first split: none where it holds no guest time or no
    /// life to reckon it over.
    fn own_over_life(&self) -> Option<u64> {
        let (Ok(guest), Some(life)) = (&self.guest_ns, self.life) else {
            return None;
        };
        let vcpus = guest.iter().fold(0u64, |sum, ns| sum.saturating_add(*ns));
        milli_over(self.cpu_ns.saturating_sub(vcpus), life)
    }
}

/// `duration` in nanoseconds, at most a u64's.
fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

/// `ns` of CPU over `span` in milli-CPU: none over no span.
fn milli_over(ns: u64, span: Duration) -> Option<u64> {
    u128::from(ns)
        .checked_mul(u128::from(CPU_MILLI))
        .and_then(|n| n.checked_div(span.as_nanos()))
        .map(|n| u64::try_from(n).unwrap_or(u64::MAX))
}

/// What a VM ran over a window, its whole CPU `cpu_ns` since its reading before: split into each vCPU's guest time and
/// its VMM's own where its host read each vCPU's both times (D241), its VMM's own the whole less its vCPUs', never less
/// than none past the clocks' steps; whole where it didn't. Two readings naming different vCPU counts are refused,
/// typed: no vCPU's time is paired with another's.
fn ran(cpu_ns: u64, now: Option<&[u64]>, before: Option<&[u64]>) -> Result<Cpu, ChargeError> {
    let (Some(now), Some(before)) = (now, before) else {
        return Ok(Cpu::Whole(Duration::from_nanos(cpu_ns)));
    };
    if now.len() != before.len() {
        return Err(ChargeError::Invalid {
            what: "a CPU reading",
            why: "its guest times name a vCPU count other than its reading before's",
        });
    }
    let guest: Vec<Duration> = now
        .iter()
        .zip(before)
        .map(|(now, before)| Duration::from_nanos(now.saturating_sub(*before)))
        .collect();
    let vcpus = now.iter().zip(before).fold(0u64, |sum, (now, before)| {
        sum.saturating_add(now.saturating_sub(*before))
    });
    Ok(Cpu::Split {
        guest,
        own: Duration::from_nanos(cpu_ns.saturating_sub(vcpus)),
    })
}

/// What moving a VM of `class` to another machine instead of ending it awaits, where that move is another product's
/// (C5): a Shards VM saved whole and resumed elsewhere is Shards' D176, which shards-api's image run (D231) doesn't
/// offer. A Firecracker VM's move would be Hex's own, through Firecracker's snapshot, which Hex doesn't drive yet: none
/// to await.
fn move_awaits(class: Class) -> Option<Upstream> {
    match class {
        Class::Shards => Some(Upstream::LiveMove),
        _ => None,
    }
}

/// The last resort's state from window to window (C5), explicit (R04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resort {
    /// What the fleet's VMs hold, with the fastest growth on record in one window, is within its budget.
    Held,
    /// It wasn't: the VM `id` held most past its own budget, lowest band first, and was ended for `why`. None other is
    /// ended until it is gone, so one end is never two.
    Ending {
        scope: Scope,
        id: LearnedHolder,
        why: Excess,
    },
    /// It isn't, and the node can't act: every VM holds within its own budget, growth being no fact yet, or ending
    /// the VM the rule chose was refused. Kept until a window moves it.
    Refused(Refusal),
}

/// How a learner's last resort reaches a VM (C5): its record, to say why before the VM is ended, and the VM itself.
pub(crate) trait Reach {
    /// Writes `end`, why the VM of `scope` is to be ended, into holder `id`'s record where it is still that VM's
    /// lifetime, before the VM is ended (intent first): whether it was written.
    fn intend(&self, id: &LearnedHolder, scope: Scope, end: &LearnedEnd) -> Result<bool, Refusal>;
    /// Ends the VM of `scope`: Linux kills its cgroup, macOS and Windows stop it through its managed receipt.
    fn end(&self, scope: &Scope) -> Result<(), Refusal>;
}

/// Writes into VMs' records the last resort keeps for after its window, oldest first: by holder and VM lifetime, why it
/// ended the VM, or none, an intent withdrawn (C5).
pub(crate) type Writes = VecDeque<(LearnedHolder, Scope, Option<LearnedEnd>)>;

/// The last resort and its record of why (C5), the same on every OS: one VM ended at a time, as the core's verdict
/// (`charge::Node::last_resort`) chooses it, why written into its record before it is ended (intent first) and kept for
/// its class's capacity answers.
pub(crate) struct LastResort {
    resort: Resort,
    /// The VMs it ended and why, by class, oldest first: at most `ends` of them, the oldest forgotten first, a machine
    /// never holding more VMs than its population at once.
    ended: VecDeque<(Class, LearnedEnd)>,
    ends: usize,
    /// Writes into VMs' records not yet made, oldest first, one per VM lifetime, the latest asked: why it ended a VM whose
    /// intent wasn't written first, or none, an intent withdrawn because its end was refused. The node makes them after
    /// the window ([`write_ends`]) and keeps one that failed for the next, at most `ends` of them, the oldest forgotten
    /// first.
    unrecorded: Writes,
    /// Ends a node before this one wrote into its VMs' records and stopped before making, their VMs running when this
    /// one met them, oldest first: carried out as they were decided, before the rule is asked again. At most `ends`.
    due: VecDeque<(Scope, Class, LearnedEnd)>,
}

/// A recorded end's why as the core says it.
fn excess(end: &LearnedEnd) -> Excess {
    Excess {
        held: end.held,
        budget: end.budget,
        band: end.band,
        fleet: charge::Pressure {
            held: end.fleet_held,
            growth: end.fleet_growth,
            budget: end.fleet_budget,
        },
    }
}

impl LastResort {
    /// A last resort keeping at most `ends` ends: the machine's VM population, at most a node's boxes.
    pub(crate) fn new(ends: usize) -> Self {
        Self {
            resort: Resort::Held,
            ended: VecDeque::new(),
            ends,
            unrecorded: VecDeque::new(),
            due: VecDeque::new(),
        }
    }

    /// The learner met the VM of `scope` and class `class` for the first time, its record naming `intent`: why a node
    /// before this one was ending it, which this one carries out (intent first).
    pub(crate) fn met(
        &mut self,
        class: Class,
        scope: Scope,
        intent: Option<LearnedEnd>,
    ) -> Result<(), Refusal> {
        let Some(end) = intent else {
            return Ok(());
        };
        if self.due.len() >= self.ends {
            return Err(Refusal::Exhausted {
                what: "the last resort's ends due",
                bound: self.ends,
            });
        }
        self.due.try_reserve(1).map_err(|_| Refusal::Exhausted {
            what: "the last resort's ends due",
            bound: self.ends,
        })?;
        self.due.push_back((scope, class, end));
        Ok(())
    }

    /// The VM of `scope` is gone: nothing of it is due.
    pub(crate) fn gone(&mut self, scope: &Scope) {
        self.due.retain(|(due, ..)| due != scope);
    }

    /// One window's last resort, once the placement holds what it can: while a VM it ended still runs (`vm` names it),
    /// nothing more, so one end is never two; else an end a node before this one decided, as it decided it; else the
    /// core's verdict on `node`, why written into the chosen VM's record before `reach` ends it, or refused, typed,
    /// where the node can't act. An end refused withdraws its intent; an intent that couldn't be written holds no end,
    /// and is written after it. `vm` names each VM the learner holds: its class and box.
    pub(crate) fn window<'a>(
        &mut self,
        node: &charge::Node<Lifetime>,
        vm: impl Fn(&Scope) -> Option<(Class, &'a LearnedHolder)>,
        reach: &impl Reach,
    ) -> Result<(), Refusal> {
        if let Resort::Ending { scope, .. } = &self.resort
            && vm(scope).is_some()
        {
            return Ok(());
        }
        if let Some((scope, class, end)) = self.due.pop_front() {
            self.resort = match reach.end(&scope) {
                Ok(()) => {
                    self.remember(class, &end)?;
                    Resort::Ending {
                        scope,
                        id: end.id.try_clone()?,
                        why: excess(&end),
                    }
                }
                Err(refusal) => {
                    self.write(end.id.try_clone()?, scope, None)?;
                    Resort::Refused(refusal)
                }
            };
            return Ok(());
        }
        self.resort = match node.last_resort().map_err(refused)? {
            charge::Resort::Held => Resort::Held,
            charge::Resort::Within(fleet) => Resort::Refused(Refusal::Exhausted {
                what: "a microVM fleet's memory budget, every VM within its own",
                bound: usize::try_from(fleet.budget).unwrap_or(usize::MAX),
            }),
            charge::Resort::End { vm: lifetime, why } => {
                let scope = lifetime.scope;
                let (class, id) = vm(&scope).ok_or_else(|| {
                    Refusal::Broken("the last resort chose a VM the learner doesn't hold".into())
                })?;
                let end = LearnedEnd {
                    id: id.try_clone()?,
                    held: why.held,
                    budget: why.budget,
                    band: why.band,
                    fleet_held: why.fleet.held,
                    fleet_growth: why.fleet.growth,
                    fleet_budget: why.fleet.budget,
                    unmoved: move_awaits(class),
                };
                // What was kept for the VM's record before is this end's to say now.
                self.unrecorded.retain(|(_, kept, _)| *kept != scope);
                let written = matches!(reach.intend(id, scope, &end), Ok(true));
                match reach.end(&scope) {
                    Ok(()) => {
                        if !written {
                            self.write(id.try_clone()?, scope, Some(end.try_clone()?))?;
                        }
                        self.remember(class, &end)?;
                        Resort::Ending {
                            scope,
                            id: id.try_clone()?,
                            why,
                        }
                    }
                    Err(refusal) => {
                        if written {
                            self.write(id.try_clone()?, scope, None)?;
                        }
                        Resort::Refused(refusal)
                    }
                }
            }
        };
        Ok(())
    }

    /// Keeps why the last resort ended a VM of `class`, for its class's capacity answers, forgetting the oldest past the
    /// bound.
    fn remember(&mut self, class: Class, end: &LearnedEnd) -> Result<(), Refusal> {
        if self.ended.len() >= self.ends {
            self.ended.pop_front();
        }
        self.ended.try_reserve(1).map_err(|_| Refusal::Exhausted {
            what: "the last resort's ends",
            bound: self.ends,
        })?;
        self.ended.push_back((class, end.try_clone()?));
        Ok(())
    }

    /// Keeps `end` (none: an intent withdrawn) for box `id`'s record of lifetime `scope`, after the window, in place of
    /// any kept for it before, forgetting the oldest past the bound.
    fn write(
        &mut self,
        id: LearnedHolder,
        scope: Scope,
        end: Option<LearnedEnd>,
    ) -> Result<(), Refusal> {
        self.unrecorded.retain(|(_, kept, _)| *kept != scope);
        if self.unrecorded.len() >= self.ends {
            self.unrecorded.pop_front();
        }
        self.unrecorded
            .try_reserve(1)
            .map_err(|_| Refusal::Exhausted {
                what: "the last resort's unrecorded ends",
                bound: self.ends,
            })?;
        self.unrecorded.push_back((id, scope, end));
        Ok(())
    }

    /// The VMs of `class` it ended and why, latest first (C5, C8).
    pub(crate) fn ended(&self, class: Class) -> Result<Vec<LearnedEnd>, Refusal> {
        let mut ended = Vec::new();
        for (of, end) in self.ended.iter().rev() {
            if *of == class {
                ended.try_reserve(1).map_err(|_| Refusal::Exhausted {
                    what: "a class's learned ends",
                    bound: self.ends,
                })?;
                ended.push(end.try_clone()?);
            }
        }
        Ok(ended)
    }

    /// Takes the writes into VMs' records not yet made, oldest first (C5).
    pub(crate) fn unrecorded(&mut self) -> Writes {
        std::mem::take(&mut self.unrecorded)
    }

    /// Keeps `left`, writes that failed, for the next window: before any asked since, none for a VM one asked since
    /// replaces, within the bound.
    pub(crate) fn keep_unrecorded(&mut self, mut left: Writes) {
        left.retain(|(_, scope, _)| !self.unrecorded.iter().any(|(_, kept, _)| kept == scope));
        left.append(&mut self.unrecorded);
        while left.len() > self.ends {
            left.pop_front();
        }
        self.unrecorded = left;
    }

    /// The last resort's state (C5).
    #[cfg(test)]
    pub(crate) fn resort(&self) -> &Resort {
        &self.resort
    }

    /// Keeps why the last resort ended the VM `id` of lifetime `scope` and class `class` for `why`, as an end does.
    #[cfg(test)]
    fn record_end(
        &mut self,
        class: Class,
        id: &LearnedHolder,
        scope: Scope,
        why: Excess,
    ) -> Result<(), Refusal> {
        let end = LearnedEnd {
            id: id.try_clone()?,
            held: why.held,
            budget: why.budget,
            band: why.band,
            fleet_held: why.fleet.held,
            fleet_growth: why.fleet.growth,
            fleet_budget: why.fleet.budget,
            unmoved: move_awaits(class),
        };
        self.write(id.try_clone()?, scope, Some(end.try_clone()?))?;
        self.remember(class, &end)
    }
}

/// Writes `end`, why the last resort ended or is ending VM `id`, into its record in `store` where the record is still
/// that VM's lifetime (`scope`), or withdraws it (none): false where none is, the box deleted since or made again. Its
/// write waits for the box's record turn as every change of its record does (`Store::update`), by decision 9's rule
/// (the person's, 2026-11-13; ROOT's item 2b): following the change under way, this process's own thread, until it
/// lets go, a hold past the slowest seen told to the census as a run-out (`store.turns`), never refused at a bound of
/// its own (`turns::tests::a_wait_past_the_slowest_hold_follows_the_holder_and_learns_its_hold`).
pub(crate) fn record_last_resort(
    store: &Store,
    id: &LearnedHolder,
    scope: Scope,
    end: Option<&LearnedEnd>,
) -> Result<bool, Refusal> {
    let id = match id {
        LearnedHolder::Box(id) => id,
        // A node VM's row keeps its own (step 39); no learner holds one yet.
        LearnedHolder::Node(_) => {
            return Err(Refusal::Broken(
                "a node VM's last resort is kept in its row, which no learner here holds"
                    .to_owned(),
            ));
        }
    };
    let end = end.map(LearnedEnd::try_clone).transpose()?;
    store.update(id, move |record| {
        if Scope::of(record) != Some(scope) {
            return false;
        }
        record.last_resort = end;
        true
    })
}

/// Makes each of `writes` in its box's record in `store` (C5), off the learner's lock, as the node does after each
/// window: the writes that failed, which the node keeps for the next window, and the first failure, which fails this
/// one.
pub(crate) fn write_ends(store: &Store, mut writes: Writes) -> (Writes, Option<Refusal>) {
    let mut failed = None;
    writes.retain(
        |(id, scope, end)| match record_last_resort(store, id, *scope, end.as_ref()) {
            Ok(_) => false,
            Err(why) => {
                failed.get_or_insert(why);
                true
            }
        },
    );
    (writes, failed)
}

/// A VM box's inspect from its backend, with why the last resort ended it where its record says so (C5), across the
/// node's restarts.
pub(crate) fn with_last_resort(info: &mut BoxInfo, record: &BoxRecord) -> Result<(), Refusal> {
    info.last_resort = record
        .last_resort
        .as_ref()
        .map(LearnedEnd::try_clone)
        .transpose()?;
    Ok(())
}

/// The last resort's reach on macOS and Windows: a VM's record through the node's records, its end through its managed
/// receipt.
struct Stops<'a, 'h, M> {
    io: &'a M,
    running: &'a [Holder<'h>],
}

impl<M: Managed> Reach for Stops<'_, '_, M> {
    fn intend(&self, id: &LearnedHolder, scope: Scope, end: &LearnedEnd) -> Result<bool, Refusal> {
        self.io.intend(id, scope, end)
    }

    fn end(&self, scope: &Scope) -> Result<(), Refusal> {
        let holder = self
            .running
            .iter()
            .find(|holder| holder.scope() == Some(*scope))
            .ok_or_else(|| Refusal::Broken("a learned VM without its record".into()))?;
        self.io.stop(*holder)
    }
}

/// One VM the learner holds.
struct Vm {
    id: LearnedHolder,
    /// How the core holds it: its place in the node's start order and its scope (C3).
    lifetime: Lifetime,
    class: Class,
    /// What its receipt booked: what the ledger holds of it.
    receipt: CapacityResources,
    /// Its VMM's CPU so far, in nanoseconds, and when it was read.
    last: (u64, Instant),
    /// Each vCPU's guest time at that reading, where its host reads it (D241, D260).
    guest_ns: Option<Vec<u64>>,
    /// Its VMM's own CPU as last read apart from its vCPUs', in milli-CPU (D260: over its last window; at its first
    /// split, over its life so far); why not, while it isn't.
    own: Result<u64, String>,
    /// The cap it runs under, its share: its allocation's CPU until the learner first caps it.
    rate: u64,
    /// Its VMM's own its governor's rate last left out of its share, where one was (D260).
    told: Option<u64>,
    /// The room its guest was told last, an image VM's (D188): none until it is first told.
    room: Option<i64>,
    /// The period its cap holds over as last read: an image VM's its run's own (D247), a managed VM's its governor's
    /// (D255); none for a managed VM whose governor hasn't answered or whose platform's cap holds it, its platform's
    /// window holding instead.
    period: Option<Duration>,
    /// What holds it at its cap (D255): none until the learner first caps it.
    held: Option<hex_api::RateHeld>,
}

/// A managed VM its learner asked its governor at first sight, on a platform with no window (D255).
struct Governed {
    class: Class,
    /// What holds its rate: its governor (under its job on Windows), or its platform's cap and why.
    held: hex_api::RateHeld,
    /// Whether its governor answered a period, so its learner learns it over that.
    period: bool,
    /// Whether its ask failed, so it is asked again next window.
    again: bool,
}

/// The node's learner (module docs).
pub(crate) struct Learner {
    node: charge::Node<Lifetime>,
    vms: BTreeMap<Scope, Vm>,
    /// Its platform's window for the managed VMs it caps (C1, [`crate::recorded::Recover::charge_window`]): none where
    /// the platform derives none, its managed VMs booked whole off the fleet's budget ([`sort`]).
    managed: Option<Duration>,
    /// Why the last window failed: until one passes, the capacity answers carry nothing learned.
    fault: Option<Refusal>,
    /// Why no VM's memory budget is held on this platform (C5), typed.
    unheld: Refusal,
    /// The VMs its last window couldn't read and why, each counted whole against the fleet's budget (ROOT, 2026-11-13).
    unread: Vec<(LearnedHolder, Refusal)>,
    /// The managed VMs it asked their governors at first sight, on a platform with no window (D255), by their scopes.
    governors: BTreeMap<Scope, Governed>,
    /// The last resort (C5).
    last: LastResort,
}

/// The most a learner on a machine of `machine` holds (CLAUDE.md §2), every OS's alike. VMs: the machine's population,
/// the most boxes placement places on it (`placement::budget`'s runner count: the machine over Hex's least bounds,
/// `MIN_CPU_MILLI` of its CPU and `MIN_MEMORY` of its memory), so a VM past it, which only a node driven past its
/// placement meets, is refused, typed, and its window with it. Classes: as many, the least bound at which no VM's
/// class is refused, as VMs of that many classes may run at once, and while fewer run one class no VM holds is there
/// to forget.
pub(crate) fn bounds(machine: CapacityResources) -> Result<charge::Bounds, Refusal> {
    let cpus = machine
        .cpu_milli
        .checked_div(u64::from(hex_api::MIN_CPU_MILLI));
    let memory = machine.mem_bytes.checked_div(hex_api::MIN_MEMORY);
    let population = cpus
        .zip(memory)
        .and_then(|(cpus, memory)| usize::try_from(cpus.min(memory)).ok())
        .ok_or_else(|| Refusal::Invalid {
            what: "a microVM's learned charge",
            why: "the machine's population passes a usize".into(),
        })?;
    Ok(charge::Bounds {
        vms: population,
        classes: population,
    })
}

/// `a` less `b` in each dimension, none below nothing.
fn less(a: CapacityResources, b: CapacityResources) -> CapacityResources {
    CapacityResources {
        cpu_milli: a.cpu_milli.saturating_sub(b.cpu_milli),
        mem_bytes: a.mem_bytes.saturating_sub(b.mem_bytes),
        disk_bytes: a.disk_bytes.saturating_sub(b.disk_bytes),
    }
}

/// A VM's vCPUs: its admitted guest's CPU in whole CPUs, rounded up.
fn vcpus(allocation: hex_api::Allocation) -> Result<u32, Refusal> {
    let guest = allocation.guest()?;
    u32::try_from(guest.cpu_milli.div_ceil(CPU_MILLI)).map_err(|_| Refusal::Invalid {
        what: "a microVM's learned charge",
        why: "its guest's vCPUs pass a u32".into(),
    })
}

impl Learner {
    /// A learner on a machine of `machine` whose platform caps managed VMs over `managed`, none where it derives no
    /// window: it reads over that until it holds an image VM whose run's period is longer (C1).
    pub(crate) fn new(
        managed: Option<Duration>,
        machine: CapacityResources,
    ) -> Result<Self, Refusal> {
        let bounds = bounds(machine)?;
        Ok(Self {
            node: charge::Node::new(managed, machine, bounds).map_err(refused)?,
            vms: BTreeMap::new(),
            managed,
            fault: None,
            last: LastResort::new(bounds.vms.min(MAX_LEARNED_HOLDINGS)),
            unread: Vec::new(),
            governors: BTreeMap::new(),
            unheld: Refusal::Platform {
                what: "memory_high",
                platform: std::env::consts::OS,
                why: "a managed VM's memory is held at its ceiling alone: the platform ends (macOS's footprint \
                      limit) or refuses (Windows' job commit limit) past a bound, and throttles at no level below it"
                    .to_owned(),
            },
        })
    }

    /// Whether it learns managed VMs: where their platform has a window (C1).
    pub(crate) fn manages(&self) -> bool {
        self.managed.is_some()
    }

    /// Whether it learns managed VM `holder` (C1, D255): where its platform has a window, or its governor has answered a
    /// period ([`Learner::govern`]).
    pub(crate) fn learns(&self, holder: Holder<'_>) -> bool {
        self.manages()
            || holder
                .scope()
                .and_then(|scope| self.governors.get(&scope))
                .is_some_and(|governed| governed.period)
    }

    /// Asks the governor of each managed VM among `records` it hasn't asked, on a platform with no window (D255; ROOT,
    /// 2026-11-13), at the VM's ceiling, its allocation's CPU, which holds nothing below its vCPUs: a cold start that
    /// can't starve it. One whose governor answers a period is learned over it from then on ([`Learner::learns`]); one
    /// whose process takes no rate stays booked whole, told; one whose ask or period read fails stays booked whole,
    /// told, and is asked again next window. Those no longer running are forgotten.
    pub(crate) fn govern(
        &mut self,
        io: &impl Managed,
        records: &[Holder<'_>],
    ) -> Result<(), Refusal> {
        self.governors
            .retain(|scope, _| records.iter().any(|record| record.scope() == Some(*scope)));
        if self.manages() {
            return Ok(());
        }
        for &record in records {
            if !record.managed() {
                continue;
            }
            let (Some(scope), Some(allocation)) = (record.scope(), record.allocation()) else {
                continue;
            };
            if self.vms.contains_key(&scope)
                || self
                    .governors
                    .get(&scope)
                    .is_some_and(|governed| !governed.again)
            {
                continue;
            }
            let class = match record.class() {
                hex_api::Class::Shards => Class::Shards,
                hex_api::Class::Firecracker => Class::Firecracker,
                hex_api::Class::Process | hex_api::Class::Oci => continue,
            };
            let first = Cap {
                share: allocation.resources.cpu_milli,
                own: Own::Unheld("its learner asks its governor before it reads its use"),
            };
            let asked = io.set_rate(record, first).and_then(|held| match held {
                hex_api::RateHeld::Platform(_) => Ok((held, false)),
                held => io
                    .rate_period(record)
                    .map(|period| (held, period.is_some())),
            });
            let (held, period, again) = match asked {
                Ok((held, period)) => (held, period, false),
                Err(why) => (hex_api::RateHeld::Platform(why.to_string()), false, true),
            };
            self.governors.insert(
                scope,
                Governed {
                    class,
                    held,
                    period,
                    again,
                },
            );
        }
        Ok(())
    }

    /// The window it reads over next (C1): the longest of its platform's and the periods of the image VMs it holds,
    /// none while it holds none and its platform has none ([`wait`]).
    pub(crate) fn next_window(&self) -> Option<Duration> {
        self.node.window()
    }

    /// The longest period of the caps it holds now (C1): its platform's for managed VMs and each image VM's its run's
    /// own, as last read.
    fn longest(&self) -> Option<Duration> {
        self.vms
            .values()
            .filter_map(|vm| vm.period)
            .chain(self.managed)
            .max()
    }

    /// One window (C1 to C5, C7) read at `now`: the VMs it learns `running` now (their records, [`sort`]), within the
    /// fleet's `budget`. A failure is kept until a window passes.
    pub(crate) fn window(
        &mut self,
        io: &impl Managed,
        running: &[Holder<'_>],
        budget: CapacityResources,
        now: Instant,
    ) -> Result<(), Refusal> {
        let outcome = self.learn(io, running, budget, now);
        // Its next window is the longest period of the caps it holds whatever this one reached (C1); one that settled
        // set it already, before its last resort.
        let outcome = outcome.and(self.node.set_window(self.longest()).map_err(refused));
        self.fault = outcome.as_ref().err().cloned();
        outcome
    }

    fn learn(
        &mut self,
        io: &impl Managed,
        running: &[Holder<'_>],
        budget: CapacityResources,
        now: Instant,
    ) -> Result<(), Refusal> {
        let scope_of = |holder: &Holder<'_>| holder.scope();
        let gone: Vec<Scope> = self
            .vms
            .keys()
            .filter(|scope| !running.iter().any(|r| scope_of(r) == Some(**scope)))
            .copied()
            .collect();
        for scope in gone {
            if let Some(vm) = self.vms.get(&scope) {
                self.node.end(&vm.lifetime).map_err(refused)?;
            }
            self.vms.remove(&scope);
            self.last.gone(&scope);
        }
        // What each VM used of the cap it ran under in this window, for its next cap.
        let mut used = BTreeMap::new();
        // What the VMs it couldn't read hold past their bookings, their receipts whole.
        let mut unread = CapacityResources::default();
        self.unread.clear();
        for &record in running {
            let Some(scope) = record.scope() else {
                continue;
            };
            let Some(allocation) = record.allocation() else {
                continue;
            };
            // Each window reads the VM's use and the period its cap holds over (C1): an image VM's its run's own (D247),
            // a managed VM's its governor's as its VM process last answered (D255), none before. A VM either read fails
            // for counts whole, at its receipt, against the fleet's budget this window, its learner untouched (ROOT,
            // 2026-11-13: never starving its neighbours by holding nothing): no sample taken, its booking as it was,
            // and the failure told ([`Learner::unread`]). One that ended since its listing is ended by the next.
            // A VM whose last reading split it is read as one; else its first split is over its life so far (D260).
            let first = self.vms.get(&scope).is_none_or(|vm| vm.guest_ns.is_none());
            let reading = io
                .rate_period(record)
                .and_then(|period| Read::of(io, record, first).map(|read| (period, read)));
            let (period, read) = match reading {
                Ok(reading) => reading,
                Err(why) => {
                    let booked = self
                        .vms
                        .get(&scope)
                        .and_then(|vm| self.node.booking(&vm.lifetime))
                        .unwrap_or_default();
                    let past = less(allocation.resources, booked);
                    unread = unread.checked_add(past).ok_or_else(|| Refusal::Invalid {
                        what: "a microVM's learned charge",
                        why: "the unread VMs' receipts overflow".into(),
                    })?;
                    self.unread.try_reserve(1).map_err(|_| Refusal::Exhausted {
                        what: "the charge learner's unread VMs",
                        bound: running.len(),
                    })?;
                    self.unread.push((record.id()?, why));
                    continue;
                }
            };
            if let Some(vm) = self.vms.get_mut(&scope) {
                let (before, at) = vm.last;
                let cpu_ns = read.cpu_ns.saturating_sub(before);
                let span = now.saturating_duration_since(at);
                let cpu = ran(
                    cpu_ns,
                    read.guest_ns.as_deref().ok(),
                    vm.guest_ns.as_deref(),
                )
                .map_err(refused)?;
                // Its VMM's own CPU (D260): over the window where both readings split it, over its life where this is
                // its first split.
                let own = match &cpu {
                    Cpu::Split { own, .. } => milli_over(nanos(*own), span),
                    Cpu::Whole(_) => read.own_over_life(),
                };
                self.node
                    .observe(
                        vm.lifetime,
                        &Sample {
                            cpu,
                            span,
                            mem_bytes: read.memory_bytes,
                            disk_bytes: None,
                            cap_milli: Some(vm.rate),
                        },
                    )
                    .map_err(refused)?;
                let milli = u128::from(cpu_ns)
                    .checked_mul(u128::from(CPU_MILLI))
                    .and_then(|n| n.checked_div(span.as_nanos().max(1)))
                    .and_then(|n| u64::try_from(n).ok())
                    .unwrap_or(u64::MAX);
                used.insert(vm.lifetime, (milli, vm.rate));
                vm.last = (read.cpu_ns, now);
                let _ = own;
                vm.guest_ns = read.guest_ns.ok();
                vm.period = period;
                continue;
            }
            let class = match record.class() {
                hex_api::Class::Shards => Class::Shards,
                hex_api::Class::Firecracker => Class::Firecracker,
                hex_api::Class::Process | hex_api::Class::Oci => continue,
            };
            let receipt = allocation.booking();
            let lifetime = Lifetime::of(scope, record.started());
            self.node
                .track(
                    lifetime,
                    ClassKey::of(class, allocation)?,
                    record.band(),
                    vcpus(allocation)?,
                    receipt,
                )
                .map_err(refused)?;
            // Its first split, from its first reading holding its guest time: over its life so far (D260).
            let guest_ns = read.guest_ns.ok();
            let own = Err("its use isn't asked".to_owned());
            self.vms.insert(
                scope,
                Vm {
                    id: record.id()?,
                    lifetime,
                    class,
                    receipt,
                    last: (read.cpu_ns, now),
                    guest_ns,
                    own,
                    rate: allocation.resources.cpu_milli,
                    told: None,
                    room: None,
                    period,
                    held: None,
                },
            );
            let intent = record.intent().map(LearnedEnd::try_clone).transpose()?;
            self.last.met(class, scope, intent)?;
        }
        // The fleet's budget this window, less what its unread VMs hold past their bookings.
        let budget = less(budget, unread);
        self.node.set_budget(budget);
        self.node.settle().map_err(refused)?;
        // The window it reads over next (C1), the one its last resort projects the growth on record over (C5).
        self.node.set_window(self.longest()).map_err(refused)?;
        let rates = self
            .node
            .rates(budget.cpu_milli, &used, u64::from(hex_api::MIN_CPU_MILLI))
            .map_err(refused)?;
        for (lifetime, rate) in rates {
            let scope = lifetime.scope;
            let Some(vm) = self.vms.get_mut(&scope) else {
                continue;
            };
            // Its governor is told its share less its VMM's own as last read (D260), each time either moves, and once
            // at first sight, so what holds it is told from its first window.
            let own = match &vm.own {
                Ok(own) => Own::Read(*own),
                Err(why) => Own::Unheld(why),
            };
            let told = match own {
                Own::Read(own) => Some(own),
                Own::Unheld(_) => None,
            };
            if vm.rate == rate {
                continue;
            }
            let record = running
                .iter()
                .find(|record| scope_of(record) == Some(scope))
                .ok_or_else(|| Refusal::Broken("a learned VM without its record".into()))?;
            vm.held = Some(io.set_rate(*record, Cap { share: rate, own })?);
            vm.rate = rate;
            vm.told = told;
        }
        // Each image VM's guest told its room as it moves (C5, C7; D188): its budget less what it holds, everything it
        // holds coming out of it, negative where its budget is short of that.
        for (lifetime, budget) in self.node.budgets().map_err(refused)? {
            let scope = lifetime.scope;
            let Some(vm) = self.vms.get_mut(&scope) else {
                continue;
            };
            if !matches!(scope, Scope::Started(_)) || vm.room == Some(budget.room) {
                continue;
            }
            let record = running
                .iter()
                .find(|record| scope_of(record) == Some(scope))
                .ok_or_else(|| Refusal::Broken("a learned VM without its record".into()))?;
            io.set_room(*record, budget.room)?;
            vm.room = Some(budget.room);
        }
        let vms = &self.vms;
        self.last.window(
            &self.node,
            |scope| vms.get(scope).map(|vm| (vm.class, &vm.id)),
            &Stops { io, running },
        )
    }

    /// Takes the writes into VMs' records not yet made, oldest first (C5).
    pub(crate) fn unrecorded(&mut self) -> Writes {
        self.last.unrecorded()
    }

    /// Keeps `left`, writes that failed, for the next window (C5).
    pub(crate) fn keep_unrecorded(&mut self, left: Writes) {
        self.last.keep_unrecorded(left);
    }

    /// The window's failure is `why`, which no read reached.
    pub(crate) fn failed(&mut self, why: Refusal) {
        self.fault = Some(why);
    }

    /// Why no VM's memory budget is held on this platform (C5).
    pub(crate) fn unheld(&self) -> &Refusal {
        &self.unheld
    }

    /// The VMs its last window couldn't read, and why: each counted whole, at its receipt, against the fleet's budget
    /// until a read succeeds (joint plan step 14; ROOT, 2026-11-13).
    #[cfg(test)]
    pub(crate) fn unread(&self) -> &[(LearnedHolder, Refusal)] {
        &self.unread
    }

    /// The cap each VM runs under now, by its lifetime.
    #[cfg(test)]
    fn rate(&self, scope: &Scope) -> Option<u64> {
        self.vms.get(scope).map(|vm| vm.rate)
    }

    /// What `class`'s capacity answer carries (C2, C8): the priors of `costs`, the class's learned use, and each of
    /// its running VMs' holdings, by their records' ids, those most past their receipts first. None after a failed
    /// window, so its VMs are booked whole.
    pub(crate) fn learned(
        &self,
        class: Class,
        costs: &[RuntimeCost],
        running: &[Holder<'_>],
    ) -> Result<Option<Learned>, Refusal> {
        if self.fault.is_some() {
            return Ok(None);
        }
        let mut vms = Vec::new();
        for record in running {
            let Some(scope) = record.scope() else {
                continue;
            };
            // One its last window couldn't read carries no holding, so the scheduler counts it at its receipt, whole
            // (C8; ROOT, 2026-11-13): the failure told where its booking would be.
            if self.unread.iter().any(|(id, _)| record.is(id)) {
                continue;
            }
            if let Some(vm) = self.vms.get(&scope)
                && vm.class == class
            {
                vms.push((vm.lifetime, &vm.id, vm.receipt));
            }
        }
        let mut learned = learned(&self.node, class, costs, vms, self.last.ended(class)?)?;
        learned.rates = self.rates(class, running)?;
        Ok(Some(learned))
    }

    /// What holds each of `class`'s VMs among `running` at its cap (D255), those its governor doesn't hold alone first:
    /// its platform's cap, then its governor under its job, then its governor; each in its records' order.
    fn rates(
        &self,
        class: Class,
        running: &[Holder<'_>],
    ) -> Result<Vec<hex_api::LearnedRate>, Refusal> {
        let rank = |held: &hex_api::RateHeld| match held {
            hex_api::RateHeld::Platform(_) => 0u8,
            hex_api::RateHeld::OwnTakesShare { .. } => 1,
            hex_api::RateHeld::GovernorOwnUnheld(_) => 2,
            hex_api::RateHeld::GovernorUnderJob => 3,
            hex_api::RateHeld::Governor => 4,
        };
        let mut held: Vec<(u8, LearnedHolder, &hex_api::RateHeld)> = Vec::new();
        held.try_reserve_exact(running.len().min(MAX_LEARNED_HOLDINGS))
            .map_err(|_| Refusal::Exhausted {
                what: "learned rates",
                bound: MAX_LEARNED_HOLDINGS,
            })?;
        for record in running {
            let Some(scope) = record.scope() else {
                continue;
            };
            // What its last cap's answer said; else what its governor's ask at first sight did (D255).
            let governed = self
                .governors
                .get(&scope)
                .filter(|governed| governed.class == class)
                .map(|governed| &governed.held);
            let how = match self.vms.get(&scope) {
                Some(vm) if vm.class == class => vm.held.as_ref().or(governed),
                Some(_) => None,
                None => governed,
            };
            if let Some(how) = how
                && held.len() < MAX_LEARNED_HOLDINGS
            {
                held.push((rank(how), record.id()?, how));
            }
        }
        held.sort_by_key(|(rank, _, _)| *rank);
        let mut rates = Vec::new();
        rates
            .try_reserve_exact(held.len())
            .map_err(|_| Refusal::Exhausted {
                what: "learned rates",
                bound: held.len(),
            })?;
        for (_, id, how) in held {
            rates.push(hex_api::LearnedRate {
                id,
                held: how.try_clone()?,
            });
        }
        Ok(rates)
    }

    /// Gives `capacity`, `class`'s capacity answer, what this learner carries (C2, C8), fitted to one message: the
    /// priors of the profiles it offers (its own and its routes'), and the holdings of its VMs among `records`, managed
    /// and Shards image VMs alike.
    pub(crate) fn answer<'a>(
        &self,
        class: Class,
        capacity: &mut hex_api::Capacity,
        records: impl IntoIterator<Item = Holder<'a>>,
    ) -> Result<(), Refusal> {
        let mut costs = Vec::new();
        costs
            .try_reserve_exact(capacity.routes.len().saturating_add(1))
            .map_err(|_| Refusal::Exhausted {
                what: "a capacity answer's cost profiles",
                bound: capacity.routes.len(),
            })?;
        costs.extend(capacity.cost);
        costs.extend(capacity.routes.iter().filter_map(|route| route.cost));
        let running: Vec<Holder<'a>> = records
            .into_iter()
            .filter(|record| record.managed() || record.image())
            .collect();
        capacity.learned = self.learned(class, &costs, &running)?;
        hex_core::wire::fit_learned(capacity).map_err(|e| Refusal::Invalid {
            what: "a capacity answer",
            why: e.to_string(),
        })
    }
}

/// What a capacity answer for `class` carries of `node` (C1, C2, C5, C8), shared by every OS's learner: the priors of
/// `costs`, each within its receipt, a cold class's its admission (at most one per profile and ceiling, none booked
/// whole), so a VM is booked below its whole receipt only where a learner learns its VMs' use; each profile's ceiling,
/// its envelope and the VMM overhead its VMs' windows have shown past it, each dimension cold, typed, until one has; the
/// class's learned use and each of its `vms`' holdings (by lifetime, box and receipt), those most past their receipts
/// first (memory, then CPU, then disk), and the class's `ended` VMs.
pub(crate) fn learned(
    node: &charge::Node<Lifetime>,
    class: Class,
    costs: &[RuntimeCost],
    vms: Vec<(Lifetime, &LearnedHolder, CapacityResources)>,
    ended: Vec<hex_api::LearnedEnd>,
) -> Result<Learned, Refusal> {
    let mut priors: Vec<hex_api::LearnedPrior> = Vec::new();
    let mut ceilings: Vec<hex_api::LearnedCeiling> = Vec::new();
    for cost in costs {
        let receipt = cost.allocate(CapacityResources::default())?;
        let key = ClassKey::of(class, receipt)?;
        // A prior books within its receipt, as a receipt's booking is (`Allocation::check`): what a VM's VMM takes past
        // its envelope reaches the scheduler through its holding (C8), never its prior.
        let ceiling = receipt.resources;
        let prior = node.prior(&key);
        let booking = CapacityResources {
            cpu_milli: prior.cpu_milli.min(ceiling.cpu_milli),
            mem_bytes: prior.mem_bytes.min(ceiling.mem_bytes),
            disk_bytes: prior.disk_bytes.min(ceiling.disk_bytes),
        };
        let known = priors
            .iter()
            .any(|prior| prior.profile == cost.id && prior.ceiling == ceiling);
        if booking != ceiling && !known && priors.len() < hex_api::MAX_LEARNED_PRIORS {
            priors.push(hex_api::LearnedPrior {
                profile: cost.id,
                ceiling,
                booking,
            });
        }
        let envelope = key.admission();
        let said = ceilings
            .iter()
            .any(|said| said.profile == cost.id && said.envelope == envelope);
        if !said && ceilings.len() < hex_api::MAX_LEARNED_PRIORS {
            let [cpu_milli, mem_bytes, disk_bytes] = node.overhead(&key);
            ceilings.push(hex_api::LearnedCeiling {
                profile: cost.id,
                envelope,
                overhead: hex_api::LearnedOverhead {
                    cpu_milli,
                    mem_bytes,
                    disk_bytes,
                },
            });
        }
    }
    let mut used = LearnedUse::default();
    let mut holdings = Vec::new();
    holdings
        .try_reserve_exact(vms.len())
        .map_err(|_| Refusal::Exhausted {
            what: "a class's learned holdings",
            bound: vms.len(),
        })?;
    for (lifetime, id, receipt) in vms {
        let holding = node
            .holding(&lifetime)
            .ok_or_else(|| Refusal::Broken("a learned VM without a holding".into()))?;
        used = used
            .checked_add(LearnedUse {
                booked: receipt,
                reservation: holding,
            })
            .ok_or_else(|| Refusal::Invalid {
                what: "a microVM's learned charge",
                why: "a class's learned use overflows".into(),
            })?;
        let past = (
            holding.mem_bytes.saturating_sub(receipt.mem_bytes),
            holding.cpu_milli.saturating_sub(receipt.cpu_milli),
            holding.disk_bytes.saturating_sub(receipt.disk_bytes),
        );
        holdings.push((
            past,
            hex_api::LearnedHolding {
                id: id.try_clone()?,
                holding,
            },
        ));
    }
    holdings.sort_by_key(|(past, _)| std::cmp::Reverse(*past));
    Ok(Learned {
        priors,
        used,
        holdings: holdings.into_iter().map(|(_, holding)| holding).collect(),
        ended,
        ceilings,
        // What holds each VM's rate its learner tells (`Learner::learned`, D255); Linux's holds each by its cpu.max.
        rates: Vec::new(),
    })
}

#[cfg(test)]
mod tests;
