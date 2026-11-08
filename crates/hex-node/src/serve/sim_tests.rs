//! The socket's core over a simulated OS (native plan step 8: one node core for every OS, over each OS's
//! event sources): the same requests, bounds and disconnect rule the Linux tests drive through a real
//! socket (linux_tests.rs, HEX-SOCK-3 to 10), here through an [`Os`] whose connections are queues and whose
//! programs end when the test says, so they run on every OS the core builds for. The simulated OS is
//! level-triggered, as epoll is by default: a watched connection is readable while messages wait or its
//! client has gone, and writable while it has room.

use super::*;

use std::collections::BTreeSet;
use std::sync::atomic::AtomicUsize;

use hex_api::sim::SimRuntime;
use hex_api::{Bounds, BoxInfo, BoxSpec, ClassSpec, Signal, Usage};

/// Longest a test waits for a fact it needs: a bound, so a core that never makes it true fails the test
/// instead of hanging it.
const PATIENCE: Duration = Duration::from_secs(30);

/// One simulated connection: what the client sent and what the server sent back.
#[derive(Default)]
struct Sock {
    to_server: Mutex<VecDeque<Vec<u8>>>,
    to_client: Mutex<VecDeque<Vec<u8>>>,
    /// Messages the server may send before it would block.
    room: Mutex<usize>,
    client_gone: AtomicBool,
    shut: AtomicBool,
    /// Signalled when the server sends.
    sent: Condvar,
    sent_lock: Mutex<()>,
}

impl Sock {
    fn new(room: usize) -> Arc<Self> {
        Arc::new(Self {
            room: Mutex::new(room),
            ..Self::default()
        })
    }
}

#[derive(Default)]
struct SimState {
    pending: VecDeque<(Arc<Sock>, bool)>,
    accepting: bool,
    spent: bool,
    woken: bool,
    conns: BTreeMap<u64, (Arc<Sock>, Interest)>,
    /// Programs watched, by pid: their token, and whether they have ended.
    programs: BTreeMap<u32, (u64, bool)>,
    killed: BTreeSet<u32>,
    /// Every pid the core asked to watch as a host process.
    ever_watched: BTreeSet<u32>,
    unwatchable: bool,
}

#[derive(Default)]
struct SimOs {
    state: Mutex<SimState>,
    changed: Condvar,
}

impl SimOs {
    fn lock(&self) -> MutexGuard<'_, SimState> {
        self.state.lock().unwrap()
    }

    fn poke(&self) {
        self.changed.notify_all();
    }

    /// A client connects; `allowed` says whether it is this user.
    fn connect(&self, allowed: bool, room: usize) -> Arc<Sock> {
        let sock = Sock::new(room);
        self.lock().pending.push_back((Arc::clone(&sock), allowed));
        self.poke();
        sock
    }

    fn put(&self, sock: &Sock, bytes: Vec<u8>) {
        sock.to_server.lock().unwrap().push_back(bytes);
        self.poke();
    }

    fn leave(&self, sock: &Sock) {
        sock.client_gone.store(true, Ordering::SeqCst);
        self.poke();
    }

    fn give_room(&self, sock: &Sock, room: usize) {
        *sock.room.lock().unwrap() = room;
        self.poke();
    }

    /// The program `pid` ends of itself.
    fn end(&self, pid: u32) {
        if let Some(p) = self.lock().programs.get_mut(&pid) {
            p.1 = true;
        }
        self.poke();
    }

    fn ended(&self, pid: u32) -> bool {
        self.lock().programs.get(&pid).is_none_or(|p| p.1)
    }
}

struct Accept(Arc<Sock>, bool);

impl Os for SimOs {
    type Conn = Accept;
    type Watch = u32;
    type Passed = ();

