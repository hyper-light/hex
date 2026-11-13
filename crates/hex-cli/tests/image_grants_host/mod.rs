//! What the host shows of a run (joint plan step 15, AC-T7.7.2; docs/design/network-grants.md N4): its processes,
//! under the one run host the node `node` started (`shards api-run`, D231), and the TCP and UDP sockets they hold.
//! One reader for each OS behind one interface (`os::processes`, `os::sockets`), each saying how it reads
//! ([`HOW`], told with every reading): /proc on Linux, `ps` and `lsof` on macOS, CIM's `Win32_Process` and the
//! NetTCPIP module's tables on Windows. What a reader can't read fails the test: what can't be seen isn't passed.

use std::net::SocketAddr;

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod os;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod os;
#[cfg(windows)]
#[path = "windows.rs"]
mod os;

pub use os::HOW;

/// A process as the host shows it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    pub parent: u32,
    /// Its program's file name as the OS keeps it: Linux's `comm`, macOS's accounting name, Windows' image name
    /// without its `.exe`.
    pub name: String,
    /// Its command line's words, its program's first.
    pub args: Vec<String>,
}

/// A TCP or UDP socket a process holds, as the host shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Socket {
    pub tcp: bool,
    pub local: SocketAddr,
    /// Its peer, where it has one.
    pub remote: Option<SocketAddr>,
    /// A TCP socket in its listening state.
    pub listening: bool,
}

impl Socket {
    /// It takes what comes in: a listening TCP socket, or a UDP socket with no peer.
    pub fn listens(&self) -> bool {
        if self.tcp {
            self.listening
        } else {
            self.remote.is_none()
        }
    }

    /// It carries a flow to a peer past the host's own loopback.
    pub fn leaves(&self) -> bool {
        !self.listens()
            && self.remote.is_some_and(|peer| {
                let peer = peer.ip().to_canonical();
                !peer.is_loopback() && !peer.is_unspecified()
            })
    }
}

/// The roots and every process under them, each found by its parent, breadth first, with no recursion
/// (CLAUDE.md §11).
fn under(all: &[Process], roots: Vec<u32>) -> Vec<u32> {
    let mut found = roots;
    let mut at = 0;
    while let Some(&parent) = found.get(at) {
        found.extend(
            all.iter()
                .filter(|p| p.parent == parent && p.pid != parent)
                .map(|p| p.pid),
        );
        at += 1;
    }
    found
}

/// The run hosts under the node `node`: `shards api-run` (D231), its first argument after its program.
fn run_hosts(all: &[Process], node: u32) -> Vec<u32> {
    let of_node = under(all, vec![node]);
    all.iter()
        .filter(|p| of_node.contains(&p.pid) && p.args.get(1).is_some_and(|a| a == "api-run"))
        .map(|p| p.pid)
        .collect()
}

/// The run's processes: the one run host under the node `node` and every process under it.
fn run_of(node: u32) -> Vec<Process> {
    let all = os::processes();
    let hosts = run_hosts(&all, node);
    assert_eq!(hosts.len(), 1, "the node's run hosts ({HOW}): {hosts:?}");
    let pids = under(&all, hosts);
    let mut run: Vec<Process> = all.into_iter().filter(|p| pids.contains(&p.pid)).collect();
    run.sort_by_key(|p| p.pid);
    eprintln!("GRANTS-HOST read by {HOW}");
    for p in &run {
        eprintln!("GRANTS-PROCESS {} {} {:?}", p.pid, p.name, p.args);
    }
    run
}

/// The TCP and UDP sockets the run's processes hold, each once.
fn sockets_of(run: &[Process]) -> Vec<Socket> {
    let pids: Vec<u32> = run.iter().map(|p| p.pid).collect();
    let sockets = os::sockets(&pids);
    for s in &sockets {
        eprintln!(
            "GRANTS-SOCKET {} {} -> {} {}",
            if s.tcp { "tcp" } else { "udp" },
            s.local,
            s.remote
                .map_or_else(|| "none".to_owned(), |r| r.to_string()),
            if s.listens() { "listens" } else { "" }
        );
    }
    sockets
}

/// How many of the run's processes are named `name`.
fn named(run: &[Process], name: &str) -> usize {
    run.iter().filter(|p| p.name == name).count()
}

/// A VM granted nothing whose image links its harness past the microVM: its run's VM process beside its run host,
/// and one network process (D250: the run keeps its network, the network process told nothing kept, so it lets
/// nothing past), and no socket that listens or leaves the host.
pub fn granted_nothing(node: u32) {
    let run = run_of(node);
    assert_eq!(
        named(&run, "shards-vm"),
        1,
        "the run's VM process: {run:#?}"
    );
    assert_eq!(
        named(&run, "shards-net"),
        1,
        "the run's network process (D250): {run:#?}"
    );
    let sockets = sockets_of(&run);
    let listening: Vec<&Socket> = sockets.iter().filter(|s| s.listens()).collect();
    assert!(
        listening.is_empty(),
        "the run of a VM granted nothing listens: {listening:#?}"
    );
    let leaving: Vec<&Socket> = sockets.iter().filter(|s| s.leaves()).collect();
    assert!(
        leaving.is_empty(),
        "the run of a VM granted nothing holds a flow past the host's loopback: {leaving:#?}"
    );
}

