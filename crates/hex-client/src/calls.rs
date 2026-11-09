//! What every client of a node agent does whatever carries its messages: a call waits for its answer by its
//! request's id, a program's end goes to its `Proc`, and the connection's end reaches every call and program still
//! waiting. A [`Transport`] moves the bytes: the agent's local `SOCK_SEQPACKET` socket on Linux (`linux.rs`), its
//! framed `SOCK_STREAM` socket on macOS (`macos.rs`), or a framed byte stream to the node agent in a Shards guest
//! (`guest.rs`).
//!
//! No thread of the client's reads the connection: whoever waits on it reads, one at a time, the reader's turn (a
//! call for its answer or for room to send in, a `Proc` for its program's end), handing each message to whoever it
//! is for and the turn to another still waiting as it goes, so an answer wakes the thread it answers and no thread
//! between (goal 16: the socket's per-call cost; decision 3's client, which compares this with a reading thread of the
//! client's own). In the event form a node's loop holds the turn for good and reads as its connection turns ready
//! ([`Core::begin`], the guest's `Pump`). Each message is sent whole under the connection's send lock, so a stream's
//! frames never interleave; one that finds no room waits for it reading what the agent sends while it holds the turn,
//! as the agent reads no request while its answers wait for room in the client's socket, else waits on the connection
//! for room and on a wake of its own, which the turn's holder writes as it hands the turn to it. The core waits on
//! every connection the same way (poll(2), Winsock's `WSAPoll`), through the handle its transport lends it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags};

use hex_api::{
    BoxId, BoxInfo, BoxSpec, Class, Exit, Proc, Program, Refusal, Runtime, Signal, Stdio, Usage,
};
use hex_core::netns;
use hex_core::patience::Patience;
use hex_core::wire::{self, Answer, Call, Header, Limits, Received, Request, Streams};

use crate::MAX_LIST;
use crate::copy::{copy, copy_in};

/// What takes a call's answer in the event form ([`Core::begin`]), with its request's id (an exec's names
/// its program): run once, on the thread that reads the connection, after the client's lock is let go.
pub type Then = Box<dyn FnOnce(u64, Result<Answer, Refusal>) + Send>;
/// What takes an exec'd program's end in the event form: run once, as [`Then`] is.
pub type Ended = Box<dyn FnOnce(Result<Exit, Refusal>) + Send>;

/// How many kinds of call a client waits for an answer to, one wait each ([`kind_of`]): a hello and each [`Call`].
pub const KINDS: usize = 12;

/// The wait of `request`'s kind in `waits`, a client's table of its call kinds' learned waits (decision 9: a client's
/// call's wait for its answer and its send is learned per call kind, since a delete waits on its box's emptying and a
/// list on its parts, so a fast inspect's answer says nothing of theirs); `None` for one that has no answer (a
/// release). A table's order: hello, create, start, exec, signal, inspect, usage, list, delete, netns, then wire
/// a program's kill and an exec asking for streams.
pub(crate) fn kind_of<'a>(waits: &'a [Patience; KINDS], request: &Request) -> Option<&'a Patience> {
    let n = match request {
        Request::Hello { .. } => 0,
        Request::Call(call) => match call {
            Call::Create(_) => 1,
            Call::Start(_) => 2,
            Call::Exec { .. } => 3,
            Call::Signal { .. } => 4,
            Call::Inspect(_) => 5,
            Call::Usage(_) => 6,
            Call::List => 7,
            Call::Delete(_) => 8,
            Call::Netns => 9,
            Call::Kill { .. } => 10,
            Call::ExecStreams { .. } => 11,
        },
        Request::Release | Request::Taken => return None,
    };
    waits.get(n)
}

/// Why a send didn't go.
pub enum Sent {
    /// The connection is gone: the call's wait says why.
    Gone,
    /// No room in the agent's socket in all the call's patience: the agent isn't reading.
    Full(Refusal),
    Refused(Refusal),
}

/// A descriptor lent to a message as it is sent (an exec's stdio, a pipe's end of the caller's): the agent gets
/// its own, and this one stays the caller's.
#[cfg(unix)]
pub type Lent<'a> = std::os::fd::BorrowedFd<'a>;
#[cfg(windows)]
pub type Lent<'a> = std::os::windows::io::BorrowedHandle<'a>;

/// No descriptors: what lends nothing to a message.
const NONE: &[Lent<'static>] = &[];

/// An OS handle the core waits on: a file descriptor on Unix, a Winsock socket on Windows.
#[cfg(unix)]
pub type Handle<'a> = std::os::fd::BorrowedFd<'a>;
#[cfg(windows)]
pub type Handle<'a> = std::os::windows::io::BorrowedSocket<'a>;

/// What a wait on a connection found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Ready {
    /// Something to read, or its end.
    read: bool,
    /// Room to write.
    write: bool,
    /// The waiter's wake was written: the reader's turn was handed to it, or the connection ended.
    woken: bool,
}

/// Waits until `conn` has something to read or has ended, or has room to write (with `write`), or `wake` is
/// written, at most `wait` (none: until one does). An interrupted wait finds nothing.
fn ready(
    conn: Handle<'_>,
    write: bool,
    wake: Option<&Wake>,
    wait: Option<Duration>,
) -> Result<Ready, Refusal> {
    let flags = if write {
        PollFlags::IN | PollFlags::OUT
    } else {
        PollFlags::IN
    };
    let mut fds = vec![PollFd::from_borrowed_fd(conn, flags)];
    if let Some(wake) = wake {
        fds.push(PollFd::from_borrowed_fd(wake.handle(), PollFlags::IN));
    }
    match rustix::event::poll(&mut fds, wait.map(timespec).as_ref()) {
        Ok(_) => {}
        Err(e) if e == rustix::io::Errno::INTR => return Ok(Ready::default()),
        Err(e) => {
            return Err(Refusal::Broken(format!(
                "waiting on the node agent's connection: {e}"
            )));
        }
    }
    let got = |n: usize| fds.get(n).map_or(PollFlags::empty(), PollFd::revents);
    let (conn, woken) = (got(0), got(1));
    Ok(Ready {
        read: conn.intersects(PollFlags::IN | PollFlags::ERR | PollFlags::HUP),
        write: conn.contains(PollFlags::OUT),
        woken: !woken.is_empty(),
    })
}

/// What wakes a send waiting for room on the connection when the reader's turn is handed to it, or the connection
/// ends: one end of a pair the waiter polls beside the connection, made only when a send first waits with the turn
/// another's, so a send that finds room pays nothing for it. A Unix socket pair, or on Windows (whose `WSAPoll` waits
/// on sockets alone) a loopback datagram socket connected to itself, so no other socket can write it.
struct Wake {
    #[cfg(unix)]
    read: std::os::unix::net::UnixStream,
    #[cfg(unix)]
    write: std::os::unix::net::UnixStream,
    #[cfg(windows)]
    sock: std::net::UdpSocket,
}

impl Wake {
    #[cfg(unix)]
    fn new() -> std::io::Result<Self> {
        let (read, write) = std::os::unix::net::UnixStream::pair()?;
        read.set_nonblocking(true)?;
        write.set_nonblocking(true)?;
        Ok(Self { read, write })
    }

    #[cfg(windows)]
    fn new() -> std::io::Result<Self> {
        let sock = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        sock.connect(sock.local_addr()?)?;
        sock.set_nonblocking(true)?;
        Ok(Self { sock })
    }

    /// Wakes the waiter: a byte it reads as one wake, however many came.
    fn wake(&self) {
        #[cfg(unix)]
        let _ = std::io::Write::write(&mut &self.write, &[1]);
        #[cfg(windows)]
        let _ = self.sock.send(&[1]);
    }

