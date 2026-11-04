//! The OCI lifecycle hooks (runtime-spec v1.3.0 config.md:548-661, docs/research/runtime/oci-hooks.md):
//! running a list of them with the container's state on each one's stdin, each within its timeout, and
//! the frames that carry a state between the runtime and the container's init, which runs the hooks the
//! spec puts in the container's namespaces (createContainer, startContainer).

use std::io::Write as _;
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::process::CommandExt as _;
use std::process::Command;
use std::time::{Duration, Instant};

use rustix::fs::OFlags;

use crate::linux::oci::OciError;
use crate::linux::oci::spec::{Hook, Hooks, MAX_CONFIG_BYTES};
use crate::linux::oci::state::State;

/// Longest a hook may run when it names no timeout; a bound so a stuck hook can't wedge the runtime.
pub const HOOK_TIMEOUT: Duration = Duration::from_secs(30);

/// The most a state handed to the init may hold: its annotations are the config's (at most
/// [`MAX_CONFIG_BYTES`]), and its other fields are an id, a path and numbers, which the second half bounds.
pub const MAX_STATE_BYTES: usize = 2 * MAX_CONFIG_BYTES;

/// The most a startContainer failure written on the exec FIFO holds, its tag byte included: `PIPE_BUF`
/// (pipe(7)), so the init's one write lands whole and `start` reads it whole without taking it.
pub const MAX_START_FAILURE: usize = 4096;

/// The exec FIFO's byte for a start taken; a startContainer failure is [`START_FAILED`] and why.
pub const START_TAKEN: u8 = b'0';
pub const START_FAILED: u8 = b'h';

impl Hooks {
    /// Whether the init stops before its root switch for the runtime: it runs prestart and createRuntime
    /// there (config.md:607) and hands the init the state its createContainer and startContainer hooks
    /// are given.
    pub fn sync_init(&self) -> bool {
        !(self.prestart.is_empty()
            && self.create_runtime.is_empty()
            && self.create_container.is_empty()
            && self.start_container.is_empty())
    }
}

/// The seconds `hooks` may take together, each its timeout or [`HOOK_TIMEOUT`]; refused, naming `kind`,
/// where a timeout isn't above zero (config.md:559-560) or the sum overflows.
pub fn budget(kind: &str, hooks: &[Hook]) -> Result<u64, OciError> {
    hooks.iter().try_fold(0_u64, |sum, hook| {
        let secs = timeout(kind, hook)?.as_secs();
        sum.checked_add(secs)
            .ok_or_else(|| OciError::Config(format!("the {kind} hooks' timeouts overflow")))
    })
}

fn timeout(kind: &str, hook: &Hook) -> Result<Duration, OciError> {
    match hook.timeout {
        None => Ok(HOOK_TIMEOUT),
        Some(t) => u64::try_from(t)
            .ok()
            .filter(|t| *t > 0)
            .map(Duration::from_secs)
            .ok_or_else(|| {
                OciError::Config(format!(
                    "a {kind} hook's timeout is {t}: it must be above zero"
                ))
            }),
    }
}

/// Runs each hook in order with `state` as JSON on its stdin (config.md:592); a hook that can't start,
/// exits non-zero or outlives its timeout fails, as `op` (the list's name).
pub fn run(op: &'static str, hooks: &[Hook], state: &State) -> Result<(), OciError> {
    if hooks.is_empty() {
        return Ok(());
    }
    let json = serde_json::to_vec(state)
        .map_err(|e| OciError::Broken(format!("encoding state for a hook: {e}")))?;
    for hook in hooks {
        run_one(op, hook, &json)?;
    }
    Ok(())
}

fn run_one(op: &'static str, hook: &Hook, state_json: &[u8]) -> Result<(), OciError> {
    let mut cmd = Command::new(&hook.path);
    if let Some(argv0) = hook.args.first() {
        cmd.arg0(argv0);
        cmd.args(hook.args.get(1..).unwrap_or_default());
    }
    cmd.env_clear();
    for entry in &hook.env {
        if let Some((k, v)) = entry.split_once('=') {
            cmd.env(k, v);
        }
    }
    cmd.stdin(std::process::Stdio::piped());
    crate::linux::sys::sweep_on_exec(&mut cmd);
    let failed = |why: String| OciError::Os { op, why };
    let mut child = cmd
        .spawn()
        .map_err(|e| failed(format!("{:?} didn't start: {e}", hook.path)))?;
    let limit = timeout(op, hook)?;
    // The state goes in as the hook takes it, never blocking: a hook that doesn't read its stdin can't hold
    // the runtime past its timeout.
    let mut stdin = child.stdin.take();
    if let Some(pipe) = &stdin {
        rustix::fs::fcntl_setfl(pipe, OFlags::NONBLOCK)
            .map_err(|e| OciError::os("making a hook's stdin non-blocking", e))?;
    }
    let mut left = state_json;
    let deadline = Instant::now().checked_add(limit);
    loop {
        if let Some(pipe) = stdin.as_mut() {
            match pipe.write(left) {
                Ok(n) => left = left.get(n..).unwrap_or_default(),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                // A hook that closed its stdin has the state it wanted.
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => left = &[],
                Err(e) => return Err(OciError::os("writing a hook's state", e)),
            }
            if left.is_empty() {
                stdin = None;
            }
        }
        match child
            .try_wait()
            .map_err(|e| OciError::os("waiting for a hook", e))?
        {
            Some(status) if status.success() => return Ok(()),
            Some(status) => return Err(failed(format!("{:?} exited {status}", hook.path))),
            None => {
                if deadline.is_none_or(|d| Instant::now() >= d) {
                    let _ = child.kill();
                    return Err(failed(format!(
                        "{:?} outlived its {limit:?} timeout",
                        hook.path
                    )));
                }
                // park_timeout (not the disallowed thread::sleep) yields the CPU between polls.
                std::thread::park_timeout(Duration::from_millis(5));
            }
        }
    }
}

