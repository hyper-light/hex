# Decision (2026-11-12): the Shards class on Agentfile images under shards-init

Joint plan step 12 (kiln plan `01a1cd8e862e7169a074db3d5fa33f3d`, "Every kiln harness in Shards microVMs, Hex as
runtime"), AC-T7.4.1. Status: **built to Shards' D231 at `482c5d78`**; its real-VM runs, the records its waits seed
from and its macOS and Windows hosts are owed (§8).

Sources, read at the revisions named:
- Shards' D231 ("An image booted for an orchestrator", `docs/design/architecture.md` at `482c5d78`) and its research
  record `docs/research/runtime-image-boot-decision-v1.md` (`31b3f106b`): the decision this class binds to.
- `shards_api::image` at `482c5d78`: `crates/api/src/image.rs` (`Images`, `ImageGuest`, `ImageSpec`,
  `ImageRefusal`), `image/session.rs` (the run host's process, `stop`, `wait_for`, `stats`) and `image/wire.rs`
  (`Stats`, `StatsScope`, `RunEnd`, `Narrowing`); `crates/shards/src/api_run.rs` (how a narrowing keeps grants).
- Shards' D54 (a harness started from its `harness.json`), D59 and D115 (members in confined domains, the run's own
  command apart from them), D24 (`shards run IMAGE COMMAND`), D215 (the direct boot) and D233 (a client's native
  launch); `crates/image/src/reference.rs` and `store.rs` (references and digests as Shards reads them).
- Hex's `docs/design/shards-guest.md` (the hex-guest path, `hex` as PID 1) and `runtime-architecture.md`
  decision 9 (learned waits, seeded from records).

## 1. The shape

- A box of `ClassSpec::ShardsImage(ShardsImage { image, command })` is of `Class::Shards`: one microVM of its own,
  which `shards_api::image::Images::boot` boots under shards-init. shards-init applies the image's Agentfile: each
  member (agent or harness) starts in its confined domain from its own `harness.json`/`agent.json` (D54, D59).
- Hex's other Shards form, `ClassSpec::Shards(OciSpec)`, keeps booting Hex's own guest (`hex` as PID 1, its node
  inside; `shards-guest.md`), and so does the `oci` class on macOS and Windows. Both stay unchanged until step 39.
- **Size.** `Allocation::vm_size`: the admitted guest envelope in whole vCPUs (1,000 CPU milli each) and MiB, as the
  dedicated Shards guest's receipt is sized. It is never a field of the spec, and never Shards' own sizing (D231's
  0): users set budgets and VM sizes, the scheduler admits them, and the VM is what was admitted. A VM may have more
  than one vCPU.
- **Image.** Pinned by digest (`NAME@sha256:…`; go-digest's sha256, sha384 or sha512 with their exact lowercase hex
  lengths, as Shards' reference parser takes them). Shards' store finds a pinned image by content, whatever it is
  named (`Store::holding`). Pinned because create records the box and start boots it: a tag that moved between them
  would boot another image than the one admitted.
- **Store.** The image is in the node's Shards home (its runtime paths), given to the run host as `SHARDS_HOME`.
  kiln builds or pulls harness images there (the joint plan's steps 19-30).
- **Command.** `None` runs the image's members alone (D231's `ATTEND`: the standby holds, the members start), the
  usual harness VM. `Some(args)` is the run's own command, as `shards run IMAGE COMMAND` takes one; its exit is the
  box's.
- **Hosts** (`hex_runtime::guest::host_images`). Linux boots image VMs. On Windows a create is refused
  `Awaits(WindowsRunHost)`: D231's Windows run host is owed. On macOS it is refused `Awaits(RunHostStdioOnly)`:
  D231 starts its run host (`shards api-run`, `image/session.rs`) through std's `Command`, whose macOS spawn passes
  no `POSIX_SPAWN_CLOEXEC_DEFAULT`, and macOS's pipe, socket, socketpair and accept take no close-on-exec flag, so
  the run host would hold every descriptor another node thread had made and not yet marked (a box's pipe, a
  client's connection, the box helper's socket) for as long as its VM runs. Every program Hex starts on macOS holds
  only its stdio (`crates/hex-runtime/src/macos/command.rs`; CVE-2024-21626's class), and Hex's other Shards form
  launches its VM through the client's native owner (D233), so this class waits for the same of D231.

## 2. Mapping onto D231

D231's `boot` is create and start in one, and returns once the members have started. Hex keeps the runtime-spec's
split, so a box can be admitted and recorded before it is needed:

| Hex | D231 |
|---|---|
| `create` | No call. The box's record, `Creating`: admitted and named, nothing booted. |
| `start` | `Phase::Ready` written first (the boot's intent, before any effect), then `Images::boot`. |
| `signal(KILL)`, Hex's stop | `ImageGuest::stop`: the VM process killed at once; the run host says `RunEnd::Stopped` and exits. The exit, `Signal(9)`, is recorded then. |
| `signal(other)` | Refused `Awaits(MemberSignal)`: a member's signal is the agent-control endpoint's (step 11). |
| `exec` | Refused `Awaits(MemberExec)`: a member's exec is the agent-control endpoint's (step 11). |
| `inspect`, `list` | The record's phase and exit, and this process's run (`wait_for` with no wait). |
| `usage` | `stats(StatsScope::Run)` (§4). |
| `delete` | `Deleting`, then `stop` if it runs and the run host's exit, then the record removed. |

Every member operation D231 doesn't give (stdin, a terminal, exec, signals) is refused typed, naming the capability
(`Refusal::Awaits`), never stubbed.

`RunEnd` becomes the box's exit as Shards reports it: `Exited(status)` is `Exit::Code(status)` (`docker run`'s
status, 128 plus a signal's number where one ended it, kept as the number it is rather than guessed back into a
signal), `Stopped` is `Exit::Signal(9)` (HEX-RT-12's KILL), `Failed` is `Exit::Unknown`.

**Grants (goal 15).** The boot's `publish` and `egress` come from goal 15's `program(&grants)`. Until it lands, the
boot publishes nothing and keeps none of the image's grants (`Keep::Only` empty for egress and MCP, no DNS): default
deny. D231 can only narrow the image's own grants, never widen them.

## 3. States

The record (the guest's Books, the node's Store) and this process's run:

| State | Record | Run | Leaves by |
|---|---|---|---|
| Created | `Creating`, no exit | none | start: Booting; delete: removed |
| Booting | `Ready`, no exit | a boot in flight, on a thread of its own | its answer: Running, or Ended where Shards refused the run |
| Running | `Ready`, no exit | the run held | KILL, or its own end: Ended; delete: Deleting |
| Ended | `Ready`, its exit | none | delete: removed |
| Deleting | `Deleting` | stopping, or none | the run host's exit: removed |
| Held | `Ready` or `Deleting`, no exit | none: a run of an earlier node process | a host reboot (§5) |

A call on a Booting box waits on nothing: it answers what is known (`Created`'s inspect, `Lifecycle { Pending }`
for a stop or a delete), as `LifecycleCause::Pending` says. A Held box is refused `Awaits(AttachAfterCrash)` by every
call, recovery's adoption among them, and is listed running: Hex can't prove it isn't.

## 4. Usage

`stats(StatsScope::Run)` reads the run's cgroup in the guest: `cpu_ns` becomes `cpu_usec`, `memory_bytes`,
`pids`, `read_bytes` and `write_bytes` their counters. **Guest time per vCPU** is `GuestTime::Awaits
(GuestCpuTimePerVcpu)`: Shards' guest CPU time work has no commits, and a number Hex made up would be worse than
none. When it lands, `GuestTime::PerVcpuNs` carries it: nanoseconds each vCPU ran the guest since the VM booted,
by vCPU index, one for each vCPU of the VM's size, each only growing, none past the wall time the VM ran. Its wire
bound, `GuestTime::MAX_VCPUS`, is every 8-bit vCPU index: a Shards VM numbers its x86_64 vCPUs by 8-bit APIC id (at
most 255) and has fewer on arm64 (123).

Goal 14's learned charge (`hex-node` `scheduler/charges.rs`) reads `cpu_usec` and `memory_bytes` as a microVM's
use on the host: its VMM's, as the dedicated Shards guest's native scope gives them. An image VM's are its run's in
the guest, short of the guest kernel's and the run host's own, so the charge it learns for an image VM is low by
those until the run host is launched through the client's native owner, where its scope gives the host's (§8), or
the learner reads guest time per vCPU.

D231's STATS has no memory peak, OOM kill count or refused fork count. The image class reports none of these
(0, as the Windows process class reports no refused forks), and says so here; Shards owes them for a full Usage.

## 5. The owner's crash

D231's run ends with its owner: its `ImageGuest` dropped, stopped or its channel gone, the run host kills the VM.
D231 gives no identity of the run's processes and no `attach`. So:

- A Hex node that restarts ends its image VMs, but **Hex never assumes it**. A box whose record is `Ready` or
  `Deleting` with no exit, of a node process before this one, is **held**: its record and its claim are kept, and
  every call on it, recovery's adoption among them, is refused `Awaits(AttachAfterCrash)`, which recovery reports
  in its summary's `held` rather than failing the node's start.
- The proof that ends a held box is a reboot of the host (the record's boot session differs from the host's, T-T7.1.2:
  that boot's end ended everything that ran of it). D231's `attach`, or the run's identity, would give a proof
  within a boot: that is `Upstream::AttachAfterCrash`, owed by Shards.
- A `Creating` record was never booted: recovery removes it, its absence proved by the record itself. A `Ready`
  record with its exit is an Ended box, adopted as it is.

## 6. Waits

Each wait is learned through hex-core's `Patience` (decision 9), seeded from the slowest row of
`bench/native/guest/records.tsv` for this host class:

| Wait | What it waits for | Record |
|---|---|---|
| `image_boot` | `Images::boot`'s answer: the members started, or the refusal | owed |
| `image_stats` | `stats(Run)`'s answer | owed |
| `image_stop` | `stop`'s return: the run host's exit after its run's end | owed |

D231's own boot and stop wait without a bound of their own (D231 owes "a boot patience"), so Hex makes each on a
thread of its own and bounds its caller's wait; past the bound the call is refused `Lifecycle { Pending }` and the
answer, when it comes, is recorded. No record exists yet for any of them, and a measure no class records is refused
(`hex_client::guest::shards::recorded`): **a create is refused until all three are recorded**, so no box exists
that a wait of its own could not bound.

## 7. What step 39 removes

Step 39 runs Hex's node as the workload of a Shards VM booted through shards-init, with step 38's grants (a delegated
cgroup subtree, CAP_SYS_ADMIN for its boxes' namespaces, DRM render nodes), reached over step 11's endpoint. Once
Hex's oci class passes its conformance suite that way on macOS and Windows, it removes:

- `hex` as PID 1: `crates/hex-cli/src/guest.rs`'s init and the relay (`hex guest relay`, `crates/hex-node/src/relay.rs`);
- the guest image Hex builds (`scripts/guest/build.sh`, its initramfs) and `GuestConfig`'s kernel and initrd;
- the vsock channel's ports (`CONTROL_PORT`, `STDIO_PORT`, `FILES_PORT`, `STAMPS_PORT`) and the staging of roots
  into the guest (`hex_client::guest::stage`, `rewrite`);
- `ClassSpec::Shards(OciSpec)`'s route through Hex's own guest node, the shared and dedicated guests alike.

This class stays: it is how every Shards VM boots, once Hex's own node boots the same way.

## 8. Owed

- **Records** for `image_boot`, `image_stats` and `image_stop` on Linux-KVM, and on macOS-HVF once its run host is
  given (ROOT's measurement): `Images::boot` of a harness image to its return, `stats(Run)` to its answer, `stop` to
  its return, each `n` with its slowest, added to `bench/native/guest/records.tsv` with their RECORD.md. Until then
  a create is refused.
- **Real-VM runs** (ROOT): the image class's conformance cases (HEX-IMG-1..6) on a node with Shards configured, on
  Linux KVM, and on macOS HVF once its run host is given; HEX-IMG-4 fails until Shards gives guest time per vCPU,
  naming it.
- **From Shards:** the run host started holding only its stdio on macOS (`RunHostStdioOnly`;
  `POSIX_SPAWN_CLOEXEC_DEFAULT` in D231's spawn, or the run host's launch through the client's native owner, as
  D233's guests have), the Windows run host (`WindowsRunHost`), `attach` or the run's identity after an owner's
  crash (`AttachAfterCrash`), guest time per vCPU (`GuestCpuTimePerVcpu`), member exec and signals (`MemberExec`,
  `MemberSignal`; step 11), a boot patience, and STATS' memory peak, OOM kills and refused forks. Until those two
  hosts are given, a create on each is refused naming its own. The native-owner launch would also give Linux what
  it lacks: the run host runs in the node's own process tree today, so Hex charges the VM's envelope by admission
  but cannot contain the run host per box, nor read its use on the host (§4).
- **Hex, once the macOS or Windows host is given:** the node there forwards a call on a box its host runtime
  doesn't hold to the guest's node (`hex-node` `recorded::forward_guest`), and an image VM's guest runs none, so
  that node must pass an image VM's calls back to the guest runtime, as the Linux node passes its exec back
  (`node/backends.rs` `guest_node_class`). No image VM exists on either host until then.

## 9. What would falsify it

- A run whose VM outlives a stop that `ImageGuest::stop` returned from (then a stop proves nothing, and every
  stopped box is held).
- A harness that needs Shards' own sizing rather than its admitted size (then `vm_size` is the wrong source).
- A digest-pinned image Shards' store cannot find whatever it is named (then pinning needs the name too).