    /// What woke it, read, so it wakes no one until written again.
    fn drain(&self) {
        let mut buf = [0u8; 64];
        #[cfg(unix)]
        while matches!(std::io::Read::read(&mut &self.read, &mut buf), Ok(n) if n > 0) {}
        #[cfg(windows)]
        while self.sock.recv(&mut buf).is_ok() {}
    }

    fn handle(&self) -> Handle<'_> {
        #[cfg(unix)]
        return std::os::fd::AsFd::as_fd(&self.read);
        #[cfg(windows)]
        std::os::windows::io::AsSocket::as_socket(&self.sock)
    }
}

/// What carries a client's messages to its node agent. Its connection is read and written without waiting, and
/// waited on through its handle ([`Transport::handle`]), by whoever holds the reader's turn.
pub trait Transport: Send + Sync + 'static {
    /// What the turn's holder reads into, and what of a message has come so far: kept across reads and across
    /// holders, so a frame cut across reads is whole when its last bytes come.
    type Reading: Send;
    /// Its call kinds' learned waits for an answer, by [`kind_of`]'s order: each its own record's, as its
    /// transport and its agent answer it, measuring where none is recorded yet (decision 9).
    fn answers() -> &'static [Patience; KINDS];
    /// A read state for a fresh connection.
    fn reading(&self) -> Self::Reading;
    /// The connection's OS handle, lent for the core to wait on.
    fn handle(&self) -> Handle<'_>;
    /// Reads what has come, without waiting: each whole message, with the descriptors that came with it, handed to
    /// `take`, a message cut across reads kept in `reading` for the next. Whether anything came; the connection's end,
    /// or a message `take` refuses, is the error, saying why.
    fn recv(
        &self,
        reading: &mut Self::Reading,
        take: &mut dyn FnMut(&Received<'_>, Vec<hex_api::End>) -> Result<(), Refusal>,
    ) -> Result<bool, Refusal>;
    /// The bytes a message goes as: itself where the transport keeps messages whole, framed where it is a stream.
    fn frame(message: Vec<u8>) -> Result<Vec<u8>, Refusal> {
        Ok(message)
    }
    /// Writes what it can of `bytes` past `*at` without waiting, the descriptors lent riding with its first byte,
    /// moving `at` on: whether all of it is out. The caller holds the connection's send lock across a message's
    /// writes, so no other message's bytes come between them.
    fn try_send(&self, bytes: &[u8], at: &mut usize, lent: &[Lent<'_>]) -> Result<bool, Sent>;
    /// Ends the connection both ways: whoever reads next sees the end.
    fn shutdown(&self);
    /// Whether every stream of an exec is a connection of its own that it makes, and no descriptor rides with a
    /// message: a guest's. It makes those an exec asks for ([`Transport::asks`]) and takes the pipes the caller
    /// carries ([`Transport::carries`]); elsewhere the carried pipes ride with the exec, and a stream asked for
    /// is the agent's to make, which gives its end with its answer ([`Answer::Given`]).
    const MAKES_ASKED: bool = false;
    /// The caller's own pipes for exec `exec`, each copied between it and a connection of its own made for it,
    /// here in the caller's client: only where [`Transport::MAKES_ASKED`].
    fn carries(&self, exec: u64, stdio: Stdio) -> Result<(), Refusal> {
        let _ = (exec, stdio);
        Err(Refusal::Broken(
            "this transport lends an exec's pipes to its message".to_owned(),
        ))
    }
    /// The streams `ask` names for exec `exec`, made here, its program's ends the agent's to find (a
    /// guest's stdio connections, named for it): only where [`Transport::MAKES_ASKED`].
    fn asks(&self, exec: u64, ask: hex_api::Ask) -> Result<hex_api::Given, Refusal> {
        let _ = (exec, ask);
        Err(Refusal::Broken(
            "this transport asks the agent for an exec's streams".to_owned(),
        ))
    }
}

pub(crate) fn poisoned<T>(_: std::sync::PoisonError<T>) -> Refusal {
    Refusal::Broken("a lock of the node agent's client was poisoned".to_owned())
}

/// The longest one wait on a connection can be: `i32::MAX` milliseconds, the most poll(2) takes where it has no
/// `ppoll` (macOS; Windows' `WSAPoll`), its timeout a C `int` of milliseconds. A longer wait waits this long at a
/// time, its caller waiting again until its own deadline.
const MOST_ONE_WAIT: Duration = Duration::from_millis(2_147_483_647);
/// [`MOST_ONE_WAIT`] as poll(2) takes it.
const MOST_ONE_WAIT_SPEC: rustix::event::Timespec = rustix::event::Timespec {
    tv_sec: 2_147_483,
    tv_nsec: 647_000_000,
};

/// `wait` as poll(2) takes it, at most [`MOST_ONE_WAIT`].
fn timespec(wait: Duration) -> rustix::event::Timespec {
    if wait >= MOST_ONE_WAIT {
        return MOST_ONE_WAIT_SPEC;
    }
    rustix::event::Timespec::try_from(wait).unwrap_or(MOST_ONE_WAIT_SPEC)
}

/// What the callers and the `Proc`s share: the connection, what waits on it, what its reader's turn reads into, and
/// its send lock.
pub struct Shared<T: Transport> {
    pub(crate) transport: T,
    state: Mutex<State>,
    /// The turn's holder's: one read at a time.
    reading: Mutex<T::Reading>,
    /// Held across one whole message's writes.
    sending: Mutex<()>,
}

#[derive(Default)]
struct State {
    /// Calls waiting for their answers, by request id.
    calls: BTreeMap<u64, Waiting>,
    /// Programs whose end something waits for, by their exec's id.
    programs: BTreeMap<u64, Watching>,
    next: u64,
    /// Why the connection ended, once it has.
    closed: Option<Refusal>,
    /// Why the agent won't serve the connection, when it said (an answer to no request, id 0).
    refused: Option<Refusal>,
    /// Each exec's streams' ends, by its request's id, until its exec takes them: those its transport made (a
    /// guest's connections), or those its agent's answer gave as its descriptors. An exec refused has its ends
    /// closed with its answer.
    given: BTreeMap<u64, hex_api::Given>,
    /// Whether a thread holds the reader's turn, or the event form's loop does for good.
    reading: bool,
    /// The execs whose `Proc` is in its wait, which the turn may be handed to.
    waiting: BTreeSet<u64>,
}

impl State {
    /// The reader's turn given up by its holder: handed to the oldest call that can take it (one whose request
    /// went, or whose send waits for room), else to a `Proc` in its wait, else free, so while anyone waits on the
    /// connection someone reads it. A call queued for the send lock can't read until it has it, so it is passed by.
    fn pass_turn(&mut self) {
        for waiting in self.calls.values() {
            if waiting.reads
                && let Taker::Wait(mail) = &waiting.answer
                && mail.try_send(Mail::Turn).is_ok()
            {
                if let Some(wake) = &waiting.wake {
                    wake.wake();
                }
                return;
            }
        }
        for exec in &self.waiting {
            if let Some(Watching::Proc(ending)) = self.programs.get(exec)
                && let Ok(mut ended) = ending.state.lock()
            {
                ended.turn = true;
                ending.came.notify_all();
                return;
            }
        }
        self.reading = false;
    }
}

struct Waiting {
    answer: Taker,
    /// A list's boxes from the parts that came so far.
    boxes: Vec<BoxInfo>,
    /// Whether it can take the reader's turn: its request sent, or its send waiting for room with the send lock held.
    reads: bool,
    /// While its send waits for room with the reader's turn another's: the wake it waits on besides the connection,
    /// written as the turn is handed to it or the connection ends.
    wake: Option<Arc<Wake>>,
}

/// What a waiting call is handed: its answer, or the reader's turn. The answer is boxed, so a turn is no bigger than
/// a pointer.
enum Mail {
    Answer(Box<Result<Answer, Refusal>>),
    Turn,
}

/// What takes a call's answer: a caller waiting on it, or a completion ([`Then`]).
enum Taker {
    Wait(SyncSender<Mail>),
    Then(Then),
}

/// What waits for a program's end: its `Proc`, or a completion ([`Ended`]).
enum Watching {
    Proc(Arc<Ending>),
    Then(Ended),
}

/// A completion to run once the client's lock is let go.
type Due = Box<dyn FnOnce() + Send>;

impl Taker {
    /// Gives `answer` to its taker: at once to a waiting caller, or as a completion run later.
    fn give(self, id: u64, answer: Result<Answer, Refusal>, due: &mut Vec<Due>) {
        match self {
            Self::Wait(mail) => {
                let _ = mail.try_send(Mail::Answer(Box::new(answer)));
            }
            Self::Then(then) => due.push(Box::new(move || then(id, answer))),
        }
    }
}

/// A program's end, once it has come, and its `Proc`'s waking: by its end, or by the reader's turn handed to its wait.
#[derive(Default)]
struct Ending {
    state: Mutex<EndState>,
    came: Condvar,
}

#[derive(Default)]
struct EndState {
    exit: Option<Result<Exit, Refusal>>,
    turn: bool,
}

impl<T: Transport> Shared<T> {
    pub(crate) fn new(transport: T) -> Self {
        let reading = transport.reading();
        Self {
            transport,
            state: Mutex::new(State::default()),
            reading: Mutex::new(reading),
            sending: Mutex::new(()),
        }
    }

    fn state(&self) -> Result<MutexGuard<'_, State>, Refusal> {
        self.state.lock().map_err(poisoned)
    }

    /// Hands one message to whoever waits for it: an answer to its call, a program's end to its `Proc` or completion;
    /// a given answer's descriptors, one for each stream it names, in order, kept for its exec. One nobody waits for (a
    /// call that gave up waiting, a program released) has no taker. Completions run after the lock is let go, so one
    /// may call the client again.
    fn deliver(
        &self,
        header: Header,
        answer: Answer,
        fds: Vec<hex_api::End>,
    ) -> Result<(), Refusal> {
        let mut due: Vec<Due> = Vec::new();
        {
            let mut state = self.state()?;
            if let Answer::Given { given, .. } = &answer {
                let mut fds = fds.into_iter();
                let mut take = |on: bool| if on { fds.next() } else { None };
                let ends = hex_api::Given {
                    stdin: take(given.stdin),
                    stdout: take(given.stdout),
                    stderr: take(given.stderr),
                };
                state.given.insert(header.id, ends);
            }
            match answer {
                Answer::Exited(exit) => match state.programs.remove(&header.id) {
                    Some(Watching::Proc(ending)) => {
                        ending.state.lock().map_err(poisoned)?.exit = Some(Ok(exit));
                        ending.came.notify_all();
                        // Its Proc forgets it when it is dropped or has its end.
                        state.programs.insert(header.id, Watching::Proc(ending));
                    }
                    Some(Watching::Then(ended)) => due.push(Box::new(move || ended(Ok(exit)))),
                    None => {}
                },
                Answer::Refused(refusal) if header.id == 0 => state.refused = Some(refusal),
                Answer::Boxes { infos, more: true } => {
                    if let Some(waiting) = state.calls.get_mut(&header.id) {
                        if waiting.boxes.len().saturating_add(infos.len()) > MAX_LIST {
                            if let Some(w) = state.calls.remove(&header.id) {
                                w.answer.give(
                                    header.id,
                                    Err(Refusal::Exhausted {
                                        what: "boxes in one list",
                                        bound: MAX_LIST,
                                    }),
                                    &mut due,
                                );
                            }
                        } else {
                            waiting.boxes.extend(infos);
                        }
                    }
                }
                answer => {
                    // An exec that started no program has no end to come, and no streams: a completion waiting for
                    // its end is let go, and the ends made for it close.
                    if !matches!(answer, Answer::Started { .. } | Answer::Given { .. }) {
                        if matches!(state.programs.get(&header.id), Some(Watching::Then(_))) {
                            state.programs.remove(&header.id);
                        }
                        state.given.remove(&header.id);
                    }
                    if let Some(mut waiting) = state.calls.remove(&header.id) {
                        let answer = match answer {
                            Answer::Boxes { infos, more } => {
                                waiting.boxes.extend(infos);
                                Answer::Boxes {
                                    infos: std::mem::take(&mut waiting.boxes),
                                    more,
                                }
                            }
                            other => other,
                        };
                        waiting.answer.give(header.id, Ok(answer), &mut due);
                    }
                }
            }
        }
        for run in due {
            run();
        }
        Ok(())
    }

    /// The connection has ended: every call waiting and every program's `Proc` or completion hear why, and no one
    /// reads any more.
    pub(crate) fn close(&self, why: Refusal) {
        let mut due: Vec<Due> = Vec::new();
        {
            let Ok(mut state) = self.state() else {
                return;
            };
            let why = state.refused.take().unwrap_or(why);
            for (id, waiting) in std::mem::take(&mut state.calls) {
                waiting.answer.give(id, Err(why.clone()), &mut due);
                if let Some(wake) = &waiting.wake {
                    wake.wake();
                }
            }
            let lost = Refusal::Broken(format!(
                "the connection to the node agent closed before the program's end came: {why}"
            ));
            for (exec, watching) in std::mem::take(&mut state.programs) {
                match watching {
                    Watching::Proc(ending) => {
                        if let Ok(mut ended) = ending.state.lock()
                            && ended.exit.is_none()
                        {
                            ended.exit = Some(Err(lost.clone()));
                            ending.came.notify_all();
                        }
                        state.programs.insert(exec, Watching::Proc(ending));
                    }
                    Watching::Then(ended) => {
                        let lost = lost.clone();
                        due.push(Box::new(move || ended(Err(lost))));
                    }
                }
            }
            state.closed = Some(why);
            state.reading = false;
        }
        for run in due {
            run();
        }
    }

    /// The connection ended for `why`: everyone waiting told ([`Shared::close`]), and why, as each is told.
    fn end(&self, why: Refusal) -> Refusal {
        self.transport.shutdown();
        self.close(why.clone());
        self.state()
            .ok()
            .and_then(|state| state.closed.clone())
            .unwrap_or(why)
    }

    /// One wait and read by the turn's holder: the connection waited on at most `wait` (none: as long as it takes),
    /// then what has come read and handed to whoever it is for. Whether anything came; the connection's end is the
    /// error, everyone waiting told why.
    fn turn(&self, wait: Option<Duration>) -> Result<bool, Refusal> {
        let ready = match ready(self.transport.handle(), false, None, wait) {
            Ok(ready) => ready,
            Err(why) => return Err(self.end(why)),
        };
        if ready.read { self.recv() } else { Ok(false) }
    }

    /// What has come, read by the turn's holder (or the event form's loop) without waiting and handed to whoever it
    /// is for. Whether anything came; the connection's end, or a message that isn't an answer, is the error, everyone
    /// waiting told why.
    pub(crate) fn recv(&self) -> Result<bool, Refusal> {
        // A lock poisoned mid-read leaves the connection's messages unknown: it ends, everyone waiting told.
        let mut reading = match self.reading.lock() {
            Ok(reading) => reading,
            Err(e) => return Err(self.end(poisoned(e))),
        };
        let mut take = |msg: &Received<'_>, fds: Vec<hex_api::End>| -> Result<(), Refusal> {
            let (header, answer) = wire::read_answer(msg, wire::MAX_MESSAGE).map_err(|fault| {
                match fault {
                    // An agent of another wire: refused by type, naming both wires and both builds.
                    wire::Fault::Wire { .. } => Refusal::from(fault),
                    fault => Refusal::Broken(format!(
                        "the node agent sent a message that isn't an answer: {fault}"
                    )),
                }
            })?;
            self.deliver(header, answer, fds)
        };
        let got = self.transport.recv(&mut reading, &mut take);
        drop(reading);
        got.map_err(|why| self.end(why))
    }

    /// The event form: the connection is read by the caller's loop from now on ([`Shared::recv`]), never by a call
    /// or a `Proc`, which wait for what the loop hands them.
    pub(crate) fn read_by_loop(&self) -> Result<(), Refusal> {
        self.state()?.reading = true;
        Ok(())
    }

    /// Whether its connection has ended.
    fn closed(&self) -> bool {
        !self.state().is_ok_and(|state| state.closed.is_none())
    }
}

