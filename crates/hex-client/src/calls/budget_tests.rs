//! AC-T7.4.1: caller caps at the actual Core/guest setup seam, no VMM or GPU.

use super::*;
use crate::guest::{self, Duplex, DuplexOwner, Link, own_duplex};
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
    let shared = Shared::try_new(Stalled {
        socket,
        sent: Mutex::new(Some(sent)),
        closed: AtomicBool::new(false),
        budget: Some(CAP),
    })
    .unwrap();
    let control = triomphe::Arc::clone(&shared);
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
    fn dial(&self, _: u32) -> Result<DuplexOwner, Refusal> {
        TcpStream::connect_timeout(&self.address, CAP)
            .map_err(|error| Refusal::os("bounded original-control dial", error))
            .and_then(own_duplex)
    }
    fn dial_before(&self, port: u32, deadline: Option<Instant>) -> Result<DuplexOwner, Refusal> {
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
            .map_err(|error| Refusal::os("bounded original-control dial", error))
            .and_then(own_duplex)
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
            .send(
                guest::own_link(TokenLink { address })
                    .and_then(guest::Client::connect)
                    .map(drop),
            )
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
        conn: triomphe::Arc::try_new(Conn {
            shared: Shared::try_new(transport).unwrap(),
        })
        .unwrap(),
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
    dials: Arc<AtomicUsize>,
}
impl Link for NoDial {
    fn reply_budget(&self) -> Option<Duration> {
        Some(self.budget)
    }
    fn dial(&self, _: u32) -> Result<DuplexOwner, Refusal> {
        self.dials.fetch_add(1, Ordering::SeqCst);
        Err(Refusal::Broken("forbidden dial effect".into()))
    }
    fn dial_before(&self, port: u32, _: Option<Instant>) -> Result<DuplexOwner, Refusal> {
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
        let dials = Arc::new(AtomicUsize::new(0));
        let link = guest::own_link(NoDial {
            budget,
            dials: Arc::clone(&dials),
        })
        .unwrap();
        assert!(matches!(
            guest::Client::connect(link.clone()),
            Err(Refusal::Invalid {
                what: "client reply budget",
                ..
            })
        ));
        assert_eq!(dials.load(Ordering::SeqCst), 0);
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
    fn try_clone(&self) -> io::Result<DuplexOwner> {
        own_duplex(Self {
            socket: self.socket.try_clone()?,
            read: self.read.clone(),
        })
        .map_err(|_| io::Error::from(io::ErrorKind::OutOfMemory))
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
    deadlines: Arc<Mutex<Vec<(u32, Instant)>>>,
    read: Option<mpsc::SyncSender<TokenRead>>,
}
impl Link for RecordedLink {
    fn reply_budget(&self) -> Option<Duration> {
        Some(self.budget)
    }
    fn dial(&self, port: u32) -> Result<DuplexOwner, Refusal> {
        self.dial_before(port, Some(Instant::now().checked_add(self.budget).unwrap()))
    }
    fn dial_before(&self, port: u32, deadline: Option<Instant>) -> Result<DuplexOwner, Refusal> {
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
            own_duplex(ReadNotice {
                socket,
                read: read.clone(),
            })
        } else {
            own_duplex(socket)
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
    let recorded_deadlines = Arc::new(Mutex::new(Vec::new()));
    let link = guest::own_link(RecordedLink {
        control: address,
        stdio: address,
        budget: OBSERVE,
        deadlines: Arc::clone(&recorded_deadlines),
        read: Some(read),
    })
    .unwrap();
    let control = link.clone();
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
        let deadline = recorded_deadlines
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
    assert_eq!(recorded_deadlines.lock().unwrap().len(), 1);
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
    let recorded_deadlines = Arc::new(Mutex::new(Vec::new()));
    let link = guest::own_link(RecordedLink {
        control: control.local_addr().unwrap(),
        stdio: stdio.local_addr().unwrap(),
        budget: CAP,
        deadlines: Arc::clone(&recorded_deadlines),
        read: None,
    })
    .unwrap();
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
        let deadlines = recorded_deadlines.lock().unwrap().clone();
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
    let outbound = shared.transport.outbound.lock().unwrap();
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

fn absolute_spec() -> BoxSpec {
    BoxSpec {
        id: BoxId::new("absolute-operation-control").unwrap(),
        bounds: hex_api::Bounds::default(),
        class: hex_api::ClassSpec::Process,
        owner: None,
        devices: Vec::new(),
        allocation: None,
        route: None,
    }
}

/// A private, initially Measuring table so these controls never depend on other tests'
/// process-wide census samples. It retains the existing Stalled I/O/cleanup fixture.
struct AbsoluteStalled(Stalled);
impl Transport for AbsoluteStalled {
    type Reading = ();
    fn answers() -> &'static [Patience; KINDS] {
        static WAITS: [Patience; KINDS] =
            [const { Patience::measuring("test.client_absolute.unarmed") }; KINDS];
        &WAITS
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

/// AC-T7.4.1: an expired absolute operation refuses before call/program IDs,
/// writes, or stream setup; the same client remains usable by its owner.
#[test]
fn absolute_operations_reject_expired_before_effects() {
    let (transport, request) = stalled(None);
    let client = configured(AbsoluteStalled(transport));
    let expired = Instant::now();
    let run = client.run_before(
        absolute_spec(),
        Program::of(&["unused"]),
        Stdio::null(),
        expired,
    );
    let delete = client.delete_before(&BoxId::new("absolute-operation-control").unwrap(), expired);
    assert!(matches!(run, Err(Refusal::Os { why, .. }) if why.contains("absolute")));
    assert!(matches!(delete, Err(Refusal::Os { why, .. }) if why.contains("absolute")));
    assert!(matches!(request.try_recv(), Err(mpsc::TryRecvError::Empty)));
    let state = client.conn.shared.state().unwrap();
    assert_eq!(state.next, 0);
    assert!(state.calls.is_empty() && state.programs.is_empty() && state.given.is_empty());
    assert!(state.closed.is_none());
}

/// AC-T7.4.1: run/delete use the actual Core answer path with no configured Link
/// cap and an unarmed Measuring wait. Caller expiry is not a learned sample.
#[test]
fn absolute_operations_cap_unarmed_answers_without_link_defaults() {
    for run in [true, false] {
        let (transport, request) = stalled(None);
        let client = Arc::new(configured(AbsoluteStalled(transport)));
        let control = Arc::clone(&client);
        let (finished, answer) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now().checked_add(CAP).unwrap();
            let result = if run {
                control
                    .run_before(
                        absolute_spec(),
                        Program::of(&["unused"]),
                        Stdio::null(),
                        deadline,
                    )
                    .map(drop)
            } else {
                control.delete_before(&BoxId::new("absolute-operation-control").unwrap(), deadline)
            };
            let _ = finished.send(result);
        });
        let observed_send = request.recv_timeout(OBSERVE);
        let result = answer.recv_timeout(OBSERVE);
        // Every observation failure still closes this exact connection and joins its actor.
        client.conn.shared.end(Refusal::Broken(
            "absolute operation observation cleanup".into(),
        ));
        let joined = worker.join();
        eprintln!(
            "client_absolute run={run} observed_send={observed_send:?} result={result:?} joined={}",
            joined.is_ok()
        );
        joined.unwrap();
        observed_send.unwrap();
        assert!(
            matches!(result.unwrap(), Err(Refusal::Os { why, .. }) if why.contains("absolute") && !why.contains("twice"))
        );
        let state = client.conn.shared.state().unwrap();
        assert!(state.calls.is_empty() && state.programs.is_empty() && state.given.is_empty());
    }
    assert!(
        AbsoluteStalled::answers()
            .iter()
            .all(|wait| wait.deadline().is_none())
    );
}

/// AC-T7.4.1: the same caller deadline bounds sender ownership without taking
/// another sender's guard or positively reporting remote cleanup.
#[test]
fn absolute_delete_cap_covers_queued_sender() {
    let (transport, request) = stalled(None);
    let client = Arc::new(configured(AbsoluteStalled(transport)));
    let whole = client.conn.shared.sending.lock(None).unwrap().unwrap();
    let control = Arc::clone(&client);
    let (finished, answer) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let deadline = Instant::now().checked_add(CAP).unwrap();
        let _ = finished.send(
            control.delete_before(&BoxId::new("absolute-operation-control").unwrap(), deadline),
        );
    });
    let result = answer.recv_timeout(OBSERVE);
    drop(whole);
    if result.is_err() {
        client
            .conn
            .shared
            .end(Refusal::Broken("absolute queued-send cleanup".into()));
    }
    let joined = worker.join();
    joined.unwrap();
    assert!(matches!(result.unwrap(), Err(Refusal::Os { why, .. }) if why.contains("absolute")));
    assert!(matches!(request.try_recv(), Err(mpsc::TryRecvError::Empty)));
    let state = client.conn.shared.state().unwrap();
    assert!(state.calls.is_empty() && state.programs.is_empty() && state.given.is_empty());
    assert!(state.closed.is_none());
    assert!(
        !client
            .conn
            .shared
            .transport
            .0
            .closed
            .load(Ordering::Acquire)
    );
}

/// AC-T7.4.1: supplied instants survive unchanged; an earlier transport/learned
/// bound may shorten authority, while no later/default bound can enlarge it.
#[test]
fn absolute_caller_limit_keeps_original_instant_and_smaller_bounds() {
    let began = Instant::now();
    let at = began.checked_add(OBSERVE).unwrap();
    let absolute = ReplyDeadline::caller_before(at).unwrap();
    assert_eq!(absolute.at(), at);
    assert_eq!(absolute.limited_by(None).at(), at);
    let earlier = began.checked_add(CAP).unwrap();
    let configured = ReplyDeadline {
        at: earlier,
        source: ReplyBound::Caller(CAP),
    };
    let limited = absolute.limited_by(Some(configured));
    assert_eq!(limited.at(), earlier);
    let later = ReplyDeadline {
        at,
        source: ReplyBound::Caller(OBSERVE),
    };
    assert_eq!(limited.limited_by(Some(later)).at(), earlier);
    let tied = absolute.limited_by(Some(later));
    assert!(matches!(tied.source, ReplyBound::CallerAbsolute));
    assert_eq!(
        ReplyDeadline::combine(Some(absolute), began, None)
            .unwrap()
            .at(),
        at
    );
    let learned = ReplyDeadline::combine(Some(absolute), began, Some(CAP)).unwrap();
    assert_eq!(learned.at(), earlier);
    assert!(matches!(learned.source, ReplyBound::Learned(_)));
}

/// Shared observations of each actual port and its original optional dial deadline.
type DialDeadlines = Arc<Mutex<Vec<(u32, Option<Instant>)>>>;

struct AbsoluteNamedLink {
    control: Mutex<Option<TcpStream>>,
    stdio: Mutex<VecDeque<TcpStream>>,
    deadlines: DialDeadlines,
}

impl Link for AbsoluteNamedLink {
    fn dial(&self, port: u32) -> Result<DuplexOwner, Refusal> {
        self.dial_before(port, None)
    }
    fn dial_before(&self, port: u32, deadline: Option<Instant>) -> Result<DuplexOwner, Refusal> {
        self.deadlines.lock().unwrap().push((port, deadline));
        if deadline.is_some_and(|at| at <= Instant::now()) {
            return Err(Refusal::os(
                "absolute fixture dial",
                io::ErrorKind::TimedOut,
            ));
        }
        let stream = if port == hex_core::stream::CONTROL_PORT {
            self.control.lock().unwrap().take()
        } else {
            self.stdio.lock().unwrap().pop_front()
        };
        stream
            .ok_or_else(|| Refusal::Broken("owned absolute fixture stream absent".into()))
            .and_then(own_duplex)
    }
}

fn absolute_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let client = TcpStream::connect_timeout(&listener.local_addr().unwrap(), OBSERVE).unwrap();
    let peer = accept_before(
        &listener,
        &AtomicBool::new(false),
        &Wake::new().unwrap(),
        Instant::now().checked_add(OBSERVE).unwrap(),
    )
    .unwrap();
    client.set_nodelay(true).unwrap();
    peer.set_nodelay(true).unwrap();
    (client, peer)
}

