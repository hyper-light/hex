# Decision (2026-11-13): Hex's node under shards-init, with Hex's own PID 1 kept

Joint plan step 39 (kiln plan `01a1cd8e862e7169a074db3d5fa33f3d` version 12, "Hex's node under shards-init, with
Hex's own PID 1 kept"), AC-T7.4.1. Status: **decided; Hex's side built**. Built: init under shards-init (§2, §3;
hex-cli `guest.rs`'s `Host` and `duties`), and the node image and its build (§4). The host's boot through
shards-init stops at its channel: Shards' step 11 has no lane to a nested workload's socket, so the node refuses that
form by type until it does (§5, `Awaits(NestedChannel)`). The real-VM runs and the measurements are ROOT's (§9).

The person, 2026-11-13: "we are not getting rid of Hex's PID1 which lets it work with basically anything non-Shards.
We are to have init work *with* that." So Hex's PID-1 path (§8) is permanent, and this step adds a second host for the
same init: shards-init.

Sources, read at the revisions named:
- Shards `docs/design/architecture.md` at `7a4ef5c0` (Shards main, 2026-11-13):
  - D234, the nesting grant (step 38), and its record `docs/research/nesting-grant-decision-v1.md` (findings f1 to
    f39, cited `D234 fN`);
  - D231, an image booted for an orchestrator;
  - D238, agent control: step 10's decision, at the host, and step 11's endpoint;
  - D240 and D241 (the room), D188, D215 (the direct boot), and D233.
- Shards' code at `66113379`, the revision Hex vendors:
  - `crates/init/src/run.rs`:
    - the run's root, "The image on virtio-pmem becomes the root filesystem under a tmpfs overlay";
    - `WORKLOAD_CGROUP` and its `LEAF`, `init`;
    - `Inherited::env`, "The nesting grant's environment (D234), `NOTIFY_SOCKET` and `SHARDS_READY_BY`, over what
      the run or an exec gives its own".
  - `crates/init/src/readiness.rs`: `NOTIFY_SOCKET` is `/run/shards/notify`, and `SHARDS_READY_BY` is on the
    guest's CLOCK_BOOTTIME, 0 for one already passed.
  - shards-api `crates/api/src/image.rs`: `ImageSpec::{nesting, ready_by, shares, command}`, `ImageShare`, and
    `ImageGuest::{wait_ready, stats, stop, wait_for}`.
  - `crates/shards/src/daemon/managed.rs`: `bind_agent`, the agent-control endpoint a daemon's run with
    `shards.control=true` binds.
- Hex at main `49500736`:
  - `crates/hex-cli/src/guest.rs` (init and `hex guest relay`) and `crates/hex-node/src/relay.rs`;
  - `docs/design/shards-guest.md` (§3, §3a, §4, §4a) and `shards-image-class.md`;
  - `runtime-architecture.md` decision 9, under the person's rule of 2026-11-13: "we do not tolerate arbitrary
    constants. learned and data-derived values. Period. Account for cold starts well in any such approach."
- External:
  - pid_namespaces(7): "If the "init" process of a PID namespace terminates, the kernel terminates all of the
    processes in the namespace via a SIGKILL signal";
  - reboot(2), "Behavior inside PID namespaces": since Linux 3.4, a power-off asked in a PID namespace other than the
    initial one terminates that namespace's init, which its parent's wait(2) reports killed by SIGINT. The caller
    needs CAP_SYS_BOOT in its user namespace;
  - sd_notify(3): READY=1 "Tells the service manager that service startup is finished". Each call sends "a single
    datagram with the state string as payload", and a `/` in front of `$NOTIFY_SOCKET` names a file-system AF_UNIX
    socket. "It is also possible to reimplement the simple readiness notification protocol without external
    dependencies";
  - Linux `fs/overlayfs/params.c`, `ovl_mount_dir_check`, which refuses an upper directory whose dentry has
    `DCACHE_OP_REAL`, which an overlayfs gives its own: "filesystem on %s not supported as upperdir";
  - Linux `Documentation/admin-guide/cgroup-v2.rst`: delegation and the no-internal-process rule;
  - OpenSSH `PROTOCOL` §2.4: `direct-streamlocal@openssh.com`, a channel to a Unix socket named by its path;
  - Firecracker `docs/vsock.md`, "Host-Initiated Connections" (`CONNECT <port>`), the form Shards' vsock speaks.

## 1. The composition

Hex's PID 1, `hex` as `/init`, runs as the command of a Shards run under the nesting grant. There it is PID 1 of the
workload's own PID namespace. shards-init stays the VM's PID 1. The `hex` binary and `init_main` are the same in
both hosts.

| Process | Runs as | Owns |
|---|---|---|
| shards-init | the VM's PID 1, the kernel's init | the VM: its root over the initramfs, `/proc`, `/sys`, `/dev` and D44's device rules; its network and default deny, held past the VM by its network process; D188's room and give-back; STATS; templates and pools; the readiness it tells the host (D234); the run's end; the agent-control endpoint's leg inside the VM (D238) |
| Hex's PID 1 (`hex` as `/init`) | the run's own command under the nesting grant: root, PID 1 of the workload's own PID namespace, in the leaf `run/workload/init` | Hex's guest: the node's tmpfs, its delegated subtree's controllers and box root, the boot's deadline over the node and the relay, their start, its READY=1, every orphan of its namespace, and the run's end (it exits) |
| `hex node` and `hex guest relay` | its children, unchanged | the boxes, as on any Linux host; the node's wire, carried by the relay |

The alternatives, and why not:
- **shards-init as Hex's init's child.** shards-init would run without being PID 1 and Hex's init would stay the VM's.
  shards-init's duties are the VM's: the root switch over the initramfs, the control page, the run protocol and the
  room. D234 nests a runtime *under* it, so this would be another program than shards-init, and Hex would gain none of
  D234's grant or D231's run. Refused.
- **shards-init handing PID 1 over** (an exec into `hex`). Its live duties (the room, STATS, readiness, the run's end)
  would end with its process. Refused.
- **Hex's node as the workload with no Hex init.** That was the step's first wording ("without PID 1"), and the person
  corrected it. Hex's init still does work there: the tmpfs its boxes' overlays need, the controllers, the deadline over
  two children, READY=1 and the end. Without it, that work would move into one-off code in the node. Refused.
- **Chosen:** Hex's PID 1 as the nested workload's command. One init and one code path, PID 1 under either host, and
  each host keeps the duties that are its own.

## 2. Hex's PID-1 duties under each host

| Duty | As the VM's init (Shards' direct boot, Firecracker) | Under shards-init |
|---|---|---|
| Leave the initramfs | moves itself onto a tmpfs root (`MS_MOVE`, `chroot`) and runs again, since pivot_root(2) refuses the rootfs | none: shards-init moved its overlay root over the initramfs before the run (D234 f28) |
| `/proc`, `/sys`, cgroup2, `/dev`, tmpfs `/run` and `/tmp` | mounts them | given by shards-init, and mounted by Hex's init no second time (D234, "Already the workload's"): `/proc` masked, `/sys`, the cgroup namespace's hierarchy writable (`cgroups-rw`), `/dev` with D44's devices. Mounting them again would undo shards-init's masks and device rules |
| The node's tmpfs | `/run`, whole | a tmpfs on `/run/hex/state` and one on `/run/hex/staged`. The run's root is an overlay, and overlayfs refuses an overlay as a box's upper directory (`ovl_mount_dir_check`). The relay's staged layers also carry overlay whiteouts and opaque xattrs of their own. The tmpfs mounts use the grant's CAP_SYS_ADMIN in the workload's own mount namespace, and leave `/run/shards` and the shares as shards-init made them |
| Shares | `hex.shares=` on the kernel's command line; init mounts each as virtiofs at `/run/hex/shares/<i>` | the run's shares (`ImageShare`), mounted by shards-init at `/run/hex/shares/<i>` before the command starts (D109, D231). The paths are the same, so the host's rewrite (`shards-guest.md` §4a) is unchanged |
| Controllers and box root | `+cpu +io +memory +pids` at the root's `cgroup.subtree_control`; the box root `/sys/fs/cgroup/hex` | the same, in the delegated subtree. The namespace root is `run/workload`, and its processes are in the leaf `init`, so the write meets no internal process (cgroup-v2.rst; D234 f8, f9) |
| Descriptors | raises `fs.nr_open` to the node's need, then sets the node's soft and hard RLIMIT_NOFILE to it | none. `fs.nr_open` and the limit are the run's: 1048576 soft and hard by default (the kernel's default `nr_open`, D234 f14), or the run's own, with `fs.nr_open` raised by shards-init (f15). The node raises its soft limit to the hard one and fits every descriptor bound to it (9c75ca76) |
| The boot's deadline | `hex.ready-ms` or `hex.firecracker-ready-ms` on the kernel's command line, counted from the kernel's start | `SHARDS_READY_BY`: the same clock, unit and form (CLOCK_BOOTTIME nanoseconds, D234 f24), counted from the run's arrival, so neither a template's restore nor a pool's wait counts against it; 0 where it passed before the run arrived. Either way init hands it to the node and the relay as `HEX_BOOT_READY_BY` and holds each child's ready line to it |
| GPU authority | `hex.gpu.box=` and `hex.gpu=` on the kernel's command line | none. Under shards-init the kernel's command line is Shards' own. D234's `gpus=` (`/run/shards/gpus`) is not built, so a node VM under shards-init is CPU-only (§5) |
| Start the node, then the relay | each within the deadline; the relay on vsock (`--vsock`) | the same, but the relay listens on AF_UNIX sockets in `/run/hex/channel` (`--unix`, a form the relay has today): Docker's default filter, which shards-init gives the workload, refuses `socket(AF_VSOCK)` (D234 f27) |
| Readiness | `hex guest ready` on the console, and Firecracker's ready word over vsock where Firecracker asks | `hex guest ready`, then READY=1 to `NOTIFY_SOCKET` (sd_notify(3)) once the node and the relay are ready. shards-init owns the readiness state and tells the host (D234's table; `ImageGuest::wait_ready`) |
| Reaping | every process that ends | the same, as PID 1 of its namespace (pid_namespaces(7)) |
| The end | when the node or the relay ends, init says why and powers the VM off (`reboot(2)`) | init says why and exits. reboot(2) in a non-initial PID namespace would only terminate this init, and needs CAP_SYS_BOOT, which the grant doesn't give. The exit ends the namespace (pid_namespaces(7)), and the run ends with it (D234). shards-init's policy decides what happens to the VM |

