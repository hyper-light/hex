# Hex's wire over hyper-transport (`hex-quic`)

Joint kiln x Hex x Shards plan (kiln plan `01a1cd8e862e7169a074db3d5fa33f3d`, v9), step 4: "Add a hex-client
transport and a node listener that carry hex-core's wire over hyper-transport lanes: control (the node agent's and
the scheduler's calls); one stream lane per stdio stream; staging and unstaging of file trees; a log lane. TLS is
AES-256-GCM only, and identity comes from kiln's CA through CSR enrollment, so keys never travel. Waits are learned
per (call kind, class), never set by constants." Serves CP-T7.3 (the control plane, from one node to a cell). The
person's directives it holds to (the plan's header): "all such communications should be AESGCM256 bidirectional
encrypted"; "Hex and Kiln's load balancer are peer-to-peer"; no arbitrary constants, only learned and derived values.

Each decision says what it chose, its sources, the alternatives, and what would falsify it (CLAUDE.md §5, R02).

## Q1. How Hex takes hyper-transport

**Chosen.** Vendor hyper-raft at `0856e05df3d136f0870a98808113294a73399206`, the revision kiln vendors (kiln's
`vendor/MANIFEST.toml`), so kiln, Hex and the fleet hold one hyper-raft (kiln's CP-M1). Hex already vendored
hyper-raft at `6a236ae4` (an ancestor of `0856e05d`, 2026-10-16) for hyper-swim and hyper-timing; both move to
`0856e05d` with it (hyper-swim's sources are unchanged between the two; hyper-timing gains `folds.rs` and `link.rs`
additions, its API kept). The take list adds `hyper-transport`, `hyper-quic` and `hyper-tls`, each manifest with the
targets it names (cargo loads a package by them) and their licenses, and the root `LICENSE`; Hex depends on
`hyper-transport` and `hyper-quic` by name, pinned `=0.1.0` as kiln pins them, and patches both names to the copy
(`Cargo.toml` `[patch.crates-io]`). hyper-raft has no `-api` crate (CLAUDE.md §9); the exception recorded for
hyper-swim in `vendor/MANIFEST.toml` is extended to these crates in the same words, and goes when hyper-raft
publishes one.

Three recorded changes (`vendor/hyper-raft/UPSTREAM.md`):
1. hyper-transport takes hyper-quic with `default-features = false`. hyper-quic's default `qlog` feature turns on
   serde_json's `preserve_order` for every crate of a build (hyper-quic `VENDORED.md` §20); Hex writes JSON
   documents (OCI configs, records, HCS documents) whose key order would change. Shards takes hyper-quic without it
   for the same reason (its `crates/remote/Cargo.toml`). hyper-transport names no qlog type.
2. `Endpoint::negotiated(peer)`: the suite and group a connection negotiated, read from hyper-quic's handshake data,
   so each end of a link reports and a test asserts `TLS13_AES_256_GCM_SHA384` (Q5).
3. `Fixed::new` and `Receive::new` grow to their bounds instead of laying them out at the start, so an endpoint pays
   for the buffers and chunks it holds, not for its bounds (Q8): a node's bounds are its own, which can be a host's.

**Alternatives.** A git dependency at a revision (Shards' `crates/remote` takes hyper-quic so): not Hex's vendoring
rule (CLAUDE.md, Vendoring), and kiln, which vendors Hex, could not prove one revision. Taking the crates whole, as
kiln does: hyper-tls alone carries 11 MB of test data and BoGo shims Hex never builds; the take list keeps what
cargo loads (5.5 MB in all).

**Cost.** aws-lc-rs and aws-lc-sys enter Hex's build (AWS-LC's C), the first C Hex compiles: checking another
target from one host needs a C compiler for it. `scripts/zig-cc` (zig, as Shards' `scripts/lint` uses it) compiles
it for each cross target in `scripts/gates.sh`; an MSVC target's lint compiles it against zig's MinGW headers, which a
check never links; a Windows build compiles with MSVC on Windows.

**Falsified by** a hyper-raft revision kiln moves to that changes these crates' API: re-vendor at kiln's revision
(`scripts/vendor.py hyper-raft <commit>`) and carry the changes forward.

## Q2. How synchronous Hex drives the endpoint

hyper-transport's `Endpoint` is sans-IO: "fed `now`, datagrams and timeouts, and it returns datagrams, a timeout and
events. It never spawns, never reads a clock and never opens a socket" (its `lib.rs`).

**Chosen.** One driver thread per endpoint owns the endpoint and its UDP socket and waits on both the socket and a
wake (poll(2), Winsock's `WSAPoll`) until the endpoint's next timeout (`poll_timeout`). Other threads reach it
through a bounded command queue and the wake; each connection's bytes cross between the driver and its owner through
a bounded queue of their own (Q3). This is Shards' driver for hyper-quic (Shards `crates/remote/src/net.rs`, D180:
"A QUIC endpoint on a UDP socket of its own, driven by one thread"), the same owner's rules, and Hex's own shape:
the node agent's I/O thread and the client's reader's turn wait the same way (`hex-node` `serve.rs`, `hex-client`
`calls.rs`).

**Alternative: hyper-rt.** kiln drives the endpoint with `hyper-rt-transport`'s `Driver` on a one-shard hyper-rt
`LocalRuntime` thread (kiln `crates/kiln/src/net.rs`), with batched UDP I/O. Rejected for now: Hex has no async
runtime, so it would take a second runtime into the node beside its own threads, with that runtime's calibrated
configuration (its `RuntimeConfig`, from a machine probe of its own) for one socket. What hyper-rt-transport's driver
adds over this one (batched receives and sends, segmentation offload) is a throughput optimization step 10's
measurement can ask for; the endpoint is the same either way, so moving to it later changes the shell alone.

**Falsified by** step 10's measurement (ROOT): if the single driver thread's datagram rate or stream throughput
falls short of what kiln's balancer drives, the shell moves to batched I/O.

## Q3. The lanes: one byte stream per port connection, on lanes used again

A hyper-transport lane is a unidirectional stream that lasts as long as its connection, frames in order, never
closed (hyper-transport `lane.rs`); its exchanges carry a request and a reply whose bodies' lengths are known before
they are sent (`Endpoint::open`'s `body: Option<u64>`). A program's stdout has no length, and a lane per program would
outlast the program.

**Chosen** (`hex_core::lanes`, pure): a connection is a byte stream to a port on the peer, the guest's ports
(`hex_core::stream`: control, stdio, files) and a log port after them, so the guest's byte protocol runs unchanged
on it (`hex_client::guest`: its client over a link, one code path with the guest, CLAUDE.md §8). A connection holds
one lane of each side's while it is open: Open (port, id) binds the opener's, Accept (id) the acceptor's, data frames
of its port's class carry its bytes, End ends a half and frees its lane from the next frame on, Reset ends both;
lane 0 carries refusals and the accepting side's Welcome (a TLS 1.3 client's handshake completes before the server
has verified its certificate, RFC 8446 §4.4.4, so a dialer counts itself connected once it hears it). One lane per
stdio stream, as the plan asks. Each side may send a connection's first `INITIAL_CREDIT` (two whole frames) and then
what its peer grants as its owner reads (Credit frames): a reader that stops holds its own peer's writer alone.
Holding a slow reader's lane instead (`Endpoint::hold_lane`) would leave its unread bytes in QUIC, whose connection
window every stream shares and only bytes read give back (RFC 9000 §4.1, quinn's MAX_DATA), so one stopped reader
would stall every connection of the peer; with credit the driver reads every lane as frames come.

Classes, most urgent first: control (every connection's own frames and the control port's bytes), stream (stdio),
bulk (staging, the log). Every class is lossless (hyper-transport `Classes::lossless`): a frame is its sender's only
copy (CLAUDE.md §3). The application protocol is `hex/1` (RFC 7301): frame kinds only append, and a change that
doesn't names a new protocol, refused at the handshake.

**Alternatives.** Exchanges per call: the node's wire isn't request and reply alone (a program's end follows its
exec's answer; a list answers in parts; `Taken` and `Release` have no answer). A fixed lane per port, its streams
multiplexed inside: one slow program's stdout would hold every other program's. A new lane per connection: lanes
never close, and QUIC's stream credit (RFC 9000 §4.6) is spent by every one ever opened.

**Falsified by** a test where one connection whose reader stopped stalls another (HEX-QUIC-11), a lane reused
before its last connection's End reached the peer (HEX-QUIC-1, -4), or bytes past a connection's credit taken
(HEX-QUIC-18).

## Q4. The node's listener

**Chosen.** The node serves its control connections through its own socket server's core (`hex-node` `serve.rs`:
its bounds, workers, disconnect rule and program watches), each control connection one of the server's connections,
whole messages framed as the guest's are; an exec's stdio connections are gathered as the guest's relay gathers them
(`hex_core::stream::Gather`) and become the program's pipes, copied to and from their lanes. A stdin connection ends
as the relay's does (HEX-GUEST-41): the peer's End ends the program's pipe; the program's letting its stdin go resets
the connection, so the peer's client lets its caller's pipe go. On Unix its copier waits in poll(2) for the pipe's
readers and for the connection, which writes to a pipe of the copier's own as it changes; Windows' pipes can't be
waited on, so there the copy ends at the next write the pipe refuses, as the client's own stdin copy does. The log
port carries the node's log lines, each counted where a slow reader makes it let one go.

Staging is a Linux node's alone, as on every transport Hex has: a macOS or Windows node stages no file tree on any
transport. At this tree:
- The one staging server is the guest relay's (`hex-node` `relay/files.rs`), built on Linux alone (`hex-node`
  `lib.rs`: `#[cfg(target_os = "linux")] pub mod relay`). `hex guest relay` runs it inside a Linux guest, on the
  guest's vsock files port (`hex_core::stream::files::FILES_PORT`), or on `files.sock` with `--unix` for the tests.
  `hex-cli`'s `guest` module, which holds `hex guest relay`, is built on Linux alone too. A Linux node given a folder
  to stage into (`hex node --staged`, Q9) serves the same server on its endpoint's files port.
- A macOS or Windows node's local socket (`hex-node` `serve/macos.rs`, `serve/windows.rs`) carries the node's calls,
  framed, and an exec's stdio as descriptors or handles. Its wire has no staging call (`hex_core::wire::Call`), and it
  serves no files port.
- Such a node stages only as a client. Its `oci` class sends each box's root into its own Shards guest over the guest
  link (`hex_runtime::guest::GuestRuntime`'s `stage`, through `hex_client::guest::stage::stage`), and the guest's
  relay, a Linux node, receives it.

So its endpoint serves no files port. A peer's staging connection is refused by type (`Why::NoPort`), and a folder
given it is refused as its platform's (`Refusal::Platform`). A peer over QUIC names the host paths such a node holds,
as the node's local clients do.

**Falsified by** the hex-api conformance suite failing through the listener (HEX-QUIC-20, and through `hex node`
itself, HEX-QUIC-23), a stdin connection that outlives its program's stdin or its peer's End (HEX-QUIC-21), or a node
that stages nothing taking a staging connection (HEX-QUIC-22).

## Q5. TLS: AES-256-GCM alone, mutual, under kiln's CA

**Chosen** (`hex-quic` `tls.rs`): TLS 1.3 alone; `TLS13_AES_256_GCM_SHA384` alone, hyper-quic's node provider
narrowed as kiln's reach links (`crates/kiln/src/reach/e2e.rs`) and Shards' QUIC links (Shards e12d5b96) narrow it;
the node provider's hybrid post-quantum groups alone (SecP384r1MLKEM1024 first). TLS 1.3 negotiates one suite for
both directions, keyed apart (RFC 8446 §4.1.3, §7.3), so a peer offering no AES-256-GCM is refused at its
handshake. QUIC's Initial packets keep AES-128-GCM under keys derived from the connection ID (RFC 9001 §5.2): they
carry no application data. Both sides present a certificate kiln's CA issued and verify the other's under the CA's
roots (`WebPkiClientVerifier`); 0-RTT is off (hyper-transport `Tls::prepare`). A peer is named by the DNS name its
certificate carries, which kiln's CA binds at enrollment (kiln `pki.rs` `sign_csr`: "The certificate's name is
forced"); a peer with no certificate under the roots is refused at the handshake, typed.

**Falsified by** either end of a link reporting another suite (HEX-QUIC-10, checkpoint 2), or a peer limited to
ChaCha20-Poly1305 or AES-128-GCM connecting.

## Q6. Identity by CSR enrollment

A node generates its key where it runs and sends kiln's CA a certificate signing request (PKCS #10, RFC 2986) for
its name; the CA answers with the node's chain and its roots. The key never leaves the node. Hex reaches kiln's CA
through a seam (CLAUDE.md §9): a trait, a simulator CA and a conformance suite both pass (checkpoint 4). A `hex node`
keeps its enrollment in a folder of its own (`hex_quic::enroll::store`, Q9): `key.pem` (PKCS #8, made there, mode 0600
on Unix, never sent), `request.pem` (the PKCS #10), and the CA's answer beside them, `chain.pem` (its own certificate
first) and `roots.pem`, all PEM (RFC 7468).

**What kiln implements against the seam.** kiln's CA is kiln's work; nothing here stands in for it.
1. `impl hex_quic::enroll::Ca` over kiln's CA (`crates/kiln/src/pki.rs`), `Send + Sync`:
   `fn enroll(&self, request: &Request) -> Result<Issued, CaRefused>`. `Request { name, csr, grant }` carries the
   name the peer enrolls as, its PKCS #10 request in DER (kiln's `sign_csr` takes PEM), and kiln's enrollment token
   for that name. The implementation:
   - admits `name` only with the grant kiln issued for it, and that grant once, else `CaRefused::NotAdmitted { name,
     why }` (HEX-ENROLL-3, -5);
   - takes a request whose own signature verifies, else `CaRefused::Malformed { why }` (HEX-ENROLL-4). The signature is
     ECDSA P-384 with SHA-384 (`hex_quic::enroll::request`, rcgen's `PKCS_ECDSA_P384_SHA384`); rcgen's
     `CertificateSigningRequestParams::from_der` verifies it as it reads;
   - issues a certificate for the request's public key whose first DNS subject alternative name is `name`, byte for
     byte, whatever the request asked (HEX-ENROLL-2). Hex names a peer by its certificate's first valid DNS name
     (`webpki`'s `valid_dns_names`) and refuses an answer naming another peer (`Refused::Issued`, HEX-ENROLL-7). So a
     name kiln's `sanitize_name` would change is refused `NotAdmitted`, never issued changed;
   - gives it extended key usages serverAuth and clientAuth both, because a node listens and dials (HEX-ENROLL-1
     verifies the certificate for each). kiln's `machine_params` gives clientAuth alone; its `leaf_params` gives
     both;
   - makes it valid now, signed under kiln's CA with an algorithm aws-lc-rs's WebPKI set verifies: ECDSA P-256, P-384
     or P-521, Ed25519, or RSA PKCS #1 or PSS. hyper-quic's node provider narrows suites and groups, not signatures;
   - answers `Issued { chain, roots }`: `chain` is the certificate first, then any intermediates; `roots` are exactly
     the roots every peer verifies under (HEX-ENROLL-1 compares them with the fixture's);
   - refuses `CaRefused::Unreachable { why }` when the CA can't be reached or answers nothing usable.
2. It passes `hex_quic::enroll::conformance::run` (`CASES`: HEX-ENROLL-1 to -5) with a `conformance::Fixture`. The
   fixture's `admit(name)` returns a grant kiln's CA takes once for `name`, and its `roots()` returns the CA's roots.
3. For a `hex node` it starts with `--quic`: it runs `hex node enroll --name <name> --credentials <dir>` on the
   node's machine, takes `<dir>/request.pem` (read with `hex_quic::enroll::store::request(dir, grant)`) to its CA
   with the node's grant, and keeps the answer in the folder with `hex_quic::enroll::store::finish(dir, &issued)`,
   which takes it only once it proves to be for that key and name; then it starts `hex node ... --quic <address>
   --credentials <dir>` and reads `quic <address>` and `ready <path>` from its stdout. Its peers dial the node by
   the name its certificate carries.

Nothing else crosses the seam. No private key travels (HEX-ENROLL-6). A folder holds one enrollment: `store::begin`
refuses a folder that holds a key or request already, and `store::load` refuses a certificate that isn't valid now.
So re-enrolling uses a new folder, with a new key and request under a new grant.

## Q7. Waits

A call's wait for its answer is learned per (call kind, class) through `hex_core::patience::Patience` (decision 9 of
`runtime-architecture.md`), as every transport's is: this transport's own table (`client.quic.answer.<kind>` and
`client.quic.answer.<kind>.<class>`), measuring until a census over it records them, never another transport's
record. The transport's own liveness is QUIC's: a connection that hears nothing for its idle timeout closes, and every
call and program waiting on it is told why. So a dial or a connect with no deadline of its caller's
(`Endpoint::dial`, `link::connect`) waits as long as the transport does, the driver telling every dial its end (a
handshake nothing answers ends once the idle timeout passes with nothing from the peer, Q8), and the hello then waits
on its kind's learned wait. The tests' waits over QUIC are each for their own fact (`test.hex_quic.*`,
`test.hex_node.quic.*`, `test.hex_node.capacity.*`), measuring until a census over QUIC records them: the suites'
program-end record bounds a program's end alone (hex-bench's `test_waits`).

## Q8. Numbers

Each count is derived where the endpoint is made; none is picked.

| Number | Value | Derivation |
| --- | --- | --- |
| data frame bound | `stream::MAX_FRAME` (128 KiB + 4) | the longest unit any port's stream carries whole: one framed wire message (`wire::MAX_MESSAGE`, from the SEQPACKET record bound) |
| lane window | 1 | a lossless lane holds one frame past what QUIC took (hyper-transport `send_frame`), and the driver polls each frame as it comes; QUIC's stream window holds the rest |
| a connection's initial credit | 2 data frames | one frame its peer's owner reads while the next waits: a reader that keeps up never waits on credit, and one that stops holds at most these |
| connections, identities, pending handshakes (node) | the node's connections bound | the peers it serves as many as the connections its socket does (`serve::limits`) |
| connections (client) | 1 | a client's endpoint dials its one node |
| connections an identity holds | 2 | a connection and the one replacing it after its path changed |
| lanes a peer may open (node) | its calls in flight, its programs' streams and its gathering execs' (`programs * 3 + in_flight * 3`), one log connection, and its calls in flight again where it stages | every connection to a port that can do work at once for one peer (`hex_node::quic::shape`): a control connection past the calls the node runs has no call to run, and a second log reader reads the same lines |
| lanes a peer may open (client) | half its soft `RLIMIT_NOFILE` on Unix; on Windows half the dynamic port range's 16,384 (RFC 6335 §6, Windows' own since Vista) | each connection is bridged to a local socket pair, two descriptors on Unix, and on Windows a loopback TCP pair holding two ports of that range |
| exchanges, streams a connection | 1 | the least the endpoint accepts: Hex opens none, and a peer's head is refused at 0 bytes |
| window ceiling | `stream_window_ceiling()` | hyper-transport's: the largest window quinn's assembler can't be made to refuse |
| idle timeout | 3 initial probe timeouts | RFC 9000 §10.1's floor, from RFC 9002's initial RTT (333 ms, §6.2.2), its PTO (§6.2.1) and the default max_ack_delay (25 ms, RFC 9000 §18.2); QUIC raises it with the path's own PTO |
| keep-alive | 1 initial probe timeout | an idle connection stays up: three keep-alives fit in its idle timeout |
| responses queued | 1 | the driver polls transmits after each datagram (hyper-transport `Limits::max_responses`) |

A bound is paid for as it is used, never laid out ahead: the driver's command queue and hyper-transport's spare
buffers and receive chunks grow to their bounds (vendor change 3). hyper-quic does lay out one row for each lane a
peer may open as a connection starts (quinn's `StreamsState::new` inserts every remote stream up to the limit), so a
connected peer costs that table, a node's per peer and a client's for its one node.

## Q9. `hex node` over QUIC

**Chosen.** `hex node --socket <path> --state <dir> [--quic <address> --credentials <dir> [--staged <dir>]]` serves
its local socket and, with `--quic`, the same server over QUIC (Q4): its endpoint binds `<address>` and presents the
enrollment kept in `--credentials` (Q6), and it says `quic <address bound>` on stdout before its `ready <path>`, so a
port of 0 is told. The enrollment is read and checked whole before the node opens anything or binds: its key and its
chain's own certificate must match, the certificate must name the node, and the chain must verify under the roots for
both sides of a link; anything else is refused, typed, naming the file, with nothing bound (exit 1). `--quic` without
`--credentials`, or the other way round, is refused as a usage (exit 2). Without `--quic` no endpoint starts and no UDP
socket is bound. `--staged` is a Linux node's (Q4). `hex node enroll --name <name> --credentials <dir>` begins an
enrollment: a key made there and its request, whose path it says as `request <path>`.

**Alternatives.** Enrolling inside `hex node` at its start, through the seam: the node would need kiln's CA's address
and a grant on its command line, and a start would wait on the CA; the folder keeps the CA's answer, so a restart
needs no CA. Credentials in the environment: a key in the environment is readable by whatever reads the process's
environment, and the rule is that keys never travel.

**Falsified by** a configured `hex node` failing the suite over QUIC (HEX-QUIC-23), or one without `--quic` holding
a UDP socket, or one asked for QUIC without an enrollment it can serve with binding anything (HEX-QUIC-24).

## Q10. Admission by name

hyper-transport admits a connection twice (its `admission.rs`): a pending place while it handshakes, then, once its
certificate names a peer (`Directory::identify`), an entry for that peer's events and a charge to its identity, under
the endpoint's bounds. A connection `identify` names no peer for is refused `Refusal::Identity`, with no entry and
charged to no identity (its `Directory`; `endpoint.rs` `connected`). The capacity lane's keeper heard only the holders
its host names, but refused the others at the lane (capacity-view.md CV8): their connections were admitted first,
held as long as their peers stayed (QUIC's keep-alive, Q8), and counted against the lane's bounds, sized for its
holders (two connections each). Two peers kiln's CA enrolled but the lane doesn't name filled a one-holder lane: each
held one of its two peer entries (bounded by the larger of the identity and connection bounds, and kept while their
peers are connected), so the holder it names was refused `Refusal::Events` (red at 036bbabf); past that table, the
identity bound and then max-min's share (⌊2/3⌋ = 0; Bertsekas and Gallager, *Data Networks*, §6.5.2) refuse it alike.

**Chosen** (`hex_quic::Admits`, `Endpoint::start_admitting`). An endpoint that names its peers admits theirs alone: a
connection a peer opens is identified only where the name its certificate carries is listed (`Admits::Named`), and any
other is refused at its handshake as a peer kiln's CA didn't enroll is, so it holds a pending place for its handshake's
length, as every handshake does, and none of the endpoint's connections. The rule is the endpoint's from its first
datagram, set before its driver starts. A dial isn't filtered: this side names the peer it dials, and the handshake
checks the certificate for that name (Q5). `hex schedule serve`'s lane admits the holders its configuration names;
`hex node --quic` admits every peer kiln's CA enrolled (`Admits::Enrolled`): its peers, kiln's balancer and the
fleet's nodes, are named by no configuration. The lane keeps CV8 for a named holder that speaks as another (closed
`NotAuthorized`, logged). No number is added, and a peer's lanes are unchanged.

What each side sees. A TLS client's handshake completes before its server verifies its certificate (RFC 8446 §4.4.4,
RFC 9001 §4.1.2), so the refused peer is closed after its own handshake, with the refusal's code in the application
close (RFC 9000 §10.2): its dial ends `Refused::Identity`. The endpoint counts each refusal (hyper-transport's
`AdmissionStats::refused_identity`) and logs none: a line a handshake would let any enrolled peer write the host's log
at its handshake rate. Exporting the count is a metric, to agree with kiln's Observe owner (CLAUDE.md §10).

**Alternatives.**
- Refusing at the lane alone, as built before: the finding above.
- Closing an unnamed peer's connection once the lane hears it (`Endpoint::refuse`): it holds its place until its first
  word, and one that opens no connection to the port is never heard.
- A client-certificate verifier that checks the name in TLS (rustls' `ClientCertVerifier` over its
  `WebPkiClientVerifier`): refused with an alert, which hyper-transport reports as `Refusal::Identity` too, but the
  certificate's name would be read in a second place, and the Directory is where hyper-transport asks which peer a
  connection is.
- A larger connection bound for the lane: enough enrolled peers fill any finite bound.
- A refusal of its own for "enrolled, not admitted": a change to hyper-transport's wire refusals, which kiln and Hex
  hold at one revision (Q1). The peer's remedy is the same as an unenrolled certificate's (the person's
  configuration), so the existing typed refusal says what it can act on.

**Falsified by** a peer the endpoint doesn't name holding one of its connections, or a named holder refused for room
an unnamed peer took (`hex-node` `scheduler::capacity_lane_tests::a_peer_the_lane_doesnt_name_holds_none_of_its_connections`).

## Q11. A stream cut short is told as cut

A connection gone before its peer's End, by a reset or the link's end, is cut short: hex-quic's `Conn` reads the bytes
that came and then the error (`conn.rs` `take`). The bridge (Q3) gives its owner one end of a local socket pair, and a
socket pair carries no reset. Linux reports `ECONNRESET` to an AF_UNIX peer only when the closing socket's receive queue
still holds unread bytes (net/unix/af_unix.c, `unix_release_sock`), and XNU reports a disconnect as an end
(bsd/kern/uipc_usrreq.c, `unp_disconnect`). So the bridge ended its owner's socket as cleanly for a cut as for an End,
and a truncated stdout read as complete. Over vsock the kernel reports the reset, but hex-client's copy of a carried
stream swallowed read errors (`copy.rs`: "there is no one else to tell"), so a carried stream cut short read as complete
there too.

**Chosen** (route (b); ROOT, 2026-11-12). The bridge records the cut, the error its connection's read ended with, as it
ends the owner's socket. The owner's end that `link::Link` dials (`bridge::Bridged`) reads the bytes that came, then
that error (`ConnectionReset` for a reset, `ConnectionAborted` for the link's close), as a vsock socket reads a reset.
hex-client's own reads of a bridged stream (the control connection, a carried stream's copy, staging) see the cut so.
To follow in hex-client: the copy hands its outcome on instead of swallowing it, and the exec's `Proc` ends
`Refusal::Broken` naming the stream that was cut, for a carried stream and for an asked one, whose socket the caller reads
itself. `bridge::socket`, which hex-node's staging server reads, keeps a clean end.

**Alternatives.**
- Route (a): loopback TCP pairs on Unix too (Windows' adapter), closed with `SO_LINGER` at zero on a cut, so the reset
  (RFC 9293 §3.10.5's ABORT) reads as `ECONNRESET` after the bytes before it. It would also remove XNU's lost
  half-close wakeup (Shards M236: no TCP reader of 50,000 asleep). Rejected for its costs. Each open bridge holds two
  ports of the dynamic range (16,384, RFC 6335 §6). Each normal close leaves one side in TIME-WAIT for twice the MSL
  (RFC 9293 §3.6), 30 s on macOS (`net.inet.tcp.msl`, 15,000 ms; its ports 49152 to 65535), which caps a client at about 16,384 / 30 s, 546 closes a
  second, less the bridges open. A Unix socket pair holds two descriptors and no port.
- Closing the owner's socket with unread bytes, Linux's reset: Linux's alone, and only with bytes the bridge can't put
  there.
- A marker in the stream: the stream carries a program's bytes as they are, so no byte is free to mean "cut".

**Falsified by** a stream cut short whose owner reads a clean end (`tests::ac_t7_4_1_a_stream_cut_short_reads_as_cut`),
or, once hex-client's part lands, a program whose stream was cut short ending without `Refusal::Broken`.
