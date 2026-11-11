//! A node over any OS's runtime (native plan step 8): each box's record kept durably in the node's state
//! directory beside the runtime that makes the box ([`crate::store`], the records Linux's node keeps), and the
//! programs the socket's disconnect rule ends counted into it ([`crate::abandoned`]), so a node served on
//! any OS's socket ([`crate::serve`]) answers as Linux's does. Linux's node ([`crate::Node`]) keeps the same
//! records and counts through the same code, with its cgroups and containers beside them; macOS's node is
//! this over its runtime (the `process` class, and Linux images in its Shards guest).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use hex_api::{
    Ask, BoxId, BoxInfo, BoxSpec, Class, DeviceId, Proc, Program, Refusal, Runtime, Signal, Stdio,
    Usage,
};
use hex_core::netns;
use hex_core::record::{BoxRecord, Phase, ProcId};
use hex_core::recovery::{self, Decision, Parts, Seen, Why};

use crate::abandoned::Abandoned;
use crate::serve::{self, Forward, Forwarded, Host, Loop, Reply};
use crate::store::Store;
use crate::turns::{Hold, Turn, Turns};
use hex_core::wire::{Answer, Call};

/// What a recovery did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoverySummary {
    /// Boxes taken back as they were.
    pub adopted: Vec<BoxId>,
    /// Boxes removed, each with why.
    pub removed: Vec<(BoxId, String)>,
    /// Slots removed that no box's record named: idle when the agent stopped, or a create's caught
    /// before its record (goal 5 extended, step 6). Linux's node's alone: no other OS's hands a box's
    /// cgroup on.
    pub slots_removed: Vec<u64>,
}

/// Why a box was removed, for its summary.
pub(crate) fn why_text(why: &Why) -> String {
    match why {
        Why::MidCreate => "the node agent stopped while it was being made".to_owned(),
        Why::MidDelete => "the node agent stopped while it was being removed".to_owned(),
        Why::InitGone => "its container's init had ended".to_owned(),
        Why::RunEnded => "the program it was run for had ended".to_owned(),
        Why::NoCgroup => "its record had no cgroup".to_owned(),
        Why::NoRecord(parts) => {
            let left: Vec<&str> = [
                (parts.cgroup, "its cgroup"),
                (parts.state_dir, "its container's state"),
                (parts.bundle, "its bundle"),
            ]
            .into_iter()
            .filter(|(found, _)| *found)
            .map(|(_, what)| what)
            .collect();
            format!(
                "no record named it, made only in part: {} left",
                left.join(", ")
            )
        }
        Why::Unreadable(w) => format!("its record couldn't be read: {w}"),
        Why::StartInterrupted => {
            "the node agent stopped while it started the box, in its startContainer hooks"
                .to_owned()
        }
    }
}

/// What a node needs of its runtime to take its boxes back after a crash (T-T7.1.2).
pub trait Recover: Runtime {
    /// Launches within this same native owner's envelope. The process remains suspended until
    /// `publish` durably introduces its immutable identity in the canonical Creating record.
    fn launch_managed(
        &self,
        record: &BoxRecord,
        plan: &hex_runtime::devices::ManagedLaunch<'_>,
        io: shards_api::LaunchIo,
        patience: std::time::Duration,
        publish: &mut dyn FnMut(&shards_api::Identity) -> Result<(), Refusal>,
    ) -> Result<shards_api::ManagedReceipt, Refusal> {
        let _ = (plan, io, patience, publish);
        Err(uncertain(&record.spec.id))
    }

    fn attach_managed(
        &self,
        record: &BoxRecord,
        identity: Option<&shards_api::Identity>,
    ) -> Result<shards_api::ManagedReceipt, Refusal> {
        let _ = identity;
        Err(uncertain(&record.spec.id))
    }

    /// Called only after the selected VM receipt durably records positive whole-scope release.
    fn finalize_managed(
        &self,
        record: &BoxRecord,
        identity: Option<&shards_api::Identity>,
    ) -> Result<(), Refusal> {
        let _ = identity;
        Err(uncertain(&record.spec.id))
    }

    fn managed_usage(&self, record: &BoxRecord) -> Result<Usage, Refusal> {
        Err(uncertain(&record.spec.id))
    }

    /// Initialized native routes only; physical claims are overlaid from this owner's Store.
    fn attachment_offers(&self) -> Result<Vec<hex_api::DeviceInfo>, Refusal> {
        Ok(self
            .capacity(Class::Oci)?
            .map_or_else(Vec::new, |capacity| capacity.devices))
    }
    /// An initialized backend that enters this same Store/Turns authority itself. The outer
    /// Recorded wrapper must not write another record or acquire a reentrant method turn.
    fn owns_records(&self, spec: &BoxSpec) -> Result<bool, Refusal> {
        let _ = spec;
        Ok(false)
    }
    /// The persisted class chooses the backend before any cleanup or native operation.
    fn runtime_for(&self, spec: &BoxSpec) -> Result<&dyn Runtime, Refusal>
    where
        Self: Sized,
    {
        let _ = spec;
        Ok(self)
    }
    /// What names box `id`'s processes after the node is gone, recorded with the box once it has any (its
    /// first member started): a process and its start time, which tells it from one given its pid since.
    fn trace(&self, id: &BoxId) -> Result<Option<ProcId>, Refusal>;
    /// Takes back box `spec`, its processes the ones `trace` names where they still run.
    fn adopt(&self, spec: &BoxSpec, trace: Option<ProcId>) -> Result<(), Refusal>;
    /// Ends what still runs of what `trace` names, for a box recovery removes.
    fn end(&self, trace: Option<ProcId>) -> Result<(), Refusal>;
    fn end_for(&self, spec: &BoxSpec, trace: Option<ProcId>) -> Result<(), Refusal> {
        let _ = spec;
        self.end(trace)
    }
    /// For an `oci` box: whether its init still runs, so recovery takes it back; `None` for a class with
    /// no init. By default an `oci` box's init went with the node.
    fn alive(&self, spec: &BoxSpec) -> Result<Option<bool>, Refusal> {
        Ok(match spec.class.class() {
            Class::Oci | Class::Firecracker | Class::Shards => Some(false),
            Class::Process => None,
        })
    }
    /// Removes what is left of box `spec` that `trace` doesn't name, for a box recovery removes. By
    /// default there is nothing.
    fn discard(&self, spec: &BoxSpec) -> Result<(), Refusal> {
        let _ = spec;
        Ok(())
    }
    /// The core's loop, for connections of its own ([`Host::attach`]). By default it has none.
    fn attach(&self, lp: Arc<dyn Loop>) -> Result<(), Refusal> {
        let _ = lp;
        Ok(())
    }
    /// A call it answers later ([`Host::forward`]). By default every call runs on the worker.
    fn forward(&self, call: Call, stdio: Stdio, reply: Reply, ended: serve::Ended) -> Forward {
        let _ = (reply, ended);
        Forward::Back(Box::new((call, stdio)))
    }
    fn forward_for(
        &self,
        spec: &BoxSpec,
        call: Call,
        stdio: Stdio,
        reply: Reply,
        ended: serve::Ended,
    ) -> Forward {
        let _ = spec;
        self.forward(call, stdio, reply, ended)
    }
}

