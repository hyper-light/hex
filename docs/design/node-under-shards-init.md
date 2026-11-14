# Decision (2026-11-13): Hex's node under shards-init, with Hex's own PID 1 kept

Joint plan step 39 (kiln plan `01a1cd8e862e7169a074db3d5fa33f3d` version 12, "Hex's node under shards-init, with
Hex's own PID 1 kept"), AC-T7.4.1. Status, 2026-11-13 (built, merged and live apart; live is GitHub's main,
`025aae6d`): **decided: Hex's own PID 1 is kept, and shards-init works with it, never replacing it**. Live:
- init under shards-init (§2, §3; hex-cli `guest.rs`'s `Host` and `duties`; `7f4bda5b`);
- the node image and its build (§4; `7f4bda5b`, `982f1a6b`);
- the host's boot of it to READY (§6; hex-runtime `guest::hosted`; `b29e0bc3`), and Hex's PID-1 path booting again
  (`9590d4d4`).

Merged on Hex's main, the form whole; its real-VM runs are ROOT's (§9):
- **Its channel** (§5): Shards' step 11, a lane to a socket of the run's own command and the run's own agent-control
  endpoint (D251, D252), vendored since `c8b4ce6b`; a guest's boot as one form, `Boot::Kernel` or `Boot::Image`, and
  the Link's lanes (`a8f6bfe4`); the node's descriptors fitted on every OS with each hosted VM's lanes among them
  (`f2e6d8a2`, `885738a1`); a lane its socket cuts short, read to its cut on Linux and macOS (`ce3ab706`); the image
  form's tests on Windows, through the lane ends Shards' run host makes there (`5cc5efbc`).
- **The node VM's entry** (§4, §5a): E1, its run (`NodeRun`), a holder of the node's ledger (`74f3f3ca`, `4a478b8c`);
  E2, its record (`NodeVmRecord`) in rows of its own, its boot, stop and host use through the image class's runs
  (`1639cdb1`, `605c2e89`); E3, `Boot::Image` in the guest runtime, its receipt on the oci answer, booked by the
  scheduler (`28c57ba8`); its rows its owner's while its owner's lock holds (`421e6946`); init's children started
  without shards-init's words (`14cca959`); E4, this record as built.
- **Its charge and its place** (§5a): a holder of its node's charge learner (`45582d0a`, `4f791dec`); its
  last-resort end kept in its row across its node's restart (`fe3d5198`); its guest told its room (`6a8c6da8`); its
  boot charged, and one Ready a node (`2846e94f`); its oci boxes counted once, under a class of its own (`1ad96650`);
  what its learner couldn't read, and its boxes' end, told (`08800bcd`); its boxes placed in its guest's room and
  charged once (`3eb841a2`).

READY through shards-init against the direct boot: on Linux KVM at `8aa7c028`, n=23, p50 152.4 ms against 134.3 ms;
on macOS HVF at `cf535152`, about 165 ms against 103 ms. Owed (§9): the conformance suite's test through a `hex node`
in this form, next; then, ROOT's with Shards ROOT, its runs on HVF and WHP, the host VMM's memory both ways, and every
WHP run.

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
- Shards' code at `66113379`, the revision Hex vendored when this was decided:
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
- As built (§4 to §9), Hex at main `3eb841a2`, which vendors Shards `16451e19f` (since `59f7890f`):
  - hex-runtime `guest::image::node` (the node VM's boot, stop, room and waits), `guest::GuestRuntime::boot_image`
    and `add_node_vm_offers`, and `guest::hosted` (`node_hosted_real`'s boot);
  - hex-client `guest::lanes` (the channel) and hex-core `record::node_vm`;
  - hex-node `store::node_vm` (the rows and their owners), `recorded::charges` (the learner) and `placement`, and
    hex-sim `index::routes` (the scheduler's hold);
  - Shards `docs/design/architecture.md` at `16451e19f`: D234, D238, D251, D252 and D253's status lines.
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
  - `NOTIFY_SOCKET` not an absolute path;
  - `SHARDS_READY_BY` without `NOTIFY_SOCKET`, or anything but decimal nanoseconds.

  A socket path longer than `sun_path` holds is refused by the datagram's send (std's Unix socket address), and
  init's boot ends with it.

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
  - `vcpus` and `memory_mib`: its admitted allocation's, its receipt (§5a): the configured VM (`HEX_GUEST_CPUS`,
    `HEX_GUEST_MEMORY_MIB`) under the guest's cost profile, whose floor and ceiling are that VM. A size Shards would
    boot as another (`shards_api::envelope`) is refused.
  - `command`: `["/init"]`.
  - `nesting`: true.
  - `ready_by`: none. Hex's init would end the boot at a deadline, and a node's learned bound is where its wait tells
    a run-out, never where a boot ends (§6; decision 9).
  - `shares`: the node's shares at `/run/hex/shares/<i>`, as the direct boot gives them.
  - `publish`: none.
  - `egress`: every image grant dropped, so default deny holds.
  - `control`: the run's own agent-control endpoint (D252), taking the node's key alone, with lanes to the relay's
    sockets (D251; §5).
  - `gpu_devices`: none, so a node VM under shards-init is CPU-only (§5).
  - `binary`: `SHARDS_BIN`.
  - `env`: the node's own Shards paths, `SHARDS_HOME` (whose store holds the image) and `SHARDS_CACHE`.
- **Which form.** `HEX_GUEST_IMAGE` names the image. It and `HEX_GUEST_KERNEL` with `HEX_GUEST_INITRD` are two forms
  of the node's guest, and a node uses one. Both set is refused by name. The direct boot stays the default.

## 5. The channel: step 11's agent-control endpoint

The host reaches its guest's node through `hex_client::guest::Link::dial(port)`, a byte stream to one of the relay's
four listeners: control, stdio, files and stamps (`shards-guest.md` §3a). Under shards-init these are AF_UNIX stream
sockets in the workload's `/run/hex/channel`: `control.sock`, `stdio.sock`, `files.sock` and `stamps.sock`
(`hex_core::stream::socket_name`), the relay's `--unix` form. Hex needed two things of step 11, and Shards built both
(vendored since `c8b4ce6b`; their real-VM runs owed, §7):
- **A socket lane** (D251). The controller opens a lane naming one of a nested workload's sockets. The endpoint carries
  it to the microVM over vsock (D238's arm h), and shards-init's relay connects to that socket. The bytes arrive exact
  both ways, with a half-close carried as Shards' vsock carries one (D12), and a lane to a socket nothing listens on is
  refused, typed. It is OpenSSH's `direct-streamlocal@openssh.com` (PROTOCOL §2.4) in the endpoint's own terms.
- **An endpoint for an orchestrator's run** (D252). Its run host binds one for that run alone, its key the run's own,
  and takes the one controller key its run names.

As built (`a8f6bfe4`, `f2e6d8a2`, `28c57ba8`):
- **The ask.** The node VM's spec asks its run's endpoint (`ControlAsk`, §4): its controller the SHA-256 of the node's
  own key, made once by its guest runtime from the OS's random numbers; its socket grant `/run/hex/channel`
  (`hex_client::guest::lanes::DIR`), at most the lanes the node fitted to its descriptors with this VM counted among
  its hosted ones (hex-node's `serve::fit`), each lane's bytes in flight a guest frame each way
  (`hex_core::stream::MAX_FRAME`), the most one control message is.
- **The connection.** `hex_client::guest::lanes::Lanes::dial` connects to the endpoint the boot answered
  (`ControlAt`), pinning the microVM's key (D252), for socket lanes alone, and waits for both ends' word on the
  connection's TLS (§6).
- **The lanes.** Each port is a lane to the socket it names, bridged to a local socket pair (`guest::bridge`), so the
  client's event loop waits on an OS handle as it does on vsock's. A lane carries a half-close each way, and its reset
  or its connection's end is told after its bytes; a lane its socket cuts short is read to its cut, on Linux and macOS
  (`ce3ab706`). On Windows the lane's ends are the plain pipe pair Shards' run host makes there (`5cc5efbc`).
- **The greeting.** The guest runtime dials the relay's control socket through a lane and greets its node
  (`Client::connect`), whose control connection then joins the node's loop as a direct boot's does. A node VM whose
  node won't greet it, or whose connection can't join, is stopped, and its boot refused in its words.

The alternatives, declined:
- **An exec lane per connection** (D238's exec lanes, design (B)). Each control, stdio and staging connection would
  start a guest process to bridge its stdio to the socket, as `kubectl port-forward` once bridged through `socat`.
  Every box's exec would pay for three such processes. It was the fallback, had Shards not added a socket lane.
- **A published port into the workload** (D231 publishes to the run's own command). It is the network path step 10
  measured and declined for control (D238: decided at the host), and the plan names step 11.
- **vsock granted to the workload.** D234 refuses it (f27).

No call awaits `Upstream::NestedChannel` now; its tag stays, the node's wire being append-only. A node VM asked a GPU
under shards-init (`HEX_GUEST_GPU=true` beside `HEX_GUEST_IMAGE`) is refused `Awaits(NestedRuntime)` before anything
boots. D234's `gpus=` is vendored (`ImageSpec::gpu_devices`, since `c8b4ce6b`), so what that refusal waits on now is
Hex's own: the node VM's spec asks no GPU (§7).

## 5a. The node VM: its record, its charge and its place

A node VM is its node's own and never a box: no box call reads it. It is booked, charged and placed as an image VM is,
through the image class's and the learner's own code (ROOT, 2026-11-13: one code path, not a third). Its oci boxes run
in its guest, the node there unchanged.
- **Its record** (`hex_core::record::NodeVmRecord`; `605c2e89`): its run (`NodeRun`, minted by the node's store), its
  image, its admitted allocation, its phase (Booting, Ready, Ended), its diagnostics' file, its end, its last-resort end
  and its start place. It is kept in the store's rows of its own (`.node-vms`), apart from box and pool rows: Booting
  before any effect, Ready once its init says it is, and Ended with its end.
- **Its owner** (`421e6946`). A run's row is its owner's while its owner's lock holds, one lock a run
  (`.node-vms/<run>.lock`): fcntl(2)'s F_SETLK on Unix, which fork(2) doesn't hand on, and LockFileEx on Windows, on a
  handle no child inherits. The basis is D231's: no microVM outlives its owner. At a node's start
  (`Store::settle_node_vms`), an Ended row is let go, a row whose lock the start can take is let go (its owner, or the
  host's boot, ended), and a row whose owner still runs is followed to that owner's end, then let go; its recovery
  summary tells each. A node holds one node VM Ready at a time (`2846e94f`): its oci class has one guest a node, which
  serves every oci box of the node.
- **Its receipt** (`28c57ba8`): its admitted allocation, the configured VM under the guest's cost profile (§4), as the
  direct boot's guest is charged, so no node VM runs uncharged. The oci answer carries each node VM's receipt, last
  (`Capacity::node_vms`: its run, its booking, its phase, and its guest's envelope since `3eb841a2`). The scheduler
  books the Booting and Ready ones in its node's ledger before any admission (`Driver::book_nodes`), lets go of what has
  Ended or gone, and refuses a run named twice.
- **Its charge** (macOS and Windows; `45582d0a`, `4f791dec`, `6a8c6da8`, `2846e94f`, `1ad96650`, `08800bcd`). A Ready
  node VM is a holder of its node's charge learner (`Holder::Node`), learned under a class of its own (`ClassKey::node`)
  from its run's host use (`ImageClass::host_use_node`) over its run's own rate period, capped through its run's rate
  (`rate_node`, D247) and told held as an image VM is (D260), and its guest told its room through its run (D188,
  `room_node`). A booting node VM's booking comes off the fleet's budget until it is Ready and learned. Its oci boxes
  are its, counted once: none is charged natively beside it. Its band is what ending it ends, the highest of its live
  oci boxes' bands; an unreadable record ranks it above every box, and the answer says so (`Learned::ranked`), as it
  names each VM its window couldn't read (`unread`), each room a guest awaits (`awaits`) and why a window failed
  (`Capacity::unlearned`).
- **Its last resort** (`fe3d5198`, `08800bcd`). The learner's intent to end it is written into its row and into each
  oci box it runs, before the end. Its end survives its node's restart and joins the learner's ends at the next start;
  where none is on record, the start says so, a cold start told. Once its run has ended, its oci boxes are retired for
  `LifecycleCause::NodeVmEnded`: kept, ended and their why on record until their owners delete them, each window and
  at start before recovery plans.
- **Its place** (`3eb841a2`). The oci answer offers one route for its boxes, `Execution::NodeVm`: a Linux image of the
  host's architecture with no GPU, charged nothing at the host, since its node VM's receipt pays the host envelope
  once. That route's room is the Ready node VM's guest envelope (`NodeVmReceipt::guest`, hex-sim's
  `RuntimeRoutes::node_vm_room`), which the scheduler holds those boxes to (`holds_inner`); with no node VM Ready, it
  admits none. A host box gets only the room past its node VM, as admission takes the node VM's booking off the node's
  own budget (`admission_spec`). On macOS the image form answers its oci class on the host's plane, and its process
  class keeps its native route there; on Windows the route joins the host's native ones. A box its guest refuses for
  room after the host's pool admitted it is told with both numbers: the pool's room, what the pool books with that box,
  and the guest's own free room and words. Placement rows written before `Execution::NodeVm` read back as they were
  written.

## 6. Waits

The person's rule, 2026-11-13, applies to every wait: no record seeds, no chosen multiples. A cold start follows its
party while that party lives, and after it the bound is the slowest answer seen (decision 9). The host's waits below are
the node VM's, in hex-runtime's `guest::image::node` (`605c2e89`); `node_hosted_real`'s boot
(`guest::hosted::Hosted::boot`) follows READY as it does (`follow_ready`). Each run-out past a W* is told once on the
process's log (`hex.wait event=ran_out`), naming its wait, its W*, how long and its party (`7a524d20`). Inside the VM,
init waits for its node's and its relay's ready lines by each child's life, with no deadline, as its run carries none
(§4).
- **The boot** (`hex.image.boot`): the image boot runs on a thread of its own, as a box's does, its own patience
  `Duration::MAX`, until the run starts or its run host ends. Within the boot's W*, the image class's, which its boxes'
  boots share, its answer; past it, the run-out told and the boot followed by its thread's life. It is never answered
  Pending, as a box's start can be: the node waits for its own VM.
- **`guest.hosted.ready`**: the node VM's READY, through `ImageGuest::wait_ready`, on time or late.
  - At its cold start it follows the run, with no deadline: the run's end refuses it (`ImageRefusal::Ended`).
  - After that it is bounded by the slowest READY of a node VM this process has seen
    (`hex_core::patience::Patience::measuring`). A run-out is no sample (Karn's point).
  - Past that bound the wait goes on a W* window at a time while the run progresses: its progress is its VM process's
    CPU time as its host counts it (D241, `stats(Run)`). It is refused, stalled, only where a whole window shows
    none, and a window with no reading judges no stall, the run then followed by its life alone. A VM's boot is the
    one place decision 9 judges a stall.
  - The run's end, its host's end or a refusal of Shards' ends it at any time, typed.
- **The channel** (`Lanes::dial`): both ends' word on the connection's TLS, followed by the connection: its
  handshake's end, the endpoint's refusal or the connection's end answers it.
- **The room** (`room_node`): its guest's answer, waited for while its run lives; the run's end refuses it.
- **Stop** (`hex.image.stop`): D231's stop is never refused, and its thread answers once the run host has gone. Past
  the stop's W*, the run-out told, the stop is followed by its thread's life.
- **A row whose owner still runs** (`store.node_vm_owner`): its owner's lock, on a thread of its own, to that owner's
  end. Past its W*, the run-out told, it follows the owner by its life. A follow that fails is said as
  `hex.guest event=node_vm_unfollowed`, its why a JSON string.

No record seeds any of them. The waits census keeps their answers as evidence.

## 7. What the shards-init form gains, and where each part stands

| Part | Where | At Shards `16451e19f`, which Hex vendors (`59f7890f`) |
|---|---|---|
| The nesting grant: the subtree, CAP_SYS_ADMIN and CAP_NET_ADMIN, the filter with `pivot_root`, the deadline, READY | D234 | built. Its real-VM test, red on Shards CI's KVM x86_64 for two defects of its own, is fixed; its green and other runs are owed (Shards ROOT). `gpus=` is built (`ImageSpec::gpu_devices`); a node VM asks none yet, which is Hex's to do (§5) |
| The agent-control endpoint | D238, step 11 | the node's channel built: a lane to a workload's socket (D251) and an orchestrator's run's own endpoint (D252), each with its real-VM runs owed. Built beside them, and none of this form's: members' stdio, state, signal and log lanes, and exec lanes |
| Templates and warm pools | D25, D26, D253 | built for an orchestrator's run, their real-VM runs owed: a run's microVM restored from its image's template where one is saved, and saved where none is, as the daemon's are; warm hosts kept by the orchestrator's own pools (`ImagePools`), which Hex's image class doesn't take yet |
| D188's room and give-back | D241 through D231 | built, its real-VM validation owed. A node VM's guest is told its room through its run (§5a) |
| Metrics | STATS by scope, D231 | built: a node VM's host use, and its boot's progress (§5a, §6) |
| Default-deny networks | D231's narrowing, D236 | built, its real-VM validation owed. A node VM's spec drops every grant (§4) |

## 8. What Hex keeps: everything

The PID-1 path stays as it is:
- the init and the relay;
- the guest image (`scripts/guest/build.sh`, `image.sh`, `initramfs.py`);
- the vsock ports and the staging;
- the guest's kernel and initrd, now `GuestConfig`'s `Boot::Kernel` beside `Boot::Image` (`a8f6bfe4`).

It boots on every hypervisor Hex uses (Shards' direct boot, Firecracker), and it stays the default form. The change to
init adds the `Manager` host beside it (`7f4bda5b`). That change's first form broke the PID-1 path's boot, as its init
found itself through `/proc/self/exe` before mounting `/proc` (`219a8844`), and `9590d4d4` mended it. The PID-1
path's tests, and ROOT's real-VM runs of it, pass on their own, which is the acceptance's second half (§9).

## 9. Acceptance, and what ROOT runs

- **The suite through shards-init.** Hex's oci class passes its conformance suite through shards-init on macOS (HVF)
  and Windows (WHP) with Hex's PID 1 intact: a `hex node` whose guest is its image (`HEX_GUEST_IMAGE`), every case
  through the node's socket. The channel it needs is built (§5). Its test is owed next (goal 6's item 4); its runs are
  ROOT's.
- **The PID-1 path on its own.** `crates/hex-cli/tests/guest_real.rs` (HEX-GUEST-19: the suite in process, and through
  a macOS node's socket), in the direct boot's form (`HEX_GUEST_KERNEL`, `HEX_GUEST_INITRD`), on a real VM (ROOT's).
- **Both forms measured.** ROOT measures start latency and memory per node VM in both forms. Start latency is boot to
  READY, then to the first create. Memory is STATS' `memory_bytes` and the host's VM process. Under shards-init, the
  first run of an image at its shape saves its template and later ones restore it (D253), so the two are told apart by
  how each started (`ImageGuest::started_from`).
  - `crates/hex-cli/tests/node_hosted_real.rs` (`--ignored`, ROOT's) boots the node image through shards-init to READY,
    reads `stats(Run)` and stops it, n times, with n, p50, p90, p99 and max.
  - `crates/hex-cli/tests/guest_boot_record.rs` is the direct boot's counterpart.
- **On every OS, with no VM.** The image form's w39 cases in hex-runtime's `guest::image` and `guest` tests (its read,
  boot, lanes, greeting, record, rate, room and stop, through a run host of the test's) run in each OS's `cargo test`,
  Windows' through the lane ends its run host makes there (`5cc5efbc`). Their native Windows run is ROOT's.

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