    fn wait(&self, events: &mut Vec<Event>, timeout: Option<Duration>) -> Result<(), Refusal> {
        let deadline = timeout.and_then(|t| Instant::now().checked_add(t));
        let mut s = self.lock();
        loop {
            if std::mem::take(&mut s.woken) {
                events.push(Event::Wake);
            }
            if s.accepting && !s.pending.is_empty() {
                events.push(Event::Listener);
            }
            for (token, (sock, interest)) in &s.conns {
                let gone = sock.client_gone.load(Ordering::SeqCst);
                let readable = matches!(interest, Interest::Read)
                    && (gone || !sock.to_server.lock().unwrap().is_empty());
                let writable =
                    matches!(interest, Interest::Write) && *sock.room.lock().unwrap() > 0;
                if readable || writable || gone {
                    events.push(Event::Ready {
                        token: *token,
                        readable,
                        writable,
                        hung_up: gone,
                    });
                }
            }
            for (token, ended) in s.programs.values() {
                if *ended {
                    events.push(Event::Ready {
                        token: *token,
                        readable: true,
                        writable: false,
                        hung_up: false,
                    });
                }
            }
            if !events.is_empty() {
                return Ok(());
            }
            match deadline {
                None => s = self.changed.wait(s).unwrap(),
                Some(d) => {
                    let left = d.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Ok(());
                    }
                    s = self.changed.wait_timeout(s, left).unwrap().0;
                }
            }
        }
    }

    fn wake(&self) -> Result<(), Refusal> {
        self.lock().woken = true;
        self.poke();
        Ok(())
    }

    fn accept(&self) -> Accepted<Accept> {
        let mut s = self.lock();
        if s.spent {
            return Accepted::Spent;
        }
        match s.pending.pop_front() {
            Some((sock, allowed)) => Accepted::Conn(Accept(sock, allowed)),
            None => Accepted::None,
        }
    }

    fn accepting(&self, on: bool) -> Result<(), Refusal> {
        self.lock().accepting = on;
        Ok(())
    }

    fn peer(&self, conn: &Accept) -> Peer {
        if conn.1 {
            Peer::Allowed
        } else {
            Peer::Refused("a simulated stranger".to_owned())
        }
    }

    fn watch(&self, conn: &Accept, token: u64, interest: Interest) -> Result<(), Refusal> {
        self.lock()
            .conns
            .insert(token, (Arc::clone(&conn.0), interest));
        self.poke();
        Ok(())
    }

    fn rewatch(&self, conn: &Accept, token: u64, interest: Interest) -> Result<(), Refusal> {
        self.watch(conn, token, interest)
    }

    fn unwatch(&self, conn: &Accept) {
        self.lock()
            .conns
            .retain(|_, (s, _)| !Arc::ptr_eq(s, &conn.0));
    }

    fn recv(&self, conn: &Accept, buf: &mut [u8]) -> Recv<()> {
        match conn.0.to_server.lock().unwrap().pop_front() {
            Some(bytes) => {
                let n = bytes.len().min(buf.len());
                buf[..n].copy_from_slice(&bytes[..n]);
                Recv::Message {
                    size: bytes.len(),
                    got: n,
                    passed: Vec::new(),
                    cut: false,
                }
            }
            None if conn.0.client_gone.load(Ordering::SeqCst) => Recv::Message {
                size: 0,
                got: 0,
                passed: Vec::new(),
                cut: false,
            },
            None => Recv::WouldBlock,
        }
    }

    fn send(&self, conn: &Accept, bytes: &[u8]) -> Sent {
        let sock = &conn.0;
        if sock.client_gone.load(Ordering::SeqCst) || sock.shut.load(Ordering::SeqCst) {
            return Sent::Gone;
        }
        {
            let mut room = sock.room.lock().unwrap();
            match room.checked_sub(1) {
                Some(left) => *room = left,
                None => return Sent::WouldBlock,
            }
        }
        sock.to_client.lock().unwrap().push_back(bytes.to_vec());
        let _held = sock.sent_lock.lock().unwrap();
        sock.sent.notify_all();
        Sent::Sent
    }

    fn shutdown(&self, conn: &Accept) {
        conn.0.shut.store(true, Ordering::SeqCst);
        let _held = conn.0.sent_lock.lock().unwrap();
        conn.0.sent.notify_all();
    }

    fn hung_up(&self, conn: &Accept) -> bool {
        conn.0.client_gone.load(Ordering::SeqCst)
    }

    fn watch_program(&self, pid: u32, token: u64) -> Result<u32, Unwatched> {
        let mut s = self.lock();
        if s.unwatchable {
            return Err(Unwatched {
                why: Refusal::Os {
                    op: "watching a started program",
                    why: "a simulated failure: the node agent ended it".to_owned(),
                },
                killed: true,
            });
        }
        s.programs.insert(pid, (token, false));
        s.ever_watched.insert(pid);
        Ok(pid)
    }

    fn kill(&self, watch: &u32) {
        let mut s = self.lock();
        s.killed.insert(*watch);
        if let Some(p) = s.programs.get_mut(watch) {
            p.1 = true;
        }
        drop(s);
        self.poke();
    }

    fn unwatch_program(&self, watch: u32) {
        self.lock().programs.remove(&watch);
    }

    fn stdio(&self, _streams: Streams, _passed: Vec<()>) -> Result<Stdio, Refusal> {
        Ok(Stdio::null())
    }

    fn stop_listening(&self) {
        self.lock().accepting = false;
    }
}

