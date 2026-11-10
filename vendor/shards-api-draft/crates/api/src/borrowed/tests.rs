//! HEX-RT-17: borrowed streams have a handshake and an identity check, never a VM lifecycle.

use super::*;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};

#[cfg(windows)]
use shards_afunix::UnixListener;
#[cfg(unix)]
use std::os::unix::net::UnixListener;

struct Process(Child);

impl Process {
    fn new() -> Self {
        Self(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "borrowed::tests::process_waits_for_its_owners_stdin",
                    "--ignored",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }

    fn identity(&self, path: &Path) -> Identity {
        #[cfg(unix)]
        let started = crate::taken::started(self.0.id()).unwrap();
        #[cfg(windows)]
        let started = windows::Held::open(self.0.id()).unwrap().started().unwrap();
        Identity {
            pid: self.0.id(),
            started,
            vsock: path.to_owned(),
        }
    }

    fn alive(&mut self) {
        assert!(self.0.try_wait().unwrap().is_none());
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        drop(self.0.stdin.take());
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "subprocess helper; its owner closes stdin to end it"]
fn process_waits_for_its_owners_stdin() {
    std::io::copy(&mut std::io::stdin(), &mut std::io::sink()).unwrap();
}

fn request(stream: &mut impl Read) {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while line.last() != Some(&b'\n') {
        stream.read_exact(&mut byte).unwrap();
        line.extend(byte);
    }
    assert_eq!(line, b"CONNECT 1234\n");
}

/// HEX-RT-17: connect to a real AF_UNIX stand-in muxer, exchange bytes and drop the connection; the named
/// process remains alive and its socket directory remains the owner's.
#[test]
fn a_borrowed_connection_never_takes_or_ends_the_vms_life() {
    let dir = shards_testdir::TempDir::new("borrowed-connect").unwrap();
    let path = dir.join("v");
    let listener = UnixListener::bind(&path).unwrap();
    let mut vm = Process::new();
    let identity = vm.identity(&path);
    let muxer = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        request(&mut stream);
        stream.write_all(b"OK 42\n").unwrap();
        let mut ping = [0u8; 4];
        stream.read_exact(&mut ping).unwrap();
        assert_eq!(&ping, b"ping");
        stream.write_all(b"pong").unwrap();
        assert_eq!(stream.read(&mut [0u8; 1]).unwrap(), 0);
    });
    let mut stream = connect_borrowed(&identity, 1234, Duration::ZERO).unwrap();
    stream.write_all(b"ping").unwrap();
    let mut pong = [0u8; 4];
    stream.read_exact(&mut pong).unwrap();
    assert_eq!(&pong, b"pong");
    drop(stream);
    muxer.join().unwrap();
    vm.alive();
    assert!(dir.is_dir());
}

/// HEX-RT-17: a reused/wrong creation time is refused before any dial reaches the listening muxer.
#[test]
fn a_wrong_creation_time_is_refused_before_dial() {
    let dir = shards_testdir::TempDir::new("borrowed-wrong").unwrap();
    let path = dir.join("v");
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut vm = Process::new();
    let mut identity = vm.identity(&path);
    identity.started = identity.started.checked_add(1).unwrap();
    assert!(matches!(
        connect_borrowed(&identity, 1234, Duration::ZERO),
        Err(Refusal::Gone { .. })
    ));
    assert_eq!(
        listener.accept().err().unwrap().kind(),
        std::io::ErrorKind::WouldBlock
    );
    vm.alive();
}

/// HEX-RT-17: an ended VM is refused before dial even while a stand-in muxer still listens at its path.
#[test]
fn an_ended_process_is_refused_before_dial() {
    let dir = shards_testdir::TempDir::new("borrowed-dead").unwrap();
    let path = dir.join("v");
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut vm = Process::new();
    let identity = vm.identity(&path);
    drop(vm.0.stdin.take());
    vm.0.wait().unwrap();
    assert!(matches!(
        connect_borrowed(&identity, 1234, Duration::ZERO),
        Err(Refusal::Gone { .. })
    ));
    assert_eq!(
        listener.accept().err().unwrap().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

/// HEX-RT-17: a muxer that withholds its answer is bounded by the caller's handshake patience, with only
/// one connection attempt; the borrowed VM remains alive when that connection is refused.
#[test]
fn a_blocked_answer_ends_at_the_provided_handshake_patience() {
    let dir = shards_testdir::TempDir::new("borrowed-blocked").unwrap();
    let path = dir.join("v");
    let listener = UnixListener::bind(&path).unwrap();
    let mut vm = Process::new();
    let identity = vm.identity(&path);
    let muxer = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        request(&mut stream);
        assert_eq!(stream.read(&mut [0u8; 1]).unwrap(), 0);
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().err().unwrap().kind(),
            std::io::ErrorKind::WouldBlock
        );
    });
    let patience = crate::conformance::UNSERVED_PATIENCE;
    let (worker, answer) =
        crate::while_runs::on_a_thread(move || connect_borrowed(&identity, 1234, patience));
    let got = crate::while_runs::answer(&answer, worker, patience);
    assert!(matches!(got, Err(Refusal::Channel { port: 1234, .. })));
    muxer.join().unwrap();
    vm.alive();
}

/// HEX-RT-17: a successful muxer answer cannot mask the VM ending while CONNECT was under way.
#[test]
fn the_process_is_checked_again_after_the_handshake() {
    let dir = shards_testdir::TempDir::new("borrowed-ended-during").unwrap();
    let path = dir.join("v");
    let listener = UnixListener::bind(&path).unwrap();
    let mut vm = Process::new();
    let identity = vm.identity(&path);
    std::thread::scope(|scope| {
        let muxer = scope.spawn(|| {
            let (mut stream, _) = listener.accept().unwrap();
            request(&mut stream);
            drop(vm.0.stdin.take());
            vm.0.wait().unwrap();
            stream.write_all(b"OK 42\n").unwrap();
        });
        assert!(matches!(
            connect_borrowed(&identity, 1234, Duration::ZERO),
            Err(Refusal::Gone { .. })
        ));
        muxer.join().unwrap();
    });
}
