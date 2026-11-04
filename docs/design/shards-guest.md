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
  names in its first frame (the exec's id and which stream). In the guest, that connection's socket **is** the
  descriptor passed to the unchanged node over its own socket: the program reads and writes the vsock socket
  directly, so the guest copies no stdio bytes. On the host, the client copies between the caller's pipes and the
  stream, one bounded buffer per stream.
- **In the guest**, a relay (`hex guest relay`) accepts the host's vsock connections and speaks the node's own
  socket, so the node and its wire stay as they are (CLAUDE.md §8). It is a sans-IO core (the framing and the
  stream naming) driven by a shell (CLAUDE.md §9).
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

## 7. What would falsify it

- Shards' owner choosing another seam (§2): the routing (§1) and channel (§3) stand; the implementation changes.
- A vsock socket refused as a program's stdio in the guest (§3): then the relay pumps bytes through pipes instead.
- The guest's interpreted BPF (`CONFIG_BPF_JIT` unset) making a device check measurably slower than the bar: then
  the guest kernel adds the JIT, with Shards.
