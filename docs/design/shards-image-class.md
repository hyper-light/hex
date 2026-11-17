# Decision (2026-11-12): the Shards class on Agentfile images under shards-init

Joint plan step 12 (kiln plan `01a1cd8e862e7169a074db3d5fa33f3d`, "Every kiln harness in Shards microVMs, Hex as
runtime"), AC-T7.4.1. Status, 2026-11-13 (built, merged and live apart; live is GitHub's main, `025aae6d`): **built to
Shards' D231**, on Linux, macOS and Windows hosts, its run host started holding only its stdio by Shards' own spawn
(D248, §1) and each vCPU's guest time in its usage (D241, §4). Live at Shards `1389dc0b` (`68ed140a`, `2c728252`).
Merged on Hex's local main at Shards `f17b8fb77`, image channel v41 (built on `cp/t7-v41-swaps`, not pushed: a member's
component swapped in and its image's own back as its run runs, D270 slice 4, v40, `Runtime::swap`, `restore` and
`components`, with its members' generations, slice 3b, v38, §11; whether a share's files may be run, D279, v41,
`ImageShare::exec`; and two this class asks none of, a share given its members as they start, D277, v37, and a member's
own reach past its microVM within its ceiling, D278, v39); `9975d1f55` before it, image channel v36 (an orchestrator's run told the memory it may plug, as a
daemon's is, D275: harness members are no longer ended at their boot share, a member past its run's room still held at
the run's hold (D275's share within a room held back); an HVF vCPU taking memory plugged after it as memory, D274; live volumes, `ImageGuest::plug(&Plug)` and
`ImageSpec::plug_roots`, D271, v35, and a run's grants past its microVM changed as it runs, D273, v36, neither of which
this class asks; a run's published set its live set, D269, v34, its network refusals typed `ImageRefusal::Net`; pause and
resume, D267, v33; the warm pools' room, D253, v32; a VM's charge, `HostUse::charged`, D265; `d993d83ee` before it,
image channel v31: Shards' console exported, `shards_api::CONSOLE`, D261, which
Hex's guest line names; a host's check before any boot, `shards_api::host_check`, D262; `888879412` before it, a run whose
guest init failed after its command started
ends `RunEnd::Failed` in init's words, never `Exited(128)`, D249; the init capture race of 5231d8279 fixed; a VM process's
waits name their bound, a rate floor or stillness; `fa953e3ce` before it, a GPU report taken with no clock, `Until::Released`, its VM
process's end refusing it `GpuProbe(Ended)`; a VM's and a network process's names their own, `PR_SET_NAME`; Windows'
randomness by `ProcessPrng`, no `bcrypt.dll`; `0290f1e9f` before that, a rate's hold period carried with each rate's
answer, none before the first, derived per limit from the granule Shards learned, D191's migrate-09; `9d8a05f6a` before
that, v30, a
run host's diagnostics held within a bound, `keep_diagnostics(to, bound)`, and a managed guest's VM process named,
`vm_process`; `d7ff8a094` before that, a member's
stdio read and written without waiting, and an exec's hangup; `2246a8829` before it, at `d91ebb32` and `d7179108`, every socket a run's agent-control endpoint holds
told, its waker and wake in inspect; `2737fc4ec` before it, v29, at `20015fb5`, a run's stop returning
released, so a delete frees nothing the run still holds, the review's B-4; `ce80cbb37` before that, at `9ff60efb`, a run's end with its microVM's final
use, of which the class reads the end alone yet, a member's lane asks waited within their W* and followed past it, told,
and no GPU report asked; `16451e19f` before that, v27, at `cf844022`, a member's end typed, a member a signal killed
`Exit::Signal(n)`, never its 128-plus status; `37ddeb080` before that, v26, at `2f55dccf`, no upstream root named;
`097ec0047` before that, v25, at `1314e5f2`, a member's MCP bridge refused by its field while the class asks no agent-control endpoint with
`2f5d20e7`; `3114e082f` before that, at `8dfe6df8`, a delete stopping a
boot under way with `31c651fb`; `db4168345` before that, at `1d60abad`, each credential grant's placeholder with `1c4d11b3`; `740fa372` before
that, at `8aa7c028`; `e0cc059c` before that, at `c8b4ce6b`), with a failed run's end in its
runtime's words (`32b66a10`, `94d519d6`), the class whole and its host use read to its end (`ae7038d7`, `2a8bf6be`), a
member's arguments and the VM's shares (`cf535152`), and `NoSuchBox` where a shared guest holds no record (`fac528d9`).
On real VMs, HEX-IMG-1 to 7 passed on Linux KVM at `c8b4ce6b` and `8aa7c028`, and HEX-IMG-2 to 5 on macOS HVF at
`cf535152`. Owed (§8): HEX-IMG-1, 6 and 7 on HVF at `fac528d9` or later, and every case on Windows WHP. Built on
`cp/t7-p5-5b-pause-v2`, not merged: a box's pause and resume through its node (kiln plan P5.5b, D267; §10).