/// A program that ends when the simulated OS says it has.
struct SimProc {
    pid: u32,
    os: Arc<SimOs>,
}

impl Proc for SimProc {
    fn pid(&self) -> u32 {
        self.pid
    }
    fn try_wait(&mut self) -> Result<Option<Exit>, Refusal> {
        Ok(if self.os.lock().killed.contains(&self.pid) {
            Some(Exit::Signal(9))
        } else if self.os.ended(self.pid) {
            Some(Exit::Code(0))
        } else {
            None
        })
    }
    fn wait_for(&mut self, _timeout: Duration) -> Result<Option<Exit>, Refusal> {
        self.try_wait()
    }
    fn kill(&mut self) -> Result<(), Refusal> {
        self.os.kill(&self.pid);
        Ok(())
    }
}

/// A simulated node in a guest: box [`g`]'s calls are forwarded to it ([`Host::forward`]) and answered when
/// its gate is open (held, a guest slow to answer), and its programs' pids are the guest's, which no host
/// process has; their ends come from it, as messages.
/// An answer the simulated guest gives once its gate is open.
type Waiting = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct Guest {
    /// Whether its gate is shut, and the answers waiting behind it, oldest first.
    gate: Mutex<(bool, Vec<Waiting>)>,
    /// What takes each running program's end, by guest pid.
    programs: Mutex<BTreeMap<u32, Ended>>,
}

impl Guest {
    fn hold(&self, held: bool) {
        let waiting = {
            let mut gate = self.gate.lock().unwrap();
            gate.0 = held;
            if held {
                Vec::new()
            } else {
                std::mem::take(&mut gate.1)
            }
        };
        for answer in waiting {
            answer();
        }
    }

    fn holding(&self) -> usize {
        self.gate.lock().unwrap().1.len()
    }

    /// Gives `answer` now, or once the gate opens.
    fn answer(&self, answer: Waiting) {
        let mut gate = self.gate.lock().unwrap();
        if gate.0 {
            gate.1.push(answer);
        } else {
            drop(gate);
            answer();
        }
    }

    /// Its program `pid` ends with `exit`, unless it has ended or been let go.
    fn end(&self, pid: u32, exit: Exit) {
        let taker = self.programs.lock().unwrap().remove(&pid);
        if let Some(ended) = taker {
            ended(Ok(exit));
        }
    }
}

/// A program in the guest, by its guest pid.
struct GuestProgram {
    pid: u32,
    guest: Arc<Guest>,
}

