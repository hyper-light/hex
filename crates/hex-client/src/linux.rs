//! The client on Linux: one `SOCK_SEQPACKET` connection, a thread that reads every message the agent
//! sends and hands each answer to the call waiting for it and each program's end to its `Proc`, and
//! calls that send their request (with an exec's stdio as `SCM_RIGHTS` descriptors) and wait, at most
//! [`ANSWER_PATIENCE`], for the answer.

use std::collections::BTreeMap;
use std::io::{IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::Path;
use std::sync::mpsc::{RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hex_api::{
    BoxId, BoxInfo, BoxSpec, Class, Exit, Proc, Program, Refusal, Runtime, Signal, Stdio, Usage,
};
use hex_core::netns;
use hex_core::wire::{self, Answer, Call, Header, Limits, Received, Request, Streams};
use rustix::io::Errno;
use rustix::net::sockopt::Timeout;
use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags,
    SendAncillaryBuffer, SendAncillaryMessage, SendFlags, Shutdown, SocketAddrUnix, SocketFlags,
    SocketType,
};

use crate::{ANSWER_PATIENCE, MAX_LIST};

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
    reader: Mutex<Option<JoinHandle<()>>>,
}

/// What the reading thread and the callers share.
struct Shared {
    fd: OwnedFd,
    state: Mutex<State>,
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
}

struct Waiting {
    answer: SyncSender<Result<Answer, Refusal>>,
    /// A list's boxes from the parts that came so far.
    boxes: Vec<BoxInfo>,
}

/// A program's end, once it has come.
#[derive(Default)]
struct Ending {
    exit: Mutex<Option<Result<Exit, Refusal>>>,
    came: Condvar,
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> Refusal {
    Refusal::Broken("a lock of the node agent's client was poisoned".to_owned())
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
                    *ending.exit.lock().map_err(poisoned)? = Some(Ok(exit));
                    ending.came.notify_all();
                }
            }
            Answer::Refused(refusal) if header.id == 0 => state.refused = Some(refusal),
            Answer::Boxes { infos, more: true } => {
                if let Some(waiting) = state.calls.get_mut(&header.id) {
                    if waiting.boxes.len().saturating_add(infos.len()) > MAX_LIST {
                        let waiting = state.calls.remove(&header.id);
                        if let Some(w) = waiting {
                            let _ = w.answer.try_send(Err(Refusal::Exhausted {
                                what: "boxes in one list",
                                bound: MAX_LIST,
                            }));
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
                    let _ = waiting.answer.try_send(Ok(answer));
                }
            }
        }
        Ok(())
    }

    /// The connection has ended: every call waiting and every program's `Proc` hear why.
    fn close(&self, why: Refusal) {
        let Ok(mut state) = self.state() else {
            return;
        };
        let why = state.refused.take().unwrap_or(why);
        for (_, waiting) in std::mem::take(&mut state.calls) {
            let _ = waiting.answer.try_send(Err(why.clone()));
        }
        let lost = Refusal::Broken(format!(
            "the connection to the node agent closed before the program's end came: {why}"
        ));
        for ending in state.programs.values() {
            if let Ok(mut exit) = ending.exit.lock()
                && exit.is_none()
            {
                *exit = Some(Err(lost.clone()));
                ending.came.notify_all();
            }
        }
        state.closed = Some(why);
    }
}

