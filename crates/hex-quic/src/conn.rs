//! One connection to a port on a peer, as its owner holds it (docs/design/hex-quic.md Q3): a byte stream whose
//! bytes cross between the owner's threads and the driver's through two bounded seats, each guarded by one lock.
//!
//! - **In:** the frames the peer sent, at most [`HELD_AT`] waiting for the owner; the driver holds the peer's lane
//!   once they are there (`Endpoint::hold_lane`), so QUIC's stream flow control holds the peer, and lets it go once
//!   the owner has read them all.
//! - **Out:** at most one frame waiting for the driver, which a writer waits to have gone before it places the next:
//!   QUIC's stream holds what the peer's credit lets it, and the writer waits behind it, never a queue that grows.
//!
//! Every wait ends on a fact: bytes, room, the peer's End, or the connection's end with why (CLAUDE.md §4).

use std::io;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Instant;

use hex_core::lanes::{self, Why};

use crate::driver::{Command, Commands};

/// The frames that wait for the owner before the driver holds the peer's lane: one past the one being read, so
/// a reader that keeps up never stops the lane, and one that doesn't holds at most two frames and QUIC's window.
pub const HELD_AT: usize = 2;

/// A connection's end as its owner sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum There {
    /// The peer may still send.
    Open,
    /// The peer's half ended: what was read is all.
    Ended,
}

#[derive(Debug)]
struct State {
    inbound: std::collections::VecDeque<Vec<u8>>,
    /// How far into the front frame the owner has read.
    read: usize,
    there: There,
    /// A data frame waiting for the driver.
    outbox: Option<Vec<u8>>,
    /// This side's half ended (its End sent), or its owner reset it.
    here_ended: bool,
    /// The connection is gone both ways: why.
    gone: Option<Why>,
    /// The driver holds the peer's lane for this connection: the owner tells it once it has read all.
    held: bool,
}

/// What a connection's driver and owner share.
pub(crate) struct Shared {
    pub(crate) peer: hyper_transport::PeerId,
    pub(crate) key: lanes::Conn,
    pub(crate) port: u32,
    state: Mutex<State>,
    changed: Condvar,
    commands: Commands,
    /// Told whenever what the owner may do changed: an owner that waits in an event loop of its own.
    notify: Mutex<Option<Notify>>,
}

/// What an evented owner is told when a connection's state changes.
pub type Notify = Arc<dyn Fn() + Send + Sync>;

fn gone_error(why: Why) -> io::Error {
    let kind = match why {
        Why::Closed => io::ErrorKind::ConnectionAborted,
        Why::NoPort | Why::Lanes | Why::Conns => io::ErrorKind::ConnectionRefused,
        Why::Gone | Why::Protocol | Why::Other(_) => io::ErrorKind::ConnectionReset,
    };
    io::Error::new(kind, why)
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> io::Error {
    io::Error::other("a lock of a QUIC connection was poisoned")
}

/// What of the front frame fits `buf`, and whether the owner has now read all that came while the driver held the
/// peer's lane for it; `None` while nothing has come and the peer's half is open.
fn take(state: &mut MutexGuard<'_, State>, buf: &mut [u8]) -> io::Result<Option<(usize, bool)>> {
    if let Some(why) = state.gone {
        return Err(gone_error(why));
    }
    let at = state.read;
    let Some(front) = state.inbound.front() else {
        return Ok((state.there == There::Ended).then(|| (0, false)));
    };
    let rest = front.get(at..).unwrap_or_default();
    let n = rest.len().min(buf.len());
    if let (Some(to), Some(from)) = (buf.get_mut(..n), rest.get(..n)) {
        to.copy_from_slice(from);
    }
    let whole = at.saturating_add(n) >= front.len();
    if whole {
        state.inbound.pop_front();
        state.read = 0;
    } else {
        state.read = at.saturating_add(n);
    }
    let drained = state.inbound.is_empty() && state.held;
    if drained {
        state.held = false;
    }
    Ok(Some((n, drained)))
}

impl Shared {
    pub(crate) fn new(
        peer: hyper_transport::PeerId,
        key: lanes::Conn,
        port: u32,
        commands: Commands,
    ) -> Self {
        Self {
            peer,
            key,
            port,
            state: Mutex::new(State {
                inbound: std::collections::VecDeque::with_capacity(HELD_AT),
                read: 0,
                there: There::Open,
                outbox: None,
                here_ended: false,
                gone: None,
                held: false,
            }),
            changed: Condvar::new(),
            commands,
            notify: Mutex::new(None),
        }
    }

    fn state(&self) -> io::Result<MutexGuard<'_, State>> {
        self.state.lock().map_err(poisoned)
    }

    fn told(&self) {
        self.changed.notify_all();
        if let Ok(notify) = self.notify.lock()
            && let Some(notify) = notify.as_ref()
        {
            notify();
        }
    }

    /// The driver: bytes from the peer. Whether the peer's lane is to be held now.
    pub(crate) fn arrived(&self, bytes: Vec<u8>) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.gone.is_some() {
            return false;
        }
        state.inbound.push_back(bytes);
        let hold = state.inbound.len() >= HELD_AT && !state.held;
        if hold {
            state.held = true;
        }
        drop(state);
        self.told();
        hold
    }

    /// The driver: the peer's half ended.
    pub(crate) fn ended(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.there = There::Ended;
        }
        self.told();
    }

    /// The driver: the connection is gone both ways, for `why`.
    pub(crate) fn gone(&self, why: Why) {
        if let Ok(mut state) = self.state.lock() {
            if state.gone.is_none() {
                state.gone = Some(why);
            }
            state.outbox = None;
            state.inbound.clear();
        }
        self.told();
    }

    /// The driver: the frame waiting for it, if any.
    pub(crate) fn take_outbox(&self) -> Option<Vec<u8>> {
        self.state.lock().ok()?.outbox.take()
    }

    /// The driver: whether a frame waits for it.
    pub(crate) fn outbox_waits(&self) -> bool {
        self.state.lock().is_ok_and(|state| state.outbox.is_some())
    }

    /// The driver: the frame it took went (or the connection is gone): a writer may place the next.
    pub(crate) fn sent(&self) {
        self.told();
    }

    fn command(&self, command: Command) -> io::Result<()> {
        self.commands
            .send(command)
            .map_err(|_| io::Error::new(io::ErrorKind::NotConnected, "the QUIC driver has ended"))
    }
}

