//! AC-T7.4.1: caller caps at the actual Core/guest setup seam, no VMM or GPU.

use super::*;
use crate::guest::{self, Duplex, Link};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

// These are caller-supplied fixture policies, not production calibration or performance targets.
const CAP: Duration = Duration::from_millis(25);
const OBSERVE: Duration = Duration::from_millis(100);

struct Stalled {
    socket: UdpSocket,
    sent: Mutex<Option<mpsc::SyncSender<()>>>,
    closed: AtomicBool,
    budget: Option<Duration>,
}

fn stalled(budget: Option<Duration>) -> (Stalled, mpsc::Receiver<()>) {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    socket.connect(socket.local_addr().unwrap()).unwrap();
    socket.set_nonblocking(true).unwrap();
    let (sent, request) = mpsc::sync_channel(1);
    (
        Stalled {
            socket,
            sent: Mutex::new(Some(sent)),
            closed: AtomicBool::new(false),
            budget,
        },
        request,
    )
}

impl Transport for Stalled {
    type Reading = ();
    fn answers() -> &'static [Patience; KINDS] {
        static WAITS: [Patience; KINDS] =
            [const { Patience::measuring("test.client_budget.unarmed") }; KINDS];
        &WAITS
    }
    fn reply_budget(&self) -> Option<Duration> {
        self.budget
    }
    fn reading(&self) {}
    fn handle(&self) -> Handle<'_> {
        #[cfg(unix)]
        return std::os::fd::AsFd::as_fd(&self.socket);
        #[cfg(windows)]
        std::os::windows::io::AsSocket::as_socket(&self.socket)
    }
    fn recv(&self, _: &mut (), _: &mut Take<'_>) -> Result<bool, Refusal> {
        let _ = self.socket.recv(&mut [0]);
        if self.closed.load(Ordering::Acquire) {
            Err(Refusal::Broken("owned original-control release".into()))
        } else {
            Ok(false)
        }
    }
    fn try_send(&self, bytes: &[u8], at: &mut usize, _: &[Lent<'_>]) -> Result<bool, Sent> {
        *at = bytes.len();
        if let Some(sent) = self.sent.lock().unwrap().take() {
            let _ = sent.send(());
        }
        Ok(true)
    }
    fn shutdown(&self) {
        self.closed.store(true, Ordering::Release);
        let _ = self.socket.send(&[1]);
    }
}

/// Do send an actual Core hello with an explicit caller cap and an unarmed learned wait;
/// expect caller expiry instead of waiting until the external original-control release.
#[test]
fn explicit_budget_caps_an_unarmed_core_hello() {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    socket.connect(socket.local_addr().unwrap()).unwrap();
    socket.set_nonblocking(true).unwrap();
    let (sent, request) = mpsc::sync_channel(1);
    let shared = Arc::new(Shared::new(Stalled {
        socket,
        sent: Mutex::new(Some(sent)),
        closed: AtomicBool::new(false),
        budget: Some(CAP),
    }));
    let control = Arc::clone(&shared);
    let (finished, answer) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let result = Core::hello(control).map(drop);
        finished.send(result).unwrap();
    });
    let observed_send = request.recv_timeout(OBSERVE);
    let result = answer.recv_timeout(OBSERVE);
    // Always release and join the exact owned actor before asserting the original symptom.
    shared.end(Refusal::Broken("owned original-control release".into()));
    worker.join().unwrap();
    observed_send.unwrap();
    eprintln!(
        "client_budget_control phase=unarmed-hello cap={CAP:?} observation={OBSERVE:?} returned={}",
        result.is_ok()
    );
    assert!(
        result.is_ok(),
        "unarmed hello ignored the configured caller cap"
    );
    assert!(matches!(result.unwrap(), Err(Refusal::Os { .. })));
}

struct TokenLink {
    address: SocketAddr,
}
impl Link for TokenLink {
    fn reply_budget(&self) -> Option<Duration> {
        Some(CAP)
    }
    fn dial(&self, _: u32) -> Result<Box<dyn Duplex>, Refusal> {
        TcpStream::connect_timeout(&self.address, CAP)
            .map(|stream| Box::new(stream) as Box<dyn Duplex>)
            .map_err(|error| Refusal::os("bounded original-control dial", error))
    }
    fn dial_before(
        &self,
        port: u32,
        deadline: Option<Instant>,
    ) -> Result<Box<dyn Duplex>, Refusal> {
        let Some(deadline) = deadline else {
            return self.dial(port);
        };
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Refusal::os(
                "bounded original-control dial",
                std::io::ErrorKind::TimedOut,
            ));
        }
        TcpStream::connect_timeout(&self.address, left)
            .map(|stream| Box::new(stream) as Box<dyn Duplex>)
            .map_err(|error| Refusal::os("bounded original-control dial", error))
    }
}

