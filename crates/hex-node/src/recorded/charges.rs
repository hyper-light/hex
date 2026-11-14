//! Joint plan step 14 on macOS and Windows (AC-T7.4.1; `docs/design/learned-charge.md` C1 to C5, C8): the charge
//! learner of a node whose platform caps a microVM's CPU rather than weighs it. Once a window, the longest any cap it
//! holds runs over, it reads each running VM it learns, learns its charge and booking (`hex_sim::charge`), and caps it
//! at its water-filled share (`charge::Node::rates`): a managed VM through its governor, its VM process's (D255), or
//! where that takes no rate through its native owner, macOS's managed control's rate or Windows' job's hard cap
//! ([`crate::recorded::Recover::set_managed_rate`], over that cap's cycle at the VM's share,
//! [`crate::recorded::Recover::cap_cycle`]); a Shards image VM through its rate (`Runtime::rate`, Shards' D247, over
//! its run's own period, read each window, [`crate::recorded::Recover::image_rate_period`]), its cost read from its run
//! host (`Runtime::host_use`, D241) and its guest told its room, its budget less what it holds (`Runtime::room`, D188).
//! A managed VM on a platform whose cap derives no cycle is booked whole off the fleet's budget, as a native box books
//! its caps, unless its governor answers a period ([`sort`]); holding no cap, the learner waits for a box to begin
//! ([`wait`]).
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
    BoxId, BoxInfo, CapacityResources, HostUse, Learned, LearnedEnd, LearnedHolder, LearnedUse,
    LearnedWindow, MAX_LEARNED_HOLDINGS, Refusal, RuntimeCost, Upstream, Usage,
};
use hex_core::record::{BoxRecord, NodeVmRecord, Phase};
use hex_runtime::guest::ManagedGuestTime;
use hex_sim::charge::{self, ChargeError, ClassKey, Cpu, Excess, Sample};
use hex_sim::model::Class;

use crate::store::Store;

/// One VM's lifetime, as its record names it (C3): a managed VM's native scope, its random bytes, or a Shards image VM's
/// place in its node's start order (`BoxRecord::started`), which its node gives each lifetime once; or a node VM's run,
/// which its node's store mints once a boot (joint plan step 39).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Scope {
    Native([u8; 16]),
    Started(u64),
    Run(hex_api::NodeRun),
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

/// What a learner holds a VM by (joint plan steps 14 and 39; ROOT, 2026-11-13): a box's record, or a node VM's row, a
/// VM the node runs for itself and a holder of its own in the node's ledger, with its band, what ending it ends
/// ([`node_band`]).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Holder<'a> {
    Box(&'a BoxRecord),
    /// A node VM's row, its band, and why its band is unknown, ranking it above every box, where it is ([`NodeBand`]).
    Node {
        row: &'a NodeVmRecord,
        band: u8,
        unknown: Option<&'a str>,
    },
}

impl<'a> Holder<'a> {
    /// Its holder in the node's ledger and on its class's capacity answers.
    pub(crate) fn id(self) -> Result<LearnedHolder, Refusal> {
        match self {
            Self::Box(record) => Ok(LearnedHolder::Box(record.spec.id.try_clone()?)),
            Self::Node { row, .. } => Ok(LearnedHolder::Node(row.run)),
        }
    }

    /// Whether it is holder `id`.
    pub(crate) fn is(self, id: &LearnedHolder) -> bool {
        match (self, id) {
            (Self::Box(record), LearnedHolder::Box(id)) => record.spec.id == *id,
            (Self::Node { row, .. }, LearnedHolder::Node(run)) => row.run == *run,
            (Self::Box(_), LearnedHolder::Node(_)) | (Self::Node { .. }, LearnedHolder::Box(_)) => {
                false
            }
        }
    }

    /// The lifetime it names ([`Scope::of`]): a node VM's its run.
    pub(crate) fn scope(self) -> Option<Scope> {
        match self {
            Self::Box(record) => Scope::of(record),
            Self::Node { row, .. } => Some(Scope::Run(row.run)),
        }
    }

    /// Its receipt.
    pub(crate) fn allocation(self) -> Option<hex_api::Allocation> {
        match self {
            Self::Box(record) => record.spec.allocation,
            Self::Node { row, .. } => Some(row.allocation),
        }
    }

    /// Its class: a node VM is a Shards microVM, booted through the image class.
    pub(crate) fn class(self) -> hex_api::Class {
        match self {
            Self::Box(record) => record.spec.class.class(),
            Self::Node { .. } => hex_api::Class::Shards,
        }
    }

    /// Its class as its learner learns it: a node VM's a class of its own, never its profile's box VMs' (ROOT,
    /// 2026-11-13), as it runs every oci box of its node.
    pub(crate) fn key(
        self,
        class: Class,
        allocation: hex_api::Allocation,
    ) -> Result<ClassKey, Refusal> {
        match self {
            Self::Box(_) => ClassKey::of(class, allocation),
            Self::Node { .. } => ClassKey::node(class, allocation),
        }
    }

    /// Its priority band.
    pub(crate) fn band(self) -> u8 {
        match self {
            Self::Box(record) => record.spec.band,
            Self::Node { band, .. } => band,
        }
    }

    /// Its place in its node's start order.
    pub(crate) fn started(self) -> Option<u64> {
        match self {
            Self::Box(record) => record.started,
            Self::Node { row, .. } => row.started,
        }
    }

    /// Why a node before this one was ending it, where its record says so (C5).
    pub(crate) fn intent(self) -> Option<&'a LearnedEnd> {
        match self {
            Self::Box(record) => record.last_resort.as_ref(),
            Self::Node { row, .. } => row.last_resort.as_ref(),
        }
    }

    /// Whether it is a managed microVM that runs now ([`running`]).
    pub(crate) fn managed(self) -> bool {
        match self {
            Self::Box(record) => running(record),
            Self::Node { .. } => false,
        }
    }

    /// Whether it is a Shards image VM made and not ended ([`running_image`]), as a node VM that runs is: Ready, its run
    /// not ended.
    pub(crate) fn image(self) -> bool {
        match self {
            Self::Box(record) => running_image(record),
            Self::Node { row, .. } => {
                row.phase == hex_api::NodeVmPhase::Ready && row.exit.is_none()
            }
        }
    }
}

/// A node VM's band, and why it is unknown where it is (ROOT, 2026-11-13: told, never silent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NodeBand {
    pub(crate) band: u8,
    pub(crate) unknown: Option<String>,
}

