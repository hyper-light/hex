# A Linux guest that Shards boots: what Shards offers a host program today

Source notes for native plan step 5 (kiln plan `01a194669ce477718e3b3b8c7be44138`): Linux images on macOS and
Windows run in a Linux guest that Shards boots, Hex's Linux node inside it, reached over vsock. Hex reaches Shards
only through a Shards `-api` crate (CLAUDE.md §9). This note says what Shards has for that today, so the seam Hex
and Shards' owner agree (`docs/design/runtime.md`, "Linux images on macOS and Windows") can cite it.

Shards was read at `hyper-light/shards` `origin/integrate` `b3e5ca2784cb` (2026-11-03, its integration branch;
`main` at `9216e6db` lacks most crates named here), cloned to `~/.cache/hex-t7/native-s5/shards` on adacachy
(2026-11-04). `path:line` is at that revision; each quote is one line of its source, copied exactly.

## 1. No `-api` crate exists

No branch of `hyper-light/shards` has a crate named `*-api` (`git ls-tree` of `main`, `integrate`, `p/hotplug`,
`p/migrate`, 2026-11-04), and its docs name none: "-api" appears only as `claude-api` and `--no-api`. Shards' one
interface for other programs is Docker's Engine API (D170), which is image- and container-shaped and takes no
kernel or root file system (`crates/shards/src/cli/version.rs:23`:
"pub(crate) const ENGINE_API: (&str, &str) = ("1.56", "1.40");"). Its daemon's IPC is internal: the host and its
halves are kept together by build, not by version (`docs/design/architecture.md:1753`: "- **shards-init is inside
`shardsd`.** `shards-abi` has no version field, so a host and"). So the seam is new, and agreeing it with Shards'
owner is the first step (CLAUDE.md §9).

## 2. Booting a guest in process: `shards-vmm`

`shards-vmm` (`crates/vmm/Cargo.toml:3`: "name = "shards-vmm"", version 0.1.0) boots a machine from a `Config`
(`crates/vmm/src/vm/mod.rs:1094`):

- A kernel, an initramfs or an init to run as PID 1 from a generated one, a command line, vCPUs and memory in MiB,
  virtio-blk disks ("virtio-blk disks, in the order the guest enumerates them (vda, vdb, ...)."), read-only pmem
  files, virtio-fs shares and a vsock device (`:1094-1170`).
- `start` boots it and returns a handle and the running machine (`crates/vmm/src/vm/runtime.rs:812`:
  "pub fn start(cfg: &Config) -> Result<(Handle, Running), String> {"). The handle is `Clone` (`:44`) and stops it
  from any thread (`:432`: "pub fn stop(&self) {"); waiting for the running machine, or dropping it, stops it
  (`:715`: "/// The running machine, which it owns: waiting for it, or dropping it, stops it and tears").
- Its end is one of `PowerOff`, `Reset`, `Stopped`, `Snapshotted` or `Error(String)`
  (`crates/vmm/src/vm/mod.rs:2098-2110`).
- Bounds are checked before boot and refused, as strings: vCPUs (`crates/vmm/src/vm/runtime.rs:800-810`, "at least
  one vCPU is required" and the host's maximum) and memory ("guest memory must be an even number of MiB, at least
  {MIN_MEMORY_MIB}", `crates/vmm/src/vm/x86_64.rs:364`; 128 MiB on x86_64, 64 MiB on arm64,
  `crates/vmm/src/vm/aarch64.rs:29`).
- The machine has no "ready" event: a guest says it is up over vsock, as Shards' own daemon does (`kind::READY`,
  `crates/ipc/src/lib.rs:75`).
