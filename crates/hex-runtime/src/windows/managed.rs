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
            % denominator
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

fn job_name(runtime: &WinRuntime, spec: &BoxSpec) -> Result<Option<String>, Refusal> {
    let Some(node) = runtime.node.as_ref() else {
        return Ok(None);
    };
    let prefix = "Local\\hex-";
    let size = prefix
        .len()
        .checked_add(node.len())
        .and_then(|size| size.checked_add(1))
        .and_then(|size| size.checked_add(spec.id.as_str().len()))
        .ok_or_else(|| super::allocation(usize::MAX))?;
    let mut name = String::new();
    name.try_reserve_exact(size)
        .map_err(|_| super::allocation(size))?;
    name.push_str(prefix);
    name.push_str(node);
    name.push('-');
    name.push_str(spec.id.as_str());
    Ok(Some(name))
}

pub(super) fn launch(
    runtime: &WinRuntime,
    spec: &BoxSpec,
    plan: &shards_api::PreparedBoot,
    io: shards_api::LaunchIo,
    publish: &mut dyn FnMut(&Identity) -> Result<(), Refusal>,
) -> Result<ManagedReceipt, Refusal> {
    let bounds = host_bounds(spec)?;
    if !matches!(spec.class.class(), Class::Shards | Class::Oci) {
        return Err(uncertain(spec));
    }
    // Prepare every argument, path and stream before a native controller can exist.
    let vsock = copy_path(plan.vsock())?;
    let mut spawn = sys::Spawn {
        command_line: command_line(plan)?,
        environment: Vec::new(),
        cwd: None,
        stdio: std_handles(Stdio {
            stdin: None,
            stdout: Some(io.stdout),
            stderr: Some(io.stderr),
            ask: hex_api::Ask::default(),
        })?,
    };
    let name = job_name(runtime, spec)?;
    let boxed = JobBox::prepare_vm(
        spec,
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
    spec: &BoxSpec,
    identity: Option<&Identity>,
) -> Result<ManagedReceipt, Refusal> {
    let boxed = {
        let boxes = runtime.boxes()?;
        let at = super::slot_index(&boxes, &spec.id).map_err(|_| uncertain(spec))?;
        let (_, slot) = boxes.get(at).ok_or_else(|| uncertain(spec))?;
        match slot.physical() {
            Some(Held::Process(boxed)) if boxed.matches_vm(spec) => Arc::clone(boxed),
            _ => return Err(uncertain(spec)),
        }
    };
    if let Some(expected) = identity {
        let receipt = boxed.managed.get().ok_or_else(|| uncertain(spec))?;
        if receipt.identity.pid != expected.pid
            || receipt.identity.started != expected.started
            || receipt.identity.vsock != expected.vsock
        {
            return Err(uncertain(spec));
        }
    }
    // None is cleanup-only. Across a node crash, a bare textual job name is not sufficient
    // ownership proof; existing keeper receipt adoption is required before this can succeed.
    Ok(shards_api::managed_receipt(boxed))
}

pub(super) fn finalize(
    runtime: &WinRuntime,
    spec: &BoxSpec,
    identity: Option<&Identity>,
) -> Result<(), Refusal> {
    let boxed = {
        let boxes = runtime.boxes()?;
        let at = super::slot_index(&boxes, &spec.id).map_err(|_| uncertain(spec))?;
        let (_, slot) = boxes.get(at).ok_or_else(|| uncertain(spec))?;
        match slot.physical() {
            Some(Held::Process(boxed)) if boxed.matches_vm(spec) => Arc::clone(boxed),
            _ => return Err(uncertain(spec)),
        }
    };
    match (identity, boxed.managed.get()) {
        (Some(expected), Some(receipt)) if receipt.identity == *expected => {}
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
    let failure = match error.raw_os_error() {
        Some(code) => ProcessFailure::Os { operation, code },
        None => ProcessFailure::UnrepresentedIo {
            operation,
            kind: error.kind(),
        },
    };
    cause(identity, failure)
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

impl ManagedProcess for JobBox {
    fn identity(&self) -> Result<&Identity, shards_api::Refusal> {
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