/// A runtime whose boxes' records the node keeps.
pub struct Recorded<R> {
    runtime: R,
    books: Arc<Books>,
}

/// The node's records of its boxes, shared with the completions of calls its guest answers.
struct Books {
    store: Store,
    /// The CLI/Service's same native provider allocation; never opened by a guest backend.
    prober: Option<crate::facts::SharedProber>,
    /// Active create/publication or delete/removal methods, not a second reservation authority.
    lifecycle: Turns,
    abandoned: Abandoned,
    /// Boxes a recovery took back, which inspect reports as adopted.
    adopted: Mutex<BTreeSet<BoxId>>,
}

/// Concrete shared transaction; its caller keeps the same method/device Turn on stack.
struct Transaction<'a> {
    store: &'a Store,
    id: &'a BoxId,
    devices: &'a [DeviceId],
    native: &'a Native<'a>,
}

type ManagedReceipt = shards_api::ManagedReceipt;
type NativeLaunch<'a> = dyn Fn(
        &BoxRecord,
        &hex_runtime::devices::ManagedLaunch<'_>,
        shards_api::LaunchIo,
        std::time::Duration,
        &mut dyn FnMut(&shards_api::Identity) -> Result<(), Refusal>,
    ) -> Result<ManagedReceipt, Refusal>
    + 'a;
type NativeAttach<'a> =
    dyn Fn(&BoxRecord, Option<&shards_api::Identity>) -> Result<ManagedReceipt, Refusal> + 'a;
type NativeFinalize<'a> =
    dyn Fn(&BoxRecord, Option<&shards_api::Identity>) -> Result<(), Refusal> + 'a;
type NativeUsage<'a> = dyn Fn(&BoxRecord) -> Result<Usage, Refusal> + 'a;

/// Borrowed callbacks into Node's or Recorded's existing native owner. They remain on the same
/// synchronous transaction stack, with no map lock, boxed transaction or second holder owner.
pub(crate) struct Native<'a> {
    pub launch: &'a NativeLaunch<'a>,
    pub attach: &'a NativeAttach<'a>,
    pub finalize: &'a NativeFinalize<'a>,
    pub usage: &'a NativeUsage<'a>,
}

fn uncertain(id: &BoxId) -> Refusal {
    match id.try_clone() {
        Ok(id) => Refusal::Lifecycle {
            id,
            cause: hex_api::LifecycleCause::Uncertain,
        },
        Err(error) => error,
    }
}

fn broken(message: &'static str) -> Refusal {
    let mut why = String::new();
    if why.try_reserve_exact(message.len()).is_err() {
        return Refusal::Exhausted {
            what: "recorded refusal bytes",
            bound: message.len(),
        };
    }
    why.push_str(message);
    Refusal::Broken(why)
}

impl Transaction<'_> {
    fn owns(&self, record: &BoxRecord) -> Result<(), Refusal> {
        if &record.spec.id != self.id
            || record
                .spec
                .devices
                .iter()
                .any(|id| !self.devices.contains(id))
        {
            return Err(uncertain(self.id));
        }
        Ok(())
    }
}

impl hex_runtime::devices::Transaction for Transaction<'_> {
    fn managed_usage(&self) -> Result<Usage, Refusal> {
        let record = self
            .store
            .read_lifecycle(self.id)?
            .ok_or_else(|| uncertain(self.id))?;
        self.owns(&record)?;
        record.spec.check()?;
        if record.native_scope.is_none()
            || record.spec.allocation.is_none()
            || !matches!(
                record.spec.class.class(),
                Class::Shards | Class::Oci | Class::Firecracker
            )
        {
            return Err(uncertain(self.id));
        }
        (self.native.usage)(&record)
    }

    fn launch_managed(
        &self,
        plan: &hex_runtime::devices::ManagedLaunch<'_>,
        io: shards_api::LaunchIo,
        patience: std::time::Duration,
    ) -> Result<ManagedReceipt, Refusal> {
        let deadline = std::time::Instant::now()
            .checked_add(patience)
            .filter(|_| patience != std::time::Duration::MAX && !patience.is_zero())
            .ok_or_else(|| uncertain(self.id))?;
        let record = self
            .store
            .read_lifecycle(self.id)?
            .ok_or_else(|| uncertain(self.id))?;
        self.owns(&record)?;
        record.spec.check()?;
        let backend_matches = match plan {
            hex_runtime::devices::ManagedLaunch::Shards(_) => {
                matches!(record.spec.class.class(), Class::Shards | Class::Oci)
            }
            hex_runtime::devices::ManagedLaunch::Firecracker { .. } => {
                record.spec.class.class() == Class::Firecracker
            }
        };
        if !backend_matches
            || record.spec.allocation.is_none()
            || record.native_scope.is_none()
            || record.phase != Phase::Creating
            || record.init.is_some()
        {
            return Err(uncertain(self.id));
        }
        // The native owner invokes this while the atomically bound process remains suspended.
        // An error leaves Creating and its claims intact; the native owner retains the receipt.
        let mut introduced = None;
        let spec = &record.spec;
        let mut publish = |identity: &shards_api::Identity| {
            if identity.pid == 0 || identity.started == 0 || identity.vsock != plan.vsock() {
                return Err(uncertain(self.id));
            }
            let init = ProcId {
                pid: identity.pid,
                start_ticks: identity.started,
            };
            if introduced.is_some() {
                return Err(uncertain(self.id));
            }
            let mut current = self
                .store
                .read_lifecycle(self.id)?
                .ok_or_else(|| uncertain(self.id))?;
            if current.spec != *spec
                || current.phase != Phase::Creating
                || current.native_scope != record.native_scope
            {
                return Err(uncertain(self.id));
            }
            current.init = Some(init);
            self.store.put_lifecycle(&current)?;
            introduced = Some(init);
            Ok(())
        };
        let patience = deadline.saturating_duration_since(std::time::Instant::now());
        if patience.is_zero() {
            return Err(uncertain(self.id));
        }
        let receipt = (self.native.launch)(&record, plan, io, patience, &mut publish)?;
        if introduced.is_none() {
            return Err(uncertain(self.id));
        }
        Ok(receipt)
    }

    fn attach_managed(
        &self,
        identity: Option<&shards_api::Identity>,
    ) -> Result<ManagedReceipt, Refusal> {
        let record = self
            .store
            .read_lifecycle(self.id)?
            .ok_or_else(|| uncertain(self.id))?;
        self.owns(&record)?;
        record.spec.check()?;
        if !matches!(
            record.spec.class.class(),
            Class::Shards | Class::Oci | Class::Firecracker
        ) || record.spec.allocation.is_none()
            || record.native_scope.is_none()
        {
            return Err(uncertain(self.id));
        }
        match identity {
            Some(identity) => {
                let expected = record.init.ok_or_else(|| uncertain(self.id))?;
                if expected.pid != identity.pid
                    || expected.start_ticks != identity.started
                    || identity.pid == 0
                    || identity.started == 0
                {
                    return Err(uncertain(self.id));
                }
            }
            None if !matches!(record.phase, Phase::Creating | Phase::Deleting) => {
                return Err(uncertain(self.id));
            }
            None => {}
        }
        (self.native.attach)(&record, identity)
    }

    fn finalize_managed(&self, identity: Option<&shards_api::Identity>) -> Result<(), Refusal> {
        let record = self
            .store
            .read_lifecycle(self.id)?
            .ok_or_else(|| uncertain(self.id))?;
        self.owns(&record)?;
        record.spec.check()?;
        if !matches!(
            record.spec.class.class(),
            Class::Shards | Class::Oci | Class::Firecracker
        ) || record.spec.allocation.is_none()
            || record.native_scope.is_none()
            || record.phase != Phase::Deleting
        {
            return Err(uncertain(self.id));
        }
        match (record.init, identity) {
            (Some(expected), Some(actual))
                if expected.pid == actual.pid && expected.start_ticks == actual.started => {}
            // A controller may have been created but its canonical publication failed. Cleanup
            // may retain its actual identity; the native owner still checks its same receipt.
            (None, Some(actual)) if actual.pid != 0 && actual.started != 0 => {}
            (None, None) => {}
            _ => return Err(uncertain(self.id)),
        }
        (self.native.finalize)(&record, identity)
    }

    fn records(&self) -> Result<hex_runtime::devices::Records, Refusal> {
        self.store.all()
    }
    fn read(&self, id: &BoxId) -> Result<Option<BoxRecord>, Refusal> {
        if id != self.id {
            return Err(uncertain(self.id));
        }
        self.store.read_lifecycle(id)
    }
    fn put(&self, record: &BoxRecord) -> Result<(), Refusal> {
        self.owns(record)?;
        if self.store.read_lifecycle(self.id)?.is_none() {
            check_claims(&self.store.all()?, self.id, &record.spec.devices)?;
        }
        self.store.put_lifecycle(record)
    }
    fn remove(&self, id: &BoxId) -> Result<(), Refusal> {
        if id != self.id {
            return Err(uncertain(self.id));
        }
        if let Some(record) = self.store.read_lifecycle(id)? {
            self.owns(&record)?;
        }
        self.store.remove_lifecycle(id)
    }
}

