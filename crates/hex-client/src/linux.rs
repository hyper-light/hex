//! The client on Linux: one `SOCK_SEQPACKET` connection and calls that send their request (with an exec's stdio as
//! `SCM_RIGHTS` descriptors) and wait for the answer, at most twice the slowest answer this process has had to a call of
//! its kind ([`ANSWERS`]). No thread of its own reads the connection: whoever waits on it does, one at a time, the
//! reader's turn (a call for its answer or for room to send in, a `Proc` for its program's end), handing each message
//! to whoever it is for and the turn to another still waiting as it goes, so an answer wakes the thread it answers and
//! no thread between (goal 16: the socket's per-call cost; a reading thread of the client's woke a second thread for
//! every answer).

use std::collections::{BTreeMap, BTreeSet};
use std::io::{IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use hex_api::{
    BoxId, BoxInfo, BoxSpec, Class, Exit, Proc, Program, Refusal, Runtime, Signal, Stdio, Usage,
};
use hex_core::netns;
use hex_core::wire::{self, Answer, Call, Header, Limits, Received, Request, Streams};
use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec};
use rustix::io::Errno;
use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags,
    SendAncillaryBuffer, SendAncillaryMessage, SendFlags, Shutdown, SocketAddrUnix, SocketFlags,
    SocketType,
};

use hex_core::patience::Patience;

use crate::MAX_LIST;

/// How long a call waits for the node agent's answer, and its send for room in the agent's socket, one wait a kind
/// of call, since a delete waits on its box's emptying and a list on its parts, so a fast inspect's answer says
/// nothing of theirs: twice the slowest answer this process has had to a call of that kind (decision 9; the lead's
/// failure-bound rule of 2026-11-06), past which the call is refused by type, the only way left to it. Each kind is
/// seeded by build profile from its `client.answer.<kind>` rows of `bench/competitive/probes/waits_census.md`, in
/// nanoseconds as the census keeps them: a release build from run 4's largest max of the release arms with a row's 1,000
/// samples (adacachy2), a debug build from run 5's largest answer of the debug-build arms, the debug arm and the suites'
/// (adacachy2), for the kind and the tests' waits seeded from it, never under a bound one ran out at (decision 9).
static ANSWERS: [Patience; 10] = [
    Patience::by_profile(
        "client.answer.hello",
        Duration::from_nanos(1_177_037),
        Some(Duration::from_nanos(17_232_102)),
    ),
    Patience::by_profile(
        "client.answer.create",
        Duration::from_nanos(43_655_221),
        Some(Duration::from_nanos(49_798_121)),
    ),
    Patience::by_profile(
        "client.answer.start",
        Duration::from_nanos(11_263_989),
        Some(Duration::from_nanos(16_120_605)),
    ),
    Patience::by_profile(
        "client.answer.exec",
        Duration::from_nanos(17_330_544),
        Some(Duration::from_nanos(100_780_659)),
    ),
    Patience::by_profile(
        "client.answer.signal",
        Duration::from_nanos(583_198),
        Some(Duration::from_nanos(3_952_426)),
    ),
    Patience::by_profile(
        "client.answer.inspect",
        Duration::from_nanos(10_914_126),
        Some(Duration::from_nanos(12_007_514)),
    ),
    Patience::by_profile(
        "client.answer.usage",
        Duration::from_nanos(757_183),
        Some(Duration::from_nanos(386_833)),
    ),
    Patience::by_profile(
        "client.answer.list",
        Duration::from_nanos(386_711_080),
        Some(Duration::from_nanos(868_709_473)),
    ),
    Patience::by_profile(
        "client.answer.delete",
        Duration::from_nanos(35_715_197),
        Some(Duration::from_nanos(706_090_511)),
    ),
    Patience::by_profile(
        "client.answer.netns",
        Duration::from_nanos(715_105),
        Some(Duration::from_nanos(2_733_541)),
    ),
];

/// The wait for `request`'s answer, `None` for one that has none (a release).
fn answers(request: &Request) -> Option<&'static Patience> {
    let n = match request {
        Request::Hello => 0,
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
        },
        Request::Release => return None,
    };
    ANSWERS.get(n)
}

/// A node agent, reached through its socket.
pub struct Client {
    conn: Arc<Conn>,
    classes: Vec<Class>,
    limits: Limits,
    /// The wire version this client writes: [`wire::FIRST`] for its hello, then the highest both it
    /// and its agent speak.
    version: u16,
}

/// The connection, held by the client and by each `Proc` it returned: it closes when the last goes.
struct Conn {
    shared: Arc<Shared>,
}

/// What the callers and the `Proc`s share: the socket, what waits on it, and the buffer the reader's turn reads into.
struct Shared {
    fd: OwnedFd,
    state: Mutex<State>,
    /// One message at a time, the turn's holder's.
    buf: Mutex<Vec<u8>>,
}

#[derive(Default)]
struct State {
    /// Calls waiting for their answers, by request id.
    calls: BTreeMap<u64, Waiting>,
    /// Programs whose end a `Proc` waits for, by their exec's id.
    programs: BTreeMap<u64, Arc<Ending>>,
    next: u64,
    /// Why the connection ended, once it has.
    closed: Option<Refusal>,
    /// Why the agent won't serve the connection, when it said (an answer to no request, id 0).
    refused: Option<Refusal>,
    /// Whether a thread holds the reader's turn: it reads the socket and hands each message to whoever it is for.
    reading: bool,
    /// The execs whose `Proc` is in its wait, which the turn may be handed to.
    waiting: BTreeSet<u64>,
    /// An agent that closed the connection with a request of ours unread leaves a reset, reported once and before what
    /// it sent (net/unix/af_unix.c, `unix_release_sock`): its last answers, a refusal of the connection among them, are
    /// still there to read, and the reset is why the connection ended once they are.
    reset: Option<Refusal>,
}