/// Do complete a bounded real loopback dial, then withhold the token;
/// expect the same caller cap to bound setup, rather than the peer's eventual close.
#[test]
fn explicit_budget_caps_token_after_a_bounded_dial() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let stop = AtomicBool::new(false);
    let wake = Wake::new().unwrap();
    let (finished, answer) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        finished
            .send(guest::Client::connect(Arc::new(TokenLink { address })).map(drop))
            .unwrap();
    });
    let peer = accept_before(
        &listener,
        &stop,
        &wake,
        Instant::now().checked_add(OBSERVE).unwrap(),
    );
    let result = answer.recv_timeout(OBSERVE);
    // No VM, signal or foreign PID: close only this test's peer and join its worker.
    let cleanup = peer.as_ref().map(|peer| peer.shutdown(Shutdown::Both));
    worker.join().unwrap();
    assert!(
        peer.is_ok(),
        "the token control must actually complete its dial"
    );
    assert!(
        matches!(cleanup, Ok(Ok(())))
            || matches!(cleanup, Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotConnected)
    );
    eprintln!(
        "client_budget_control phase=token cap={CAP:?} observation={OBSERVE:?} returned={}",
        result.is_ok()
    );
    assert!(
        result.is_ok(),
        "token read outlived the configured bounded dial budget"
    );
    assert!(matches!(result.unwrap(), Err(Refusal::Os { .. })));
}

fn configured<T: Transport>(transport: T) -> Core<T> {
    let client = Core {
        conn: Arc::new(Conn {
            shared: Arc::new(Shared::new(transport)),
        }),
        classes: vec![Class::Oci],
        limits: Limits {
            connections: 1,
            message_bytes: wire::MAX_MESSAGE,
            in_flight: 4,
            programs: 4,
        },
        budgets: wire::Budgets {
            in_flight: 4,
            programs: 4,
        },
    };
    client.conn.shared.owed.lock().unwrap().bound = client.budgets.programs;
    client
}

struct NoDial {
    budget: Duration,
    dials: AtomicUsize,
}
impl Link for NoDial {
    fn reply_budget(&self) -> Option<Duration> {
        Some(self.budget)
    }
    fn dial(&self, _: u32) -> Result<Box<dyn Duplex>, Refusal> {
        self.dials.fetch_add(1, Ordering::SeqCst);
        Err(Refusal::Broken("forbidden dial effect".into()))
    }
    fn dial_before(&self, port: u32, _: Option<Instant>) -> Result<Box<dyn Duplex>, Refusal> {
        self.dial(port)
    }
}

/// Do supply zero or an overflowing explicit budget; expect refusal before a request id,
/// transport write or Link dial is allocated/performed, rather than unbounded fallback.
#[test]
fn invalid_budgets_refuse_before_call_or_dial_effects() {
    for budget in [Duration::ZERO, Duration::MAX] {
        let (transport, request) = stalled(Some(budget));
        let client = configured(transport);
        assert!(matches!(
            client.list(),
            Err(Refusal::Invalid {
                what: "client reply budget",
                ..
            })
        ));
        let state = client.conn.shared.state().unwrap();
        assert_eq!(state.next, 0);
        assert!(state.calls.is_empty());
        assert!(state.programs.is_empty());
        drop(state);
        assert!(matches!(request.try_recv(), Err(mpsc::TryRecvError::Empty)));
        let link = Arc::new(NoDial {
            budget,
            dials: AtomicUsize::new(0),
        });
        assert!(matches!(
            guest::Client::connect(link.clone()),
            Err(Refusal::Invalid {
                what: "client reply budget",
                ..
            })
        ));
        assert_eq!(link.dials.load(Ordering::SeqCst), 0);
    }
}

/// Do queue an actual API call behind the existing whole-message sender; expect its own cap
/// to end acquisition with no write and no damage to the held sender or connection.
#[test]
fn queued_sender_consumes_its_own_budget_without_taking_authority() {
    let (transport, request) = stalled(Some(CAP));
    let client = Arc::new(configured(transport));
    let whole = client.conn.shared.sending.lock(None).unwrap().unwrap();
    let control = Arc::clone(&client);
    let (finished, answer) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || finished.send(control.list()).unwrap());
    let result = answer.recv_timeout(OBSERVE);
    // Release the exact owned guard even if the budget assertion fails, then join its waiter.
    drop(whole);
    if result.is_err() {
        client.conn.shared.end(Refusal::Broken(
            "owned queued-sender observation cleanup".into(),
        ));
    }
    worker.join().unwrap();
    let why = result.unwrap().unwrap_err();
    assert!(
        matches!(why, Refusal::Os { ref why, .. } if why.contains("caller") && !why.contains("twice"))
    );
    assert!(matches!(request.try_recv(), Err(mpsc::TryRecvError::Empty)));
    let state = client.conn.shared.state().unwrap();
    assert!(state.calls.is_empty());
    assert!(state.programs.is_empty());
    assert!(state.closed.is_none());
    assert!(!client.conn.shared.transport.closed.load(Ordering::Acquire));
}