/// A call while it is made: its place among the calls, its mailbox, its answer once come, and whether it holds the
/// reader's turn. However it goes it hands on a turn it holds or was handed, and forgets the call if unanswered.
struct Caller<'a, T: Transport> {
    shared: &'a Shared<T>,
    id: u64,
    mail: Receiver<Mail>,
    /// Whether it holds the reader's turn.
    mine: bool,
    /// Its answer, come to its mailbox and not yet returned.
    answer: Option<Result<Answer, Refusal>>,
    /// Answered: an exec's place for its program's end is then its `Proc`'s, and its streams' ends its exec's.
    answered: bool,
}

impl<'a, T: Transport> Caller<'a, T> {
    /// A call's place among the calls, with `program`, an exec's place for the end of the program it starts, made
    /// before the exec goes, so the end can't come first.
    fn new(shared: &'a Shared<T>, program: Option<Arc<Ending>>) -> Result<Self, Refusal> {
        // An answer, and the reader's turn handed to it before that: never more at once.
        let (tx, mail) = sync_channel(2);
        let mut state = shared.state()?;
        if let Some(why) = &state.closed {
            return Err(why.clone());
        }
        let id = state
            .next
            .checked_add(1)
            .ok_or_else(|| Refusal::Broken("the client's request ids ran out".to_owned()))?;
        state.next = id;
        state.calls.insert(
            id,
            Waiting {
                answer: Taker::Wait(tx),
                boxes: Vec::new(),
                reads: false,
                wake: None,
            },
        );
        if let Some(program) = program {
            state.programs.insert(id, Watching::Proc(program));
        }
        drop(state);
        Ok(Self {
            shared,
            id,
            mail,
            mine: false,
            answer: None,
            answered: false,
        })
    }

