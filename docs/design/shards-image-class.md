# Decision (2026-11-12): the Shards class on Agentfile images under shards-init

Joint plan step 12 (kiln plan `01a1cd8e862e7169a074db3d5fa33f3d`, "Every kiln harness in Shards microVMs, Hex as
runtime"), AC-T7.4.1. Status: **built to Shards' D231 at `740fa372`**, on Linux, macOS and Windows hosts, its run
host started holding only its stdio by Shards' own spawn (D248, §1) and each vCPU's guest time in its usage (D241,
§4); its real-VM runs and the records its waits seed from are owed (§8).

Sources, read at the revisions named:
- Shards' D231 ("An image booted for an orchestrator", `docs/design/architecture.md` at `e0cc059c` and `740fa372`) and
  its research record `docs/research/runtime-image-boot-decision-v1.md` (`31b3f106b`, and at `e0cc059c`): the decision
  this class binds to, its Windows run host among it (731b5e54, 53277586). At `e0cc059c` beside it: D234 (a nested
  runtime's grant, `ImageSpec::nesting` and `ready_by`), D235 (a project plugged into a running member,
  `ImageGuest::plug` and `ImageRefusal::Plug`), D236 (destinations by name, `Narrowing::destinations`, `Bound::to`), the
  joint plan's step 5 (a member's stdin and working directory, `MemberSpec::stdin` and `workdir`,
  `ImageGuest::take_input`), D241 (the room an orchestrator's budget leaves the run, and the run's microVM cost in its
  stats, `Stats::microvm`), D242 (credentials granted by reference), D244 (a member's terminal), D246 (each member held
  to its reach of the names kept, `Narrowing::members`), D247 (the run's CPU rate), D248 (the run host started holding
  its three standard streams alone, through `shards_ipc`'s spawn), D249 (a run whose command never started ends failed,
  in its VM's words) and D250 (a run that keeps nothing past the microVM keeps the network its image links its members
  to, its network process letting nothing past), D251 and D252 (a lane to a socket of the run's own command, and the
  run's own agent-control endpoint, `ImageSpec::control` and `ImageGuest::control`), D234's `gpus=`
  (`ImageSpec::gpu_devices`) and D247's period (`ImageGuest::rate_period`): image channel version 17. At `740fa372`,
  versions 18 to 21: D256 (a member's run-time arguments, `MemberSpec::args`), D253 (a run's microVM restored from its
  image's template, or a warm run host's, `ImageGuest::started_from`, which a box's usage says and its record keeps;
  `ImagePools`, not used here), D242's leases (`ImageSpec::lessor`: none here, as no box grants a credential) and D257
  (the setup keys a run's spec may name), and D258 (a run's end saying why: its asks' end typed, its VM's failure to
  serve it told, its end kept once seen, and its run host's diagnostics kept whole in a file its orchestrator names,
  `ImageGuest::keep_diagnostics`).
- `shards_api::image` at `e0cc059c` and `740fa372`: `crates/api/src/image.rs` (`Images`, its `boot` and patience,
  `ImageGuest`, `ImageSpec`, `ImageRefusal`), `image/session.rs` (the run host's process, `shards_ipc::spawn_with`,
  `stop`, `wait_for`, `stats`) and `image/wire.rs` (`Stats`, `Microvm`, `GuestTimes`, `StatsScope`, `RunEnd`,
  `Narrowing`, `MemberReach`, `Bound`); `crates/shards/src/api_run.rs` (how a narrowing keeps grants; the STATS it
  answers).
- Shards' guest-cpu-time-v1 (221c7377, 6396a78b; its record
  `docs/research/measurements/guest-cpu-time-v1/README.md`): each vCPU's guest time, given to the daemon's runs, and
  through D241 to D231's.
