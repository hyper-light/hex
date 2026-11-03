//! The check a network namespace an owner's box left must pass before the node hands it to the owner's next
//! box (goal 5 extended, step 6; Ada's call of 2026-11-03; docs/research/runtime/goal5-netns-reuse.md). A box
//! that may share one held neither CAP_NET_ADMIN nor CAP_SYS_ADMIN and had a read-only `/proc/sys`
//! ([`crate::linux::oci::spec::Spec::may_share_its_network`]), so it could change none of its namespace's
//! links, addresses, routes, rules, firewall tables or sysctls (Linux v6.19 net/core/rtnetlink.c:6885;
//! net/netfilter/nfnetlink.c:661; net/ipv4/netfilter/ip_tables.c:1625; net/sysctl_net.c:42-54). This reads
//! what it could still have left, and what nothing but a process outside it could have changed, from a thread
//! of the node's that enters the namespace with `setns` and goes back after:
//!
//! - **sockets**: `/proc/net/sockstat`'s "sockets: used" first, every socket of the namespace's from its making to
//!   its freeing (net/core/sock.c:2316-2319, :2417-2418). The check's own netlink socket is made in the namespace the
//!   first time it's checked and kept with it ([`check`]'s `socket`), never closed between checks, since a closed
//!   one would stay counted until an RCU grace period had passed (net/netlink/af_netlink.c:794-796, :703-715). So a
//!   count of 1 is that socket alone: no socket of any kind is left, bound or not. Any other count reads
//!   `/proc/net/protocols`, each protocol's sockets in use (`sock_prot_inuse_get`, net/core/sock.c:4320, a file of
//!   each namespace's, :4373), which must be a fresh namespace's but for the check's own, so a box's netlink sockets
//!   waiting out their grace period aren't taken for sockets left. A unix, netlink or packet socket counts there from
//!   its making to its release (net/unix/af_unix.c:1128, :653; net/netlink/af_netlink.c:688, :794;
//!   net/packet/af_packet.c:3395, :3134), a UDP socket once bound (net/ipv4/udp.c:333, :2244), and a TCP one once
//!   listening or connected, until it's unhashed, so a connection still ending counts
//!   (net/ipv4/inet_hashtables.c:747, :811, :839). The box's processes are gone, so a socket still counted was
//!   handed to a process outside it, which would still reach the next box. A TCP or UDP socket never bound, or a
//!   TCP one bound but neither listening nor connected, counts in no protocol, and past a count of 1 this doesn't
//!   see one. A box that may share its namespace can't write to the check's socket: NETLINK_ROUTE takes a message
//!   to another socket only from one with CAP_NET_ADMIN over the namespace's owner (net/core/rtnetlink.c:7038;
//!   net/netlink/af_netlink.c:1842-1847, :904-908);
//! - **TIME_WAIT**: `/proc/net/sockstat`'s "tw" counts the namespace's TCP connections in TIME_WAIT, IPv4's and
//!   IPv6's (net/ipv4/proc.c:60-63; IPv6's share the namespace's death row, net/ipv6/tcp_ipv6.c:287, :314). One
//!   would meet the next box's own on its port, and none can be ended without CAP_NET_ADMIN, so it's refused;
//! - **links**: `/proc/net/dev` lists loopback alone, read under RCU, with no RTNL (net/core/net-procfs.c);
//! - **firewall**: no iptables table (`/proc/net/ip_tables_names` and `ip6_tables_names` empty or absent);
//! - **addresses, routes and rules**: the namespace's dumps equal a fresh namespace's ([`Fresh`]), each family
//!   dumped by itself, since every one of those dumps takes no RTNL (`inet_dump_ifaddr`,
//!   net/ipv4/devinet.c:2890-2891; `inet6_dump_ifaddr`, net/ipv6/addrconf.c:7515-7517; `inet_dump_fib`,
//!   net/ipv4/fib_frontend.c:1699-1700; `inet6_dump_fib`, net/ipv6/ip6_fib.c:2551-2553; `fib_nl_dumprule`,
//!   net/core/fib_rules.c:1451-1452), where an `AF_UNSPEC` dump goes through `rtnl_dump_all`, which does
//!   (net/core/rtnetlink.c:7069-7070).
//!
//! Every read is bounded, and one that fails or doesn't parse refuses the namespace as unreadable
//! ([`Refused::Unreadable`]): the shell then closes it and the box gets a fresh one.

