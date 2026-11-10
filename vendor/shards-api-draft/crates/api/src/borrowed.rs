//! HEX-RT-17: one connection to a verified, already-listening guest, without acquiring its lifecycle.

#![allow(unsafe_code)]

use std::time::Duration;

use crate::client::{Attempt, handshake};
use crate::{Identity, Refusal, Stream};

#[cfg(unix)]
use unix::Held;
#[cfg(windows)]
use windows::Held;

/// Opens one port of an already-listening VM after verifying its process identity, then verifies it again.
/// This borrows no `Guest` and owns only the returned connection: dropping it cannot stop the VM. It makes
/// one handshake attempt, with no retry or watcher thread. Positive representable `patience` bounds that
/// whole handshake; zero (or a duration beyond the clock's range) leaves the one answer to the VM's own
/// existing vsock request bound, as [`crate::Guest::connect`] does for its first zero-patience attempt.
pub fn connect_borrowed(
    identity: &Identity,
    port: u32,
    patience: Duration,
) -> Result<Box<dyn Stream>, Refusal> {
    let gone = |why| Refusal::Gone {
        pid: identity.pid,
        why,
    };
    let held = Held::find(identity).map_err(gone)?;
    let wait = if patience.is_zero() { None } else { Some(patience) };
    let mut cost = Duration::ZERO;
    let attempted = handshake(&identity.vsock, port, wait, &mut cost);
    held.check(identity).map_err(gone)?;
    attempted
        .map(|stream| Box::new(stream) as Box<dyn Stream>)
        .map_err(|why| {
            let why = match why {
                Attempt::Again(why) | Attempt::Refused(why) => why,
            };
            Refusal::Channel { port, why }
        })
}

#[cfg(unix)]
mod unix {
    use crate::Identity;

    #[derive(Debug)]
    pub(super) struct Held(crate::taken::Held);

    impl Held {
        pub(super) fn find(identity: &Identity) -> Result<Self, String> {
            crate::taken::Held::find(identity.pid, identity.started).map(Self)
        }

        pub(super) fn check(&self, identity: &Identity) -> Result<(), String> {
            let now = crate::taken::started(identity.pid).map_err(|e| format!("its start: {e}"))?;
            if now != identity.started {
                return Err(format!(
                    "pid {} started at {now}, not {}",
                    identity.pid, identity.started
                ));
            }
            match self
                .0
                .try_exited()
                .map_err(|e| format!("checking its exit: {e}"))?
            {
                true => Err("its VM process has ended".into()),
                false => Ok(()),
            }
        }
    }
}

#[cfg(windows)]
mod windows {
    use crate::Identity;
    use std::io;
    use windows_sys::Win32::Foundation::{
        CloseHandle, FILETIME, HANDLE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        WaitForSingleObject,
    };

    /// An owned, non-inheritable query/synchronize handle. It cannot terminate or modify the process.
    #[derive(Debug)]
    pub(super) struct Held(HANDLE);

    impl Held {
        pub(super) fn open(pid: u32) -> io::Result<Self> {
            // SAFETY: OpenProcess takes a numeric pid. Request only query/synchronize, never termination or duplication.
            let handle =
                unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(handle))
        }

        pub(super) fn started(&self) -> io::Result<u64> {
            let zero = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
            // SAFETY: this owns a live query handle; all four FILETIME outputs are writable for the call.
            if unsafe {
                GetProcessTimes(
                    self.0,
                    &raw mut created,
                    &raw mut exited,
                    &raw mut kernel,
                    &raw mut user,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
        }

        fn exited(&self) -> io::Result<bool> {
            // SAFETY: this holds SYNCHRONIZE access to the process object; zero observes its state without waiting.
            match unsafe { WaitForSingleObject(self.0, 0) } {
                WAIT_OBJECT_0 => Ok(true),
                WAIT_TIMEOUT => Ok(false),
                WAIT_FAILED => Err(io::Error::last_os_error()),
                other => Err(io::Error::other(format!(
                    "unexpected process wait result {other}"
                ))),
            }
        }

        pub(super) fn find(identity: &Identity) -> Result<Self, String> {
            let held = Self::open(identity.pid).map_err(|e| format!("opening pid {}: {e}", identity.pid))?;
            held.check(identity)?;
            Ok(held)
        }

        pub(super) fn check(&self, identity: &Identity) -> Result<(), String> {
            let now = self.started().map_err(|e| format!("its creation time: {e}"))?;
            if now != identity.started {
                return Err(format!(
                    "pid {} started at {now}, not {}",
                    identity.pid, identity.started
                ));
            }
            match self.exited().map_err(|e| format!("checking its exit: {e}"))? {
                true => Err("its VM process has ended".into()),
                false => Ok(()),
            }
        }
    }

    impl Drop for Held {
        fn drop(&mut self) {
            // SAFETY: this uniquely owns the real process handle returned by OpenProcess, closed exactly once.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(test)]
mod tests;
