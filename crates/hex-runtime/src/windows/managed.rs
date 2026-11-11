//! The original Shards row's native job receipt. The API drains output; the existing BoxProc alone
//! observes the controller. Helper completion waits observe the same nested job and never reap PIDs.
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use triomphe::Arc;

use hex_api::{Bounds, BoxSpec, Class, Exit, Refusal, RuntimeCost, Stdio};
use shards_api::{
    Identity, ManagedEnd, ManagedProcess, ManagedReceipt, ManagedStatus, ProcessFailure,
    ProcessOperation, TerminalScope,
};

use super::proc::{BoxProc, ObserveError, Operation};
use super::process::JobBox;
use super::sys::{self, Member};
use super::{Held, WinRuntime, copy_path, copy_text, pending, std_handles};

pub(super) struct Receipt {
    identity: Identity,
    process: Mutex<BoxProc>,
}

fn uncertain(spec: &BoxSpec) -> Refusal {
    match spec.id.try_clone() {
        Ok(id) => Refusal::Lifecycle {
            id,
            cause: hex_api::LifecycleCause::Uncertain,
        },
        Err(error) => error,
    }
}

fn host_bounds(spec: &BoxSpec) -> Result<Bounds, Refusal> {
    spec.check()?;
    let allocation = spec.allocation.ok_or_else(|| uncertain(spec))?;
    allocation.guest()?;
    let bounds = Bounds {
        cpu_max_milli: Some(
            u32::try_from(allocation.resources.cpu_milli).map_err(|_| uncertain(spec))?,
        ),
        memory_max: Some(allocation.resources.mem_bytes),
        ..Bounds::default()
    };
    bounds.check()?;
    Ok(bounds)
}

/// Called at initialized backend registration, before any candidate can be admitted. It checks the
/// exact CPU representation of every guest quantum plus host overhead; it invents no allowance.
pub(super) fn check_cost(runtime: &WinRuntime, cost: &RuntimeCost) -> Result<(), Refusal> {
    cost.check()?;
    let host = cost
        .allocate(hex_api::CapacityResources::default())?
        .resources;
    let denominator = u64::from(runtime.processors.get())
        .checked_mul(1000)
        .ok_or_else(|| pending("native CPU denominator overflows"))?;
    let exact = |cpu: u64| -> Result<bool, Refusal> {
        Ok(cpu
            .checked_mul(10_000)
            .ok_or_else(|| pending("native CPU cost conversion overflows"))?
            .checked_rem(denominator)
            .ok_or_else(|| pending("native CPU denominator is zero"))?
            == 0)
    };
    if !exact(host.cpu_milli)? || !exact(cost.quantum.cpu_milli)? {
        return Err(Refusal::Invalid {
            what: "initialized VM runtime cost",
            why: copy_text(
                "the measured CPU floor/overhead and quantum are not exactly representable by this host's job hard-cap units",
            )?,
        });
    }
    Ok(())
}

