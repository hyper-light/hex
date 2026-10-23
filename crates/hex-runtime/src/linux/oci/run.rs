//! The OCI lifecycle the runtime-tools suite drives: `create`, `start`, `state`, `kill`, `delete`.
//! `create` forks the shim ([`super::init`]), runs the sync protocol (user-namespace maps, the
//! container pid, the ready signal), and writes the state; the container then blocks until `start`
//! opens the exec FIFO.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::fs::{Mode, OFlags};
use rustix::io::FdFlags;
use rustix::process::PidfdFlags;

use crate::linux::oci::init;
use crate::linux::oci::spec::{IdMapping, Spec};
use crate::linux::oci::state::{Dir, State, Status};
use crate::linux::oci::{OciError, sys};

/// The OCI runtime: containers under one state root.
pub struct Runtime {
    root: PathBuf,
}

impl Runtime {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// The default state root: `$HEX_OCI_ROOT`, else `$XDG_RUNTIME_DIR/hex-oci`, else
    /// `/run/hex-oci`.
    pub fn default_root() -> PathBuf {
        if let Some(root) = std::env::var_os("HEX_OCI_ROOT") {
            return PathBuf::from(root);
        }
        if let Some(xdg) = std::env::var_os("XDG_RUNTIME_DIR") {
            return PathBuf::from(xdg).join("hex-oci");
        }
        PathBuf::from("/run/hex-oci")
    }

    fn dir(&self, id: &str) -> Dir {
        Dir::new(&self.root, id)
    }

    /// Makes the container (OCI `create`): sets up its directory and the init blocked at the exec
    /// FIFO, in the `created` state. `bundle` holds `config.json`; `pid_file`, if given, receives
    /// the container init's pid.
    pub fn create(&self, id: &str, bundle: &Path, pid_file: Option<&Path>) -> Result<(), OciError> {
        let dir = self.dir(id);
        dir.create(id)?;
        let result = self.create_inner(id, bundle, &dir, pid_file);
        if result.is_err() {
            let _ = dir.remove();
        }
        result
    }

