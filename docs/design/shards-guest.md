# Decision (proposed 2026-11-04): Linux images on macOS and Windows run in one Shards guest per node

Native plan step 5 (kiln plan `01a194669ce477718e3b3b8c7be44138`). Status: **proposed; the seam (§2) agreed with
Shards' owner 2026-11-04; §4 amended and §4a added 2026-11-06 (bind mounts through shares, the lead's steer)** (CLAUDE.md §9), its crate to be drafted by Hex as a pull request Shards reviews.

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

**Agreement** (Shards' lead, kiln agent `01a148884a4770fdaf0111f3f792a248`, answering on 2026-11-04, checked
against `hyper-light/shards` `integrate` `b3e5ca27`):
- No `-api` crate exists; what exists is a process interface: `shards run --kernel K [--init I] [--disk PATH[:ro]]…
  [--vsock PATH]` (`crates/shards/src/vm_run.rs:355`: `"--kernel" => kernel = Some(...)`, `:216` `--vsock`,
  `:359-365` `--cpus` and `--memory`). The guest's vsock is a host Unix socket at the `--vsock` path, Firecracker's
  `CONNECT <port>` handshake. Lifecycle is the process: it ends when the guest powers off or is killed, its exit
  code the guest's; bounds are refused before boot with a message and a non-zero exit; accounting is the OS's for
  that process.
