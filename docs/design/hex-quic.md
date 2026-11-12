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

Two recorded changes (`vendor/hyper-raft/UPSTREAM.md`):
1. hyper-transport takes hyper-quic with `default-features = false`. hyper-quic's default `qlog` feature turns on
   serde_json's `preserve_order` for every crate of a build (hyper-quic `VENDORED.md` §20); Hex writes JSON
   documents (OCI configs, records, HCS documents) whose key order would change. Shards takes hyper-quic without it
   for the same reason (its `crates/remote/Cargo.toml`). hyper-transport names no qlog type.
2. `Endpoint::negotiated(peer)`: the suite and group a connection negotiated, read from hyper-quic's handshake data,
   so each end of a link reports and a test asserts `TLS13_AES_256_GCM_SHA384` (Q5).

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
(`hex_core::stream::Gather`) and become the program's pipes, copied to and from their lanes. Staging connections are
served by the relay's staging server (`hex-node` `relay/files.rs`, Linux). The log port carries the node's log lines,
each counted where a slow reader makes it let one go.

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
through a seam (CLAUDE.md §9): a trait, a simulator CA and a conformance suite both pass (checkpoint 4).

## Q7. Waits

A call's wait for its answer is learned per (call kind, class) through `hex_core::patience::Patience` (decision 9 of
`runtime-architecture.md`), as every transport's is: this transport's own table (`client.quic.answer.<kind>` and
`client.quic.answer.<kind>.<class>`), measuring until a census over it records them, never another transport's
record. The transport's own liveness is QUIC's: a connection that hears nothing for its idle timeout closes, and every
call and program waiting on it is told why.

## Q8. Numbers

Each count is derived where the endpoint is made; none is picked.

| Number | Value | Derivation |
| --- | --- | --- |
| data frame bound | `stream::MAX_FRAME` (128 KiB + 4) | the longest unit any port's stream carries whole: one framed wire message (`wire::MAX_MESSAGE`, from the SEQPACKET record bound) |
| lane window | 1 | a lossless lane holds one frame past what QUIC took (hyper-transport `send_frame`), and the driver polls each frame as it comes; QUIC's stream window holds the rest |
| a connection's initial credit | 2 data frames | one frame its peer's owner reads while the next waits: a reader that keeps up never waits on credit, and one that stops holds at most these |
| connections, identities, pending handshakes (node) | the node's connections bound | each connection holds one of the node's descriptors (`serve::limits`) |
| connections (client) | 1 | a client's endpoint dials its one node |
| connections an identity holds | 2 | a connection and the one replacing it after its path changed |
| lanes a peer may open | its connections' bound | every connection holds a lane each way, and lane 0 beside them |
| exchanges, streams a connection | 1 | the least the endpoint accepts: Hex opens none, and a peer's head is refused at 0 bytes |
| window ceiling | `stream_window_ceiling()` | hyper-transport's: the largest window quinn's assembler can't be made to refuse |
| idle timeout | 3 initial probe timeouts | RFC 9000 §10.1's floor, from RFC 9002's initial RTT (333 ms, §6.2.2), its PTO (§6.2.1) and the default max_ack_delay (25 ms, RFC 9000 §18.2); QUIC raises it with the path's own PTO |
| keep-alive | 1 initial probe timeout | an idle connection stays up: three keep-alives fit in its idle timeout |
| responses queued | 1 | the driver polls transmits after each datagram (hyper-transport `Limits::max_responses`) |