    /// What came to its mailbox: the reader's turn, its answer.
    fn collect(&mut self) {
        while let Ok(mail) = self.mail.try_recv() {
            self.take(mail);
        }
    }

    fn take(&mut self, mail: Mail) {
        match mail {
            Mail::Turn => self.mine = true,
            Mail::Answer(answer) => self.answer = Some(*answer),
        }
    }

    /// Whether it can take the reader's turn from now: its send under way with the send lock held, or its request
    /// sent and its answer awaited.
    fn reads(&self) -> Result<(), Refusal> {
        if let Some(waiting) = self.shared.state()?.calls.get_mut(&self.id) {
            waiting.reads = true;
        }
        Ok(())
    }

    /// Sends `bytes` whole with `lent`, under the send lock, waiting for room in the agent's socket until `deadline`
    /// (none: as long as it takes), its call's learned wait (`patience`, named in a refusal). The node agent reads no
    /// request while its answers wait for room in the client's socket, so a send waiting for room has someone reading:
    /// itself while it holds the reader's turn, which it takes when free or is handed, reading what comes as it waits;
    /// else the turn's holder, which hands the turn to it, waking it, as it goes.
    fn send(
        &mut self,
        bytes: &[u8],
        lent: &[Lent<'_>],
        deadline: Option<Instant>,
        patience: Option<Duration>,
    ) -> Result<(), Sent> {
        let shared = self.shared;
        let _whole = shared
            .sending
            .lock()
            .map_err(|e| Sent::Refused(poisoned(e)))?;
        self.reads().map_err(Sent::Refused)?;
        let mut at = 0;
        // Made the first time the send waits with the turn another's, so a send that finds room pays nothing for it.
        let mut woken: Option<Arc<Wake>> = None;
        let sent = loop {
            match shared.transport.try_send(bytes, &mut at, lent) {
                Ok(true) => break Ok(()),
                Ok(false) => {}
                Err(e) => break Err(e),
            }
            let left = deadline.map(|d| d.saturating_duration_since(Instant::now()));
            if left.is_some_and(|l| l.is_zero()) {
                break Err(Sent::Full(Refusal::Os {
                    op: "sending to the node agent",
                    why: format!(
                        "no room in its socket for {:?}, twice the slowest answer to a call of its kind: it isn't \
                         reading",
                        patience.unwrap_or_default()
                    ),
                }));
            }
            if let Err(e) = self.wait_for_room(&mut woken, left) {
                break Err(e);
            }
        };
        if woken.is_some()
            && let Ok(mut state) = shared.state()
            && let Some(waiting) = state.calls.get_mut(&self.id)
        {
            waiting.wake = None;
        }
        // A message cut short on a stream leaves the connection unable to carry another: it goes.
        if sent.is_err() && at > 0 {
            shared.transport.shutdown();
        }
        sent
    }

    /// One wait for room for its send, at most `left` (none: as long as it takes): reading what the agent sends as
    /// it waits while it holds the reader's turn, which it takes when free or is handed; else waiting on the
    /// connection for room and on its wake (`woken`, made once and kept with the call, where the holder handing the
    /// turn on finds it) for the turn. The connection's end is [`Sent::Gone`], its why the call's answer.
    fn wait_for_room(
        &mut self,
        woken: &mut Option<Arc<Wake>>,
        left: Option<Duration>,
    ) -> Result<(), Sent> {
        let shared = self.shared;
        let wake = {
            let mut state = shared.state().map_err(Sent::Refused)?;
            self.collect();
            if self.answer.is_some() || state.closed.is_some() {
                return Err(Sent::Gone);
            }
            if !self.mine && !state.reading {
                state.reading = true;
                self.mine = true;
            }
            if self.mine {
                None
            } else {
                let wake = match woken {
                    Some(wake) => Arc::clone(wake),
                    None => {
                        let made = Wake::new().map_err(|e| {
                            Sent::Refused(Refusal::os(
                                "making the wake a send waits on for the reader's turn",
                                e,
                            ))
                        })?;
                        Arc::clone(woken.insert(Arc::new(made)))
                    }
                };
                if let Some(waiting) = state.calls.get_mut(&self.id) {
                    waiting.wake = Some(Arc::clone(&wake));
                }
                Some(wake)
            }
        };
        let handle = shared.transport.handle();
        match wake {
            // The turn its: what the agent sends read as it comes, until there's room.
            None => {
                let got =
                    ready(handle, true, None, left).map_err(|e| Sent::Refused(shared.end(e)))?;
                if got.read && shared.recv().is_err() {
                    // The connection ended: every call waiting was told why, this one among them.
                    self.mine = false;
                    return Err(Sent::Gone);
                }
            }
            // The turn another's: room waited for, and the turn, should it be handed to this call.
            Some(wake) => {
                let got = ready(handle, true, Some(&wake), left)
                    .map_err(|e| Sent::Refused(shared.end(e)))?;
                if got.woken {
                    wake.drain();
                }
            }
        }
        Ok(())
    }

    /// Its answer, waited for until `deadline` (none: as long as it takes): read by this thread while it holds the
    /// reader's turn, taken when free or handed to it, else handed to it by the thread that holds the turn (or the
    /// event form's loop). `None` once the deadline has passed.
    fn wait(&mut self, deadline: Option<Instant>) -> Option<Result<Answer, Refusal>> {
        let shared = self.shared;
        if let Err(why) = self.reads() {
            return Some(Err(why));
        }
        loop {
            self.collect();
            if let Some(answer) = self.answer.take() {
                self.answered = answer.is_ok();
                return Some(answer);
            }
            if !self.mine {
                match shared.state() {
                    Ok(mut state) => {
                        if !state.reading && state.closed.is_none() {
                            state.reading = true;
                            self.mine = true;
                        }
                    }
                    Err(why) => return Some(Err(why)),
                }
            }
            let left = deadline.map(|d| d.saturating_duration_since(Instant::now()));
            if left.is_some_and(|l| l.is_zero()) {
                return None;
            }
            if self.mine {
                if shared.turn(left).is_err() {
                    // The connection ended: every call waiting was told why, this one among them.
                    self.mine = false;
                }
            } else {
                match self.mail.recv_timeout(left.unwrap_or(Duration::MAX)) {
                    Ok(mail) => self.take(mail),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        return Some(Err(Refusal::Broken(
                            "the node agent's answer was lost".to_owned(),
                        )));
                    }
                }
            }
        }
    }
}