- Backends: macOS arm64 (HVF), Linux x86_64 and arm64 (KVM) and Windows x86_64 (WHP, AF_UNIX there too) boot Linux
  with vsock today; Windows arm64 is in progress (Shards' plan step 10); macOS x86_64 is out of scope, refused by
  name.
- **Constraint:** Shards is one binary, and on macOS its VM process runs in App Sandbox with the hypervisor
  entitlement under Shards' own signing, so **Hex must not link Shards' VMM**. So `shards-api` is a thin client of
  the `shards` binary: `boot` spawns `shards run --kernel … --vsock <private path>`, `connect` does the vsock
  handshake on that socket, `wait_for`/`stop` are the process's (stop: SIGTERM, then kill at the deadline).
- Shards owns `crates/api` (`shards-api`): that client, `SimGuests` (loopback streams) and the conformance suite,
  run against both (the real one on Shards CI's KVM, HVF and WHP runners).
- The trait shape is accepted with two changes: the root file system is `disks: Vec<Disk { path, read_only }>`, and
  the vsock ports a guest may be reached on are an allowlist in the spec, which the client enforces.
- Hex drafts it as a pull request to `hyper-light/shards`, branched off `gpu` once the current integration lands
  (Shards' lead says when); it lands under Shards' rules (no panics, its 8-target lint, real-VM tests).

So the host's `hex node` needs no hypervisor entitlement of its own: it starts Shards' signed binary.

Two details, from Shards' lead's second answer (2026-11-04):
- **Which binary:** a path in the spec (or the environment), else `shards` on `PATH`. On macOS it is a signed Shards
  build as shipped, run where it is installed and never copied, since the VM process it carries holds the
  entitlement.
- **Readiness is over vsock only:** stdout and stderr are diagnostics, never a protocol. The guest is ready when a
  `CONNECT` to the relay's control port gets `OK`. Until the guest listens, the handshake is refused or closed, so
  the client retries with backoff up to the boot deadline (a typed `Boot` refusal past it), and the process ending
  first is a boot failure carrying its exit code.

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
  `/sys`, cgroup2, `devtmpfs` and a tmpfs `/run`, and starts the node agent and the relay. It first moves
  itself off the initramfs onto a tmpfs root (`MS_MOVE` over `/`, then `chroot`, as switch_root does), since
  each container's start pivots its root and pivot_root(2) refuses to leave the initramfs ("EINVAL The
  current root directory is on the rootfs (initial ramfs) filesystem", pivot_root(2), ERRORS).
- No disk (amended 2026-11-06): the node's record, its runtime directory and the staged bundles (§4a) live on the
  guest's tmpfs, charged to the guest's memory, which the node's budget holds (§1). What the record is for is a
  node's or relay's restart, which tmpfs survives; a guest's own reboot ends every box in it, so a disk would keep
  only records of boxes that no longer exist. Falsified if a guest must keep boxes across its own reboot (it
  can't: their processes are gone) or if staged images outgrow the guest's memory budget: then a disk, as first
  written, holds the store.
- Its build is `scripts/guest/build.sh`, its outputs' SHA-256 recorded in `bench/native/guest/<arch>/RECORD.md`.
  Two paths, both recorded there:
  - **From source** (build.sh): the kernel by Shards' own `scripts/build-kernel.sh` in Shards' pinned builder,
    checked against `scripts/guest/kernel-needs.config`; `hex` static against glibc, the runtime's choice for the
    `oci` class's start (static glibc spawned 0.22 ms against 0.34 ms dynamic, and musl gained nothing over it).
  - **The bring-up's** (2026-11-06, while no Linux host could build): the kernel is Shards' release of the same
    pinned build (`crates/shards/src/kernel.rs:21-61` pins each URL and SHA-256), its released config checked
    against the same list; `hex` is static **musl**, linked by the `rust-lld` Rust ships, built on the Mac Studio
    as Shards builds its own Linux guests there (Shards' `.cargo/config.toml:21-27`), so no cross C toolchain is
    needed. This departs from static glibc: musl's allocator and its thread and process start are the unknowns
    the runtime chose glibc to avoid. Falsified, and replaced by the from-source glibc build, if the guest's
    `oci` create-to-start or exec measures slower with the musl `hex` than with a static-glibc `hex` on the same
    guest kernel.

## 4a. Host paths: the bundle and rootfs copied in, bind mounts through Shards' shares

A box's spec names host paths: a bundle's directory, its `root.path`, a `Rootfs::Dir` or its layers, and each
bind mount's source. The guest sees none of them by itself. (Amended 2026-11-06 on the lead's steer: a first
draft refused writable bind mounts, which fails the OCI runtime-spec validation's `mounts` test, which binds
host `/etc` writable eight ways, bind and rbind under each propagation (runtime-tools
`validation/mounts/mounts.go:38-72`).)

- **Bind mounts go through Shards' shares, live.** Shards' share device is virtio-fs, tagged `shards{i}`
  (`crates/vmm/src/devices/virtio/fs/mod.rs:1-6`, `crates/vmm/src/vm/mod.rs:835`, at `90de623f`), writes going
  both ways on all three host OSes (D38, `docs/design/architecture.md:2774-2853`; Windows D197, `:7482-7518`), and
  a plain `shards run` takes a share another process serves (`--slates-fs-fd N`, `crates/shards/src/vm_run.rs:390-401`).
  So `shards-api`'s `GuestSpec` carries `shares: Vec<Share { dir, read_only }>`, served from boot (the interface
  that starts the server is Shards' call, asked 2026-11-06). The node shares the roots it's configured with
  (default `/`, writable) at the guest's boot; the guest's init mounts each as `virtiofs` at
  `/run/hex/shares/<i>`, as the kernel's command line names them (`hex.shares=rw,ro,…`, a mode per tag in
  order; HEX-GUEST-11) (the guest kernel has `CONFIG_VIRTIO_FS`, `resources/kernel/shards.config:19-24`); the host
  rewrites each bind mount's source to its path under the share that holds it, and a source under no share is
  refused by name (`Refusal::Unsupported`, naming the mount and the shared roots). Shares are fixed at boot, as
  Shards attaches them then; a plain run has no hotplug of them.
- **The bundle and rootfs are copied into the guest before create**, over a third vsock port
  (`stream::FILES_PORT`): a bounded stream of entries (directories, regular files with their bytes, symlinks as
  their target text, each with its mode), which the relay unpacks under the guest's `/run/hex/staged/<box id>/`
  through `RESOLVE_IN_ROOT`, refusing an absolute or `..` name, a device node, a FIFO or a socket by name, and every
  count past its bound (entries, bytes, path length; CLAUDE.md §2). Its framing is a sans-IO core in
  `hex_core::stream` (CLAUDE.md §9), with hostile-input tests (§6). A container's root is the guest's own copy, as
  a Linux host's is its own disk: a root over virtio-fs would put every exec's page cache and the overlay's upper
  layer across the share. The host rewrites `root.path` (or `Rootfs`) to the copy; the copy is deleted with its box.
- A non-bind mount (`proc`, `tmpfs`, …) names no host path and passes unchanged; hooks run in the guest's runtime
  namespace, so their paths are the guest's.

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
- HEX-GUEST-9: `hex guest init` mounts §4's file systems, keeps the node's state on the tmpfs `/run`, and
  takes no disk (`crates/hex-cli/src/guest.rs`).
- HEX-GUEST-10: `hex guest init` reads the root's file system from mountinfo, so it leaves the initramfs
  once; hostile lines name no root (`crates/hex-cli/src/guest.rs`).
- HEX-GUEST-11: `hex guest init` reads the shares from the kernel's command line, `hex.shares=` and `rw` or
  `ro` per share in tag order (`hex.shares=rw,ro`: `shards0` writable, `shards1` read-only), and mounts each as
  `virtiofs` at `/run/hex/shares/<i>`; a bad mode, the parameter twice or more than 32 shares are refused by
  name (`crates/hex-cli/src/guest.rs`).
- HEX-GUEST-8: the conformance suite, every case, on `oci` and `process` boxes, through `hex guest relay` and the
  guest client, Unix sockets standing in for vsock (`crates/hex-cli/tests/node_socket.rs`): passed on fedora,
  rootless, 2026-11-04 (unit `hex-native-s5-test-20261104T091420Z`, tree `2f93b9d`).

## 7. What would falsify it

- A create's staging copy (§4a) costing more than the `oci` class's own create on the same image, measured in the
  guest: then the host keeps a content-addressed store of staged layers in the guest and sends only what's new.
- Shards' owner choosing another seam (§2): the routing (§1) and channel (§3) stand; the implementation changes.
- A copy per stdio byte in the relay measurably slowing a program's output against a Linux host's: then the relay
  splices (`splice(2)`) between the socket and the pipe instead of copying.
- The guest's interpreted BPF (`CONFIG_BPF_JIT` unset) making a device check measurably slower than the bar: then
  the guest kernel adds the JIT, with Shards.

## Appendix A: the draft `shards-api`, ready to apply (not agreed)

Drafted by Hex; the shape is agreed with Shards' owner (§2), and the crate lands in Shards through its review.

**Where it lives.** A crate `shards-api` at `crates/api` in Shards' repository, owned by Shards. Hex takes it by
name and version (`shards-api = "0.1"`), patched in Hex's `[patch.crates-io]` to its vendored copy.

**The trait** (its real implementation a client of the `shards` binary, as agreed in §2; not `shards-vmm`
linked in):

```rust
pub struct GuestSpec {
    pub kernel: PathBuf,
    pub init: Init,
    pub cmdline: String,
    pub vcpus: u32,
    pub memory_mib: u64,
    pub disks: Vec<Disk>,
    /// The guest's vsock ports the host may connect to; any other is refused as `Channel`.
    pub ports: Vec<u32>,
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

**On Windows the stream must be Shards' overlapped AF_UNIX socket.** Hex's guest client reads on one thread while
others write on the same connection, and a Winsock socket made without `WSA_FLAG_OVERLAPPED` (`socket2` 0.6's,
`src/sys/windows.rs:253-265`) serializes them (Shards `crates/afunix/src/lib.rs:63`: "same connection, by any of
its handles, 2.9-3.9 s in M207's runs, where an overlapped"). A receive timed out by `SO_RCVTIMEO` leaves the
connection indeterminate (`:212`: "(PM M207): Winsock leaves a connection whose receive timed out in an
indeterminate"), so timeouts poll first. Shards' own `shards_afunix::UnixStream` does both, so the real
`Guest::connect` on Windows returns one behind `Stream`, whose timeouts are poll-based on every platform. Hex never
names the type: it wraps the `Stream` as a `hex_client::guest::Duplex`. Found by Hex's Windows agent (kiln agent
`01a1a9972b9970c8b057b99dbf6df870`), checked at `b3e5ca27`.

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
  `shards-vm` so, `docs/design/architecture.md:1979`). As agreed (§2), that process is Shards' own binary, so Hex
  vendors only `crates/api`'s client and needs no entitlement.