/// Do omit the optional cap on an unarmed kind; expect the legacy wait to remain pending
/// until the exact connection is ended, rather than inheriting another client's cap.
#[test]
fn default_none_preserves_the_unarmed_wait() {
    let (transport, request) = stalled(None);
    let client = Arc::new(configured(transport));
    let control = Arc::clone(&client);
    let (finished, answer) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || finished.send(control.list()).unwrap());
    let observed_send = request.recv_timeout(OBSERVE);
    let before = answer.recv_timeout(CAP);
    client
        .conn
        .shared
        .end(Refusal::Broken("owned legacy-wait release".into()));
    worker.join().unwrap();
    observed_send.unwrap();
    assert!(matches!(before, Err(mpsc::RecvTimeoutError::Timeout)));
    assert!(matches!(
        answer.try_recv().unwrap(),
        Err(Refusal::Broken(_))
    ));
}

struct Calibrated<const LONG: bool>(Stalled);
impl<const LONG: bool> Transport for Calibrated<LONG> {
    type Reading = ();
    fn answers() -> &'static [Patience; KINDS] {
        static LONG_WAIT: [Patience; KINDS] =
            [const { Patience::seeded("test.client_budget.long", OBSERVE) }; KINDS];
        static SHORT: [Patience; KINDS] =
            [const { Patience::seeded("test.client_budget.short", Duration::from_millis(1)) };
                KINDS];
        if LONG { &LONG_WAIT } else { &SHORT }
    }
    fn reply_budget(&self) -> Option<Duration> {
        self.0.reply_budget()
    }
    fn reading(&self) {}
    fn handle(&self) -> Handle<'_> {
        self.0.handle()
    }
    fn recv(&self, reading: &mut (), take: &mut Take<'_>) -> Result<bool, Refusal> {
        self.0.recv(reading, take)
    }
    fn try_send(&self, bytes: &[u8], at: &mut usize, lent: &[Lent<'_>]) -> Result<bool, Sent> {
        self.0.try_send(bytes, at, lent)
    }
    fn shutdown(&self) {
        self.0.shutdown();
    }
}

fn timed_call<T: Transport>(transport: T, request: mpsc::Receiver<()>) -> Refusal {
    let client = Arc::new(configured(transport));
    let control = Arc::clone(&client);
    let (finished, answer) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || finished.send(control.list()).unwrap());
    let observed_send = request.recv_timeout(OBSERVE);
    let result = answer.recv_timeout(OBSERVE);
    client
        .conn
        .shared
        .end(Refusal::Broken("owned calibrated control release".into()));
    worker.join().unwrap();
    observed_send.unwrap();
    result.unwrap().unwrap_err()
}

/// Do exhaust caller and learned bounds separately in actual Core calls. The census runs in
/// a fresh exact-test child so it never competes with the existing refusal-census unit's OnceLock.
#[test]
fn timeout_cause_and_learning_keep_caller_authority_separate() {
    const CHILD: &str = "HEX_CLIENT_BUDGET_CENSUS_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "calls::budget_tests::timeout_cause_and_learning_keep_caller_authority_separate",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .spawn()
            .unwrap();
        let pid = child.id();
        let status = child.wait().unwrap();
        eprintln!("client_budget_census child_pid={pid} reaped=true exit={status}");
        assert!(status.success());
        return;
    }
    static SAMPLES: AtomicUsize = AtomicUsize::new(0);
    static RUN_OUT: AtomicUsize = AtomicUsize::new(0);
    fn sink(name: &'static str, sample: hex_core::patience::Sample) {
        if name.starts_with("test.client_budget.") {
            SAMPLES.fetch_add(1, Ordering::SeqCst);
            if name == "test.client_budget.short"
                && matches!(sample, hex_core::patience::Sample::RanOut(_))
            {
                RUN_OUT.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
    assert!(hex_core::patience::census(sink));
    let (transport, request) = stalled(Some(CAP));
    let caller = timed_call(Calibrated::<true>(transport), request);
    assert!(
        matches!(caller, Refusal::Os { why, .. } if why.contains("caller") && !why.contains("twice"))
    );
    assert_eq!(
        SAMPLES.load(Ordering::SeqCst),
        0,
        "caller expiry cannot calibrate a learned bound"
    );
    let (transport, request) = stalled(Some(CAP));
    let learned = timed_call(Calibrated::<false>(transport), request);
    assert!(
        matches!(learned, Refusal::Os { why, .. } if why.contains("twice") && !why.contains("caller"))
    );
    assert_eq!(SAMPLES.load(Ordering::SeqCst), 1);
    assert_eq!(RUN_OUT.load(Ordering::SeqCst), 1);
}

fn until(at: Instant) -> Result<(), Refusal> {
    let timer = Wake::new().map_err(|e| Refusal::os("making the fixture timer wake", e))?;
    loop {
        let left = at.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(());
        }
        // The Condvar fixture wait overshot the injection event in retained08a diagnostics.
        // Use the same readiness clock as the protocol, without changing its cap or guards.
        ready(timer.handle(), false, None, Some(left))?;
    }
}

#[derive(Clone, Copy, Debug)]
struct TokenRead {
    at: Instant,
    result: Result<usize, io::ErrorKind>,
}

struct ReadNotice {
    socket: TcpStream,
    read: mpsc::SyncSender<TokenRead>,
}
impl Read for ReadNotice {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let want = bytes.len().min(1);
        let result = self.socket.read(&mut bytes[..want]);
        self.read
            .try_send(TokenRead {
                at: Instant::now(),
                result: result.as_ref().copied().map_err(io::Error::kind),
            })
            .map_err(|error| io::Error::other(format!("token observation queue: {error}")))?;
        result
    }
}
impl Write for ReadNotice {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.socket.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}
impl Duplex for ReadNotice {
    fn try_clone(&self) -> io::Result<Box<dyn Duplex>> {
        Ok(Box::new(Self {
            socket: self.socket.try_clone()?,
            read: self.read.clone(),
        }))
    }
    fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        self.socket.shutdown(how)
    }
    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.socket.set_write_timeout(timeout)
    }
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.socket.set_read_timeout(timeout)
    }
    fn handle(&self) -> Handle<'_> {
        #[cfg(unix)]
        return std::os::fd::AsFd::as_fd(&self.socket);
        #[cfg(windows)]
        std::os::windows::io::AsSocket::as_socket(&self.socket)
    }
    fn set_nonblocking(&self, on: bool) -> io::Result<()> {
        self.socket.set_nonblocking(on)
    }
}