// CreateProcessW and Microsoft's CRT command-line grammar operate on UTF-16, including unpaired
// surrogates in an OS path. The core's String-only quoting cannot represent those paths losslessly.
fn command_line(plan: &shards_api::PreparedBoot) -> Result<Vec<u16>, Refusal> {
    let bound = hex_core::hcs::COMMAND_LINE_UNITS
        .checked_add(1)
        .ok_or_else(|| pending("native command-line bound overflows"))?;
    let mut line = Vec::new();
    line.try_reserve_exact(bound)
        .map_err(|_| super::allocation(bound))?;
    for (at, word) in std::iter::once(plan.binary().as_os_str())
        .chain(plan.arguments().iter().map(|arg| arg.as_os_str()))
        .enumerate()
    {
        if at != 0 {
            put(&mut line, bound, u16::from(b' '))?;
        }
        quote(&mut line, bound, word)?;
    }
    put(&mut line, bound, 0)?;
    Ok(line)
}
fn put(line: &mut Vec<u16>, bound: usize, value: u16) -> Result<(), Refusal> {
    if line.len() >= bound {
        return Err(Refusal::Invalid {
            what: "managed VM command line",
            why: copy_text("the prepared command exceeds CreateProcessW's UTF-16 bound")?,
        });
    }
    line.push(value);
    Ok(())
}
fn slashes(line: &mut Vec<u16>, bound: usize, count: usize) -> Result<(), Refusal> {
    for _ in 0..count {
        put(line, bound, u16::from(b'\\'))?;
    }
    Ok(())
}
fn quote(line: &mut Vec<u16>, bound: usize, word: &OsStr) -> Result<(), Refusal> {
    let quoted = word.is_empty()
        || word.encode_wide().any(|unit| {
            [
                u16::from(b' '),
                u16::from(b'\t'),
                u16::from(b'\n'),
                u16::from(b'\x0b'),
                u16::from(b'"'),
            ]
            .contains(&unit)
        });
    if !quoted {
        for unit in word.encode_wide() {
            if unit == 0 {
                return Err(pending("a managed argument contains NUL"));
            }
            put(line, bound, unit)?;
        }
        return Ok(());
    }
    put(line, bound, u16::from(b'"'))?;
    let mut backslashes = 0usize;
    for unit in word.encode_wide() {
        if unit == 0 {
            return Err(pending("a managed argument contains NUL"));
        }
        if unit == u16::from(b'\\') {
            backslashes = backslashes
                .checked_add(1)
                .ok_or_else(|| super::allocation(bound))?;
            continue;
        }
        let count = if unit == u16::from(b'"') {
            backslashes
                .checked_mul(2)
                .and_then(|count| count.checked_add(1))
                .ok_or_else(|| super::allocation(bound))?
        } else {
            backslashes
        };
        slashes(line, bound, count)?;
        put(line, bound, unit)?;
        backslashes = 0;
    }
    slashes(
        line,
        bound,
        backslashes
            .checked_mul(2)
            .ok_or_else(|| super::allocation(bound))?,
    )?;
    put(line, bound, u16::from(b'"'))
}

fn job_name(
    runtime: &WinRuntime,
    record: &hex_core::record::BoxRecord,
) -> Result<Option<String>, Refusal> {
    let scope = record.native_scope.ok_or_else(|| uncertain(&record.spec))?;
    let Some(node) = runtime.node.as_ref() else {
        return Ok(None);
    };
    let prefix = "Local\\hex-";
    let suffix = "-vm-";
    let hex_bytes = scope
        .bytes()
        .len()
        .checked_mul(2)
        .ok_or_else(|| super::allocation(usize::MAX))?;
    let size = prefix
        .len()
        .checked_add(node.len())
        .and_then(|size| size.checked_add(suffix.len()))
        .and_then(|size| size.checked_add(hex_bytes))
        .ok_or_else(|| super::allocation(usize::MAX))?;
    let mut name = String::new();
    name.try_reserve_exact(size)
        .map_err(|_| super::allocation(size))?;
    name.push_str(prefix);
    name.push_str(node);
    name.push_str(suffix);
    use std::fmt::Write;
    for byte in scope.bytes() {
        write!(name, "{byte:02x}").map_err(|_| super::allocation(size))?;
    }
    Ok(Some(name))
}

fn runtime_environment(paths: &shards_api::RuntimePaths) -> Result<Vec<u16>, Refusal> {
    // The exact configured provider pair, in Windows' case-insensitive environment order.
    let values = [
        ("SHARDS_CACHE", paths.cache()),
        ("SHARDS_HOME", paths.home()),
    ];
    let mut size = 1usize;
    for (name, path) in values {
        let entry = name
            .len()
            .checked_add(path.as_os_str().encode_wide().count())
            .and_then(|count| count.checked_add(2))
            .ok_or_else(|| super::allocation(usize::MAX))?;
        if entry > hex_core::hcs::ENV_VAR_UNITS {
            return Err(Refusal::Invalid {
                what: "configured native runtime environment",
                why: copy_text(
                    "a configured runtime path exceeds Windows' environment variable bound",
                )?,
            });
        }
        size = size
            .checked_add(entry)
            .ok_or_else(|| super::allocation(usize::MAX))?;
    }
    let mut environment = Vec::new();
    environment
        .try_reserve_exact(size)
        .map_err(|_| super::allocation(size))?;
    for (name, path) in values {
        environment.extend(name.encode_utf16());
        environment.push(u16::from(b'='));
        for unit in path.as_os_str().encode_wide() {
            if unit == 0 {
                return Err(pending("a configured runtime path contains NUL"));
            }
            environment.push(unit);
        }
        environment.push(0);
    }
    environment.push(0);
    Ok(environment)
}