impl Remote for GuestProgram {
    fn kill(&self, done: Killed) {
        let (pid, guest) = (self.pid, Arc::clone(&self.guest));
        // Answered later, as a guest's answer comes on its connection.
        std::thread::spawn(move || {
            done(Ok(()));
            guest.end(pid, Exit::Signal(9));
        });
    }
    fn release(&self) {
        self.guest.programs.lock().unwrap().remove(&self.pid);
    }
}

/// Boxes in the simulator; each exec a [`SimProc`]; inspect held shut on purpose to keep calls in flight.
struct SimHost {
    sim: SimRuntime,
    os: Arc<SimOs>,
    next_pid: AtomicUsize,
    shut: Mutex<bool>,
    opened: Condvar,
    abandoned: Mutex<Vec<BoxId>>,
    guest: Arc<Guest>,
}

impl SimHost {
    fn new(os: &Arc<SimOs>) -> Arc<Self> {
        let host = Arc::new(Self {
            sim: SimRuntime::new(),
            os: Arc::clone(os),
            next_pid: AtomicUsize::new(100),
            shut: Mutex::new(false),
            opened: Condvar::new(),
            abandoned: Mutex::new(Vec::new()),
            guest: Arc::default(),
        });
        for id in [b(), g()] {
            host.sim
                .create(BoxSpec {
                    id,
                    bounds: Bounds::default(),
                    class: ClassSpec::Process,
                })
                .unwrap();
        }
        host
    }

    fn hold(&self, shut: bool) {
        *self.shut.lock().unwrap() = shut;
        self.opened.notify_all();
    }
}

impl Runtime for SimHost {
    fn classes(&self) -> Vec<hex_api::Class> {
        self.sim.classes()
    }
    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        self.sim.create(spec)
    }
    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        self.sim.start(id)
    }
    fn exec(&self, id: &BoxId, _program: Program, _stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        self.sim.inspect(id)?;
        let pid = u32::try_from(self.next_pid.fetch_add(1, Ordering::SeqCst)).unwrap();
        Ok(Box::new(SimProc {
            pid,
            os: Arc::clone(&self.os),
        }))
    }
    fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal> {
        self.sim.signal(id, signal)
    }
    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        let deadline = Instant::now() + PATIENCE;
        let mut shut = self.shut.lock().unwrap();
        while *shut && Instant::now() < deadline {
            shut = self.opened.wait_timeout(shut, PATIENCE).unwrap().0;
        }
        drop(shut);
        self.sim.inspect(id)
    }
    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        self.sim.usage(id)
    }
    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        self.sim.list()
    }
    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        self.sim.delete(id)
    }
}

impl Host for SimHost {
    /// Box [`g`]'s inspects and execs are its guest's; every other call runs here.
    fn forward(&self, call: Call, stdio: Stdio, reply: Reply, ended: Ended) -> Forward {
        match call {
            Call::Inspect(id) if id == g() => {
                let info = self.sim.inspect(&id);
                self.guest.answer(Box::new(move || {
                    reply(info.map(|i| Forwarded::Answer(Answer::Info(i))));
                }));
                Forward::Taken
            }
            Call::Exec { id, .. } if id == g() => {
                // A guest pid: no host process has it.
                let pid = u32::try_from(self.next_pid.fetch_add(1, Ordering::SeqCst)).unwrap()
                    + GUEST_PIDS;
                self.guest.programs.lock().unwrap().insert(pid, ended);
                let program = Box::new(GuestProgram {
                    pid,
                    guest: Arc::clone(&self.guest),
                });
                self.guest.answer(Box::new(move || {
                    reply(Ok(Forwarded::Started { pid, program }));
                }));
                Forward::Taken
            }
            call => Forward::Back(Box::new((call, stdio))),
        }
    }

    fn abandoned(&self, id: &BoxId) -> Result<(), Refusal> {
        self.abandoned.lock().unwrap().push(id.clone());
        Ok(())
    }
    fn netns(&self) -> Result<netns::Counts, Refusal> {
        Ok(netns::Counts::default())
    }
}