/// AC-T7.4.1: actual guest Stream run setup with no Link default writes all three
/// native named headers under one caller cap. No server answers the Run, so the
/// same cap must also bound its unarmed control answer and close the asked ends.
#[test]
fn absolute_run_caps_named_stdio_and_preserves_headers() {
    use hex_core::stream::{self, Named};
    let (control, mut control_peer) = absolute_pair();
    control_peer.write_all(&11u64.to_le_bytes()).unwrap();
    let mut named = VecDeque::new();
    let mut peers = Vec::new();
    for _ in 0..3 {
        let (stream, peer) = absolute_pair();
        named.push_back(stream);
        peers.push(peer);
    }
    let deadlines = Arc::new(Mutex::new(Vec::new()));
    let link = guest::own_link(AbsoluteNamedLink {
        control: Mutex::new(Some(control)),
        stdio: Mutex::new(named),
        deadlines: Arc::clone(&deadlines),
    })
    .unwrap();
    let transport = guest::Stream::dial(link).unwrap();
    let client = Arc::new(configured(transport));
    let control = Arc::clone(&client);
    let (begun, began) = mpsc::sync_channel(1);
    let (finished, answer) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let deadline = Instant::now().checked_add(CAP).unwrap();
        let _ = begun.send(deadline);
        let result = control
            .run_before(
                absolute_spec(),
                Program::of(&["unused"]),
                Stdio {
                    ask: hex_api::Ask {
                        stdin: true,
                        stdout: true,
                        stderr: true,
                    },
                    ..Stdio::null()
                },
                deadline,
            )
            .map(drop);
        let _ = finished.send(result);
    });
    let original = began.recv_timeout(OBSERVE);
    let result = answer.recv_timeout(OBSERVE);
    if result.is_err() {
        client
            .conn
            .shared
            .end(Refusal::Broken("absolute named observation cleanup".into()));
        let _ = control_peer.shutdown(Shutdown::Both);
        for peer in &peers {
            let _ = peer.shutdown(Shutdown::Both);
        }
    }
    let joined = worker.join();
    // All blocking reads below have a finite observation guard, after the actor joined.
    let observed = (|| -> io::Result<Vec<Named>> {
        let mut names = Vec::new();
        for peer in &mut peers {
            peer.set_read_timeout(Some(OBSERVE))?;
            let mut name = [0; stream::NAME_BYTES];
            peer.read_exact(&mut name)?;
            names.push(
                stream::read_name(&name).map_err(|error| io::Error::other(format!("{error:?}")))?,
            );
            if peer.read(&mut [0])? != 0 {
                return Err(io::Error::other("unasked payload"));
            }
        }
        Ok(names)
    })();
    let request_seen = control_peer
        .set_read_timeout(Some(OBSERVE))
        .and_then(|()| next_request(&mut control_peer));
    let state_clean = {
        let state = client.conn.shared.state().unwrap();
        state.calls.is_empty()
            && state.programs.is_empty()
            && state.given.is_empty()
            && state.closed.is_none()
    };
    let recorded = deadlines.lock().unwrap().clone();
    let _ = control_peer.shutdown(Shutdown::Both);
    for peer in &peers {
        let _ = peer.shutdown(Shutdown::Both);
    }
    joined.unwrap();
    let original = original.unwrap();
    eprintln!(
        "client_absolute named_deadlines={recorded:?} caller={original:?} result={result:?} observed={observed:?} request={request_seen:?}"
    );
    assert!(matches!(result.unwrap(), Err(Refusal::Os { why, .. }) if why.contains("caller")));
    let names = observed.unwrap();
    let (request_header, request) = request_seen.unwrap();
    assert!(matches!(
        request,
        Request::Call(Call::Run {
            streams: Streams {
                stdin: true,
                stdout: true,
                stderr: true
            },
            ..
        })
    ));
    assert_eq!(recorded.len(), 4);
    assert_eq!(recorded[0], (stream::CONTROL_PORT, None));
    let setup = recorded[1].1.unwrap();
    assert!(
        setup <= original
            && recorded[1..]
                .iter()
                .all(|(port, at)| *port == stream::STDIO_PORT && *at == Some(setup))
    );
    assert_eq!(names.len(), 3);
    assert!(
        names
            .iter()
            .all(|name| name.exec.conn == 11 && name.exec.id == request_header.id)
    );
    assert_eq!(names[0].stream, stream::Stdio::In);
    assert_eq!(names[1].stream, stream::Stdio::Out);
    assert_eq!(names[2].stream, stream::Stdio::Err);
    assert!(state_clean);
}

