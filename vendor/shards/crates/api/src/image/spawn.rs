//! The run host's process (`shards api-run`, D231): started with its three standard streams
//! piped to this process, waited for while left unreaped, killed while it is unreaped, and
//! reaped, as the session's watcher and stop need it.

use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard, PoisonError};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A run host started, and the ends of its standard streams this process holds.
#[derive(Debug)]
pub(super) struct Started {
    pub(super) process: Process,
    pub(super) stdin: io::PipeWriter,
    pub(super) stdout: io::PipeReader,
    pub(super) stderr: io::PipeReader,
}

/// Starts `program` with `args`, its environment this process's with `env` over it, and its
/// standard streams pipes to this process.
pub(super) fn start(program: &Path, args: &[&str], env: &[(OsString, OsString)]) -> io::Result<Started> {
    let mut child = Command::new(program)
        .args(args)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pipes = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    let process = Process::of(child)?;
    let (Some(stdin), Some(stdout), Some(stderr)) = pipes else {
        process.kill();
        let _ = process.reap();
        return Err(io::ErrorKind::BrokenPipe.into());
    };
    Ok(Started {
        process,
        stdin: owned(stdin).into(),
        stdout: owned(stdout).into(),
        stderr: owned(stderr).into(),
    })
}

#[cfg(unix)]
fn owned(stream: impl Into<std::os::fd::OwnedFd>) -> std::os::fd::OwnedFd {
    stream.into()
}

#[cfg(windows)]
fn owned(stream: impl Into<std::os::windows::io::OwnedHandle>) -> std::os::windows::io::OwnedHandle {
    stream.into()
}

/// The run host's process: held to reap it, and its pid (Unix) or a handle of its own
/// (Windows) to wait for its exit without the lock a kill takes.
#[derive(Debug)]
pub(super) struct Process {
    child: Mutex<Child>,
    #[cfg(unix)]
    pid: u32,
    #[cfg(windows)]
    handle: std::os::windows::io::OwnedHandle,
}

impl Process {
    fn of(child: Child) -> io::Result<Process> {
        #[cfg(unix)]
        let pid = child.id();
        #[cfg(windows)]
        let handle = {
            use std::os::windows::io::AsHandle as _;
            child.as_handle().try_clone_to_owned()?
        };
        Ok(Process {
            child: Mutex::new(child),
            #[cfg(unix)]
            pid,
            #[cfg(windows)]
            handle,
        })
    }