/// Both native Node and Recorded enter their existing method/attachment turns before this call.
/// No Store turn spans native I/O; individual record writes take only their short Store turn.
pub(crate) fn transaction(
    store: &Store,
    id: &BoxId,
    devices: &[DeviceId],
    native: &Native<'_>,
    callback: &mut dyn FnMut(&dyn hex_runtime::devices::Transaction) -> Result<(), Refusal>,
) -> Result<(), Refusal> {
    callback(&Transaction {
        store,
        id,
        devices,
        native,
    })
}

fn check_claims(
    records: &hex_runtime::devices::Records,
    own: &BoxId,
    devices: &[DeviceId],
) -> Result<(), Refusal> {
    check_records(records)?;
    for (id, record) in records {
        let record = record.as_ref().map_err(|_| uncertain(id))?;
        if id != own && record.spec.devices.iter().any(|id| devices.contains(id)) {
            return Err(Refusal::Lifecycle {
                id: own.try_clone()?,
                cause: hex_api::LifecycleCause::Busy,
            });
        }
    }
    Ok(())
}

fn check_records(records: &hex_runtime::devices::Records) -> Result<(), Refusal> {
    for (id, record) in records {
        let record = record.as_ref().map_err(|_| uncertain(id))?;
        if &record.spec.id != id || record.spec.check().is_err() {
            return Err(uncertain(id));
        }
    }
    Ok(())
}

fn recorded_holders(
    report: &mut hex_api::Capacity,
    records: &hex_runtime::devices::Records,
) -> Result<(), Refusal> {
    // The native report establishes current attachment capability. The same Store supplies
    // cross-backend claims, including Creating and Deleting; no reverse holder cache is kept.
    check_records(records)?;
    for offer in &mut report.devices {
        let mut found = None;
        for (id, record) in records {
            let record = record.as_ref().map_err(|_| uncertain(id))?;
            if record.spec.devices.contains(&offer.id) {
                if found.is_some() {
                    return Err(uncertain(id));
                }
                found = Some(id);
            }
        }
        if let Some(holder) = found {
            if let Some(native) = &offer.holder
                && native != holder
            {
                return Err(uncertain(holder));
            }
            offer.holder = Some(holder.try_clone()?);
        }
    }
    report.check()?;
    Ok(())
}

fn native_metadata(
    report: &mut hex_api::Capacity,
    mut physical: Vec<hex_api::DeviceInfo>,
) -> Result<(), Refusal> {
    report.check()?;
    for native in &physical {
        native.check()?;
    }
    for offer in &mut report.devices {
        if physical
            .iter()
            .filter(|native| native.id == offer.id)
            .count()
            != 1
        {
            return Err(broken(
                "an initialized route has no unambiguous native physical identity",
            ));
        }
        let native = physical
            .iter_mut()
            .find(|native| native.id == offer.id)
            .ok_or_else(|| broken("native physical identity disappeared within its snapshot"))?;
        if native.holder.is_some() {
            return Err(broken(
                "native discovery cannot declare reservation holders",
            ));
        }
        // Move actual metadata out of this fresh snapshot; no string clone or retained cache.
        offer.model = std::mem::take(&mut native.model);
        offer.size = native.size;
    }
    report.check()
}

impl<R: Recover> hex_runtime::devices::Books for Recorded<R> {
    fn records(&self) -> Result<hex_runtime::devices::Records, Refusal> {
        self.books.store.all()
    }
    fn physical_devices(&self) -> Result<Vec<hex_api::DeviceInfo>, Refusal> {
        let prober = self
            .books
            .prober
            .as_ref()
            .ok_or_else(|| broken("the recorded owner has no initialized native provider"))?;
        prober
            .lock()
            .map_err(|_| broken("the shared native provider lock was poisoned"))?
            .physical_devices()
    }
    fn capacity(&self) -> Result<hex_api::Capacity, Refusal> {
        // Calling WithGuest::capacity here would reenter GuestRuntime -> Books. Probe the same
        // owner's host volume directly; device routes add their initialized catalog separately.
        let mut capacity =
            crate::facts::capacity(self.books.store.directory(), self.books.authority_id()?)?;
        capacity.devices = self.runtime.attachment_offers()?;
        if !capacity.devices.is_empty() {
            native_metadata(
                &mut capacity,
                hex_runtime::devices::Books::physical_devices(self)?,
            )?;
        }
        recorded_holders(&mut capacity, &self.books.store.all()?)?;
        Ok(capacity)
    }
    fn with_transaction(
        &self,
        id: &BoxId,
        devices: &[DeviceId],
        callback: &mut dyn FnMut(&dyn hex_runtime::devices::Transaction) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        let _method = self.books.lifecycle.try_take_devices(id, devices)?;
        let launch =
            |record: &BoxRecord,
             plan: &hex_runtime::devices::ManagedLaunch<'_>,
             io: shards_api::LaunchIo,
             patience: std::time::Duration,
             publish: &mut dyn FnMut(&shards_api::Identity) -> Result<(), Refusal>| {
                self.runtime
                    .launch_managed(record, plan, io, patience, publish)
            };
        let attach = |record: &BoxRecord, identity: Option<&shards_api::Identity>| {
            self.runtime.attach_managed(record, identity)
        };
        let finalize = |record: &BoxRecord, identity: Option<&shards_api::Identity>| {
            self.runtime.finalize_managed(record, identity)
        };
        let usage = |record: &BoxRecord| self.runtime.managed_usage(record);
        transaction(
            &self.books.store,
            id,
            devices,
            &Native {
                launch: &launch,
                attach: &attach,
                finalize: &finalize,
                usage: &usage,
            },
            callback,
        )
    }
}