impl<T: Transport> Drop for Caller<'_, T> {
    /// Going: the call forgotten if unanswered, its streams' ends with it, and the reader's turn it holds, or was
    /// handed as it went, handed on.
    fn drop(&mut self) {
        let Ok(mut state) = self.shared.state() else {
            return;
        };
        if !self.answered {
            state.calls.remove(&self.id);
            state.programs.remove(&self.id);
            state.given.remove(&self.id);
        }
        while let Ok(mail) = self.mail.try_recv() {
            if matches!(mail, Mail::Turn) {
                self.mine = true;
            }
        }
        if self.mine && state.closed.is_none() {
            state.pass_turn();
        }
    }
}

/// The connection, held by the client and by each `Proc` it returned: it closes when the last goes.
pub(crate) struct Conn<T: Transport> {
    shared: Arc<Shared<T>>,
}

impl<T: Transport> Drop for Conn<T> {
    fn drop(&mut self) {
        // The agent sees the hang-up.
        self.shared.transport.shutdown();
    }
}

impl<T: Transport> Conn<T> {
    /// Sends `request` with the descriptors `with` lends it, once `with` has made what its id names (an exec's
    /// streams its transport makes), and waits for its answer: at most twice the slowest answer this process has had
    /// to a call of its kind ([`Transport::answers`]), past which the call is refused by type, the only way left to
    /// it; as long as it takes while its kind's wait measures. With `program`, a `Proc`'s place for the end of the
    /// program an exec starts is made before the exec goes, so the end can't come first.
    fn call<'l>(
        &self,
        bound: usize,
        request: &Request,
        with: impl FnOnce(u64) -> Result<&'l [Lent<'l>], Refusal>,
        program: Option<Arc<Ending>>,
    ) -> Result<(u64, Answer), Refusal> {
        let shared = &*self.shared;
        let mut caller = Caller::new(shared, program)?;
        let waits = kind_of(T::answers(), request);
        let patience = waits.and_then(Patience::deadline);
        let asked = Instant::now();
        let deadline = patience.and_then(|p| asked.checked_add(p));
        let bytes = wire::request(caller.id, request, bound)
            .map_err(Refusal::from)
            .and_then(T::frame)?;
        let lent = with(caller.id)?;
        match caller.send(&bytes, lent, deadline, patience) {
            // Sent, or the agent has gone: why reaches the call as it waits, which sees the end.
            Ok(()) | Err(Sent::Gone) => {}
            // No room for the request in all the call's patience: its wait ran out, as an answer's would.
            Err(Sent::Full(r)) => {
                if let (Some(waits), Some(wait)) = (waits, patience) {
                    waits.ran_out(wait);
                }
                return Err(r);
            }
            Err(Sent::Refused(r)) => return Err(r),
        }
        match caller.wait(deadline) {
            Some(Ok(answer)) => {
                if let Some(waits) = waits {
                    waits.answered(asked.elapsed());
                }
                Ok((caller.id, answer))
            }
            Some(Err(why)) => Err(why),
            None => {
                // No sample (Karn's point), but the census is told the answer took longer than its bound.
                if let (Some(waits), Some(wait)) = (waits, patience) {
                    waits.ran_out(wait);
                }
                Err(Refusal::Os {
                    op: "waiting for the node agent's answer",
                    why: format!(
                        "none came in {:?}, twice the slowest answer to a call of its kind",
                        patience.unwrap_or_default()
                    ),
                })
            }
        }
    }

    /// Sends one message that has no answer (a release) now or not at all: a release that finds no room is the
    /// agent's disconnect rule's to settle, so dropping a `Proc` never waits on the agent. One whose first bytes went
    /// on a stream is sent whole, waiting for room as a send does, since a frame cut short would leave the connection
    /// unable to carry another.
    fn send_now(&self, bytes: &[u8]) {
        let shared = &*self.shared;
        let Ok(framed) = T::frame(bytes.to_vec()) else {
            return;
        };
        let mut at = 0;
        {
            let Ok(_whole) = shared.sending.lock() else {
                return;
            };
            match shared.transport.try_send(&framed, &mut at, NONE) {
                Ok(true) | Err(_) => return,
                Ok(false) if at == 0 => return,
                Ok(false) => {}
            }
        }
        // Part of it went: the rest goes as a send does, the frame whole on the stream.
        let Ok(mut caller) = Caller::new(shared, None) else {
            shared.transport.shutdown();
            return;
        };
        let rest = framed.get(at..).unwrap_or_default();
        if caller.send(rest, NONE, None, None).is_err() {
            shared.transport.shutdown();
        }
    }
}

/// A node agent, reached through a transport.
pub struct Core<T: Transport> {
    conn: Arc<Conn<T>>,
    classes: Vec<Class>,
    limits: Limits,
}

impl<T: Transport> Core<T> {
    /// Says hello over `shared`, naming this build, and learns what the agent runs and its bounds. An agent of
    /// another wire is refused here, by type, naming both wires and both builds (`hex_core::wire::Fault::Wire`):
    /// two sides talk only when built from the same wire.
    pub(crate) fn hello(shared: Arc<Shared<T>>) -> Result<Self, Refusal> {
        let client = Self {
            conn: Arc::new(Conn { shared }),
            classes: Vec::new(),
            limits: Limits {
                connections: 1,
                message_bytes: wire::MAX_MESSAGE,
                in_flight: 1,
                programs: 0,
            },
        };
        let hello = Request::Hello {
            build: wire::build(),
        };
        let answer = client.call(&hello, |_| Ok(NONE), None)?.1;
        client.greeted(answer)
    }

    /// Whether its connection has ended: no call it sends is answered.
    pub fn closed(&self) -> bool {
        self.conn.shared.closed()
    }

    /// This client, told by the hello's `answer` what its agent runs and its bounds.
    fn greeted(mut self, answer: Answer) -> Result<Self, Refusal> {
        match answer {
            Answer::Hello(hello) => {
                self.classes = hello.classes;
                self.limits = Limits {
                    message_bytes: hello.limits.message_bytes.min(wire::MAX_MESSAGE),
                    ..hello.limits
                };
                Ok(self)
            }
            Answer::Refused(r) => Err(r),
            other => Err(unexpected("hello", &other)),
        }
    }