/// One connection to a port on a peer, read and written by its owner's threads. Dropped with its half open, it is
/// reset: the peer hears its owner let it go ([`Why::Gone`]).
pub struct Conn {
    pub(crate) shared: Arc<Shared>,
}

impl std::fmt::Debug for Conn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Conn")
            .field("peer", &self.shared.peer)
            .field("conn", &self.shared.key)
            .field("port", &self.shared.port)
            .finish()
    }
}

impl Conn {
    /// The port it is to.
    pub fn port(&self) -> u32 {
        self.shared.port
    }

    /// Tells `notify` whenever it may be read, written or has ended: an owner that waits in an event loop of its
    /// own reads and writes it then with [`Conn::try_read`] and [`Conn::try_write`].
    pub fn set_notify(&self, notify: Notify) -> io::Result<()> {
        *self.shared.notify.lock().map_err(poisoned)? = Some(notify);
        Ok(())
    }

    /// Reads what has come into `buf`, waiting for it until `deadline` (none: until it comes or the connection
    /// ends); 0 once the peer's half has ended and all of it was read.
    pub fn read(&self, buf: &mut [u8], deadline: Option<Instant>) -> io::Result<usize> {
        let mut state = self.shared.state()?;
        loop {
            if let Some((n, drained)) = take(&mut state, buf)? {
                drop(state);
                self.drained(drained)?;
                return Ok(n);
            }
            state = self.wait(state, deadline)?;
        }
    }

    /// Reads what has come into `buf` without waiting: `None` while nothing has.
    pub fn try_read(&self, buf: &mut [u8]) -> io::Result<Option<usize>> {
        let mut state = self.shared.state()?;
        let taken = take(&mut state, buf)?;
        drop(state);
        match taken {
            Some((n, drained)) => {
                self.drained(drained)?;
                Ok(Some(n))
            }
            None => Ok(None),
        }
    }

    /// Tells the driver the owner read all that had come, once it held the peer's lane for it: never under the
    /// connection's lock, which the driver takes to hand it bytes.
    fn drained(&self, drained: bool) -> io::Result<()> {
        if !drained {
            return Ok(());
        }
        self.shared.command(Command::Drained {
            peer: self.shared.peer,
            conn: self.shared.key,
        })
    }