/// The existing in-memory offer fixture supplies an actual Started response,
/// while no Exit is ever published. It has no relative transport reply cap.
struct DropOriginal(Offers);

impl Transport for DropOriginal {
    type Reading = ();

    fn answers() -> &'static [Patience; KINDS] {
        static WAITS: [Patience; KINDS] =
            [const { Patience::measuring("test.client_drop_original.unarmed") }; KINDS];
        &WAITS
    }
    fn reading(&self) {}
    fn handle(&self) -> Handle<'_> {
        self.0.handle()
    }
    fn recv(&self, reading: &mut (), take: &mut Take<'_>) -> Result<bool, Refusal> {
        self.0.recv(reading, take)
    }
    fn try_send(&self, bytes: &[u8], at: &mut usize, lent: &[Lent<'_>]) -> Result<bool, Sent> {
        let (header, request) = wire::read_request_apart(bytes, wire::MAX_MESSAGE).unwrap();
        if matches!(&request, Request::Call(Call::Run { .. })) {
            self.0.queue(header.id, Answer::Started { pid: 7 });
            self.0.outbound.lock().unwrap().push((header.id, request));
            *at = bytes.len();
            Ok(true)
        } else {
            self.0.try_send(bytes, at, lent)
        }
    }
    fn shutdown(&self) {
        self.0.shutdown();
    }
}

/// AC-T7.4.1 original regression: after run_before's absolute cap expires,
/// returning an observer error must not renew authority while dropping its actual
/// unfinished RemoteProc. Only the existing sender guard delays that Drop here.
#[test]
fn absolute_run_error_return_does_not_renew_drop_sender_budget() {
    let (io, _) = stalled(None);
    let (entered, _) = mpsc::sync_channel(1);
    let client = Arc::new(configured(DropOriginal(Offers {
        io,
        inbound: Mutex::new(VecDeque::new()),
        outbound: Mutex::new(Vec::new()),
        entered,
        program: Mutex::new(None),
    })));
    let original = Instant::now().checked_add(CAP).unwrap();
    let mut process = client
        .run_before(
            absolute_spec(),
            Program::of(&["owned-drop-original"]),
            Stdio::null(),
            original,
        )
        .unwrap();
    let run_id = {
        let sent = client.conn.shared.transport.0.outbound.lock().unwrap();
        assert_eq!(sent.len(), 1);
        let (id, request) = &sent[0];
        assert!(matches!(request, Request::Call(Call::Run { .. })));
        *id
    };
    assert_eq!(process.pid(), 7);
    assert_eq!(process.try_wait().unwrap(), None);
    assert_eq!(client.conn.shared.state().unwrap().programs.len(), 1);
    assert_eq!(client.conn.shared.transport.reply_budget(), None);

    let (release, begin) = mpsc::sync_channel(1);
    let (starting, started) = mpsc::sync_channel(1);
    let (finished, answer) = mpsc::sync_channel(1);
    // Spawn before holding the sender guard: even spawn failure drops the
    // captured Proc without a self-owned guard that could strand its destructor.
    let worker = std::thread::Builder::new()
        .name("owned-drop-original".into())
        .spawn(move || {
            let permitted = begin.recv();
            let began = Instant::now();
            let _ = starting.send((began, permitted.is_ok()));
            let fail_observer = move || -> Result<(), Refusal> {
                let _unfinished = process;
                Err(Refusal::os(
                    "owned observer original-deadline failure",
                    io::ErrorKind::TimedOut,
                ))
            };
            // This return must run RemoteProc::drop before the result can be sent.
            let result = fail_observer();
            let _ = finished.send((Instant::now(), result));
        })
        .unwrap();

    // From this point every observation is captured; cleanup/join precedes ALL
    // assertions. No production hook, fake clock, new timer or periodic wait.
    let held = client.conn.shared.sending.lock(None);
    let held_exact_guard = matches!(&held, Ok(Some(_)));
    let expired = until(original);
    let released = release.send(());
    let began = started.recv_timeout(OBSERVE);
    let before_cleanup = answer.recv_timeout(OBSERVE);
    let before_release_at = Instant::now();
    drop(held);
    client.conn.shared.end(Refusal::Broken(
        "owned Drop original observation cleanup".into(),
    ));
    let joined = worker.join();
    let after_cleanup = answer.try_recv();
    let (release_count, other_count) = {
        let sent = client.conn.shared.transport.0.outbound.lock().unwrap();
        let releases = sent
            .iter()
            .filter(|(_, request)| matches!(request, Request::Release))
            .count();
        let other = sent
            .iter()
            .filter(|(_, request)| {
                !matches!(request, Request::Call(Call::Run { .. }) | Request::Release)
            })
            .count();
        (releases, other)
    };
    let clean =
        client.conn.shared.state().map(|state| {
            state.calls.is_empty() && state.programs.is_empty() && state.given.is_empty()
        });
    eprintln!(
        "client_drop_original run_id={run_id} original={original:?} cap={CAP:?} observation={OBSERVE:?} held={held_exact_guard} expired={expired:?} gate={released:?} began={began:?} before_release={before_release_at:?} before_cleanup={before_cleanup:?} after_cleanup={after_cleanup:?} release_count={release_count} other_count={other_count} joined={} clean={clean:?}",
        joined.is_ok()
    );
    joined.unwrap();
    expired.unwrap();
    released.unwrap();
    assert!(held_exact_guard, "the exact existing sender must be held");
    let (began, permitted) = began.unwrap();
    assert!(
        permitted && began >= original,
        "Drop must begin after the original cap"
    );
    assert_eq!(other_count, 0);
    assert!(clean.unwrap());
    assert!(
        client
            .conn
            .shared
            .transport
            .0
            .io
            .closed
            .load(Ordering::Acquire)
    );
    assert!(
        before_cleanup.is_ok(),
        "original absolute deadline was lost: the observer's final error return waited in unfinished Proc::drop until its held sender was released"
    );
    let (returned, result) = before_cleanup.unwrap();
    assert!(returned <= before_release_at);
    assert!(matches!(
        result,
        Err(Refusal::Os {
            op: "owned observer original-deadline failure",
            ..
        })
    ));
}

struct BootstrapProbe {
    io: Offers,
    reply: Mutex<Option<Answer>>,
}

impl Transport for BootstrapProbe {
    type Reading = ();