struct RecordedLink {
    control: SocketAddr,
    stdio: SocketAddr,
    budget: Duration,
    deadlines: Mutex<Vec<(u32, Instant)>>,
    read: Option<mpsc::SyncSender<TokenRead>>,
}
impl Link for RecordedLink {
    fn reply_budget(&self) -> Option<Duration> {
        Some(self.budget)
    }
    fn dial(&self, port: u32) -> Result<Box<dyn Duplex>, Refusal> {
        self.dial_before(port, Some(Instant::now().checked_add(self.budget).unwrap()))
    }
    fn dial_before(
        &self,
        port: u32,
        deadline: Option<Instant>,
    ) -> Result<Box<dyn Duplex>, Refusal> {
        let deadline = deadline.unwrap();
        self.deadlines.lock().unwrap().push((port, deadline));
        let address = if port == hex_core::stream::CONTROL_PORT {
            self.control
        } else {
            self.stdio
        };
        let socket = TcpStream::connect_timeout(
            &address,
            deadline.saturating_duration_since(Instant::now()),
        )
        .map_err(|error| Refusal::os("recorded loopback dial", error))?;
        // These controls deliberately send individual bytes/small frames; TCP coalescing is
        // unrelated to whether protocol progress consumes one deadline.
        socket.set_nodelay(true).unwrap();
        if port == hex_core::stream::CONTROL_PORT
            && let Some(read) = &self.read
        {
            Ok(Box::new(ReadNotice {
                socket,
                read: read.clone(),
            }))
        } else {
            Ok(Box::new(socket))
        }
    }
}