    fn create_inner(
        &self,
        id: &str,
        bundle: &Path,
        dir: &Dir,
        pid_file: Option<&Path>,
    ) -> Result<(), OciError> {
        let config = std::fs::read(bundle.join("config.json"))
            .map_err(|e| OciError::os("reading config.json", e))?;
        let spec = Spec::parse(&config)?;
        make_fifo(&dir.fifo())?;

        // Two pipes: the init writes the sync pipe, the runtime writes the map pipe.
        let (sync_r, sync_w) = rustix::pipe::pipe().map_err(|e| OciError::os("pipe", e))?;
        let (map_r, map_w) = rustix::pipe::pipe().map_err(|e| OciError::os("pipe", e))?;
        // The init inherits sync_w and map_r across exec; clear their close-on-exec.
        keep_across_exec(&sync_w)?;
        keep_across_exec(&map_r)?;

        let exe = std::env::current_exe().map_err(|e| OciError::os("finding the hex binary", e))?;
        let mut cmd = Command::new(exe);
        cmd.arg("__oci-init")
            .env(init::ENV_SYNC_W, sync_w.as_raw_fd().to_string())
            .env(init::ENV_MAP_R, map_r.as_raw_fd().to_string())
            .env(init::ENV_DIR, dir.path())
            .env(init::ENV_BUNDLE, bundle)
            .env(init::ENV_ID, id);
        let mut shim = cmd
            .spawn()
            .map_err(|e| OciError::os("starting the container init", e))?;
        // The runtime holds the read end of sync and the write end of map; the init holds the others.
        drop(sync_w);
        drop(map_r);

        let mut container_pid = None;
        let outcome = self.handshake(id, &spec, sync_r, map_w, &mut container_pid);
        if outcome.is_err() {
            // A create that fails leaves nothing running: the container the shim forked, if it said
            // which, and the shim itself, killed and reaped here (it detaches only on success).
            if let Some(pid) = container_pid {
                let _ = sys::kill(pid, libc::SIGKILL, false);
            }
            let _ = shim.kill();
            let _ = shim.wait();
        }
        match outcome {
            Ok(container_pid) => {
                let mut state = State::new(id, &bundle.to_string_lossy());
                state.status = Status::Created;
                state.pid = container_pid;
                state.annotations = spec.annotations.clone();
                dir.put(&state)?;
                if let Some(path) = pid_file {
                    std::fs::write(path, format!("{container_pid}\n"))
                        .map_err(|e| OciError::os("writing the pid file", e))?;
                }
                // The create-time hooks run in the runtime's namespace while the container waits at
                // the exec FIFO (OCI §Lifecycle): prestart (deprecated) then createRuntime.
                run_hooks(&spec.hooks.prestart, &state)?;
                run_hooks(&spec.hooks.create_runtime, &state)?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Reads the sync pipe: writes the user-namespace maps when asked, then returns the container
    /// pid once the init has sent both it and its ready signal. `container_pid` holds the pid as
    /// soon as it is known, so a create that fails can still end the container.
    fn handshake(
        &self,
        id: &str,
        spec: &Spec,
        sync_r: OwnedFd,
        map_w: OwnedFd,
        container_pid: &mut Option<i32>,
    ) -> Result<i32, OciError> {
        let mut reader = BufReader::new(std::fs::File::from(sync_r));
        let mut map_w = std::fs::File::from(map_w);
        read_sync(
            &mut reader,
            id,
            |pid| {
                write_id_maps(pid, &spec.linux.uid_mappings, &spec.linux.gid_mappings)?;
                map_w
                    .write_all(&[1])
                    .map_err(|e| OciError::os("acking the maps", e))
            },
            container_pid,
        )
    }

    /// Starts the container (OCI `start`): opens the exec FIFO, which the init is blocked on, then
    /// records `running`.
    pub fn start(&self, id: &str) -> Result<(), OciError> {
        let dir = self.dir(id);
        let mut state = dir.get(id)?;
        if state.status != Status::Created {
            return Err(OciError::WrongState(
                id.to_owned(),
                state.status.name(),
                "created",
            ));
        }
        let fifo = rustix::fs::open(dir.fifo(), OFlags::RDONLY, Mode::empty())
            .map_err(|e| OciError::os("opening the exec fifo", e))?;
        let mut byte = [0u8; 1];
        let _ = rustix::io::read(&fifo, &mut byte)
            .map_err(|e| OciError::os("reading the exec fifo", e))?;
        state.status = Status::Running;
        dir.put(&state)?;
        // poststart hooks run after the workload has started (OCI §Lifecycle).
        if let Some(hooks) = load_hooks(&state) {
            run_hooks(&hooks.poststart, &state)?;
        }
        Ok(())
    }

    /// The container's state (OCI `state`), with `stopped` reflected when the init is gone even if
    /// the shim has not yet recorded it.
    pub fn state(&self, id: &str) -> Result<State, OciError> {
        let mut state = self.dir(id).get(id)?;
        if matches!(state.status, Status::Created | Status::Running) && !sys::alive(state.pid) {
            state.status = Status::Stopped;
        }
        Ok(state)
    }

    /// Sends signal `sig` to the container (OCI `kill`). `all` sends it to every process (the
    /// process group); otherwise only the init.
    pub fn kill(&self, id: &str, sig: i32, all: bool) -> Result<(), OciError> {
        let state = self.dir(id).get(id)?;
        if !matches!(state.status, Status::Created | Status::Running) {
            return Err(OciError::WrongState(
                id.to_owned(),
                state.status.name(),
                "created or running",
            ));
        }
        if !sys::alive(state.pid) {
            return Err(OciError::WrongState(
                id.to_owned(),
                "stopped",
                "created or running",
            ));
        }
        sys::kill(state.pid, sig, all)
    }

    /// Removes the container (OCI `delete`). A running container is refused unless `force`, which
    /// kills it first.
    pub fn delete(&self, id: &str, force: bool) -> Result<(), OciError> {
        let dir = self.dir(id);
        let state = dir.get(id)?;
        let running =
            matches!(state.status, Status::Created | Status::Running) && sys::alive(state.pid);
        if running {
            if !force {
                return Err(OciError::WrongState(
                    id.to_owned(),
                    state.status.name(),
                    "stopped",
                ));
            }
            // The pidfd is opened before the kill, so the wait below is for this container and
            // not a later process given its pid; a container already gone has none to wait on.
            let pidfd = rustix::process::Pid::from_raw(state.pid)
                .and_then(|p| rustix::process::pidfd_open(p, PidfdFlags::empty()).ok());
            sys::kill(state.pid, 9, true)?;
            if let Some(pidfd) = pidfd
                && !wait_exited(&pidfd, DELETE_PATIENCE)?
            {
                // Its state is kept, so a later delete can finish what this one could not.
                return Err(OciError::Broken(format!(
                    "container {id} did not exit within {DELETE_PATIENCE:?} of SIGKILL"
                )));
            }
        }
        // poststop hooks run after the container is stopped, before its directory is removed.
        if let Some(hooks) = load_hooks(&state) {
            let mut stopped = state.clone();
            stopped.status = Status::Stopped;
            run_hooks(&hooks.poststop, &stopped)?;
        }
        dir.remove()
    }
}

/// How long `delete --force` waits for the killed container to exit: `hex box run`'s cleanup patience
/// (`crates/hex-cli/src/box_run.rs`, `CLEANUP_PATIENCE`). A SIGKILLed process exits once it is
/// scheduled unless it is blocked in uninterruptible I/O; past this the delete is refused, typed.
const DELETE_PATIENCE: Duration = Duration::from_secs(10);

/// Waits up to `patience` for the process behind `pidfd` to exit; a pidfd turns readable then, whether
/// or not its parent (the shim) has reaped it yet. Returns whether it exited.
fn wait_exited(pidfd: &OwnedFd, patience: Duration) -> Result<bool, OciError> {
    let deadline = Instant::now().checked_add(patience);
    loop {
        let left = match deadline {
            Some(d) => d.saturating_duration_since(Instant::now()),
            None => patience,
        };
        let ts = Timespec {
            tv_sec: i64::try_from(left.as_secs()).unwrap_or(i64::MAX),
            tv_nsec: i64::from(left.subsec_nanos()),
        };
        let mut fds = [PollFd::new(pidfd, PollFlags::IN)];
        match rustix::event::poll(&mut fds, Some(&ts)) {
            Ok(n) if n > 0 => return Ok(true),
            Ok(_) if left.is_zero() => return Ok(false),
            Ok(_) | Err(rustix::io::Errno::INTR) => {}
            Err(e) => return Err(OciError::os("poll on the container's pidfd", e)),
        }
    }
}

/// The sync protocol's reading half, apart from any pipe: `U<pid>` asks for the user-namespace maps
/// (`on_map`), `P<pid>` is the container's pid, `R` its ready signal, `E<why>` a setup failure.
/// Returns the pid once both `P` and `R` have come, in either order: the shim writes `P` after its
/// fork and the container writes `R` after its setup, two processes on one pipe, so under load the
/// container can finish first. `container_pid` holds the pid as soon as it arrives.
fn read_sync(
    reader: &mut impl BufRead,
    id: &str,
    mut on_map: impl FnMut(i32) -> Result<(), OciError>,
    container_pid: &mut Option<i32>,
) -> Result<i32, OciError> {
    let mut ready = false;
    let mut line = String::new();
    loop {
        if ready && let Some(pid) = *container_pid {
            return Ok(pid);
        }
        line.clear();
        let n = reader
            .read_line(&mut line)
            .map_err(|e| OciError::os("reading the sync pipe", e))?;
        if n == 0 {
            let what = if ready {
                "a ready signal but no container pid"
            } else {
                "no ready signal"
            };
            return Err(OciError::Broken(format!(
                "the container init for {id} closed after {what}"
            )));
        }
        let msg = line.trim_end();
        let mut chars = msg.chars();
        let tag = chars.next();
        let rest = chars.as_str();
        match tag {
            Some('U') => {
                let pid: i32 = rest
                    .parse()
                    .map_err(|_| OciError::Broken(format!("bad map request {msg:?}")))?;
                on_map(pid)?;
            }
            Some('P') => {
                *container_pid = Some(
                    rest.parse()
                        .map_err(|_| OciError::Broken(format!("bad pid {msg:?}")))?,
                );
            }
            Some('R') => ready = true,
            Some('E') => {
                return Err(OciError::Os {
                    op: "container init",
                    why: rest.to_owned(),
                });
            }
            None => {}
            _ => return Err(OciError::Broken(format!("unknown sync message {msg:?}"))),
        }
    }
}

/// The hooks from a container's bundle (`state.bundle/config.json`); `None` when the config can no
/// longer be read (the bundle is gone), so `delete`/`start` still proceed.
fn load_hooks(state: &State) -> Option<crate::linux::oci::spec::Hooks> {
    let bytes = std::fs::read(Path::new(&state.bundle).join("config.json")).ok()?;
    Spec::parse(&bytes).ok().map(|s| s.hooks)
}

/// Runs each hook in order with the container state JSON on its stdin (OCI §POSIX hooks); a hook
/// that exits non-zero, or outlives its timeout (default [`HOOK_TIMEOUT`]), is an error.
fn run_hooks(hooks: &[crate::linux::oci::spec::Hook], state: &State) -> Result<(), OciError> {
    if hooks.is_empty() {
        return Ok(());
    }
    let json = serde_json::to_vec(state)
        .map_err(|e| OciError::Broken(format!("encoding state for a hook: {e}")))?;
    for hook in hooks {
        run_one_hook(hook, &json)?;
    }
    Ok(())
}

/// Longest a hook may run when it names no timeout; a bound so a stuck hook can't wedge the runtime.
pub const HOOK_TIMEOUT: Duration = Duration::from_secs(30);

fn run_one_hook(hook: &crate::linux::oci::spec::Hook, state_json: &[u8]) -> Result<(), OciError> {
    use std::io::Write as _;
    use std::os::unix::process::CommandExt as _;
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
    let mut child = cmd
        .spawn()
        .map_err(|e| OciError::os("starting a hook", e))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(state_json);
    }
    let timeout = hook
        .timeout
        .and_then(|t| u64::try_from(t).ok())
        .map(Duration::from_secs)
        .unwrap_or(HOOK_TIMEOUT);
    match wait_timeout(&mut child, timeout)? {
        Some(status) if status.success() => Ok(()),
        Some(status) => Err(OciError::Os {
            op: "hook",
            why: format!("{:?} exited {status}", hook.path),
        }),
        None => {
            let _ = child.kill();
            Err(OciError::Os {
                op: "hook",
                why: format!("{:?} outlived its {timeout:?} timeout", hook.path),
            })
        }
    }
}

/// Waits at most `timeout` for `child`; `Ok(None)` when it is still running (a reaper thread then
/// collects it after the kill).
fn wait_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<Option<std::process::ExitStatus>, OciError> {
    let deadline = std::time::Instant::now().checked_add(timeout);
    loop {
        match child
            .try_wait()
            .map_err(|e| OciError::os("waiting for a hook", e))?
        {
            Some(status) => return Ok(Some(status)),
            None => {
                let past = deadline.is_none_or(|d| std::time::Instant::now() >= d);
                if past {
                    return Ok(None);
                }
                // park_timeout (not the disallowed thread::sleep) yields the CPU between polls.
                std::thread::park_timeout(Duration::from_millis(5));
            }
        }
    }
}

/// Makes the exec FIFO, world-writable so a container that has dropped to a non-root user (or a
/// user namespace's mapped id) can still open it to wait for `start`; `mknod`'s mode is masked by
/// the umask, so the mode is set explicitly after.
fn make_fifo(path: &Path) -> Result<(), OciError> {
    rustix::fs::mknodat(
        rustix::fs::CWD,
        path,
        rustix::fs::FileType::Fifo,
        Mode::from_bits_truncate(0o622),
        rustix::fs::makedev(0, 0),
    )
    .map_err(|e| OciError::os("making the exec fifo", e))?;
    rustix::fs::chmodat(
        rustix::fs::CWD,
        path,
        Mode::from_bits_truncate(0o622),
        rustix::fs::AtFlags::empty(),
    )
    .map_err(|e| OciError::os("setting the exec fifo mode", e))
}

/// Clears an fd's close-on-exec, so the re-exec'd init inherits it.
fn keep_across_exec(fd: &OwnedFd) -> Result<(), OciError> {
    rustix::io::fcntl_setfd(fd, FdFlags::empty())
        .map_err(|e| OciError::os("clearing close-on-exec", e))?;
    Ok(())
}

/// Writes a user namespace's uid and gid maps for process `pid` (OCI `uidMappings`/`gidMappings`).
/// `setgroups` is denied before the gid map, as the kernel requires when mapping without privilege.
fn write_id_maps(pid: i32, uids: &[IdMapping], gids: &[IdMapping]) -> Result<(), OciError> {
    let render = |ms: &[IdMapping]| {
        ms.iter()
            .map(|m| format!("{} {} {}", m.container_id, m.host_id, m.size))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let _ = std::fs::write(format!("/proc/{pid}/setgroups"), "deny");
    std::fs::write(format!("/proc/{pid}/uid_map"), render(uids))
        .map_err(|e| OciError::os("writing uid_map", e))?;
    std::fs::write(format!("/proc/{pid}/gid_map"), render(gids))
        .map_err(|e| OciError::os("writing gid_map", e))?;
    Ok(())
}

/// Reads a pid file's pid (helper for callers that passed `--pid-file`).
pub fn read_pid_file(path: &Path) -> Result<i32, OciError> {
    let mut s = String::new();
    std::fs::File::open(path)
        .and_then(|mut f| f.read_to_string(&mut s))
        .map_err(|e| OciError::os("reading the pid file", e))?;
    s.trim()
        .parse()
        .map_err(|_| OciError::Broken(format!("pid file {path:?} isn't a pid")))
}

#[cfg(test)]
mod tests {
    //! The sync protocol's reading half (AC-T7.1.4: a conforming `create`). The competitive bench
    //! found `create` failing under load with "ready before the container pid": the shim's `P` and
    //! the container's `R` come from two processes, in either order.

    use std::io::Cursor;

    use super::*;

    fn read(wire: &str) -> (Result<i32, OciError>, Option<i32>, Vec<i32>) {
        let mut maps = Vec::new();
        let mut pid = None;
        let got = read_sync(
            &mut Cursor::new(wire.as_bytes()),
            "c1",
            |p| {
                maps.push(p);
                Ok(())
            },
            &mut pid,
        );
        (got, pid, maps)
    }

    #[test]
    fn ready_before_the_pid_still_gives_the_pid() {
        let (got, pid, _) = read("R\nP4242\n");
        assert_eq!(got.unwrap(), 4242);
        assert_eq!(pid, Some(4242));
    }

    #[test]
    fn the_pid_then_ready_gives_the_pid() {
        assert_eq!(read("P17\nR\n").0.unwrap(), 17);
    }

    #[test]
    fn a_map_request_is_answered_and_the_pid_still_follows() {
        let (got, _, maps) = read("U900\nP901\nR\n");
        assert_eq!(got.unwrap(), 901);
        assert_eq!(maps, vec![900]);
    }

    #[test]
    fn a_setup_failure_is_the_error_and_keeps_the_pid_for_cleanup() {
        let (got, pid, _) = read("P55\nEpivot_root: EINVAL\n");
        assert!(
            matches!(got, Err(OciError::Os { op: "container init", ref why }) if why == "pivot_root: EINVAL")
        );
        assert_eq!(
            pid,
            Some(55),
            "a failed create must still know the container to end"
        );
    }

    #[test]
    fn closing_without_ready_is_refused_with_the_pid_kept() {
        let (got, pid, _) = read("P8\n");
        assert!(matches!(got, Err(OciError::Broken(ref why)) if why.contains("no ready signal")));
        assert_eq!(pid, Some(8));
    }

    #[test]
    fn closing_after_ready_without_a_pid_is_refused() {
        let (got, pid, _) = read("R\n");
        assert!(matches!(got, Err(OciError::Broken(ref why)) if why.contains("no container pid")));
        assert_eq!(pid, None);
    }

    /// `delete --force` waits for the killed container to exit (AC-T7.1.4); the competitive bench
    /// found it returning at once, which made its delete look 16x faster than crun's.
    fn sleeper() -> (std::process::Child, OwnedFd) {
        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = rustix::process::Pid::from_raw(i32::try_from(child.id()).unwrap()).unwrap();
        let pidfd = rustix::process::pidfd_open(pid, PidfdFlags::empty()).unwrap();
        (child, pidfd)
    }

    #[test]
    fn a_killed_process_is_seen_to_exit() {
        let (mut child, pidfd) = sleeper();
        child.kill().unwrap();
        assert!(wait_exited(&pidfd, Duration::from_secs(10)).unwrap());
        child.wait().unwrap();
    }

    #[test]
    fn a_live_process_outlasts_a_short_patience() {
        let (mut child, pidfd) = sleeper();
        assert!(!wait_exited(&pidfd, Duration::from_millis(50)).unwrap());
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn an_unknown_message_is_refused() {
        assert!(
            matches!(read("Xhello\n").0, Err(OciError::Broken(ref why)) if why.contains("unknown sync message"))
        );
    }
}