/// The reading thread: every message the agent sends, until the connection ends.
fn read(shared: &Shared) {
    let mut buf = vec![0u8; wire::MAX_MESSAGE];
    let mut reset = None;
    let why = loop {
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
        let mut control = RecvAncillaryBuffer::new(&mut space);
        let msg = match rustix::net::recvmsg(
            &shared.fd,
            &mut [IoSliceMut::new(&mut buf)],
            &mut control,
            RecvFlags::CMSG_CLOEXEC | RecvFlags::TRUNC,
        ) {
            Ok(msg) => msg,
            Err(Errno::INTR) => continue,
            // An agent that closed the connection with a request of ours unread leaves this error,
            // reported once and before what it sent (net/unix/af_unix.c, `unix_release_sock`):
            // its last answers, a refusal of the connection among them, are still there to read.
            Err(Errno::CONNRESET) if reset.is_none() => {
                reset = Some(Refusal::Broken(
                    "the node agent closed the connection with requests unread".to_owned(),
                ));
                continue;
            }
            Err(e) => break Refusal::Broken(format!("reading the node agent's socket: {e}")),
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
            break reset.unwrap_or_else(|| {
                Refusal::Broken("the node agent closed the connection".to_owned())
            });
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
            .and_then(|(header, answer)| shared.deliver(header, answer));
        if let Err(why) = delivered {
            break why;
        }
    };
    shared.close(why);
}

impl Drop for Conn {
    fn drop(&mut self) {
        // The reading thread sees the end and goes; the agent sees the hang-up.
        let _ = rustix::net::shutdown(&self.shared.fd, Shutdown::Both);
        if let Some(reader) = self.reader.lock().ok().and_then(|mut r| r.take()) {
            let _ = reader.join();
        }
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
        // Bounds a send, and the connect itself, by the agent's room to take them.
        rustix::net::sockopt::set_socket_timeout(&fd, Timeout::Send, Some(ANSWER_PATIENCE))
            .map_err(|e| Refusal::os("bounding a socket's sends", e))?;
        rustix::net::connect(&fd, &addr).map_err(|e| Refusal::Os {
            op: "connecting to the node agent's socket",
            why: format!("{}: {e}", path.display()),
        })?;
        let shared = Arc::new(Shared {
            fd,
            state: Mutex::new(State::default()),
        });
        let reader = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("hex-client".to_owned())
                .spawn(move || read(&shared))
                .map_err(|e| Refusal::os("starting the client's reading thread", e))?
        };
        let mut client = Self {
            conn: Arc::new(Conn {
                shared,
                reader: Mutex::new(Some(reader)),
            }),
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

    /// Sends `request` with `fds`, closing this side's copies of them once sent, and waits for its
    /// answer. With `program`, a `Proc`'s place for the end of the program an exec starts is made
    /// before the exec goes, so the end can't come first.
    fn call(
        &self,
        request: &Request,
        fds: Vec<OwnedFd>,
        program: Option<Arc<Ending>>,
    ) -> Result<(u64, Answer), Refusal> {
        let shared = &self.conn.shared;
        let (tx, rx) = sync_channel(1);
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
                    answer: tx,
                    boxes: Vec::new(),
                },
            );
            if let Some(program) = program {
                state.programs.insert(id, program);
            }
            id
        };
        let forget = |shared: &Shared| {
            if let Ok(mut state) = shared.state() {
                state.calls.remove(&id);
                state.programs.remove(&id);
            }
        };
        let sent = wire::request(self.version, id, request, self.limits.message_bytes)
            .map_err(|fault| Sent::Refused(fault.into()))
            .and_then(|bytes| send(&shared.fd, &bytes, &fds));
        drop(fds);
        match sent {
            Ok(()) => {}
            // The agent has gone, or closed the connection: why reaches the waiting call through
            // the reading thread, which sees the end.
            Err(Sent::Gone) => {}
            Err(Sent::Refused(r)) => {
                forget(shared);
                return Err(r);
            }
        }
        let answer = match rx.recv_timeout(ANSWER_PATIENCE) {
            Ok(answer) => answer,
            Err(RecvTimeoutError::Timeout) => {
                forget(shared);
                return Err(Refusal::Os {
                    op: "waiting for the node agent's answer",
                    why: format!("none came in {ANSWER_PATIENCE:?}"),
                });
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(shared.state()?.closed.clone().unwrap_or_else(|| {
                    Refusal::Broken("the node agent's answer was lost".to_owned())
                }))
            }
        };
        match answer {
            Ok(answer) => Ok((id, answer)),
            Err(r) => {
                forget(shared);
                Err(r)
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
    /// The connection is gone: the reading thread says why.
    Gone,
    Refused(Refusal),
}

fn send(fd: &OwnedFd, bytes: &[u8], fds: &[OwnedFd]) -> Result<(), Sent> {
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
    loop {
        match rustix::net::sendmsg(
            fd,
            &[IoSlice::new(bytes)],
            &mut control,
            SendFlags::NOSIGNAL,
        ) {
            Ok(_) => return Ok(()),
            Err(Errno::INTR) => continue,
            Err(Errno::PIPE | Errno::CONNRESET | Errno::NOTCONN) => return Err(Sent::Gone),
            Err(Errno::AGAIN) => {
                return Err(Sent::Refused(Refusal::Os {
                    op: "sending to the node agent",
                    why: format!("no room in its socket for {ANSWER_PATIENCE:?}: it isn't reading"),
                }));
            }
            Err(e) => {
                return Err(Sent::Refused(Refusal::os("sending to the node agent", e)));
            }
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
        self.info(Call::Create(spec), "create")
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
    /// Its end if it has come, waiting for it at most `timeout` (none: not at all).
    fn wait(&mut self, timeout: Option<Duration>) -> Result<Option<Exit>, Refusal> {
        if let Some(exit) = self.exit {
            return Ok(Some(exit));
        }
        let deadline = timeout.map(|t| Instant::now().checked_add(t));
        let mut came = self.ending.exit.lock().map_err(poisoned)?;
        loop {
            match &*came {
                Some(Ok(exit)) => {
                    self.exit = Some(*exit);
                    return Ok(Some(*exit));
                }
                Some(Err(why)) => return Err(why.clone()),
                None => {}
            }
            let left = match deadline {
                None => return Ok(None),
                // A timeout past what an instant holds waits as long as it can at a time.
                Some(None) => timeout.unwrap_or(Duration::ZERO),
                Some(Some(deadline)) => deadline.saturating_duration_since(Instant::now()),
            };
            if left.is_zero() {
                return Ok(None);
            }
            came = self
                .ending
                .came
                .wait_timeout(came, left)
                .map_err(poisoned)?
                .0;
        }
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
                .exit
                .lock()
                .map(|exit| exit.is_some())
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
            let _ = send(&shared.fd, &bytes, &[]);
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