/// Writes `state` to the init as a frame: its length (4 bytes, little-endian), then its JSON.
pub fn send_state(to: &mut impl std::io::Write, state: &State) -> Result<(), OciError> {
    let json = serde_json::to_vec(state)
        .map_err(|e| OciError::Broken(format!("encoding state for the init's hooks: {e}")))?;
    let len = u32::try_from(json.len())
        .ok()
        .filter(|_| json.len() <= MAX_STATE_BYTES)
        .ok_or_else(|| {
            OciError::Config(format!(
                "the container's state is {} bytes, over the {MAX_STATE_BYTES} its hooks are given",
                json.len()
            ))
        })?;
    to.write_all(&len.to_le_bytes())
        .and_then(|()| to.write_all(&json))
        .map_err(|e| OciError::os("handing the init its hooks' state", e))
}

/// Reads the frame [`send_state`] wrote. An end before it says the runtime refused the create (a
/// createRuntime hook failed, say) and closed its end.
pub fn read_state(from: BorrowedFd<'_>) -> Result<State, OciError> {
    let mut len = [0_u8; 4];
    read_exact(from, &mut len)?;
    let len = usize::try_from(u32::from_le_bytes(len))
        .ok()
        .filter(|n| *n <= MAX_STATE_BYTES)
        .ok_or_else(|| OciError::Broken("the hooks' state is over its bound".to_owned()))?;
    let mut json = vec![0_u8; len];
    read_exact(from, &mut json)?;
    serde_json::from_slice(&json)
        .map_err(|e| OciError::Broken(format!("the hooks' state isn't one: {e}")))
}

fn read_exact(from: BorrowedFd<'_>, mut buf: &mut [u8]) -> Result<(), OciError> {
    while !buf.is_empty() {
        match rustix::io::read(from, &mut *buf) {
            Ok(0) => {
                return Err(OciError::Broken(
                    "the runtime ended the create before handing the hooks' state".to_owned(),
                ));
            }
            Ok(n) => buf = std::mem::take(&mut buf).get_mut(n..).unwrap_or_default(),
            Err(rustix::io::Errno::INTR) => {}
            Err(e) => return Err(OciError::os("reading the hooks' state", e)),
        }
    }
    Ok(())
}

/// What the init writes on the exec FIFO when a startContainer hook fails: [`START_FAILED`], then why,
/// cut to [`MAX_START_FAILURE`] at a character's edge.
pub fn start_failure(e: &OciError) -> Vec<u8> {
    let why = e.to_string();
    let mut end = why.len().min(MAX_START_FAILURE.saturating_sub(1));
    while !why.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let mut out = Vec::with_capacity(end.saturating_add(1));
    out.push(START_FAILED);
    out.extend_from_slice(why.get(..end).unwrap_or_default().as_bytes());
    out
}

/// Reads what the init wrote on the exec FIFO `fifo` without taking it (`tee(2)` into a pipe of its own),
/// so another start at once still finds the byte: `None` for a start taken, why for a startContainer
/// failure.
pub fn peek_start(fifo: BorrowedFd<'_>) -> Result<Option<String>, OciError> {
    let (r, w) = rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC)
        .map_err(|e| OciError::os("pipe", e))?;
    let n = rustix::pipe::tee(
        fifo,
        w.as_fd(),
        MAX_START_FAILURE,
        rustix::pipe::SpliceFlags::NONBLOCK,
    )
    .map_err(|e| OciError::os("reading the exec fifo", e))?;
    let mut buf = vec![0_u8; n];
    read_exact(r.as_fd(), &mut buf)?;
    match buf.split_first() {
        Some((&START_TAKEN, _)) => Ok(None),
        Some((&START_FAILED, why)) => Ok(Some(String::from_utf8_lossy(why).into_owned())),
        _ => Err(OciError::Broken(format!(
            "the exec fifo holds {buf:?}, not a start's byte"
        ))),
    }
}