fn accept_before(
    listener: &TcpListener,
    stop: &AtomicBool,
    wake: &Wake,
    at: Instant,
) -> io::Result<TcpStream> {
    loop {
        if stop.load(Ordering::Acquire) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        match listener.accept() {
            Ok((peer, _)) => {
                peer.set_nonblocking(false)?;
                return Ok(peer);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(error),
        }
        let left = at.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        #[cfg(unix)]
        let handle = std::os::fd::AsFd::as_fd(listener);
        #[cfg(windows)]
        let handle = std::os::windows::io::AsSocket::as_socket(listener);
        ready(handle, false, Some(wake), Some(left))
            .map_err(|error| io::Error::other(error.to_string()))?;
    }
}

fn actual_token_read(
    reads: &mpsc::Receiver<TokenRead>,
    events: &mut Vec<TokenRead>,
    at: Instant,
) -> Result<Instant, Refusal> {
    loop {
        let event = reads
            .recv_timeout(at.saturating_duration_since(Instant::now()))
            .map_err(|error| Refusal::os("observing an actual token read", error))?;
        events.push(event);
        match event.result {
            Ok(n) if n > 0 => return Ok(event.at),
            Err(io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {}
            other => return Err(Refusal::Broken(format!("the token probe read {other:?}"))),
        }
    }
}

/// Do make real partial token progress just before the original deadline; expect expiry at
/// that deadline, rather than another whole cap after the most recent byte.
#[test]
fn partial_token_progress_does_not_reset_the_absolute_deadline() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let stop = AtomicBool::new(false);
    let wake = Wake::new().unwrap();
    // The fixed token has eight bytes; this two-byte case records positive and WouldBlock reads.
    let (read, reads) = mpsc::sync_channel(hex_core::stream::TOKEN_BYTES * 2);
    let link = Arc::new(RecordedLink {
        control: address,
        stdio: address,
        budget: OBSERVE,
        deadlines: Mutex::new(Vec::new()),
        read: Some(read),
    });
    let control = Arc::clone(&link);
    let (finished, answer) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        finished
            .send(guest::Client::connect(control).map(drop))
            .unwrap()
    });
    let mut peer = None;
    let mut events = Vec::new();
    let observation = (|| -> Result<_, Refusal> {
        peer = Some(
            accept_before(
                &listener,
                &stop,
                &wake,
                Instant::now().checked_add(OBSERVE).unwrap(),
            )
            .map_err(|error| Refusal::os("accepting the owned token peer", error))?,
        );
        let peer = peer.as_mut().unwrap();
        peer.set_nodelay(true)
            .map_err(|error| Refusal::os("configuring token probe TCP", error))?;
        peer.write_all(&[1])
            .map_err(|error| Refusal::os("writing the first token byte", error))?;
        let first = actual_token_read(
            &reads,
            &mut events,
            Instant::now().checked_add(OBSERVE).unwrap(),
        )?;
        let deadline = link
            .deadlines
            .lock()
            .unwrap()
            .first()
            .copied()
            .ok_or_else(|| Refusal::Broken("the token peer had no recorded dial".into()))?
            .1;
        until(deadline.checked_sub(CAP).unwrap())?;
        if Instant::now() >= deadline {
            return Err(Refusal::Broken(
                "harness miss: the original token injection window elapsed".into(),
            ));
        }
        peer.write_all(&[2])
            .map_err(|error| Refusal::os("writing the second token byte", error))?;
        let second = actual_token_read(
            &reads,
            &mut events,
            Instant::now().checked_add(CAP).unwrap(),
        )?;
        Ok((first, second, deadline))
    })();
    let completed = answer.recv_timeout(CAP.checked_mul(2).unwrap());
    // Every observation error, including the first event/dial failure, reaches this cleanup.
    let cleanup = peer.as_ref().map(|peer| peer.shutdown(Shutdown::Both));
    drop(peer);
    stop.store(true, Ordering::Release);
    wake.wake();
    worker.join().unwrap();
    let after_cleanup = completed.is_err().then(|| answer.try_recv().ok()).flatten();
    events.extend(reads.try_iter());
    let witnessed = observation
        .as_ref()
        .is_ok_and(|(first, second, deadline)| first < second && second < deadline);
    eprintln!(
        "client_budget_token harness_miss={} observation={observation:?} actual_reads={events:?} client_before_cleanup={completed:?} client_after_cleanup={after_cleanup:?}",
        !witnessed
    );
    assert!(cleanup.is_none_or(|result| result.is_ok()
        || matches!(result, Err(error) if error.kind() == io::ErrorKind::NotConnected)));
    let (first, second, deadline) = observation.unwrap();
    assert!(
        first < second && second < deadline,
        "both actual partial reads must precede the original deadline"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event.result, Err(io::ErrorKind::WouldBlock)))
    );
    assert!(matches!(completed.unwrap(), Err(Refusal::Os { why, .. }) if why.contains("caller")));
    assert_eq!(link.deadlines.lock().unwrap().len(), 1);
}