use std::collections::BTreeMap;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::time::Duration;

use hex_core::netns_reuse::Refused;
use rustix::fs::{Mode, OFlags};
use rustix::net::{AddressFamily, RecvFlags, SendFlags, SocketFlags, SocketType};

use crate::linux::oci::{OciError, net, sys};

/// Most bytes one of the namespace's `/proc/net` files may hold: a clean namespace's are a few hundred, its
/// `protocols` about 5 KiB.
const MAX_PROC_FILE: usize = 64 << 10;
/// Most bytes one dump may answer with, all its messages together: a fresh namespace's are under 2 KiB.
const MAX_DUMP_BYTES: usize = 256 << 10;
/// Most reads one dump may take before it ends.
const MAX_DUMP_READS: usize = 64;
/// Room for one read of a dump's answer: what the kernel allocates each of the dump's answers at, so the less the
/// better, but more than `NLMSG_GOODSIZE`, or an answer could be cut. A socket's dumps are answered in buffers of
/// its largest read so far, capped at 32 KiB (Linux v6.19 net/netlink/af_netlink.c:1940-1943, :2279-2290), each
/// filled no further than that (:2302-2312), where none was read larger than `NLMSG_GOODSIZE`, just under a page
/// or 8 KiB, whichever is less (include/linux/netlink.h:273-275); no dump this asks for sets a larger least
/// (net/core/rtnetlink.c:6905-6906, links' alone). At 32 KiB, fedora spent 5 to 7% of a check allocating and
/// clearing them (`bench/competitive/probes/create_serial.md`, run 15). 8 KiB is over `NLMSG_GOODSIZE` at any page
/// size, and a fresh namespace's answers are under 2 KiB.
const READ_BYTES: usize = 8 << 10;
/// Most one read of a dump's answer waits. A bound, not a tuning: the kernel builds a dump's answer as it's
/// read, so a read waits on nothing but the reading.
const READ_WAIT: Duration = Duration::from_secs(1);
/// How many times a fresh namespace's dumps are read, each after the kernel's next notice of an IPv6
/// address or [`SETTLE_WAIT`], until its loopback's IPv6 address is no longer tentative. `::1` is added
/// tentative (net/ipv6/addrconf.c:1148-1149) and its DAD work is queued (:4173), which clears that once it
/// gets RTNL (:4194, :4107-4121), then puts in its host route (:6283) and tells `RTNLGRP_IPV6_IFADDR` (:4320,
/// :5621). So how long it takes is how long the host's RTNL is held by others: a bound of 1 s in all, not a
/// tuning.
const SETTLE_TRIES: u32 = 8;
const SETTLE_WAIT: Duration = Duration::from_millis(125);
/// `IFA_F_TENTATIVE` (include/uapi/linux/if_addr.h), in an `ifaddrmsg`'s `ifa_flags`, its third byte.
const IFA_F_TENTATIVE: u8 = 0x40;
/// `RTMGRP_IPV6_IFADDR` (include/uapi/linux/rtnetlink.h): the group of `RTNLGRP_IPV6_IFADDR`'s notices.
const RTMGRP_IPV6_IFADDR: u32 = 0x100;

/// The protocol of the socket the check keeps in each namespace it checks, one, as `/proc/net/protocols` names it
/// (net/netlink/af_netlink.c:613).
const OWN_PROTOCOL: &str = "NETLINK";

