//! `peerclose <rounds> [watch_ms]`: how soon a peer closes after the node ends its connection by
//! half-closing it (native plan step 8; the Windows node's two-phase close, the lead's ruling 2026-11-09).
//! Each round the node reads a peer's hostile request (a prefix past the format's largest message), answers
//! it (a refusal's size), shuts its sending side (`SD_SEND`, the end-of-stream) and waits for the peer's
//! close: the time from the shutdown to the peer's close is one sample. The peer, a caller process of this
//! probe's, reads to the end it is given and closes at once, as Hex's client and its tests do.
//!
//! afunix.sys now and then loses that end-of-stream to a peer in another process (2026-11-09, ~2% of
//! graceful closes): a round whose peer hasn't closed within the node's watch (`watch_ms`, the probe's own,
//! well past any close it times) is counted lost and its connection reset (linger 0), not taken as a sample.
//! The caller watches twice as long, so the node's watch always ends a lost round first. Prints a CSV row
//! per round: round, close_us (empty when lost), lost, and the machine's CPU load over the last second.
//!
//! The record's slowest close seeds the node's learned wait for a peer's close (Hex's rule: twice the slowest
//! of the record and what it has seen since).

use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use shards_afunix::{UnixListener, UnixStream};
use windows_sys::Win32::Networking::WinSock::{
    LINGER, SD_SEND, SO_LINGER, SO_RCVTIMEO, SOCKET, SOL_SOCKET, setsockopt, shutdown,
};

/// The peer's request: a frame prefix past the format's largest message, as the hostile test's.
const REQUEST: [u8; 4] = [0xff; 4];
/// The node's answer to it: a refusal's size, as the node's own (62 bytes on jupiter, 2026-11-09).
const ANSWER: usize = 62;

/// The node's side: `rounds` connections from one caller process, each answered and half-closed.
pub fn peerclose(rounds: usize, watch: Duration) {
    let path = crate::socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    let caller_watch = watch.saturating_mul(2).as_millis().to_string();
    let mut child = crate::spawn(&[
        "peerclose-caller",
        path.to_str().unwrap(),
        &rounds.to_string(),
        &caller_watch,
    ]);
    // The load column reads the machine over the last whole second: the first reading is taken a second
    // before the first round, so every round's spans one (a run of 299 rounds lasts about a second).
    let mut samples = std::collections::VecDeque::new();
    let _ = crate::load_over_last_second(&mut samples);
    std::thread::sleep(Duration::from_secs(1));
    println!("round,close_us,lost,load_pct_last_s");
    for r in 0..rounds {
        let (mut s, ()) = listener.accept().unwrap();
        let mut request = [0u8; REQUEST.len()];
        s.read_exact(&mut request).unwrap();
        s.write_all(&[0x5a; ANSWER]).unwrap();
        receive_watch(&s, watch);
        // SAFETY: s is a connected socket this side owns; SD_SEND ends its sending side.
        let rc = unsafe { shutdown(s.raw() as SOCKET, SD_SEND) };
        assert!(rc == 0, "shutdown: {}", std::io::Error::last_os_error());
        let t0 = Instant::now();
        let mut rest = [0u8; 16];
        let closed = matches!(s.read(&mut rest), Ok(0));
        let took = t0.elapsed();
        if !closed {
            reset(&s);
        }
        drop(s);
        let load = crate::load_over_last_second(&mut samples);
        if closed {
            println!("{r},{},0,{load:.1}", took.as_micros());
        } else {
            println!("{r},,1,{load:.1}");
        }
    }
    child.wait().unwrap();
    let _ = std::fs::remove_file(&path);
}

/// The peer's side: `rounds` connections in turn, each sent the request and read to its end (or its
/// reset), then closed at once.
pub fn caller(path: &Path, rounds: usize, watch: Duration) {
    let mut buf = [0u8; 256];
    for _ in 0..rounds {
        let mut c = UnixStream::connect(path).unwrap();
        c.write_all(&REQUEST).unwrap();
        receive_watch(&c, watch);
        while let Ok(n) = c.read(&mut buf) {
            if n == 0 {
                break;
            }
        }
        drop(c);
    }
}

/// Bounds a blocking receive on `s` by `watch` (`SO_RCVTIMEO`, milliseconds).
fn receive_watch(s: &UnixStream, watch: Duration) {
    let ms = u32::try_from(watch.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: SO_RCVTIMEO takes a DWORD of milliseconds, alive for the call.
    let rc = unsafe {
        setsockopt(
            s.raw() as SOCKET,
            SOL_SOCKET,
            SO_RCVTIMEO,
            (&ms as *const u32).cast(),
            4,
        )
    };
    assert!(rc == 0, "SO_RCVTIMEO: {}", std::io::Error::last_os_error());
}

/// Makes `s`'s close a reset (linger 0), for a round whose peer never saw its end.
fn reset(s: &UnixStream) {
    let l = LINGER {
        l_onoff: 1,
        l_linger: 0,
    };
    // SAFETY: SO_LINGER takes a LINGER, alive for the call.
    let rc = unsafe {
        setsockopt(
            s.raw() as SOCKET,
            SOL_SOCKET,
            SO_LINGER,
            (&l as *const LINGER).cast(),
            size_of::<LINGER>() as i32,
        )
    };
    assert!(rc == 0, "SO_LINGER: {}", std::io::Error::last_os_error());
}