struct Lab {
    os: Arc<SimOs>,
    host: Arc<SimHost>,
    server: Server<Arc<SimOs>>,
}

impl Lab {
    fn new() -> Self {
        Self::with_workers(8)
    }

    fn with_workers(workers: usize) -> Self {
        let os = Arc::new(SimOs::default());
        let host = SimHost::new(&os);
        let server = Server::with_os(
            Arc::clone(&os),
            Arc::clone(&host) as Arc<dyn Host>,
            limits(),
            workers,
            Path::new("/nonexistent/hex-sim.sock"),
        )
        .unwrap();
        Self { os, host, server }
    }

    fn connections(&self) -> usize {
        self.server.shared.table.lock().unwrap().conns.len()
    }

    fn watched(&self) -> usize {
        self.server.shared.table.lock().unwrap().watched.len()
    }
}

fn limits() -> Limits {
    Limits {
        connections: 2,
        message_bytes: 4096,
        in_flight: 2,
        programs: 2,
    }
}

fn b() -> BoxId {
    BoxId::new("b").unwrap()
}

/// The box the simulated guest holds.
fn g() -> BoxId {
    BoxId::new("g").unwrap()
}

/// Where the simulated guest's pids start: past any the host gives.
const GUEST_PIDS: u32 = 1 << 20;

fn request(id: u64, call: Call) -> Vec<u8> {
    wire::request(wire::VERSION, id, &Request::Call(call), wire::MAX_MESSAGE).unwrap()
}

fn hello() -> Vec<u8> {
    wire::request(wire::VERSION, 1, &Request::Hello, wire::MAX_MESSAGE).unwrap()
}

fn exec(id: u64) -> Vec<u8> {
    exec_in(id, b())
}

fn exec_in(id: u64, b: BoxId) -> Vec<u8> {
    request(
        id,
        Call::Exec {
            id: b,
            program: Program::of(&["p"]),
            streams: Streams::default(),
        },
    )
}

/// The next message the server sends `sock`, waiting at most [`PATIENCE`].
fn answer_of(sock: &Sock) -> (Header, Answer) {
    let deadline = Instant::now() + PATIENCE;
    let mut held = sock.sent_lock.lock().unwrap();
    loop {
        if let Some(bytes) = sock.to_client.lock().unwrap().pop_front() {
            let got = Received {
                bytes: &bytes,
                size: bytes.len(),
                descriptors: 0,
                descriptors_cut: false,
            };
            return wire::read_answer(&got, wire::MAX_MESSAGE).unwrap();
        }
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "no answer came");
        held = sock.sent.wait_timeout(held, left).unwrap().0;
    }
}

fn wait_until(what: &str, mut fact: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !fact() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::park_timeout(Duration::from_millis(1));
    }
}

/// HEX-SOCK-3 (the core): a hello is answered with the host's classes and the server's limits, over an OS
/// that isn't Linux's.
#[test]
fn a_hello_is_answered_over_any_os() {
    let lab = Lab::new();
    let sock = lab.os.connect(true, 64);
    lab.os.put(&sock, hello());
    match answer_of(&sock) {
        (_, Answer::Hello(h)) => {
            assert_eq!(h.classes, lab.host.sim.classes());
            assert_eq!(h.limits, limits());
        }
        other => panic!("{other:?}"),
    }
}