    fn answers() -> &'static [Patience; KINDS] {
        static WAITS: [Patience; KINDS] =
            [const { Patience::measuring("test.client_bootstrap.unarmed") }; KINDS];
        &WAITS
    }

    fn reading(&self) {}
    fn handle(&self) -> Handle<'_> {
        self.io.handle()
    }
    fn recv(&self, reading: &mut (), take: &mut Take<'_>) -> Result<bool, Refusal> {
        self.io.recv(reading, take)
    }
    fn try_send(&self, bytes: &[u8], at: &mut usize, lent: &[Lent<'_>]) -> Result<bool, Sent> {
        let (header, request) = wire::read_request_apart(bytes, wire::MAX_MESSAGE).unwrap();
        self.io.outbound.lock().unwrap().push((header.id, request));
        if let Some(answer) = self.reply.lock().unwrap().take() {
            self.io.queue(header.id, answer);
        }
        self.io.io.try_send(bytes, at, lent)
    }
    fn shutdown(&self) {
        self.io.shutdown();
    }
}

fn bootstrap_probe(reply: Option<Answer>) -> (Core<BootstrapProbe>, mpsc::Receiver<()>) {
    let (io, sent) = stalled(None);
    let (entered, _) = mpsc::sync_channel(1);
    (
        configured(BootstrapProbe {
            io: Offers {
                io,
                inbound: Mutex::new(VecDeque::new()),
                outbound: Mutex::new(Vec::new()),
                entered,
                program: Mutex::new(None),
            },
            reply: Mutex::new(reply),
        }),
        sent,
    )
}

#[derive(Clone, Copy, Debug)]
enum BootstrapCall {
    Capacity,
    PhysicalReport,
    Create,
}

impl BootstrapCall {
    fn invoke(self, client: &Core<BootstrapProbe>, at: Instant) -> Result<Answer, Refusal> {
        match self {
            Self::Capacity => client.capacity_before(Class::Oci, at).map(Answer::Capacity),
            Self::PhysicalReport => client
                .physical_report_before(at)
                .map(Answer::PhysicalReport),
            Self::Create => client.create_before(absolute_spec(), at).map(Answer::Info),
        }
    }

    fn reply(self) -> Answer {
        match self {
            Self::Capacity => Answer::Capacity(None),
            Self::PhysicalReport => Answer::PhysicalReport(hex_api::PhysicalReport {
                unavailable: Vec::new(),
                authority: "owned-bootstrap-stand-in".into(),
                devices: Vec::new(),
            }),
            Self::Create => {
                let spec = absolute_spec();
                Answer::Info(BoxInfo {
                    id: spec.id,
                    class: Class::Process,
                    state: hex_api::BoxState::Empty,
                    bounds: spec.bounds,
                    allocation: None,
                    adopted: false,
                    membership: None,
                    exit: None,
                    abandoned: 0,
                    route: None,
                })
            }
        }
    }

    fn matches_request(self, request: &Request) -> bool {
        match (self, request) {
            (Self::Capacity, Request::Call(Call::Capacity(Class::Oci)))
            | (Self::PhysicalReport, Request::Call(Call::PhysicalReport)) => true,
            (Self::Create, Request::Call(Call::Create(spec))) => **spec == absolute_spec(),
            _ => false,
        }
    }
}

/// AC-T7.4.1: all three bootstrap wrappers retain typed request/answer/refusal
/// behavior and reject an already expired original deadline before any send.
#[test]
fn absolute_bootstrap_preserves_requests_and_typed_answers() {
    for kind in [
        BootstrapCall::Capacity,
        BootstrapCall::PhysicalReport,
        BootstrapCall::Create,
    ] {
        let (client, sent) = bootstrap_probe(None);
        let expired = kind.invoke(&client, Instant::now());
        assert!(matches!(expired, Err(Refusal::Os { .. })));
        assert!(matches!(sent.try_recv(), Err(mpsc::TryRecvError::Empty)));
        assert!(
            client
                .conn
                .shared
                .transport
                .io
                .outbound
                .lock()
                .unwrap()
                .is_empty()
        );

        let reply = kind.reply();
        let expected = wire::answer(0, &reply, wire::MAX_MESSAGE).unwrap();
        let (client, _) = bootstrap_probe(Some(reply));
        let at = Instant::now().checked_add(OBSERVE).unwrap();
        let got = kind.invoke(&client, at).unwrap();
        assert_eq!(wire::answer(0, &got, wire::MAX_MESSAGE).unwrap(), expected);
        let outbound = client.conn.shared.transport.io.outbound.lock().unwrap();
        assert_eq!(outbound.len(), 1);
        assert!(kind.matches_request(&outbound[0].1));
        assert!(client.conn.shared.state().unwrap().programs.is_empty());
        drop(outbound);

        let (client, _) = bootstrap_probe(Some(Answer::Done));
        let at = Instant::now().checked_add(OBSERVE).unwrap();
        assert!(matches!(kind.invoke(&client, at), Err(Refusal::Broken(_))));

        let why = Refusal::Broken("owned typed bootstrap refusal".into());
        let (client, _) = bootstrap_probe(Some(Answer::Refused(why.clone())));
        let at = Instant::now().checked_add(OBSERVE).unwrap();
        assert_eq!(kind.invoke(&client, at), Err(why));
    }
}

/// AC-T7.4.1: even an unarmed answer kind and absent relative transport budget
/// cannot extend the original bootstrap authority. Cleanup/join precedes assertions.
#[test]
fn absolute_bootstrap_caps_unarmed_answer_waits() {
    for kind in [
        BootstrapCall::Capacity,
        BootstrapCall::PhysicalReport,
        BootstrapCall::Create,
    ] {
        let (client, sent) = bootstrap_probe(None);
        let client = Arc::new(client);
        let control = Arc::clone(&client);
        let at = Instant::now().checked_add(CAP).unwrap();
        let (finished, answer) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let result = kind.invoke(&control, at);
            let _ = finished.send(result);
        });
        let observed_send = sent.recv_timeout(OBSERVE);
        let observed = answer.recv_timeout(OBSERVE);
        client.conn.shared.end(Refusal::Broken(
            "owned bootstrap observation cleanup".into(),
        ));
        let joined = worker.join();
        let outbound = client.conn.shared.transport.io.outbound.lock().unwrap();
        let clean = client.conn.shared.state().map(|state| {
            state.calls.is_empty() && state.programs.is_empty() && state.given.is_empty()
        });
        eprintln!(
            "client_bootstrap_absolute kind={kind:?} at={at:?} send={observed_send:?} observed={observed:?} joined={} clean={clean:?}",
            joined.is_ok()
        );
        joined.unwrap();
        observed_send.unwrap();
        assert_eq!(outbound.len(), 1);
        assert!(kind.matches_request(&outbound[0].1));
        assert!(clean.unwrap());
        assert!(matches!(
            observed.unwrap(),
            Err(Refusal::Os { why, .. }) if why.contains("absolute") && !why.contains("twice")
        ));
    }
}

struct SettleOriginal {
    io: Offers,
    setup: Mutex<Option<(u64, hex_api::Ask, ReplyDeadline)>>,
    started: mpsc::SyncSender<(u64, Instant)>,
    kill: mpsc::SyncSender<(u64, u64, Instant)>,
    exits: AtomicUsize,
}

impl Transport for SettleOriginal {
    type Reading = ();
    const MAKES_ASKED: bool = true;