- Its fields carry Shards' own internal types (`join`, `io_limit`, `join_share` are `&'static` Shards objects) and
  its errors are `String`s, so it is not itself a seam Hex can hold to CLAUDE.md §3 (typed refusals): the `-api`
  crate wraps it.

## 3. The vsock channel

vsock is Shards' host↔guest control plane (D12, `docs/design/architecture.md:59`), on Firecracker's AF_UNIX
mapping (`crates/vmm/src/devices/virtio/vsock/muxer.rs:4`: "//! - A host client connects to the device's socket,
writes `CONNECT <port>\n`, and reads"; `:5`: "//!   `OK <host port>\n` once the guest accepts. Then the socket is
the stream."). A guest connection to host port P reaches the socket `<path>_P` (`:6`), or, for ports the host
process serves itself, arrives as one end of a socket pair with no socket file (`crates/vmm/src/vm/mod.rs:1944-1948`).
At most 1023 connections are open or pending at once (`muxer.rs:42`: "const MAX_CONNECTIONS: usize = 1023;").

On Windows these are real AF_UNIX stream sockets, not named pipes (`crates/vmm/src/platform/mod.rs:47`:
"pub use shards_afunix::{RawSocket, UnixListener, UnixStream, startup as winsock};"), so the protocol is the same
on every host. A vsock stream carries bytes only: no descriptors pass over it, unlike the node agent's
`SOCK_SEQPACKET` socket, which carries an exec's stdio as `SCM_RIGHTS` (`crates/hex-client/src/linux.rs:1-4`).

## 4. Backends

`docs/design/architecture.md:206-211` lists Shards' hosts: Hypervisor.framework on macOS arm64 ("booting Linux;
snapshots with cold and warm restore"), the Windows Hypervisor Platform on Windows x86_64 ("booting Linux: SMP,
virtio-blk, pmem and vsock; ..."), KVM on Linux; macOS x86_64 and Windows arm64 are "planned" (a `whp-arm64` branch
exists, 2026-11-03). The guest's architecture is the host's (`:202`: "the guest arch is always the host arch.").

## 5. The guest kernel has what the `oci` class needs

Shards pins Linux 6.18.48 by SHA-256 (`resources/kernel/README.md`, `scripts/build-kernel.sh:19`), built from
Firecracker's configs with Shards' fragments merged on top (`resources/kernel/shards.config` adds EROFS only).
`resources/kernel/firecracker-x86_64-6.18.config` sets every option goal 5's class rests on: `CONFIG_BPF_SYSCALL=y`
(`:118`), `CONFIG_CGROUPS=y` (`:197`), `CONFIG_MEMCG=y` (`:200`), `CONFIG_CGROUP_PIDS=y` (`:212`),
`CONFIG_CGROUP_DEVICE=y` (`:220`), `CONFIG_CGROUP_BPF=y` (`:223`), `CONFIG_USER_NS=y` (`:231`), `CONFIG_PID_NS=y`
and `CONFIG_NET_NS=y` (`:232-233`), `CONFIG_SECCOMP_FILTER=y` (`:773`), `CONFIG_OVERLAY_FS=y` (`:2876`); the
aarch64 config the same. `mount_setattr` and idmapped mounts are in every 6.x kernel with no option. The BPF JIT is
off (`:119`: "# CONFIG_BPF_JIT is not set"), so a device program runs interpreted: a cost to measure, not a refusal.

## 6. What would falsify the use made of this

- A Shards backend that boots Linux without vsock on a host Hex targets: then the channel needs another transport.
- A guest kernel lacking an option above: then the class is refused in the guest by the node's own checks, as on any
  Linux host.
- Shards' owner choosing a different seam (the daemon's protocol, or the Engine API): then this note's §2 is the
  implementation behind it, not the seam.

## 7. The default shared roots: Docker Desktop's file-sharing list

Checked 2026-11-06 for the guest's default shares (`docs/design/shards-guest.md` §4a, the lead's ruling that the
host's `/` is never shared by default).

- Docker Desktop's settings page, "File sharing"
  (<https://docs.docker.com/desktop/settings-and-maintenance/settings/>), quoted: "By default the `/Users`,
  `/Volumes`, `/private`, `/tmp` and `/var/folders` directory are shared." The page gives that list under the
  settings for Mac, Linux and Windows Hyper-V alike, and no Windows path of its own.
- The same page, "Resources": the memory limit "Defaults to 50% of your host's memory"; for CPUs it says only
  "Specify the maximum number of CPUs to be used by Docker Desktop", no default. Hex therefore guesses neither: the
  guest's vCPUs and memory are the node's configuration (`HEX_GUEST_CPUS`, `HEX_GUEST_MEMORY_MIB`), charged to
  its budget (§1 of the design).
- On Windows the page lists no Windows folder, so the Windows default (the users folder, read from its known
  folder `FOLDERID_UserProfiles`, and the temp folder) is the lead's ruling of 2026-11-06, the counterparts of
  `/Users` and `/tmp`, not a quote of Docker's.

What would falsify it: Docker Desktop changing its default list (then Hex's default follows it, or says why not), or
a host whose users or temporary folders are not where those calls say.

## 8. The tarball roots' tests can fail: a mutation check (2026-11-06)

The hostile-archive tests (`crates/hex-client/src/guest/stage/tar_tests.rs`, HEX-GUEST-15) were written before
`stage::Source::Tar` and first ran with it, never red. So each check they cover was disabled in turn, in a scratch copy
of `c230d0a` never committed, and its test run and seen to fail, then restored. On fedora, units
`hex-ns5route-mut-20261106T110202Z` and `hex-ns5route-mut2-20261106T110229Z`; the Windows refusal on jupiter.

| check disabled | test | failure |
|---|---|---|
| absolute and `..` names | `a_tarball_naming_its_way_out_or_out_of_order_is_refused_by_name` | `tar_tests.rs:126:9` (the refusal no longer says why) |
| device and FIFO entries staged as files | `a_tarball_with_a_device_fifo_hard_link_or_unknown_kind_is_refused_by_name` | `tar_tests.rs:70:36` (`unwrap_err` on `Ok`: packed) |
| hard links staged as files | the same test | `tar_tests.rs:70:36` |
| the gzip trailer not read | `a_lying_or_truncated_tarball_is_refused` | `tar_tests.rs:70:36` |
| the end-of-archive block not required | the same test | `tar_tests.rs:70:36` |
| a Windows directory root not refused | `a_tarball_root_stages_everywhere_and_a_directory_root_is_refused_on_windows` (`crates/hex-runtime/src/guest/tests.rs`), on jupiter | `tests.rs:457:13`, "expected a Windows directory root refused, got Ok(…)" |

Each test passed again with the check restored.

## 9. What real image layers hold that a tarball root must carry (2026-11-06)

`bench/native/tarball-census/census.py` over each layer of `docker save` output (Python's tarfile, which decodes PAX and GNU
records), run on fedora by `bench/native/tarball-census/run.sh`, unit `hex-ns5route-census-20261106T120325Z`. Images,
pinned by the digest each tag resolved to:

| image | digest | entries | hard links (groups) | non-root uid/gid | xattrs | setuid/setgid | distinct mtimes |
|---|---|---|---|---|---|---|---|
| debian:bookworm-slim | `sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc` | 4,226 | 2 (2) | 10 | 0 | 13 | 139 |
| ubuntu:24.04 | `sha256:fe81560990fae86dc4743202b2cb4d3df98d0bcf949173cecef15ce1127ef852` | 3,443 | 2 (2) | 18 | 0 | 14 | 230 |
| alpine:latest | `sha256:3f4d6b32e26703c6082839234f43769d5046b3ac7c910033911d1e71d8cd630a` | 515 | 0 | 1 | 0 | 0 | 11 |
| fedora:44 | `sha256:b3e011d47e686a89781495c9a87bc80349af742965e5d51e79cd51711982e4a2` | 9,033 | 1,281 (551) | 1 | 0 | 9 | 203 |
| caddy:latest | `sha256:ce773ec7a742ce15ee6bf2bb5f65882b9e2a4487b10733e6f1484f86e6b5d8bc` | 949 | 0 | 2 | 1 | 0 | 30 |
| grafana/grafana:latest | `sha256:488984b716c0862639f91a698adbe2862677a5a07bad42da5f03f3030883b205` | 17,883 | 254 (111) | 16,280 | 0 | 1 | 91 |

What each is, by example:
- Hard links: debian's and ubuntu's `usr/bin/perl` = `usr/bin/perl5.36.0` / `perl5.38.2` (perl-base) and `usr/bin/gunzip` =
  `usr/bin/uncompress`; fedora's rpm platform macros (`usr/lib/rpm/platform/{amd64,ia32e,x86_64}-linux/macros`, 551
  groups); grafana's (Alpine-based) `usr/share/zoneinfo` aliases (111 groups). Every hard link names an earlier entry of
  its own layer.
- Owners: the shadow group (gid 42) on `etc/shadow`, `etc/gshadow`, `usr/bin/chage`, `usr/sbin/unix_chkpwd` in debian,
  ubuntu, alpine; tty (5), mail (8, 12), staff (50) elsewhere; ubuntu's `home/ubuntu` 1000:1000; grafana's whole
  application 1000:1000 and its data 472:0; caddy's binary 1001:1001. Names (uname/gname) are empty in every image:
  the layers carry numeric ids only.
- Xattrs: one, caddy's `usr/bin/caddy` `security.capability` (20 bytes, `cap_net_bind_service=+ep`), as
  `SCHILY.xattr.security.capability`; no `LIBARCHIVE.xattr.*` in any.
- Times: every entry has a non-zero mtime (none 0), from 1996 (debian) to the build day; no layer has atime or ctime.
- None has a sparse entry (GNU `S` or `GNU.sparse.*`), a device node or FIFO, or a whiteout (each is a single-layer
  base or adds files only); the only PAX keys are `path` and `linkpath` (long names).

So owners, hard links, mtimes and capabilities are in nearly every image, and a staged root without them differs
from what Linux's own unpack holds: every non-root image's files would be root's, perl and fedora's macros would be
copies, and caddy would lose `cap_net_bind_service`.