/// HEX-SOCK-4 (the core): connections past the bound, and a caller the OS says isn't this user, are each
/// refused by type, naming why, and let go; the held ones are served.
#[test]
fn strangers_and_connections_past_the_bound_are_refused_by_name() {
    let lab = Lab::new();
    let stranger = lab.os.connect(false, 64);
    match answer_of(&stranger) {
        (
            _,
            Answer::Refused(Refusal::Invalid {
                what: "caller",
                why,
            }),
        ) => {
            assert!(why.contains("simulated stranger"), "{why}")
        }
        other => panic!("{other:?}"),
    }
    let held: Vec<_> = (0..2).map(|_| lab.os.connect(true, 64)).collect();
    wait_until("both to be admitted", || lab.connections() == 2);
    let third = lab.os.connect(true, 64);
    assert_eq!(
        answer_of(&third).1,
        Answer::Refused(Refusal::Exhausted {
            what: CONNECTIONS,
            bound: 2
        })
    );
    lab.os.put(&held[0], hello());
    assert!(matches!(answer_of(&held[0]).1, Answer::Hello(_)));
}

/// HEX-SOCK-5 (the core): requests past a connection's in-flight bound are refused by type, and every
/// admitted one is answered once its host call returns.
#[test]
fn requests_past_the_in_flight_bound_are_refused_and_none_lost() {
    let lab = Lab::new();
    lab.host.hold(true);
    let sock = lab.os.connect(true, 64);
    for id in 1..=3 {
        lab.os.put(&sock, request(id, Call::Inspect(b())));
    }
    let (h, refused) = answer_of(&sock);
    assert_eq!(
        (h.id, refused),
        (
            3,
            Answer::Refused(Refusal::Exhausted {
                what: IN_FLIGHT,
                bound: 2
            })
        )
    );
    lab.host.hold(false);
    let mut ids: Vec<u64> = (0..2).map(|_| answer_of(&sock).0.id).collect();
    ids.sort_unstable();
    assert_eq!(ids, [1, 2]);
}

/// HEX-SOCK-7 (the core): a program's start is answered before its end, which comes as its exec's next
/// message once the OS's watch says it ended; the watch is then let go.
#[test]
fn a_programs_end_follows_its_start_and_its_watch_is_let_go() {
    let lab = Lab::new();
    let sock = lab.os.connect(true, 64);
    lab.os.put(&sock, exec(7));
    let pid = match answer_of(&sock) {
        (Header { id: 7, .. }, Answer::Started { pid }) => pid,
        other => panic!("{other:?}"),
    };
    assert_eq!(lab.watched(), 1);
    lab.os.end(pid);
    let (h, end) = answer_of(&sock);
    assert_eq!((h.id, end), (7, Answer::Exited(Exit::Code(0))));
    wait_until("the watch to be let go", || {
        lab.watched() == 0 && !lab.os.lock().programs.contains_key(&pid)
    });
}

/// HEX-SOCK-6 (the core), the disconnect rule: a program whose client goes is killed through the OS's
/// watch, reaped and counted in its box; execs past the programs bound are refused naming it.
#[test]
fn a_program_whose_client_goes_is_killed_and_counted_and_the_bound_holds() {
    let lab = Lab::new();
    let sock = lab.os.connect(true, 64);
    for id in 1..=2 {
        lab.os.put(&sock, exec(id));
        assert!(matches!(answer_of(&sock).1, Answer::Started { .. }));
    }
    lab.os.put(&sock, exec(3));
    assert_eq!(
        answer_of(&sock).1,
        Answer::Refused(Refusal::Exhausted {
            what: PROGRAMS,
            bound: 2
        })
    );
    lab.os.leave(&sock);
    wait_until("both programs to be counted in their box", || {
        lab.host.abandoned.lock().unwrap().len() == 2
    });
    assert_eq!(lab.os.lock().killed.len(), 2);
    wait_until("the connection and watches to go", || {
        lab.connections() == 0 && lab.watched() == 0
    });
}