pub(super) fn launch(
    runtime: &WinRuntime,
    record: &hex_core::record::BoxRecord,
    launch: &crate::devices::ManagedLaunch<'_>,
    io: shards_api::LaunchIo,
    patience: Duration,
    publish: &mut dyn FnMut(&Identity) -> Result<(), Refusal>,
) -> Result<ManagedReceipt, Refusal> {
    let spec = &record.spec;
    let crate::devices::ManagedLaunch::Shards(plan) = launch else {
        return Err(uncertain(spec));
    };
    let paths = plan.runtime_paths().ok_or_else(|| uncertain(spec))?;
    let deadline = Instant::now()
        .checked_add(patience)
        .filter(|_| patience != Duration::MAX && !patience.is_zero())
        .ok_or_else(|| uncertain(spec))?;
    record.native_scope.ok_or_else(|| uncertain(spec))?;
    let bounds = host_bounds(spec)?;
    if !matches!(spec.class.class(), Class::Shards | Class::Oci) {
        return Err(uncertain(spec));
    }
    // Prepare every argument, path and stream before a native controller can exist.
    let vsock = copy_path(plan.vsock())?;
    let mut spawn = sys::Spawn {
        command_line: command_line(plan)?,
        environment: runtime_environment(paths)?,
        cwd: None,
        stdio: std_handles(Stdio {
            stdin: None,
            stdout: Some(io.stdout),
            stderr: Some(io.stderr),
            ask: hex_api::Ask::default(),
        })?,
    };
    let name = job_name(runtime, record)?;
    let boxed = JobBox::prepare_vm(
        record,
        bounds,
        runtime.processors,
        name.as_deref(),
        &runtime.holding,
    )?;
    // The canonical Store alone holds selected devices. This is its native process/boot receipt,
    // without another physical-device selection or holder list.
    runtime.holding.reserve(&spec.id, &[], None)?;
    let held = Held::Process(Arc::clone(&boxed));
    if runtime.holding.made(&spec.id, held.clone())? {
        boxed.not_submitted()?;
        return Err(uncertain(spec));
    }
    let result = (|| {
        if Instant::now() >= deadline {
            boxed.not_submitted()?;
            return Err(uncertain(spec));
        }
        boxed.bind_vm(name.as_deref())?;
        // The existing passive keeper holds this same job before any VM/helper exists.
        runtime.holding.keep(&spec.id, &held)?;
        let sys::Started {
            process,
            thread,
            pid,
        } = boxed.create_controller(&mut spawn)?;
        let started = sys::created(&process)
            .map_err(|error| Refusal::os("reading the VM controller's birth", error))?;
        if pid == 0 || started == 0 {
            return Err(uncertain(spec));
        }
        let identity = Identity {
            pid,
            started,
            vsock,
        };
        let receipt = Receipt {
            identity,
            process: Mutex::new(BoxProc::new(process, pid, boxed.shared())),
        };
        boxed.managed.set(receipt).map_err(|_| uncertain(spec))?;
        let identity = &boxed.managed.get().ok_or_else(|| uncertain(spec))?.identity;
        publish(identity)?;
        if Instant::now() >= deadline {
            return Err(uncertain(spec));
        }
        sys::resume(&thread)
            .map_err(|error| Refusal::os("resuming the recorded VM controller", error))?;
        if !runtime.holding.ready(&spec.id)? {
            return Err(uncertain(spec));
        }
        Ok(shards_api::managed_receipt(Arc::clone(&boxed)))
    })();
    if result.is_err() {
        // The original slot/job/keeper retain cleanup authority and exact claims on every failure.
        // No error is promoted to scope absence; the caller's same Creating row remains durable.
        let _ = runtime.holding.uncertain(&spec.id);
    }
    result
}

pub(super) fn attach(
    runtime: &WinRuntime,
    record: &hex_core::record::BoxRecord,
    identity: Option<&Identity>,
) -> Result<ManagedReceipt, Refusal> {
    let spec = &record.spec;
    record.native_scope.ok_or_else(|| uncertain(spec))?;
    if identity.is_none()
        && !matches!(
            record.phase,
            hex_core::record::Phase::Creating | hex_core::record::Phase::Deleting
        )
    {
        return Err(uncertain(spec));
    }
    let boxed = match existing(runtime, record)? {
        Some(boxed) => boxed,
        None => cold(runtime, record, identity)?,
    };
    if let Some(expected) = identity {
        match boxed.managed.get() {
            Some(receipt) if receipt.identity == *expected => {}
            None if boxed.recovered() && same_init(boxed.original_init(), expected) => {}
            _ => return Err(uncertain(spec)),
        }
    }
    // A recovered scope without its controller has no usable identity; its API view can only clean
    // up the proven same job. The canonical Store remains the sole device/resource holder.
    Ok(shards_api::managed_receipt(boxed))
}

