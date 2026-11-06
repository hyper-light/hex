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
  that starts the server is Shards' call, asked 2026-11-06). The node shares the roots it's configured with at the
  guest's boot, never the host's `/` by default: that would hand any guest process, a container's escape included,
  everything the node's user can write, and undo the VM boundary (the lead, 2026-11-06). The default is Docker
  Desktop's file-sharing list: on macOS `/Users`, `/Volumes`, `/private`, `/tmp` and `/var/folders`; on Windows the
  users folder and the temp folder; read-write, served as the node's own user, never root. Any other root is the
  operator's configuration (`HEX_GUEST_SHARES`). The OCI validation shares only its runtime-tools checkout
  (read-only) and its out directory (read-write), and its record names them (§6a): its `mounts` test binds `/etc`
  of the runtime's own host, which in the guest is the lab's own `/etc`, as on Linux; the guest's init mounts each as `virtiofs` at
  `/run/hex/shares/<i>`, as the kernel's command line names them (`hex.shares=rw,ro,…`, a mode per tag in
  order; HEX-GUEST-11) (the guest kernel has `CONFIG_VIRTIO_FS`, `resources/kernel/shards.config:19-24`); the host
  rewrites each bind mount's source to its path under the share that holds it, and a source under no share is
  refused by name (`Refusal::Platform`, the closest variant `hex_api` has, naming the mount and the shared roots). Shares are fixed at boot, as
  Shards attaches them then; a plain run has no hotplug of them.
- **The bundle and rootfs are copied into the guest before create**, over a third vsock port
  (`stream::FILES_PORT`): a bounded stream of entries (directories, regular files with their bytes, symlinks as
  their target text, each with its mode), which the relay unpacks under the guest's `/run/hex/staged/<box id>/`
  through `RESOLVE_IN_ROOT`, refusing an absolute or `..` name, a device node, a FIFO or a socket by name, and every
  count past its bound (entries, bytes, path length; CLAUDE.md §2). Its framing is a sans-IO core in
  `hex_core::stream` (CLAUDE.md §9), with hostile-input tests (§6). A container's root is the guest's own copy, as
  a Linux host's is its own disk: a root over virtio-fs would put every exec's page cache and the overlay's upper
  layer across the share. The host rewrites `root.path` (or `Rootfs`) to the copy; the copy is deleted with its box.