impl State {
    /// The reader's turn given up by its holder: handed to the oldest call still waiting, else to a `Proc` in its wait,
    /// else free, so while anyone waits on the connection someone reads it.
    fn pass_turn(&mut self) {
        for waiting in self.calls.values() {
            if waiting.mail.try_send(Mail::Turn).is_ok() {
                if let Some(fd) = &waiting.wake {
                    wake(fd);
                }
                return;
            }
        }
        for exec in &self.waiting {
            if let Some(ending) = self.programs.get(exec)
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

/// What a waiting call is handed: its answer, or the reader's turn. The answer is boxed, so a turn is no bigger than a
/// pointer.
enum Mail {
    Answer(Box<Result<Answer, Refusal>>),
    Turn,
}

struct Waiting {
    mail: SyncSender<Mail>,
    /// A list's boxes from the parts that came so far.
    boxes: Vec<BoxInfo>,
    /// While its send waits for room with the reader's turn another's: the eventfd it waits on besides the socket,
    /// written as the turn is handed to it or the connection ends.
    wake: Option<Arc<OwnedFd>>,
}

/// Wakes a send waiting for room ([`Waiting::wake`]).
fn wake(fd: &OwnedFd) {
    // An eventfd's count can't overflow from writes of one, one a turn or an end.
    let _ = rustix::io::write(fd, &1u64.to_ne_bytes());
}

/// A program's end, once it has come, and its `Proc`'s waking: by its end, or by the reader's turn handed to its wait.
#[derive(Default)]
struct Ending {
    state: Mutex<Ended>,
    came: Condvar,
}

#[derive(Default)]
struct Ended {
    exit: Option<Result<Exit, Refusal>>,
    turn: bool,
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> Refusal {
    Refusal::Broken("a lock of the node agent's client was poisoned".to_owned())
}

/// `wait` as poll(2) takes it.
fn timespec(wait: Duration) -> Timespec {
    Timespec {
        tv_sec: i64::try_from(wait.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: i64::from(wait.subsec_nanos()),
    }
}

impl Shared {
    fn state(&self) -> Result<MutexGuard<'_, State>, Refusal> {
        self.state.lock().map_err(poisoned)
    }

    /// Hands one message to whoever waits for it: an answer to its call, a program's end to its
    /// `Proc`. One nobody waits for (a call that gave up waiting, a program released) has no taker.
    fn deliver(&self, header: Header, answer: Answer) -> Result<(), Refusal> {
        let mut state = self.state()?;
        match answer {
            Answer::Exited(exit) => {
                if let Some(ending) = state.programs.get(&header.id) {
                    ending.state.lock().map_err(poisoned)?.exit = Some(Ok(exit));
                    ending.came.notify_all();
                }
            }
            Answer::Refused(refusal) if header.id == 0 => state.refused = Some(refusal),
            Answer::Boxes { infos, more: true } => {
                if let Some(waiting) = state.calls.get_mut(&header.id) {
                    if waiting.boxes.len().saturating_add(infos.len()) > MAX_LIST {
                        let waiting = state.calls.remove(&header.id);
                        if let Some(w) = waiting {
                            let _ =
                                w.mail
                                    .try_send(Mail::Answer(Box::new(Err(Refusal::Exhausted {
                                        what: "boxes in one list",
                                        bound: MAX_LIST,
                                    }))));
                        }
                    } else {
                        waiting.boxes.extend(infos);
                    }
                }
            }
            answer => {
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
                    let _ = waiting.mail.try_send(Mail::Answer(Box::new(Ok(answer))));
                }
            }
        }
        Ok(())
    }

    /// The connection has ended: every call waiting and every program's `Proc` hear why, and no one reads any more.
    fn close(&self, why: Refusal) {
        let Ok(mut state) = self.state() else {
            return;
        };
        let why = state.refused.take().unwrap_or(why);
        for (_, waiting) in std::mem::take(&mut state.calls) {
            let _ = waiting
                .mail
                .try_send(Mail::Answer(Box::new(Err(why.clone()))));
            if let Some(fd) = &waiting.wake {
                wake(fd);
            }
        }
        let lost = Refusal::Broken(format!(
            "the connection to the node agent closed before the program's end came: {why}"
        ));
        for ending in state.programs.values() {
            if let Ok(mut ended) = ending.state.lock()
                && ended.exit.is_none()
            {
                ended.exit = Some(Err(lost.clone()));
                ending.came.notify_all();
            }
        }
        state.closed = Some(why);
        state.reading = false;
    }

    /// The connection ended for `why`: everyone waiting told ([`Shared::close`]), and why, as each is told.
    fn end(&self, why: Refusal) -> Refusal {
        self.close(why.clone());
        self.state()
            .ok()
            .and_then(|state| state.closed.clone())
            .unwrap_or(why)
    }

    /// One read by the turn's holder: a message waited for at most `wait` (none: as long as it takes) and handed to
    /// whoever it is for ([`Shared::recv_one`]).
    fn read_one(&self, wait: Option<Duration>) -> Result<bool, Refusal> {
        let mut ready = [PollFd::new(&self.fd, PollFlags::IN)];
        match rustix::event::poll(&mut ready, wait.map(timespec).as_ref()) {
            Ok(0) | Err(Errno::INTR) => Ok(false),
            Ok(_) => self.recv_one(),
            Err(e) => Err(self.end(Refusal::Broken(format!(
                "waiting on the node agent's socket: {e}"
            )))),
        }
    }

    /// The message waiting, if one is, read by the turn's holder and handed to whoever it is for. Says whether one
    /// was; the connection's end is the error, everyone waiting told why.
    fn recv_one(&self) -> Result<bool, Refusal> {
        // A lock poisoned mid-read leaves the connection's messages unknown: it ends, everyone waiting told.
        let mut buf = match self.buf.lock() {
            Ok(buf) => buf,
            Err(e) => return Err(self.end(poisoned(e))),
        };
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
        let mut control = RecvAncillaryBuffer::new(&mut space);
        let msg = match rustix::net::recvmsg(
            &self.fd,
            &mut [IoSliceMut::new(&mut buf)],
            &mut control,
            RecvFlags::CMSG_CLOEXEC | RecvFlags::TRUNC | RecvFlags::DONTWAIT,
        ) {
            Ok(msg) => msg,
            Err(Errno::INTR | Errno::AGAIN) => return Ok(false),
            Err(Errno::CONNRESET) => {
                let mut state = match self.state() {
                    Ok(state) => state,
                    Err(why) => return Err(self.end(why)),
                };
                if state.reset.is_none() {
                    state.reset = Some(Refusal::Broken(
                        "the node agent closed the connection with requests unread".to_owned(),
                    ));
                    return Ok(true);
                }
                drop(state);
                return Err(self.end(Refusal::Broken(format!(
                    "reading the node agent's socket: {}",
                    Errno::CONNRESET
                ))));
            }
            Err(e) => {
                return Err(self.end(Refusal::Broken(format!(
                    "reading the node agent's socket: {e}"
                ))));
            }
        };
        // An answer carries no descriptors: any that came are counted, closed, and refused below.
        let descriptors: usize = control
            .drain()
            .map(|m| match m {
                RecvAncillaryMessage::ScmRights(fds) => fds.count(),
                _ => 0,
            })
            .sum();
        if msg.bytes == 0 && descriptors == 0 {
            let reset = self.state().ok().and_then(|mut state| state.reset.take());
            return Err(self.end(reset.unwrap_or_else(|| {
                Refusal::Broken("the node agent closed the connection".to_owned())
            })));
        }
        let received = Received {
            bytes: buf.get(..msg.bytes.min(buf.len())).unwrap_or_default(),
            size: msg.bytes,
            descriptors,
            descriptors_cut: msg.flags.contains(ReturnFlags::CTRUNC),
        };
        let delivered = wire::read_answer(&received, wire::MAX_MESSAGE)
            .map_err(|fault| {
                Refusal::Broken(format!(
                    "the node agent sent a message that isn't an answer: {fault}"
                ))
            })
            .and_then(|(header, answer)| self.deliver(header, answer));
        match delivered {
            Ok(()) => Ok(true),
            Err(why) => Err(self.end(why)),
        }
    }
}

/// A call while it is made: its place among the calls, its mailbox, its answer once come, and whether it holds the
/// reader's turn. However it goes it hands on a turn it holds or was handed, and forgets the call if unanswered.
struct Caller<'a> {
    shared: &'a Shared,
    id: u64,
    mail: Receiver<Mail>,
    /// Whether it holds the reader's turn.
    mine: bool,
    /// Its answer, come to its mailbox and not yet returned.
    answer: Option<Result<Answer, Refusal>>,
    /// Answered: an exec's place for its program's end is then its `Proc`'s.
    answered: bool,
}

impl<'a> Caller<'a> {
    /// A call's place among the calls, with `program`, an exec's place for the end of the program it starts, made
    /// before the exec goes, so the end can't come first.
    fn new(shared: &'a Shared, program: Option<Arc<Ending>>) -> Result<Self, Refusal> {
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
                mail: tx,
                boxes: Vec::new(),
                wake: None,
            },
        );
        if let Some(program) = program {
            state.programs.insert(id, program);
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
            match mail {
                Mail::Turn => self.mine = true,
                Mail::Answer(answer) => self.answer = Some(*answer),
            }
        }
    }