fn same_init(init: Option<hex_core::record::ProcId>, identity: &Identity) -> bool {
    init.is_some_and(|init| init.pid == identity.pid && init.start_ticks == identity.started)
}

fn existing(
    runtime: &WinRuntime,
    record: &hex_core::record::BoxRecord,
) -> Result<Option<Arc<JobBox>>, Refusal> {
    let commitment = record.native_commitment()?;
    let boxes = runtime.boxes()?;
    let Ok(at) = super::slot_index(&boxes, &record.spec.id) else {
        return Ok(None);
    };
    let (_, slot) = boxes.get(at).ok_or_else(|| uncertain(&record.spec))?;
    match slot.physical() {
        Some(Held::Process(boxed)) if boxed.matches_vm(record, commitment) => {
            Ok(Some(Arc::clone(boxed)))
        }
        _ => Err(uncertain(&record.spec)),
    }
}

pub(super) fn usage(
    runtime: &WinRuntime,
    record: &hex_core::record::BoxRecord,
) -> Result<hex_api::Usage, Refusal> {
    let boxed = match existing(runtime, record)? {
        Some(boxed) => boxed,
        None => cold(runtime, record, None)?,
    };
    boxed.usage()
}

fn controller(
    boxed: &JobBox,
    expected: &Identity,
    vsock: std::path::PathBuf,
) -> Result<Receipt, ProcessFailure> {
    use windows_sys::Win32::System::Threading::{
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };
    let process = super::oci::sys::open_process(
        expected.pid,
        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
    )
    .map_err(|error| io_failure(ProcessOperation::OpenProcess, error))?;
    let started = sys::created(&process)
        .map_err(|error| io_failure(ProcessOperation::GetProcessTimes, error))?;
    if started != expected.started {
        return Err(ProcessFailure::DifferentStart { observed: started });
    }
    let job = boxed
        .job()
        .map_err(|_| ProcessFailure::InvalidStartRecord)?;
    if !super::oci::sys::in_job(&process, job)
        .map_err(|error| io_failure(ProcessOperation::QueryScope, error))?
    {
        return Err(ProcessFailure::DifferentScope);
    }
    Ok(Receipt {
        identity: Identity {
            pid: expected.pid,
            started,
            vsock,
        },
        process: Mutex::new(BoxProc::new(process, expected.pid, boxed.shared())),
    })
}