    /// Waits for it to exit, leaving it unreaped.
    pub(super) fn awaited(&self) -> io::Result<()> {
        #[cfg(unix)]
        loop {
            // SAFETY: a siginfo_t, plain data, for waitid(2) to fill.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // SAFETY: waitid(2) of our own child into `info`; WNOWAIT leaves it unreaped.
            let r = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.pid,
                    &raw mut info,
                    libc::WEXITED | libc::WNOWAIT,
                )
            };
            if r == 0 {
                return Ok(());
            }
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle as _;
            use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
            use windows_sys::Win32::System::Threading::{INFINITE, WaitForSingleObject};
            // SAFETY: a process handle this value owns.
            if unsafe { WaitForSingleObject(self.handle.as_raw_handle(), INFINITE) } == WAIT_OBJECT_0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }
    }

    /// Kills it at once. Only while it is unreaped: the caller holds what says so.
    pub(super) fn kill(&self) {
        #[cfg(unix)]
        if let Ok(pid) = libc::pid_t::try_from(self.pid) {
            // SAFETY: kill(2) of our own child, unreaped while the caller says so.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
        #[cfg(windows)]
        {
            let _ = lock(&self.child).kill();
        }
    }

    /// Reaps it, once it has exited.
    pub(super) fn reap(&self) -> io::Result<()> {
        lock(&self.child).wait().map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::path::PathBuf;

    use super::*;

    /// The variable under which the leak check runs, in a test process of its own.
    #[cfg(any(target_os = "macos", windows))]
    const ALONE: &str = "SHARDS_API_SPAWN_ALONE";

    /// A program that reads its stdin to its end, saying each line with its argument in it, by its
    /// path: findstr on Windows, grep on macOS.
    #[cfg(any(target_os = "macos", windows))]
    fn reader() -> PathBuf {
        if cfg!(windows) {
            let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
            PathBuf::from(root).join("System32").join("findstr.exe")
        } else {
            PathBuf::from("/usr/bin/grep")
        }
    }

    /// Leaves `write` inheritable: as a descriptor another thread has made and not yet marked
    /// close-on-exec is on macOS, or a handle another part of this process made inheritable for a
    /// child of its own is on Windows.
    #[cfg(any(target_os = "macos", windows))]
    fn inheritable(write: &io::PipeWriter) {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd as _;
            // SAFETY: fcntl(2) of a descriptor `write` owns, clearing its close-on-exec flag.
            assert_eq!(unsafe { libc::fcntl(write.as_raw_fd(), libc::F_SETFD, 0) }, 0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle as _;
            use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
            // SAFETY: a pipe handle `write` owns, alive for the call.
            let ok = unsafe {
                SetHandleInformation(write.as_raw_handle(), HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT)
            };
            assert_ne!(ok, 0, "{}", io::Error::last_os_error());
        }
    }

    /// Whether every write end of `read`'s pipe is gone, every process's: a read that would wait
    /// finds its end (Unix), or a look at it finds it broken (Windows), at once.
    #[cfg(any(target_os = "macos", windows))]
    fn writers_gone(read: &io::PipeReader) -> bool {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd as _;
            // SAFETY: fcntl(2) of a descriptor `read` owns, making its reads not wait.
            assert_eq!(
                unsafe { libc::fcntl(read.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) },
                0
            );
            match (&*read).read(&mut [0u8; 1]) {
                Ok(0) => true,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => false,
                other => panic!("a read of a pipe nothing was written to: {other:?}"),
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle as _;
            use windows_sys::Win32::Foundation::{ERROR_BROKEN_PIPE, GetLastError};
            use windows_sys::Win32::System::Pipes::PeekNamedPipe;
            let mut ready = 0u32;
            // SAFETY: a pipe handle `read` owns; only the count is asked for, into `ready`.
            let ok = unsafe {
                PeekNamedPipe(
                    read.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &raw mut ready,
                    std::ptr::null_mut(),
                )
            };
            // SAFETY: reads this thread's last error, which the failed call above set.
            ok == 0 && unsafe { GetLastError() } == ERROR_BROKEN_PIPE
        }
    }

    /// D231's run host holds its three standard streams alone, as every program its orchestrator
    /// starts does: do leave a pipe's write end inheritable in this process, then start a run host
    /// and let go of that end. Expect: the pipe's read end finds every writer gone, the run host
    /// holding none of it. It runs in a test process of its own, so no other test's spawn holds that
    /// end for its moment: one in flight beside it holds every descriptor of this process until its
    /// exec closes them, and std's on Windows inherits it. Linux makes every descriptor
    /// close-on-exec as it makes it (pipe2, SOCK_CLOEXEC, accept4, O_CLOEXEC), so none waits
    /// unmarked there.
    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn a_run_host_holds_no_end_it_was_not_given() {
        if std::env::var_os(ALONE).is_some() {
            let (read, write) = io::pipe().unwrap();
            inheritable(&write);
            let started = start(&reader(), &["api-run"], &[]).unwrap();
            drop(write);
            let gone = writers_gone(&read);
            drop(started.stdin);
            started.process.awaited().unwrap();
            started.process.reap().unwrap();
            assert!(
                gone,
                "the run host holds the write end of a pipe it was never given"
            );
            return;
        }
        let alone = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "image::spawn::tests::a_run_host_holds_no_end_it_was_not_given",
                "--test-threads=1",
            ])
            .env(ALONE, "1")
            .output()
            .unwrap();
        assert!(
            alone.status.success(),
            "{}{}",
            String::from_utf8_lossy(&alone.stdout),
            String::from_utf8_lossy(&alone.stderr)
        );
    }

    /// Do start a run host with a variable over this process's environment, write it lines and end
    /// its stdin. Expect: its stdout says the variable and the line it was asked for, its stderr
    /// nothing, and it ends.
    #[test]
    fn a_run_host_takes_its_environment_and_speaks_on_its_streams() {
        let env = [("SHARDS_API_SPAWN_TEST".into(), "said".into())];
        // Each says the variable, then reads its stdin to its end, saying the line asked for.
        let started = if cfg!(windows) {
            let cmd = PathBuf::from("cmd");
            let args = [
                "/d",
                "/q",
                "/c",
                "echo",
                "%SHARDS_API_SPAWN_TEST%&",
                "findstr",
                "api-run",
            ];
            start(&cmd, &args, &env)
        } else {
            let sh = PathBuf::from("sh");
            start(
                &sh,
                &["-c", "printf '%s\\n' \"$SHARDS_API_SPAWN_TEST\"; grep api-run"],
                &env,
            )
        }
        .unwrap();
        let Started {
            process,
            mut stdin,
            mut stdout,
            mut stderr,
        } = started;
        stdin.write_all(b"other\napi-run\n").unwrap();
        drop(stdin);
        let mut said = String::new();
        stdout.read_to_string(&mut said).unwrap();
        let mut why = String::new();
        stderr.read_to_string(&mut why).unwrap();
        process.awaited().unwrap();
        process.reap().unwrap();
        let lines: Vec<&str> = said.lines().map(str::trim_end).collect();
        assert_eq!(lines, ["said", "api-run"], "{said:?}");
        assert_eq!(why, "");
    }
}