/// HEX-SOCK-7 (the core), `Proc::kill` through the socket: a kill naming a program's exec is answered done,
/// sends KILL through the OS's watch to that program alone, and its end, `Signal(9)`, comes as its exec's
/// message, not counted as abandoned; a kill naming no running program of the connection is done too.
#[test]
fn a_kill_ends_its_program_alone_and_its_end_is_sent() {
    let lab = Lab::new();
    let sock = lab.os.connect(true, 64);
    let mut pids = Vec::new();
    for id in [7, 8] {
        lab.os.put(&sock, exec(id));
        match answer_of(&sock) {
            (Header { id: got, .. }, Answer::Started { pid }) if got == id => pids.push(pid),
            other => panic!("{other:?}"),
        }
    }
    lab.os.put(&sock, request(9, Call::Kill { exec: 7 }));
    let mut got = [answer_of(&sock), answer_of(&sock)].map(|(h, a)| (h.id, a));
    got.sort_by_key(|(id, _)| *id);
    assert_eq!(
        got,
        [(7, Answer::Exited(Exit::Signal(9))), (9, Answer::Done)]
    );
    assert_eq!(
        lab.os.lock().killed.iter().copied().collect::<Vec<_>>(),
        [pids[0]]
    );
    assert!(!lab.os.ended(pids[1]), "the other program was ended");
    lab.os.put(&sock, request(10, Call::Kill { exec: 7 }));
    let (h, again) = answer_of(&sock);
    assert_eq!((h.id, again), (10, Answer::Done));
    assert!(lab.host.abandoned.lock().unwrap().is_empty());
}

/// HEX-SOCK-5 (the core), native plan step 8 (1): a call its guest is slow to answer holds no other client's
/// call, even with one worker: client A's inspect of a guest box waits at the guest while client B's inspect
/// of a host box is answered; A's is answered once the guest answers.
#[test]
fn a_call_its_guest_holds_holds_no_other_clients_call() {
    let lab = Lab::with_workers(1);
    lab.host.guest.hold(true);
    let a = lab.os.connect(true, 64);
    let other = lab.os.connect(true, 64);
    lab.os.put(&a, request(1, Call::Inspect(g())));
    wait_until("the guest to hold A's call", || {
        lab.host.guest.holding() == 1
    });
    lab.os.put(&other, request(2, Call::Inspect(b())));
    let (h, answer) = answer_of(&other);
    assert!(
        matches!((h.id, &answer), (2, Answer::Info(_))),
        "{answer:?}"
    );
    lab.host.guest.hold(false);
    let (h, answer) = answer_of(&a);
    assert!(
        matches!((h.id, &answer), (1, Answer::Info(_))),
        "{answer:?}"
    );
}

/// HEX-SOCK-7 (the core), native plan step 8 (1): a program in the guest has a guest pid, which names no host
/// process, so the core never watches it as one; its end comes from the guest and goes to its exec as any
/// end does.
#[test]
fn a_guest_programs_end_comes_from_its_guest_and_no_host_pid_is_watched() {
    let lab = Lab::new();
    let sock = lab.os.connect(true, 64);
    lab.os.put(&sock, exec_in(7, g()));
    let pid = match answer_of(&sock) {
        (Header { id: 7, .. }, Answer::Started { pid }) => pid,
        other => panic!("{other:?}"),
    };
    assert!(
        !lab.os.lock().ever_watched.contains(&pid),
        "the core watched guest pid {pid} as a host process"
    );
    lab.host.guest.end(pid, Exit::Code(3));
    let (h, end) = answer_of(&sock);
    assert_eq!((h.id, end), (7, Answer::Exited(Exit::Code(3))));
}

/// HEX-SOCK-7 (the core), native plan step 8 (1): a kill of a guest program goes to its guest, is answered
/// once the guest says it was sent, and its end, `Signal(9)`, comes as its exec's message; the OS is asked to
/// kill nothing.
#[test]
fn a_guest_programs_kill_goes_to_its_guest() {
    let lab = Lab::new();
    let sock = lab.os.connect(true, 64);
    lab.os.put(&sock, exec_in(7, g()));
    assert!(matches!(answer_of(&sock).1, Answer::Started { .. }));
    lab.os.put(&sock, request(8, Call::Kill { exec: 7 }));
    let mut got = [answer_of(&sock), answer_of(&sock)].map(|(h, a)| (h.id, a));
    got.sort_by_key(|(id, _)| *id);
    assert_eq!(
        got,
        [(7, Answer::Exited(Exit::Signal(9))), (8, Answer::Done)]
    );
    assert!(lab.os.lock().killed.is_empty());
}

