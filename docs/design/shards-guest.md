# Decision (proposed 2026-11-04): Linux images on macOS and Windows run in one Shards guest per node

Native plan step 5 (kiln plan `01a194669ce477718e3b3b8c7be44138`). Status: **proposed; the seam (§2) waits on
Shards' owner** (CLAUDE.md §9). Nothing here says Shards agreed until §2 cites its answer.

Sources: `docs/research/runtime/shards-guest.md` (what Shards offers, at `hyper-light/shards` `b3e5ca27`),
`docs/research/runtime/native-platforms.md` §1 and §4 (why a Linux image needs a Linux kernel; Shards' backends),
`docs/design/runtime.md` ("each OS runs its own boxes natively").

## 1. The shape

- A macOS or Windows node that is asked for an `oci` box boots **one** Linux guest through Shards, the first time,
  and runs Hex's Linux node agent inside it, unchanged (CLAUDE.md §8): goal 5's device programs, `mount_setattr`,
  the seed, the record and recovery are the same code that runs on a Linux host. Every later `oci` box on that node
  goes to the same guest.
  - Why one guest, not one per box (Apple's Containerization's shape, `native-platforms.md` §1): the step asks for
    Hex's Linux node in the guest, and a node per box would make each box pay a boot and a kernel's memory, and
    would split one node's bounds and record across many. Falsified if a guest's boot is under the `oci` class's
    own create-to-start time on the same host and per-box guests then cost less memory than one guest's slack:
    then a guest per box is the cheaper isolation.
- The guest's bounds are the node's: its vCPUs and memory are configured on the node, charged to the node's budget
  (CLAUDE.md §2), and each box inside is bounded by the guest's cgroups as on any Linux node. A box whose bounds
  exceed the guest's is refused by the guest node's own check, as on a Linux host whose machine is smaller.
- The host's `Runtime` for the `oci` class is a **client of the guest node**: the same `hex_api::Runtime` seam the
  host's `process` class answers, so the host's node routes by class and nothing above it knows a guest exists.

## 2. The seam to Shards (to agree with Shards' owner)

Shards has no `-api` crate today (`shards-guest.md` §1). Hex proposes the smallest one it needs, `shards-api`, owned
by Shards, with a simulator and a conformance suite both pass (CLAUDE.md §9), its real implementation over
`shards-vmm`'s `start`/`Handle::stop`/`Running` (`shards-guest.md` §2):

```rust
pub struct GuestSpec {
    pub kernel: PathBuf,
    pub init: Init,            // an initramfs, or an executable to run as PID 1 from a generated one
    pub cmdline: String,
    pub vcpus: u32,
    pub memory_mib: u64,
    pub disks: Vec<Disk>,      // { path, read_only }, in guest order
}
pub trait Guests: Send + Sync {
    /// What this host can boot, or why not: a refusal naming the missing backend (macOS x86_64, Windows arm64).
    fn backend(&self) -> Result<Backend, Refusal>;
    fn boot(&self, spec: &GuestSpec) -> Result<Box<dyn Guest>, Refusal>;
}
pub trait Guest: Send + Sync {
    /// A stream to the guest's vsock `port` (`CONNECT <port>`), within `patience`.
    fn connect(&self, port: u32, patience: Duration) -> Result<Stream, Refusal>;
    fn stop(&self) -> Result<(), Refusal>;
    fn wait_for(&self, timeout: Duration) -> Result<Option<End>, Refusal>;
}
```

`Refusal` is typed (no backend, bounds, boot, channel, ended), not `shards-vmm`'s `String`s (CLAUDE.md §3).
Hex takes `shards-api` by name and version, patched to its vendored copy (CLAUDE.md "Vendoring").

**Agreement:** to record here, citing Shards' answer (asked 2026-11-04 08:29Z, kiln agent
`01a148884a4770fdaf0111f3f792a248`).

## 3. The channel: the node's wire over a byte stream

The node agent's socket is `SOCK_SEQPACKET` and carries an exec's stdio as `SCM_RIGHTS` descriptors
(`crates/hex-client/src/linux.rs:1-4`); a vsock stream carries bytes only (`shards-guest.md` §3). So:

- **Control:** the same `hex_core::wire` messages, each framed on the stream by its length (a big-endian `u32`, at
  most the wire's `MAX_MESSAGE`; a longer one is a typed fault that closes the connection, never a partial read).
- **Stdio:** each exec's stdin, stdout and stderr ride a vsock connection of their own, which the host opens and
  names in its first 24 bytes (its control connection's token, the exec's request id and which stream;
  `hex_core::stream::name`). The relay sends each control connection its token first, as request ids are a
  connection's own. The node takes only a
  pipe's end as a program's stream (`crates/hex-node/src/serve.rs:1208`: "the right end of a pipe, as the
  seam's `Stdio` holds; anything else is refused by name"), so the relay makes a pipe per stream, passes its end
  to the node with the exec, and copies between the pipe and the vsock connection; on the host, the client
  copies between the caller's pipes and its connections. One bounded buffer per stream on each side.
- **In the guest**, a relay (`hex guest relay`) accepts the host's vsock connections and speaks the node's own
  socket, one node connection per host control connection, so the node, its wire and its disconnect rule stay
  as they are (CLAUDE.md §8). Its framing, stream names and gathering of an exec's streams are a sans-IO core
  (`hex_core::stream`, HEX-GUEST-1..3) driven by the relay and the host's client (CLAUDE.md §9). The guest
  listens on AF_VSOCK through `socket2`'s safe `SockAddr::vsock` (socket2 0.6, feature `all`; rustix 1.1 has
  the family but no address type), so no `unsafe` is added.
- **Bounds:** at most `wire::Limits`' calls in flight, as the local client; at most three stdio streams per exec;
  stdio connections that never name an exec within the answer patience are closed with a typed refusal logged on
  `hex.guest` (`event = "stdio_unnamed"`). Shards' muxer holds at most 1023 connections (`muxer.rs:42`), so a
  node's concurrent execs are bounded below that, and the next is refused by name.

## 4. The guest image

- Kernel: Shards' pinned 6.18.48 (`shards-guest.md` §5), which has every option the class needs.
- PID 1: Hex's own static `hex` binary as the generated initramfs's init (`GuestSpec::init`), which mounts `/proc`,
  `/sys`, cgroup2, `devtmpfs` and a tmpfs `/run`, and starts the node agent and the relay.
- A disk (ext4) for the node's record and image store, so a guest's boxes survive a relay or node restart as on a
  Linux host.
- Its build is a script in `scripts/`, its outputs' SHA-256 recorded in `bench/native/guest/`.

## 5. Where Shards has no backend

macOS on x86_64 and Windows on arm64 (`shards-guest.md` §4): the `oci` class is refused by name
(`Refusal::Platform`, naming the missing Shards backend) before anything is booted.

## 6. Done, as the step says

The `oci` class's conformance suite (`hex_api::conformance`) and the OCI runtime-spec validation
(`bench/oci-validation/`) pass in the guest on the Mac Studio and on jupiter, reached from the host's node through
the seam, and every gate passes.

Hex's tests of the channel, each citing its ID:
- HEX-GUEST-1: the wire framed by length over a stream, refused from its prefix past its bound; an exec whose stdio
  travels apart (`hex_core::stream`, `hex_core::wire::read_request_apart`).
- HEX-GUEST-2: a stdio connection's name, and hostile names refused.
- HEX-GUEST-3: an exec's streams gathered in either order, per connection, within their bound and patience.
- HEX-GUEST-4..5: the guest client's calls, stdio and end over a stream; an agent's bad bytes refused
  (`hex_client::guest`).
- HEX-GUEST-6..7: the relay to the node's own socket; its bounds refused by type (`hex_node::relay`).
- HEX-GUEST-8: the conformance suite, every case, on `oci` and `process` boxes, through `hex guest relay` and the
  guest client, Unix sockets standing in for vsock (`crates/hex-cli/tests/node_socket.rs`): passed on fedora,
  rootless, 2026-11-04 (unit `hex-native-s5-test-20261104T091420Z`, tree `2f93b9d`).

## 7. What would falsify it

- Shards' owner choosing another seam (§2): the routing (§1) and channel (§3) stand; the implementation changes.
- A copy per stdio byte in the relay measurably slowing a program's output against a Linux host's: then the relay
  splices (`splice(2)`) between the socket and the pipe instead of copying.
- The guest's interpreted BPF (`CONFIG_BPF_JIT` unset) making a device check measurably slower than the bar: then
  the guest kernel adds the JIT, with Shards.

## Appendix A: the draft `shards-api`, ready to apply (not agreed)

Drafted by Hex for Shards' owner to review, change or replace (CLAUDE.md §9). Nothing here is built against
until the seam is agreed, or Ada waives that (asked through the lead, 2026-11-04 09:16Z).

**Where it lives.** A crate `shards-api` at `crates/api` in Shards' repository, owned by Shards. Hex takes it by
name and version (`shards-api = "0.1"`), patched in Hex's `[patch.crates-io]` to its vendored copy.

**The trait** (its real implementation in Shards, over `shards-vmm`'s `start`, `Handle::stop`, `Running::wait` and
`VsockHost`, `shards-guest.md` §2-§3):

```rust
pub struct GuestSpec {
    pub kernel: PathBuf,
    pub init: Init,
    pub cmdline: String,
    pub vcpus: u32,
    pub memory_mib: u64,
    pub disks: Vec<Disk>,
}
pub enum Init { Initrd(PathBuf), Executable(PathBuf) }      // shards-vmm's `initrd` / `init`
pub struct Disk { pub path: PathBuf, pub read_only: bool }
pub enum Backend { Kvm, HypervisorFramework, WindowsHypervisorPlatform }
pub enum End { PowerOff, Reset, Stopped, Failed(String) }  // shards-vmm's ExitReason, less Snapshotted

#[derive(thiserror::Error)]
pub enum Refusal {
    NoBackend { os: &'static str, arch: &'static str },    // macOS x86_64, Windows arm64 today
    Bounds { what: &'static str, why: String },            // vCPUs, memory (shards-vmm's checks)
    Boot(String),                                          // the kernel, the init, a disk
    Channel { port: u32, why: String },                    // CONNECT <port> refused or unanswered
    Ended(End),                                            // the guest is gone
}

pub trait Stream: Read + Write + Send + Sync {
    fn try_clone(&self) -> io::Result<Box<dyn Stream>>;
    fn shutdown(&self, how: Shutdown) -> io::Result<()>;
    fn set_read_timeout(&self, t: Option<Duration>) -> io::Result<()>;
    fn set_write_timeout(&self, t: Option<Duration>) -> io::Result<()>;
}

pub trait Guests: Send + Sync {
    /// What this host boots with, or the refusal naming the backend it lacks.
    fn backend(&self) -> Result<Backend, Refusal>;
    /// Boots a guest; refused before boot past the host's bounds.
    fn boot(&self, spec: &GuestSpec) -> Result<Arc<dyn Guest>, Refusal>;
}

pub trait Guest: Send + Sync {
    /// A stream to the guest's vsock `port` (`CONNECT <port>`, Firecracker's mapping), within `patience`.
    fn connect(&self, port: u32, patience: Duration) -> Result<Box<dyn Stream>, Refusal>;
    /// Stops it: its `wait_for` then returns `Stopped`.
    fn stop(&self) -> Result<(), Refusal>;
    /// Its end, once it has come, waiting at most `timeout`.
    fn wait_for(&self, timeout: Duration) -> Result<Option<End>, Refusal>;
}
```

**The simulator** (`shards_api::sim`): `SimGuests` boots a `SimGuest` whose "guest" is a closure the test gives,
run on a thread, serving vsock ports over in-process socket pairs (`UnixStream::pair` on Unix, `shards_afunix`'s on
Windows): `connect(port)` hands the closure the other end, refusing an unserved port as `Channel`; `stop` and the
closure's return end it. Its bounds mirror `shards-vmm`'s (vCPUs ≥ 1 and ≤ the host's; memory even, ≥ 64 MiB on
arm64, 128 MiB on x86_64), so the same refusals come from both.

**The conformance suite** (`shards_api::conformance`), run against the simulator in `shards-api`'s tests and
against the real implementation on each backend: a spec past the bounds is refused before boot; a booted guest's
served port answers bytes both ways and a half-close reaches the other side (D12's half-close); an unserved port is
refused as `Channel` within its patience; `stop` ends it as `Stopped` and every later `connect` is refused
`Ended`; a guest that powers off ends as `PowerOff`; and `backend()` on macOS x86_64 or Windows arm64 is
`NoBackend` naming them.

**Hex's side**: `hex_client::guest::Link` over a `Guest` (`connect(CONTROL_PORT | STDIO_PORT)`), and the host's
`oci` class booting one guest per node (§1) and routing to `hex_client::guest::Client`.

**Vendoring Shards** (checked 2026-11-04, `scripts/vendor.py` and Shards `b3e5ca27`):
- `vendor.py` takes a `kind = "git"` project at a full commit, re-applies `vendor/patches/shards/*.patch` in a
  scratch git (`Scratch.apply`), so a patch may add a new crate's files; `check` proves the tree is upstream plus
  those, each a row of `vendor/shards/UPSTREAM.md`. A draft `crates/api` would be recorded so until Shards lands it.
- Shards is 175 MB in 5,391 files and holds a symbolic link (`scripts/frontend/agentfile-cases/symlink/agent/escape`),
  which `vendor.py` refuses unless excluded. `shards-vmm` needs, of Shards' own crates, `abi`, `afunix`, `ipc`,
  `netring`, `ntfs`, `syscall-filter` and `testdir`; everything else (its docs, `resources`, `vendor/aws-lc-sys`,
  the other crates) goes in the manifest's `exclude`.
- Shards' root `Cargo.toml` lists every crate as a member (`:3`) and excludes `vendor` (`:49`); with the rest
  excluded, a recorded patch trims its members to those kept, and Hex's workspace excludes `vendor/shards`.
- On macOS the process that calls Hypervisor.framework must be signed with the hypervisor entitlement (Shards signs
  `shards-vm` so, `docs/design/architecture.md:1979`); the host's `hex node` is signed with it, ad hoc on the Mac
  Studio.