    /// The event form of a call: sends `call`, an exec's with `stdio`, now, and returns its request's id at once.
    /// `then` gets its answer on the thread that reads the connection (the event form's loop), and an exec's program's
    /// end goes to `ended` once its start has come; the ends of the streams it asked for are taken with
    /// [`Core::given`]. The event form copies no stream: where the transport makes an exec's streams (a guest's), each
    /// is asked for, and a pipe of the caller's own is refused. A call that can't be sent now is refused here, and
    /// `then` never runs.
    pub fn begin(
        &self,
        call: Call,
        stdio: Stdio,
        then: Then,
        ended: Ended,
    ) -> Result<u64, Refusal> {
        let (id, program) = match call {
            Call::Exec { id, program, .. } | Call::ExecStreams { id, program, .. } => (id, program),
            call => {
                return self.begun(&Request::Call(call), |_| Ok(NONE), then, None);
            }
        };
        stdio.check()?;
        let (carried, asked) = (carried(&stdio), streams(stdio.ask));
        let (request, made) = if T::MAKES_ASKED {
            if carried != Streams::default() {
                return Err(Refusal::Invalid {
                    what: "stdio",
                    why: "the event form copies no stream: ask for each of a guest program's streams, whose ends are the caller's".to_owned(),
                });
            }
            let exec = Call::Exec {
                id,
                program,
                streams: asked,
            };
            (exec, asked)
        } else if asked == Streams::default() {
            let exec = Call::Exec {
                id,
                program,
                streams: carried,
            };
            (exec, Streams::default())
        } else {
            let exec = Call::ExecStreams {
                id,
                program,
                streams: carried,
                ask: asked,
            };
            (exec, Streams::default())
        };
        let lent = lend(&stdio);
        let shared = &*self.conn.shared;
        self.begun(
            &Request::Call(request),
            |exec| {
                make(shared, exec, made)?;
                Ok(&lent[..])
            },
            then,
            Some(Watching::Then(ended)),
        )
    }

    /// Sends `request` now with what `with` makes for its id and lends it, its answer to go to `then` and an exec's
    /// program's end to `program`, each placed before the request goes; refused, and forgotten, when it can't go now.
    fn begun<'l>(
        &self,
        request: &Request,
        with: impl FnOnce(u64) -> Result<&'l [Lent<'l>], Refusal>,
        then: Then,
        program: Option<Watching>,
    ) -> Result<u64, Refusal> {
        let shared = &*self.conn.shared;
        let id = {
            let mut state = shared.state()?;
            if let Some(why) = &state.closed {
                return Err(why.clone());
            }
            let id = state
                .next
                .checked_add(1)
                .ok_or_else(|| Refusal::Broken("the client's request ids ran out".to_owned()))?;
            state.next = id;
            state.calls.insert(
                id,
                Waiting {
                    answer: Taker::Then(then),
                    boxes: Vec::new(),
                    reads: false,
                    wake: None,
                },
            );
            if let Some(program) = program {
                state.programs.insert(id, program);
            }
            id
        };
        let forget = || {
            if let Ok(mut state) = shared.state() {
                state.calls.remove(&id);
                state.programs.remove(&id);
                state.given.remove(&id);
            }
        };
        let sent = wire::request(id, request, self.limits.message_bytes)
            .map_err(Refusal::from)
            .and_then(T::frame)
            .and_then(|bytes| {
                let lent = with(id)?;
                let _whole = shared.sending.lock().map_err(poisoned)?;
                let mut at = 0;
                match shared.transport.try_send(&bytes, &mut at, lent) {
                    // The agent has gone: why reaches the completion through the loop, which sees the end.
                    Ok(true) | Err(Sent::Gone) => Ok(()),
                    Ok(false) => {
                        if at > 0 {
                            shared.transport.shutdown();
                        }
                        Err(Refusal::Os {
                            op: "sending to the node agent",
                            why: "no room in its connection now, and the event form doesn't wait"
                                .to_owned(),
                        })
                    }
                    Err(Sent::Full(r) | Sent::Refused(r)) => Err(r),
                }
            });
        match sent {
            Ok(()) => Ok(id),
            Err(r) => {
                forget();
                Err(r)
            }
        }
    }

    /// The ends of exec `exec`'s streams once its start has come, taken once: those its transport made (a
    /// guest's connections) or its agent's answer gave. The event form's completion ([`Then`]) takes them
    /// here; `None` when there are none, or they were taken.
    pub fn given(&self, exec: u64) -> Option<hex_api::Given> {
        self.conn.shared.state().ok()?.given.remove(&exec)
    }

    /// The event form's release of exec `exec`'s program: its end is no longer taken, and the agent lets
    /// it run on, as dropping an in-process `Proc` does. One the connection can't carry needs none: the
    /// agent's disconnect rule has it.
    pub fn release(&self, exec: u64) {
        if let Ok(mut state) = self.conn.shared.state() {
            state.programs.remove(&exec);
        }
        if let Ok(bytes) = wire::request(exec, &Request::Release, self.limits.message_bytes) {
            self.conn.send_now(&bytes);
        }
    }

    /// The event form's kill of exec `exec`'s program: `then` hears whether the agent sent it.
    pub fn kill_then(&self, exec: u64, then: Then) -> Result<u64, Refusal> {
        self.begin(Call::Kill { exec }, Stdio::null(), then, Box::new(|_| {}))
    }

    /// The agent's bounds on this connection and its socket, as its hello told them.
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// The agent's network namespace counts (decision 4), as `hex_node::Node::netns` reads them in
    /// process.
    pub fn netns(&self) -> Result<netns::Counts, Refusal> {
        match self
            .call(&Request::Call(Call::Netns), |_| Ok(NONE), None)?
            .1
        {
            Answer::Netns(counts) => Ok(counts),
            Answer::Refused(r) => Err(r),
            other => Err(unexpected("netns", &other)),
        }
    }

    /// Sends `request` on the connection with what `with` makes for its id and lends it, and waits for its
    /// answer.
    fn call<'l>(
        &self,
        request: &Request,
        with: impl FnOnce(u64) -> Result<&'l [Lent<'l>], Refusal>,
        program: Option<Arc<Ending>>,
    ) -> Result<(u64, Answer), Refusal> {
        self.conn
            .call(self.limits.message_bytes, request, with, program)
    }

    /// A call that carries nothing beside its message.
    fn plain(&self, call: Call) -> Result<Answer, Refusal> {
        Ok(self.call(&Request::Call(call), |_| Ok(NONE), None)?.1)
    }

    /// A call whose answer is `Done`.
    fn done(&self, call: Call, name: &'static str) -> Result<(), Refusal> {
        match self.plain(call)? {
            Answer::Done => Ok(()),
            Answer::Refused(r) => Err(r),
            other => Err(unexpected(name, &other)),
        }
    }

    /// A call whose answer is a box's info.
    fn info(&self, call: Call, name: &'static str) -> Result<BoxInfo, Refusal> {
        match self.plain(call)? {
            Answer::Info(info) => Ok(info),
            Answer::Refused(r) => Err(r),
            other => Err(unexpected(name, &other)),
        }
    }
}

/// Why `answer` can't be the answer to a `call`.
pub(crate) fn unexpected(call: &str, answer: &Answer) -> Refusal {
    let kind = match answer {
        Answer::Hello(_) => "a hello",
        Answer::Info(_) => "a box's info",
        Answer::Done => "done",
        Answer::Started { .. } => "a started program",
        Answer::Usage(_) => "a usage",
        Answer::Boxes { .. } => "a list",
        Answer::Refused(_) => "a refusal",
        Answer::Exited(_) => "a program's end",
        Answer::Netns(_) => "network namespace counts",
        Answer::Given { .. } => "a started program with streams",
    };
    Refusal::Broken(format!("the node agent answered a {call} with {kind}"))
}