- **A root may be a tarball** (`.tar`, `.tar.gz` or `.tgz`; amended 2026-11-06): a bundle's `root.path`, a
  `Rootfs::Dir` or a layer naming one is staged from the archive's own entries, each with its Linux mode and a
  symbolic link's target text exactly as written. A Linux root unpacked into a Windows directory can't be staged
  faithfully: unpacking the OCI validation lab's Alpine root (§6a) with Git Bash's tar on jupiter turned its 322
  absolute links into Windows ones (`bin/ls` → `C:\Program Files\Git\usr\bin\busybox`, MSYS's translation of
  `/usr/bin/busybox`), and NTFS keeps no Unix modes at all, so the copy would hold broken links and guessed modes, a
  silent corruption (CLAUDE.md §3). So on Windows a directory root is refused by name (`Refusal::Platform`), pointing
  at the tarball form; on macOS and Linux, whose file systems keep both, a directory stays a root. OCI image layers
  are tarballs to begin with. The archive is read flat, one entry at a time, within the stage's bounds (entries,
  bytes, path length); an absolute name or a `..` component, a device, FIFO or socket entry, a corrupt or
  truncated archive (a gzip stream whose CRC or length doesn't match, an archive ending mid-entry) are refused by
  name. A hard link was refused by name too, since none of the lab's inputs holds one; that was falsified the same day
  by real images (below) and is superseded by what a tarball root carries. So was an entry before its directory,
  superseded by containerd's `mkparent` (below).
  - **The reader is the `tar` crate**, 0.4.46 from crates.io (alexcrichton's tar-rs), already a workspace
    dependency (`Cargo.toml:40`), used as it is rather than a reader of Hex's own; not vendored, since Hex changes
    nothing in it (CLAUDE.md "Vendoring"). Hex's checks wrap what it yields, and every hostile-input test runs
    against it. Hex reads it raw (`Entries::raw`, tar 0.4.46 `src/archive.rs:267`) and reads the extension
    records itself, each within a bound: the crate's own pass over them holds a GNU long-name, long-link or PAX
    record whole, however long (`src/archive.rs:418,429,440`, `read_all`), and leaves PAX `GNU.sparse.*`
    entries uninterpreted, so one would stage as a file holding its sparse map. A long name or link is read
    within the path bound, a PAX record within 64 KiB, and its `path` and `linkpath` taken with the crate's own
    parser (`tar::PaxExtensions`). Why not the crate's own pass (the lead's ruling, 2026-11-06): its `read_all`
    preallocates at most 128 KiB but then `read_to_end`s the record's whole declared size (`src/entry.rs:297-301`,
    `EntryFields::read_all`, called at `src/archive.rs:418,429,440`), so a few MB of gzip can make it buffer
    gigabytes. tar-rs leaves bounding the stream to its callers, so this is no bug to patch, vendor or report;
    Hex bounds it. A global PAX record (`g`) is read within the same 64 KiB and may set only keys staging doesn't
    carry, so can't get wrong: `atime`, `ctime`, `uname`, `gname`, `comment`, `charset`, `hdrcharset` (a global
    atime is left unapplied, as Go's archive/tar leaves it for containerd, never ctime (the kernel's), ignores owner names for numeric ids, and a root's texts are
    UTF-8 either way; `uid`, `gid` and `mtime` left the list when staging began carrying them, below); any other key (`path`, `linkpath`, `size`, sparse keys, one Hex doesn't know) would apply to
    every later entry, and is refused by name, never ignored (CLAUDE.md §3); a key on neither list is refused naming
    it, so a real archive that meets one shows exactly what to weigh before it is added. `git archive`'s tarballs,
    which begin with a global record holding only `comment=<commit>`, stage with every entry exact (a real one,
    git 2.56.0, is the test's fixture: `crates/hex-client/src/guest/stage/fixtures/git-archive.tar`). Refused by name: a PAX
    `size` (a file past a header's 8 GiB), sparse entries (GNU `S` and PAX `GNU.sparse.*`, which none of the
    roots it serves holds: `tar -tv`), and an extension record with no entry after it. An entry written through
    one of the archive's own symbolic links was refused too, until the lead's ruling of 2026-11-06 (below): it
    is now resolved within the root, as containerd does. The tests that pin this handling are
    `crates/hex-client/src/guest/stage/tar_tests.rs` and the relay's end-to-end tests in
    `crates/hex-node/src/relay/tests.rs` (HEX-GUEST-14, -15).
- **A tarball root carries what Linux's own unpack holds** (amended 2026-11-06, the lead's call): owners, times,
  extended attributes and hard links, not only a path, a mode and a link text. The evidence
  (`docs/research/runtime/shards-guest.md` §9, six public images pinned by digest): every image but alpine has hard
  links (debian's and ubuntu's perl-base ship `usr/bin/perl` and `perl5.x` as one inode; fedora:44 has 551 groups),
  grafana has 16,280 entries owned by 1000:1000 or 472:0, the shadow group owns `etc/shadow` in debian, ubuntu and
  alpine, and caddy's binary holds `security.capability`. Dropping them would make every non-root image's files
  root's and copies of hard links, and strip capabilities; refusing them would refuse nearly every image. So:
  - **The staging stream's entry gains, append-only** (its version 2, first released with this follow-up and its
    layers' kinds below; a version-1 stream reads as before): a numeric
    uid and gid (names ignored, as containerd does: the layers carry none), an mtime (seconds and nanoseconds), up to
    a bound of extended attributes (name, value), and a hard-link kind whose target is an earlier entry of the same
    root, a regular file or a link (below), named as a staged path is and refused if it names its way out, a later
    or missing entry, or a directory. Bounds (CLAUDE.md §2), from Linux's own: at most 64 xattrs an entry, a name at most 255 bytes
    (`XATTR_NAME_MAX`), a value at most 64 KiB (`XATTR_SIZE_MAX`) and all of an entry's at most 64 KiB
    (`XATTR_LIST_MAX`); a link target within the path bound.
  - **Where they come from:** the tar header's uid, gid and mtime, or PAX `uid`, `gid`, `mtime` past what a header
    holds; xattrs from PAX `SCHILY.xattr.*` and `LIBARCHIVE.xattr.*` (base64 there). Every xattr in a Linux namespace
    (`security.`, `system.`, `trusted.`, `user.`) is carried; one in no Linux namespace (`com.apple.*`, from macOS
    bsdtar) can't exist on Linux, so not carrying it loses nothing Linux's unpack would hold, and is stated, not
    refused. A global PAX record setting `uid`, `gid` or `mtime` would now change what's staged, so those leave the
    global record's allowed keys and are refused by name there.
  - **The relay applies them in Linux's order** for each entry, since a chown and a write each clear setuid, setgid and
    `security.capability`: contents written and closed, then the owner (`fchownat` with `AT_SYMLINK_NOFOLLOW`), the
    mode, the xattrs, and last the times (`utimensat` with `AT_SYMLINK_NOFOLLOW`, atime as below). A directory's are
    applied when its layer ends, deepest first, after everything in it, so writing its children doesn't move its
    mtime. A hard link is `linkat` from the target's directory, resolved as an entry's parent is (below). An xattr the guest refuses to set is refused by name. All through rustix's safe
    wrappers; no new `unsafe`.
  - **User namespaces:** the ids are staged as the image holds them, as Linux's unpack stores them; mapping them for a
    box in a user namespace is the runtime's, as on any Linux host.
  - **atime** is the later of the archive's atime and mtime, as containerd's unpack sets it (`pkg/archive/tar.go:335,
    445` at containerd v2.3.6, `ee273536`): the entry's metadata carries it beside mtime.
  - **A multi-layer image is its layers' own tarballs, applied in order into one staged root**, by containerd's
    rules (`applyNaive`, `pkg/archive/tar.go:161-340` at v2.3.6): a layer's entry replaces whatever was at its path,
    of any kind, except a directory onto a directory, which merges and takes the later header's owner, mode,
    xattrs and times (`:304-311`); `.wh.<name>` removes `<name>` (`:218-222`); `.wh..wh..opq` removes every entry of
    its directory this layer hasn't made (`:184-216`); a directory's mode and times are its latest header's, set
    when its layer ends, after everything in it (`:330-337`; a later layer that writes into it without a header
    of its own moves its mtime, there as here). The stream marks each layer's start, and carries a whiteout and an
    opaque marker as kinds of their own, never as files; one staging can't apply is refused by name. At most 500
    layers (`hex_core::bundle::MAX_LAYERS`); the relay's set of what this layer made is bounded by the stage's
    entries, and a removal by a counted walk. An `OciSpec`'s `Rootfs::Layers` of tarballs reaches the guest as
    `Rootfs::Layers` of the one staged root, under the box's own writable layer; a bundle's `root.path` is one path,
    so one tarball. The packer keeps the same state across layers, so a hard link can't name what a later layer
    removed or replaced.
  - **An entry's name resolves within its root, as containerd's does** (the lead's ruling, 2026-11-06): its
    parent through the root's own symbolic links, an absolute link or a `..` resolved inside the root
    (`fs.RootPath`, `:268-270`), and a hard link's target the same way (`hardlinkRootPath`, `:795-807`). The relay
    opens the parent with openat2's `RESOLVE_IN_ROOT` (without `RESOLVE_NO_SYMLINKS`; with `RESOLVE_NO_MAGICLINKS`
    and `RESOLVE_NO_XDEV`) on the layer's root, through rustix, the kernel holding the bound; a hard link's
    target must resolve to a regular file staged. So `bin` → `usr/bin` in one layer and `bin/tool` in the next
    lands at `usr/bin/tool`, and `a` → `/etc` then `a/x` lands at the root's `etc/x`, never the guest's. No
    surveyed image writes through a link (research §9: 0 in debian, ubuntu, alpine, fedora, caddy, grafana,
    python:3.12-slim, nginx and node:22-slim), but merged-usr layering may, and a refusal would refuse it.
    Outside a layer (a directory root's walk, a bundle's `config.json`) a name still resolves through no link,
    which a host walk never writes through.
  - **A parent no layer made is made as containerd makes it** (the lead's ruling, 2026-11-06): hand-made and
    Bazel layers, written straight from package archives, may omit parent directories, and containerd makes
    each (`mkparent`, `pkg/archive/tar.go:448-503`: `mkdir` 0755 then an explicit chmod past the umask,
    `tar_unix.go:90-97`; its attributes copied only from a lower layer that has the directory, which in one
    merged root is never the case for a missing one). The relay makes each missing component within the layer's
    root, 0755, owned by itself (root in the guest, as containerd's unpack runs) and timed when it's made, as
    containerd's are; such a directory's times are the wall clock's on both sides, so HEX-GUEST-21 leaves them
    out of its comparison. A name in the way that resolves nowhere (a link to nothing) is refused rather than
    made through.
  - **A hard link is another name for what its target resolves to, unfollowed**, as containerd's `link`
    (os.Link, linkat(2) without `AT_SYMLINK_FOLLOW`, `:386-394`): a regular file or a symbolic link itself. A
    directory, which link(2) refuses, and a target that resolves to nothing are refused, as containerd's unpack
    fails them.
  - **One difference from containerd, stated:** a literal absolute name or a `..` component, of an entry or a
    hard link's target, is refused by name, where containerd clamps it to the root (`filepath.Clean`, then
    `RootPath`): a layer a Docker or BuildKit build writes never holds one, and one that does is hostile or
    broken. Falsified the day a real image holds one, which HEX-GUEST-21 would show.
  - A directory root (a `Path` source) carries none of these: a host directory holds no Linux owners (and on Windows
    is refused, above). The test that holds it: HEX-GUEST-21, each image staged as a tarball root in a real guest,
    compared entry by entry with the same image unpacked as root on Linux.
- A non-bind mount (`proc`, `tmpfs`, …) names no host path and passes unchanged; hooks run in the guest's runtime
  namespace, so their paths are the guest's.

## 5. Where Shards has no backend

macOS on x86_64 and Windows on arm64 (`shards-guest.md` §4): the `oci` class is refused by name
(`Refusal::Platform`, naming the missing Shards backend) before anything is booted.

## 6. Done, as the step says

The `oci` class's conformance suite (`hex_api::conformance`) and the OCI runtime-spec validation
(`bench/oci-validation/`) pass in the guest on the Mac Studio and on jupiter, reached from the host's node through
the seam, and every gate passes.

### 6a. Where the OCI validation runs: in the guest, its lab made by the host's node (decided 2026-11-06)

runtime-tools (`opencontainers/runtime-tools` `bd3b0bd`) needs one Linux kernel under both its harness and the
runtime it tests:
- it drives the runtime's CLI (`create`, `start`, `state`, `kill`, `delete`: `validation/util/container.go:79,126,
  138,172,196`), giving the container a stdout file in the bundle (`:81-85`, read back at `:109-110`), and builds
  each bundle with `tar` (`validation/util/test.go:130`) around its `runtimetest`, which checks from inside
  (`:199`, `:276`);
- it reads the container's namespaces at `/proc/<state.pid>/ns` (`validation/linux_ns_itype/linux_ns_itype.go:83`,
  `validation/linux_ns_path/linux_ns_path.go:96`), and its cgroup files through `/proc/self/mountinfo`
  (`cgroups/cgroups.go:33-34`; `validation/util/linux_resources_memory.go:11,18` and the other `linux_cgroups_*`);
- it runs `unshare … --fork sleep` itself and hands the runtime `/proc/<pid>/ns/<x>` to join
  (`linux_ns_path.go:113,63,93`);
- its `mounts` test binds the runtime's host's `/etc` eight ways (`validation/mounts/mounts.go:38-72`).

Hex's Linux run (`bench/oci-validation/run_hex.sh:12-16`, `inside.sh:6-16`) runs it in a privileged container with
a cgroup namespace of its own, the suite's `RUNTIME` a shim to `hex oci`.

So in the guest it runs as on Linux, the docker container replaced by a Hex `oci` box, the **lab**, which the
host's node makes and drives through the seam (HEX-GUEST-20, `crates/hex-cli/tests/guest_validation.rs`): the host
boots the guest through `hex_runtime::guest`, creates the lab from a bundle (an Alpine userland with util-linux and
the guest's `hex`; every capability, no seccomp filter, its own pid, mount and cgroup namespaces, cgroup2 mounted
writable), staged as every root is (§4a), with the runtime-tools checkout for the guest's architecture (read-only)
and an out directory (read-write) bound through the run's two shares. The lab runs the unchanged `inside.sh`; every
call, stdio stream and exit crosses vsock; the host reads `results.tsv` from the out share and checks it against
the Linux record's pass/fail list (`bench/oci-validation/SUMMARY-hex.txt`). A test that passes on Linux and fails
in the guest is a failure to fix at its cause; the record's two deliberate refusals (`linux_mount_label`,
`linux_process_apparmor_profile`) stand as they are.

The other reading, the harness on the macOS or Windows host and a host-side `hex oci` forwarding to the guest,
can't run the suite unmodified: its binaries are Linux's, and the `/proc`, cgroup and `unshare` steps above exist
on no other OS. Answering them from the guest would rewrite runtime-tools, and then it would no longer be the OCI
runtime-spec validation.

Falsified if the suite needs the host's own files (a test reading a macOS or Windows path), which no test above
does.

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
- HEX-GUEST-12: as the kernel's first process (PID 1 with `argv[0]` `/init`; a container's PID 1 runs as itself),
  `hex` is the guest's init whatever words the kernel passes it, so the command line
  needs no `-- guest init` (Shards appends its own words, ` quiet` with no console reader,
  `crates/vmm/src/vm/mod.rs:1007-1018`), and as any other process it is not (`crates/hex-cli/src/guest.rs`).
- HEX-GUEST-8: the conformance suite, every case, on `oci` and `process` boxes, through `hex guest relay` and the
  guest client, Unix sockets standing in for vsock (`crates/hex-cli/tests/node_socket.rs`): passed on fedora,
  rootless, 2026-11-04 (unit `hex-native-s5-test-20261104T091420Z`, tree `2f93b9d`).

- HEX-GUEST-13: the staging stream (§4a): a stage's entries read back as written however the stream cuts them;
  hostile names (absolute, `..`, `.`, empty components, NUL, not UTF-8), kinds (a device's, a FIFO's, a socket's),
  modes and fields refused by type; every count past its bound refused from its head (`hex_core::stream::files`).
- HEX-GUEST-14: the relay's staging port: a staged tree lands under `<staged>/<box id>/` as it was, modes kept, and
  goes when removed; a stage that names its way out through a link it made, comes twice, carries a device's kind or
  is cut short is refused and leaves nothing (`hex_node::relay::files`).
- HEX-GUEST-15: the host's packer: a host tree packs parents first, files with their bytes; a device node or socket
  is refused by its path; the stage's bounds refused before anything past them is sent
  (`hex_client::guest::stage`).
- HEX-GUEST-16: the spec rewrite and the shares: shared roots read from `HEX_GUEST_SHARES` or this OS's default
  (Docker Desktop's list, never `/`); a bundle's `config.json` and an `OciSpec` rewritten to the staged root and
  each bind source's path under its share, writable binds passing; a source under no share refused by name, naming
  the shared roots (`hex_runtime::guest`).
- HEX-GUEST-17: the guest runtime: its inputs (`SHARDS_BIN`, `HEX_GUEST_KERNEL`, `HEX_GUEST_INITRD`,
  `HEX_GUEST_CPUS`, `HEX_GUEST_MEMORY_MIB`) read before any boot, every missing one named in one refusal;
  where Shards has no backend the class is refused as the platform's before
  anything boots; the guest's spec names its init, the channel's three ports and its shares (`hex_runtime::guest`).
- HEX-GUEST-18: the conformance suite on `oci` boxes through `hex_runtime::guest::WithGuest` over
  `shards_api::sim::SimGuests`, whose guest runs `hex guest relay` to a node agent: the real link, staging and
  rewrite on Linux (`crates/hex-cli/tests/node_socket.rs`).
- HEX-GUEST-19: the same suite through the real `shards_api::Shards` from the host's node, run where
  `HEX_GUEST_KERNEL`, `HEX_GUEST_INITRD` and `SHARDS_BIN` are set (`crates/hex-cli/tests/guest_real.rs`,
  ignored elsewhere).
- HEX-GUEST-20: the OCI runtime-spec validation in the guest (§6a): the host's node boots the guest, makes the lab
  and runs `inside.sh` in it through the seam; each test's result matches the Linux record's
  (`crates/hex-cli/tests/guest_validation.rs`, run where its inputs are set, skipped elsewhere).

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