    /// Sends `bytes` with `fds`, waiting for room in the agent's socket until `deadline` (none: as long as it takes),
    /// its call's learned wait (`patience`, named in a refusal). The node agent reads no request while its answers
    /// wait for room in the client's socket, so a send waiting for room has someone reading: itself while it holds the
    /// reader's turn, which it takes when free or is handed, reading what comes as it waits; else the turn's holder,
    /// which hands the turn on to it, waking it, as it goes.
    fn send(
        &mut self,
        bytes: &[u8],
        fds: &[OwnedFd],
        deadline: Option<Instant>,
        patience: Option<Duration>,
    ) -> Result<(), Sent> {
        let borrowed: Vec<BorrowedFd<'_>> = fds.iter().map(AsFd::as_fd).collect();
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        if !borrowed.is_empty() && !control.push(SendAncillaryMessage::ScmRights(&borrowed)) {
            return Err(Sent::Refused(Refusal::Invalid {
                what: "stdio",
                why: format!(
                    "{} descriptors, past the {} a message carries",
                    borrowed.len(),
                    wire::MAX_FDS
                ),
            }));
        }
        let shared = self.shared;
        // Made the first time the send waits with the turn another's, so a send that finds room pays nothing for it.
        let mut woken: Option<Arc<OwnedFd>> = None;
        let sent = loop {
            match send_now(&shared.fd, bytes, &mut control) {
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
            let timeout = left.map(timespec);
            match self.turn_for_room(&mut woken) {
                Err(e) => break Err(e),
                // The turn its: what the agent sends read as it comes, until there's room.
                Ok(None) => {
                    let mut ready = [PollFd::new(&shared.fd, PollFlags::IN | PollFlags::OUT)];
                    match rustix::event::poll(&mut ready, timeout.as_ref()) {
                        Ok(_) | Err(Errno::INTR) => {}
                        Err(e) => {
                            break Err(Sent::Refused(Refusal::os(
                                "waiting to send to the node agent",
                                e,
                            )));
                        }
                    }
                    let [sock] = &ready;
                    if sock
                        .revents()
                        .intersects(PollFlags::IN | PollFlags::ERR | PollFlags::HUP)
                        && shared.recv_one().is_err()
                    {
                        // The connection ended: every call waiting was told why, this one among them.
                        self.mine = false;
                        break Err(Sent::Gone);
                    }
                }
                // The turn another's: room waited for, and the turn, should it be handed to this call.
                Ok(Some(turn)) => {
                    let mut ready = [
                        PollFd::new(&shared.fd, PollFlags::OUT),
                        PollFd::new(turn.as_ref(), PollFlags::IN),
                    ];
                    match rustix::event::poll(&mut ready, timeout.as_ref()) {
                        Ok(_) | Err(Errno::INTR) => {}
                        Err(e) => {
                            break Err(Sent::Refused(Refusal::os(
                                "waiting to send to the node agent",
                                e,
                            )));
                        }
                    }
                    let [_, handed] = &ready;
                    if handed.revents().contains(PollFlags::IN) {
                        // Its count read, the eventfd wakes no one until it is written again.
                        let _ = rustix::io::read(turn.as_ref(), &mut [0u8; 8]);
                    }
                }
            }
        };
        if woken.is_some()
            && let Ok(mut state) = shared.state()
            && let Some(waiting) = state.calls.get_mut(&self.id)
        {
            waiting.wake = None;
        }
        sent
    }

    /// The reader's turn for a send waiting for room: taken if it was handed to this call or is free (`None`), else
    /// the eventfd that wakes the send should it be handed the turn, made once (`woken`) and kept with the call, where
    /// the holder handing the turn on finds it. The connection's end is [`Sent::Gone`], its why the call's answer.
    fn turn_for_room(
        &mut self,
        woken: &mut Option<Arc<OwnedFd>>,
    ) -> Result<Option<Arc<OwnedFd>>, Sent> {
        let shared = self.shared;
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
            return Ok(None);
        }
        let turn = match woken {
            Some(turn) => Arc::clone(turn),
            None => {
                let made =
                    rustix::event::eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)
                        .map_err(|e| {
                            Sent::Refused(Refusal::os(
                                "making the eventfd a send waits on for the reader's turn",
                                e,
                            ))
                        })?;
                Arc::clone(woken.insert(Arc::new(made)))
            }
        };
        if let Some(waiting) = state.calls.get_mut(&self.id) {
            waiting.wake = Some(Arc::clone(&turn));
        }
        Ok(Some(turn))
    }

    /// Its answer, waited for until `deadline` (none: as long as it takes): read by this thread while it holds the
    /// reader's turn, taken when free or handed to it, else handed to it by the thread that holds the turn. `None`
    /// once the deadline has passed.
    fn wait(&mut self, deadline: Option<Instant>) -> Option<Result<Answer, Refusal>> {
        let shared = self.shared;
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
                if shared.read_one(left).is_err() {
                    // The connection ended: every call waiting was told why, this one among them.
                    self.mine = false;
                }
            } else {
                match self.mail.recv_timeout(left.unwrap_or(Duration::MAX)) {
                    Ok(Mail::Turn) => self.mine = true,
                    Ok(Mail::Answer(answer)) => self.answer = Some(*answer),
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

impl Drop for Caller<'_> {
    /// Going: the call forgotten if unanswered, and the reader's turn it holds, or was handed as it went, handed on.
    fn drop(&mut self) {
        let shared = self.shared;
        let Ok(mut state) = shared.state() else {
            return;
        };
        if !self.answered {
            state.calls.remove(&self.id);
            state.programs.remove(&self.id);
        }
        self.collect();
        if self.mine && state.closed.is_none() {
            state.pass_turn();
        }
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        // The agent sees the hang-up.
        let _ = rustix::net::shutdown(&self.shared.fd, Shutdown::Both);
    }
}

impl Client {
    /// Connects to the node agent at `path` and learns what it runs and its bounds.
    pub fn connect(path: &Path) -> Result<Self, Refusal> {
        let addr = SocketAddrUnix::new(path).map_err(|e| Refusal::Invalid {
            what: "socket path",
            why: format!("{}: {e}", path.display()),
        })?;
        let fd = rustix::net::socket_with(
            AddressFamily::UNIX,
            SocketType::SEQPACKET,
            SocketFlags::CLOEXEC,
            None,
        )
        .map_err(|e| Refusal::os("making a socket", e))?;
        // A connect to a socket the agent listens on doesn't wait for it to accept (unix(7)); each call's send
        // is bounded by its kind's learned wait.
        rustix::net::connect(&fd, &addr).map_err(|e| Refusal::Os {
            op: "connecting to the node agent's socket",
            why: format!("{}: {e}", path.display()),
        })?;
        let shared = Arc::new(Shared {
            fd,
            state: Mutex::new(State::default()),
            buf: Mutex::new(vec![0u8; wire::MAX_MESSAGE]),
        });
        let mut client = Self {
            conn: Arc::new(Conn { shared }),
            classes: Vec::new(),
            limits: Limits {
                connections: 1,
                message_bytes: wire::MAX_MESSAGE,
                in_flight: 1,
                programs: 0,
            },
            // The hello goes in the first version, which an agent of any version reads.
            version: wire::FIRST,
        };
        match client.call(&Request::Hello, Vec::new(), None)?.1 {
            Answer::Hello(hello) if hello.version < wire::FIRST => Err(Refusal::Broken(format!(
                "the node agent says it speaks wire version {}, below the first",
                hello.version
            ))),
            Answer::Hello(hello) => {
                client.classes = hello.classes;
                client.limits = Limits {
                    message_bytes: hello.limits.message_bytes.min(wire::MAX_MESSAGE),
                    ..hello.limits
                };
                client.version = hello.version.min(wire::VERSION);
                Ok(client)
            }
            Answer::Refused(r) => Err(r),
            other => Err(unexpected("hello", &other)),
        }
    }

    /// The agent's bounds on this connection and its socket, as its hello told them.
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// The agent's network namespace counts (decision 4), as [`hex_node::Node::netns`] reads them in
    /// process. An agent of wire version 1 has none to give: the call is refused by type before it is
    /// sent, as a kind version 1 doesn't define.
    pub fn netns(&self) -> Result<netns::Counts, Refusal> {
        match self.call(&Request::Call(Call::Netns), Vec::new(), None)?.1 {
            Answer::Netns(counts) => Ok(counts),
            Answer::Refused(r) => Err(r),
            other => Err(unexpected("netns", &other)),
        }
    }

    /// Sends `request` with `fds`, closing this side's copies of them once sent, and waits for its answer
    /// ([`Caller`]). With `program`, a `Proc`'s place for the end of the program an exec starts is made before the exec
    /// goes, so the end can't come first.
    fn call(
        &self,
        request: &Request,
        fds: Vec<OwnedFd>,
        program: Option<Arc<Ending>>,
    ) -> Result<(u64, Answer), Refusal> {
        let mut caller = Caller::new(&self.conn.shared, program)?;
        let waits = answers(request);
        let patience = waits.and_then(Patience::deadline);
        let asked = Instant::now();
        let deadline = patience.and_then(|p| asked.checked_add(p));
        let sent = wire::request(self.version, caller.id, request, self.limits.message_bytes)
            .map_err(|fault| Sent::Refused(fault.into()))
            .and_then(|bytes| caller.send(&bytes, &fds, deadline, patience));
        drop(fds);
        match sent {
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

    /// A call whose answer is `Done`.
    fn done(&self, call: Call, name: &'static str) -> Result<(), Refusal> {
        match self.call(&Request::Call(call), Vec::new(), None)?.1 {
            Answer::Done => Ok(()),
            Answer::Refused(r) => Err(r),
            other => Err(unexpected(name, &other)),
        }
    }

    /// A call whose answer is a box's info.
    fn info(&self, call: Call, name: &'static str) -> Result<BoxInfo, Refusal> {
        match self.call(&Request::Call(call), Vec::new(), None)?.1 {
            Answer::Info(info) => Ok(info),
            Answer::Refused(r) => Err(r),
            other => Err(unexpected(name, &other)),
        }
    }
}

/// Why a send didn't go.
enum Sent {
    /// The connection is gone: the call's wait says why.
    Gone,
    /// No room in the agent's socket in all the call's patience: the agent isn't reading.
    Full(Refusal),
    Refused(Refusal),
}

/// One send of `bytes` with `control`'s descriptors, waiting for nothing: whether it went, or found no room now.
fn send_now(
    fd: &OwnedFd,
    bytes: &[u8],
    control: &mut SendAncillaryBuffer<'_, '_, '_>,
) -> Result<bool, Sent> {
    loop {
        match rustix::net::sendmsg(
            fd,
            &[IoSlice::new(bytes)],
            control,
            SendFlags::NOSIGNAL | SendFlags::DONTWAIT,
        ) {
            Ok(_) => return Ok(true),
            Err(Errno::INTR) => {}
            Err(Errno::AGAIN) => return Ok(false),
            Err(Errno::PIPE | Errno::CONNRESET | Errno::NOTCONN) => return Err(Sent::Gone),
            Err(e) => return Err(Sent::Refused(Refusal::os("sending to the node agent", e))),
        }
    }
}

fn unexpected(call: &str, answer: &Answer) -> Refusal {
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
    };
    Refusal::Broken(format!("the node agent answered a {call} with {kind}"))
}

impl Runtime for Client {
    fn classes(&self) -> Vec<Class> {
        self.classes.clone()
    }

    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        self.info(Call::Create(Box::new(spec)), "create")
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        self.done(Call::Start(id.clone()), "start")
    }

    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        let streams = Streams {
            stdin: stdio.stdin.is_some(),
            stdout: stdio.stdout.is_some(),
            stderr: stdio.stderr.is_some(),
        };
        let fds: Vec<OwnedFd> = [
            stdio.stdin.map(OwnedFd::from),
            stdio.stdout.map(OwnedFd::from),
            stdio.stderr.map(OwnedFd::from),
        ]
        .into_iter()
        .flatten()
        .collect();
        let ending = Arc::new(Ending::default());
        let request = Request::Call(Call::Exec {
            id: id.clone(),
            program,
            streams,
        });
        let (exec, answer) = self.call(&request, fds, Some(Arc::clone(&ending)))?;
        match answer {
            Answer::Started { pid } => Ok(Box::new(RemoteProc {
                conn: Arc::clone(&self.conn),
                version: self.version,
                exec,
                pid,
                ending,
                exit: None,
            })),
            other => {
                if let Ok(mut state) = self.conn.shared.state() {
                    state.programs.remove(&exec);
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
        match self
            .call(&Request::Call(Call::Usage(id.clone())), Vec::new(), None)?
            .1
        {
            Answer::Usage(usage) => Ok(usage),
            Answer::Refused(r) => Err(r),
            other => Err(unexpected("usage", &other)),
        }
    }

    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        match self.call(&Request::Call(Call::List), Vec::new(), None)?.1 {
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
struct RemoteProc {
    conn: Arc<Conn>,
    /// The wire version its client writes, which its release goes in.
    version: u16,
    exec: u64,
    pid: u32,
    ending: Arc<Ending>,
    exit: Option<Exit>,
}

impl RemoteProc {
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
                match shared.read_one(Some(left)) {
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

impl Proc for RemoteProc {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn try_wait(&mut self) -> Result<Option<Exit>, Refusal> {
        self.wait(None)
    }

    fn wait_for(&mut self, timeout: Duration) -> Result<Option<Exit>, Refusal> {
        self.wait(Some(timeout))
    }
}

impl Drop for RemoteProc {
    /// Let go before its end came: the agent is told, and lets the program run on as dropping an
    /// in-process `Proc` does. A release that can't be sent (the connection gone) needs none: the
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
        if !ended
            && let Ok(bytes) = wire::request(
                self.version,
                self.exec,
                &Request::Release,
                wire::MAX_MESSAGE,
            )
        {
            // Now or not at all: a release that finds no room is the agent's disconnect rule's to settle.
            let _ = send_now(&shared.fd, &bytes, &mut SendAncillaryBuffer::default());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::event::{PollFd, PollFlags, Timespec};

    /// A stand-in agent's listening socket, in a directory of its own.
    fn agent_socket() -> (tempfile::TempDir, std::path::PathBuf, OwnedFd) {
        let cache = std::path::PathBuf::from(std::env::var_os("HOME").unwrap())
            .join(".cache/hex-t7/client");
        std::fs::create_dir_all(&cache).unwrap();
        let dir = tempfile::Builder::new()
            .prefix("agent-")
            .tempdir_in(&cache)
            .unwrap();
        let path = dir.path().join("node.sock");
        let listener = rustix::net::socket_with(
            AddressFamily::UNIX,
            SocketType::SEQPACKET,
            SocketFlags::CLOEXEC,
            None,
        )
        .unwrap();
        rustix::net::bind(&listener, &SocketAddrUnix::new(&path).unwrap()).unwrap();
        rustix::net::listen(&listener, 1).unwrap();
        (dir, path, listener)
    }

    /// The next request on `conn`, waiting at most 30 s for it.
    fn next_request(conn: &OwnedFd) -> (Header, Request) {
        let mut ready = [PollFd::new(conn, PollFlags::IN)];
        let ts = Timespec {
            tv_sec: 30,
            tv_nsec: 0,
        };
        assert_eq!(rustix::event::poll(&mut ready, Some(&ts)).unwrap(), 1);
        let mut buf = vec![0u8; wire::MAX_MESSAGE];
        let (n, _) = rustix::net::recv(conn, &mut buf, RecvFlags::empty()).unwrap();
        let msg = Received {
            bytes: &buf[..n],
            size: n,
            descriptors: 0,
            descriptors_cut: false,
        };
        wire::read_request(&msg, wire::MAX_MESSAGE).unwrap()
    }

    /// A stand-in agent's answer to a hello: version 4, a process class, room for `in_flight` calls at once.
    fn hello(conn: &OwnedFd, in_flight: usize) {
        let (h, request) = next_request(conn);
        assert_eq!(request, Request::Hello);
        let answer = Answer::Hello(wire::Hello {
            version: wire::VERSION,
            classes: vec![Class::Process],
            limits: Limits {
                connections: 1,
                message_bytes: wire::MAX_MESSAGE,
                in_flight,
                programs: 1,
            },
            budgets: None,
        });
        let bytes = wire::answer(wire::FIRST, h.id, &answer, wire::MAX_MESSAGE).unwrap();
        rustix::net::send(conn, &bytes, SendFlags::NOSIGNAL).unwrap();
    }

    /// Sends a stand-in agent's `answer` to request `id`.
    fn answer(conn: &OwnedFd, id: u64, answer: &Answer) {
        let bytes = wire::answer(wire::VERSION, id, answer, wire::MAX_MESSAGE).unwrap();
        rustix::net::send(conn, &bytes, SendFlags::NOSIGNAL).unwrap();
    }

    /// The request on `conn` that is next to be read, left there unread, waiting at most 30 s for it.
    fn peek_request(conn: &OwnedFd) -> (Header, Request) {
        let mut ready = [PollFd::new(conn, PollFlags::IN)];
        let ts = Timespec {
            tv_sec: 30,
            tv_nsec: 0,
        };
        assert_eq!(rustix::event::poll(&mut ready, Some(&ts)).unwrap(), 1);
        let mut buf = vec![0u8; wire::MAX_MESSAGE];
        let (n, _) = rustix::net::recv(conn, &mut buf, RecvFlags::PEEK).unwrap();
        let msg = Received {
            bytes: &buf[..n],
            size: n,
            descriptors: 0,
            descriptors_cut: false,
        };
        wire::read_request(&msg, wire::MAX_MESSAGE).unwrap()
    }

    /// A program whose exec's request fills `client`'s socket's send buffer, which it shrinks to fit: once the agent
    /// holds that request unread, every send finds no room until the agent reads it. A unix socket's send finds room
    /// while what its peer holds unread of it, each message counted at its buffer's true size, is under its send
    /// buffer (net/unix/af_unix.c, `unix_dgram_sendmsg` through net/core/sock.c, `sock_alloc_send_pskb`), and a message
    /// may be as large as that buffer less 32 bytes; the kernel takes twice the size asked of it (socket(7), SO_SNDBUF).
    fn filling(client: &Client) -> Program {
        let fd = &client.conn.shared.fd;
        let program = Program::of(&["/bin/true", &"x".repeat(wire::MAX_MESSAGE / 8)]);
        let call = Request::Call(Call::Exec {
            id: BoxId::new("b").unwrap(),
            program: program.clone(),
            streams: Streams::default(),
        });
        let len = wire::request(client.version, u64::MAX, &call, wire::MAX_MESSAGE)
            .unwrap()
            .len();
        rustix::net::sockopt::set_socket_send_buffer_size(fd, (len + 32).div_ceil(2)).unwrap();
        let room = rustix::net::sockopt::socket_send_buffer_size(fd).unwrap();
        assert!(
            (len + 32..len + 34).contains(&room),
            "a send buffer of {room} bytes for a request of {len}"
        );
        program
    }

    /// Answers a stand-in agent sends while it reads no request, as the node agent does while its answers wait for
    /// room, each to a request the client never made: until what it sent is more than its send buffer holds, and once
    /// more, which waits until the client reads, the send that crosses the buffer being let through (`filling`).
    fn more_than_room(conn: &OwnedFd) {
        let room = rustix::net::sockopt::socket_send_buffer_size(conn).unwrap();
        let filler = Answer::Refused(Refusal::Broken("x".repeat(room / 8)));
        let bytes = wire::answer(wire::VERSION, u64::MAX, &filler, wire::MAX_MESSAGE).unwrap();
        let mut sent = 0;
        loop {
            rustix::net::send(conn, &bytes, SendFlags::NOSIGNAL).unwrap();
            if sent > room {
                break;
            }
            sent += bytes.len();
        }
    }

    /// The socket's per-call cost (goal 16; the lead, 2026-11-09): no thread reads the agent's socket but those waiting
    /// on it, so an answer wakes the call it answers and no thread between hands it on. Do: connect to a stand-in agent
    /// and delete a box. Expect: the call answered, and no thread of this process the client's reader.
    #[test]
    fn a_calls_answer_is_read_by_the_thread_waiting_for_it() {
        let (_dir, path, listener) = agent_socket();
        let agent = std::thread::spawn(move || {
            let conn = rustix::net::accept_with(&listener, SocketFlags::CLOEXEC).unwrap();
            hello(&conn, 1);
            let (h, request) = next_request(&conn);
            let Request::Call(Call::Delete(id)) = request else {
                panic!("a delete, not {request:?}")
            };
            answer(&conn, h.id, &Answer::Refused(Refusal::NoSuchBox(id)));
        });
        let client = Client::connect(&path).unwrap();
        let b = BoxId::new("b").unwrap();
        assert_eq!(client.delete(&b), Err(Refusal::NoSuchBox(b.clone())));
        let readers: Vec<String> = std::fs::read_dir("/proc/self/task")
            .unwrap()
            .filter_map(|t| std::fs::read_to_string(t.ok()?.path().join("comm")).ok())
            .filter(|comm| comm.trim() == "hex-client")
            .collect();
        assert_eq!(readers, Vec::<String>::new(), "the client's reading thread");
        agent.join().unwrap();
    }

    /// Many calls at once, answered last first: each gets its own answer, whichever thread read it, and the turn to read
    /// passes from each call answered to one still waiting. Do: eight threads each delete a box of its own; the stand-in
    /// agent reads all eight requests, then answers them in the reverse of their order. Expect: each refused naming its
    /// own box.
    #[test]
    fn many_calls_answered_out_of_order_each_get_their_own() {
        let (_dir, path, listener) = agent_socket();
        let n = 8;
        let agent = std::thread::spawn(move || {
            let conn = rustix::net::accept_with(&listener, SocketFlags::CLOEXEC).unwrap();
            hello(&conn, n);
            let asked: Vec<(Header, Request)> = (0..n).map(|_| next_request(&conn)).collect();
            for (h, request) in asked.into_iter().rev() {
                let Request::Call(Call::Delete(id)) = request else {
                    panic!("a delete, not {request:?}")
                };
                answer(&conn, h.id, &Answer::Refused(Refusal::NoSuchBox(id)));
            }
        });
        let client = Client::connect(&path).unwrap();
        std::thread::scope(|s| {
            for i in 0..n {
                let client = &client;
                s.spawn(move || {
                    let b = BoxId::new(&format!("b{i}")).unwrap();
                    assert_eq!(client.delete(&b), Err(Refusal::NoSuchBox(b.clone())));
                });
            }
        });
        agent.join().unwrap();
    }

    /// A call that runs out while it holds the reader's turn hands it to a call still waiting, whose answer then comes.
    /// Do: an inspect sent first, taking the turn; then a delete, sent while the stand-in agent holds the inspect; the
    /// agent answers the delete alone, once the inspect has run out. Expect: the inspect refused for no answer in its
    /// patience, and the delete answered.
    #[test]
    fn a_call_that_runs_out_hands_the_turn_to_one_still_waiting() {
        let (_dir, path, listener) = agent_socket();
        let (held, inspect_held) = std::sync::mpsc::channel();
        let (go, answer_delete) = std::sync::mpsc::channel::<()>();
        let agent = std::thread::spawn(move || {
            let conn = rustix::net::accept_with(&listener, SocketFlags::CLOEXEC).unwrap();
            hello(&conn, 2);
            let (_, inspect) = next_request(&conn);
            assert!(
                matches!(inspect, Request::Call(Call::Inspect(_))),
                "{inspect:?}"
            );
            held.send(()).unwrap();
            let (h, delete) = next_request(&conn);
            let Request::Call(Call::Delete(id)) = delete else {
                panic!("a delete, not {delete:?}")
            };
            answer_delete.recv().unwrap();
            answer(&conn, h.id, &Answer::Refused(Refusal::NoSuchBox(id)));
            conn
        });
        let client = Client::connect(&path).unwrap();
        let a = BoxId::new("a").unwrap();
        let b = BoxId::new("b").unwrap();
        std::thread::scope(|s| {
            let inspect = s.spawn(|| client.inspect(&a));
            inspect_held.recv().unwrap();
            let delete = s.spawn(|| client.delete(&b));
            assert!(
                matches!(
                    inspect.join().unwrap(),
                    Err(Refusal::Os {
                        op: "waiting for the node agent's answer",
                        ..
                    })
                ),
                "the inspect ran out"
            );
            go.send(()).unwrap();
            assert_eq!(delete.join().unwrap(), Err(Refusal::NoSuchBox(b.clone())));
        });
        drop(agent.join().unwrap());
    }

    /// A send that waits for room reads what the agent sends once the reader's turn is its: the node agent reads no
    /// request while its answers wait for room in the client's socket, so a send waiting with no one reading would
    /// wait for good. Do: a list, its request read, holds the turn; an exec's request as large as the client's send
    /// buffer is left unread, and answered by its id; a delete is made, which finds no room; the agent answers the
    /// list, then sends more than its own send buffer holds before it reads the exec's request and the delete's.
    /// Expect: the list answered, and the delete answered, not refused for want of room.
    #[test]
    fn a_send_waiting_for_room_reads_once_the_turn_is_its() {
        let (_dir, path, listener) = agent_socket();
        let (list_read, list_was_read) = std::sync::mpsc::channel();
        let (go, answer_list) = std::sync::mpsc::channel::<()>();
        let agent = std::thread::spawn(move || {
            let conn = rustix::net::accept_with(&listener, SocketFlags::CLOEXEC).unwrap();
            hello(&conn, 3);
            let (list, request) = next_request(&conn);
            assert_eq!(request, Request::Call(Call::List));
            list_read.send(()).unwrap();
            let (exec, request) = peek_request(&conn);
            let Request::Call(Call::Exec { id, .. }) = request else {
                panic!("an exec, not {request:?}")
            };
            answer(&conn, exec.id, &Answer::Refused(Refusal::NoSuchBox(id)));
            answer_list.recv().unwrap();
            answer(
                &conn,
                list.id,
                &Answer::Boxes {
                    infos: Vec::new(),
                    more: false,
                },
            );
            more_than_room(&conn);
            let (_, request) = next_request(&conn);
            assert!(
                matches!(request, Request::Call(Call::Exec { .. })),
                "{request:?}"
            );
            let (delete, request) = next_request(&conn);
            let Request::Call(Call::Delete(id)) = request else {
                panic!("a delete, not {request:?}")
            };
            answer(&conn, delete.id, &Answer::Refused(Refusal::NoSuchBox(id)));
            conn
        });
        let client = Client::connect(&path).unwrap();
        let program = filling(&client);
        let b = BoxId::new("b").unwrap();
        let d = BoxId::new("d").unwrap();
        std::thread::scope(|s| {
            let list = s.spawn(|| client.list());
            list_was_read.recv().unwrap();
            assert_eq!(
                client.exec(&b, program, Stdio::null()).err(),
                Some(Refusal::NoSuchBox(b.clone()))
            );
            let delete = s.spawn(|| client.delete(&d));
            // The delete has its place among the calls, beside the list's.
            while client.conn.shared.state().unwrap().calls.len() < 2 {
                std::thread::yield_now();
            }
            go.send(()).unwrap();
            assert_eq!(list.join().unwrap(), Ok(Vec::new()));
            assert_eq!(delete.join().unwrap(), Err(Refusal::NoSuchBox(d.clone())));
        });
        drop(agent.join().unwrap());
    }

    /// A call that runs out waiting for room hands on the reader's turn it holds, so the calls after it are still
    /// read. Do: as for the send waiting for room, but an inspect finds no room, and the agent sends nothing more and
    /// reads nothing until it has run out; then the agent reads the exec's request, and a delete is made. Expect: the
    /// inspect refused for want of room, and the delete answered.
    #[test]
    fn a_call_that_runs_out_waiting_for_room_hands_on_its_turn() {
        let (_dir, path, listener) = agent_socket();
        let (list_read, list_was_read) = std::sync::mpsc::channel();
        let (go, answer_list) = std::sync::mpsc::channel::<()>();
        let (read_on, inspect_ran_out) = std::sync::mpsc::channel::<()>();
        let agent = std::thread::spawn(move || {
            let conn = rustix::net::accept_with(&listener, SocketFlags::CLOEXEC).unwrap();
            hello(&conn, 3);
            let (list, request) = next_request(&conn);
            assert_eq!(request, Request::Call(Call::List));
            list_read.send(()).unwrap();
            let (exec, request) = peek_request(&conn);
            let Request::Call(Call::Exec { id, .. }) = request else {
                panic!("an exec, not {request:?}")
            };
            answer(&conn, exec.id, &Answer::Refused(Refusal::NoSuchBox(id)));
            answer_list.recv().unwrap();
            answer(
                &conn,
                list.id,
                &Answer::Boxes {
                    infos: Vec::new(),
                    more: false,
                },
            );
            inspect_ran_out.recv().unwrap();
            let (_, request) = next_request(&conn);
            assert!(
                matches!(request, Request::Call(Call::Exec { .. })),
                "{request:?}"
            );
            let (delete, request) = next_request(&conn);
            let Request::Call(Call::Delete(id)) = request else {
                panic!("a delete, not {request:?}")
            };
            answer(&conn, delete.id, &Answer::Refused(Refusal::NoSuchBox(id)));
            conn
        });
        let client = Client::connect(&path).unwrap();
        let program = filling(&client);
        let b = BoxId::new("b").unwrap();
        let i = BoxId::new("i").unwrap();
        let d = BoxId::new("d").unwrap();
        std::thread::scope(|s| {
            let list = s.spawn(|| client.list());
            list_was_read.recv().unwrap();
            assert_eq!(
                client.exec(&b, program, Stdio::null()).err(),
                Some(Refusal::NoSuchBox(b.clone()))
            );
            let inspect = s.spawn(|| client.inspect(&i));
            // The inspect has its place among the calls, beside the list's.
            while client.conn.shared.state().unwrap().calls.len() < 2 {
                std::thread::yield_now();
            }
            go.send(()).unwrap();
            assert_eq!(list.join().unwrap(), Ok(Vec::new()));
            assert!(
                matches!(
                    inspect.join().unwrap(),
                    Err(Refusal::Os {
                        op: "sending to the node agent",
                        ..
                    })
                ),
                "the inspect ran out for want of room"
            );
        });
        read_on.send(()).unwrap();
        assert_eq!(client.delete(&d), Err(Refusal::NoSuchBox(d.clone())));
        drop(agent.join().unwrap());
    }

    /// A program's end comes to its `Proc` waiting for it with no call in flight: the wait reads it itself, there being
    /// no other reader. Do: exec through a stand-in agent that answers it started and then sends its end. Expect: the
    /// `Proc`'s wait returns that exit.
    #[test]
    fn a_programs_end_comes_to_its_wait_with_no_call_in_flight() {
        let (_dir, path, listener) = agent_socket();
        let agent = std::thread::spawn(move || {
            let conn = rustix::net::accept_with(&listener, SocketFlags::CLOEXEC).unwrap();
            hello(&conn, 1);
            let (h, request) = next_request(&conn);
            assert!(
                matches!(request, Request::Call(Call::Exec { .. })),
                "{request:?}"
            );
            answer(&conn, h.id, &Answer::Started { pid: 4321 });
            answer(&conn, h.id, &Answer::Exited(Exit::Code(3)));
            conn
        });
        let client = Client::connect(&path).unwrap();
        let b = BoxId::new("b").unwrap();
        let mut proc = client
            .exec(&b, Program::of(&["/bin/true"]), Stdio::null())
            .unwrap();
        assert_eq!(proc.pid(), 4321);
        assert_eq!(
            proc.wait_for(Duration::from_secs(60)),
            Ok(Some(Exit::Code(3)))
        );
        drop(proc);
        drop(agent.join().unwrap());
    }

    /// HEX-SOCK-4: an agent that refuses a connection and closes it with the client's hello unread
    /// leaves the client a reset, reported once and before the refusal it sent; the client still reads
    /// the refusal, so connect fails with it, typed, rather than with the reset.
    #[test]
    fn a_refusal_before_a_reset_is_the_one_connect_returns() {
        let (_dir, path, listener) = agent_socket();
        let refusal = Refusal::Exhausted {
            what: "connections to the node agent's socket",
            bound: 2,
        };
        let agent = {
            let refusal = refusal.clone();
            std::thread::spawn(move || {
                let conn = rustix::net::accept_with(&listener, SocketFlags::CLOEXEC).unwrap();
                // The client's hello is in this side's queue, unread, before the refusal goes.
                let mut hello = [PollFd::new(&conn, PollFlags::IN)];
                let ts = Timespec {
                    tv_sec: 30,
                    tv_nsec: 0,
                };
                assert_eq!(rustix::event::poll(&mut hello, Some(&ts)).unwrap(), 1);
                // In the first version, as the node agent refuses a connection.
                let bytes =
                    wire::answer(wire::FIRST, 0, &Answer::Refused(refusal), wire::MAX_MESSAGE)
                        .unwrap();
                rustix::net::send(&conn, &bytes, SendFlags::NOSIGNAL).unwrap();
            })
        };
        assert_eq!(Client::connect(&path).err(), Some(refusal));
        agent.join().unwrap();
    }

    /// HEX-SOCK-2, append-only: a client of version 2 serves the seam to an agent of version 1. Its
    /// hello goes in version 1; told the agent speaks version 1 alone, it writes every call in
    /// version 1, and the network namespace counts, which version 1 doesn't define, are refused by
    /// type without a message sent: the agent's next request is the call after them.
    #[test]
    fn a_version_1_agent_is_served_in_version_1() {
        let (_dir, path, listener) = agent_socket();
        let agent = std::thread::spawn(move || {
            let conn = rustix::net::accept_with(&listener, SocketFlags::CLOEXEC).unwrap();
            let mut seen = Vec::new();
            for _ in 0..3 {
                let (h, request) = next_request(&conn);
                let answer = match request {
                    Request::Hello => Answer::Hello(wire::Hello {
                        version: 1,
                        classes: vec![Class::Process],
                        limits: Limits {
                            connections: 1,
                            message_bytes: wire::MAX_MESSAGE,
                            in_flight: 1,
                            programs: 1,
                        },
                        budgets: None,
                    }),
                    Request::Call(Call::Inspect(id)) => Answer::Refused(Refusal::NoSuchBox(id)),
                    other => panic!("a version 1 agent was sent {other:?}"),
                };
                let bytes = wire::answer(1, h.id, &answer, wire::MAX_MESSAGE).unwrap();
                rustix::net::send(&conn, &bytes, SendFlags::NOSIGNAL).unwrap();
                seen.push((h.version, h.kind));
            }
            seen
        });
        let client = Client::connect(&path).unwrap();
        let b = BoxId::new("b").unwrap();
        assert_eq!(client.inspect(&b), Err(Refusal::NoSuchBox(b.clone())));
        assert_eq!(
            client.netns(),
            Err(Refusal::Invalid {
                what: "message",
                why: "kind 0x000b isn't a request version 1 defines".to_owned(),
            })
        );
        assert_eq!(client.inspect(&b), Err(Refusal::NoSuchBox(b.clone())));
        assert_eq!(
            agent.join().unwrap(),
            [(1, 0x0001), (1, 0x0006), (1, 0x0006)],
            "the hello and both inspects, each in version 1, and nothing between them"
        );
    }
}