/// `AF_INET`, `AF_INET6` and `AF_UNSPEC` (include/linux/socket.h).
const AF_INET: u8 = 2;
const AF_INET6: u8 = 10;
const AF_UNSPEC: u8 = 0;
/// `RTM_GETADDR`, `RTM_GETROUTE`, `RTM_GETRULE` (include/uapi/linux/rtnetlink.h).
const RTM_GETADDR: u16 = 22;
const RTM_GETROUTE: u16 = 26;
const RTM_GETRULE: u16 = 34;
/// `NLMSG_ERROR` and `NLMSG_DONE` (include/uapi/linux/netlink.h).
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
/// `NLM_F_REQUEST | NLM_F_DUMP` (include/uapi/linux/netlink.h).
const DUMP: u16 = 0x1 | 0x300;
/// A `struct nlmsghdr`'s bytes.
const NLMSG_HDR: usize = 16;
/// The family header each dump carries after the `nlmsghdr`: `struct ifaddrmsg` (8), `struct rtmsg` (12),
/// `struct fib_rule_hdr` (12).
const IFADDRMSG: usize = 8;
const RTMSG: usize = 12;
const FIB_RULE_HDR: usize = 12;
/// Attributes whose values change with time, not with what the namespace holds: `IFA_CACHEINFO` (6),
/// `RTA_CACHEINFO` (12) and `RTA_EXPIRES` (23).
const IFA_CACHEINFO: u16 = 6;
const RTA_CACHEINFO: u16 = 12;
const RTA_EXPIRES: u16 = 23;
/// The bits of an attribute's type that name it (`NLA_TYPE_MASK`).
const NLA_TYPE_MASK: u16 = 0x3fff;

/// One dump: what it asks, how long the family header its answers carry is, and the attributes dropped.
#[derive(Debug, Clone, Copy)]
struct Dump {
    kind: u16,
    family: u8,
    header: usize,
    volatile: &'static [u16],
    refused: Refused,
}

/// The five dumps a namespace is compared by, each family by itself.
const DUMPS: [Dump; 5] = [
    Dump {
        kind: RTM_GETADDR,
        family: AF_INET,
        header: IFADDRMSG,
        volatile: &[IFA_CACHEINFO],
        refused: Refused::Addresses,
    },
    Dump {
        kind: RTM_GETADDR,
        family: AF_INET6,
        header: IFADDRMSG,
        volatile: &[IFA_CACHEINFO],
        refused: Refused::Addresses,
    },
    Dump {
        kind: RTM_GETROUTE,
        family: AF_INET,
        header: RTMSG,
        volatile: &[RTA_CACHEINFO, RTA_EXPIRES],
        refused: Refused::Routes,
    },
    Dump {
        kind: RTM_GETROUTE,
        family: AF_INET6,
        header: RTMSG,
        volatile: &[RTA_CACHEINFO, RTA_EXPIRES],
        refused: Refused::Routes,
    },
    Dump {
        kind: RTM_GETRULE,
        family: AF_UNSPEC,
        header: FIB_RULE_HDR,
        volatile: &[],
        refused: Refused::Rules,
    },
];

/// The buffers a checking thread reads a namespace's files and dumps into, kept from one check to the next, so
/// a check allocates and clears none of them: with a buffer each per check, a fifth of a check's CPU on fedora
/// was its own code, the C library's most of it (`bench/competitive/probes/create_serial.md`, run 15). One file's
/// room is [`MAX_PROC_FILE`] and a byte (one past it is a file too long), one read of a dump's [`READ_BYTES`],
/// and a dump's answer grows to the longest one read, at most [`MAX_DUMP_BYTES`].
pub struct Scratch {
    text: Vec<u8>,
    answer: Vec<u8>,
    read: Vec<u8>,
}

impl Default for Scratch {
    fn default() -> Self {
        Self {
            text: vec![0; MAX_PROC_FILE.saturating_add(1)],
            answer: Vec::new(),
            read: vec![0; READ_BYTES],
        }
    }
}