fn cold(
    runtime: &WinRuntime,
    record: &hex_core::record::BoxRecord,
    identity: Option<&Identity>,
) -> Result<Arc<JobBox>, Refusal> {
    let spec = &record.spec;
    if let Some(identity) = identity
        && !same_init(record.init, identity)
    {
        return Err(uncertain(spec));
    }
    let name = job_name(runtime, record)?.ok_or_else(|| uncertain(spec))?;
    let Some(keeping) = runtime.holding.keeper.as_ref() else {
        return Err(uncertain(spec));
    };
    let path = super::keeper::vm::path(&keeping.state, &spec.id)?;
    let vsock = identity
        .map(|identity| copy_path(&identity.vsock))
        .transpose()?;
    // Both typed native owners are allocated before opening any old native scope. A retained
    // filename/PID/handle alone is never enough: all commitments and kernel object identity agree.
    let boxed = JobBox::prepare_vm(
        record,
        host_bounds(spec)?,
        runtime.processors,
        Some(&name),
        &runtime.holding,
    )?;
    let held = super::keeper::vm::read(&path)?.ok_or_else(|| uncertain(spec))?;
    if !held.commits(boxed.vm_commitment()) {
        return Err(uncertain(spec));
    }
    let _keeper = keeping
        .keeper
        .lock()
        .map_err(|_| pending("native keeper lock is poisoned"))?;
    let process = sys::open_process(held.pid)
        .map_err(|error| Refusal::os("opening the committed VM keeper", error))?
        .ok_or_else(|| uncertain(spec))?;
    if sys::created(&process)
        .map_err(|error| Refusal::os("checking the committed VM keeper birth", error))?
        != held.started
        || sys::wait(&process, 0)
            .map_err(|error| Refusal::os("observing the committed VM keeper lifetime", error))?
    {
        return Err(uncertain(spec));
    }
    let job = sys::Job::open(&name)
        .map_err(|error| Refusal::os("opening the committed native VM scope", error))?
        .ok_or_else(|| uncertain(spec))?;
    if !sys::same_in(&process, held.value, job.borrowed())
        .map_err(|error| Refusal::os("checking the committed native VM kernel object", error))?
    {
        return Err(uncertain(spec));
    }
    boxed.adopt_vm(job)?;
    runtime.holding.reserve(&spec.id, &[], None)?;
    let retained = Held::Process(Arc::clone(&boxed));
    let cancelled = runtime.holding.made(&spec.id, retained)?;
    runtime.holding.keeper_receipt(
        &spec.id,
        super::KeeperHold::Held(super::KeeperReceipt {
            pid: held.pid,
            created: held.started,
            value: held.value,
        }),
    )?;
    if let (Some(identity), Some(vsock)) = (identity, vsock) {
        match controller(&boxed, identity, vsock) {
            Ok(receipt) => boxed.managed.set(receipt).map_err(|_| uncertain(spec))?,
            Err(error) => boxed
                .observation_failure
                .set(error)
                .map_err(|_| uncertain(spec))?,
        }
    }
    if cancelled || !runtime.holding.ready(&spec.id)? {
        return Err(uncertain(spec));
    }
    Ok(boxed)
}

pub(super) fn finalize(
    runtime: &WinRuntime,
    record: &hex_core::record::BoxRecord,
    identity: Option<&Identity>,
) -> Result<(), Refusal> {
    let spec = &record.spec;
    record.native_scope.ok_or_else(|| uncertain(spec))?;
    if record.phase != hex_core::record::Phase::Deleting {
        return Err(uncertain(spec));
    }
    let boxed = match existing(runtime, record)? {
        Some(boxed) => boxed,
        None if finalized_keeper_is_gone(runtime, record)? => return Ok(()),
        None => cold(runtime, record, identity)?,
    };
    match (identity, boxed.managed.get()) {
        (Some(expected), Some(receipt)) if receipt.identity == *expected => {}
        (Some(expected), None)
            if boxed.recovered() && same_init(boxed.original_init(), expected) => {}
        (None, None) => {}
        _ => return Err(uncertain(spec)),
    }
    // This query retains keeper/slot ownership. Only the durable Released caller may discharge it.
    if !matches!(
        boxed.terminal().map_err(|_| uncertain(spec))?,
        Some(ManagedEnd {
            scope: TerminalScope::Released,
            ..
        })
    ) {
        return Err(uncertain(spec));
    }
    boxed.release_scope()
}

/// The caller already published the exact backend Released receipt. After a crash this path only
/// disposes this scope's private metadata; it never opens or terminates a possibly reused job name.
fn finalized_keeper_is_gone(
    runtime: &WinRuntime,
    record: &hex_core::record::BoxRecord,
) -> Result<bool, Refusal> {
    let Some(keeping) = runtime.holding.keeper.as_ref() else {
        return Err(uncertain(&record.spec));
    };
    let scope = record.native_scope.ok_or_else(|| uncertain(&record.spec))?;
    let commitment = record.native_commitment()?;
    let path = super::keeper::vm::path(&keeping.state, &record.spec.id)?;
    let _keeper = keeping
        .keeper
        .lock()
        .map_err(|_| pending("native keeper lock is poisoned"))?;
    let Some(held) = super::keeper::vm::read(&path)? else {
        // Guarded finalize may have completed just before the canonical Store remove was interrupted.
        return Ok(true);
    };
    if !held.commits(Some((scope, commitment))) {
        return Err(uncertain(&record.spec));
    }
    if let Some(process) = sys::open_process(held.pid)
        .map_err(|error| Refusal::os("observing the finalized VM keeper", error))?
        && sys::created(&process)
            .map_err(|error| Refusal::os("checking the finalized VM keeper birth", error))?
            == held.started
        && !sys::wait(&process, 0)
            .map_err(|error| Refusal::os("observing the finalized VM keeper end", error))?
    {
        return Ok(false);
    }
    std::fs::remove_file(path)
        .map_err(|error| Refusal::os("removing the finalized VM keeper receipt", error))?;
    Ok(true)
}