    fn answers() -> &'static [Patience; KINDS] {
        static WAITS: [Patience; KINDS] =
            [const { Patience::measuring("test.client_settle_original.unarmed") }; KINDS];
        &WAITS
    }

    fn reading(&self) {}
    fn handle(&self) -> Handle<'_> {
        self.io.handle()
    }

    fn recv(&self, reading: &mut (), take: &mut Take<'_>) -> Result<bool, Refusal> {
        let mut observed = |message: &Received<'_>, fds| {
            let (header, answer) =
                wire::read_answer(message, wire::MAX_MESSAGE).map_err(Refusal::from)?;
            take(message, fds)?;
            match answer {
                Answer::Started { .. } => {
                    eprintln!("client_settle_phase phase=started id={}", header.id);
                    let _ = self.started.try_send((header.id, Instant::now()));
                }
                Answer::Exited(_) => {
                    self.exits.fetch_add(1, Ordering::SeqCst);
                }
                _ => {}
            }
            Ok(())
        };
        let got = self.io.recv(reading, &mut observed)?;
        // The wake is only a datagram, not persistent EOF. Drain actual queued
        // answers first, then preserve the same terminal condition as Stalled.
        if self.io.io.closed.load(Ordering::Acquire) {
            eprintln!("client_settle_phase phase=transport_closed");
            Err(Refusal::Broken("owned settle transport closed".into()))
        } else {
            Ok(got)
        }
    }

    fn try_send(&self, bytes: &[u8], at: &mut usize, _: &[Lent<'_>]) -> Result<bool, Sent> {
        let (header, request) = wire::read_request_apart(bytes, wire::MAX_MESSAGE).unwrap();
        eprintln!(
            "client_settle_phase phase=send kind={:#06x} id={}",
            header.kind, header.id
        );
        match &request {
            Request::Call(Call::Run { .. }) => self.io.queue(header.id, Answer::Started { pid: 7 }),
            Request::Call(Call::Kill { exec }) => {
                eprintln!("client_settle_phase phase=kill_request exec={exec}");
                // The real Kill reached the sender after wait(None). Withhold only its answer.
                let _ = self.kill.try_send((header.id, *exec, Instant::now()));
            }
            Request::Release => {}
            _ => {
                return Err(Sent::Refused(Refusal::Broken(
                    "unexpected owned settle-original request".into(),
                )));
            }
        }
        self.io.outbound.lock().unwrap().push((header.id, request));
        *at = bytes.len();
        Ok(true)
    }

    fn asks_before(
        &self,
        exec: u64,
        ask: hex_api::Ask,
        deadline: ReplyDeadline,
    ) -> Result<hex_api::Given, Refusal> {
        deadline.remaining("owned malformed stream setup")?;
        *self.setup.lock().unwrap() = Some((exec, ask, deadline));
        // Deliberately return the wrong end: the caller asks for stdin only. This
        // bounded adapter fault is refused by real settle AFTER actual Started.
        let mut program = Stdio {
            ask: hex_api::Ask {
                stdout: true,
                ..hex_api::Ask::default()
            },
            ..Stdio::null()
        };
        let given = program.make_asked()?;
        *self.io.program.lock().unwrap() = Some(program);
        eprintln!(
            "client_settle_phase phase=setup_ends_returned exec={exec} deadline={:?}",
            deadline.at()
        );
        Ok(given)
    }

    fn carries_before(&self, _: u64, stdio: Stdio, deadline: ReplyDeadline) -> Result<(), Refusal> {
        deadline.remaining("owned null carried streams")?;
        stdio.check()?;
        if carried(&stdio) != Streams::default() {
            return Err(Refusal::Broken("unexpected owned carried pipe".into()));
        }
        Ok(())
    }

    fn shutdown(&self) {
        self.io.shutdown();
    }
}

/// AC-T7.4.1 original: an actual Started followed by a bounded stream-settlement
/// refusal must not replace run_before's authority with a fresh Kill answer wait.
#[test]
fn absolute_run_settle_error_does_not_renew_kill_answer_budget() {
    let (io, _) = stalled(None);
    let (entered, _) = mpsc::sync_channel(1);
    let (starting, started) = mpsc::sync_channel(1);
    let (killing, killed) = mpsc::sync_channel(1);
    let client = Arc::new(configured(SettleOriginal {
        io: Offers {
            io,
            inbound: Mutex::new(VecDeque::new()),
            outbound: Mutex::new(Vec::new()),
            entered,
            program: Mutex::new(None),
        },
        setup: Mutex::new(None),
        started: starting,
        kill: killing,
        exits: AtomicUsize::new(0),
    }));
    let control = Arc::clone(&client);
    let original = Instant::now().checked_add(CAP).unwrap();
    let (finished, answer) = mpsc::sync_channel(1);
    let worker = std::thread::Builder::new()
        .name("owned-settle-original".into())
        .spawn(move || {
            eprintln!("client_settle_phase phase=run_begin original={original:?}");
            let result = control
                .run_before(
                    absolute_spec(),
                    Program::of(&["owned-settle-original"]),
                    Stdio {
                        ask: hex_api::Ask {
                            stdin: true,
                            ..hex_api::Ask::default()
                        },
                        ..Stdio::null()
                    },
                    original,
                )
                .map(drop);
            eprintln!("client_settle_phase phase=worker_return result={result:?}");
            let _ = finished.send((Instant::now(), result));
        })
        .unwrap();

    // Capture every error branch; end ONLY this connection, close its retained
    // pipe, and join this worker BEFORE asserting any result or phase witness.
    let actual_started = started.recv_timeout(OBSERVE);
    let actual_kill = killed.recv_timeout(OBSERVE);
    let expired = until(original);
    let before_cleanup = answer.recv_timeout(OBSERVE);
    let cleanup_at = Instant::now();
    eprintln!(
        "client_settle_phase phase=cleanup_begin started={actual_started:?} kill={actual_kill:?} before={before_cleanup:?}"
    );
    client
        .conn
        .shared
        .end(Refusal::Broken("owned settle-original cleanup".into()));
    eprintln!("client_settle_phase phase=end_returned");
    let (pipe, pipe_poisoned) = match client.conn.shared.transport.io.program.lock() {
        Ok(mut program) => (program.take(), false),
        // Cleanup may recover only this test-owned guard so a poisoned fixture
        // cannot skip its pipe close/join. The poison remains a failed witness.
        Err(poisoned) => (poisoned.into_inner().take(), true),
    };
    let pipe_was_owned = pipe.is_some();
    drop(pipe);
    eprintln!("client_settle_phase phase=pipe_closed join=begin");
    let joined = worker.join();
    eprintln!("client_settle_phase phase=joined ok={}", joined.is_ok());
    let after_cleanup = answer.try_recv();
    let transport = &client.conn.shared.transport;
    let setup = *transport.setup.lock().unwrap();
    let exits = transport.exits.load(Ordering::SeqCst);
    let sent = transport.io.outbound.lock().unwrap();
    let run_ids: Vec<_> = sent
        .iter()
        .filter_map(|(id, request)| matches!(request, Request::Call(Call::Run { .. })).then(|| *id))
        .collect();
    let kills: Vec<_> = sent
        .iter()
        .filter_map(|(id, request)| match request {
            Request::Call(Call::Kill { exec }) => Some((*id, *exec)),
            _ => None,
        })
        .collect();
    let releases = sent
        .iter()
        .filter(|(_, request)| matches!(request, Request::Release))
        .count();
    let clean = client.conn.shared.state().map(|state| {
        state.calls.is_empty()
            && state.programs.is_empty()
            && state.given.is_empty()
            && state.waiting.is_empty()
    });
    eprintln!(
        "client_settle_original original={original:?} cap={CAP:?} observation={OBSERVE:?} setup={setup:?} started={actual_started:?} kill={actual_kill:?} runs={run_ids:?} kills={kills:?} exits={exits} releases={releases} before_cleanup={before_cleanup:?} cleanup_at={cleanup_at:?} after_cleanup={after_cleanup:?} pipe_owned={pipe_was_owned} pipe_poisoned={pipe_poisoned} joined={} clean={clean:?}",
        joined.is_ok()
    );
    joined.unwrap();
    expired.unwrap();
    assert!(pipe_was_owned);
    assert!(!pipe_poisoned);
    assert!(transport.io.io.closed.load(Ordering::Acquire));
    assert!(transport.io.program.lock().unwrap().is_none());
    assert!(clean.unwrap());
    assert_eq!(transport.reply_budget(), None);
    assert_eq!(run_ids.len(), 1);
    assert_eq!(exits, 0);
    let (exec, ask, cap) = setup.unwrap();
    assert_eq!(exec, run_ids[0]);
    assert!(ask.stdin && !ask.stdout && !ask.stderr);
    assert_eq!(cap.at(), original);
    assert_eq!(actual_started.unwrap().0, exec);
    if before_cleanup.is_err() {
        // In the original, this positive witness localizes the delay AFTER the
        // zero-duration wait(None) poll: it reached the real unarmed Kill call.
        let (request, target, _) = actual_kill.expect("missing Kill is an unverified phase");
        assert_eq!(target, exec);
        assert_eq!(kills, [(request, exec)]);
    }
    let returned_error = before_cleanup
        .as_ref()
        .ok()
        .or_else(|| after_cleanup.as_ref().ok());
    assert!(
        matches!(returned_error, Some((_, Err(Refusal::Broken(why)))) if why.contains("stdout") && why.contains("neither carried nor asked")),
        "the original bounded setup refusal must survive cleanup"
    );
    assert!(
        before_cleanup.is_ok(),
        "original stream-settlement error return exceeded run_before authority while waiting for the ordinary Kill answer"
    );
    let (returned, _) = before_cleanup.unwrap();
    assert!(returned <= cleanup_at);
}