struct ForwardTurn {
    books: Arc<Books>,
    hold: Option<Hold>,
    expected: Option<(BoxId, Class, Option<hex_api::Allocation>)>,
}

impl Drop for ForwardTurn {
    fn drop(&mut self) {
        if let Some(hold) = self.hold.take() {
            self.books.lifecycle.release(hold);
        }
    }
}

impl<R: Recover> Recorded<R> {
    /// Native methods use this same owner. A backend registered with Books enters it itself;
    /// route changes between the initial read and acquisition are refused before native I/O.
    fn method_turn(&self, id: &BoxId) -> Result<Option<Turn<'_>>, Refusal> {
        let before = self.books.store.read_lifecycle(id)?;
        if let Some(record) = &before
            && self.runtime.owns_records(&record.spec)?
        {
            return Ok(None);
        }
        let devices = before
            .as_ref()
            .map_or(&[][..], |r| r.spec.devices.as_slice());
        let turn = self.books.lifecycle.try_take_devices(id, devices)?;
        let after = self.books.store.read_lifecycle(id)?;
        if before.as_ref().map(|r| &r.spec) != after.as_ref().map(|r| &r.spec) {
            return Err(uncertain(id));
        }
        Ok(Some(turn))
    }

    fn runtime_at(&self, id: &BoxId) -> Result<&dyn Runtime, Refusal> {
        match self.books.store.read_lifecycle(id)? {
            Some(record) => self.runtime.runtime_for(&record.spec),
            None => Ok(&self.runtime),
        }
    }
    /// `runtime`'s boxes, their records in `state_dir`. Does not recover; call [`Recorded::recover`]. The records
    /// that needn't outlive the host ([`crate::store`]) are kept beside the rest, in `@records` under it (`@` is in
    /// no box id, so it is never read as a box's record): these OSes give the node no tmpfs runtime directory.
    pub fn open(runtime: R, state_dir: &Path) -> Result<Self, Refusal> {
        Self::open_using(runtime, state_dir, None)
    }

    /// The native node's single Prober/Gpus. Construct it once in the native CLI and pass this
    /// Arc before guest registration/recovery. A separate scheduler reads its flat RPC report.
    pub fn open_with_prober(
        runtime: R,
        state_dir: &Path,
        prober: crate::facts::SharedProber,
    ) -> Result<Self, Refusal> {
        Self::open_using(runtime, state_dir, Some(prober))
    }

    fn open_using(
        runtime: R,
        state_dir: &Path,
        prober: Option<crate::facts::SharedProber>,
    ) -> Result<Self, Refusal> {
        Ok(Self {
            runtime,
            books: Arc::new(Books {
                store: Store::open(state_dir, &state_dir.join("@records"))?,
                prober,
                lifecycle: Turns::new(
                    "changing a recorded box's lifecycle",
                    "recorded lifecycle methods",
                    crate::store::TURNS,
                    // try_take never waits. Zero expresses immediate overlap refusal, not a
                    // timeout guess; the existing Turns owner records the method's actual hold.
                    hex_core::patience::Patience::seeded(
                        "recorded.lifecycle",
                        std::time::Duration::ZERO,
                    ),
                ),
                abandoned: Abandoned::default(),
                adopted: Mutex::new(BTreeSet::new()),
            }),
        })
    }

    /// After a restart: takes each box back or cleans it up as its record says (T-T7.1.2), by
    /// `hex_core::recovery`'s decisions, Linux's node's. A box is a record and the runtime's processes it
    /// names: a record made is the box, whatever runs in it now, so it is taken back; one being made or
    /// removed when the node stopped is removed, with what its record names ended. Safe to call again after
    /// a crash during it: ending a box tolerates its processes being gone already.
    pub fn recover(&self) -> Result<RecoverySummary, Refusal> {
        // What a crash at a record write left: a temp file never renamed into place.
        self.books.store.remove_strays()?;
        let mut summary = RecoverySummary::default();
        let records = self.books.store.all()?;
        let mut seen: BTreeMap<BoxId, Seen> = BTreeMap::new();
        for (id, record) in &records {
            // Without a readable class/identity this owner cannot choose a cleanup backend.
            // Retain the row and its claims rather than run native host cleanup by guesswork.
            record.as_ref().map_err(|_| uncertain(id))?;
            let alive = match record {
                // Incomplete creates and durable deletes are removed, never adopted. Asking alive
                // first would require a pending native create to look Ready before cleanup can run.
                Ok(r) if r.phase == Phase::Ready => self.runtime.alive(&r.spec)?,
                Ok(_) => None,
                Err(_) => None,
            };
            seen.insert(
                id.clone(),
                Seen {
                    record: Some(record.clone()),
                    // A record is the box here, whatever runs in it now: its runtime's part stands for the
                    // cgroup Linux's node finds.
                    parts: Parts {
                        cgroup: true,
                        ..Parts::default()
                    },
                    init_alive: alive,
                    start_interrupted: false,
                },
            );
        }
        for (id, decision) in recovery::plan(&seen) {
            let record = records.get(&id).and_then(|r| r.as_ref().ok());
            let trace = record.and_then(|r| r.init);
            let managed = record
                .map(|r| self.runtime.owns_records(&r.spec))
                .transpose()?
                .unwrap_or(false);
            let _method = if managed {
                None
            } else {
                self.method_turn(&id)?
            };
            match decision {
                Decision::Adopt => {
                    if let Some(r) = record {
                        self.runtime.adopt(&r.spec, trace)?;
                    }
                    self.books
                        .adopted
                        .lock()
                        .map_err(|_| poisoned())?
                        .insert(id.clone());
                    summary.adopted.push(id);
                }
                Decision::Remove(why) => {
                    if managed {
                        let r = record.ok_or_else(|| uncertain(&id))?;
                        self.runtime.runtime_for(&r.spec)?.delete(&id)?;
                    } else if let Some(r) = record {
                        self.runtime.end_for(&r.spec, trace)?;
                        self.runtime.discard(&r.spec)?;
                        self.books.store.remove(&id)?;
                    } else {
                        return Err(uncertain(&id));
                    }
                    summary.removed.push((id, why_text(&why)));
                }
            }
        }
        Ok(summary)
    }

    fn counted(&self, info: BoxInfo) -> Result<BoxInfo, Refusal> {
        self.books.counted(info)
    }

    fn forward_runtime(
        &self,
        call: Call,
        stdio: Stdio,
        reply: Reply,
        ended: serve::Ended,
    ) -> Forward {
        let id = match &call {
            Call::Inspect(id) | Call::Start(id) | Call::Usage(id) => id,
            Call::Signal { id, .. } | Call::Exec { id, .. } | Call::ExecStreams { id, .. } => id,
            _ => return self.runtime.forward(call, stdio, reply, ended),
        };
        match self.books.store.read_lifecycle(id) {
            Ok(Some(record)) => self
                .runtime
                .forward_for(&record.spec, call, stdio, reply, ended),
            Ok(None) => self.runtime.forward(call, stdio, reply, ended),
            Err(error) => Forward::Refused(error),
        }
    }
}