fn cause(identity: Option<&Identity>, failure: ProcessFailure) -> shards_api::Refusal {
    match identity {
        Some(identity) => shards_api::Refusal::Process {
            pid: identity.pid,
            expected_started: identity.started,
            failure,
        },
        None => shards_api::Refusal::Scope(failure),
    }
}
fn io_cause(
    identity: Option<&Identity>,
    operation: ProcessOperation,
    error: std::io::Error,
) -> shards_api::Refusal {
    cause(identity, io_failure(operation, error))
}
fn io_failure(operation: ProcessOperation, error: std::io::Error) -> ProcessFailure {
    match error.raw_os_error() {
        Some(code) => ProcessFailure::Os { operation, code },
        None => ProcessFailure::UnrepresentedIo {
            operation,
            kind: error.kind(),
        },
    }
}
fn observed(identity: Option<&Identity>, error: ObserveError) -> shards_api::Refusal {
    match error {
        ObserveError::DeadlineOverflow => {
            match super::copy_text("the finite native wait deadline is not representable") {
                Ok(why) => shards_api::Refusal::Bounds {
                    what: "managed native wait",
                    why,
                },
                Err(_) => shards_api::Refusal::Allocation {
                    what: "managed wait refusal",
                },
            }
        }
        ObserveError::UnexpectedWait(returned) => {
            cause(identity, ProcessFailure::UnexpectedWait { returned })
        }
        ObserveError::Unpublished => shards_api::Refusal::Launch(shards_api::LaunchError::Observer),
        ObserveError::State(_) => cause(
            identity,
            ProcessFailure::Poisoned {
                what: "native job signal counts",
            },
        ),
        ObserveError::Os { operation, source } => io_cause(
            identity,
            match operation {
                Operation::Wait => ProcessOperation::WaitForExit,
                Operation::ExitCode => ProcessOperation::GetExitCode,
                Operation::StopScope => ProcessOperation::StopScope,
                Operation::QueryScope => ProcessOperation::QueryScope,
            },
            source,
        ),
    }
}
fn status(exit: Exit) -> ManagedStatus {
    match exit {
        Exit::Code(code) => ManagedStatus::Code(code),
        Exit::Signal(signal) => ManagedStatus::Signal(signal),
        Exit::Unknown => ManagedStatus::Unknown,
    }
}

fn scope_proof(boxed: &JobBox) -> Result<(), shards_api::Refusal> {
    if let Some(failure @ ProcessFailure::DifferentScope) = boxed.observation_failure.get() {
        // A positive contradictory membership fact cannot become release merely because the
        // retained job is empty: this observed controller may still own out-of-scope helpers.
        return Err(shards_api::Refusal::Scope(failure.clone()));
    }
    Ok(())
}