/// The most of a program's arguments and environment that crosses to the node agent: a program can't pass
/// the bound on one wire message (`hex_core::wire::MAX_MESSAGE`), so one that would is refused here, as a
/// program, before it is encoded; the node then holds it to its OS's own exec bound (HEX-RT-14).
pub const WIRE_EXEC_LIMIT: hex_api::ExecLimit = hex_api::ExecLimit {
    block: wire::MAX_MESSAGE,
    counts: hex_api::BlockCount::Execve,
    string: None,
    unit: hex_api::StringUnit::Bytes,
    source: "the node agent's wire (one message, hex_core::wire::MAX_MESSAGE)",
};

/// The streams an exec carries: the caller's own pipes.
fn carried(stdio: &Stdio) -> Streams {
    Streams {
        stdin: stdio.stdin.is_some(),
        stdout: stdio.stdout.is_some(),
        stderr: stdio.stderr.is_some(),
    }
}

/// The streams `ask` names.
fn streams(ask: hex_api::Ask) -> Streams {
    Streams {
        stdin: ask.stdin,
        stdout: ask.stdout,
        stderr: ask.stderr,
    }
}

/// The streams in `a` or `b`.
fn union(a: Streams, b: Streams) -> Streams {
    Streams {
        stdin: a.stdin || b.stdin,
        stdout: a.stdout || b.stdout,
        stderr: a.stderr || b.stderr,
    }
}

/// Makes the streams `made` names for exec `exec`, where its transport makes them (a guest's connections), kept
/// for its exec to take once its start comes; nothing when it names none.
fn make<T: Transport>(shared: &Shared<T>, exec: u64, made: Streams) -> Result<(), Refusal> {
    if made == Streams::default() {
        return Ok(());
    }
    let ask = hex_api::Ask {
        stdin: made.stdin,
        stdout: made.stdout,
        stderr: made.stderr,
    };
    let given = shared.transport.asks(exec, ask)?;
    shared.state()?.given.insert(exec, given);
    Ok(())
}

/// The descriptors of the pipes an exec carries, lent to its message, in the wire's order.
#[cfg(unix)]
fn lend(stdio: &Stdio) -> Vec<Lent<'_>> {
    use std::os::fd::AsFd;
    [
        stdio.stdin.as_ref().map(AsFd::as_fd),
        stdio.stdout.as_ref().map(AsFd::as_fd),
        stdio.stderr.as_ref().map(AsFd::as_fd),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(windows)]
fn lend(stdio: &Stdio) -> Vec<Lent<'_>> {
    use std::os::windows::io::AsHandle;
    [
        stdio.stdin.as_ref().map(AsHandle::as_handle),
        stdio.stdout.as_ref().map(AsHandle::as_handle),
        stdio.stderr.as_ref().map(AsHandle::as_handle),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The ends an exec's start gave, each settled: a stream the caller carried as a pipe of its own is copied between
/// that pipe and its end, here in the caller's client (the one copy left, native plan step 8); a stream it asked
/// for is the caller's to take ([`Proc::streams`]). A carried pipe whose program took it has no end, and is let go
/// here. An end of a stream the exec neither carried nor asked for is the agent's fault, refused.
fn settle(stdio: Stdio, ends: hex_api::Given) -> Result<hex_api::Given, Refusal> {
    let Stdio {
        stdin,
        stdout,
        stderr,
        ask,
    } = stdio;
    let unasked = |name: &str| {
        Refusal::Broken(format!(
            "the node agent gave {name}'s end, which its exec neither carried nor asked for"
        ))
    };
    let mut given = hex_api::Given::default();
    match (stdin, ends.stdin) {
        (Some(pipe), Some(end)) => copy_in(pipe, std::fs::File::from(end))?,
        (None, Some(end)) if ask.stdin => given.stdin = Some(end),
        (None, Some(_)) => return Err(unasked("stdin")),
        (_, None) => {}
    }
    for (pipe, end, asked, name, to) in [
        (stdout, ends.stdout, ask.stdout, "stdout", &mut given.stdout),
        (stderr, ends.stderr, ask.stderr, "stderr", &mut given.stderr),
    ] {
        match (pipe, end) {
            (Some(pipe), Some(end)) => copy(std::fs::File::from(end), pipe, drop)?,
            (None, Some(end)) if asked => *to = Some(end),
            (None, Some(_)) => return Err(unasked(name)),
            (_, None) => {}
        }
    }
    Ok(given)
}

impl<T: Transport> Runtime for Core<T> {
    fn classes(&self) -> Vec<Class> {
        self.classes.clone()
    }

    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        if let hex_api::ClassSpec::Oci(oci) = &spec.class {
            oci.init.fits(&WIRE_EXEC_LIMIT)?;
        }
        self.info(Call::Create(Box::new(spec)), "create")
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        self.done(Call::Start(id.clone()), "start")
    }

    /// The caller's own pipes ride with the exec as descriptors, which a local program takes as its streams. A
    /// stream asked for is made where the program's streams are: by the transport, where they are connections of
    /// its own (a guest's), or by the agent, which gives its end with its answer (the wire's
    /// [`Call::ExecStreams`]). A program that can't take the caller's own pipe (a guest's) has its stream's
    /// connection copied to and from that pipe here: by the transport where it makes the connection, or once the
    /// agent's answer gives its end. The one copy left is this client's, the caller's, never the node's (native
    /// plan step 8).
    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        program.fits(&WIRE_EXEC_LIMIT)?;
        stdio.check()?;
        let (carried, asked) = (carried(&stdio), streams(stdio.ask));
        let ending = Arc::new(Ending::default());
        let shared = &*self.conn.shared;
        let (exec, answer, kept) = if T::MAKES_ASKED {
            let request = Request::Call(Call::Exec {
                id: id.clone(),
                program,
                streams: union(carried, asked),
            });
            let Stdio {
                stdin,
                stdout,
                stderr,
                ask,
            } = stdio;
            let pipes = Stdio {
                stdin,
                stdout,
                stderr,
                ..Stdio::null()
            };
            let (exec, answer) = self.call(
                &request,
                |exec| {
                    make(shared, exec, asked)?;
                    shared.transport.carries(exec, pipes)?;
                    Ok(NONE)
                },
                Some(Arc::clone(&ending)),
            )?;
            let kept = Stdio {
                ask,
                ..Stdio::null()
            };
            (exec, answer, kept)
        } else {
            let request = if asked == Streams::default() {
                Request::Call(Call::Exec {
                    id: id.clone(),
                    program,
                    streams: carried,
                })
            } else {
                Request::Call(Call::ExecStreams {
                    id: id.clone(),
                    program,
                    streams: carried,
                    ask: asked,
                })
            };
            let (exec, answer) = {
                let lent = lend(&stdio);
                self.call(&request, |_| Ok(&lent[..]), Some(Arc::clone(&ending)))?
            };
            (exec, answer, stdio)
        };
        match answer {
            Answer::Started { pid } | Answer::Given { pid, .. } => {
                let ends = shared.state()?.given.remove(&exec).unwrap_or_default();
                let mut proc = RemoteProc {
                    conn: Arc::clone(&self.conn),
                    bound: self.limits.message_bytes,
                    exec,
                    pid,
                    ending,
                    exit: None,
                    given: None,
                };
                match settle(kept, ends) {
                    Ok(given) => {
                        proc.given = Some(given);
                        Ok(Box::new(proc))
                    }
                    Err(why) => {
                        let _ = proc.kill();
                        Err(why)
                    }
                }
            }
            other => {
                if let Ok(mut state) = shared.state() {
                    state.programs.remove(&exec);
                    state.given.remove(&exec);
                }
                match other {
                    Answer::Refused(r) => Err(r),
                    other => Err(unexpected("exec", &other)),
                }
            }
        }
    }

    fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal> {
        self.done(
            Call::Signal {
                id: id.clone(),
                signal,
            },
            "signal",
        )
    }

    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        self.info(Call::Inspect(id.clone()), "inspect")
    }

    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        match self.plain(Call::Usage(id.clone()))? {
            Answer::Usage(usage) => Ok(usage),
            Answer::Refused(r) => Err(r),
            other => Err(unexpected("usage", &other)),
        }
    }

    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        match self.plain(Call::List)? {
            Answer::Boxes { infos, .. } => Ok(infos),
            Answer::Refused(r) => Err(r),
            other => Err(unexpected("list", &other)),
        }
    }

    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        self.done(Call::Delete(id.clone()), "delete")
    }
}