impl<P: Recover + 'static> Recorded<hex_runtime::guest::WithGuest<P>> {
    /// After constructing the CLI's one Arc, register a weak view of that same allocation.
    /// The guest holds no strong back-reference and no second Store/holder owner is created.
    pub fn bind_guest_books(self: &Arc<Self>) -> Result<(), Refusal> {
        let authority: Arc<dyn hex_runtime::devices::Books> = self.clone();
        self.runtime.guest().bind_books(Arc::downgrade(&authority))
    }
}

impl Books {
    /// The same Store identity backs both initialized backend capacities and the native report.
    /// It introduces no device inventory or reservation state.
    fn authority_id(&self) -> Result<String, Refusal> {
        let name = self
            .store
            .directory()
            .to_str()
            .ok_or_else(|| broken("recorded authority path is not Unicode"))?;
        let bytes = "recorded:"
            .len()
            .checked_add(name.len())
            .ok_or(Refusal::Exhausted {
                what: "recorded authority identity bytes",
                bound: usize::MAX,
            })?;
        let mut id = String::new();
        id.try_reserve_exact(bytes)
            .map_err(|_| Refusal::Exhausted {
                what: "recorded authority identity bytes",
                bound: bytes,
            })?;
        id.push_str("recorded:");
        id.push_str(name);
        Ok(id)
    }

    fn forward_turn(self: &Arc<Self>, id: &BoxId) -> Result<ForwardTurn, Refusal> {
        let record = self.store.read_lifecycle(id)?;
        let devices = record
            .as_ref()
            .map_or(&[][..], |r| r.spec.devices.as_slice());
        let hold = self.lifecycle.try_hold_devices(id, devices)?;
        let expected = record.map(|r| {
            let class = r.spec.class.class();
            (r.spec.id, class, r.spec.allocation)
        });
        // This clones the existing Books allocation, not a new owner/table. Live receipts remain
        // bounded by the same TURNS admission as synchronous methods.
        Ok(ForwardTurn {
            books: Arc::clone(self),
            hold: Some(hold),
            expected,
        })
    }
    /// `info` with the abandoned programs counted for its box, recorded and not yet, and whether a recovery
    /// took it back.
    fn counted(&self, mut info: BoxInfo) -> Result<BoxInfo, Refusal> {
        let recorded = match self.store.get(&info.id)? {
            Some(Ok(record)) => {
                if info.allocation != record.spec.allocation {
                    return Err(uncertain(&info.id));
                }
                record.abandoned
            }
            Some(Err(_)) => return Err(uncertain(&info.id)),
            None => 0,
        };
        info.abandoned = recorded.saturating_add(self.abandoned.unrecorded(&info.id)?);
        info.adopted |= self
            .adopted
            .lock()
            .map_err(|_| poisoned())?
            .contains(&info.id);
        Ok(info)
    }
}

fn poisoned() -> Refusal {
    Refusal::Broken("the node's adopted boxes' lock was poisoned".to_owned())
}

impl<R: Recover> Runtime for Recorded<R> {
    fn physical_report(&self) -> Result<hex_api::PhysicalReport, Refusal> {
        let prober = self
            .books
            .prober
            .as_ref()
            .ok_or_else(|| broken("the recorded owner has no initialized native provider"))?;
        let authority = self.books.authority_id()?;
        prober
            .lock()
            .map_err(|_| broken("the shared native provider lock was poisoned"))?
            .physical_report(authority)
    }

    fn capacity(&self, class: Class) -> Result<Option<hex_api::Capacity>, Refusal> {
        let Some(mut report) = self.runtime.capacity(class)? else {
            return Ok(None);
        };
        if !report.devices.is_empty() {
            native_metadata(
                &mut report,
                hex_runtime::devices::Books::physical_devices(self)?,
            )?;
        }
        let records = self.books.store.all()?;
        recorded_holders(&mut report, &records)?;
        Ok(Some(report))
    }

    fn classes(&self) -> Vec<Class> {
        self.runtime.classes()
    }

    /// Records the box as being made before anything is made, so a crash leaves a half-made box known as
    /// one (T-T7.1.2), then as made once the runtime has made it; a refused create leaves no record.
    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        spec.check()?;
        if self.runtime.owns_records(&spec)? {
            return self.counted(self.runtime.create(spec)?);
        }
        let _lifecycle = self
            .books
            .lifecycle
            .try_take_devices(&spec.id, &spec.devices)?;
        check_claims(&self.books.store.all()?, &spec.id, &spec.devices)?;
        let devices = !spec.devices.is_empty();
        if devices {
            if self.books.store.get(&spec.id)?.is_some() {
                return Err(Refusal::Exists(spec.id));
            }
            let report =
                Runtime::capacity(self, spec.class.class())?.ok_or_else(|| Refusal::Platform {
                    what: "devices",
                    platform: std::env::consts::OS,
                    why: "runtime reports no native attachment catalog".into(),
                })?;
            report.check()?;
            if spec.devices.iter().any(|id| {
                !report
                    .devices
                    .iter()
                    .any(|offer| offer.id == *id && offer.holder.is_none())
            }) {
                return Err(Refusal::Invalid {
                    what: "devices",
                    why: "requested physical identity is unavailable in the native catalog".into(),
                });
            }
        }
        let record = BoxRecord::new(spec.clone(), Phase::Creating, Vec::new());
        self.books.store.begin(&record)?;
        // A returned error is not proof of physical absence for any backend. Delete/recovery owns
        // cleanup; keep Creating and its exact claims even when the backend could not publish.
        let info = self.runtime.create(spec)?;
        self.books.store.made(&record)?;
        self.counted(info)
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        let _lifecycle = self.method_turn(id)?;
        self.runtime_at(id)?.start(id)
    }

    /// Its runtime's to refuse: none this wraps makes a box for one program's life (macOS's and Windows' refuse a
    /// run, naming their platform, and an `oci` spec's is refused everywhere; decision 7), so there is no run's box
    /// to record.
    fn run(&self, spec: BoxSpec, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        spec.check_no_devices(std::env::consts::OS)?;
        self.runtime.run(spec, program, stdio)
    }

    /// Runs `program`, and once the box has processes, records what names them after a crash.
    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        let _lifecycle = self.method_turn(id)?;
        let proc = self.runtime_at(id)?.exec(id, program, stdio)?;
        if _lifecycle.is_some()
            && let Some(Ok(record)) = self.books.store.get(id)?
            && record.init.is_none()
            && let Some(trace) = self.runtime.trace(id)?
        {
            self.books.store.update(id, |r| {
                r.init = Some(trace);
                true
            })?;
        }
        Ok(proc)
    }

    fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal> {
        let _lifecycle = self.method_turn(id)?;
        self.runtime_at(id)?.signal(id, signal)
    }

    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        self.counted(self.runtime_at(id)?.inspect(id)?)
    }

    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        self.runtime_at(id)?.usage(id)
    }

    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        self.runtime
            .list()?
            .into_iter()
            .map(|info| self.counted(info))
            .collect()
    }

    /// Records the box as being deleted, deletes it, then removes its record: a crash between leaves a
    /// record recovery finishes.
    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        if let Some(record) = self.books.store.read_lifecycle(id)?
            && self.runtime.owns_records(&record.spec)?
        {
            self.runtime.runtime_for(&record.spec)?.delete(id)?;
            self.books.abandoned.forget(id);
            if let Ok(mut adopted) = self.books.adopted.lock() {
                adopted.remove(id);
            }
            return Ok(());
        }
        let _lifecycle = self.method_turn(id)?;
        let mut devices = false;
        if let Some(Ok(mut record)) = self.books.store.get(id)? {
            devices = !record.spec.devices.is_empty();
            record.phase = Phase::Deleting;
            self.books.store.put(&record)?;
        }
        match self.runtime_at(id)?.delete(id) {
            Ok(()) => {}
            Err(Refusal::NoSuchBox(_)) if devices => {}
            Err(why) => return Err(why),
        }
        self.books.store.remove(id)?;
        self.books.abandoned.forget(id);
        if let Ok(mut adopted) = self.books.adopted.lock() {
            adopted.remove(id);
        }
        Ok(())
    }
}