fn next_request(peer: &mut TcpStream) -> io::Result<(Header, Request)> {
    let mut prefix = [0; size_of::<u32>()];
    peer.read_exact(&mut prefix)?;
    let length = usize::try_from(u32::from_be_bytes(prefix)).unwrap();
    assert!(length <= wire::MAX_MESSAGE);
    let mut bytes = vec![0; length];
    peer.read_exact(&mut bytes)?;
    Ok(wire::read_request_apart(&bytes, wire::MAX_MESSAGE).unwrap())
}
fn answer_to(peer: &mut TcpStream, id: u64, answer: Answer) -> io::Result<()> {
    peer.write_all(
        &hex_core::stream::frame(
            &wire::answer(id, &answer, wire::MAX_MESSAGE).unwrap(),
            wire::MAX_MESSAGE,
        )
        .unwrap(),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ControlPhase {
    Start,
    Delete,
    Disconnected,
    Stop,
}

fn pending_control(
    peer: &TcpStream,
    phases: &mpsc::Receiver<ControlPhase>,
    expected: ControlPhase,
) -> io::Result<()> {
    // Workload idle is not a protocol wait. The owner always sends Stop/closes peers on error;
    // the unchanged read observation guard starts only when a control operation is actually pending.
    peer.set_read_timeout(None)?;
    match phases.recv() {
        Ok(phase) if phase == expected => peer.set_read_timeout(Some(OBSERVE)),
        Ok(ControlPhase::Stop) | Err(_) => Err(io::ErrorKind::Interrupted.into()),
        Ok(other) => Err(io::Error::other(format!(
            "expected {expected:?}, got {other:?}"
        ))),
    }
}

enum StreamPhase {
    WriteLate,
    Stop,
}

/// Do ask for all three streams in one actual guest exec; expect one common setup deadline,
/// later API use after it expired, and successful stdio/Proc completion beyond the cap.
#[test]
fn named_streams_share_one_operation_cap_without_a_workload_or_connection_timer() {
    use hex_core::stream::{self, Named};
    let control = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let stdio = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    control.set_nonblocking(true).unwrap();
    stdio.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let wake = Arc::new(Wake::new().unwrap());
    let owned = Arc::new(Mutex::new(Vec::<TcpStream>::new()));
    let link = Arc::new(RecordedLink {
        control: control.local_addr().unwrap(),
        stdio: stdio.local_addr().unwrap(),
        budget: CAP,
        deadlines: Mutex::new(Vec::new()),
        read: None,
    });
    let (agent_stop, agent_wake, agent_owned) =
        (Arc::clone(&stop), Arc::clone(&wake), Arc::clone(&owned));
    let (agent_finished, agent_done) = mpsc::sync_channel(1);
    // At most one pending operation followed by Stop can await this actor.
    let (phase, pending) = mpsc::sync_channel(2);
    let agent = std::thread::spawn(move || {
        let result = (|| -> io::Result<()> {
            let mut peer = accept_before(
                &control,
                &agent_stop,
                &agent_wake,
                Instant::now().checked_add(OBSERVE).unwrap(),
            )?;
            agent_owned.lock().unwrap().push(peer.try_clone()?);
            peer.set_nodelay(true)?;
            peer.set_read_timeout(Some(OBSERVE))?;
            peer.write_all(&11u64.to_le_bytes())?;
            let (exec, request) = next_request(&mut peer)?;
            eprintln!("client_budget_named phase=agent_exec id={}", exec.id);
            assert!(matches!(
                request,
                Request::Call(Call::Exec {
                    streams: Streams {
                        stdin: true,
                        stdout: true,
                        stderr: true
                    },
                    ..
                })
            ));
            answer_to(&mut peer, exec.id, Answer::Started { pid: 7 })?;
            pending_control(&peer, &pending, ControlPhase::Start)?;
            let (header, request) = next_request(&mut peer)?;
            eprintln!("client_budget_named phase=agent_start id={}", header.id);
            assert!(matches!(request, Request::Call(Call::Start(_))));
            answer_to(&mut peer, header.id, Answer::Done)?;
            pending_control(&peer, &pending, ControlPhase::Delete)?;
            let (header, request) = next_request(&mut peer)?;
            assert!(matches!(request, Request::Call(Call::Delete(_))));
            answer_to(&mut peer, header.id, Answer::Done)?;
            answer_to(&mut peer, exec.id, Answer::Exited(Exit::Code(0)))?;
            eprintln!(
                "client_budget_named phase=agent_exited_written id={}",
                exec.id
            );
            pending_control(&peer, &pending, ControlPhase::Disconnected)?;
            // A completed Proc sends no Release; dropping the last client closes this control.
            assert_eq!(peer.read(&mut [0])?, 0);
            Ok(())
        })();
        let _ = agent_finished.send(());
        result
    });
    // One WriteLate and then Stop; neither idle phase is a workload lifetime timer.
    let (late, stream_phase) = mpsc::sync_channel(2);
    let (streams_stop, streams_wake, streams_owned) =
        (Arc::clone(&stop), Arc::clone(&wake), Arc::clone(&owned));
    let streams = std::thread::spawn(move || -> io::Result<Vec<Named>> {
        let mut held = Vec::new();
        let accepting = Instant::now().checked_add(OBSERVE).unwrap();
        for _ in 0..3 {
            let mut peer = accept_before(&stdio, &streams_stop, &streams_wake, accepting)?;
            streams_owned.lock().unwrap().push(peer.try_clone()?);
            peer.set_nodelay(true)?;
            peer.set_read_timeout(Some(OBSERVE))?;
            let mut name = [0; stream::NAME_BYTES];
            peer.read_exact(&mut name)?;
            held.push((stream::read_name(&name).unwrap(), peer));
        }
        if matches!(stream_phase.recv(), Ok(StreamPhase::WriteLate)) {
            for (name, peer) in &mut held {
                if name.stream == stream::Stdio::Out {
                    peer.write_all(b"late")?;
                }
            }
            let _ = stream_phase.recv();
        }
        Ok(held.into_iter().map(|(name, _)| name).collect())
    });
    let mut reading = None;
    let result = (|| -> Result<_, Refusal> {
        let transport = guest::Stream::dial(link.clone())?;
        let client = configured(transport);
        let id = BoxId::new("budget-unit")?;
        let mut proc = client.exec(
            &id,
            Program::of(&["cat"]),
            Stdio {
                ask: hex_api::Ask {
                    stdin: true,
                    stdout: true,
                    stderr: true,
                },
                ..Stdio::null()
            },
        )?;
        eprintln!("client_budget_named phase=client_started");
        let deadlines = link.deadlines.lock().unwrap().clone();
        let deadline = deadlines.last().unwrap().1;
        until(deadline)?;
        let pending = proc.try_wait()?;
        eprintln!("client_budget_named phase=client_proc_pending value={pending:?}");
        phase
            .send(ControlPhase::Start)
            .map_err(|error| Refusal::os("announcing pending fixture start", error))?;
        client.start(&id)?;
        late.send(StreamPhase::WriteLate)
            .map_err(|error| Refusal::os("requesting late fixture stdio", error))?;
        let given = proc.streams()?;
        let mut stdout = hex_api::OwnedStream::from(
            given
                .stdout
                .ok_or_else(|| Refusal::Broken("fixture stdout was absent".into()))?,
        );
        let (read, bytes) = mpsc::sync_channel(1);
        reading = Some(std::thread::spawn(move || {
            let mut bytes = [0; 4];
            let result = stdout.read_exact(&mut bytes).map(|()| bytes);
            let _ = read.send(result);
        }));
        let bytes = bytes
            .recv_timeout(OBSERVE)
            .map_err(|error| Refusal::os("observing requested late fixture bytes", error))?
            .map_err(|error| Refusal::os("reading late fixture stdio", error))?;
        eprintln!("client_budget_named phase=client_late_stdio_read");
        phase
            .send(ControlPhase::Delete)
            .map_err(|error| Refusal::os("announcing pending fixture delete", error))?;
        client.delete(&id)?;
        let ended = proc.wait_for(OBSERVE)?;
        drop(proc);
        let clean = {
            let state = client.conn.shared.state()?;
            state.calls.is_empty() && state.programs.is_empty() && state.given.is_empty()
        };
        phase
            .send(ControlPhase::Disconnected)
            .map_err(|error| Refusal::os("announcing fixture disconnect", error))?;
        drop(client);
        Ok((deadlines, pending, bytes, ended, clean))
    })();
    let _ = phase.send(ControlPhase::Stop);
    let _ = late.send(StreamPhase::Stop);
    let natural_agent_end = agent_done.recv_timeout(OBSERVE);
    // Failed dial/setup before all three accepts still reaches a wakeable, bounded accept loop.
    stop.store(true, Ordering::Release);
    wake.wake();
    if result.is_err() || natural_agent_end.is_err() {
        for peer in owned.lock().unwrap().iter() {
            let _ = peer.shutdown(Shutdown::Both);
        }
    }
    let names = streams.join().unwrap();
    let agent_result = agent.join().unwrap();
    if let Some(reading) = reading {
        reading.join().unwrap();
    }
    eprintln!(
        "client_budget_named result={result:?} agent={agent_result:?} named_count={:?}",
        names.as_ref().map(Vec::len)
    );
    // No assertion can strand either owned accept/read actor.
    let (deadlines, pending, bytes, ended, clean) = result.unwrap();
    natural_agent_end.unwrap();
    let names = names.unwrap();
    agent_result.unwrap();
    assert_eq!(
        deadlines.len(),
        4,
        "one control dial, then three named dials"
    );
    assert!(
        deadlines[1..]
            .iter()
            .all(|(port, at)| *port == stream::STDIO_PORT && *at == deadlines[1].1)
    );
    assert_eq!(pending, None);
    assert_eq!(&bytes, b"late");
    assert_eq!(ended, Some(Exit::Code(0)));
    assert!(clean);
    assert_eq!(names.len(), 3);
    assert!(
        names
            .iter()
            .all(|name| name.exec.conn == 11 && name.exec.id == names[0].exec.id)
    );
}

struct Offers {
    io: Stalled,
    inbound: Mutex<VecDeque<Vec<u8>>>,
    outbound: Mutex<Vec<(u64, Request)>>,
    entered: mpsc::SyncSender<ReplyDeadline>,
    program: Mutex<Option<Stdio>>,
}
impl Offers {
    fn queue(&self, id: u64, answer: Answer) {
        self.inbound
            .lock()
            .unwrap()
            .push_back(wire::answer(id, &answer, wire::MAX_MESSAGE).unwrap());
        self.io.socket.send(&[1]).unwrap();
    }
}
impl Transport for Offers {
    type Reading = ();
    fn answers() -> &'static [Patience; KINDS] {
        Stalled::answers()
    }
    fn reply_budget(&self) -> Option<Duration> {
        Some(OBSERVE)
    }
    fn reading(&self) {}
    fn handle(&self) -> Handle<'_> {
        self.io.handle()
    }
    fn recv(&self, _: &mut (), take: &mut Take<'_>) -> Result<bool, Refusal> {
        while self.io.socket.recv(&mut [0]).is_ok() {}
        let mut came = false;
        loop {
            let Some(bytes) = self.inbound.lock().unwrap().pop_front() else {
                return Ok(came);
            };
            take(
                &Received {
                    size: bytes.len(),
                    bytes: &bytes,
                    descriptors: 0,
                    descriptors_cut: false,
                },
                Vec::new(),
            )?;
            came = true;
        }
    }
    fn try_send(&self, bytes: &[u8], at: &mut usize, _: &[Lent<'_>]) -> Result<bool, Sent> {
        let (header, request) = wire::read_request_apart(bytes, wire::MAX_MESSAGE).unwrap();
        match &request {
            Request::Call(Call::ExecStreams { .. }) => self.queue(
                header.id,
                Answer::Offered(wire::StreamOffer {
                    pid: 1,
                    started: 1,
                    vsock: "owned-budget-offer".into(),
                    token: 1,
                    exec: header.id,
                    streams: Streams {
                        stdin: true,
                        ..Streams::default()
                    },
                }),
            ),
            Request::Call(Call::Start(_)) => self.queue(header.id, Answer::Done),
            Request::Release => {}
            other => panic!("unexpected budget offer request: {other:?}"),
        }
        self.outbound.lock().unwrap().push((header.id, request));
        *at = bytes.len();
        Ok(true)
    }
    fn open_offer_before(
        &self,
        _: &wire::StreamOffer,
        deadline: ReplyDeadline,
    ) -> Result<hex_api::Given, Refusal> {
        let mut program = Stdio {
            ask: hex_api::Ask {
                stdin: true,
                stdout: false,
                stderr: false,
            },
            ..Stdio::null()
        };
        let given = program.make_asked()?;
        *self.program.lock().unwrap() = Some(program);
        self.entered.send(deadline).unwrap();
        until(deadline.at())?;
        drop(given);
        Err(deadline
            .remaining("opening budgeted offered streams")
            .unwrap_err())
    }
    fn shutdown(&self) {
        self.io.shutdown();
    }
}

/// Do stall one bounded offered-stream open while a sibling completes; expect reader progress,
/// same-id Release, closed made ends and no remaining call/program/given reservation at expiry.
#[test]
fn offered_expiry_releases_only_its_ownership_while_a_sibling_progresses() {
    let (io, _) = stalled(Some(OBSERVE));
    let (entered, opening) = mpsc::sync_channel(1);
    let client = Arc::new(configured(Offers {
        io,
        inbound: Mutex::new(VecDeque::new()),
        outbound: Mutex::new(Vec::new()),
        entered,
        program: Mutex::new(None),
    }));
    let control = Arc::clone(&client);
    let id = BoxId::new("offered-budget-unit").unwrap();
    let target = id.clone();
    let (finished, answer) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        finished
            .send(
                control
                    .exec(
                        &target,
                        Program::of(&["cat"]),
                        Stdio {
                            ask: hex_api::Ask {
                                stdin: true,
                                stdout: false,
                                stderr: false,
                            },
                            ..Stdio::null()
                        },
                    )
                    .map(drop),
            )
            .unwrap()
    });
    let opened = opening.recv_timeout(OBSERVE);
    let entered_before_expiry = opened
        .as_ref()
        .is_ok_and(|deadline| deadline.at() > Instant::now());
    let reading = client.conn.shared.state().unwrap().reading;
    let sibling = client.start(&id);
    let expired = answer.recv_timeout(OBSERVE);
    worker.join().unwrap();
    // Even a missing/late opening event must not strand the call actor.
    opened.unwrap();
    let shared = &client.conn.shared;
    let outbound = shared.transport.outbound.lock().unwrap().clone();
    let state = shared.state().unwrap();
    let clean = state.calls.is_empty()
        && state.programs.is_empty()
        && state.given.is_empty()
        && state.closed.is_none();
    drop(state);
    let mut stdin = shared
        .transport
        .program
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .stdin
        .unwrap();
    let (read, eof) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || read.send(stdin.read(&mut [0])).unwrap());
    let closed = eof.recv_timeout(OBSERVE);
    // If a future regression retained ends in the owned ledger, clear only that ledger to reap
    // the observation actor; the earlier clean/EOF witnesses still make the test fail.
    shared.state().unwrap().given.clear();
    reader.join().unwrap();
    assert!(entered_before_expiry);
    assert!(!reading, "an offered open must relinquish the reader turn");
    assert_eq!(sibling, Ok(()));
    assert!(matches!(expired.unwrap(), Err(Refusal::Os { why, .. }) if why.contains("caller")));
    assert!(clean);
    assert_eq!(closed.unwrap().unwrap(), 0);
    assert_eq!(outbound.len(), 3);
    assert_eq!(outbound[0].0, outbound[2].0);
    assert!(matches!(outbound[1].1, Request::Call(Call::Start(_))));
    assert_eq!(outbound[2].1, Request::Release);
}