/// AC-T7.4.1 control A: actual selection chooses the existing private smaller
/// learned policy, consumed at the existing setup seam before any send is attempted.
#[test]
fn selected_smaller_learned_setup_expiry_prevents_send() {
    let (transport, writes) = stalled(None);
    let client = Arc::new(configured(Calibrated::<false>(transport)));
    let control = Arc::clone(&client);
    let hello = Request::Hello {
        build: wire::build(),
    };
    let (selected, selection) = mpsc::sync_channel(1);
    let (finished, answer) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let original = Instant::now().checked_add(CAP).unwrap();
        let cap = ReplyDeadline::caller_before(original).unwrap();
        let result = control
            .conn
            .call_before(
                wire::MAX_MESSAGE,
                &hello,
                |_, deadline| {
                    let deadline = deadline.ok_or_else(|| {
                        Refusal::Broken("selected setup deadline was absent".into())
                    })?;
                    selected
                        .try_send((original, deadline))
                        .map_err(|why| Refusal::os("observing selected setup authority", why))?;
                    // Deliberately consume exactly the selected authority, without
                    // replacing it or changing a seed. Caller::send must now emit nothing.
                    until(deadline.at())?;
                    Ok(NONE)
                },
                None,
                Some(cap),
            )
            .map(drop);
        let _ = finished.send((Instant::now(), result));
    });
    let selected = selection.recv_timeout(OBSERVE);
    let observed = answer.recv_timeout(OBSERVE);
    client.conn.shared.end(Refusal::Broken(
        "owned selected-deadline observation cleanup".into(),
    ));
    let joined = worker.join();
    let writes = writes.try_recv();
    let clean =
        client.conn.shared.state().map(|state| {
            state.calls.is_empty() && state.programs.is_empty() && state.given.is_empty()
        });
    eprintln!(
        "client_hello_phase_a selected={selected:?} observed={observed:?} writes={writes:?} joined={} clean={clean:?}",
        joined.is_ok()
    );
    joined.unwrap();
    let (original, selected) = selected.unwrap();
    assert!(matches!(selected.source, ReplyBound::Learned(_)));
    assert!(selected.at() < original);
    let (returned, result) = observed.unwrap();
    assert!(returned >= selected.at());
    assert!(matches!(
        result,
        Err(Refusal::Os { op: "sending to the node agent", why }) if why.contains("twice")
    ));
    // This Stalled transport commits its whole nonempty input on its first try_send;
    // no notification therefore independently witnesses zero send calls/bytes.
    assert!(matches!(writes, Err(mpsc::TryRecvError::Empty)));
    assert!(clean.unwrap());
}

struct UnarmedOrdinaryHello {
    stream: guest::Stream,
    writes: Arc<Mutex<Vec<(usize, Instant)>>>,
}

impl Transport for UnarmedOrdinaryHello {
    type Reading = guest::Reading;
    const MAKES_ASKED: bool = <guest::Stream as Transport>::MAKES_ASKED;

    fn answers() -> &'static [Patience; KINDS] {
        static WAITS: [Patience; KINDS] =
            [const { Patience::measuring("test.ordinary_hello_phase.unarmed") }; KINDS];
        &WAITS
    }
    fn reply_budget(&self) -> Option<Duration> {
        self.stream.reply_budget()
    }
    fn reading(&self) -> Self::Reading {
        self.stream.reading()
    }
    fn handle(&self) -> Handle<'_> {
        self.stream.handle()
    }
    fn frame(message: Vec<u8>) -> Result<Vec<u8>, Refusal> {
        <guest::Stream as Transport>::frame(message)
    }
    fn recv(&self, reading: &mut Self::Reading, take: &mut Take<'_>) -> Result<bool, Refusal> {
        self.stream.recv(reading, take)
    }
    fn try_send(&self, bytes: &[u8], at: &mut usize, lent: &[Lent<'_>]) -> Result<bool, Sent> {
        let before = *at;
        let result = self.stream.try_send(bytes, at, lent);
        if *at > before {
            self.writes
                .lock()
                .unwrap()
                .push((*at - before, Instant::now()));
        }
        result
    }
    fn shutdown(&self) {
        self.stream.shutdown();
    }
}

/// One actual TCP frame, using the existing readiness helper and one original
/// deadline for every wait. No per-read timeout reset can extend this observation.
fn ordinary_hello_frame(
    peer: &mut TcpStream,
    deadline: ReplyDeadline,
) -> Result<(Header, Request, Vec<u8>), Refusal> {
    use hex_core::stream::Deframer;

    peer.set_nonblocking(true)
        .map_err(|why| Refusal::os("observing ordinary Hello without waiting", why))?;
    let mut decoder = Deframer::new(wire::MAX_MESSAGE);
    let mut buffer = vec![0; hex_core::stream::MAX_FRAME];
    let mut captured = Vec::new();
    loop {
        // Already available data is evidence even if observation is scheduled late;
        // only a new readiness wait consumes the remaining original authority.
        let n = match peer.read(&mut buffer) {
            Ok(0) => {
                return Err(Refusal::Broken(
                    "ordinary Hello peer ended mid-frame".into(),
                ));
            }
            Ok(n) => n,
            Err(why) if why.kind() == io::ErrorKind::Interrupted => {
                deadline.remaining("observing ordinary Hello")?;
                continue;
            }
            Err(why) if why.kind() == io::ErrorKind::WouldBlock => {
                protocol_ready(
                    Duplex::handle(peer),
                    false,
                    deadline,
                    "observing ordinary Hello",
                )?;
                continue;
            }
            Err(why) => return Err(Refusal::os("reading ordinary Hello", why)),
        };
        if captured.len() + n > hex_core::stream::MAX_FRAME {
            return Err(Refusal::Broken("ordinary Hello exceeded one frame".into()));
        }
        captured.extend_from_slice(&buffer[..n]);
        let mut input = &buffer[..n];
        while !input.is_empty() {
            let (took, message) = decoder.take(input).map_err(Refusal::from)?;
            if took == 0 || took > input.len() {
                return Err(Refusal::Broken(
                    "ordinary Hello parser made no progress".into(),
                ));
            }
            input = &input[took..];
            if let Some(message) = message {
                if !input.is_empty() || !decoder.at_boundary() {
                    return Err(Refusal::Broken("bytes after ordinary Hello frame".into()));
                }
                let (header, request) = wire::read_request_apart(&message, wire::MAX_MESSAGE)
                    .map_err(|bad| Refusal::from(bad.fault))?;
                return Ok((header, request, captured));
            }
        }
    }
}