/// HEX-SOCK-6 (the core), native plan step 8 (1), the disconnect rule for a guest program: its client goes,
/// so it is killed through its guest, and its end is counted in its box, not sent.
#[test]
fn a_guest_program_whose_client_goes_is_killed_by_its_guest_and_counted() {
    let lab = Lab::new();
    let sock = lab.os.connect(true, 64);
    lab.os.put(&sock, exec_in(7, g()));
    assert!(matches!(answer_of(&sock).1, Answer::Started { .. }));
    lab.os.leave(&sock);
    wait_until("the guest program to be counted in its box", || {
        lab.host.abandoned.lock().unwrap().as_slice() == [g()]
    });
    wait_until("the connection and its program to go", || {
        lab.connections() == 0 && lab.watched() == 0
    });
    assert!(lab.os.lock().killed.is_empty());
}

/// HEX-SOCK-7 (the core): an exec whose program the OS can't watch is refused, saying the node ended it.
#[test]
fn an_exec_whose_program_cant_be_watched_is_refused() {
    let lab = Lab::new();
    lab.os.lock().unwatchable = true;
    let sock = lab.os.connect(true, 64);
    lab.os.put(&sock, exec(1));
    match answer_of(&sock).1 {
        Answer::Refused(Refusal::Os { op, why }) => {
            assert_eq!(op, "watching a started program");
            assert!(why.contains("ended it"), "{why}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(lab.watched(), 0);
}

/// HEX-SOCK-8 (the core): a client that stops reading stops being read: its answers wait for room, and once
/// it has room again they go, and only then are its later requests read.
#[test]
fn a_client_that_doesnt_read_stops_being_read_until_it_has_room() {
    let lab = Lab::new();
    let sock = lab.os.connect(true, 0);
    lab.os.put(&sock, request(1, Call::Inspect(b())));
    lab.os.put(&sock, request(2, Call::Inspect(b())));
    wait_until("both answers to wait for room", || {
        lab.server
            .shared
            .table
            .lock()
            .unwrap()
            .conns
            .values()
            .any(|c| {
                let out = c.out.lock().unwrap();
                out.backlog.len() == 2 && !out.reading
            })
    });
    lab.os.put(&sock, request(3, Call::Inspect(b())));
    lab.os.give_room(&sock, 64);
    let ids: Vec<u64> = (0..3).map(|_| answer_of(&sock).0.id).collect();
    // Calls 1 and 2 ran on two workers, which answer in the order they finish (the wire matches answers
    // to requests by id); 3 wasn't read until their answers had room, so it comes after both.
    let mut first: Vec<u64> = ids.get(..2).unwrap().to_vec();
    first.sort_unstable();
    assert_eq!(
        (first.as_slice(), ids[2]),
        ([1, 2].as_slice(), 3),
        "{ids:?}"
    );
}

/// The core when the OS runs out of descriptors: accepting pauses, and a connection that closes resumes it,
/// so a waiting client is admitted.
#[test]
fn accepting_pauses_when_descriptors_are_spent_and_resumes_on_a_close() {
    let lab = Lab::new();
    let first = lab.os.connect(true, 64);
    wait_until("the first to be admitted", || lab.connections() == 1);
    lab.os.lock().spent = true;
    let second = lab.os.connect(true, 64);
    wait_until("accepting to pause", || !lab.os.lock().accepting);
    lab.os.lock().spent = false;
    lab.os.leave(&first);
    wait_until("the second to be admitted", || {
        lab.connections() == 1 && lab.os.lock().pending.is_empty()
    });
    lab.os.put(&second, hello());
    assert!(matches!(answer_of(&second).1, Answer::Hello(_)));
}