/// A fresh namespace's sockets in use by protocol and its five dumps, each dump's messages normalized and
/// sorted: what an idle namespace's must equal. Read from a namespace made as the pool makes them
/// ([`net::make_ahead`]), its loopback up, once its loopback's IPv6 address is no longer tentative, as an idle
/// one's long is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fresh {
    protocols: BTreeMap<String, u64>,
    dumps: Vec<Vec<Vec<u8>>>,
}

impl Fresh {
    /// A fresh namespace's counts and dumps, read on the calling thread, which `home` is the namespace of and
    /// `proc_net` its `/proc/thread-self/net` ([`ProcNet`]): the namespace is made, entered, read and closed, and
    /// the thread goes back home whether or not the reads worked.
    pub fn read(home: BorrowedFd<'_>, proc_net: &ProcNet) -> Result<Self, OciError> {
        let ns = net::make_ahead(home)?;
        sys::setns(ns.as_fd(), libc::CLONE_NEWNET)?;
        let mut scratch = Scratch::default();
        let read = proc_net
            .file("protocols", &mut scratch.text)
            .and_then(|text| in_use(text.ok_or(Refused::Unreadable)?))
            .map_err(|why| OciError::os("reading a fresh network namespace", why.name().to_owned()))
            .and_then(|protocols| settled(&mut scratch).map(|dumps| Self { protocols, dumps }));
        let back = sys::setns(home, libc::CLONE_NEWNET);
        match (read, back) {
            (Ok(fresh), Ok(())) => Ok(fresh),
            (Err(e), _) | (Ok(_), Err(e)) => Err(e),
        }
    }
}

/// The dumps of the namespace the calling thread is in, once no IPv6 address in it is tentative: read at
/// most [`SETTLE_TRIES`] times, each after the namespace's next IPv6 address notice or [`SETTLE_WAIT`],
/// whichever comes first. The socket the notices come to is bound before the first read, so none is missed.
fn settled(scratch: &mut Scratch) -> Result<Vec<Vec<Vec<u8>>>, OciError> {
    const WAITING: &str = "waiting for a fresh network namespace's IPv6 address notices";
    let notices = rustix::net::socket_with(
        AddressFamily::NETLINK,
        SocketType::RAW,
        SocketFlags::CLOEXEC,
        None,
    )
    .map_err(|e| OciError::os(WAITING, e))?;
    rustix::net::sockopt::set_socket_timeout(
        &notices,
        rustix::net::sockopt::Timeout::Recv,
        Some(SETTLE_WAIT),
    )
    .map_err(|e| OciError::os(WAITING, e))?;
    rustix::net::bind(
        &notices,
        &rustix::net::netlink::SocketAddrNetlink::new(0, RTMGRP_IPV6_IFADDR),
    )
    .map_err(|e| OciError::os(WAITING, e))?;
    let mut notice = vec![0u8; READ_BYTES];
    let socket = dump_socket()
        .map_err(|why| OciError::os("reading a fresh network namespace", why.name().to_owned()))?;
    for _ in 0..SETTLE_TRIES {
        let seen = dumps(&socket, scratch).map_err(|why| {
            OciError::os("reading a fresh network namespace", why.name().to_owned())
        })?;
        if !tentative(&seen) {
            return Ok(seen);
        }
        match rustix::net::recv(&notices, &mut notice, RecvFlags::empty()) {
            Ok(_) | Err(rustix::io::Errno::AGAIN) => {}
            Err(e) => return Err(OciError::os(WAITING, e)),
        }
    }
    Err(OciError::Broken(format!(
        "a fresh network namespace's loopback IPv6 address was still tentative after {SETTLE_TRIES} reads, \
         each after a notice or {} ms",
        SETTLE_WAIT.as_millis()
    )))
}

/// Whether an address in `dumps` is tentative: each normalized address message is its type (2 bytes), then
/// its `ifaddrmsg`, whose third byte is its flags.
fn tentative(dumps: &[Vec<Vec<u8>>]) -> bool {
    DUMPS.iter().zip(dumps).any(|(d, msgs)| {
        d.kind == RTM_GETADDR
            && msgs
                .iter()
                .any(|m| m.get(4).is_some_and(|flags| flags & IFA_F_TENTATIVE != 0))
    })
}