## 3. How Hex's init knows its host

Init reads its host once, as it starts, into an explicit state (R04). The state comes from words that only that host
sets:
- **`Kernel`**: no `NOTIFY_SOCKET` in init's environment. The kernel's command line is the host's: Hex's
  `GuestConfig`, or Firecracker's configuration. Init reads it as it does today, and the kernel puts any other
  `name=value` words in init's environment.
- **`Manager { notify, ready_by }`**: `NOTIFY_SOCKET` names an absolute path (sd_notify(3)'s file-system form;
  shards-init's `/run/shards/notify`). `SHARDS_READY_BY`, where present, is decimal nanoseconds on CLOCK_BOOTTIME.
  shards-init sets both over anything the run or the image gives (`Inherited::env`).
- **Refused**, typed, init saying why and ending:
  - `NOTIFY_SOCKET` not an absolute path, or longer than `sun_path` holds;
  - `SHARDS_READY_BY` without `NOTIFY_SOCKET`, or anything but decimal nanoseconds.

Why `NOTIFY_SOCKET` decides it:
- sd_notify(3) names it as the variable by which a service manager tells its service it listens;
- it is the grant's own word (D234);
- no other host sets it.

Why not something else:
- **argv**: it is the image's, and an image can say anything;
- **the kernel's command line**: under shards-init it is Shards';
- **PID 1 or the parent PID**: they are the same in both hosts (`getppid()` is 0 for a namespace's init too).

## 4. The image

- **What it holds.** Hex's static `hex` (musl, for the guest's architecture, as `scripts/guest/image.sh` builds it) at
  `/init`, and nothing else. It declares `NESTING` and no `AGENT` or `HARNESS`, and runs its command as root (D234: a
  nested runtime's image runs its command as root). Its entry point is `["/init"]`, so `argv[0]` is `/init` as the
  kernel gives it, and `runs_as_init` holds in both hosts.
  - The Agentfile is `scripts/guest/node.Agentfile`.
  - `scripts/guest/node-image.sh` builds it with `shards build` into the node's Shards home.
  - Its digest is recorded beside the kernel's and initrd's SHA-256 in `bench/native/guest/<arch>/RECORD.md`.
- **How it boots.** Through shards-api's image boot (D231):
  - `image`: `<name>@sha256:<digest>`, pinned, as the image class pins (`shards-image-class.md` §1). A tag that moved
    between create and boot would boot another image.
  - `vcpus` and `memory_mib`: the node VM's, as the direct boot's (`HEX_GUEST_CPUS`, `HEX_GUEST_MEMORY_MIB`).
  - `command`: `["/init"]`.
  - `nesting`: true.
  - `ready_by`: the boot's learned bound, or none while it follows (§6).
  - `shares`: the node's shares at `/run/hex/shares/<i>`, as the direct boot gives them.
  - `publish`: none.
  - `egress`: every image grant dropped, so default deny holds.
  - `binary`: `SHARDS_BIN`.
  - `env`: `SHARDS_HOME`, whose store holds the image.
- **Which form.** `HEX_GUEST_IMAGE` names the image. It and `HEX_GUEST_KERNEL` with `HEX_GUEST_INITRD` are two forms
  of the node's guest, and a node uses one. Both set is refused by name. The direct boot stays the default.

## 5. The channel: step 11's agent-control endpoint

The host reaches its guest's node through `hex_client::guest::Link::dial(port)`, a byte stream to one of the relay's
four listeners: control, stdio, files and stamps (`shards-guest.md` §3a). Under shards-init these are AF_UNIX stream
sockets in the workload's `/run/hex/channel`: `control.sock`, `stdio.sock`, `files.sock` and `stamps.sock`, the
relay's `--unix` form, which exists today. So Hex needs two things of step 11:
- **A socket lane.** The controller opens a lane naming one of a nested workload's sockets. The endpoint carries it to
  the microVM over vsock (D238's arm h), and shards-init's relay connects to that socket. The bytes arrive exact both
  ways, with a half-close carried as Shards' vsock carries one (D12), and a lane to a socket nothing listens on is
  refused, typed. It is OpenSSH's `direct-streamlocal@openssh.com` (PROTOCOL §2.4) in the endpoint's own terms.
- **An endpoint for an orchestrator's run.** Today only a daemon's run with `shards.control=true` binds one
  (`daemon/managed.rs`, `bind_agent`); D231's run host binds none.

The alternatives:
- **An exec lane per connection** (D238's exec lanes, design (B)). Each control, stdio and staging connection would
  start a guest process to bridge its stdio to the socket, as `kubectl port-forward` once bridged through `socat`.
  Every box's exec would pay for three such processes. It is the fallback should Shards not add a socket lane.
- **A published port into the workload** (D231 publishes to the run's own command). It exists today, but it is the
  network path step 10 measured and declined for control (D238: decided at the host), and the plan names step 11.
- **vsock granted to the workload.** D234 refuses it (f27).

Until Shards gives the lane, the node refuses its shards-init form by type before it boots anything:
`Refusal::Awaits(Upstream::NestedChannel)`, naming step 11's lane. The image class refuses its missing run host the
same way (`shards-image-class.md` §1). No success is stubbed. A node VM asked a GPU under shards-init waits on D234's
`gpus=` too.

## 6. Waits

The person's rule, 2026-11-13, applies to every wait: no record seeds, no chosen multiples. A cold start follows its
party while that party lives, and after it the bound is the slowest answer seen.
- **`guest.hosted.ready`**: the node VM's READY, through `ImageGuest::wait_ready`, on time or late.
  - At its cold start it follows the run, with no deadline: the run's end refuses it (`ImageRefusal::Ended`).
  - After that it is bounded by the slowest READY this node has seen (`hex_core::patience::Patience::measuring`).
  - A run-out is no sample (Karn's point), and the next wait follows again.
  - The same bound is the run's `ready_by`, so Hex's init holds its node and relay to it inside the VM.
- **`Images::boot`'s own patience**: `Duration::MAX`, until the run starts or its host ends. The run host's life is
  that wait's liveness.
- **Stop**: `ImageGuest::stop` returns once the run's end is told. Shards' stop takes no patience.

No record seeds any of them. The waits census keeps their answers as evidence.

## 7. What the shards-init form gains, and where each part stands

| Part | Where | On Shards main `7a4ef5c0` |
|---|---|---|
| The nesting grant: the subtree, CAP_SYS_ADMIN and CAP_NET_ADMIN, the filter with `pivot_root`, the deadline, READY | D234 | built, and in Hex's vendored shards-api (`66113379`). Its real-VM green is owed (Shards ROOT). `gpus=` is not built |
| The agent-control endpoint | D238, step 11 | building. Built: its connection, the microVM's pinned key, members' stdio lanes, the daemon's endpoint per run, and exec lanes' endpoint side. Not built: a lane to a workload's socket, an endpoint for D231's run host, the exec's host and the controller's exec handle |
| Templates and warm pools | D25, D26 | the daemon's runs have them; D231 owes them for an orchestrator's run |
| D188's room and give-back | D241 through D231 | built |
| Metrics | STATS by scope, D231 | built |
| Default-deny networks | D231's narrowing, D236 | built |

## 8. What Hex keeps: everything

The PID-1 path stays as it is:
- the init and the relay;
- the guest image (`scripts/guest/build.sh`, `image.sh`, `initramfs.py`);
- the vsock ports and the staging;
- `GuestConfig`'s kernel and initrd.

It boots on every hypervisor Hex uses (Shards' direct boot, Firecracker), and it stays the default form. The change to
init adds the `Manager` host beside it. Its tests and ROOT's real-VM runs of it still pass on their own, which is the
acceptance's second half.

## 9. Acceptance, and what ROOT runs

- Hex's oci class passes its conformance suite through shards-init on macOS (HVF) and Windows (WHP) with Hex's PID 1
  intact, once step 11's lane is given. Hex's PID-1 path still passes on its own.
- ROOT measures start latency and memory per node VM in both forms. Start latency is boot to READY, then to the first
  create. Memory is STATS' `memory_bytes` and the host's VM process.
- `crates/hex-cli/tests/node_hosted_real.rs` (`--ignored`, ROOT's) boots the node image through shards-init to READY,
  reads `stats(Run)` and stops it, n times, with n, p50, p90, p99 and max. Until step 11, that is the form's measurable
  part.

## 10. What would falsify it

- Hex's node needing a capability or syscall beyond D234's grant to make its boxes. Then Shards adds it by name, which
  is D234's own falsifier. Two candidates:
  - CAP_SYS_RESOURCE, for a box asking an rlimit above the node's hard limit;
  - a syscall Docker's default filter refuses, since that filter stacks onto every box beneath the workload.
- A box's overlay on the node's tmpfs failing for a reason other than the overlay's own. Then the tmpfs is not enough.
- `/run/shards/notify` or a share unreachable once init mounts its tmpfs. Then the mount points move.
- A node's descriptor need above the run's hard limit (1048576 by default). The node then runs at lower capacity,
  which ImageSpec can't raise today, since it has no ulimit field.
- shards-init's form costing more start latency or memory per node VM than the direct boot by more than its gains are
  worth. ROOT's measurement decides which form is the default; both stay.