Sources, read at the revisions named:
- Shards' D231 ("An image booted for an orchestrator", `docs/design/architecture.md` at `e0cc059c`, `740fa372`,
  `db4168345`, `3114e082f`, `097ec0047`, `37ddeb080`, `16451e19f`, `ce80cbb37`, `2737fc4ec`, `2246a8829`, `d7ff8a094`,
  `9d8a05f6a`, `0290f1e9f`, `fa953e3ce`, `888879412`, `d993d83ee`, `9975d1f55` and `f17b8fb77`) and
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
  `ImageGuest::keep_diagnostics`). At `3114e082f`, version 24: the sign-ins a run's injector captures
  (`ImageSpec::sign_ins`, none here, its pass-through goal 13's), and a stop for a boot under way (`BootStop`,
  `Images::boot_stoppable`, `ed1136305`), which a delete asks (§3). At `db4168345`, versions 22 and 23: each credential grant's placeholder (D242's step
  3, `credential-grants.md`) and D259 (the run's microVM saved whole for its owner, `ImageGuest::save`, which this class
  asks none of, so a refusal of a save is `Broken` here). At `097ec0047`, version 25: D254's MCP bridge, a member's
  connections paired with its controller's asks (`MemberSpec::mcp`, `af8f1ebe5`), which needs the run's agent-control
  endpoint, so this class, asking none, refuses a member asking one by its field (§2); each credential and sign-in
  grant's field held to the network process's wire (`shards_dns::credential::WIRE_MOST`, `17fc36ecd`), refused as a
  spec, `Invalid` here, Hex adding no bound of its own; D231's boot stop on Windows (`9a65eb9bb`); and D260 (a managed
  guest's use, `ManagedGuest::host_use`, `867053aa1`), which this class doesn't read. At `37ddeb080`, version 26: D242's
  upstream roots (`ImageSpec::upstream_roots`, `b6814785d`), none named here, so a run's injector trusts its host's
  roots alone (and, since then, no `SSL_CERT_FILE` or `SSL_CERT_DIR` on any OS); and an orchestrator's member lanes
  served by the run host (D238, `c1773281f`: `Execs::member`, a member's signals steered through init), which a
  member's signal is to ride. At `16451e19f`, version 27: a member's end typed (#72, `7aab838e0`,
  `MemberEnd::Signaled`), the signal that killed it apart from an exit's code. At `ce80cbb37`, versions 28 and 29: a
  run's end with what its microVM used in all (D260 amended, `1e2d403ec`: `RunEnded`, `FinalUse`, `Unused`), a member's
  channel's end typed (`ImageGuest::wait_channel`, `a3352f573`), the agent-control lanes' asks waited within their
  caller's patience (`155b5ee2e`: `PendingMember`, `MemberSteering::steered`), and a run's GPU report (D233,
  `fad757af4`: `ImageSpec::gpu_report`, `ImageGuest::take_gpu_report`), which the class asks of no run. At `2737fc4ec`:
  a run's stop returning released (D231, `c133bfb56`: its run host exited and reaped, every process it started gone).
  At `2246a8829`, version 30: every socket a run's agent-control endpoint holds (D252 amended, `d3950c7d1`:
  `ControlAt::waker`, `wake`, `sockets`).
- `shards_api::image` at `e0cc059c`, `740fa372`, `db4168345`, `3114e082f`, `097ec0047`, `37ddeb080`, `16451e19f`, `ce80cbb37`, `2737fc4ec`, `2246a8829`, `d7ff8a094`, `9d8a05f6a`, `0290f1e9f`, `fa953e3ce`, `888879412`, `d993d83ee`, `9975d1f55` and `f17b8fb77`: `crates/api/src/image.rs` (`Images`, its `boot` and
  patience, `ImageGuest`, `ImageSpec`, `ImageRefusal`), `image/session.rs` (the run host's process,
  `shards_ipc::spawn_with`, `stop`, `wait_for`, `stats`) and `image/wire.rs` (`Stats`, `Microvm`, `GuestTimes`,
  `StatsScope`, `RunEnd`, `Narrowing`, `MemberReach`, `Bound`); `crates/shards/src/api_run.rs` (how a narrowing keeps
  grants; the STATS it answers).
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
- **Its run host's diagnostics, whole** (D258's `ImageGuest::keep_diagnostics`; ROOT, 2026-11-13). A box's start
  names `state/image-diagnostics/<box>.log` in its record with its boot's intent and makes the file before the boot;
  the booted run's host writes every part of its diagnostics, its VM's among them, there from then on, after what it
  kept until then (a run whose host can't is dropped, which ends it, and its boot is refused in those words). The file
  is bounded by the disk the box's allocation was admitted with (ROOT, 2026-11-14; the review's B-9): the diagnostics
  are the box's own output on the node's disk, so they are charged to it, and no number is picked. Past that bound the
  file is cut back to the tail Shards keeps for quoting, and its run host's words go on after it, through Shards'
  bounded `keep_diagnostics(to, bound)` (`9d8a05f6a`); a box admitted with no allocation has no disk to bound its file
  by, and it has none. A box admitted with less disk than that tail (an image or node VM whose root is in its RAM is
  admitted with none) holds the tail alone, Shards' own bound, its longest report of a VM's end, which Shards' refusal
  of a shorter bound names (the KVM suite at 27cdafe0c found such boxes refused; owed: the tail counted in the class's
  per-VM cost). A failed run's `BoxInfo.ended` carries its words, then the file's last bytes, as many as Shards
  quotes of a VM's end (`shards_abi::bounds::console_said` over the kernel's console records, as the dedicated guest
  reads them). The node Store keeps the file's name, the run's
  words and how its microVM started through the VM adapters' lifecycle writes (`Store::put_lifecycle`), and the
  box's delete removes the file before its record.

## 2. Mapping onto D231

D231's `boot` is create and start in one, and returns once the members have started. Hex keeps the runtime-spec's
split, so a box can be admitted and recorded before it is needed:

| Hex | D231 |
|---|---|
| `create` | No call. The box's record, `Creating`: admitted and named, nothing booted. |
| `start` | `Phase::Ready` written first (the boot's intent, before any effect), then `Images::boot`, given the boot's learned bound as its patience (§6). |
| `signal(KILL)`, Hex's stop | `ImageGuest::stop`: the VM process killed at once; it returns once the run is released (Shards `2737fc4ec`): its end, `RunEnd::Stopped`, told, its run host exited and reaped, and every process the host started (its VM, network and share processes) gone, so a delete frees nothing the run still holds (the review's B-4). The exit, `Signal(9)`, is recorded then. |
| `signal(other)` | To every member of the run that runs, each on its own lane of the run's agent-control endpoint (D238, D252; goal 2's item 2): the run asks the endpoint of the class's own key and no socket lane, and its controller is dialed as the first signal goes, kept for that run. The VM runs on; a member that has ended is skipped; a run none of whose members can take it is refused `NotRunning(id, "signal: its run has no member a signal reaches")`. |
| `exec` | Refused `Awaits(MemberExec)`: a member's exec is the agent-control endpoint's (step 11). |
| `inspect`, `list` | The record's phase and exit, and this process's run (`wait_for` with no wait). |
| `usage` | `stats(StatsScope::Run)` (§4). |
| `pause`, `resume` | `ImageGuest::pause`, `resume` (D267; kiln plan P5.5b, §10): its vCPUs held out of its guest by its VMM, or let go, one move at a time, refused typed where the box isn't as the move asks. |
| `delete` | `Deleting`, then `stop` if it runs, to its run's end, then the record removed. |