/// What a check found, and whether the thread got back home after it.
#[derive(Debug, PartialEq, Eq)]
pub struct Checked {
    /// Clean, or why not.
    pub verdict: Result<(), Refused>,
    /// Whether the thread is back in `home`. A thread that isn't is still in the namespace it checked, so
    /// the shell checks nothing more on it.
    pub home: bool,
}

/// Checks the namespace `ns` from the calling thread, whose own namespace is `home`, whose
/// `/proc/thread-self/net` is `proc_net` and whose buffers are `scratch`: entered, read and left. `socket` is the
/// netlink socket the check keeps in `ns`, made here the first time and kept by the caller with the namespace,
/// closed with it.
pub fn check(
    ns: BorrowedFd<'_>,
    home: BorrowedFd<'_>,
    proc_net: &ProcNet,
    fresh: &Fresh,
    scratch: &mut Scratch,
    socket: &mut Option<OwnedFd>,
) -> Checked {
    if sys::setns(ns, libc::CLONE_NEWNET).is_err() {
        return Checked {
            verdict: Err(Refused::Unreadable),
            home: true,
        };
    }
    let verdict = look(proc_net, fresh, scratch, socket);
    let home = sys::setns(home, libc::CLONE_NEWNET).is_ok();
    Checked { verdict, home }
}

/// The checks, in the namespace the thread is in: sockets first, before this opens one of its own there.
fn look(
    proc_net: &ProcNet,
    fresh: &Fresh,
    scratch: &mut Scratch,
    socket: &mut Option<OwnedFd>,
) -> Result<(), Refused> {
    // Made before the count is read, so the count has it.
    if socket.is_none() {
        *socket = Some(dump_socket()?);
    }
    let socket = socket.as_ref().ok_or(Refused::Unreadable)?;
    sockets_left(proc_net, fresh, &mut scratch.text)?;
    let text = &mut scratch.text;
    links(proc_net.file("dev", text)?.ok_or(Refused::Unreadable)?)?;
    for tables in ["ip_tables_names", "ip6_tables_names"] {
        if proc_net
            .file(tables, text)?
            .is_some_and(|t| !t.trim().is_empty())
        {
            return Err(Refused::Firewall);
        }
    }
    let seen = dumps(socket, scratch)?;
    for ((d, have), want) in DUMPS.iter().zip(&seen).zip(&fresh.dumps) {
        if have != want {
            return Err(d.refused);
        }
    }
    Ok(())
}

/// Whether a socket or a TCP connection in TIME_WAIT is left in the namespace the calling thread is in, whose
/// `/proc/net` is `proc_net`, the check's own socket made there already: `sockstat` first, its TIME_WAIT and every
/// socket's count, and the protocols' counts only when that count isn't the check's own socket alone.
fn sockets_left(proc_net: &ProcNet, fresh: &Fresh, text: &mut [u8]) -> Result<(), Refused> {
    let sockstat = proc_net
        .file("sockstat", text)?
        .ok_or(Refused::Unreadable)?;
    time_wait(sockstat)?;
    if used(sockstat)? == 1 {
        return Ok(());
    }
    sockets(
        proc_net
            .file("protocols", text)?
            .ok_or(Refused::Unreadable)?,
        &fresh.protocols,
    )
}

/// A thread's `/proc/thread-self/net`, held open by the thread that opened it, so each check opens a file of
/// it by its name alone, not by a walk through `/proc`, `thread-self` and the thread's directory each time. A
/// name looked up in it names the file of the namespace the thread is in at the lookup
/// (`proc_tgid_net_lookup`, `get_proc_task_net`, Linux v6.19 fs/proc/proc_net.c:268-301), and each of those
/// files is looked up again at each open, never taken from the dentry cache (`pde_force_lookup`, :123, :217;
/// `proc_net_d_revalidate`, fs/proc/generic.c:350), so after a `setns` it opens the new namespace's.
pub struct ProcNet(OwnedFd);