- Windows handle inheritance: `CreateProcessW` (Microsoft Learn, processthreadsapi.h: `bInheritHandles` TRUE hands the
  child every inheritable handle of its parent), `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` (`UpdateProcThreadAttribute`),
  Microsoft KB315939 ("PRB: Child Inherits Unintended Handles During CreateProcess Call"), and Rust's std
  (`library/std/src/sys/process/windows.rs`, alike in 1.94.1 and the 1.100 nightly of 2026-09-04, so in Hex's
  1.99.1 between them: `CreateProcessW` with `inherit_handles` TRUE and no handle list unless the caller gives one;
  its `CREATE_PROCESS_LOCK` orders std's own spawns, citing KB315939).
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
  inside; `shards-guest.md`), and so does the `oci` class on macOS and Windows. Hex's PID 1 is kept for good (§7).
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
- **Hosts** (`hex_runtime::guest::host_images`). Linux, macOS and Windows boot image VMs (Ada, 2026-11-12: the class
  works on Windows and macOS too). Every program Hex starts holds only its stdio (CVE-2024-21626's class), and so does
  D231's run host (`shards api-run`), which shards-api started through std's `Command` until D248, handing it what
  another node thread held inheritable at that moment, for as long as its VM ran:
  - macOS: std's spawn passes no `POSIX_SPAWN_CLOEXEC_DEFAULT`, and macOS's pipe, socket, socketpair and accept take
    no close-on-exec flag, so the run host would hold every descriptor another thread had made and not yet marked (a
    box's pipe, a client's connection, the box helper's socket). Hex's own red: 1341adc6, a tool std started held the
    parent's descriptors (`macos/directory.rs`).
  - Windows: std's `CreateProcessW` passes `bInheritHandles` TRUE with no handle list, so the run host would inherit
    every handle the node holds inheritable then: a member's stdio, which Hex marks inheritable for that member's own
    `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` as it starts it (`windows::sys::create`), and the node's own inherited stdout
    and stderr. A pipe end so held keeps its reader from its end while the image VM runs; std's lock orders std's own
    spawns alone. Hex's own red: e14ff1b2 on jupiter (r124, 2026-11-09), a keeper std started held the node's stdout
    and stderr, so they didn't end with the node (fixed for the keeper by 6f4b8f7a).

  Until 2026-11-12 a create on either was refused `Awaits(RunHostStdioOnly)`. The remedy was Hex's change to its
  vendored Shards (vendor/patches/shards 0001, `image/spawn.rs`), upstream since as D248 (Shards 9bdd4a7ec), so Hex
  carries none: shards-api starts the run host through `shards_ipc::spawn_with`, the spawn Shards starts its own
  processes with, holding only the three ends named for it (`POSIX_SPAWN_CLOEXEC_DEFAULT` on Apple's, CreateProcessW
  with a `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` on Windows), and `a_run_host_holds_no_end_it_was_not_given` holds a run
  host to it. The alternatives were refused: boot image VMs on std's spawn anyway (it breaks the stdio-only rule), or
  serialize Hex's member starts behind image boots (a boot takes seconds, and every start would wait on it).
  Beside it, every descriptor the macOS runtime makes is close-on-exec as it is made
  (`every_descriptor_the_runtime_makes_is_close_on_exec`), so a spawn that does inherit holds none of the node's.
- **The guest's kernel and init** (`guest_pair`; ROOT, 2026-11-12). D231's run host inherits the node's environment
  and boots the guest its `SHARDS_KERNEL` and `SHARDS_INIT` name, taking each only as a UTF-8 absolute path and
  dropping any other value without a word, both or neither: the daemon refuses one alone ("SHARDS_KERNEL and
  SHARDS_INIT go together"; Shards e0cc059c, `api_run.rs` `as_run`, `engine.rs`). `host_images` reads the pair once,
  as the node starts: one alone, or a value the run host would drop, makes the class unavailable on that node, every
  create refused `Platform` naming both variables before any record, where otherwise every start would be refused
  after its create was recorded (ROOT's r345), or its VMs boot Shards' own guest in place of the one named.
- **The node on macOS and Windows** (a guest runtime beside its host runtime, `WithGuest`) sends a call on a box its
  guest holds to that guest's node, but an image VM's: its guest runs no node of Hex's, so the guest runtime here
  answers it (`hex-node` `recorded.rs` `forward_for`), as the Linux node runs an image VM's exec here
  (`node/backends.rs` `guest_node_class`).

## 2. Mapping onto D231

D231's `boot` is create and start in one, and returns once the members have started. Hex keeps the runtime-spec's
split, so a box can be admitted and recorded before it is needed:

| Hex | D231 |
|---|---|
| `create` | No call. The box's record, `Creating`: admitted and named, nothing booted. |
| `start` | `Phase::Ready` written first (the boot's intent, before any effect), then `Images::boot`, given the boot's learned bound as its patience (§6). |
| `signal(KILL)`, Hex's stop | `ImageGuest::stop`: the VM process killed at once; it returns once the run's end, `RunEnd::Stopped`, is told, its VM gone, and the run host then exits on its own. The exit, `Signal(9)`, is recorded then. |
| `signal(other)` | Refused `Awaits(MemberSignal)`: a member's signal is the agent-control endpoint's (step 11). |
| `exec` | Refused `Awaits(MemberExec)`: a member's exec is the agent-control endpoint's (step 11). |
| `inspect`, `list` | The record's phase and exit, and this process's run (`wait_for` with no wait). |
| `usage` | `stats(StatsScope::Run)` (§4). |
| `delete` | `Deleting`, then `stop` if it runs, to its run's end, then the record removed. |

Every member operation D231 doesn't give (a terminal, exec, signals) is refused typed, naming the capability
(`Refusal::Awaits`), never stubbed. A member's own streams are the joint plan's step 17 (a kiln session's stdio as its
harness member's):
- `ShardsImage::members` names the members the box's caller drives (each its `Member`, variables over its image's
  and a working directory, checked as an exec's variables and as D231's absolute, plain `workdir`; each named once).
  The boot gives each a `MemberSpec` with `stdin` (step 5's pipe the orchestrator writes) and no terminal; the
  image's other members run as it declares them, their stdin `/dev/null`.
- `Runtime::member(id, member)` hands the caller that member as a `Proc`: the caller's ends of its stdin, stdout and
  stderr (D231's `take_input` and `take_output`, each once, so a second call is refused as Shards refuses it), and
  its end (`wait_member`; `WithRun` is the run's end, as the box's own). It is no process of this host
  (`on_this_host` false, no pid: D231 names none), and its own kill awaits the endpoint (`MemberSignal`); the box's
  KILL or delete ends it with its VM. Refused for a member the spec doesn't name, and as every call is for a box that
  isn't running here. The class serves it in process (`GuestRuntime`, `WithGuest`, `Recorded`, the Linux `Node`)
  and the simulator as its echo program, and the node's wire carries it to a caller elsewhere: `Call::Member` (kind
  `0x0017`), sent by `hex-client`'s `Runtime::member` as an exec asking for its streams is. On a local socket the
  node gives the member's own ends with its answer (`Answer::Given`, descriptors); a QUIC client makes its streams
  itself, so it carries them (lanes) and the node joins each to the member's own, a copy on a thread of its own to the
  source's end (`serve.rs` `joined`). The member has no pid here for an OS watch, so a thread of the node's waits
  for its end and tells it as a remote program's (`MemberEnd`); its release under the same lock clears an end told
  before its start was registered, and its own kill is refused `Awaits(MemberSignal)` as in process.

What D234 to D236 add is asked for as Hex holds it: no nesting grant (`nesting: false`, no `ready_by`: no harness
is a nested runtime, step 39's), the destinations a box's grants name kept and each member held to its own reach
(network-grants.md N4), and a plug's refusal, which only a plug's call could bring and this class makes none,
refused `Broken`, naming it.

`RunEnd` becomes the box's exit as Shards reports it: `Exited(status)` is `Exit::Code(status)` (`docker run`'s
status, 128 plus a signal's number where one ended it, kept as the number it is rather than guessed back into a
signal), `Stopped` is `Exit::Signal(9)` (HEX-RT-12's KILL), `Failed` is `Exit::Unknown`: a VM, host or guest that
failed, or, since D249, a command that never started, which Shards no longer reports as the status a container keeps
for it (HEX-IMG-3 read `Code(128)` of a command that never ran).

**Grants (goal 15).** The boot's `publish` and `egress` come from goal 15's `program(&grants)`
(docs/design/network-grants.md N4): the publishes its grants name, and none of the image's own grants past the VM
(`Keep::Only` empty for egress, MCP and destinations, no DNS): default deny. D231 can only narrow the image's own
grants, never widen them. A run so narrowed of an image that links its members past the microVM keeps that network,
its network process letting nothing past (D250, Shards 3a16dbd92); before D250, Shards refused it "cannot run without
a network", which refused every Hex create of such an image (Codex's).

## 3. States

The record (the guest's Books, the node's Store) and this process's run:

| State | Record | Run | Leaves by |
|---|---|---|---|
| Created | `Creating`, no exit | none | start: Booting; delete: removed |
| Booting | `Ready`, no exit | a boot in flight, on a thread of its own | its answer: Running, or Ended where Shards refused the run (past its patience among it) |
| Running | `Ready`, no exit | the run held | KILL, or its own end: Ended; delete: Deleting |
| Ended | `Ready`, its exit | none | delete: removed |
| Deleting | `Deleting` | stopping, or none | its stop's return, its run's end: removed |
| Held | `Ready` or `Deleting`, no exit | none: a run of an earlier node process | a host reboot (§5) |

A call on a Booting box waits on nothing: it answers what is known (`Created`'s inspect, `Lifecycle { Pending }`
for a stop or a delete), as `LifecycleCause::Pending` says. A Held box is refused `Awaits(AttachAfterCrash)` by every
call, recovery's adoption among them, and is listed running: Hex can't prove it isn't.

## 4. Usage

`stats(StatsScope::Run)` reads the run's cgroup in the guest: `cpu_ns` becomes `cpu_usec`, `memory_bytes`,
`pids`, `read_bytes` and `write_bytes` their counters. **Guest time per vCPU** is D241's: the run's stats carry its
microVM's cost (`Stats::microvm`), each vCPU's guest time among it, which the run host asks its VM (`GUEST_TIME`, as
the daemon asks it for its own runs, guest-cpu-time-v1). `GuestTime::PerVcpuNs` carries it: nanoseconds each vCPU ran
the guest since the VM booted, by vCPU index, one for each vCPU of the VM's size, each only growing, none past the
wall time the VM ran (`usage_of`, `per_vcpu`). A part the run host did not read is refused, naming why, never read
as none (D241's rule): no microVM part, no guest time, a vCPU the read missed (Shards then keeps the most it read
before, which no longer says the vCPU's time now), a count other than the VM's size, or time of vCPUs the VM no
longer has. Its wire bound, `GuestTime::MAX_VCPUS`, is every 8-bit vCPU index: a Shards VM numbers its x86_64 vCPUs
by 8-bit APIC id (at most 255) and has fewer on arm64 (123).

Goal 14's learned charge (`hex-node` `node/charges.rs`) reads the cgroup of each VM the node launched through its
native owner (`Supervisor::running_vms`): its VMM's use on the host. An image VM is none of those, its run host
running in the node's own process tree (§8), so the learner neither learns nor places it, and it stays booked at its
receipt, whole. Its Usage above is its run's in the guest, short of the guest kernel's and the run host's own, and
is no stand-in for the host's. The run host's launch through the client's native owner gives the learner its cgroup.

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
| `image_stop` | `stop`'s return: its run's end told, its VM gone (the run host exits after, on its own) | owed |

D231's boot waits at most the patience its caller gives: past it, its run host is killed and the boot refused
(`ImageRefusal::Boot`). Hex gives it the boot's learned bound (`Duration::MAX` while that bound measures, as in the
census), so no boot holds a VM, a run host and a box Booting past it. Hex makes the boot on a thread of its own and
bounds its caller's wait by the same bound; past it the call is refused `Lifecycle { Pending }` and the answer, when
it comes, is recorded: a run, or Shards' refusal at its own bound, whose clock starts after the caller's, once its
run host is started. D231's stop takes no patience and waits for its run's end, which its kill makes; Hex makes it
on a thread of its own too and bounds its caller's wait the same way. No record exists yet for any of them, and a
measure no class records is refused (`hex_client::guest::shards::recorded`): **a create is refused until all three
are recorded**, so no box exists that a wait of its own could not bound. The one exception is the waits census
(`hex_node::census`), which arms no wait: there each of the three measures (`ImageWaits::recorded`,
`hex_core::patience::measures_all`).
`crates/hex-cli/tests/image_record.rs` takes the three records on a host class: a harness image booted, its stats
read and stopped n times through D231 as this class does, each series' first boot an upgrade's (a fresh cache), its
rows and page written for records.tsv. `crates/hex-cli/tests/image_real.rs` runs the class's cases (`IMAGE_CASES`)
through a real `hex node` in the census, with each wait's answers printed.

## 7. Hex's PID 1 is kept

Hex's own guest path is permanent (the person, 2026-11-13: "we are not getting rid of Hex's PID1 which lets it work
with basically anything non-Shards. We are to have init work *with* that."). Nothing of it is retired:

- `hex` as PID 1: `crates/hex-cli/src/guest.rs`'s init and the relay (`hex guest relay`, `crates/hex-node/src/relay.rs`);
- the guest image Hex builds (`scripts/guest/build.sh`, its initramfs) and `GuestConfig`'s kernel and initrd;
- the vsock channel's ports (`CONTROL_PORT`, `STDIO_PORT`, `FILES_PORT`, `STAMPS_PORT`) and the staging of roots
  into the guest (`hex_client::guest::stage`, `rewrite`);
- `ClassSpec::Shards(OciSpec)`'s route through Hex's own guest node, the shared and dedicated guests alike.

Step 39 makes shards-init work with it: Hex's node under shards-init, with Hex's own PID 1 kept
(`node-under-shards-init.md`). Its acceptance is Hex's conformance suite passing through shards-init on macOS and
Windows with Hex's PID 1 intact, and Hex's own PID-1 path still passing on its own.

This class stays beside both: it is how a box boots an Agentfile image's members under shards-init.

## 8. Owed

- **Records** for `image_boot`, `image_stats` and `image_stop` on each host class, Linux-KVM, macOS-HVF and
  Windows-WHP (ROOT's measurement, `image_record`): `Images::boot` of a harness image to its return, `stats(Run)` to
  its answer, `stop` to its return, each `n` with its slowest, added to `bench/native/guest/records.tsv` with their
  RECORD.md (bench/native/guest/image/). Until then a create on that host class is refused outside the waits census
  (§6).
- **Real-VM runs** (ROOT): the image class's conformance cases (`IMAGE_CASES`) on a node with Shards configured, on
  Linux KVM (`crates/hex-cli/tests/image_real.rs`), macOS HVF and Windows WHP, each with a run host at Shards
  `740fa372` or later (image channel version 21: Hex's `shards`, rebuilt on each host by
  `scripts/shards-gpu/build.sh`), as a run host and the vendored API of different versions refuse each other at
  create.
- **From Shards:** `attach` or the run's identity after an owner's crash (`AttachAfterCrash`), member exec and
  signals (`MemberExec`, `MemberSignal`; step 11), STATS' memory peak, OOM kills and refused forks, and Windows
  arm64's run host (unbuilt, D231). Given since 482c5d78: the Windows run host (`WindowsRunHost`, 731b5e54), a boot
  patience (`Images::boot`'s), guest time per vCPU in the run's stats (D241), the run host holding only its stdio
  (D248, Hex's 0001 upstream), each member held to its reach (D246, Hex's 0002 upstream) and a never-started
  command's end in its VM's words (D249). The native-owner launch would also give the run host a scope of its own:
  it runs in the node's process tree today, so Hex charges the VM's envelope by admission but cannot contain the run
  host per box, nor read its use on the host (§4).

## 9. What would falsify it

- A run whose VM outlives a stop that `ImageGuest::stop` returned from (then a stop proves nothing, and every
  stopped box is held).
- A harness that needs Shards' own sizing rather than its admitted size (then `vm_size` is the wrong source).
- A digest-pinned image Shards' store cannot find whatever it is named (then pinning needs the name too).
- A run host found holding a descriptor or handle it was not given (then D248's spawn is not enough, and the
  macOS and Windows creates wait again).