impl ManagedProcess for JobBox {
    fn identity(&self) -> Result<&Identity, shards_api::Refusal> {
        if let Some(failure) = self.observation_failure.get() {
            return Err(shards_api::Refusal::Scope(failure.clone()));
        }
        self.managed
            .get()
            .map(|receipt| &receipt.identity)
            .ok_or(shards_api::Refusal::Launch(
                shards_api::LaunchError::Observer,
            ))
    }
    fn stop(&self) -> Result<(), shards_api::Refusal> {
        if !self.is_vm() {
            return Err(shards_api::Refusal::Launch(
                shards_api::LaunchError::NativeRefused,
            ));
        }
        self.stop_managed()
            .map_err(|error| observed(self.managed.get().map(|receipt| &receipt.identity), error))
    }
    fn terminal(&self) -> Result<Option<ManagedEnd>, shards_api::Refusal> {
        scope_proof(self)?;
        if !self.is_vm() {
            return Err(shards_api::Refusal::Launch(
                shards_api::LaunchError::NativeRefused,
            ));
        }
        let receipt = self.managed.get();
        let identity = receipt.map(|receipt| &receipt.identity);
        let exit = match receipt {
            Some(receipt) => receipt
                .process
                .lock()
                .map_err(|_| {
                    cause(
                        identity,
                        ProcessFailure::Poisoned {
                            what: "native VM observer",
                        },
                    )
                })?
                .poll_raw(0)
                .map_err(|error| observed(identity, error))?,
            None => None,
        };
        let empty = self
            .scope_population()
            .map_err(|error| observed(identity, error))?
            == 0;
        if empty {
            return Ok(Some(ManagedEnd {
                status: exit.map_or(ManagedStatus::Unknown, status),
                scope: TerminalScope::Released,
            }));
        }
        Ok(exit.map(|exit| ManagedEnd {
            status: status(exit),
            scope: TerminalScope::Process,
        }))
    }
    fn wait_for(&self, patience: Duration) -> Result<Option<ManagedEnd>, shards_api::Refusal> {
        scope_proof(self)?;
        if !self.is_vm() {
            return Err(shards_api::Refusal::Launch(
                shards_api::LaunchError::NativeRefused,
            ));
        }
        let began = Instant::now();
        let deadline = began.checked_add(patience);
        if patience != Duration::MAX && deadline.is_none() {
            return Err(observed(
                self.managed.get().map(|receipt| &receipt.identity),
                ObserveError::DeadlineOverflow,
            ));
        }
        let receipt = self.managed.get();
        let identity = receipt.map(|receipt| &receipt.identity);
        let exit = match receipt {
            Some(receipt) => match receipt
                .process
                .lock()
                .map_err(|_| {
                    cause(
                        identity,
                        ProcessFailure::Poisoned {
                            what: "native VM observer",
                        },
                    )
                })?
                .wait_raw(patience)
                .map_err(|error| observed(identity, error))?
            {
                Some(exit) => Some(exit),
                None => return Ok(None),
            },
            None => None,
        };
        loop {
            let member = self.member().map_err(|error| observed(identity, error))?;
            let process = match member {
                Member::Empty => {
                    return Ok(Some(ManagedEnd {
                        status: exit.map_or(ManagedStatus::Unknown, status),
                        scope: TerminalScope::Released,
                    }));
                }
                Member::Process(process) => process,
            };
            let left = deadline.map_or(patience, |deadline| {
                deadline.saturating_duration_since(Instant::now())
            });
            if left.is_zero() {
                return Ok(None);
            }
            let timeout = super::proc::millis(left);
            let ended = sys::wait(&process, timeout)
                .map_err(|error| io_cause(identity, ProcessOperation::WaitForExit, error))?;
            drop(process);
            if !ended {
                if patience == Duration::MAX {
                    return Err(cause(
                        identity,
                        ProcessFailure::UnexpectedWait {
                            returned: windows_sys::Win32::Foundation::WAIT_TIMEOUT,
                        },
                    ));
                }
                return Ok(None);
            }
            // A kernel exit event, not a timer probe, is followed by a fresh same-job membership
            // observation. Missing/recycled snapshot identities refuse above without another picker.
        }
    }
}

#[cfg(test)]
mod runtime_environment_tests {
    use super::*;

    /// AC-T7.4.1: native and probe receive exactly the same explicitly configured runtime pair.
    #[test]
    fn runtime_environment_contains_only_the_explicit_provider_paths() {
        let paths = shards_api::RuntimePaths::new(
            std::path::PathBuf::from("C:\\cache\\雪"),
            std::path::PathBuf::from("C:\\home"),
        )
        .unwrap();
        let expected: Vec<u16> = "SHARDS_CACHE=C:\\cache\\雪\0SHARDS_HOME=C:\\home\0\0"
            .encode_utf16()
            .collect();
        assert_eq!(runtime_environment(&paths).unwrap(), expected);
    }

    /// AC-T7.4.1: native OS paths remain lossless; no Unicode replacement or display conversion.
    #[test]
    fn runtime_environment_preserves_native_unpaired_utf16() {
        use std::os::windows::ffi::OsStringExt;
        let mut wide: Vec<u16> = "C:\\cache\\".encode_utf16().collect();
        wide.push(0xd800);
        let cache = std::ffi::OsString::from_wide(&wide);
        let paths = shards_api::RuntimePaths::new(cache.into(), "C:\\home".into()).unwrap();
        let mut expected: Vec<u16> = "SHARDS_CACHE=".encode_utf16().collect();
        expected.extend(wide);
        expected.extend("\0SHARDS_HOME=C:\\home\0\0".encode_utf16());
        assert_eq!(runtime_environment(&paths).unwrap(), expected);
    }
}