impl ProcNet {
    /// The calling thread's: the checks must run on that thread.
    pub fn of_this_thread() -> Result<Self, OciError> {
        rustix::fs::open(
            "/proc/thread-self/net",
            OFlags::DIRECTORY | OFlags::RDONLY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map(Self)
        .map_err(|e| OciError::os("opening /proc/thread-self/net", e))
    }

    /// Its file `name`, read whole into `room`, each read as large as the room left, `None` where the kernel has
    /// no such file (a module not loaded). Bounded at [`MAX_PROC_FILE`]: a file that fills `room` to one byte
    /// past it is refused.
    fn file<'r>(&self, name: &str, room: &'r mut [u8]) -> Result<Option<&'r str>, Refused> {
        let fd = match rustix::fs::openat(
            &self.0,
            name,
            OFlags::RDONLY | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => fd,
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(_) => return Err(Refused::Unreadable),
        };
        let mut filled = 0;
        // Each read fills at least a byte of `room` or ends the file, so this ends within `room`'s length.
        loop {
            let left = room.get_mut(filled..).ok_or(Refused::Unreadable)?;
            if left.is_empty() {
                break;
            }
            match rustix::io::read(&fd, left).map_err(|_| Refused::Unreadable)? {
                0 => break,
                n => filled = filled.checked_add(n).ok_or(Refused::Unreadable)?,
            }
        }
        if filled > MAX_PROC_FILE {
            return Err(Refused::Unreadable);
        }
        let room: &'r [u8] = room;
        let text = room.get(..filled).ok_or(Refused::Unreadable)?;
        std::str::from_utf8(text)
            .map(Some)
            .map_err(|_| Refused::Unreadable)
    }
}

/// `/proc/net/protocols`' sockets in use by protocol, each handed to `each` as it's read: under its header
/// (net/core/sock.c:4346-4359), each line names a protocol first and gives its sockets third (:4313-4320). A
/// file that doesn't parse, or names no protocol, is unreadable.
fn each_in_use(protocols: &str, mut each: impl FnMut(&str, u64)) -> Result<(), Refused> {
    let mut lines = protocols.lines();
    let mut header = lines
        .next()
        .ok_or(Refused::Unreadable)?
        .split_ascii_whitespace();
    if header.next() != Some("protocol") || header.nth(1) != Some("sockets") {
        return Err(Refused::Unreadable);
    }
    let mut named = false;
    for line in lines {
        let mut words = line.split_ascii_whitespace();
        let (Some(name), Some(sockets)) = (words.next(), words.nth(1)) else {
            return Err(Refused::Unreadable);
        };
        each(name, sockets.parse().map_err(|_| Refused::Unreadable)?);
        named = true;
    }
    if named {
        Ok(())
    } else {
        Err(Refused::Unreadable)
    }
}

/// A fresh namespace's sockets in use by protocol.
fn in_use(protocols: &str) -> Result<BTreeMap<String, u64>, Refused> {
    let mut out = BTreeMap::new();
    each_in_use(protocols, |name, n| {
        out.insert(name.to_owned(), n);
    })?;
    Ok(out)
}

/// Whether a socket is left: a protocol in `protocols` with more sockets in use than in a fresh namespace, the
/// check's own netlink socket aside.
fn sockets(protocols: &str, fresh: &BTreeMap<String, u64>) -> Result<(), Refused> {
    let mut left = false;
    each_in_use(protocols, |name, n| {
        let own = u64::from(name == OWN_PROTOCOL);
        left |= n > fresh.get(name).copied().unwrap_or(0).saturating_add(own);
    })?;
    if left { Err(Refused::Sockets) } else { Ok(()) }
}

/// `sockstat`'s count of the namespace's sockets: its "sockets: used" (`sock_inuse_get`, net/socket.c:3321, from
/// net/ipv4/proc.c:60).
fn used(sockstat: &str) -> Result<u64, Refused> {
    sockstat
        .lines()
        .find_map(|line| line.strip_prefix("sockets: used "))
        .and_then(|n| n.trim().parse().ok())
        .ok_or(Refused::Unreadable)
}