/// AC-T7.4.1 control B: the SAME ordinary Stream/Core Hello path under a private
/// unarmed test census sends a full actual frame, gets no answer, and exhausts the
/// caller's original cap. This is NOT the public calibrated constructor fixture.
#[test]
fn isolated_ordinary_stream_hello_withheld_answer_uses_original_cap() {
    let (socket, mut peer) = absolute_pair();
    peer.write_all(&11u64.to_le_bytes()).unwrap();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let link = guest::own_link(AbsoluteNamedLink {
        control: Mutex::new(Some(socket)),
        stdio: Mutex::new(VecDeque::new()),
        deadlines: Arc::clone(&recorded),
    })
    .unwrap();
    // The existing test-only dial consumes a complete prequeued token before the
    // isolated Hello operation. It makes no claim about the public factory's dial phase.
    let stream = guest::Stream::dial(link).unwrap();
    let writes = Arc::new(Mutex::new(Vec::new()));
    let shared = Shared::try_new(UnarmedOrdinaryHello {
        stream,
        writes: Arc::clone(&writes),
    })
    .unwrap();
    let control = triomphe::Arc::clone(&shared);
    let (starting, started) = mpsc::sync_channel(1);
    let (finished, answer) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let original = Instant::now().checked_add(CAP).unwrap();
        let deadline = ReplyDeadline::caller_before(original).unwrap();
        let _ = starting.try_send(deadline);
        let result = Core::hello_before(control, Some(deadline)).map(drop);
        let _ = finished.send((Instant::now(), result));
    });
    let deadline = started.recv_timeout(OBSERVE);
    let frame = match &deadline {
        Ok(deadline) => ordinary_hello_frame(&mut peer, *deadline),
        Err(why) => Err(Refusal::os("observing isolated Hello authority", why)),
    };
    // The peer has sent the token only: no Hello response is queued or synthesized.
    let observed = answer.recv_timeout(OBSERVE);
    let stopped = peer.shutdown(Shutdown::Both);
    drop(peer);
    shared.end(Refusal::Broken("owned ordinary-Hello cleanup".into()));
    let joined = worker.join();
    let committed = writes.lock().unwrap();
    let clean = shared
        .state()
        .map(|state| state.calls.is_empty() && state.programs.is_empty() && state.given.is_empty());
    eprintln!(
        "client_hello_phase_b scope=isolated_ordinary_seam public_constructor_proof=false deadline={deadline:?} frame={frame:?} writes={committed:?} observed={observed:?} stopped={stopped:?} joined={} clean={clean:?}",
        joined.is_ok()
    );
    joined.unwrap();
    let deadline = deadline.unwrap();
    let (header, request, bytes) = frame.expect("full actual Hello is a required phase witness");
    assert_eq!((header.kind, header.id), (1, 1));
    assert!(matches!(request, Request::Hello { build } if build == wire::build()));
    assert_eq!(
        committed.iter().map(|(n, _)| *n).sum::<usize>(),
        bytes.len()
    );
    assert!(!committed.is_empty());
    let (returned, result) = observed.unwrap();
    assert!(returned >= deadline.at());
    assert!(matches!(
        result,
        Err(Refusal::Os { op: "waiting for the node agent's answer", why })
            if why.contains("absolute") && !why.contains("twice")
    ));
    assert_eq!(shared.transport.reply_budget(), None);
    assert!(clean.unwrap());
}

/// Separate successor transport: the same real memory protocol, with all phase diagnostics
/// deferred until cleanup. The original SettleOriginal and its selector above stay untouched.
struct SettleAligned {
    io: Offers,
    prepared: Mutex<Option<hex_api::Given>>,
    setup: Mutex<Option<(u64, hex_api::Ask, ReplyDeadline)>>,
    started: mpsc::SyncSender<(u64, Instant)>,
    kill: mpsc::SyncSender<(u64, u64, Instant)>,
    exits: AtomicUsize,
    other_answers: AtomicUsize,
}

impl Transport for SettleAligned {
    type Reading = ();
    const MAKES_ASKED: bool = true;

    fn answers() -> &'static [Patience; KINDS] {
        static WAITS: [Patience; KINDS] =
            [const { Patience::measuring("test.client_settle_aligned.unarmed") }; KINDS];
        &WAITS
    }

    fn reading(&self) {}
    fn handle(&self) -> Handle<'_> {
        self.io.handle()
    }

    fn recv(&self, reading: &mut (), take: &mut Take<'_>) -> Result<bool, Refusal> {
        let mut observed = |message: &Received<'_>, fds| {
            let (header, answer) =
                wire::read_answer(message, wire::MAX_MESSAGE).map_err(Refusal::from)?;
            take(message, fds)?;
            match answer {
                Answer::Started { .. } => {
                    let _ = self.started.try_send((header.id, Instant::now()));
                }
                Answer::Exited(_) => {
                    self.exits.fetch_add(1, Ordering::SeqCst);
                    self.other_answers.fetch_add(1, Ordering::SeqCst);
                }
                _ => {
                    self.other_answers.fetch_add(1, Ordering::SeqCst);
                }
            }
            Ok(())
        };
        let got = self.io.recv(reading, &mut observed)?;
        if self.io.io.closed.load(Ordering::Acquire) {
            Err(Refusal::Broken(
                "owned aligned settle transport closed".into(),
            ))
        } else {
            Ok(got)
        }
    }

    fn try_send(&self, bytes: &[u8], at: &mut usize, _: &[Lent<'_>]) -> Result<bool, Sent> {
        let (header, request) = wire::read_request_apart(bytes, wire::MAX_MESSAGE)
            .map_err(|bad| Sent::Refused(Refusal::from(bad.fault)))?;
        let killed = match &request {
            Request::Call(Call::Run { .. }) => {
                self.io.queue(header.id, Answer::Started { pid: 7 });
                None
            }
            // Record the actual committed Kill, but never manufacture its answer or an Exit.
            Request::Call(Call::Kill { exec }) => Some(*exec),
            Request::Release => None,
            _ => {
                return Err(Sent::Refused(Refusal::Broken(
                    "unexpected owned aligned-settle request".into(),
                )));
            }
        };
        self.io
            .outbound
            .lock()
            .map_err(|why| Sent::Refused(poisoned(why)))?
            .push((header.id, request));
        *at = bytes.len();
        if let Some(exec) = killed {
            let _ = self.kill.try_send((header.id, exec, Instant::now()));
        }
        Ok(true)
    }

    fn asks_before(
        &self,
        exec: u64,
        ask: hex_api::Ask,
        deadline: ReplyDeadline,
    ) -> Result<hex_api::Given, Refusal> {
        deadline.remaining("owned prepared malformed stream setup")?;
        *self.setup.lock().map_err(poisoned)? = Some((exec, ask, deadline));
        self.prepared
            .lock()
            .map_err(poisoned)?
            .take()
            .ok_or_else(|| Refusal::Broken("prepared wrong end was already taken".into()))
    }

    fn carries_before(&self, _: u64, stdio: Stdio, deadline: ReplyDeadline) -> Result<(), Refusal> {
        deadline.remaining("owned null carried streams")?;
        stdio.check()?;
        if carried(&stdio) != Streams::default() {
            return Err(Refusal::Broken("unexpected owned carried pipe".into()));
        }
        Ok(())
    }

    fn shutdown(&self) {
        self.io.shutdown();
    }
}