impl<R: Recover> Host for Recorded<R> {
    fn attach(&self, lp: Arc<dyn Loop>) -> Result<(), Refusal> {
        self.runtime.attach(lp)
    }

    /// The calls its runtime answers later go there: an inspect's answer counted as any inspect's is
    /// ([`Books::counted`]). A create or a delete, which write the box's record and stage or remove its
    /// root, runs on the worker.
    fn forward(&self, call: Call, stdio: Stdio, reply: Reply, ended: serve::Ended) -> Forward {
        let id = match &call {
            Call::Inspect(id) | Call::Start(id) | Call::Usage(id) => Some(id),
            Call::Signal { id, .. } | Call::Exec { id, .. } | Call::ExecStreams { id, .. } => {
                Some(id)
            }
            _ => None,
        };
        let reply = if let Some(id) = id {
            let turn = match self.books.forward_turn(id) {
                Ok(turn) => turn,
                Err(why) => return Forward::Refused(why),
            };
            Box::new(move |mut answer| {
                if let Some((id, class, allocation)) = &turn.expected
                    && let Ok(Forwarded::Answer(boxed)) = &mut answer
                    && let Answer::Info(info) = boxed.as_mut()
                {
                    if info.id != *id
                        || (info.class != *class
                            && !(info.class == Class::Oci && *class == Class::Shards))
                        || (info.allocation != *allocation
                            && !(info.class == Class::Oci
                                && *class == Class::Shards
                                && info.allocation.is_none()))
                    {
                        answer = Err(uncertain(id));
                    } else {
                        info.class = *class;
                        info.allocation = *allocation;
                    }
                }
                reply(answer);
                drop(turn);
            }) as Reply
        } else {
            reply
        };
        match call {
            Call::Inspect(_) => {
                let books = Arc::clone(&self.books);
                let counted: Reply = Box::new(move |got| {
                    reply(match got {
                        Ok(Forwarded::Answer(answer)) => match *answer {
                            Answer::Info(info) => books
                                .counted(info)
                                .map(|info| Forwarded::Answer(Box::new(Answer::Info(info)))),
                            answer => Ok(Forwarded::Answer(Box::new(answer))),
                        },
                        other => other,
                    })
                });
                self.forward_runtime(call, stdio, counted, ended)
            }
            Call::Start(_)
            | Call::Signal { .. }
            | Call::Usage(_)
            | Call::Exec { .. }
            | Call::ExecStreams { .. } => self.forward_runtime(call, stdio, reply, ended),
            call => Forward::Back(Box::new((call, stdio))),
        }
    }

    fn abandoned(&self, id: &BoxId) -> Result<(), Refusal> {
        self.books.abandoned.count(&self.books.store, id)
    }

    /// Network namespaces are Linux's: a node elsewhere has none to count.
    fn netns(&self) -> Result<netns::Counts, Refusal> {
        Err(Refusal::Platform {
            what: "the node's network namespace counts",
            platform: std::env::consts::OS,
            why: "network namespaces are Linux's; this node makes none".to_owned(),
        })
    }
}

/// The macOS `process` class: a box's processes are its process group, named by its leader's pid and start
/// time (`proc_pid_rusage`'s Mach absolute time, in [`ProcId::start_ticks`]).
#[cfg(target_os = "macos")]
impl Recover for hex_runtime::macos::MacRuntime {
    fn trace(&self, id: &BoxId) -> Result<Option<ProcId>, Refusal> {
        Ok(self.group(id)?.and_then(|(pgid, started)| {
            Some(ProcId {
                pid: u32::try_from(pgid).ok()?,
                start_ticks: started,
            })
        }))
    }

    fn adopt(&self, spec: &BoxSpec, trace: Option<ProcId>) -> Result<(), Refusal> {
        hex_runtime::macos::MacRuntime::adopt(self, spec, group(trace))
    }

    fn end(&self, trace: Option<ProcId>) -> Result<(), Refusal> {
        hex_runtime::macos::MacRuntime::end(group(trace))
    }
}

#[cfg(target_os = "macos")]
fn group(trace: Option<ProcId>) -> Option<(i32, u64)> {
    trace.and_then(|t| Some((i32::try_from(t.pid).ok()?, t.start_ticks)))
}

/// The Windows node's boxes outlive it (native plan step 8, the decision "the Windows node, on one completion
/// port"): each box's job is named for the node and the box and held by the node's keeper as well as the
/// node, and an `oci` box's container outlives the node with it. The next node takes back each box its
/// records name that is still there, and removes the rest with what is left of them.
#[cfg(windows)]
impl Recover for hex_runtime::windows::WinRuntime {
    fn managed_usage(&self, record: &BoxRecord) -> Result<Usage, Refusal> {
        hex_runtime::windows::WinRuntime::managed_usage(self, record)
    }

    fn launch_managed(
        &self,
        record: &BoxRecord,
        plan: &hex_runtime::devices::ManagedLaunch<'_>,
        io: shards_api::LaunchIo,
        patience: std::time::Duration,
        publish: &mut dyn FnMut(&shards_api::Identity) -> Result<(), Refusal>,
    ) -> Result<shards_api::ManagedReceipt, Refusal> {
        hex_runtime::windows::WinRuntime::launch_managed(self, record, plan, io, patience, publish)
    }

    fn attach_managed(
        &self,
        record: &BoxRecord,
        identity: Option<&shards_api::Identity>,
    ) -> Result<shards_api::ManagedReceipt, Refusal> {
        hex_runtime::windows::WinRuntime::attach_managed(self, record, identity)
    }

    fn finalize_managed(
        &self,
        record: &BoxRecord,
        identity: Option<&shards_api::Identity>,
    ) -> Result<(), Refusal> {
        hex_runtime::windows::WinRuntime::finalize_managed(self, record, identity)
    }

    fn trace(&self, _id: &BoxId) -> Result<Option<ProcId>, Refusal> {
        Ok(None)
    }

    /// For an `oci` box, whether its take-back would take it: its container still held by HCS, and its
    /// init the very process its record names, still running, or not yet started
    /// ([`hex_runtime::windows::WinRuntime::alive`]); a box whose init ended while no node ran is removed,
    /// its container with it.
    fn alive(&self, spec: &BoxSpec) -> Result<Option<bool>, Refusal> {
        hex_runtime::windows::WinRuntime::alive(self, spec)
    }