/// `sockstat`'s word on TIME_WAIT: its TCP line's "tw" (net/ipv4/proc.c:60-63).
fn time_wait(sockstat: &str) -> Result<(), Refused> {
    let tw = sockstat
        .lines()
        .find_map(|line| {
            let mut words = line.strip_prefix("TCP:")?.split_ascii_whitespace();
            words.by_ref().find(|w| *w == "tw")?;
            words.next()?.parse::<u64>().ok()
        })
        .ok_or(Refused::Unreadable)?;
    if tw > 0 {
        return Err(Refused::TimeWait);
    }
    Ok(())
}

/// `/proc/net/dev`'s word: loopback, and no other link. Its two first lines are headers; each other names a
/// link before its colon.
fn links(dev: &str) -> Result<(), Refused> {
    let mut lo = false;
    for line in dev.lines().skip(2) {
        let name = line.split(':').next().map(str::trim).unwrap_or_default();
        match name {
            "" => {}
            "lo" => lo = true,
            _ => return Err(Refused::Links),
        }
    }
    if lo { Ok(()) } else { Err(Refused::Links) }
}

/// The five dumps of `socket`'s namespace, the one it was made in, each normalized and sorted, read through
/// `scratch`.
fn dumps(socket: &OwnedFd, scratch: &mut Scratch) -> Result<Vec<Vec<Vec<u8>>>, Refused> {
    let mut out = Vec::with_capacity(DUMPS.len());
    for (seq, d) in (1u32..).zip(DUMPS.iter()) {
        ask(socket, &request(d.kind, d.family, d.header, seq), scratch)?;
        let mut msgs = normalized(&scratch.answer, d, seq)?;
        msgs.sort_unstable();
        out.push(msgs);
    }
    Ok(out)
}

/// A netlink socket for the dumps, in the namespace the calling thread is in, each read bounded by [`READ_WAIT`].
fn dump_socket() -> Result<OwnedFd, Refused> {
    let fd = rustix::net::socket_with(
        AddressFamily::NETLINK,
        SocketType::RAW,
        SocketFlags::CLOEXEC,
        None,
    )
    .map_err(|_| Refused::Unreadable)?;
    rustix::net::sockopt::set_socket_timeout(
        &fd,
        rustix::net::sockopt::Timeout::Recv,
        Some(READ_WAIT),
    )
    .map_err(|_| Refused::Unreadable)?;
    Ok(fd)
}

/// A dump request: a `struct nlmsghdr` and the family header, zero but for its family.
fn request(kind: u16, family: u8, header: usize, seq: u32) -> Vec<u8> {
    let len = NLMSG_HDR.saturating_add(header);
    let mut m = Vec::with_capacity(len);
    m.extend_from_slice(&u32::try_from(len).unwrap_or(u32::MAX).to_ne_bytes());
    m.extend_from_slice(&kind.to_ne_bytes());
    m.extend_from_slice(&DUMP.to_ne_bytes());
    m.extend_from_slice(&seq.to_ne_bytes());
    m.extend_from_slice(&0u32.to_ne_bytes());
    m.push(family);
    m.resize(len, 0);
    m
}

/// Sends `request` and reads the whole answer into `scratch`'s, until `NLMSG_DONE`, bounded.
fn ask(fd: &OwnedFd, request: &[u8], scratch: &mut Scratch) -> Result<(), Refused> {
    let sent =
        rustix::net::send(fd, request, SendFlags::empty()).map_err(|_| Refused::Unreadable)?;
    if sent != request.len() {
        return Err(Refused::Unreadable);
    }
    scratch.answer.clear();
    for _ in 0..MAX_DUMP_READS {
        let (got, _) = rustix::net::recv(fd, &mut scratch.read, RecvFlags::empty())
            .map_err(|_| Refused::Unreadable)?;
        let chunk = scratch.read.get(..got).ok_or(Refused::Unreadable)?;
        if scratch.answer.len().saturating_add(chunk.len()) > MAX_DUMP_BYTES {
            return Err(Refused::Unreadable);
        }
        scratch.answer.extend_from_slice(chunk);
        if ends(chunk)? {
            return Ok(());
        }
    }
    Err(Refused::Unreadable)
}