    fn wait<'a>(
        &self,
        state: MutexGuard<'a, State>,
        deadline: Option<Instant>,
    ) -> io::Result<MutexGuard<'a, State>> {
        match deadline {
            None => self.shared.changed.wait(state).map_err(poisoned),
            Some(at) => {
                let left = at.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "the caller's deadline passed",
                    ));
                }
                self.shared
                    .changed
                    .wait_timeout(state, left)
                    .map(|(state, _)| state)
                    .map_err(poisoned)
            }
        }
    }

    /// Writes all of `bytes`, waiting for room until `deadline` (none: until there is or the connection ends): the
    /// frames go in order, each once the last has gone.
    pub fn write_all(&self, bytes: &[u8], deadline: Option<Instant>) -> io::Result<()> {
        for chunk in bytes.chunks(lanes::DATA_BOUND) {
            let mut state = self.shared.state()?;
            loop {
                if let Some(why) = state.gone {
                    return Err(gone_error(why));
                }
                if state.here_ended {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "this side's half has ended",
                    ));
                }
                if state.outbox.is_none() {
                    break;
                }
                state = self.wait(state, deadline)?;
            }
            state.outbox = Some(chunk.to_vec());
            drop(state);
            self.shared.command(Command::Send {
                peer: self.shared.peer,
                conn: self.shared.key,
            })?;
        }
        Ok(())
    }

    /// Places `bytes`, at most one frame's, without waiting: whether there was room for them.
    pub fn try_write(&self, bytes: &[u8]) -> io::Result<bool> {
        if bytes.len() > lanes::DATA_BOUND {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "more than one frame's bytes",
            ));
        }
        let mut state = self.shared.state()?;
        if let Some(why) = state.gone {
            return Err(gone_error(why));
        }
        if state.here_ended {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "this side's half has ended",
            ));
        }
        if state.outbox.is_some() {
            return Ok(false);
        }
        state.outbox = Some(bytes.to_vec());
        drop(state);
        self.shared.command(Command::Send {
            peer: self.shared.peer,
            conn: self.shared.key,
        })?;
        Ok(true)
    }

    /// Ends this side's half without waiting: its End goes once a frame waiting for the driver has gone, so the
    /// peer reads all that was written, then its end. What an event loop's owner closes a connection with.
    pub fn close_write(&self) -> io::Result<()> {
        let mut state = self.shared.state()?;
        if let Some(why) = state.gone {
            return Err(gone_error(why));
        }
        if state.here_ended {
            return Ok(());
        }
        state.here_ended = true;
        drop(state);
        self.shared.command(Command::End {
            peer: self.shared.peer,
            conn: self.shared.key,
        })
    }

    /// The peer it is with.
    pub fn peer(&self) -> hyper_transport::PeerId {
        self.shared.peer
    }

    /// Whether the peer's half has ended and all of it was read, or the connection is gone: a read now returns
    /// its end.
    pub fn hung_up(&self) -> bool {
        self.shared.state.lock().is_ok_and(|state| {
            state.gone.is_some() || (state.there == There::Ended && state.inbound.is_empty())
        })
    }

    /// Ends this side's half once what was written has gone: the peer reads to its end.
    pub fn end(&self, deadline: Option<Instant>) -> io::Result<()> {
        let mut state = self.shared.state()?;
        loop {
            if let Some(why) = state.gone {
                return Err(gone_error(why));
            }
            if state.here_ended {
                return Ok(());
            }
            if state.outbox.is_none() {
                break;
            }
            state = self.wait(state, deadline)?;
        }
        state.here_ended = true;
        drop(state);
        self.shared.command(Command::End {
            peer: self.shared.peer,
            conn: self.shared.key,
        })
    }

    /// Ends the connection both ways now, for `why`: what the peer sends from now is let go.
    pub fn reset(&self, why: Why) {
        let first = match self.shared.state.lock() {
            Ok(mut state) => {
                let first = !state.here_ended || state.gone.is_none();
                state.here_ended = true;
                if state.gone.is_none() {
                    state.gone = Some(why);
                }
                state.outbox = None;
                state.inbound.clear();
                first
            }
            Err(_) => true,
        };
        if first {
            let _ = self.shared.command(Command::Reset {
                peer: self.shared.peer,
                conn: self.shared.key,
                why,
            });
        }
        self.shared.told();
    }

    /// Whether it can be written now: no frame waits for the driver, and it hasn't ended.
    pub fn writable(&self) -> bool {
        self.shared
            .state
            .lock()
            .is_ok_and(|state| state.outbox.is_none() && !state.here_ended && state.gone.is_none())
    }

    /// Whether a read would return now: bytes have come, the peer's half ended, or the connection is gone.
    pub fn readable(&self) -> bool {
        self.shared.state.lock().is_ok_and(|state| {
            !state.inbound.is_empty() || state.there == There::Ended || state.gone.is_some()
        })
    }

    /// Why the connection is gone, once it is.
    pub fn gone(&self) -> Option<Why> {
        self.shared.state.lock().ok().and_then(|state| state.gone)
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        let open = self.shared.state.lock().is_ok_and(|state| {
            state.gone.is_none() && !(state.here_ended && state.there == There::Ended)
        });
        if open {
            self.reset(Why::Gone);
        }
    }
}