    /// Takes box `spec` back on this node: a `process` box by its named job, an `oci` box with its
    /// container, scratch and init ([`hex_runtime::windows::WinRuntime::adopt`]). A box's job is named for
    /// the node and the box, so it needs no per-process trace to find.
    fn adopt(&self, spec: &BoxSpec, _trace: Option<ProcId>) -> Result<(), Refusal> {
        hex_runtime::windows::WinRuntime::adopt(self, spec)
    }

    /// Windows ends a box recovery removes by its name, in [`Recover::discard`], which has its spec; a
    /// trace names nothing of a Windows box.
    fn end(&self, _trace: Option<ProcId>) -> Result<(), Refusal> {
        Ok(())
    }

    /// Ends box `spec` a recovery removes: its named job's members, and an `oci` box's container, scratch
    /// and record ([`hex_runtime::windows::WinRuntime::end`]).
    fn discard(&self, spec: &BoxSpec) -> Result<(), Refusal> {
        hex_runtime::windows::WinRuntime::end(self, &spec.id)
    }
}

/// A node whose Linux images are its Shards guest's: its `process` boxes are taken back through the host's
/// runtime. Its `oci` boxes live in its guest, which outlives the node's crash and which the node takes back
/// when it starts (`GuestRuntime::recorded_at`): one the guest holds is taken back as it is, and one it
/// doesn't (the guest was gone, and the node booted another) is removed (`hex_core::recovery`: its init has
/// ended). Neither has host processes to end; a box recovery removes that the guest still holds is deleted
/// there.
impl<P: Recover> Recover for hex_runtime::guest::WithGuest<P> {
    fn managed_usage(&self, record: &BoxRecord) -> Result<Usage, Refusal> {
        self.host().managed_usage(record)
    }

    fn launch_managed(
        &self,
        record: &BoxRecord,
        plan: &hex_runtime::devices::ManagedLaunch<'_>,
        io: shards_api::LaunchIo,
        patience: std::time::Duration,
        publish: &mut dyn FnMut(&shards_api::Identity) -> Result<(), Refusal>,
    ) -> Result<shards_api::ManagedReceipt, Refusal> {
        self.host()
            .launch_managed(record, plan, io, patience, publish)
    }

    fn attach_managed(
        &self,
        record: &BoxRecord,
        identity: Option<&shards_api::Identity>,
    ) -> Result<shards_api::ManagedReceipt, Refusal> {
        self.host().attach_managed(record, identity)
    }

    fn finalize_managed(
        &self,
        record: &BoxRecord,
        identity: Option<&shards_api::Identity>,
    ) -> Result<(), Refusal> {
        self.host().finalize_managed(record, identity)
    }

    fn attachment_offers(&self) -> Result<Vec<hex_api::DeviceInfo>, Refusal> {
        let mut offers = self.host().attachment_offers()?;
        let guest = self.guest().attachment_offers()?;
        offers
            .try_reserve(guest.len())
            .map_err(|_| Refusal::Exhausted {
                what: "initialized attachment offers",
                bound: hex_api::MAX_DEVICES,
            })?;
        for offer in guest {
            if let Some(existing) = offers.iter_mut().find(|existing| existing.id == offer.id) {
                if existing.model != offer.model
                    || existing.size != offer.size
                    || (existing.holder.is_some()
                        && offer.holder.is_some()
                        && existing.holder != offer.holder)
                {
                    return Err(Refusal::Broken(
                        "initialized routes disagree about one physical device".into(),
                    ));
                }
                if existing.holder.is_none() {
                    existing.holder = offer.holder;
                }
            } else {
                if offers.len() >= hex_api::MAX_DEVICES {
                    return Err(Refusal::Exhausted {
                        what: "initialized attachment offers",
                        bound: hex_api::MAX_DEVICES,
                    });
                }
                offers.push(offer);
            }
        }
        Ok(offers)
    }

    fn owns_records(&self, spec: &BoxSpec) -> Result<bool, Refusal> {
        Ok(
            self.route(&spec.class)? == hex_runtime::guest::Side::Guest
                && self.guest().uses_books(),
        )
    }

    fn runtime_for(&self, spec: &BoxSpec) -> Result<&dyn Runtime, Refusal> {
        match self.route(&spec.class)? {
            hex_runtime::guest::Side::Guest => Ok(self.guest()),
            hex_runtime::guest::Side::Host => Ok(self.host()),
        }
    }

    fn trace(&self, id: &BoxId) -> Result<Option<ProcId>, Refusal> {
        match self.host().inspect(id) {
            Ok(_) => self.host().trace(id),
            Err(Refusal::NoSuchBox(_)) => Ok(None),
            Err(why) => Err(why),
        }
    }

    fn adopt(&self, spec: &BoxSpec, trace: Option<ProcId>) -> Result<(), Refusal> {
        match self.route(&spec.class)? {
            // Its guest holds it already.
            hex_runtime::guest::Side::Guest => self.guest().validate_binding(spec),
            hex_runtime::guest::Side::Host => self.host().adopt(spec, trace),
        }
    }

    fn end(&self, trace: Option<ProcId>) -> Result<(), Refusal> {
        self.host().end(trace)
    }

    fn end_for(&self, spec: &BoxSpec, trace: Option<ProcId>) -> Result<(), Refusal> {
        match self.route(&spec.class)? {
            hex_runtime::guest::Side::Guest => Ok(()),
            hex_runtime::guest::Side::Host => self.host().end_for(spec, trace),
        }
    }

    fn alive(&self, spec: &BoxSpec) -> Result<Option<bool>, Refusal> {
        match self.route(&spec.class)? {
            hex_runtime::guest::Side::Guest => {
                self.guest().validate_binding(spec)?;
                match self.guest().inspect(&spec.id) {
                    Ok(_) => Ok(Some(true)),
                    Err(Refusal::NoSuchBox(_)) => Ok(Some(false)),
                    Err(why) => Err(why),
                }
            }
            hex_runtime::guest::Side::Host => self.host().alive(spec),
        }
    }

    fn discard(&self, spec: &BoxSpec) -> Result<(), Refusal> {
        match self.route(&spec.class)? {
            hex_runtime::guest::Side::Guest => match self.guest().delete(&spec.id) {
                Ok(()) | Err(Refusal::NoSuchBox(_)) => Ok(()),
                Err(why) => Err(why),
            },
            hex_runtime::guest::Side::Host => self.host().discard(spec),
        }
    }

    /// Its guest's control connection, in the event form, joins the core's loop, and so does every guest's
    /// it boots or takes back after: a call through it holds no thread while the guest answers (native plan
    /// step 8 (1)).
    fn attach(&self, lp: Arc<dyn Loop>) -> Result<(), Refusal> {
        self.guest().join_loop(Arc::new(LoopJoins(lp)))
    }