/// Whether `chunk`'s messages include the dump's end: `NLMSG_DONE`, or an `NLMSG_ERROR`, which `normalized`
/// then refuses.
fn ends(chunk: &[u8]) -> Result<bool, Refused> {
    for m in messages(chunk)? {
        if m.kind == NLMSG_DONE || m.kind == NLMSG_ERROR {
            return Ok(true);
        }
    }
    Ok(false)
}

/// One netlink message: its type, its sequence number and its payload after the `nlmsghdr`.
struct Message<'a> {
    kind: u16,
    seq: u32,
    payload: &'a [u8],
}

/// Each message of `bytes`.
fn messages(bytes: &[u8]) -> Result<Vec<Message<'_>>, Refused> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        let hdr = bytes
            .get(at..at.saturating_add(NLMSG_HDR))
            .ok_or(Refused::Unreadable)?;
        let len = usize::try_from(u32_at(hdr, 0)?).map_err(|_| Refused::Unreadable)?;
        let kind = u16_at(hdr, 4)?;
        let seq = u32_at(hdr, 8)?;
        if len < NLMSG_HDR {
            return Err(Refused::Unreadable);
        }
        let end = at.checked_add(len).ok_or(Refused::Unreadable)?;
        let payload = bytes
            .get(at.saturating_add(NLMSG_HDR)..end)
            .ok_or(Refused::Unreadable)?;
        out.push(Message { kind, seq, payload });
        at = align4(end).ok_or(Refused::Unreadable)?;
    }
    Ok(out)
}

/// The answer to dump `d` (asked as `seq`): each message its type, family header and attributes but the
/// volatile ones, as bytes. An error answer, or a message of another request, is unreadable.
fn normalized(answer: &[u8], d: &Dump, seq: u32) -> Result<Vec<Vec<u8>>, Refused> {
    let mut out = Vec::new();
    for Message {
        kind,
        seq: got_seq,
        payload,
    } in messages(answer)?
    {
        if got_seq != seq || kind == NLMSG_ERROR {
            return Err(Refused::Unreadable);
        }
        if kind == NLMSG_DONE {
            break;
        }
        let header = payload.get(..d.header).ok_or(Refused::Unreadable)?;
        let mut m = Vec::with_capacity(payload.len().saturating_add(2));
        m.extend_from_slice(&kind.to_ne_bytes());
        m.extend_from_slice(header);
        let mut at = d.header;
        while at < payload.len() {
            let len = usize::from(u16_at(payload, at)?);
            let kind = u16_at(payload, at.saturating_add(2))? & NLA_TYPE_MASK;
            if len < 4 {
                return Err(Refused::Unreadable);
            }
            let end = at.checked_add(len).ok_or(Refused::Unreadable)?;
            let attr = payload.get(at..end).ok_or(Refused::Unreadable)?;
            if !d.volatile.contains(&kind) {
                m.extend_from_slice(attr);
            }
            at = align4(end).ok_or(Refused::Unreadable)?;
        }
        out.push(m);
    }
    Ok(out)
}

fn align4(n: usize) -> Option<usize> {
    n.checked_add(3).map(|n| n & !3)
}

fn u16_at(b: &[u8], at: usize) -> Result<u16, Refused> {
    b.get(at..at.saturating_add(2))
        .and_then(|s| <[u8; 2]>::try_from(s).ok())
        .map(u16::from_ne_bytes)
        .ok_or(Refused::Unreadable)
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, Refused> {
    b.get(at..at.saturating_add(4))
        .and_then(|s| <[u8; 4]>::try_from(s).ok())
        .map(u32::from_ne_bytes)
        .ok_or(Refused::Unreadable)
}

#[cfg(test)]
mod tests;
