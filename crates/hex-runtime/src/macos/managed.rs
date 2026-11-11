//! Managed Shards controllers reuse the original native ProcessBox, its watcher and its one waiter.
//! The controller remains an unreaped child until the durable selected-boot Released publication.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use hex_api::{Bounds, BoxId, Class, Exit, LifecycleCause, Refusal, Usage};
use hex_core::record::{NativeRecord, NativeScope, OwnerRef};
use shards_api::{
    Identity, ManagedEnd, ManagedProcess, ManagedReceipt, ManagedStatus, ProcessFailure,
    ProcessOperation, TerminalScope,
};
use triomphe::Arc;

use super::proc::{BoxProc, Prepared, Reaping, process_failure};
use super::process::ProcessBox;
use super::{MacRuntime, native_allocation, native_owner_index, sys};

pub(super) fn uncertain(id: &BoxId) -> Refusal {
    match id.try_clone() {
        Ok(id) => Refusal::Lifecycle {
            id,
            cause: LifecycleCause::Uncertain,
        },
        Err(error) => error,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ownership {
    Child,
    Borrowed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Prepared,
    Held,
    Running,
    Observing,
    NotSubmitted,
    Uncertain(ProcessFailure),
    Finalizing(ManagedEnd),
    Finalized(ManagedEnd),
}

#[derive(Debug)]
pub(super) struct Scope {
    scope: NativeScope,
    commitment: [u8; 32],
    ownership: Ownership,
    phase: Mutex<Phase>,
}

impl Scope {
    pub(super) fn new(record: NativeRecord<'_>, ownership: Ownership) -> Result<Self, Refusal> {
        record
            .allocation()
            .ok_or_else(|| record.refusal(LifecycleCause::Uncertain))?
            .guest()?;
        if record.class()? != Class::Shards {
            return Err(record.refusal(LifecycleCause::Uncertain));
        }
        Ok(Self {
            scope: record
                .scope()
                .ok_or_else(|| record.refusal(LifecycleCause::Uncertain))?,
            commitment: record.commitment()?,
            ownership,
            phase: Mutex::new(Phase::Prepared),
        })
    }
    pub(super) fn refusal(&self, owner: OwnerRef<'_>) -> Refusal {
        match owner {
            OwnerRef::Box(id) => uncertain(id),
            OwnerRef::Pool(pool) => Refusal::Backing {
                key: hex_api::BackingKey {
                    pool: *pool,
                    generation: *self.scope.bytes(),
                },
                cause: LifecycleCause::Uncertain,
            },
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Phase>, ProcessFailure> {
        self.phase.lock().map_err(|_| ProcessFailure::Poisoned {
            what: "native scope phase",
        })
    }
    fn matches(&self, record: NativeRecord<'_>, commitment: [u8; 32]) -> bool {
        record.scope() == Some(self.scope) && commitment == self.commitment
    }
    pub(super) fn held(&self, id: OwnerRef<'_>) -> Result<(), Refusal> {
        let mut phase = self.lock().map_err(|_| self.refusal(id))?;
        if *phase != Phase::Prepared {
            return Err(self.refusal(id));
        }
        *phase = Phase::Held;
        Ok(())
    }
    pub(super) fn running(&self, id: OwnerRef<'_>) -> Result<(), Refusal> {
        let mut phase = self.lock().map_err(|_| self.refusal(id))?;
        if *phase != Phase::Held {
            return Err(self.refusal(id));
        }
        *phase = Phase::Running;
        Ok(())
    }
    pub(super) fn not_submitted(&self, id: OwnerRef<'_>) -> Result<(), Refusal> {
        let mut phase = self.lock().map_err(|_| self.refusal(id))?;
        if !matches!(*phase, Phase::Prepared | Phase::Held) {
            return Err(self.refusal(id));
        }
        *phase = Phase::NotSubmitted;
        Ok(())
    }
    pub(super) fn watchable(&self, id: OwnerRef<'_>) -> Result<bool, Refusal> {
        self.lock()
            .map(|phase| *phase == Phase::Running)
            .map_err(|_| self.refusal(id))
    }
    pub(super) fn finalizing(&self, end: ManagedEnd, id: OwnerRef<'_>) -> Result<(), Refusal> {
        if end.scope != TerminalScope::Released {
            return Err(self.refusal(id));
        }
        let mut phase = self.lock().map_err(|_| self.refusal(id))?;
        match &*phase {
            Phase::Finalizing(known) | Phase::Finalized(known) if *known != end => {
                return Err(self.refusal(id));
            }
            Phase::Finalized(_) => return Ok(()),
            _ => {}
        }
        *phase = Phase::Finalizing(end);
        Ok(())
    }
    fn finalized(&self, id: OwnerRef<'_>) -> Result<(), Refusal> {
        let mut phase = self.lock().map_err(|_| self.refusal(id))?;
        match *phase {
            Phase::Finalizing(end) => {
                *phase = Phase::Finalized(end);
                Ok(())
            }
            Phase::Finalized(_) => Ok(()),
            _ => Err(self.refusal(id)),
        }
    }
    pub(super) fn is_finalized(&self) -> bool {
        self.lock()
            .is_ok_and(|phase| matches!(*phase, Phase::Finalizing(_) | Phase::Finalized(_)))
    }
    pub(super) fn failed(&self, failure: ProcessFailure) {
        if let Ok(mut phase) = self.lock()
            && !matches!(*phase, Phase::Finalizing(_) | Phase::Finalized(_))
        {
            *phase = Phase::Uncertain(failure);
        }
    }
    pub(super) fn validate(
        &self,
        pgid: Option<i32>,
        started: Option<u64>,
        id: OwnerRef<'_>,
    ) -> Result<(), Refusal> {
        self.validate_native(pgid, started)
            .map_err(|_| self.refusal(id))
    }
    pub(super) fn validate_native(
        &self,
        pgid: Option<i32>,
        started: Option<u64>,
    ) -> Result<(), ProcessFailure> {
        let Some(pid) = pgid else {
            return Ok(());
        };
        if pid <= 0 {
            return Err(ProcessFailure::PidOutOfRange {
                operation: ProcessOperation::QueryScope,
            });
        }
        // A direct child is never reaped by this native owner before durable Released/finalize.
        // An adopted leader has no such lifetime pin and may only be observed, never signalled.
        if self.ownership != Ownership::Child {
            return Err(ProcessFailure::DifferentScope);
        }
        let actual = sys::rusage(pid)
            .map_err(|error| process_failure(ProcessOperation::ProcPidRusage, error))?
            .ok_or(ProcessFailure::InvalidStartRecord)?;
        if started.is_some_and(|expected| expected != actual.started) {
            return Err(ProcessFailure::DifferentStart {
                observed: actual.started,
            });
        }
        if !actual.ended
            && sys::group_of(pid)
                .map_err(|error| process_failure(ProcessOperation::QueryScope, error))?
                != Some(pid)
        {
            return Err(ProcessFailure::DifferentScope);
        }
        Ok(())
    }
}

pub(super) enum Controller {
    Unsubmitted,
    Prepared(Prepared),
    Held {
        process: BoxProc,
        failure: Option<ProcessFailure>,
    },
}

fn host_bounds(record: NativeRecord<'_>) -> Result<Bounds, Refusal> {
    if record.class()? != Class::Shards {
        return Err(record.refusal(LifecycleCause::Uncertain));
    }
    let allocation = record
        .allocation()
        .ok_or_else(|| record.refusal(LifecycleCause::Uncertain))?;
    allocation.guest()?;
    let bounds = Bounds {
        cpu_max_milli: Some(
            u32::try_from(allocation.resources.cpu_milli)
                .map_err(|_| record.refusal(LifecycleCause::Uncertain))?,
        ),
        memory_max: Some(allocation.resources.mem_bytes),
        ..Bounds::default()
    };
    bounds.check()?;
    Ok(bounds)
}

fn bytes(value: &[u8], record: NativeRecord<'_>) -> Result<CString, Refusal> {
    let count = value
        .len()
        .checked_add(1)
        .ok_or_else(|| native_allocation(usize::MAX))?;
    let mut copied = Vec::new();
    copied
        .try_reserve_exact(count)
        .map_err(|_| native_allocation(count))?;
    copied.extend_from_slice(value);
    copied.push(0);
    CString::from_vec_with_nul(copied).map_err(|_| record.refusal(LifecycleCause::Uncertain))
}
fn environment(
    name: &[u8],
    path: &std::path::Path,
    record: NativeRecord<'_>,
) -> Result<CString, Refusal> {
    let path = path.as_os_str().as_bytes();
    let count = name
        .len()
        .checked_add(path.len())
        .and_then(|count| count.checked_add(2))
        .ok_or_else(|| native_allocation(usize::MAX))?;
    let mut value = Vec::new();
    value
        .try_reserve_exact(count)
        .map_err(|_| native_allocation(count))?;
    value.extend_from_slice(name);
    value.push(b'=');
    value.extend_from_slice(path);
    value.push(0);
    CString::from_vec_with_nul(value).map_err(|_| record.refusal(LifecycleCause::Uncertain))
}
fn spawn(
    runtime: &MacRuntime,
    record: NativeRecord<'_>,
    plan: &shards_api::PreparedBoot,
    io: shards_api::LaunchIo,
    bounds: &Bounds,
) -> Result<sys::Spawn, Refusal> {
    let paths = plan
        .runtime_paths()
        .ok_or_else(|| record.refusal(LifecycleCause::Uncertain))?;
    if !plan
        .arguments()
        .iter()
        .take_while(|argument| argument.as_os_str() != std::ffi::OsStr::new("--"))
        .any(|argument| argument.as_os_str() == std::ffi::OsStr::new("--host-managed"))
    {
        return Err(record.refusal(LifecycleCause::Uncertain));
    }
    if !plan.binary().is_absolute() {
        return Err(record.refusal(LifecycleCause::Uncertain));
    }
    let count = plan
        .arguments()
        .len()
        .checked_add(1)
        .ok_or_else(|| native_allocation(usize::MAX))?;
    let mut argv = Vec::new();
    argv.try_reserve_exact(count)
        .map_err(|_| native_allocation(count))?;
    for word in std::iter::once(plan.binary().as_os_str())
        .chain(plan.arguments().iter().map(|word| word.as_os_str()))
    {
        argv.push(bytes(word.as_bytes(), record)?);
    }
    let mut envp = Vec::new();
    envp.try_reserve_exact(2)
        .map_err(|_| native_allocation(2))?;
    envp.push(environment(b"SHARDS_CACHE", paths.cache(), record)?);
    envp.push(environment(b"SHARDS_HOME", paths.home(), record)?);
    let size = argv
        .iter()
        .chain(&envp)
        .try_fold(0usize, |total, word| {
            total.checked_add(word.as_bytes_with_nul().len())
        })
        .ok_or_else(|| native_allocation(usize::MAX))?;
    let pointers = argv
        .len()
        .checked_add(envp.len())
        .and_then(|count| count.checked_add(2))
        .and_then(|count| count.checked_mul(size_of::<*const std::ffi::c_char>()))
        .ok_or_else(|| native_allocation(usize::MAX))?;
    if size
        .checked_add(pointers)
        .is_none_or(|size| size > runtime.exec_limit.block)
    {
        return Err(record.refusal(LifecycleCause::Uncertain));
    }
    let mut binaries = Vec::new();
    binaries
        .try_reserve_exact(1)
        .map_err(|_| native_allocation(1))?;
    binaries.push(bytes(plan.binary().as_os_str().as_bytes(), record)?);
    Ok(sys::Spawn {
        paths: binaries,
        argv,
        envp,
        cwd: None,
        stdio: [
            super::null(rustix::fs::OFlags::RDONLY)?,
            io.stdout.into(),
            io.stderr.into(),
        ],
        jetsam_mib: bounds.memory_max.and_then(sys::jetsam_cap_mib),
    })
}

fn existing(
    runtime: &MacRuntime,
    record: NativeRecord<'_>,
) -> Result<Option<Arc<ProcessBox>>, Refusal> {
    let commitment = record.commitment()?;
    let boxes = runtime.boxes()?;
    let Ok(at) = native_owner_index(&boxes, record.owner()) else {
        return Ok(None);
    };
    match boxes.get(at) {
        Some((_, boxed))
            if boxed
                .scope()
                .is_some_and(|scope| scope.matches(record, commitment)) =>
        {
            Ok(Some(Arc::clone(boxed)))
        }
        _ => Err(record.refusal(LifecycleCause::Uncertain)),
    }
}

fn retain(
    runtime: &MacRuntime,
    record: NativeRecord<'_>,
    boxed: &Arc<ProcessBox>,
) -> Result<(), Refusal> {
    let id = record.owner().try_owned()?;
    let mut boxes = runtime.boxes()?;
    let at = native_owner_index(&boxes, id.as_ref())
        .map_or_else(Ok, |_| Err(record.refusal(LifecycleCause::Uncertain)))?;
    if boxes.len() >= crate::MAX_BOXES {
        return Err(Refusal::Full(crate::MAX_BOXES));
    }
    boxes
        .try_reserve(1)
        .map_err(|_| native_allocation(crate::MAX_BOXES))?;
    boxes.insert(at, (id, Arc::clone(boxed)));
    Ok(())
}

pub(super) fn launch(
    runtime: &MacRuntime,
    record: NativeRecord<'_>,
    launch: &crate::devices::ManagedLaunch<'_>,
    io: shards_api::LaunchIo,
    patience: Duration,
    publish: &mut dyn FnMut(&Identity) -> Result<(), Refusal>,
) -> Result<ManagedReceipt, Refusal> {
    let crate::devices::ManagedLaunch::Shards(plan) = launch else {
        return Err(record.refusal(LifecycleCause::Uncertain));
    };
    let deadline = Instant::now()
        .checked_add(patience)
        .filter(|_| patience != Duration::MAX && !patience.is_zero())
        .ok_or_else(|| record.refusal(LifecycleCause::Uncertain))?;
    let bounds = host_bounds(record)?;
    let spawn = spawn(runtime, record, plan, io, &bounds)?;
    let vsock = copy_path(plan.vsock())?;
    sys::retained_children()
        .map_err(|error| Refusal::os("checking retained-child ownership", error))?;
    let boxed = ProcessBox::prepare_managed(record, &bounds, &runtime.shared, Ownership::Child)?;
    retain(runtime, record, &boxed)?;
    if Instant::now() >= deadline {
        *boxed
            .scope()
            .ok_or_else(|| record.refusal(LifecycleCause::Uncertain))?
            .lock()
            .map_err(|_| record.refusal(LifecycleCause::Uncertain))? = Phase::NotSubmitted;
        return Err(record.refusal(LifecycleCause::Uncertain));
    }
    let mut controller = boxed
        .controller
        .lock()
        .map_err(|_| record.refusal(LifecycleCause::Uncertain))?;
    let prepared = match std::mem::replace(&mut *controller, Controller::Unsubmitted) {
        Controller::Prepared(prepared) => prepared,
        other => {
            *controller = other;
            return Err(record.refusal(LifecycleCause::Uncertain));
        }
    };
    let pid = match boxed.held_exec(&spawn) {
        Ok(pid) => pid,
        Err(error) => {
            boxed.not_submitted()?;
            return Err(error);
        }
    };
    let (process, registered) = prepared.bind(pid, Some(boxed.members()), Reaping::Retained);
    *controller = Controller::Held {
        process,
        failure: registered
            .err()
            .map(|error| process_failure(ProcessOperation::RegisterExit, error)),
    };
    let observed = sys::rusage(pid)
        .map_err(|error| Refusal::os("reading the held VM controller birth", error))?
        .filter(|usage| usage.started != 0)
        .ok_or_else(|| record.refusal(LifecycleCause::Uncertain))?;
    boxed.introduce_group(pid, observed.started)?;
    let identity = Identity {
        pid: u32::try_from(pid).map_err(|_| record.refusal(LifecycleCause::Uncertain))?,
        started: observed.started,
        vsock,
    };
    boxed
        .identity
        .set(identity)
        .map_err(|_| record.refusal(LifecycleCause::Uncertain))?;
    if let Controller::Held {
        failure: Some(_), ..
    } = &*controller
    {
        return Err(record.refusal(LifecycleCause::Uncertain));
    }
    publish(
        boxed
            .identity
            .get()
            .ok_or_else(|| record.refusal(LifecycleCause::Uncertain))?,
    )?;
    if Instant::now() >= deadline {
        return Err(record.refusal(LifecycleCause::Uncertain));
    }
    boxed.resume_managed()?;
    drop(controller);
    Ok(shards_api::managed_receipt(boxed))
}

fn copy_path(path: &std::path::Path) -> Result<std::path::PathBuf, Refusal> {
    let mut value = std::ffi::OsString::new();
    value
        .try_reserve_exact(path.as_os_str().len())
        .map_err(|_| native_allocation(path.as_os_str().len()))?;
    value.push(path);
    Ok(value.into())
}

pub(super) fn attach(
    runtime: &MacRuntime,
    record: NativeRecord<'_>,
    identity: Option<&Identity>,
) -> Result<ManagedReceipt, Refusal> {
    if let Some(boxed) = existing(runtime, record)? {
        if identity.is_some_and(|identity| boxed.identity.get() != Some(identity)) {
            return Err(record.refusal(LifecycleCause::Uncertain));
        }
        return Ok(shards_api::managed_receipt(boxed));
    }
    let identity = identity.ok_or_else(|| record.refusal(LifecycleCause::Uncertain))?;
    if !record
        .init()
        .is_some_and(|init| init.pid == identity.pid && init.start_ticks == identity.started)
    {
        return Err(record.refusal(LifecycleCause::Uncertain));
    }
    let pid = i32::try_from(identity.pid)
        .ok()
        .filter(|pid| *pid > 0)
        .ok_or_else(|| record.refusal(LifecycleCause::Uncertain))?;
    let actual = sys::rusage(pid)
        .map_err(|error| Refusal::os("reading the recorded controller", error))?
        .filter(|actual| actual.started == identity.started && !actual.ended)
        .ok_or_else(|| record.refusal(LifecycleCause::Uncertain))?;
    if sys::group_of(pid)
        .map_err(|error| Refusal::os("reading the recorded controller group", error))?
        != Some(pid)
    {
        return Err(record.refusal(LifecycleCause::Uncertain));
    }
    let bounds = host_bounds(record)?;
    let boxed = ProcessBox::prepare_managed(record, &bounds, &runtime.shared, Ownership::Borrowed)?;
    boxed.introduce_group(pid, actual.started)?;
    let mut controller = boxed
        .controller
        .lock()
        .map_err(|_| record.refusal(LifecycleCause::Uncertain))?;
    let prepared = match std::mem::replace(&mut *controller, Controller::Unsubmitted) {
        Controller::Prepared(prepared) => prepared,
        other => {
            *controller = other;
            return Err(record.refusal(LifecycleCause::Uncertain));
        }
    };
    let (process, registered) = prepared.bind(
        pid,
        Some(boxed.members()),
        Reaping::Borrowed {
            started: identity.started,
        },
    );
    *controller = Controller::Held {
        process,
        failure: registered
            .err()
            .map(|error| process_failure(ProcessOperation::RegisterExit, error)),
    };
    boxed
        .identity
        .set(Identity {
            pid: identity.pid,
            started: identity.started,
            vsock: copy_path(&identity.vsock)?,
        })
        .map_err(|_| record.refusal(LifecycleCause::Uncertain))?;
    *boxed
        .scope()
        .ok_or_else(|| record.refusal(LifecycleCause::Uncertain))?
        .lock()
        .map_err(|_| record.refusal(LifecycleCause::Uncertain))? = Phase::Observing;
    drop(controller);
    retain(runtime, record, &boxed)?;
    Ok(shards_api::managed_receipt(boxed))
}

pub(super) fn usage(runtime: &MacRuntime, record: NativeRecord<'_>) -> Result<Usage, Refusal> {
    let boxed =
        existing(runtime, record)?.ok_or_else(|| record.refusal(LifecycleCause::Uncertain))?;
    boxed
        .scope()
        .ok_or_else(|| record.refusal(LifecycleCause::Uncertain))?
        .validate(
            boxed.group()?.map(|pair| pair.0),
            boxed.group()?.map(|pair| pair.1),
            boxed.owner(),
        )?;
    boxed.usage()
}

pub(super) fn finalize(
    runtime: &MacRuntime,
    record: NativeRecord<'_>,
    identity: Option<&Identity>,
) -> Result<(), Refusal> {
    let Some(boxed) = existing(runtime, record)? else {
        // Only the internal transaction calls this after exact durable Released publication.
        // An absent native slot is not independently promoted to a scope-empty observation.
        record
            .scope()
            .ok_or_else(|| record.refusal(LifecycleCause::Uncertain))?;
        return Ok(());
    };
    if identity.is_some_and(|identity| boxed.identity.get() != Some(identity)) {
        return Err(record.refusal(LifecycleCause::Uncertain));
    }
    let end = boxed
        .terminal()
        .map_err(|_| record.refusal(LifecycleCause::Uncertain))?
        .filter(|end| end.scope == TerminalScope::Released)
        .ok_or_else(|| record.refusal(LifecycleCause::Uncertain))?;
    boxed.finalize_watcher(end)?;
    if let Controller::Held { process, .. } = &mut *boxed
        .controller
        .lock()
        .map_err(|_| record.refusal(LifecycleCause::Uncertain))?
    {
        process
            .finalize_retained()
            .map_err(|_| record.refusal(LifecycleCause::Uncertain))?;
    }
    boxed
        .scope()
        .ok_or_else(|| record.refusal(LifecycleCause::Uncertain))?
        .finalized(record.owner())?;
    let mut boxes = runtime.boxes()?;
    let at = native_owner_index(&boxes, record.owner())
        .map_err(|_| record.refusal(LifecycleCause::Uncertain))?;
    if !boxes
        .get(at)
        .is_some_and(|(_, current)| Arc::ptr_eq(current, &boxed))
    {
        return Err(record.refusal(LifecycleCause::Uncertain));
    }
    boxes.remove(at);
    Ok(())
}

fn status(exit: Exit) -> ManagedStatus {
    match exit {
        Exit::Code(code) => ManagedStatus::Code(code),
        Exit::Signal(signal) => ManagedStatus::Signal(signal),
        Exit::Unknown => ManagedStatus::Unknown,
    }
}

impl ProcessBox {
    fn snapshot(
        &self,
        controller: &mut Controller,
        pids: &mut Vec<i32>,
    ) -> Result<Option<ManagedEnd>, ProcessFailure> {
        let scope = self.scope().ok_or(ProcessFailure::InvalidStartRecord)?;
        let phase = scope.lock()?;
        let observation_failure = match &*phase {
            Phase::Uncertain(failure) => Some(failure.clone()),
            _ => None,
        };
        if let Phase::Finalizing(end) | Phase::Finalized(end) = *phase {
            return Ok(Some(end));
        }
        if *phase == Phase::NotSubmitted {
            return Ok(Some(ManagedEnd {
                status: ManagedStatus::Unknown,
                scope: TerminalScope::Released,
            }));
        }
        drop(phase);
        let Controller::Held { process, failure } = controller else {
            return Ok(None);
        };
        let observation_failure = failure.clone().or(observation_failure);
        let exit = process.native_poll()?;
        let Some(exit) = exit else {
            return match observation_failure {
                Some(failure) => Err(failure),
                None => Ok(None),
            };
        };
        if scope.ownership != Ownership::Child {
            // A cold observer has no unreaped-child pin; it cannot signal a potentially reused PGID.
            return Ok(Some(ManagedEnd {
                status: status(exit),
                scope: TerminalScope::Process,
            }));
        }
        let pid = process.pid_i32();
        let actual = sys::rusage(pid)
            .map_err(|error| process_failure(ProcessOperation::ProcPidRusage, error))?
            .ok_or(ProcessFailure::InvalidStartRecord)?;
        if self
            .identity
            .get()
            .is_some_and(|identity| actual.started != identity.started)
        {
            return Err(ProcessFailure::DifferentStart {
                observed: actual.started,
            });
        }
        // WNOWAIT still owns this exact direct child. Its zombie pins the launch PGID even
        // where getpgid no longer exposes an exited process through proc_find.
        if !actual.ended {
            return Err(ProcessFailure::InvalidStartRecord);
        }
        if self
            .status()
            .map_err(|_| ProcessFailure::Poisoned {
                what: "native scope accounting",
            })?
            .escaped
            != 0
        {
            return Err(ProcessFailure::DifferentScope);
        }
        if sys::group(pid, pids)
            .map_err(|error| process_failure(ProcessOperation::QueryScope, error))?
        {
            return Err(ProcessFailure::BufferBounds);
        }
        let mut observation_error = None;
        pids.retain(|member| match sys::rusage(*member) {
            Ok(Some(usage)) => !usage.ended,
            Ok(None) => {
                observation_error.get_or_insert(ProcessFailure::InvalidStartRecord);
                false
            }
            Err(error) => {
                observation_error
                    .get_or_insert_with(|| process_failure(ProcessOperation::ProcPidRusage, error));
                false
            }
        });
        if let Some(error) = observation_error {
            return Err(error);
        }
        let live = !pids.is_empty();
        Ok(Some(ManagedEnd {
            status: status(exit),
            scope: if live {
                TerminalScope::Process
            } else {
                TerminalScope::Released
            },
        }))
    }
}

impl ManagedProcess for ProcessBox {
    fn identity(&self) -> Result<&Identity, shards_api::Refusal> {
        self.identity.get().ok_or(shards_api::Refusal::Scope(
            ProcessFailure::InvalidStartRecord,
        ))
    }
    fn terminal(&self) -> Result<Option<ManagedEnd>, shards_api::Refusal> {
        let mut controller = self.controller.lock().map_err(|_| {
            shards_api::Refusal::Scope(ProcessFailure::Poisoned {
                what: "native controller",
            })
        })?;
        let mut pids = self.scope_pids.lock().map_err(|_| {
            shards_api::Refusal::Scope(ProcessFailure::Poisoned {
                what: "native scope members",
            })
        })?;
        self.snapshot(&mut controller, &mut pids)
            .map_err(shards_api::Refusal::Scope)
    }
    fn wait_for(&self, patience: Duration) -> Result<Option<ManagedEnd>, shards_api::Refusal> {
        let until = if patience == Duration::MAX {
            None
        } else {
            Some(
                Instant::now()
                    .checked_add(patience)
                    .ok_or(shards_api::Refusal::Scope(
                        ProcessFailure::InvalidStartRecord,
                    ))?,
            )
        };
        let mut controller = self.controller.lock().map_err(|_| {
            shards_api::Refusal::Scope(ProcessFailure::Poisoned {
                what: "native controller",
            })
        })?;
        let mut pids = self.scope_pids.lock().map_err(|_| {
            shards_api::Refusal::Scope(ProcessFailure::Poisoned {
                what: "native scope members",
            })
        })?;
        loop {
            if let Some(end) = self
                .snapshot(&mut controller, &mut pids)
                .map_err(shards_api::Refusal::Scope)?
            {
                if end.scope == TerminalScope::Released {
                    return Ok(Some(end));
                }
                if self
                    .scope()
                    .is_none_or(|scope| scope.ownership != Ownership::Child)
                {
                    return Err(shards_api::Refusal::Scope(
                        ProcessFailure::InvalidStartRecord,
                    ));
                }
            }
            let remaining = until.map_or(Duration::MAX, |until| {
                until.saturating_duration_since(Instant::now())
            });
            if remaining.is_zero() {
                return Ok(None);
            }
            let Controller::Held { process, .. } = &mut *controller else {
                return Err(shards_api::Refusal::Scope(
                    ProcessFailure::InvalidStartRecord,
                ));
            };
            if !process
                .wait_scope(&pids, remaining)
                .map_err(shards_api::Refusal::Scope)?
            {
                return Ok(None);
            }
        }
    }
    fn stop(&self) -> Result<(), shards_api::Refusal> {
        let scope = self.scope().ok_or(shards_api::Refusal::Scope(
            ProcessFailure::InvalidStartRecord,
        ))?;
        if scope.ownership != Ownership::Child {
            return Err(shards_api::Refusal::Scope(
                ProcessFailure::InvalidStartRecord,
            ));
        }
        self.stop_managed_raw().map_err(shards_api::Refusal::Scope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> Scope {
        Scope {
            scope: NativeScope::from_bytes([1; 16]),
            commitment: [2; 32],
            ownership: Ownership::Child,
            phase: Mutex::new(Phase::Prepared),
        }
    }

    /// AC-T7.4.1: typed uncertainty retains the exact backing generation without a Box alias.
    #[test]
    fn pool_scope_errors_keep_the_same_generation() {
        let scope = scope();
        let pool = [9; 32];
        assert!(
            matches!(scope.refusal(OwnerRef::Pool(&pool)), Refusal::Backing { key, .. }
            if key.pool == pool && key.generation == [1; 16])
        );
        assert!(scope.running(OwnerRef::Pool(&pool)).is_err());
        scope.held(OwnerRef::Pool(&pool)).unwrap();
        scope.running(OwnerRef::Pool(&pool)).unwrap();
    }

    /// AC-T7.4.1: a controller cannot become watchable before its held publication stage.
    #[test]
    fn preparation_cannot_skip_held_publication() {
        let id = BoxId::new("scope-order").unwrap();
        let scope = scope();
        assert!(!scope.watchable(OwnerRef::Box(&id)).unwrap());
        assert!(scope.running(OwnerRef::Box(&id)).is_err());
        scope.held(OwnerRef::Box(&id)).unwrap();
        assert!(!scope.watchable(OwnerRef::Box(&id)).unwrap());
        scope.running(OwnerRef::Box(&id)).unwrap();
        assert!(scope.watchable(OwnerRef::Box(&id)).unwrap());
        assert!(scope.not_submitted(OwnerRef::Box(&id)).is_err());
    }

    /// AC-T7.4.1: cleanup retries keep the exact already observed terminal result after durable release.
    #[test]
    fn finalize_keeps_the_exact_released_result() {
        let id = BoxId::new("scope-result").unwrap();
        let scope = scope();
        let end = ManagedEnd {
            status: ManagedStatus::Code(7),
            scope: TerminalScope::Released,
        };
        assert!(
            scope
                .finalizing(
                    ManagedEnd {
                        status: end.status,
                        scope: TerminalScope::Process
                    },
                    OwnerRef::Box(&id)
                )
                .is_err()
        );
        scope.finalizing(end, OwnerRef::Box(&id)).unwrap();
        scope.failed(ProcessFailure::InvalidStartRecord);
        assert!(scope.is_finalized());
        assert!(
            scope
                .finalizing(
                    ManagedEnd {
                        status: ManagedStatus::Code(8),
                        ..end
                    },
                    OwnerRef::Box(&id)
                )
                .is_err()
        );
        scope.finalized(OwnerRef::Box(&id)).unwrap();
        assert_eq!(*scope.lock().unwrap(), Phase::Finalized(end));
    }
}