    /// A call about a box its guest holds goes there through the event form's client and is answered
    /// from the loop; one about a host box runs on the worker. With no guest running the box is no box; a
    /// guest whose connection can't be joined to the loop refuses the call, naming why.
    ///
    /// A guest program can't take the caller's own pipes, and the node copies no byte between them (native
    /// plan step 8): each stream an exec carries or asks for is the guest's own stdio connection, whose end
    /// goes to the caller's client with the answer, and the caller's pipes are let go here. Its client
    /// copies between those it carried and their ends.
    fn forward(&self, call: Call, stdio: Stdio, reply: Reply, ended: serve::Ended) -> Forward {
        let id = match &call {
            Call::Inspect(id) | Call::Start(id) | Call::Usage(id) => id,
            Call::Signal { id, .. } | Call::Exec { id, .. } | Call::ExecStreams { id, .. } => id,
            _ => return Forward::Back(Box::new((call, stdio))),
        };
        match self.host().inspect(id) {
            Err(Refusal::NoSuchBox(_)) => {}
            Ok(_) | Err(_) => return Forward::Back(Box::new((call, stdio))),
        }
        forward_guest(self.guest(), call, stdio, reply, ended)
    }

    fn forward_for(
        &self,
        spec: &BoxSpec,
        call: Call,
        stdio: Stdio,
        reply: Reply,
        ended: serve::Ended,
    ) -> Forward {
        match self.route(&spec.class) {
            Ok(hex_runtime::guest::Side::Guest) => {
                forward_guest(self.guest(), call, stdio, reply, ended)
            }
            Ok(hex_runtime::guest::Side::Host) => {
                self.host().forward_for(spec, call, stdio, reply, ended)
            }
            Err(error) => Forward::Refused(error),
        }
    }
}

fn forward_guest(
    guest: &hex_runtime::guest::GuestRuntime,
    call: Call,
    stdio: Stdio,
    reply: Reply,
    ended: serve::Ended,
) -> Forward {
    let id = match &call {
        Call::Inspect(id) | Call::Start(id) | Call::Usage(id) => id,
        Call::Signal { id, .. } | Call::Exec { id, .. } | Call::ExecStreams { id, .. } => id,
        _ => return Forward::Back(Box::new((call, stdio))),
    };
    let offer = cfg!(windows)
        && matches!(&call, Call::Exec { .. } | Call::ExecStreams { .. })
        && (stdio.ask.stdin
            || stdio.ask.stdout
            || stdio.ask.stderr
            || stdio.stdin.is_some()
            || stdio.stdout.is_some()
            || stdio.stderr.is_some());
    let connected = if offer {
        guest.program_client_for(id)
    } else {
        guest.evented_for(id)
    };
    let client = match connected {
        Ok(Some(client)) => client,
        Ok(None) => return Forward::Refused(Refusal::NoSuchBox(id.clone())),
        Err(why) => return Forward::Refused(why),
    };
    let (call, stdio) = match call {
        Call::Exec { id, program, .. } | Call::ExecStreams { id, program, .. } => {
            if let Err(why) = stdio.check() {
                return Forward::Refused(why);
            }
            let ask = Ask {
                stdin: stdio.stdin.is_some() || stdio.ask.stdin,
                stdout: stdio.stdout.is_some() || stdio.ask.stdout,
                stderr: stdio.stderr.is_some() || stdio.ask.stderr,
            };
            // The caller's own pipes, let go here: its client keeps its own.
            drop(stdio);
            let streams = hex_core::wire::Streams::default();
            let exec = Call::Exec {
                id,
                program,
                streams,
            };
            let asked = Stdio {
                ask,
                ..Stdio::null()
            };
            (exec, asked)
        }
        call => (call, stdio),
    };
    let started = Arc::clone(&client);
    let then: hex_client::Then = Box::new(move |exec, got| {
        reply(match got {
            Ok(Answer::Started { pid }) => Ok(Forwarded::Started {
                pid,
                given: started.given(exec),
                program: Box::new(GuestProgram {
                    client: started,
                    exec,
                }),
            }),
            Ok(Answer::Refused(why)) | Err(why) => Err(why),
            Ok(answer) => Ok(Forwarded::Answer(Box::new(answer))),
        })
    });
    if offer {
        let streams = hex_core::wire::Streams {
            stdin: stdio.ask.stdin,
            stdout: stdio.ask.stdout,
            stderr: stdio.ask.stderr,
        };
        let call = match call {
            Call::Exec { id, program, .. } | Call::ExecStreams { id, program, .. } => Call::Exec {
                id,
                program,
                streams,
            },
            _ => {
                return Forward::Refused(Refusal::Broken(
                    "only an exec can offer guest streams".to_owned(),
                ));
            }
        };
        let pending = match client.defer(call, then, ended) {
            Ok(pending) => pending,
            Err(why) => return Forward::Refused(why),
        };
        let streams = match client.stream_offer(pending.id(), streams) {
            Ok(streams) => streams,
            Err(why) => return Forward::Refused(why),
        };
        return Forward::Offered(serve::Offering {
            streams,
            ready: Box::new(move || pending.send()),
        });
    }
    // Refused before it was sent: `then` is dropped unrun, and the caller hears why.
    match client.begin(call, stdio, then, ended) {
        Ok(_) => Forward::Taken,
        Err(why) => Forward::Refused(why),
    }
}

/// The core's loop as the guest's runtime joins its connections to it.
struct LoopJoins(Arc<dyn Loop>);

impl hex_runtime::guest::Joins for LoopJoins {
    fn reserve(&self) -> Result<(u64, hex_runtime::guest::Room), Refusal> {
        let token = self.0.token()?;
        let lp = Arc::clone(&self.0);
        Ok((token, Arc::new(move || lp.room(token))))
    }

    fn join(&self, token: u64, pump: hex_client::guest::Pump) -> Result<(), Refusal> {
        self.0.add(token, Box::new(Pumped(pump)))
    }
}

/// The guest's control connection's reading side, as a source of the core's loop.
struct Pumped(hex_client::guest::Pump);

impl serve::Source for Pumped {
    fn handle(&self) -> serve::Handle<'_> {
        self.0.handle()
    }

    fn ready(&mut self) -> serve::Want {
        match self.0.ready() {
            hex_client::guest::Pumped::Read => serve::Want::Read,
            hex_client::guest::Pumped::ReadWrite => serve::Want::ReadWrite,
            hex_client::guest::Pumped::Ended => serve::Want::Done,
        }
    }
}

/// A program in the guest, by its exec's id on the event form's connection.
struct GuestProgram {
    client: Arc<hex_client::guest::Client>,
    exec: u64,
}

impl serve::Remote for GuestProgram {
    fn kill(&self, done: serve::Killed) {
        let done = Arc::new(Mutex::new(Some(done)));
        let answered = Arc::clone(&done);
        let then: hex_client::Then = Box::new(move |_, got| {
            let sent = match got {
                Ok(Answer::Done) => Ok(()),
                Ok(Answer::Refused(why)) | Err(why) => Err(why),
                Ok(_) => Err(Refusal::Broken(
                    "the guest's node answered a kill with something but done".to_owned(),
                )),
            };
            if let Some(done) = answered.lock().ok().and_then(|mut d| d.take()) {
                done(sent);
            }
        });
        if let Err(why) = self.client.kill_then(self.exec, then)
            && let Some(done) = done.lock().ok().and_then(|mut d| d.take())
        {
            done(Err(why));
        }
    }

    fn release(&self) {
        self.client.release(self.exec);
    }
}

#[cfg(test)]
#[path = "../../../docs/research/measurements/hex-complete-lifecycle/recorded_lifecycle_tests.rs"]
mod lifecycle_tests;