/// A node VM's band (joint plan step 39, place 3; ROOT, 2026-11-13): what ending it ends. It runs its node's `oci`
/// class, so ending it ends every box of that class whose run hasn't ended, and its band is the highest of theirs among
/// its node's `records`, none ranking it lowest (it ends no box). A record that can't be read may be of a box it runs,
/// of any band, so then it ranks above every box: the highest band there is, and why, naming that box and its record's
/// words (ROOT, 2026-11-13: an unknown band told, never silent).
pub(crate) fn node_band<'r>(
    records: impl IntoIterator<Item = (&'r BoxId, &'r Result<BoxRecord, String>)>,
) -> NodeBand {
    let mut band = 0;
    for (id, record) in records {
        match record {
            Err(why) => {
                return NodeBand {
                    band: u8::MAX,
                    unknown: Some(format!("box {id}'s record can't be read: {why}")),
                };
            }
            Ok(record) if runs_in_node_vm(record) => {
                band = band.max(record.spec.band);
            }
            Ok(_) => {}
        }
    }
    NodeBand {
        band,
        unknown: None,
    }
}

/// Whether `record`'s box runs in its node's node VM where one runs its node's `oci` class (place 3, A-6): an oci box
/// whose run hasn't ended.
fn runs_in_node_vm(record: &BoxRecord) -> bool {
    record.spec.class.class() == hex_api::Class::Oci && record.exit.is_none()
}

/// The node VM whose last-resort end ended `record`'s box, where its record names one (A-6): an oci box's, not yet
/// retired, that ran when its node VM was ended.
fn ended_by_node_vm(record: &BoxRecord) -> Option<hex_api::NodeRun> {
    if record.retired || !runs_in_node_vm(record) {
        return None;
    }
    match record.last_resort.as_ref()?.id {
        LearnedHolder::Node(run) => Some(run),
        LearnedHolder::Box(_) => None,
    }
}

/// The class a VM of `class` is learned in: a Shards or Firecracker microVM's; none for a native box's.
fn vm_class(class: hex_api::Class) -> Option<Class> {
    match class {
        hex_api::Class::Shards => Some(Class::Shards),
        hex_api::Class::Firecracker => Some(Class::Firecracker),
        hex_api::Class::Process | hex_api::Class::Oci => None,
    }
}

/// The node VMs of `class` among `running` ranked above every box, their bands unknown, each by its run and why
/// ([`NodeBand`]; ROOT, 2026-11-13: an unknown band told, never silent).
fn ranked(class: Class, running: &[Holder<'_>]) -> Result<Vec<hex_api::LearnedRank>, Refusal> {
    let mut told = Vec::new();
    for record in running {
        let Holder::Node {
            row,
            unknown: Some(why),
            ..
        } = record
        else {
            continue;
        };
        if vm_class(record.class()) != Some(class) {
            continue;
        }
        told.try_reserve(1).map_err(|_| Refusal::Exhausted {
            what: "learned ranked VMs",
            bound: MAX_LEARNED_HOLDINGS,
        })?;
        told.push(hex_api::LearnedRank {
            run: row.run,
            why: (*why).to_owned(),
        });
    }
    Ok(told)
}

/// What the node VMs among `rows` that boot (Booting, their runs not ended) book (ROOT, 2026-11-13): each its receipt's
/// booking, as the scheduler books it from its row's first write, off the fleet's budget until it is Ready and learned,
/// so no room or rate hands out what its boot is using.
pub(crate) fn booting(rows: &[NodeVmRecord]) -> Result<CapacityResources, Refusal> {
    rows.iter()
        .filter(|row| row.phase == hex_api::NodeVmPhase::Booting && row.exit.is_none())
        .try_fold(CapacityResources::default(), |sum, row| {
            sum.checked_add(row.allocation.booking())
        })
        .ok_or_else(|| Refusal::Invalid {
            what: "a node's booting node VMs' bookings",
            why: "they overflow".into(),
        })
}

/// The node VMs among `rows` that run (Ready, their runs not ended), as the holders a learner reads, each of `band`.
pub(crate) fn nodes<'a>(
    rows: &'a [NodeVmRecord],
    band: &'a NodeBand,
) -> Result<Vec<Holder<'a>>, Refusal> {
    let mut holders = Vec::new();
    holders
        .try_reserve_exact(rows.len())
        .map_err(|_| Refusal::Exhausted {
            what: "a window's VMs",
            bound: rows.len(),
        })?;
    holders.extend(
        rows.iter()
            .map(|row| Holder::Node {
                row,
                band: band.band,
                unknown: band.unknown.as_deref(),
            })
            .filter(|holder| holder.image()),
    );
    Ok(holders)
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
    /// The least share the VM's actuator holds it at (C4; ROOT, 2026-11-13): an image VM's governor's
    /// ([`GOVERNOR_LEAST`]), a managed VM's its platform's cap's, which holds its share whatever its governor does.
    fn least(&self, holder: Holder<'_>) -> u64;
    /// The cycle its platform's cap holds a managed VM at `rate` over (C1; ROOT, 2026-11-13): macOS's CPU control's, its
    /// run and its hold, so the VM keeps its rate on average over one; none at a rate it never holds a VM below.
    /// Refused where its platform derives none (Windows' job: its interval undocumented).
    fn cap_cycle(&self, rate: u64) -> Result<Option<Duration>, Refusal>;
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
    /// Tells an image VM's guest its room, `room` bytes past what it holds (`Runtime::room`, D188); a node VM's through its
    /// run, as an image VM's.
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
        let rate = match self.own {
            Own::Read(own) => self.share.saturating_sub(own),
            Own::Unheld(_) => self.share,
        };
        NonZeroU64::new(rate).unwrap_or(GOVERNOR_LEAST)
    }

    /// What holds the VM at its share, its governor holding its vCPUs to [`Cap::governor`], on a platform whose `job`
    /// holds its whole CPU at the share beside (Windows) or none: its governor, its VMM's own read and left out; its
    /// VMM's own taking the whole share, a finding; and while its VMM's own isn't read apart, its governor at the whole
    /// share under its job, or with nothing holding its VMM's own, why told.
    pub(crate) fn held(self, job: bool) -> hex_api::RateHeld {
        match self.own {
            Own::Read(own) if own >= self.share => hex_api::RateHeld::OwnTakesShare {
                own,
                share: self.share,
            },
            Own::Read(_) => hex_api::RateHeld::Governor,
            Own::Unheld(_) if job => hex_api::RateHeld::GovernorUnderJob,
            Own::Unheld(why) => hex_api::RateHeld::GovernorOwnUnheld(why.to_owned()),
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
/// receipt's allocation, as no cap holds it below; and what the native boxes book, their caps: its process boxes', and
/// its oci boxes' where no node VM runs them (A-5).
#[derive(Debug, Default)]
pub(crate) struct Sorted<'a> {
    pub(crate) learned: Vec<Holder<'a>>,
    pub(crate) whole: CapacityResources,
    pub(crate) native: CapacityResources,
}

/// Sorts `records` for a window of a learner ([`Sorted`]): it learns each VM whose cap's period it reads over (C1), a
/// managed VM where `learns` says so ([`Learner::learns`]) and every image VM, its cap's period its run's own
/// ([`crate::recorded::Recover::image_rate_period`]). A native box books its caps, an oci box only where no node VM on
/// record (`rows`) runs the node's oci class (A-5; ROOT, 2026-11-13): a node VM holds the boxes it runs, in its
/// footprint while it runs and its booking while it boots, and nothing holds them once it has ended, so their caps are
/// never counted beside it.
pub(crate) fn sort<'a>(
    records: impl IntoIterator<Item = &'a BoxRecord>,
    rows: &[NodeVmRecord],
    learns: impl Fn(Holder<'_>) -> bool,
) -> Result<Sorted<'a>, Refusal> {
    let overflow = || Refusal::Invalid {
        what: "a node's boxes' bookings",
        why: "they overflow".into(),
    };
    // A node VM on record runs the node's oci class: its guest is the one that serves it (`Store::put_node_vm`).
    let hosted = !rows.is_empty();
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
                && match record.spec.class.class() {
                    hex_api::Class::Process => true,
                    hex_api::Class::Oci => !hosted,
                    hex_api::Class::Shards | hex_api::Class::Firecracker => false,
                } =>
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

/// A learner as its window reaches it (A-2; the review's, 2026-11-14): taken for each of the window's pure steps and let
/// go between them, so no VM is asked and no file read or written while the lock its capacity answers take is held: its
/// node's lock, or a learner a caller holds alone.
pub(crate) trait Lease<T> {
    type Held<'g>: std::ops::DerefMut<Target = T>
    where
        Self: 'g;
    fn take(&mut self) -> Result<Self::Held<'_>, Refusal>;
}

impl<T> Lease<T> for &std::sync::Mutex<T> {
    type Held<'g>
        = std::sync::MutexGuard<'g, T>
    where
        Self: 'g;
    fn take(&mut self) -> Result<Self::Held<'_>, Refusal> {
        self.lock()
            .map_err(|_| Refusal::Broken("the charge learner's lock was poisoned".into()))
    }
}