/// AC-T7.4.1 separately pinned successor: start the same cap in an already prepared actor.
/// ROOT must compare this SAME selector before/after69a and under the missing-Kill-cap mutation.
#[test]
fn prepared_actor_settle_error_retains_original_kill_answer_authority() {
    let (io, _) = stalled(None);
    let (entered, _) = mpsc::sync_channel(1);
    let (starting, started) = mpsc::sync_channel(1);
    let (killing, killed) = mpsc::sync_channel(1);
    let (prepared, readiness) = mpsc::sync_channel(1);
    let (release, begin) = mpsc::sync_channel(1);
    let (arming, armed) = mpsc::sync_channel(1);
    let (finished, answer) = mpsc::sync_channel(1);
    let observer = Wake::new().unwrap();
    let mut inbound = VecDeque::new();
    inbound.try_reserve(1).unwrap();
    let mut outbound = Vec::new();
    outbound.try_reserve_exact(2).unwrap();
    let mut program_ends = Stdio {
        ask: hex_api::Ask {
            stdout: true,
            ..hex_api::Ask::default()
        },
        ..Stdio::null()
    };
    let wrong_end = program_ends.make_asked().unwrap();
    let client = Arc::new(configured(SettleAligned {
        io: Offers {
            io,
            inbound: Mutex::new(inbound),
            outbound: Mutex::new(outbound),
            entered,
            program: Mutex::new(Some(program_ends)),
        },
        prepared: Mutex::new(Some(wrong_end)),
        setup: Mutex::new(None),
        started: starting,
        kill: killing,
        exits: AtomicUsize::new(0),
        other_answers: AtomicUsize::new(0),
    }));
    let control = Arc::clone(&client);
    let spec = absolute_spec();
    let program = Program::of(&["owned-prepared-settle"]);
    let stdio = Stdio {
        ask: hex_api::Ask {
            stdin: true,
            ..hex_api::Ask::default()
        },
        ..Stdio::null()
    };
    let worker = std::thread::Builder::new()
        .name("owned-prepared-settle".into())
        .spawn(move || {
            if prepared.try_send(()).is_err() || begin.recv().is_err() {
                return;
            }
            let Some(original) = Instant::now().checked_add(CAP) else {
                return;
            };
            // Nonblocking notices use preallocated channels; no diagnostics or argument
            // construction separates this one anchor from the actual Core operation.
            if arming.try_send(original).is_err() {
                return;
            }
            let result = control.run_before(spec, program, stdio, original).map(drop);
            let returned = Instant::now();
            let _ = finished.try_send((returned, result));
        })
        .unwrap();

    let actor_ready = readiness.recv_timeout(OBSERVE);
    let released = actor_ready.is_ok().then(|| release.try_send(()));
    // Every early branch closes this owned barrier so joining can never wait for a lost start.
    drop(release);
    let original = armed.recv_timeout(OBSERVE);
    let actual_started = started.recv_timeout(OBSERVE);
    let actual_kill = killed.recv_timeout(OBSERVE);
    let expired = original.as_ref().ok().map(|at| {
        loop {
            let left = at.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break Ok(());
            }
            // Same existing readiness wait as until(), with its wake prepared before the cap.
            if let Err(why) = ready(observer.handle(), false, None, Some(left)) {
                break Err(why);
            }
        }
    });
    let before_cleanup = answer.recv_timeout(OBSERVE);
    let cleanup_at = Instant::now();
    client
        .conn
        .shared
        .end(Refusal::Broken("owned prepared-settle cleanup".into()));
    let transport = &client.conn.shared.transport;
    let (pipe, pipe_poisoned) = match transport.io.program.lock() {
        Ok(mut program) => (program.take(), false),
        Err(poisoned) => (poisoned.into_inner().take(), true),
    };
    let (unused, end_poisoned) = match transport.prepared.lock() {
        Ok(mut given) => (given.take(), false),
        Err(poisoned) => (poisoned.into_inner().take(), true),
    };
    let pipe_was_owned = pipe.is_some();
    let unused_end = unused.is_some();
    drop((pipe, unused));
    let joined = worker.join();
    let after_cleanup = answer.try_recv();
    let setup = *transport.setup.lock().unwrap();
    let exits = transport.exits.load(Ordering::SeqCst);
    let other_answers = transport.other_answers.load(Ordering::SeqCst);
    let sent = transport.io.outbound.lock().unwrap();
    let runs: Vec<_> = sent
        .iter()
        .filter_map(|(id, request)| matches!(request, Request::Call(Call::Run { .. })).then(|| *id))
        .collect();
    let kills: Vec<_> = sent
        .iter()
        .filter_map(|(id, request)| match request {
            Request::Call(Call::Kill { exec }) => Some((*id, *exec)),
            _ => None,
        })
        .collect();
    let releases = sent
        .iter()
        .filter(|(_, request)| matches!(request, Request::Release))
        .count();
    let clean = client.conn.shared.state().map(|state| {
        state.calls.is_empty()
            && state.programs.is_empty()
            && state.given.is_empty()
            && state.waiting.is_empty()
    });
    let phase = actor_ready.is_ok()
        && released.as_ref().is_some_and(|sent| sent.is_ok())
        && original.as_ref().is_ok_and(|at| {
            runs.len() == 1
                && setup.is_some_and(|(exec, ask, cap)| {
                    exec == runs[0] && ask.stdin && !ask.stdout && !ask.stderr && cap.at() == *at
                })
                && actual_started
                    .as_ref()
                    .is_ok_and(|(exec, seen)| *exec == runs[0] && seen < at)
                && actual_kill.as_ref().is_ok_and(|(request, exec, seen)| {
                    *exec == runs[0] && seen < at && kills == [(*request, *exec)]
                })
        })
        && pipe_was_owned
        && !unused_end;
    // First diagnostics and all assertions occur only after the owned endpoint/ends close and join.
    eprintln!(
        "client_settle_prepared classification={} scope=separate_successor ready={actor_ready:?} released={released:?} original={original:?} cap={CAP:?} observation={OBSERVE:?} setup={setup:?} started={actual_started:?} kill={actual_kill:?} runs={runs:?} kills={kills:?} exits={exits} other_answers={other_answers} releases={releases} before_cleanup={before_cleanup:?} cleanup_at={cleanup_at:?} after_cleanup={after_cleanup:?} pipe_owned={pipe_was_owned} unused_end={unused_end} pipe_poisoned={pipe_poisoned} end_poisoned={end_poisoned} joined={} clean={clean:?}",
        if phase {
            "PHASE_WITNESSED"
        } else {
            "UNVERIFIED"
        },
        joined.is_ok()
    );
    joined.unwrap();
    assert!(!pipe_poisoned && !end_poisoned);
    assert!(transport.io.io.closed.load(Ordering::Acquire));
    assert!(transport.io.program.lock().unwrap().is_none());
    assert!(transport.prepared.lock().unwrap().is_none());
    assert!(clean.unwrap());
    assert_eq!(transport.reply_budget(), None);
    assert!(
        phase,
        "UNVERIFIED: required actual prepared-settle phase missing"
    );
    assert_eq!(exits, 0);
    assert_eq!(other_answers, 0);
    assert_eq!(releases, 0);
    expired.unwrap().unwrap();
    let returned_error = before_cleanup
        .as_ref()
        .ok()
        .or_else(|| after_cleanup.as_ref().ok());
    assert!(
        matches!(returned_error, Some((_, Err(Refusal::Broken(why)))) if why.contains("stdout") && why.contains("neither carried nor asked")),
        "the exact original stream-settlement refusal must survive cleanup"
    );
    assert!(
        before_cleanup.is_ok(),
        "prepared-settle original error return exceeded its original authority awaiting Kill answer"
    );
    let (returned, _) = before_cleanup.unwrap();
    assert!(returned >= original.unwrap());
    assert!(returned <= cleanup_at);
}