/// A VM granted one publish, bound at `at`: its network process there (D36: so a VM granted nothing, which had
/// none, had a real one to lack), and its run listening at `at` alone, at no wildcard address and not on the
/// image's port `ungranted`.
pub fn listens_alone(node: u32, at: SocketAddr, ungranted: u16) {
    let run = run_of(node);
    assert_eq!(
        named(&run, "shards-net"),
        1,
        "the run's network process: {run:#?}"
    );
    let sockets = sockets_of(&run);
    let listening: Vec<&Socket> = sockets.iter().filter(|s| s.listens()).collect();
    assert!(
        listening.iter().all(|s| !s.local.ip().is_unspecified()),
        "the run listens at a wildcard address: {listening:#?}"
    );
    assert!(
        listening.iter().all(|s| s.local.port() != ungranted),
        "the run listens on the ungranted port {ungranted}: {listening:#?}"
    );
    assert!(
        matches!(listening.as_slice(), [only] if only.tcp && only.local == at),
        "the run listens other than at {at} alone: {listening:#?}"
    );
}

/// No process of the run holds a flow past the host's loopback.
pub fn nothing_leaves(node: u32) {
    let leaving: Vec<Socket> = sockets_of(&run_of(node))
        .into_iter()
        .filter(Socket::leaves)
        .collect();
    assert!(
        leaving.is_empty(),
        "the run holds a flow past the host's loopback: {leaving:#?}"
    );
}

/// No run host is left under the node `node`.
pub fn no_run_left(node: u32) {
    let left = run_hosts(&os::processes(), node);
    assert!(
        left.is_empty(),
        "run hosts left under the node ({HOW}): {left:?}"
    );
}

/// The non-vacuity of this OS's reading (no VM), on every OS: do start a known child, `sort` waiting on its stdin
/// (a program each OS has), and hold, in this process, a TCP listener and a UDP socket on the loopback and a TCP
/// connection to the listener. Expect the child found under this process by its name, the listener and the UDP
/// socket listening at their addresses, and the connection at its own, its peer the listener's, neither listening
/// nor leaving the host.
#[test]
fn the_hosts_reading_finds_a_known_child_and_a_processs_own_sockets() {
    let mut child = std::process::Command::new("sort")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let udp = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let connection = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (_accepted, _) = listener.accept().unwrap();
    let this = std::process::id();
    let all = os::processes();
    let sockets = os::sockets(&[this]);
    drop(child.stdin.take());
    let reaped = child.wait();
    eprintln!("GRANTS-HOST read by {HOW}: child {} {reaped:?}", child.id());
    let found = all
        .iter()
        .find(|p| p.pid == child.id())
        .unwrap_or_else(|| panic!("no process {} in this host's reading", child.id()));
    assert_eq!(
        (found.parent, found.name.as_str()),
        (this, "sort"),
        "{found:?}"
    );
    assert!(under(&all, vec![this]).contains(&child.id()));
    let at = |local: SocketAddr, tcp: bool| {
        sockets
            .iter()
            .find(|s| s.local == local && s.tcp == tcp)
            .unwrap_or_else(|| panic!("no socket at {local} (tcp {tcp}) in {sockets:#?}"))
    };
    assert!(at(listener.local_addr().unwrap(), true).listens());
    assert!(at(udp.local_addr().unwrap(), false).listens());
    let near = at(connection.local_addr().unwrap(), true);
    assert_eq!(near.remote, Some(listener.local_addr().unwrap()));
    assert!(!near.listens() && !near.leaves(), "{near:?}");
}

/// What leaves the host, judged alike on every OS (no VM): do judge sockets whose peers are past the loopback, at it
/// (an IPv4-mapped one too) and none; expect only those past it to leave, and a listener never to.
#[test]
fn only_a_flow_past_the_loopback_leaves() {
    let tcp = |remote: Option<&str>, listening| Socket {
        tcp: true,
        local: "10.0.0.2:40000".parse().unwrap(),
        remote: remote.map(|r| r.parse().unwrap()),
        listening,
    };
    assert!(tcp(Some("192.0.2.1:443"), false).leaves());
    assert!(!tcp(Some("127.0.0.1:443"), false).leaves());
    assert!(!tcp(Some("[::ffff:127.0.0.1]:443"), false).leaves());
    assert!(!tcp(Some("[::1]:443"), false).leaves());
    assert!(!tcp(None, true).leaves());
    assert!(!tcp(None, false).leaves());
    let udp = |remote: Option<&str>| Socket {
        tcp: false,
        local: "127.0.0.1:5353".parse().unwrap(),
        remote: remote.map(|r| r.parse().unwrap()),
        listening: false,
    };
    assert!(udp(None).listens() && !udp(None).leaves());
    assert!(udp(Some("192.0.2.53:53")).leaves());
}