/// A program a node agent started for this client: its end comes as a message.
struct RemoteProc<T: Transport> {
    conn: Arc<Conn<T>>,
    /// The agent's message bound, which its kill goes in.
    bound: usize,
    exec: u64,
    pid: u32,
    ending: Arc<Ending>,
    exit: Option<Exit>,
    /// The caller's ends of the streams its exec asked for, until taken.
    given: Option<hex_api::Given>,
}

impl<T: Transport> RemoteProc<T> {
    /// Its end if it has come, waiting for it at most `timeout` (none: not at all, but what has come read now): read by
    /// this wait itself while it holds the reader's turn, taken when free or handed to it, else handed to it by the
    /// thread that reads; the turn handed on as it goes.
    fn wait(&mut self, timeout: Option<Duration>) -> Result<Option<Exit>, Refusal> {
        if let Some(exit) = self.exit {
            return Ok(Some(exit));
        }
        let shared = Arc::clone(&self.conn.shared);
        let deadline = timeout.map(|t| Instant::now().checked_add(t));
        shared.state()?.waiting.insert(self.exec);
        let mut mine = false;
        // Every way out of the loop goes through its end, so the wait is never left registered.
        let got = loop {
            {
                let mut ended = match self.ending.state.lock() {
                    Ok(ended) => ended,
                    Err(e) => break Err(poisoned(e)),
                };
                match &ended.exit {
                    Some(Ok(exit)) => break Ok(Some(*exit)),
                    Some(Err(why)) => break Err(why.clone()),
                    None => {}
                }
                if ended.turn {
                    ended.turn = false;
                    mine = true;
                }
            }
            if !mine {
                let mut state = match shared.state() {
                    Ok(state) => state,
                    Err(why) => break Err(why),
                };
                if !state.reading && state.closed.is_none() {
                    state.reading = true;
                    mine = true;
                }
            }
            let left = match deadline {
                None => Duration::ZERO,
                // A timeout past what an instant holds waits as long as it can at a time.
                Some(None) => timeout.unwrap_or(Duration::ZERO),
                Some(Some(deadline)) => deadline.saturating_duration_since(Instant::now()),
            };
            if mine {
                match shared.turn(Some(left)) {
                    Ok(true) => continue,
                    Ok(false) if !left.is_zero() => continue,
                    // Waited out: what came is its end, or none.
                    Ok(false) => {
                        let ended = match self.ending.state.lock() {
                            Ok(ended) => ended,
                            Err(e) => break Err(poisoned(e)),
                        };
                        break match &ended.exit {
                            Some(Ok(exit)) => Ok(Some(*exit)),
                            Some(Err(why)) => Err(why.clone()),
                            None => Ok(None),
                        };
                    }
                    // The connection ended: its `Proc`s were told, this one among them.
                    Err(_) => mine = false,
                }
            } else {
                if left.is_zero() {
                    break Ok(None);
                }
                let ended = match self.ending.state.lock() {
                    Ok(ended) => ended,
                    Err(e) => break Err(poisoned(e)),
                };
                if ended.exit.is_none()
                    && !ended.turn
                    && let Err(e) = self.ending.came.wait_timeout(ended, left)
                {
                    break Err(poisoned(e));
                }
            }
        };
        // Going: no longer waiting, and the turn it holds, or was handed as it went, given to another.
        if let Ok(mut state) = shared.state() {
            state.waiting.remove(&self.exec);
            let handed = self
                .ending
                .state
                .lock()
                .is_ok_and(|mut ended| std::mem::take(&mut ended.turn));
            if (mine || handed) && state.closed.is_none() {
                state.pass_turn();
            }
        }
        if let Ok(Some(exit)) = got {
            self.exit = Some(exit);
        }
        got
    }
}

impl<T: Transport> Proc for RemoteProc<T> {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn try_wait(&mut self) -> Result<Option<Exit>, Refusal> {
        self.wait(None)
    }

    fn wait_for(&mut self, timeout: Duration) -> Result<Option<Exit>, Refusal> {
        self.wait(Some(timeout))
    }

    /// A kill request naming its exec, answered once the agent has sent KILL; its end then comes as any
    /// end does.
    fn kill(&mut self) -> Result<(), Refusal> {
        if self.wait(None)?.is_some() {
            return Ok(());
        }
        let kill = Request::Call(Call::Kill { exec: self.exec });
        match self.conn.call(self.bound, &kill, |_| Ok(NONE), None)?.1 {
            Answer::Done => Ok(()),
            Answer::Refused(r) => Err(r),
            other => Err(unexpected("kill", &other)),
        }
    }

    fn streams(&mut self) -> Result<hex_api::Given, Refusal> {
        Ok(self.given.take().unwrap_or_default())
    }
}

impl<T: Transport> Drop for RemoteProc<T> {
    /// Let go before its end came: the agent is told, and lets the program run on as dropping an
    /// in-process `Proc` does. A release that can't be sent now (no room, or the connection gone) needs none: the
    /// agent's disconnect rule has the program.
    fn drop(&mut self) {
        let shared = &self.conn.shared;
        let ended = self.exit.is_some()
            || self
                .ending
                .state
                .lock()
                .map(|ended| ended.exit.is_some())
                .unwrap_or(true);
        if let Ok(mut state) = shared.state() {
            state.programs.remove(&self.exec);
        }
        if !ended && let Ok(bytes) = wire::request(self.exec, &Request::Release, wire::MAX_MESSAGE)
        {
            self.conn.send_now(&bytes);
        }
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
impl<T: Transport> Core<T> {
    /// The connection's shared state, for a test of a transport to reach its socket.
    pub(crate) fn shared(&self) -> &Shared<T> {
        &self.conn.shared
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
impl<T: Transport> Shared<T> {
    /// How many calls wait for their answers.
    #[cfg(target_os = "linux")]
    pub(crate) fn calls_waiting(&self) -> usize {
        self.state().map_or(0, |state| state.calls.len())
    }

    /// What `f` makes of the reader's read state, between the turn's reads.
    #[cfg(target_os = "macos")]
    pub(crate) fn with_reading<R>(&self, f: impl FnOnce(&T::Reading) -> R) -> R {
        f(&self.reading.lock().unwrap())
    }

    /// A reading thread's step, as the event form's loop reads: one wait on the connection with no bound, then what
    /// came handed on. For a test that sets this core beside a client with a reading thread of its own.
    #[cfg(target_os = "linux")]
    pub(crate) fn read_as_a_thread_would(&self) -> Result<bool, Refusal> {
        self.turn(None)
    }
}