impl<T> Lease<T> for &mut T {
    type Held<'g>
        = &'g mut T
    where
        Self: 'g;
    fn take(&mut self) -> Result<Self::Held<'_>, Refusal> {
        Ok(&mut **self)
    }
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
        let guest_ns = match io.guest_time(holder)? {
            ManagedGuestTime::PerVcpu(times) => Ok(times),
            ManagedGuestTime::Unheld(why) => Err(why),
        };
        let usage = io.usage(holder)?;
        let life = match (&guest_ns, first) {
            (Ok(_), true) => Some(io.life(holder)?),
            _ => None,
        };
        Ok(Self {
            cpu_ns: usage.cpu_usec.saturating_mul(1_000),
            memory_bytes: usage.memory_bytes,
            guest_ns,
            life,
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
pub(crate) fn ran(
    cpu_ns: u64,
    now: Option<&[u64]>,
    before: Option<&[u64]>,
) -> Result<Cpu, ChargeError> {
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

/// What a window's last resort decided (C5) under its learner's lock, to be carried out outside it (A-2).
#[derive(Debug)]
pub(crate) enum Decision {
    /// A VM it ended still runs: nothing more, so one end is never two.
    Wait,
    /// The oldest end a node before this one decided, carried out as it decided it.
    Due {
        scope: Scope,
        class: Class,
        end: LearnedEnd,
    },
    /// Nothing to end: held, or refused, typed, every VM within its own budget.
    Set(Resort),
    /// End the VM of `scope` for `why`, `end` written into its record first (intent first).
    End {
        scope: Scope,
        class: Class,
        end: LearnedEnd,
        why: Excess,
    },
}

/// A last resort's decision carried out ([`Decision::carry`]): what each end and intent answered.
#[derive(Debug)]
pub(crate) enum Carried {
    Wait,
    Due {
        scope: Scope,
        class: Class,
        end: LearnedEnd,
        ended: Result<(), Refusal>,
    },
    Set(Resort),
    End {
        scope: Scope,
        class: Class,
        end: LearnedEnd,
        why: Excess,
        written: Result<bool, Refusal>,
        ended: Result<(), Refusal>,
    },
}

impl Decision {
    /// Carries the decision out through `reach` (outside its learner's lock; A-2): an end due is made, and an end the
    /// rule chose is made after its why is written into its VM's record (intent first, C5), whether that write is made or
    /// refused, the fleet's room coming first.
    pub(crate) fn carry(self, reach: &impl Reach) -> Carried {
        match self {
            Self::Wait => Carried::Wait,
            Self::Due { scope, class, end } => Carried::Due {
                ended: reach.end(&scope),
                scope,
                class,
                end,
            },
            Self::Set(resort) => Carried::Set(resort),
            Self::End {
                scope,
                class,
                end,
                why,
            } => {
                let written = reach.intend(&end.id, scope, &end);
                let ended = reach.end(&scope);
                Carried::End {
                    scope,
                    class,
                    end,
                    why,
                    written,
                    ended,
                }
            }
        }
    }
}

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

    /// One window's last resort, decided once the placement holds what it can (under its learner's lock; A-2): while a
    /// VM it ended still runs (`vm` names it), nothing more, so one end is never two; else an end a node before this one
    /// decided, as it decided it; else the core's verdict on `node`, the VM it chooses to end, or refused, typed, where
    /// the node can't act. `vm` names each VM the learner holds: its class and box. Nothing is kept until its decision
    /// is carried out ([`Decision::carry`]) and what that did kept ([`LastResort::settle`]).
    pub(crate) fn decide<'a>(
        &self,
        node: &charge::Node<Lifetime>,
        vm: impl Fn(&Scope) -> Option<(Class, &'a LearnedHolder)>,
    ) -> Result<Decision, Refusal> {
        if let Resort::Ending { scope, .. } = &self.resort
            && vm(scope).is_some()
        {
            return Ok(Decision::Wait);
        }
        if let Some((scope, class, end)) = self.due.front() {
            return Ok(Decision::Due {
                scope: *scope,
                class: *class,
                end: end.try_clone()?,
            });
        }
        Ok(match node.last_resort().map_err(refused)? {
            charge::Resort::Held => Decision::Set(Resort::Held),
            charge::Resort::Within(fleet) => Decision::Set(Resort::Refused(Refusal::Exhausted {
                what: "a microVM fleet's memory budget, every VM within its own",
                bound: usize::try_from(fleet.budget).unwrap_or(usize::MAX),
            })),
            charge::Resort::End { vm: lifetime, why } => {
                let scope = lifetime.scope;
                let (class, id) = vm(&scope).ok_or_else(|| {
                    Refusal::Broken("the last resort chose a VM the learner doesn't hold".into())
                })?;
                Decision::End {
                    scope,
                    class,
                    end: LearnedEnd {
                        id: id.try_clone()?,
                        held: why.held,
                        budget: why.budget,
                        band: why.band,
                        fleet_held: why.fleet.held,
                        fleet_growth: why.fleet.growth,
                        fleet_budget: why.fleet.budget,
                        unmoved: move_awaits(class),
                    },
                    why,
                }
            }
        })
    }

    /// Keeps what carrying a window's decision out did (C5; under its learner's lock, A-2): an end made is remembered for
    /// its class's answers and its VM's ending awaited; an end refused is kept typed and withdraws its intent; an intent
    /// that couldn't be written held no end, and is written after it, its write's refusal the window's.
    pub(crate) fn settle(&mut self, carried: Carried) -> Result<(), Refusal> {
        match carried {
            Carried::Wait => Ok(()),
            Carried::Due {
                scope,
                class,
                end,
                ended,
            } => {
                // The end carried out is the oldest due, which its decision took.
                if self.due.front().is_some_and(|(due, ..)| *due == scope) {
                    self.due.pop_front();
                }
                self.resort = match ended {
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
                Ok(())
            }
            Carried::Set(resort) => {
                self.resort = resort;
                Ok(())
            }
            Carried::End {
                scope,
                class,
                end,
                why,
                written,
                ended,
            } => {
                // What was kept for the VM's record before is this end's to say now.
                self.unrecorded.retain(|(_, kept, _)| *kept != scope);
                // Its intent first. A write refused doesn't hold the end, the fleet's room coming first: the VM is
                // ended, why written after the window, and the window refused with the write's own refusal, typed, as
                // a write after it is when it fails ([`write_ends`]; ROOT, 2026-11-13).
                let (written, unwritten) = match written {
                    Ok(written) => (written, None),
                    Err(why) => (false, Some(why)),
                };
                self.resort = match ended {
                    Ok(()) => {
                        if !written {
                            self.write(end.id.try_clone()?, scope, Some(end.try_clone()?))?;
                        }
                        self.remember(class, &end)?;
                        Resort::Ending {
                            scope,
                            id: end.id.try_clone()?,
                            why,
                        }
                    }
                    Err(refusal) => {
                        if written {
                            self.write(end.id.try_clone()?, scope, None)?;
                        }
                        Resort::Refused(refusal)
                    }
                };
                unwritten.map_or(Ok(()), Err)
            }
        }
    }

    /// Carries `ends`, the last-resort ends of the node VMs its node's start read back from their rows before letting
    /// them go (joint plan step 39, place 1), into its ends, each a Shards VM's.
    pub(crate) fn carry(&mut self, ends: Vec<LearnedEnd>) -> Result<(), Refusal> {
        for end in &ends {
            self.remember(Class::Shards, end)?;
        }
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
/// its own (`turns::tests::a_wait_past_the_slowest_hold_follows_the_holder_and_learns_its_hold`). A node VM's goes into
/// its row (joint plan step 39, place 1), its lifetime its run, under the lock every write of its row takes
/// (`Store::update_node_vm`).
pub(crate) fn record_last_resort(
    store: &Store,
    id: &LearnedHolder,
    scope: Scope,
    end: Option<&LearnedEnd>,
) -> Result<bool, Refusal> {
    let end = end.map(LearnedEnd::try_clone).transpose()?;
    match id {
        LearnedHolder::Box(id) => store.update(id, move |record| {
            if Scope::of(record) != Some(scope) {
                return false;
            }
            record.last_resort = end;
            true
        }),
        LearnedHolder::Node(run) if scope == Scope::Run(*run) => {
            let row = end.as_ref().map(LearnedEnd::try_clone).transpose()?;
            let written = store.update_node_vm(run, move |held| {
                held.last_resort = row;
                true
            })?;
            if written {
                hosted_ends(store, *run, end.as_ref())?;
            }
            Ok(written)
        }
        LearnedHolder::Node(_) => Ok(false),
    }
}

/// Writes `end`, why the last resort is ending node VM `run`, into the record of each oci box it runs, which its end
/// ends too, or withdraws it (none) from each naming it (A-6; ROOT, 2026-11-13: each box says why), every write waiting
/// for its box's record turn as [`record_last_resort`]'s does. A record that can't be read can't be written: its band
/// is told unknown ([`node_band`]).
fn hosted_ends(
    store: &Store,
    run: hex_api::NodeRun,
    end: Option<&LearnedEnd>,
) -> Result<(), Refusal> {
    let node = LearnedHolder::Node(run);
    let names = |record: &BoxRecord| {
        record
            .last_resort
            .as_ref()
            .is_some_and(|said| said.id == node)
    };
    for (id, record) in store.all()? {
        let Ok(record) = record else {
            continue;
        };
        let due = match end {
            Some(_) => runs_in_node_vm(&record) && !record.retired,
            None => names(&record),
        };
        if !due {
            continue;
        }
        let end = end.map(LearnedEnd::try_clone).transpose()?;
        store.update(&id, |record| {
            let still = match &end {
                Some(_) => runs_in_node_vm(record) && !record.retired,
                None => names(record),
            };
            if still {
                record.last_resort = end;
            }
            still
        })?;
    }
    Ok(())
}

/// A-6 (ROOT, 2026-11-13: ending a node VM ends every oci box it runs, and each must say why): retires each oci box
/// among `records` whose record names a node VM's last-resort end once that node VM's run has ended, its row among
/// `rows` Ended or gone, for `NodeVmEnded`: kept, Ready, its end unknown and why on its record, until its owner's
/// delete. Answers the boxes it retired.
pub(crate) fn retire_with_node_vms(
    store: &Store,
    records: &BTreeMap<BoxId, Result<BoxRecord, String>>,
    rows: &[NodeVmRecord],
) -> Result<Vec<BoxId>, Refusal> {
    let mut retired = Vec::new();
    for (id, record) in records {
        let Some(run) = record.as_ref().ok().and_then(ended_by_node_vm) else {
            continue;
        };
        // Its node VM still runs, or still boots: the box may still run in it.
        if rows
            .iter()
            .any(|row| row.run == run && row.phase != hex_api::NodeVmPhase::Ended)
        {
            continue;
        }
        let done = store.update(id, |record| {
            if ended_by_node_vm(record) != Some(run) {
                return false;
            }
            record.retire_for(hex_api::LifecycleCause::NodeVmEnded);
            true
        })?;
        if done {
            retired.try_reserve(1).map_err(|_| Refusal::Exhausted {
                what: "the boxes retired with their node VMs",
                bound: records.len(),
            })?;
            retired.push(id.try_clone()?);
        }
    }
    Ok(retired)
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
    /// The least share its actuator holds it at (C4): [`Managed::least`].
    least: u64,
    /// The room its guest was told last, an image VM's (D188): none until it is first told.
    room: Option<i64>,
    /// What telling its guest its room awaits, as its runtime answered the first telling (C7; place 4): a managed
    /// Shards VM's guest, whose init has no room call from Hex, awaits `GuestRoom`; none once a guest was told.
    awaits: Option<Upstream>,
    /// The period its cap holds over as last read: an image VM's its run's own (D247), a managed VM's its governor's
    /// (D255); none for a managed VM whose governor hasn't answered or whose platform's cap holds it, its platform's
    /// window holding instead.
    period: Option<Duration>,
    /// What holds it at its cap (D255): none until the learner first caps it.
    held: Option<hex_api::RateHeld>,
    /// Its platform's cap's cycle at its share, where that cap holds it (`RateHeld::Platform`): the span its rate holds
    /// over on average, which its learner's window takes (C1; ROOT, 2026-11-13).
    cycle: Option<Duration>,
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

/// What a governor a window asked answered (D255): what holds its VM's rate and whether it answered a period, or why its
/// ask failed.
type Governor = Result<(hex_api::RateHeld, bool), Refusal>;

/// A managed VM whose governor a window asks at first sight (D255): decided under its learner's lock and asked outside
/// it (A-2), at its ceiling, its allocation's CPU.
struct Governing<'h> {
    holder: Holder<'h>,
    scope: Scope,
    class: Class,
    share: u64,
}

/// What a window asks of a VM it learns (A-2): decided under its learner's lock, asked outside it.
struct Ask<'h> {
    holder: Holder<'h>,
    /// Whether its learner holds no split of it yet, so its life is read for its first (D260, [`Read::of`]).
    first: bool,
}

/// What a VM a window learns answered (C1, D260): the period its cap holds over and its reading, or why not; and the
/// least share its actuator holds it at ([`Managed::least`]).
struct Answer<'h> {
    holder: Holder<'h>,
    read: Result<(Option<Duration>, Read), Refusal>,
    least: u64,
}

/// A VM to cap (C4, D260): its share, and its VMM's own as last read, which its governor's rate leaves out, or why not.
struct Capping<'h> {
    holder: Holder<'h>,
    scope: Scope,
    share: u64,
    own: Result<u64, String>,
}

/// What a window acts on once its learner has learned (C4, C5, C7), outside its lock (A-2), in order: each VM whose cap
/// moved, each guest whose room moved, and its last resort's decision.
struct Acts<'h> {
    caps: Vec<Capping<'h>>,
    rooms: Vec<(Holder<'h>, Scope, i64)>,
    resort: Decision,
}

/// A VM a window capped: its share, its VMM's own its governor's rate left out, what holds it there, and its platform
/// cap's cycle at its share where that cap holds it.
struct Capped {
    scope: Scope,
    share: u64,
    told: Option<u64>,
    held: hex_api::RateHeld,
    cycle: Option<Duration>,
}

/// What a window's acts did, in order, to the first that failed: each VM capped, each guest told its room or what
/// telling it awaits, and its last resort carried out, where the acts reached it.
struct Acted {
    caps: Vec<Capped>,
    rooms: Vec<(Scope, Result<i64, Upstream>)>,
    resort: Option<Carried>,
    failed: Option<Refusal>,
}

/// The node's learner (module docs).
pub(crate) struct Learner {
    node: charge::Node<Lifetime>,
    vms: BTreeMap<Scope, Vm>,
    /// Whether its platform's cap holds a managed VM over a cycle it derives (C1, [`Managed::cap_cycle`]; macOS's
    /// control): where none does (Windows' job, its interval undocumented), its managed VMs are learned over their
    /// governors' periods alone, the rest booked whole off the fleet's budget ([`sort`]).
    caps: bool,
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
    /// A learner on a machine of `machine` whose platform's cap holds managed VMs over cycles it derives where `caps`:
    /// with no cap held it reads over no window, and waits for a box to begin (C1, [`wait`]).
    pub(crate) fn new(caps: bool, machine: CapacityResources) -> Result<Self, Refusal> {
        let bounds = bounds(machine)?;
        Ok(Self {
            node: charge::Node::new(None, machine, bounds).map_err(refused)?,
            vms: BTreeMap::new(),
            caps,
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

    /// Whether it learns managed VMs: where their platform's cap holds them over a cycle it derives (C1).
    pub(crate) fn manages(&self) -> bool {
        self.caps
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

    /// The managed VMs among `records` whose governors a window asks (D255; ROOT, 2026-11-13), on a platform with no
    /// window: each it hasn't asked, and each whose ask failed. Those no longer running are forgotten.
    fn governing<'h>(&mut self, records: &[Holder<'h>]) -> Result<Vec<Governing<'h>>, Refusal> {
        self.governors
            .retain(|scope, _| records.iter().any(|record| record.scope() == Some(*scope)));
        let mut asks = Vec::new();
        if self.manages() {
            return Ok(asks);
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
            let Some(class) = vm_class(record.class()) else {
                continue;
            };
            asks.try_reserve(1).map_err(|_| Refusal::Exhausted {
                what: "a window's governors asked",
                bound: records.len(),
            })?;
            asks.push(Governing {
                holder: record,
                scope,
                class,
                share: allocation.resources.cpu_milli,
            });
        }
        Ok(asks)
    }

    /// Keeps what each governor a window asked answered (D255): what holds its VM's rate, and whether it answered a
    /// period, so its VM is learned over it; an ask that failed holds its VM booked whole, told, asked again next window.
    fn governed(&mut self, answers: Vec<(Governing<'_>, Governor)>) {
        for (ask, asked) in answers {
            let (held, period, again) = match asked {
                Ok((held, period)) => (held, period, false),
                Err(why) => (hex_api::RateHeld::Platform(why.to_string()), false, true),
            };
            self.governors.insert(
                ask.scope,
                Governed {
                    class: ask.class,
                    held,
                    period,
                    again,
                },
            );
        }
    }

    /// Asks the governors of `records`' managed VMs as a window does ([`govern`]), the learner held alone.
    #[cfg(test)]
    pub(crate) fn govern(
        &mut self,
        io: &impl Managed,
        records: &[Holder<'_>],
    ) -> Result<(), Refusal> {
        govern(self, io, records)
    }

    /// The window it reads over next (C1): the longest of its platform's and the periods of the image VMs it holds,
    /// none while it holds none and its platform has none ([`wait`]).
    pub(crate) fn next_window(&self) -> Option<Duration> {
        self.node.window()
    }

    /// The longest period of the caps it holds now (C1): each image VM's its run's own and each governed managed VM's its
    /// governor's, as last read, and each VM its platform's cap holds its cap's cycle at its share ([`Vm::cycle`]); and,
    /// where such a cycle is the longest, past every period, the VM it is (ROOT, 2026-11-13: every VM's read waits on it,
    /// so it is told). None while it holds no cap.
    fn longest_by(&self) -> Option<(Duration, Option<&LearnedHolder>)> {
        let mut longest: Option<(Duration, Option<&LearnedHolder>)> = None;
        for vm in self.vms.values() {
            for (span, by) in [(vm.period, None), (vm.cycle, Some(&vm.id))] {
                let Some(span) = span else {
                    continue;
                };
                if longest.is_none_or(|(most, _)| span > most) {
                    longest = Some((span, by));
                }
            }
        }
        longest
    }

    /// The longest period of the caps it holds now ([`Learner::longest_by`]).
    fn longest(&self) -> Option<Duration> {
        self.longest_by().map(|(span, _)| span)
    }

    /// Its window as its capacity answers tell it (C1; ROOT, 2026-11-13): the longest period of the caps it holds, and
    /// the VM whose platform cap's cycle sets it, where one does; none while it holds none.
    fn told_window(&self) -> Result<Option<LearnedWindow>, Refusal> {
        self.longest_by()
            .map(|(span, by)| {
                Ok(LearnedWindow {
                    window_ns: nanos(span),
                    slowed_by: by.map(LearnedHolder::try_clone).transpose()?,
                })
            })
            .transpose()
    }

    /// Runs a window as a node does ([`window`]), the learner held alone.
    #[cfg(test)]
    pub(crate) fn window(
        &mut self,
        io: &impl Managed,
        running: &[Holder<'_>],
        budget: CapacityResources,
        now: Instant,
    ) -> Result<(), Refusal> {
        window(self, io, running, budget, now)
    }

    /// What a window asks of the VMs it learns among `running` (A-2): each one's reading, its life too where it holds no
    /// split of it yet (D260).
    fn asks<'h>(&self, running: &[Holder<'h>]) -> Result<Vec<Ask<'h>>, Refusal> {
        let mut asks = Vec::new();
        asks.try_reserve_exact(running.len())
            .map_err(|_| Refusal::Exhausted {
                what: "a window's VMs",
                bound: running.len(),
            })?;
        for &holder in running {
            let Some(scope) = holder.scope() else {
                continue;
            };
            if holder.allocation().is_none() {
                continue;
            }
            // A VM whose last reading split it is read as one; else its first split is over its life so far (D260).
            asks.push(Ask {
                holder,
                first: self.vms.get(&scope).is_none_or(|vm| vm.guest_ns.is_none()),
            });
        }
        Ok(asks)
    }

    /// Learns a window's `answers` at `now` (C1 to C5, C7; under its lock, A-2), its VMs `running`, within the fleet's
    /// `budget`: what it then acts on outside its lock ([`act`]).
    fn learn<'h>(
        &mut self,
        running: &[Holder<'h>],
        answers: Vec<Answer<'h>>,
        budget: CapacityResources,
        now: Instant,
    ) -> Result<Acts<'h>, Refusal> {
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
        for answer in answers {
            let record = answer.holder;
            let (Some(scope), Some(allocation)) = (record.scope(), record.allocation()) else {
                continue;
            };
            // Each window reads the VM's use and the period its cap holds over (C1): an image VM's its run's own (D247),
            // a managed VM's its governor's as its VM process last answered (D255), none before. A VM either read fails
            // for counts whole, at its receipt, against the fleet's budget this window, its learner untouched (ROOT,
            // 2026-11-13: never starving its neighbours by holding nothing): no sample taken, its booking as it was,
            // and the failure told ([`Learner::unread`]). One that ended since its listing is ended by the next.
            let (period, read) = match answer.read {
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
                match read.guest_ns {
                    Ok(guest) => {
                        vm.guest_ns = Some(guest);
                        if let Some(own) = own {
                            vm.own = Ok(own);
                        }
                    }
                    Err(why) => {
                        vm.guest_ns = None;
                        vm.own = Err(why);
                    }
                }
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
                    record.key(class, allocation)?,
                    record.band(),
                    vcpus(allocation)?,
                    receipt,
                )
                .map_err(refused)?;
            // Its first split, from its first reading holding its guest time: over its life so far (D260).
            let own = read.own_over_life();
            let (guest_ns, own) = match read.guest_ns {
                Ok(guest) => (
                    Some(guest),
                    own.ok_or_else(|| "its life so far reads none".to_owned()),
                ),
                Err(why) => (None, Err(why)),
            };
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
                    least: answer.least,
                    room: None,
                    awaits: None,
                    period,
                    held: None,
                    cycle: None,
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
        let vms = &self.vms;
        let rates = self
            .node
            .rates(budget.cpu_milli, &used, |lifetime| {
                vms.get(&lifetime.scope)
                    .map_or(GOVERNOR_LEAST.get(), |vm| vm.least)
            })
            .map_err(refused)?;
        let mut caps = Vec::new();
        caps.try_reserve_exact(rates.len())
            .map_err(|_| Refusal::Exhausted {
                what: "a window's caps",
                bound: rates.len(),
            })?;
        for (lifetime, rate) in rates {
            let scope = lifetime.scope;
            let Some(vm) = self.vms.get(&scope) else {
                continue;
            };
            // Its governor is told its share less its VMM's own as last read (D260), each time either moves, and once
            // at first sight, so what holds it is told from its first window.
            let told = vm.own.as_ref().ok().copied();
            if vm.held.is_some() && vm.rate == rate && vm.told == told {
                continue;
            }
            let holder = running
                .iter()
                .find(|record| scope_of(record) == Some(scope))
                .ok_or_else(|| Refusal::Broken("a learned VM without its record".into()))?;
            caps.push(Capping {
                holder: *holder,
                scope,
                share: rate,
                own: vm.own.clone(),
            });
        }
        // Each Shards VM's guest told its room as it moves (C5, C7; D188), a node VM's as an image VM's (joint plan
        // step 39): its budget less what it holds, everything it holds coming out of it, negative where its budget is
        // short of that. A guest its runtime can't yet tell (a managed VM's) is kept as awaiting what that needs, told
        // on its class's answers and never asked again (place 4); a Firecracker guest has no room to be told.
        let budgets = self.node.budgets().map_err(refused)?;
        let mut rooms = Vec::new();
        rooms
            .try_reserve_exact(budgets.len())
            .map_err(|_| Refusal::Exhausted {
                what: "a window's rooms",
                bound: budgets.len(),
            })?;
        for (lifetime, budget) in budgets {
            let scope = lifetime.scope;
            let Some(vm) = self.vms.get(&scope) else {
                continue;
            };
            if vm.class != Class::Shards || vm.awaits.is_some() || vm.room == Some(budget.room) {
                continue;
            }
            let holder = running
                .iter()
                .find(|record| scope_of(record) == Some(scope))
                .ok_or_else(|| Refusal::Broken("a learned VM without its record".into()))?;
            rooms.push((*holder, scope, budget.room));
        }
        let vms = &self.vms;
        let resort = self.last.decide(&self.node, |scope| {
            vms.get(scope).map(|vm| (vm.class, &vm.id))
        })?;
        Ok(Acts {
            caps,
            rooms,
            resort,
        })
    }

    /// Keeps what a window's acts did (under its lock; A-2), or why its learning failed, and its outcome, which is kept
    /// until a window passes. Its next window is the longest period of the caps it holds whatever this one reached (C1);
    /// one that settled set it already, before its last resort.
    fn acted(&mut self, acted: Result<Acted, Refusal>) -> Result<(), Refusal> {
        let outcome = acted.and_then(|acted| self.keep(acted));
        let outcome = outcome.and(self.node.set_window(self.longest()).map_err(refused));
        self.fault = outcome.as_ref().err().cloned();
        outcome
    }

    /// Keeps each cap and room a window's acts made, and what its last resort did, to the first act that failed, whose
    /// refusal is the window's.
    fn keep(&mut self, acted: Acted) -> Result<(), Refusal> {
        for capped in acted.caps {
            if let Some(vm) = self.vms.get_mut(&capped.scope) {
                vm.cycle = capped.cycle;
                vm.held = Some(capped.held);
                vm.rate = capped.share;
                vm.told = capped.told;
            }
        }
        for (scope, room) in acted.rooms {
            if let Some(vm) = self.vms.get_mut(&scope) {
                match room {
                    Ok(room) => vm.room = Some(room),
                    Err(needs) => vm.awaits = Some(needs),
                }
            }
        }
        if let Some(why) = acted.failed {
            return Err(why);
        }
        acted
            .resort
            .map_or(Ok(()), |carried| self.last.settle(carried))
    }

    /// Takes the writes into VMs' records not yet made, oldest first (C5).
    pub(crate) fn unrecorded(&mut self) -> Writes {
        self.last.unrecorded()
    }

    /// Keeps `left`, writes that failed, for the next window (C5).
    pub(crate) fn keep_unrecorded(&mut self, left: Writes) {
        self.last.keep_unrecorded(left);
    }

    /// Carries the last-resort ends its node's start read back from node VM rows into its ends (joint plan step 39).
    pub(crate) fn carry_ended(&mut self, ends: Vec<LearnedEnd>) -> Result<(), Refusal> {
        self.last.carry(ends)
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
        let mut learned = learned(
            &self.node,
            class,
            costs,
            vms,
            self.last.ended(class)?,
            self.told_window()?,
        )?;
        learned.rates = self.rates(class, running)?;
        learned.unread = self.unread_of(class, running)?;
        learned.awaits = self.awaits_of(class, running)?;
        learned.ranked = ranked(class, running)?;
        Ok(Some(learned))
    }

    /// The VMs of `class` among `running` its last window couldn't read, in their records' order, each with its
    /// refusal's words (place 4; ROOT, 2026-11-13): each counted whole at its receipt, told where its holding would be.
    fn unread_of(
        &self,
        class: Class,
        running: &[Holder<'_>],
    ) -> Result<Vec<hex_api::LearnedUnread>, Refusal> {
        let mut told = Vec::new();
        for record in running {
            if vm_class(record.class()) != Some(class) {
                continue;
            }
            let Some((id, why)) = self.unread.iter().find(|(id, _)| record.is(id)) else {
                continue;
            };
            told.try_reserve(1).map_err(|_| Refusal::Exhausted {
                what: "learned unread VMs",
                bound: MAX_LEARNED_HOLDINGS,
            })?;
            told.push(hex_api::LearnedUnread {
                id: id.try_clone()?,
                why: why.to_string(),
            });
        }
        Ok(told)
    }

    /// The VMs of `class` among `running` whose guests' rooms can't yet be told, in their records' order, each with
    /// what that awaits as its runtime answered it (C7; place 4).
    fn awaits_of(
        &self,
        class: Class,
        running: &[Holder<'_>],
    ) -> Result<Vec<hex_api::LearnedAwait>, Refusal> {
        let mut told = Vec::new();
        for record in running {
            let Some(vm) = record.scope().and_then(|scope| self.vms.get(&scope)) else {
                continue;
            };
            let Some(needs) = vm.awaits.filter(|_| vm.class == class) else {
                continue;
            };
            told.try_reserve(1).map_err(|_| Refusal::Exhausted {
                what: "learned awaits",
                bound: MAX_LEARNED_HOLDINGS,
            })?;
            told.push(hex_api::LearnedAwait {
                id: vm.id.try_clone()?,
                needs,
            });
        }
        Ok(told)
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
    /// and Shards image VMs alike, with what it couldn't read, what rooms await and what it ranks above every box
    /// (place 4); or, where its last window failed, why (A-3).
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
        // Where its last window failed, why (A-3; ROOT, 2026-11-13): never nothing learned with nothing said.
        capacity.unlearned = self.fault.as_ref().map(ToString::to_string);
        hex_core::wire::fit_learned(capacity).map_err(|e| Refusal::Invalid {
            what: "a capacity answer",
            why: e.to_string(),
        })
    }
}

/// Asks the governor of each managed VM among `records` its learner hasn't asked, on a platform with no window (D255;
/// ROOT, 2026-11-13), at the VM's ceiling, its allocation's CPU, which holds nothing below its vCPUs: a cold start that
/// can't starve it. One whose governor answers a period is learned over it from then on ([`Learner::learns`]); one
/// whose process takes no rate stays booked whole, told; one whose ask or period read fails stays booked whole, told,
/// and is asked again next window. Those no longer running are forgotten. Its learner is let go while it asks (A-2).
pub(crate) fn govern(
    mut learner: impl Lease<Learner>,
    io: &impl Managed,
    records: &[Holder<'_>],
) -> Result<(), Refusal> {
    let asks = learner.take()?.governing(records)?;
    let mut answers = Vec::new();
    answers
        .try_reserve_exact(asks.len())
        .map_err(|_| Refusal::Exhausted {
            what: "a window's governors asked",
            bound: asks.len(),
        })?;
    for ask in asks {
        let first = Cap {
            share: ask.share,
            own: Own::Unheld("its learner asks its governor before it reads its use"),
        };
        let asked = io.set_rate(ask.holder, first).and_then(|held| match held {
            hex_api::RateHeld::Platform(_) => Ok((held, false)),
            held => io
                .rate_period(ask.holder)
                .map(|period| (held, period.is_some())),
        });
        answers.push((ask, asked));
    }
    learner.take()?.governed(answers);
    Ok(())
}

/// One window (C1 to C5, C7) read at `now`: the VMs its learner learns `running` now (their records, [`sort`]), within
/// the fleet's `budget`. Its learner is taken for each of the window's pure steps and let go while its VMs are asked and
/// acted on (A-2; the review's, 2026-11-14), so a capacity answer, which takes it, never waits on a VM. Its node runs one
/// window at a time, on its learner's thread, so nothing but the window moves its learner between its steps. A failure is
/// kept until a window passes.
pub(crate) fn window(
    mut learner: impl Lease<Learner>,
    io: &impl Managed,
    running: &[Holder<'_>],
    budget: CapacityResources,
    now: Instant,
) -> Result<(), Refusal> {
    let asks = learner.take()?.asks(running);
    let learned = match asks.and_then(|asks| ask(io, asks)) {
        Ok(answers) => learner.take()?.learn(running, answers, budget, now),
        Err(why) => Err(why),
    };
    let acted = learned.and_then(|acts| act(io, running, acts));
    learner.take()?.acted(acted)
}

/// Asks each VM of `asks` its reading (outside its learner's lock; A-2): the period its cap holds over and its use, an
/// image VM's from its run host (D241), a managed VM's guest time first and then its scope's use (D260); and the least
/// share its actuator holds it at.
fn ask<'h>(io: &impl Managed, asks: Vec<Ask<'h>>) -> Result<Vec<Answer<'h>>, Refusal> {
    let mut answers = Vec::new();
    answers
        .try_reserve_exact(asks.len())
        .map_err(|_| Refusal::Exhausted {
            what: "a window's VMs",
            bound: asks.len(),
        })?;
    for ask in asks {
        answers.push(Answer {
            holder: ask.holder,
            read: io
                .rate_period(ask.holder)
                .and_then(|period| Read::of(io, ask.holder, ask.first).map(|read| (period, read))),
            least: io.least(ask.holder),
        });
    }
    Ok(answers)
}

/// Acts on what a window's learner decided (C4, C5, C7; outside its lock, A-2), in order, to the first act that fails:
/// each VM capped at its share, its platform cap's cycle at that share read where that cap holds it (C1; ROOT,
/// 2026-11-13); each guest told its room, or kept awaiting what telling it needs (place 4); then its last resort's
/// decision carried out, its VMs `running`.
fn act(io: &impl Managed, running: &[Holder<'_>], acts: Acts<'_>) -> Result<Acted, Refusal> {
    let mut acted = Acted {
        caps: Vec::new(),
        rooms: Vec::new(),
        resort: None,
        failed: None,
    };
    acted
        .caps
        .try_reserve_exact(acts.caps.len())
        .map_err(|_| Refusal::Exhausted {
            what: "a window's caps",
            bound: acts.caps.len(),
        })?;
    acted
        .rooms
        .try_reserve_exact(acts.rooms.len())
        .map_err(|_| Refusal::Exhausted {
            what: "a window's rooms",
            bound: acts.rooms.len(),
        })?;
    for cap in acts.caps {
        let own = match &cap.own {
            Ok(own) => Own::Read(*own),
            Err(why) => Own::Unheld(why),
        };
        let capped = io
            .set_rate(
                cap.holder,
                Cap {
                    share: cap.share,
                    own,
                },
            )
            .and_then(|held| {
                // Where its platform's cap holds it, its window takes that cap's cycle at its share (C1).
                let cycle = match held {
                    hex_api::RateHeld::Platform(_) => io.cap_cycle(cap.share)?,
                    _ => None,
                };
                Ok((held, cycle))
            });
        match capped {
            Ok((held, cycle)) => acted.caps.push(Capped {
                scope: cap.scope,
                share: cap.share,
                told: cap.own.ok(),
                held,
                cycle,
            }),
            Err(why) => {
                acted.failed = Some(why);
                return Ok(acted);
            }
        }
    }
    for (holder, scope, room) in acts.rooms {
        match io.set_room(holder, room) {
            Ok(()) => acted.rooms.push((scope, Ok(room))),
            Err(Refusal::Awaits { needs }) => acted.rooms.push((scope, Err(needs))),
            Err(why) => {
                acted.failed = Some(why);
                return Ok(acted);
            }
        }
    }
    acted.resort = Some(acts.resort.carry(&Stops { io, running }));
    Ok(acted)
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
    window: Option<LearnedWindow>,
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
        window,
        // What it couldn't read, what each guest's room awaits and what it ranks above every box, each learner tells; and
        // the boxes whose records can't be read, which macOS's and Windows' learner tells (Linux's reads its boxes from
        // their cgroups).
        unread: Vec::new(),
        awaits: Vec::new(),
        ranked: Vec::new(),
        unrecorded: Vec::new(),
    })
}

#[cfg(test)]
mod tests;