Every member operation D231 doesn't give (a terminal, exec, signals) is refused typed, naming the capability
(`Refusal::Awaits`), never stubbed. A member's own streams are the joint plan's step 17 (a kiln session's stdio as its
harness member's):
- `ShardsImage::members` names the members the box's caller drives (each its `Member`, variables over its image's,
  a working directory and run-time arguments, checked as an exec's variables, as D231's absolute, plain `workdir`,
  and as no command line's argument holds a NUL; each named once). The boot gives each a `MemberSpec` with `stdin`
  (step 5's pipe the orchestrator writes), its arguments after its image's command for it (D256's `args`, kiln's
  step 17: a harness's session flags, `--resume` and `--model`), its MCP bridge as asked (D254's `mcp`) and no
  terminal; the image's other members run as it declares them, their stdin `/dev/null`. A member asking an MCP bridge
  has it: every run asks its agent-control endpoint of the class's key (goal 2's item 2), over which the bridge's
  connections reach the run's controller (D254; `Runtime::mcp`, below).
- `ShardsImage::shares` names the host directories the guest sees (D231's shares, `-v HOST:AT[:ro]`; kiln's step
  18: a person's home volumes and a project at its host path, D235), each a `Mount`: its host directory, checked as
  a mount's source; where the guest sees it, absolute, no `..`, never the guest's root, each seen once; and whether
  read-only, and whether its files may be run (D279, §11). The boot asks Shards for exactly those, in order (default
  deny: the image's grants widen none), and a share Shards refuses refuses the start as `Invalid`, in Shards' words. A
  member gets one where its image's `VOLUME` at that path is for it (D109), the run's own command where no `VOLUME`
  declares it.
- `Runtime::member(id, member)` hands the caller that member as a `Proc`: the caller's ends of its stdin, stdout and
  stderr (D231's `take_input` and `take_output`, each once, so a second call is refused as Shards refuses it), and
  its end (`wait_member`; `WithRun` is the run's end, as the box's own), each its current generation's (D270, §11). It is no process of this host
  (`on_this_host` false, no pid: D231 names none), and its own kill is KILL on its own lane of the run's endpoint,
  its end then `Signal(9)` (a member that has ended has nothing to end); the box's KILL or delete ends it with its VM. Refused for a member the spec doesn't name, and as every call is for a box that
  isn't running here. The class serves it in process (`GuestRuntime`, `WithGuest`, `Recorded`, the Linux `Node`)
  and the simulator as its echo program, and the node's wire carries it to a caller elsewhere: `Call::Member` (kind
  `0x0017`), sent by `hex-client`'s `Runtime::member` as an exec asking for its streams is. On a local socket the
  node gives the member's own ends with its answer (`Answer::Given`, descriptors); a QUIC client makes its streams
  itself, so it carries them (lanes) and the node joins each to the member's own, a copy on a thread of its own to the
  source's end (`serve.rs` `joined`). The member has no pid here for an OS watch, so a thread of the node's waits
  for its end and tells it as a remote program's (`MemberEnd`); its release under the same lock clears an end told
  before its start was registered, and its own kill is its host's `signal_member(KILL)` on one of serve's workers, as
  every call is (no thread of its own; ROOT, 2026-11-14), told to its caller whatever becomes of it, a member that no
  longer runs having nothing to end, as in process.
- `Runtime::signal_member(id, member, signal)` sends one member alone a signal on its own lane of the run's endpoint
  (goal 2's item 2), KILL among them: the member's, never its VM's, which runs on. Refused `NotRunning(id, "signal a
  member: it has ended")` for a member that has ended, `NotRunning(id, "signal a member: it runs no program")` for
  one of files alone, `NotRunning(id, "signal a member")` where the run isn't running, and `Invalid` for a member the
  run declares not. The node's wire carries it (`Call::MemberSignal`, kind `0x001e`, appended), sent by `hex-client`'s
  `Runtime::signal_member`; the simulator's member its caller took ends by it, its echo keeping no handler.
- `Runtime::mcp(id, member)` (goal 2's item 3; ROOT's mapping, 2026-11-14) takes the next connection a member's harness
  makes to its MCP bridge (D254: `Execs::mcp`, a lane of the run's endpoint) as a `Proc`: its stdout what the harness
  sends, its stdin what it is sent, given once, and no stderr; its end the connection's (`ExecControl::wait`), or
  `Signal(9)` once killed. Its ask waits within `hex.image.mcp`'s W* and past it follows the run's connection, however
  long the harness takes to connect (§6). Refused, typed, in this order: `Invalid` where the spec asks the member no
  bridge, before anything of its run; Pending while it boots, held, and `NotRunning(id, "mcp")` before its start and
  once its stop has begun; `Invalid` for a member its run declares not; `NotRunning(id, "mcp: its member takes no MCP
  connection")` where Shards says the member takes none (`Unstarted::Refused`); `NotRunning(id, "mcp: its run
  ended")` where the run's connection ends first; and any other of Shards' faults `Os`, in its words. Two threads of its
  own pump a connection, one each way, for as long as it lasts, Shards' MCP lane giving no read or write that doesn't
  wait (`9f5b1f754`'s `try_read` is its socket lanes' alone): a connection's cost, as a stream's copy is one thread
  (the review's D-10). Its kill aborts what the harness is sent (ECONNRESET), the lane having no hangup of its own at
  Shards `2737fc4ec`, so a kill made while a record waits for the harness to take it goes once the harness takes it, or
  the run ends (ROOT's asks of g121, 2026-11-14: a hangup for an exec or MCP lane, and `Output` and `Input` that don't
  wait). The node's wire carries it (`Call::Mcp`, kind `0x0021`, appended after A-10's `Reserved`), sent by
  `hex-client`'s `Runtime::mcp`, its stdin and stdout asked for, timed by a box kind of its own
  (`client.answer.mcp.<class>`). The node serves it as `Call::Member` but for its wait (ROOT, 2026-11-14): its runtime's
  `mcp` waits on a thread of its own (`hex-node mcp`), never one of serve's workers, which the node's other calls and its
  own jobs run on (a program's collect, a member's kill, an abandoned box's count, a list's parts); the call stays in
  flight until it is answered, so the in-flight bound bounds these threads; and it is admitted from the budget's shared
  part alone, never the connection's kept call (`KEPT`, whose reason is that a connection can always make one call, so a
  connection waiting on its bridges can always stop, delete or inspect), refused `Exhausted` where that part is spent.
  The same thread then waits for the connection's end and tells it as a remote program's; a remote kill (`Call::Kill`,
  the disconnect rule) reaches it through its runtime's kill apart from it (`Proc::killer`), on a thread of its own
  (`hex-node mcp kill`). Its costs: a thread for each MCP call from its ask to its connection's end, the image class's two
  pumps for each connection, and over QUIC a copy thread for each stream it carries (`joined`); a wait whose connection
  closes waits on until its harness connects or its run ends, its place in flight held, and the disconnect rule then ends
  what it took. At the next revendor (Shards `9f5b1f754`'s `Dialed::wake`) the pending wait moves onto the node's event
  loop (`PendingMcp::wait(Duration::ZERO)` after each ring) and holds no thread; an open connection's pumps keep theirs
  until Shards gives `Output` and `Input` forms that don't wait.

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
| Booting | `Ready`, no exit | a boot in flight, on a thread of its own | its answer: Running, or Ended where Shards refused the run (past its patience among it); delete: Deleting, its boot followed to its answer |
| Running | `Ready`, no exit | the run held | KILL, or its own end: Ended; delete: Deleting; pause: Paused |
| Paused | `Ready`, no exit | the run held, its vCPUs held out of its guest or a move of them under way (§10) | resume: Running; KILL, or its failure: Ended; delete: Deleting |
| Ended | `Ready`, its exit | none | delete: removed |
| Deleting | `Deleting` | stopping, or none | its stop's return, its run's end: removed |
| Held | `Ready` or `Deleting`, no exit | none: a run of an earlier node process | a host reboot (§5) |

A call on a Booting box but a delete waits on nothing: it answers what is known (`Created`'s inspect, `Lifecycle {
Pending }` for a stop), as `LifecycleCause::Pending` says. A delete answers once the box is gone, never Pending (kiln's
choice (b), ROOT 2026-11-13), so its caller needs no retry: its intent written (`Deleting`), it stops the boot (D231's
`BootStop`, Shards `ed1136305`: its run host killed mid-boot, with its process group, or on Windows its job) and follows
the boot to its end, `Ended(Stopped)` once that host is reaped and every holder of its stderr has closed it, or its
run's start where that came first, as decision 9 has a wait follow its party, by a wait of its own,
`hex.image.delete_boot` (past its W* the run-out is told under that name, the boot it ends still under way, and the
delete waits on, refused only where the boot's thread ends with no answer, as the node's broken state in those words,
its record kept), stops a run that started and follows its stop to its end however long past the stop's W*, then removes
the box's diagnostics and record. `delete_with_host_use` answers the run's final use of its host, read as it ran before
its stop, or none where its run never started (kiln charges nothing then); a delete retried then is NoSuchBox. A run
that started is never answered none (the review's B-5): where its final use can't be read, its run having ended, or
being stopped, before the delete (Shards' stats answer only a running run), or its read refused, the delete is refused,
typed (`NotRunning`, or the read's own refusal), the box as it was. A stopped
boot's time is no sample of the boot's W*: it is the time to its stop, no boot's answer. A caller waits for a start past
W* with `Runtime::started` rather than polling (kiln's HexVms adapter, ROOT 2026-11-13): a box still booting is followed
to its boot's answer as a delete follows it, by a wait of its own, `hex.image.started` (refused only where the boot's
thread ends with no answer, the boot it follows still under way), then answered as inspect answers it, Running, or as
its run's own failure in its runtime's words (`Refusal::Failed`), as `start` answers a run that failed, or, for a run
whose publishes break its grants, which the call that took its boot's answer stops (network-grants.md N6), the grants'
refusal once that stop has ended, as `start` answers it; a box not booting is answered at once. A Held box is refused `Awaits(AttachAfterCrash)` by every call, recovery's adoption among
them, and is listed running: Hex can't prove it isn't.

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

**Paused time** is D267's: the run's stats carry how long its owner has held it paused since it started (`Stats::paused`,
every pause so far and the one under way among it), which `Usage::paused_ns` and `HostUse::paused` carry, refused,
naming why, where its run host told none, never read as none (§10).

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

Each wait is learned through hex-core's `Patience` (decision 9; `ImageWaits`): the slowest answer this process has
seen, a bound only while answers come within it (the person's ruling of 2026-11-13); before its first answer, and from
a run-out to the next, it follows its party until the answer comes. No record is read, and none is needed to make an
image box: `ImageWaits::new` measures from nothing.

| Wait | What it waits for | Its party |
|---|---|---|
| `hex.image.boot` | `Images::boot`'s answer: the run started, or the refusal | the boot's thread, D231's run host |
| `hex.image.stats` | `stats(Run)`'s answer | the run |
| `hex.image.stop` | `stop`'s return: its run released, its end told, its run host and every process it started gone (Shards `2737fc4ec`) | the stop's thread, the run host |
| `hex.image.delete_boot` | a delete's ask to the boot of the box it deletes answering, its run started or its host ended (§3) | the boot's thread |
| `hex.image.started` | a `started` call's ask to the boot of the box it asks answering (§3) | the boot's thread |
| `hex.image.member` | a member's own lane's first word (D238): whether it runs, its steering then | the run's agent-control connection |
| `hex.image.signal` | a signal's answer on a member's own lane: taken, or refused | the run's agent-control connection |
| `hex.image.mcp` | a member's harness's next connection to its MCP bridge (D254), from its ask, which may be never | the run's agent-control connection |

D231's boot waits at most the patience its caller gives: past it, its run host is killed and the boot refused
(`ImageRefusal::Boot`). Hex gives it `Duration::MAX`, so Shards waits for the run's start or its host's end alone, and
makes the boot on a thread of its own: the boot's W* is where a start answers `Lifecycle { Pending }` and tells the
census its run-out, never where a boot ends, and the answer, when a later call takes it, is the sample W* rises to. A
start answered so is followed through `Runtime::started` (`hex.image.started`), which the node's wire carries
(`Call::Started`, kind `0x001f`, appended; ROOT, 2026-11-14), so a remote caller waits on the fact, never by looks. A
stats ask is given `Duration::MAX` too and followed by its run's life alone, its W* where the census is told a run-out.
A member's lane's first word and a signal's answer on it each wait within their W* on Shards' own handle for the ask
(`PendingMember::wait`, `MemberSteering::steered`; Shards `155b5ee2e`), past it the run-out told once and the same ask
waited on by the run's connection's life, never sent again.
D231's stop takes no patience and waits for its run's end, which its kill makes; Hex makes it on a thread of its own:
past the stop's W* a signal's or a retire's stop answers `Lifecycle { Pending }`, the stop going on, and a delete's
follows it to its end (§3). The waits census (`hex_node::census`) records each wait's answers and run-outs with every
wait armed, as anywhere: it disarms none (decision 9). `crates/hex-cli/tests/image_record.rs` times the three on a host class (a harness
image booted, its stats read and stopped n times through D231 as this class does, each series' first boot an
upgrade's, a fresh cache): evidence of decision 9's rule that the census keeps, never a seed.
`crates/hex-cli/tests/image_real.rs` runs the class's cases (`IMAGE_CASES`) through a real `hex node` in the census,
with each wait's answers printed.

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

- **The waits' evidence** on each host class, Linux-KVM, macOS-HVF and Windows-WHP (ROOT's measurement,
  `image_record`): `Images::boot` of a harness image to its return, `stats(Run)` to its answer, `stop` to its return,
  each `n` with its slowest, as the census keeps it (§6). No create waits on it: decision 9 reads no record.
- **Real-VM runs** (ROOT): the image class's conformance cases (`IMAGE_CASES`) on a node with Shards configured, on
  Linux KVM (`crates/hex-cli/tests/image_real.rs`), macOS HVF and Windows WHP, each with a run host at Shards
  `f17b8fb77` or later (image channel version 41: Hex's `shards` and its shards-init, rebuilt on each host by
  `scripts/shards-gpu/build.sh`), as a run host and the vendored API of different versions refuse each other at
  create.
- **From Shards:** `attach` or the run's identity after an owner's crash (`AttachAfterCrash`), member exec
  (`MemberExec`; step 11; a member's signal rides its lane since goal 2's item 2), STATS' memory peak, OOM kills and refused forks, Windows arm64's run host
  (unbuilt, D231). Given since 482c5d78: a stop for a boot under way (D231's `BootStop`, `ed1136305`), which a delete
  of a booting box asks (§3); the Windows run host (`WindowsRunHost`, 731b5e54), a boot patience (`Images::boot`'s),
  guest time per vCPU in the run's stats (D241), the run host holding only its stdio (D248, Hex's 0001 upstream), each
  member held to its reach (D246, Hex's 0002 upstream) and a never-started command's end in its VM's words (D249). The
  native-owner launch would also give the run host a scope of its own: it runs in the node's process tree today, so
  Hex charges the VM's envelope by admission but cannot contain the run host per box, nor read its use on the host
  (§4).
- **Its diagnostics' disk use** (§1): the bytes of a box's diagnostics file are its own use of the node's disk, which
  no usage the image class reports counts yet (`Usage` has no disk use).
- **Pause and resume on real VMs** (§10, ROOT's): HEX-IMG-9 through `image_real` on Linux KVM, macOS HVF and Windows
  WHP, at Shards `9975d1f55` or later (image channel version 33 or later: D267's pause), and the learned charge across
  a pause on a macOS or Windows node's recorded trace.
- **Swaps on real VMs** (§11, ROOT's): HEX-IMG-10 through `image_real` on Linux KVM, macOS HVF and Windows WHP, at
  Shards `f17b8fb77` or later (image channel version 41), each node's Shards home holding a component for the image's
  harness (`HEX_IMAGE_COMPONENT`), and a swap now of a harness whose output no caller reads past what a pipe holds.
- **From Shards, for swaps** (§11): a start again that gives its member the run-time arguments its first start had
  (D256's `MemberSpec::args`; a swap now restarts its member with none, so a harness started with kiln's flags, its
  `--resume` among them, starts again without them), and MCP servers' paths (D270 slice 4's "not yet").

## 9. What would falsify it

- A run whose VM outlives a stop that `ImageGuest::stop` returned from (then a stop proves nothing, and every
  stopped box is held).
- A harness that needs Shards' own sizing rather than its admitted size (then `vm_size` is the wrong source).
- A digest-pinned image Shards' store cannot find whatever it is named (then pinning needs the name too).
- A run host found holding a descriptor or handle it was not given (then D248's spawn is not enough, and the
  macOS and Windows creates wait again).
- A paused VM whose guest time grows, or a pause that waits on its guest (then D267's hold is no pause, and §10's
  paused windows hide use, or its patience is wrong for a hung guest).
- A member's next generation started before its last's channels ended, or a channel's end told after its swap's answer
  (then §11's drains are not what holds a start again, or a joined drain waits for its run's end).

## 10. Pause and resume (kiln plan P5.5b)

Decided 2026-11-15, built on `cp/t7-p5-5b-pause-v2` (not merged). Kiln's sessions pause and resume a running agent
(kiln plan `01a1dd2e8618733a87f2793aab1322a8`, P5.5c), through Hex: an image box pauses and resumes through its node,
its usage saying its paused time, and a paused box holds its room (P5.5b's acceptance).

Sources, read at the revisions named: Shards' D267 ("An orchestrator's run paused and resumed: `ImageGuest::pause` and
`resume`, the VMM's own hold of every vCPU", `docs/design/architecture.md` at `9975d1f55`); `shards_api::image` at
`9975d1f55` (`ImageGuest::pause`, `resume`, `Paused`, `ImageRefusal::Paused`, `ImageRefusal::Pause`, `Stats::paused`,
`RunEnded::paused`); `crates/shards/src/api_run.rs` at `9975d1f55` (`Pause`'s states, a run's stats while paused);
`learned-charge.md` C1 (a window's sample, Karn's point).

- **The calls.** `Runtime::pause(id)` and `Runtime::resume(id)`, typed, each answering nothing past its done: the box's
  state is inspect's (`BoxState::Paused`, appended, the wire's tag 3) and its paused time usage's. The node's wire
  carries them as appended calls, `Call::Pause` (kind `0x0022`) and `Call::Resume` (`0x0023`), answered `Done`;
  hex-client's `Core` sends them on every transport (the local socket on Linux, macOS and Windows, a guest's stream,
  QUIC), each timed by a box kind of its own (`client.answer.pause.<class>`, `client.answer.resume.<class>`), which
  measures in every table as `mcp` does. Every runtime that holds an image box passes them to its class (`WithGuest`,
  `GuestRuntime`, `Recorded`, Linux's `Node`, hex-node's `ImageRuntime`); the trait's own answer for any other is the
  class refusal below, naming the class inspect says.
- **The refusals.** `Refusal::Pause { id, op, cause }` (appended, tag 17), `cause` a `PauseCause` (append-only, tags 0
  to 4): `Paused` where it is paused already, a pause's; `Running` where it isn't paused, a resume's; `NotRunning`
  before its start and once its stop has begun; `Ended` once its run has ended (Shards' `ImageRefusal::Ended` among
  it); `Class(class)` for a box of any other class or form, a Shards guest's oci box among them. Its boot under way is
  `Lifecycle { Pending }` as every call's is, a move of it under way `Lifecycle { Busy }`, a held run
  `Awaits(AttachAfterCrash)`. A move its VM didn't make, its machine stopped before every vCPU was held (D267's
  `ImageRefusal::Pause`), is `Refusal::Guest { op, failure: GuestFailure::Pause(why) }` (appended, tag 5), in its words.
  Shards answers a pause of a paused run as done (ROOT's ruling for it, 2026-11-14); Hex's seam refuses one typed
  (ROOT's P5.5b brief, 2026-11-15: already paused, not running, ended and the class can't pause, each typed), and never
  asks Shards a move its run is in already, so a caller that lost its answer reads `Paused` or `Running` as its move
  done.
- **One move at a time.** The run keeps its vCPUs' motion, D267's own states one for one: going, pausing, paused,
  resuming. A pause is begun only from going and a resume only from paused, under the runs' lock, which moves the run to
  the move under way; the move is asked of Shards outside it, and ended under it as Shards answered: paused or going
  where it was made, back where it was not. A move asked while another is under way is refused `Busy`, never queued
  behind it, so a pause and a resume asked together never leave Hex's word and the VM's apart, and a move Shards is
  slow to answer stalls no other call. A stop or delete taken meanwhile wins, as Shards orders the asks: the move's end
  leaves a stopping run as it is. The motion is this process's alone, as the run is (§5): a restarted node holds a
  paused run as any run of an earlier process.
- **Its wait.** D267's VMM makes a move at once, asking nothing of its guest, which may be slow or hung, so the ask
  waits on its VM's answer or its run's end alone (`Duration::MAX`), never a timer, as a room's and a rate's do.
- **While paused** (from its pause's ask to its resume's answer, as D267's run host refuses its guest's asks), a call
  that needs its guest's answer is refused at once, `Pause { cause: Paused }`, never waited on: a signal but KILL and a
  member's signal (whose lane would wait for its guest, on one of serve's workers for a remote member's kill), its room
  (Shards' `ImageRefusal::Paused`, typed so), and the usage of a run whose guest had said no stats before its pause.
  Answered as ever: KILL and delete (its final use read as it pauses), inspect (its publishes and endpoint still told),
  its usage (its guest's lines as last said before the pause, its guest time flat), its host use, its rate and its
  period, and its members' streams. An MCP connection's ask and an in-process member's own kill ride their lanes,
  which its guest takes once it goes on (D267: a lane's connections stay open), each waiting on it as its lane always
  does.
- **Usage.** `Usage::paused_ns`, appended (the wire writes it after how its microVM started), and `HostUse::paused`,
  appended: D267's `Stats::paused`, refused where untold. `HostUse::window` keeps its meaning, the run's whole life
  since its boot answered, its paused time among it: D267 has an orchestrator's window take the paused time off, and
  Hex gives both parts instead, since kiln's meter divides a read's CPU growth by its window's, which a window that
  stops growing through a pause would divide by none. A reader takes the paused time's growth off the window's for
  what ran between two reads.
- **Its room held.** A pause changes no record, receipt or claim, so the node's ledger and the scheduler keep the box's
  receipt booked, and its learned charge keeps its booking: a span between two of a learner's reads that holds any of
  a pause, its paused time having grown, is no sample (`learned-charge.md` C1), so the windows before the pause keep
  making its charge, and the room they book is there for its resume. Linux's learner learns no image VM (§4), which
  stays booked at its receipt whole. A paused window still tells what the VM used of its CPU cap, so the cap's
  water-filling lends what it leaves to the others for that window alone, its booking its share throughout. Its room
  isn't told while paused (Shards refuses it at once), and the window after its resume tells it again, as D267 has an
  orchestrator do.
- **Simulator and fakes.** `hex_api::sim::SimRuntime` pauses and resumes an image box at once, its guest time flat
  while paused, its usage its paused time, its refusals the class's; HEX-IMG-9 holds every runtime of the class to them.

**Alternatives not taken.** Shards' idempotent answer at Hex's seam (the brief names a typed refusal; a caller still
reads it as done). A lock held across Shards' move (a stuck move would stall each next call of the run behind it;
`Busy` tells the caller instead). The paused time taken off `HostUse::window` (D267's suggestion: it changes an existing
field's meaning under kiln's meter). The cgroup freezer for process and oci boxes (P5.5b pauses image boxes; D267
refused SIGSTOP and a freezer for a VM, whose device threads and link they freeze).

**Tests.** HEX-IMG-9 (`hex_api::conformance`) through the simulator in process, through a node's socket
(`the_image_cases_hold_through_a_nodes_socket`) and, ROOT's, on real VMs (`image_real`); over QUIC
(`p5_5b_a_vms_pause_and_resume_go_through_quic`); the class over a model of D267 (`image_tests.rs`, `p5_5b_*`: its
moves, `Busy`, a move its VM didn't make, its end while paused, a mute paused run's usage); the wire's round trips; and
the learner holding a paused VM's charge and booking through SEEDABLE paused windows
(`p5_5b_a_paused_image_vm_keeps_its_charge_and_booking`), which taking those windows as samples turns red.

**What would falsify it.** A VM whose guest time grows while Hex says it paused; a resumed VM whose use passes the
charge its pre-pause windows made more often than one window in SEEDABLE, the windows around its pause left out; a
caller of kiln's that needs a move it lost the answer of answered as done rather than refused typed.

## 11. Component swaps and a share's exec (Shards' D270 slices 3b and 4, D279)

Decided 2026-11-16, built on `cp/t7-v41-swaps` and merged on Hex's local main (not pushed), AC-T7.4.1. The Hex lead's
brief (2026-11-16): bring Hex to Shards' vendor point `f17b8fb77` (image channel v41), expose a member's component swaps
through Hex as P5.5b exposed pause and resume (§10), and carry a volume's `exec` through the image class.

Sources, read at the revisions named: Shards' D270 ("Components swapped into running image VMs, sans rebuild", slices
3b and 4, with the faults its HVF runs found), D277 (a share given its members as they start), D278 (a member's own
grants past its microVM) and D279 (a share's or a plug's files may be refused execution), `docs/design/architecture.md`
at `f17b8fb77`; `shards_api::image` at `f17b8fb77` (`ImageGuest::swap`, `restore`, `components`, `restart`, the
generation on `take_output`, `take_input`, `wait_member` and `wait_channel`, `ImageRefusal::Swap` with `SwapRefusal`,
`ImageRefusal::Restart`, `MemberEnd::Killed`, `ImageShare::exec` and `members`, `Plug::exec`, `ImageSpec::egress_ceiling`),
`image/session.rs` (each generation's pumps, given room as they start; a reader that left has the rest let go) and
`image/wire.rs` (`SwapAsk`, `SwapAnswer`, `ComponentAt`, `Swapped`); `crates/shards/src/api_run.rs` at `f17b8fb77`
(`swap`, `restart_turn`, `last_output_ended`, `end_channel`).

- **The calls.** `Runtime::swap(id, at, digest, when)`, `Runtime::restore(id, at, when)` and `Runtime::components(id)`,
  typed. `at` is a member's path (`/agents/NAME`, `/harness/NAME`), `digest` the manifest digest of an OSI artifact its
  node's Shards store holds, `when` `SwapWhen::NextStart` or `SwapWhen::Now` (the wire's tags 0 and 1). A swap or a
  restore answers `Swapped`: every member's component after it (`ComponentAt`: its path, the digest its next start lays,
  the one its current generation runs with, none for its image's own), the generation of its member a `Now` started, and
  the bytes of the ended generation's output no caller had taken, let go (the draining rule, below). The node's wire
  carries them appended: `Call::Swap` (kind `0x0024`), `Call::Restore` (`0x0025`) and `Call::Components` (`0x0026`),
  answered `Answer::Swapped` (`0x8018`) and `Answer::Components` (`0x8019`). hex-client's `Core` sends them on every
  transport, each timed by a box kind of its own (`client.answer.swap.<class>`, `.restore.`, `.components.`), measuring
  as pause's do. Every runtime that holds an image box passes them to its class (`WithGuest`, `GuestRuntime`,
  `Recorded`, Linux's `Node`, hex-node's `ImageRuntime`); the trait's own answer for any other is the class refusal.
- **The refusals.** `Refusal::Swap { id, op, cause, why }` (appended, tag 18), `cause` a `SwapCause` (append-only, tags 0
  to 10): Shards' `SwapRefusal` one for one, matched whole, `Unknown`, `Artifact`, `Kind`, `Platform`, `Asks`, `Device`,
  `Running` and `Failed`, `why` Shards' words; then `NotRunning` before its start and once its stop has begun, `Ended`
  once its run has ended (Shards' `ImageRefusal::Ended` among it), and `Class(class)` for a box of any other class or
  form, a Shards guest's oci box among them. Its boot under way is `Lifecycle { Pending }`, a held run
  `Awaits(AttachAfterCrash)`. While paused, a swap or a restore is refused at once, `Pause { cause: Paused }`, its guest
  answering nothing (§10); `components` is answered as ever, from what its node holds. Shards' `ImageRefusal::Restart`
  comes of a member's restart of its own, which this class asks none of (its starts again are its swaps'): `Broken`
  where seen, as every refusal of an ask the class makes none of is.
- **Its component first.** A swap or a restore is asked of Shards at its member's next start first: its artifact checked,
  its image built where the store has none, plugged and mounted, or its image's own wanted back. Refused there, nothing
  of its member is touched. Only then, where `Now` was asked, the same change is asked now, which starts its member
  again, its component plugged already (the run host's plug of one plugged answers at once). Shards' `Now` would do
  both in one ask, but the member's output would then be read from before its artifact was checked, and a swap refused
  for its artifact would leave the node reading the output of a member that runs on.
- **Generations.** A start again ends its member's current generation (SIGTERM to its command) and starts its next in
  the namespaces its generations keep (D270 slice 3b), each generation's stdin, stdout, stderr and end its own. The
  class keeps each member's current generation with its run, by declared index, from Shards' answer
  (`Swapped::started`), and starts a member again through these calls alone, so nothing else moves it. `Runtime::member`
  takes the member's current generation's streams, once each. A program taken before stays its generation's: its end is
  that generation's (`MemberEnd::Killed`, a generation its start again ended at once, read `Signal(9)`), and its kill,
  which rides the member's lane, is answered at once once its generation has ended, as a lane steers the generation
  running as it opens.
- **The draining rule.** Shards starts a member's next generation only once every channel of its last has ended, and its
  run host takes no more of a channel while its orchestrator's pipe is unread (shards-api's pump writes each part into
  the caller's pipe before it gives its host room for more; what no one reads backs up into that channel alone): a start
  again whose last generation's output no one reads fails past its patience, `Failed`, and Shards lets that member go.
  So before it asks the start again, the class takes each of the member's current output channels no caller took (a
  member no caller drives has none taken; `take_output` refuses one taken, its taker's to read) and reads it on a
  thread of its own to its end, letting its bytes go. A caller that took them reads them to their end, as it reads any
  program's. Each channel's end comes before Shards' answer (one stream, in order), so once the swap is answered the
  drains of channels told ended are joined, their bytes `Swapped::unread`; a channel told no end was never opened,
  nothing of it came, and its thread ends with its run. Refused after its first ask, the drains read on to their
  generation's end. The model's run host fails a start again whose last output no one could read (its worst case: past
  what a pipe holds unread), so `d270_a_swap_now_lets_go_of_its_members_unread_output_and_its_start_again_goes_on`
  fails without the drains.
- **Its wait.** Each ask waits on its run host's answer or its run's end alone (`Duration::MAX`), never a timer, as a
  pause's does. Shards' patience is its member's graceful end's too (ended at once past it, `MemberEnd::Killed`) and its
  last output's: a bound of Hex's would kill a member slower to end than it guessed, and a start again it failed would
  let its member go whole. With none, a member that never ends at its SIGTERM holds a `Now` until a KILL ends it, its
  own (`Runtime::signal_member`) or its box's, and output a caller took and never reads holds it until it is read or the
  run ends.
- **A share's exec.** `ShardsImage::shares` are `ImageShare`s (`source`, `destination`, `read_only`, `exec`), no longer
  `Mount`s, so an OCI mount keeps its own form. `exec` (D279) is on unless a spec says otherwise: serde's default, left
  out of JSON where on, so every record and request written before it reads as it did; the wire writes it after
  `read_only` in each share. The class gives it to Shards' `ImageShare::exec`; off, each of the share's mounts is
  `noexec`. The node VM's own shares (§7, `HEX_GUEST_SHARES`) stay runnable: its containers' bind mounts come from them.
  Every `ImageShare` the class makes names no members (D277's `None`: given where the image's `VOLUME` lies, as before)
  and no member's ceiling (D278's `egress_ceiling` empty): kiln asks Hex for neither yet.
- **Simulator and fakes.** `hex_api::sim::SimRuntime` swaps at once: its image runs one harness of its own
  (`SIM_HARNESS`) beside the members its spec names, and its node's store holds one harness's component
  (`SIM_COMPONENT`); a `Now` ends the program its caller took as TERM ends one with no handler, and its next
  generation's streams are takeable anew; its refusals the class's. HEX-IMG-10 holds every runtime of the class to them,
  the fixture naming its component (`Fixture::component`; `image_real` takes one from `HEX_IMAGE_COMPONENT`).

**Alternatives not taken.** Shards' `Now` in one ask (above). Each unread reader dropped rather than read (Shards' pump
then lets its bytes go uncounted; read, they are told). A finite patience (above). A `Runtime::restart` of its own (no
caller asked one; a `NextStart` swap is laid by a later `Now`). `exec` on `Mount` (the OCI class would carry a field it
honours nowhere yet).

**Tests.** HEX-IMG-10 (`hex_api::conformance`) through the simulator in process, through a node's socket
(`the_image_cases_hold_through_a_nodes_socket`) and, ROOT's, on real VMs (`image_real`); over QUIC
(`d270_a_vms_swap_and_restore_go_through_quic`); the class over a model of D270 (`image_tests.rs`, `d270_*`: the drain,
a driven member's generations each its own program, a generation killed at its start again, every refusal); the
simulator's own (`d270_a_sim_members_swap_now_ends_its_program_and_its_next_is_taken_anew`); the wire's round trips,
generators and bytes (`d270_a_swap_its_answer_and_its_refusal_cross_the_wire`,
`d279_a_shares_exec_crosses_the_wire_after_its_read_only_flag`); a share's exec in JSON
(`d279_a_shares_exec_is_on_unless_it_says_so`); and the class asking Shards for each share's exec
(`w12_an_image_vm_shares_exactly_the_host_directories_its_caller_names`).

**What would falsify it.** A member's next generation started before its last's channels ended, or a channel's end told
after its swap's answer (§9); a share whose files may not be run that runs a binary; a swap now that leaves its member's
streams to its last generation's caller.
