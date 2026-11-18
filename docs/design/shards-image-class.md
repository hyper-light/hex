# Decision (2026-11-12): the Shards class on Agentfile images under shards-init

Joint plan step 12 (kiln plan `01a1cd8e862e7169a074db3d5fa33f3d`, "Every kiln harness in Shards microVMs, Hex as
runtime"), AC-T7.4.1. Status, 2026-11-13 (built, merged and live apart; live is GitHub's main, `025aae6d`): **built to
Shards' D231**, on Linux, macOS and Windows hosts, its run host started holding only its stdio by Shards' own spawn
(D248, §1) and each vCPU's guest time in its usage (D241, §4). Live at Shards `1389dc0b` (`68ed140a`, `2c728252`).
Merged on Hex's main at Shards `04481e36b`, image channel v51 as at `f1e8a097b` (a share given to members as they start, D277, v37, which
the class gives from a share's member scope, `ImageShare::members`, P1.4, and a member's working directory its guest can't
enter refused typed, `ImageRefusal::Workdir`, v42 and v43, the spec's; a member's generations, its restart and a
component's swap, D270, v38 to v49, each member stream and end named by its generation, the class's members' their
current, a component's swap and restore starting them again (`Runtime::swap`, `restore` and `components`, §13); each member's own reach within its ceiling, D278, v39, and a member's grants on a
network shared by runs, D285, v50, neither of which this class asks, their refusals typed; a share's files run from it or
not, D279, v41, as each share's `ImageShare::exec` says (§13); its members' stdio served as its endpoint's lanes,
D252, v44, which the class doesn't ask; its run host's budget holding the run's link and its members' first lanes, a run
with no room for them refused `ImageRefusal::Capacity`, D252, v51, the host's; `9975d1f55` before it, image channel v36:
an orchestrator's run told the memory it may plug, as a
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
use, which the class keeps for a run that ended on its own since kiln's P7.1b (§3), a member's lane asks waited within their W* and followed past it, told,
and no GPU report asked; `16451e19f` before that, v27, at `cf844022`, a member's end typed, a member a signal killed
`Exit::Signal(n)`, never its 128-plus status; `37ddeb080` before that, v26, at `2f55dccf`, no upstream root named;
`097ec0047` before that, v25, at `1314e5f2`, a member's MCP bridge refused by its field while the class asks no agent-control endpoint with
`2f5d20e7`; `3114e082f` before that, at `8dfe6df8`, a delete stopping a
boot under way with `31c651fb`; `db4168345` before that, at `1d60abad`, each credential grant's placeholder with `1c4d11b3`; `740fa372` before
that, at `8aa7c028`; `e0cc059c` before that, at `c8b4ce6b`), with a failed run's end in its
runtime's words (`32b66a10`, `94d519d6`), the class whole and its host use read to its end (`ae7038d7`, `2a8bf6be`), a
member's arguments and the VM's shares (`cf535152`), and `NoSuchBox` where a shared guest holds no record (`fac528d9`).
On real VMs, HEX-IMG-1 to 7 passed on Linux KVM at `c8b4ce6b` and `8aa7c028`, and HEX-IMG-2 to 5 on macOS HVF at
`cf535152`. Owed (§8): HEX-IMG-1, 6 and 7 on HVF at `fac528d9` or later, and every case on Windows WHP. Merged from
`root/hex-p31`, kiln plan `01a1dd2e313f72f69c6d93061143cc2e` P3.1: the warm pools kiln names, each booting
its boxes through Shards' `ImagePools`, its hosts charged to the room its owner binds (§10); its real-VM runs owed.
Merged from `cp/t7-p5-5b-pause-v2`: a box's pause and resume through its node (kiln plan P5.5b, D267; §12).

Sources, read at the revisions named:
- Shards' D231 ("An image booted for an orchestrator", `docs/design/architecture.md` at `e0cc059c`, `740fa372`,
  `db4168345`, `3114e082f`, `097ec0047`, `37ddeb080`, `16451e19f`, `ce80cbb37`, `2737fc4ec`, `2246a8829`, `d7ff8a094`,
  `9d8a05f6a`, `0290f1e9f`, `fa953e3ce`, `888879412`, `d993d83ee`, `9975d1f55` and `f1e8a097b`) and
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
  `ImagePools`, which the pools kiln names boot through, §10), D242's leases (`ImageSpec::lessor`: none here, as no box grants a credential) and D257
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
- `shards_api::image` at `e0cc059c`, `740fa372`, `db4168345`, `3114e082f`, `097ec0047`, `37ddeb080`, `16451e19f`, `ce80cbb37`, `2737fc4ec`, `2246a8829`, `d7ff8a094`, `9d8a05f6a`, `0290f1e9f`, `fa953e3ce`, `888879412`, `d993d83ee`, `9975d1f55`, `f1e8a097b`, `8d6029e4d` and `04481e36b`: `crates/api/src/image.rs` (`Images`, its `boot` and
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

- A box of `ClassSpec::ShardsImage(ShardsImage { image, command })` is of its own class, `Class::ShardsImage`, run by
  the Shards backend (§11): one microVM of its own, which `shards_api::image::Images::boot` boots under shards-init.
  shards-init applies the image's Agentfile: each member (agent or harness) starts in its confined domain from its own
  `harness.json`/`agent.json` (D54, D59).
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
| `exec` | Refused `Invalid` naming exec: a program runs in one of the run's members, which `exec_member` names. |
| `exec_member` | `Execs::exec` on a lane of its own of the run's agent-control endpoint (D238's exec lane, D243's member exec), its stdio pumped and its end waited for. |
| `inspect`, `list` | The record's phase and exit, and this process's run (`wait_for` with no wait). |
| `usage` | `stats(StatsScope::Run)` (§4). |
| `pause`, `resume` | `ImageGuest::pause`, `resume` (D267; kiln plan P5.5b, §12): its vCPUs held out of its guest by its VMM, or let go, one move at a time, refused typed where the box isn't as the move asks. |
| `delete` | `Deleting`, then `stop` if it runs, to its run's end, then the record removed. |

Every member operation D231 doesn't give (a terminal; a signal to an exec once its outputs have finished) is refused
typed, naming the capability (`Refusal::Awaits`), never stubbed. A member's own streams are the joint plan's step 17
(a kiln session's stdio as its harness member's):
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
  read-only, and whether its files may be run (D279, §13). The boot asks Shards for exactly those, in order (default
  deny: the image's grants widen none), and a
  share Shards refuses refuses the start as `Invalid`, in Shards' words. A member gets one where its image's
  `VOLUME` at that path is for it (D109), or where the share's member scope names it (`ImageShare::members`, P1.4: Shards'
  `ImageShare::members`, D277, image channel v37, given as each member starts, so a harness works in its project from
  its first instruction), and the run's own command gets every one. A scope's members are checked as a grant's member
  is, each once; Shards refuses one its image doesn't declare, and a working directory no member's guest enters,
  `ImageRefusal::Workdir`, mapped `Invalid` in Shards' words.
- `Runtime::member(id, member)` hands the caller that member as a `Proc`: the caller's ends of its stdin, stdout and
  stderr (D231's `take_input` and `take_output`, each once, so a second call is refused as Shards refuses it), and
  its end (`wait_member`; `WithRun` is the run's end, as the box's own, and a run that failed refuses it `Failed` in
  its runtime's words, kiln P2.3), each its current generation's (D270, §13). It is no process of this host
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
- `Runtime::exec_member(id, member, program, stdio)` (kiln's P7.1b ask, lane replay at kiln `107a4da17`: its replay ran
  as its VM's own command, Hex running no program in a running image VM) runs `program` in any member its run declares
  that runs a program, driven by its caller or not, as a `Proc`: Shards' member exec (D243) on a lane of its own of the
  run's endpoint (D238's exec lane, `Execs::exec`, given at `9975d1f55`). It runs as its member does: its user,
  confinement, namespaces and cgroup the member's, so a program naming a user is refused `Invalid`; its environment the
  member's with the program's variables over it (where `exec` hands a program its whole environment); its working
  directory the program's, else the member's; pipes, no terminal. Its stdio is an exec's (`Stdio`: the caller's own
  ends, or ends asked for, given once). Its end is its status as Shards tells it once both its outputs have finished
  (`Exit::Code`, 128 plus a signal's number where one ended it, as `docker exec` reports it), or `Signal(9)` where its
  own kill ended it. Its kill is KILL on its lane (`EXEC_SIGNAL`) while its outputs are open; once both have finished,
  Shards' one handle of the lane (`ExecControl`) waits for its exit and steers nothing beside that wait, so a kill then
  is refused `Awaits(ExecSignalAfterOutput)` (a member's own lane gives its steering and its end apart; the exec's is
  owed, §8). Its start waits within `hex.image.exec`'s W* and past it follows the run's connection (§6). Refused,
  typed, in this order: `Invalid` for a program that can't be started, names a user, or is past what the lane carries
  (`run::MAX_PAYLOAD`), before anything of its run; Pending while it boots, held, and `NotRunning(id, "exec in a
  member")` before its start and once its stop has begun; `Invalid` for a member its run declares not;
  `NotRunning(id, "exec in a member: it runs no program")` for one of files alone; `NotRunning(id, "exec in a member:
  its member refused it, ended or confined")` where Shards says its member refused it (`Unstarted::Refused`: it has
  ended, or its confinement refused what it asked); `NotRunning(id, "exec in a member: its run ended")` where the run's
  connection ends first; and any other of Shards' faults `Os`, in its words. Three threads of its own pump it, its
  stdin's, its stderr's, and its stdout's, which then waits for its end: an MCP connection's cost, for the same reason.
  `Runtime::exec` of an image box, naming no member, is refused `Invalid` naming exec. The class serves it in process
  (`ImageClass`, `GuestRuntime`, `WithGuest`, `Recorded`, the Linux `Node`, hex-node's `ImageRuntime`, which kiln
  holds) and the simulator runs its programs in the members a spec names; the node's wire carries no member exec yet,
  so `hex-client` refuses one `Invalid` before anything is sent, as it refuses a host use.
- `Runtime::link(id)` (kiln plan step 35's `/runs/control`) answers a running VM's agent-control link as both its ends
  say it (`ControlLink`, Shards' `Linked::report`: D238, D239, D254): what each end's TLS 1.3 handshake negotiated, by
  IANA's codes and rustls' names, its controller's (the class's own) and its endpoint's (the run's, as its report on the
  link's control lane says it), and every lane its controller was granted, by Shards' names (`Granted::name`):
  `control`, `exec` and `log:run`, then `mcp:N` for each member whose bridge the run serves. Its controller is dialed
  where none is yet, as a signal's or an MCP connection's is, and kept for the run. Shards waits for the endpoint's
  report, which comes once the link's handshake is done, or for the connection's end, taking no patience: the call holds
  that run's link's own lock over the wait, which only calls asking that run's link take, so a second waits behind the
  first for the same report, which Shards keeps once heard; `hex.image.link`'s W* is where the census is told a
  run-out (§6). Refused, typed: Pending while it boots, held, and `NotRunning(id, "link")` before its start and once
  its stop has begun; `NotRunning(id, "link: its run ended")` where the run's connection ends before its endpoint
  reports; and any other of Shards' faults (the endpoint's refusal or reset of the control lane) `Os`, in its words. The
  class serves it in process (`GuestRuntime`, `WithGuest`, `Recorded`, the Linux `Node`, and hex-node's `ImageRuntime`,
  which kiln holds); the node's wire carries no link call yet, so `hex-client` refuses it `Invalid`, naming the wire,
  before anything is sent, as it refuses `room`, `rate` and `host_use`.

What D234 to D236 add is asked for as Hex holds it: no nesting grant (`nesting: false`, no `ready_by`: no harness
is a nested runtime, step 39's), the destinations a box's grants name kept and each member held to its own reach
(network-grants.md N4), and D235's plugs, D271's, for the volumes its spec gives its members alone, as its run starts
(§2a), each refusal typed by what refused it.

## 2a. Volumes (kiln plan 01a1dd2e…, P6.2; Shards' D271, D281)

An agent's volumes are kept on its machine across its sessions: each a named volume of the node's Shards home (the
store its run hosts' `SHARDS_HOME` names), made empty where it isn't there and kept past the run, so the next run
given it finds what the last one left. Status, 2026-11-16: built against Shards `f1e8a097b` (image channel v51,
`shards api-volumes` answers v3), written and unit-tested over the model and the simulator, not run (§8).

- **The spec.** `ShardsImage::volumes`, each an `ImageVolume`: its name in the store (its grammar Shards'); its bound
  in bytes, at least one; where its members see it (`at`): absolute, plain and strictly beneath a top-level
  directory, at no share's or other volume's place; the members it is plugged into, each named once, at least one;
  and whether read-only. `ShardsImage::check` refuses each by its field (`volumes[i].…`). The node's wire carries them
  after the sign-ins, a bound of 0 refused as it is read.
- **Its plug roots, granted at start.** A member's domain is given every right beneath its roots as it starts and none
  later (D271: Landlock only narrows), so the run is granted, as `ImageSpec::plug_roots`, the top-level directory of
  each volume's place, each once, and no other. A place beneath a directory its guest keeps its own (`/proc`, `/sys`,
  `/dev`, `/tmp`, `/etc`, `/run`, as Shards' `shards_abi::run::plug::root_name` says) is refused at create.
- **Plugged as its run starts.** On the boot's thread, once the run has started and its diagnostics are kept (§1),
  each volume is plugged in the spec's order (`ImageGuest::plug`: `PlugSource::Volume { name, bound }`,
  `PlugFor::Members`, its files runnable as every share's are, D279), before the boot answers: no call takes the run,
  nor a member's streams, before its volumes are in place. Each plug waits for its run host's answer or the run's
  end, as the boot does, the boot's learned wait following it (§6). The first refused stops the run (D231's stop, to
  its end) and refuses the boot in Shards' words, its box ended with them, as any refused boot's is (§3).
- **Admission.** A volume's bound is drawn from the disk its box was admitted with (its allocation's `disk_bytes`,
  kiln's `Isolation::disk`): its volumes' bounds together within it, else its create is refused `Invalid` before
  anything is made. Its run host's diagnostics (§1) are held within what they leave, so every byte the box may write
  on its node's disk is charged to it once; a box whose volumes take all of it holds its diagnostics within Shards'
  tail, as one admitted with no disk does (§1). Shards holds each bound as the files are written (D281: ENOSPC past
  it). What a volume holds already is on its node's disk outside any box, its node's free room short of it: an
  admission that counts it reads it, with its bound and its charge, through `Runtime::volumes`, a page's or an
  admission's cost, never a start's (D281: each listing walks every volume's files).
- **Listing.** `Runtime::volumes()` answers every volume of the home as `shards api-volumes list` reads it
  (`shards_api::volumes::volumes`: the class's own `shards` in the class's home): its name, the bytes its files take,
  its bound, its charge by its bound's measure, when it was made, and whether it is a run's anonymous one, which is no
  agent's. Served in process, by the class, `hex-node`'s `ImageRuntime` (kiln's) and the simulator; the node's wire
  doesn't carry it.
- **Clearing.** `Runtime::clear_volume(name)` removes it with its files (`shards_api::volumes::clear_volume`), so the
  next run given it finds it empty. Whether a run holds it is Shards' to say: each run that plugs or mounts it, an
  image box's or a daemon's, holds its lock from its plug to its end (D281's P6-fix), so a box created and not yet
  started holds nothing, and its start's plug then makes the volume anew. Hex keeps no record of holders beside
  Shards' locks.
- **Refusals** (Shards' R04), each by its retry: `Refusal::Volume { cause, why }`, in Shards' words, which name the
  volume. `Bound`: a start asked it with another bound than the runs holding it serve it with, which changes only once
  none does. `Clearing`: a clearing holds it; asked again once that has answered, it is made anew, empty. `InUse`: a
  clearing of one a run holds, its files kept. `Unknown`: a clearing of one the home has none of. A plug's other
  refusals: the spec's (`Request`, `Member`, `Outside`, `Taken`, `Volume`) `Invalid`; its host's and its microVM's
  (`Host`, `Device`) `Os`; an unplug's (`Unknown`, `Busy`) the class's own fault, `Broken`, as it unplugs nothing. A
  clearing's others: a name the store takes none by `Invalid`; files not all removed, and a `shards` that didn't
  start, answered no frame or answered another version, `Os`.

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

## 2b. Shares another server serves (Shards' D286; kiln goal 01a1ec34599974448f6b7663ddb07001)

kiln's microVM engine shares each agent's slates volume into its guest over slates' own virtio-fs device and DAX
window, slates serving it on a stream handed to Shards, with no mount of the host's between them. Shards' image boot
takes such a share as `ImageSpec::served` (D286, image channel v52): a stream its run host is handed with the run's
request and puts in the share slot after the run's own shares', with a DAX window where they have one, the server
answering the guest's FUSE requests and the window's mappings on it. Status, 2026-11-17: written against D286 on
Shards' branch `agents/g265-kiln-shards-microvm-served-image-share` (not yet on Shards' main, which ROOT lands after its
v0.8.0 tag), unit-tested over the model and the simulators; no real VM has run it (§8).

- **The spec.** `ShardsImage::served`, each a `ServedMount`: its source, in its server's own words (for kiln's, a slates
  volume's name), not empty and with no NUL; where its guest sees it, checked as a share's place is (absolute, no
  `..`, never the guest's root) and at no share's, volume's or other served share's place however its path is
  spelled; whether read-only; and its members, given it as they start as a share's are (D277). `ShardsImage::check`
  refuses each by its field (`served[i].…`). The node's wire carries them after the volumes.
- **The seam** (CLAUDE.md §9): `hex_api::serve::Servers`, the node's server of such shares, which makes each one's
  stream for one boot, its simulator `SimServers`, and its contract (`hex_api::serve::conformance`, HEX-SERVE-1 to 3: a
  stream of a source it serves is answered for that source, writable or read-only as asked; a source it serves none of
  is refused `Invalid`, naming it, with no stream). Its orchestrator binds it once (`ImageClass::bind_servers`, hex-node's
  `ImageRuntime::bind_servers`), as it binds its warm room. The stream is in no spec, record or wire: it is a descriptor
  of the node's own process, made for one boot alone, so a box with one runs where its server is.
  - *Alternatives* (§5): a stream lent with the spec (in-process alone, a field the node's wire and its records would
    drop, and one a recovered or warm boot could never be given again), or a new `Runtime` call carrying descriptors
    beside `create` (every implementation, the node's wire among them, changed for one class's need). Taken: the spec
    names the share by its source, and the node asks its own server as it boots, as it asks its lessors for a run's
    credentials (joint plan step 33's C3): nothing in a record that a restarted node couldn't ask for again.
- **Created.** A box naming one on a node bound no server is refused `Unconfigured` (`hex_api::serve::unconfigured`)
  before any record, as a box granting a credential is on a node given no lessors; the simulator refuses it alike.
- **Started.** Before the boot's intent, where its lessor is made, each served share's stream is asked of the server
  (`ImageClass::served`), in the spec's order: the first refused refuses the start, in the server's words, the box left
  Creating and nothing booted. Each goes to Shards at its place, read-only as asked, its files run from (D279, as every
  share's of the class are), and given to its members as they start (D277). Shards hands its run host a copy of each with
  the request; this process's copy goes with the boot's spec as the boot answers, so the server sees the share end once
  the run host and the VM have closed theirs, with the run.
- **What it doesn't do**, Shards' D286's: a box with one boots on a run host of its own, never a warm pool's (its warm
  host's share process starts before its run's streams come), and no save of its run is made (its server keeps the
  share's state, which no snapshot holds).

## 3. States

The record (the guest's Books, the node's Store) and this process's run:

| State | Record | Run | Leaves by |
|---|---|---|---|
| Created | `Creating`, no exit | none | start: Booting; delete: removed |
| Booting | `Ready`, no exit | a boot in flight, on a thread of its own | its answer: Running, or Ended where Shards refused the run (past its patience among it); delete: Deleting, its boot followed to its answer |
| Running | `Ready`, no exit | the run held | KILL, or its own end: Ended; delete: Deleting; pause: Paused |
| Paused | `Ready`, no exit | the run held, its vCPUs held out of its guest or a move of them under way (§12) | resume: Running; KILL, or its failure: Ended; delete: Deleting |
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
that ended on its own before its delete answers the final use its VM read once its machine was down and told with its
end (Shards' D260's `FinalUse::Read`; kiln's P7.1b, whose replay held its run until its delete for want of it): the call
that first sees that end (any settling call, the delete's own among them) converts it as a running VM's use is, each
vCPU's guest time by index, its window from the run's start, as its boot answered, to there, and the box's record keeps
it with its end (`BoxRecord::final_use`, a fact the node Store's lifecycle writes keep as they keep its exit), across
the node's restarts, until its delete. A run that started is never answered none (the review's B-5): where no final
use is kept, its run having been stopped before the delete (its use gone with its VM, Shards' stats answering only a
running run) or its VM having told none whole at its end (`FinalUse::Unread`, Shards' `Unused`, or a part of it
unread), or its read refused, the delete is refused, typed (`NotRunning`, its words saying which, or the read's own
refusal), the box as it was. A stopped
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
naming why, where its run host told none, never read as none (§12).

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
| `hex.image.boot` | `Images::boot`'s answer: the run started, its volumes plugged (§2a), or the refusal | the boot's thread, D231's run host |
| `hex.image.stats` | `stats(Run)`'s answer | the run |
| `hex.image.stop` | `stop`'s return: its run released, its end told, its run host and every process it started gone (Shards `2737fc4ec`) | the stop's thread, the run host |
| `hex.image.delete_boot` | a delete's ask to the boot of the box it deletes answering, its run started or its host ended (§3) | the boot's thread |
| `hex.image.started` | a `started` call's ask to the boot of the box it asks answering (§3) | the boot's thread |
| `hex.image.member` | a member's own lane's first word (D238): whether it runs, its steering then | the run's agent-control connection |
| `hex.image.signal` | a signal's answer on a member's own lane: taken, or refused | the run's agent-control connection |
| `hex.image.mcp` | a member's harness's next connection to its MCP bridge (D254), from its ask, which may be never | the run's agent-control connection |
| `hex.image.exec` | a program's start in a member (D243), from its exec's ask to its host's word: started, or why not | the run's agent-control connection |
| `hex.image.link` | a run's endpoint's report of its link (D238, D254), from a `link` call's ask, which comes once the link's handshake is done | the run's agent-control connection |

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
A link's report is waited for as a stats ask is: Shards' `Linked::report` takes no patience and waits for the report or
the connection's end, so `hex.image.link`'s W* is where the census is told a run-out, once the answer comes past it.
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
  `f1e8a097b` or later (image channel version 51: Hex's `shards` and its shards-init, rebuilt on each host by
  `scripts/shards-gpu/build.sh`), as a run host and the vendored API of different versions refuse each other at
  create.
- **From Shards:** `attach` or the run's identity after an owner's crash (`AttachAfterCrash`), an exec's steering apart
  from its end, as a member's own lane gives them (`ExecSignalAfterOutput`: `ExecControl` steers nothing while its exit
  is waited for, §2), STATS' memory peak, OOM kills and refused forks, Windows arm64's run host
  (unbuilt, D231). Given since 482c5d78: member exec at `9975d1f55` (`MemberExec`, D238's exec lanes and D243, which
  `Runtime::exec_member` rides, §2; a member's signal rides its lane since goal 2's item 2) and a run's final use with
  its end (D260, kept for a run that ended on its own, §3); a stop for a boot under way (D231's `BootStop`, `ed1136305`), which a delete
  of a booting box asks (§3); the Windows run host (`WindowsRunHost`, 731b5e54), a boot patience (`Images::boot`'s),
  guest time per vCPU in the run's stats (D241), the run host holding only its stdio (D248, Hex's 0001 upstream), each
  member held to its reach (D246, Hex's 0002 upstream) and a never-started command's end in its VM's words (D249). The
  native-owner launch would also give the run host a scope of its own: it runs in the node's process tree today, so
  Hex charges the VM's envelope by admission but cannot contain the run host per box, nor read its use on the host
  (§4).
- **Its diagnostics' disk use** (§1): the bytes of a box's diagnostics file are its own use of the node's disk, which
  no usage the image class reports counts yet (`Usage` has no disk use).
- **The scheduler's admission of image boxes** (§11): built on `root/p2-finish`, its tests written and owed to ROOT's
  runs, and the image class's VMs on a scheduled node owed on KVM, HVF and WHP (`image_real` through a scheduler).
- **Volumes (§2a), real-VM runs** (ROOT, KVM, HVF and WHP, at Hex's vendor point of Shards `f1e8a097b` or later): a
  box given a volume writes a file there and the next box given it reads it; a write past its bound refused ENOSPC; a
  start refused `Volume { Bound }` while another box holds it with another bound; its clearing refused
  `Volume { InUse }` while a box holds it, then its files gone once none does. The node's wire carries no listing or
  clearing yet (`Runtime::volumes`, `clear_volume`): in process alone.
- **Shares another server serves (§2b), from Shards and real-VM runs:** D286 on Shards' main and Hex re-vendored there
  (its image channel v52, its run hosts rebuilt with it); then a box given a slates volume's stream through kiln's server
  whose member reads and writes the volume and maps a file through its slot's DAX window, on KVM, HVF and WHP (ROOT's and
  kiln's end-to-end runs).
- **A member's program and a run's kept final use** (kiln's P7.1b; §2, §3): their real-VM runs (ROOT's): a program
  run in a running image VM's harness, its output and its end, its kill, and a run that ends on its own deleted with
  its host use, on KVM, HVF and WHP; a member exec carried by the node's wire, which carries none yet; and the final
  use's window, which ends where this process first sees the run's end, at its next settling call: Shards tells no
  instant with its end, and a wait on each run's end beside it (a thread for each run's life) is the alternative, its
  cost unmeasured.
- **Pause and resume on real VMs** (§12, ROOT's): HEX-IMG-10 through `image_real` on Linux KVM, macOS HVF and Windows
  WHP, at Shards `9975d1f55` or later (image channel version 33 or later: D267's pause), and the learned charge across
  a pause on a macOS or Windows node's recorded trace.
- **Swaps on real VMs** (§13, ROOT's): HEX-IMG-11 through `image_real` on Linux KVM, macOS HVF and Windows WHP, at
  Shards `f17b8fb77` or later (image channel version 41), each node's Shards home holding a component for the image's
  harness (`HEX_IMAGE_COMPONENT`), and a swap now of a harness whose output no caller reads past what a pipe holds.
- **From Shards, for swaps** (§13): a start again that gives its member the run-time arguments its first start had
  (D256's `MemberSpec::args`; a swap now restarts its member with none, so a harness started with kiln's flags, its
  `--resume` among them, starts again without them), and MCP servers' paths (D270 slice 4's "not yet").

## 9. What would falsify it

- A run whose VM outlives a stop that `ImageGuest::stop` returned from (then a stop proves nothing, and every
  stopped box is held).
- A harness that needs Shards' own sizing rather than its admitted size (then `vm_size` is the wrong source).
- A digest-pinned image Shards' store cannot find whatever it is named (then pinning needs the name too).
- A run host found holding a descriptor or handle it was not given (then D248's spawn is not enough, and the
  macOS and Windows creates wait again).
- A volume a running box holds that a clearing removes (then Shards' holds are not the authority on its holders, and
  Hex keeps a record of its own boxes' beside them, §2a).
- A box whose volumes and diagnostics together take more of its node's disk than it was admitted with, past Shards'
  diagnostics tail (then the bounds are not drawn from its admission, §2a).
- A paused VM whose guest time grows, or a pause that waits on its guest (then D267's hold is no pause, and §12's
  paused windows hide use, or its patience is wrong for a hung guest).
- A member's next generation started before its last's channels ended, or a channel's end told after its swap's answer
  (then §13's drains are not what holds a start again, or a joined drain waits for its run's end).

## 10. Warm pools (kiln plan `01a1dd2e313f72f69c6d93061143cc2e`, P3.1)

The split agreed with kiln: kiln names which pools exist (its P3.2, a pool for the agents people start on a machine, let
go once none has started for its keep window); Shards' demand rule sizes each (D26: the largest burst of its runs within
one refill window, at least one while in use, none once idle past its keep-alive); and every warm host is charged,
before it starts, to the room its node's boxes are admitted from (D253's `WarmRoom`), so admission never oversubscribes.

- **The calls** (`hex_api::Runtime`): `keep_pool(&WarmPool)` keeps a pool from now on, answered at once, no host started,
  one kept already kept; `let_go_pool(&WarmPool)` lets one go, its warm and starting hosts ended, each followed to its
  end and its room given back, before it returns, a box booted from it running on, and one not kept let go already;
  `pools()` lists each kept pool with its hosts warm and starting. They are kiln's seam's `keep`, `let_go` and `pools`
  one for one (`crates/kiln-seams/src/sandbox.rs` at kiln `da6ab301f`).
- **A pool** is `WarmPool { image, size }`: its boxes' image, pinned by its digest (`check_pinned`, refused `Invalid`
  naming `warm pool`), and their admitted size (`Allocation::vm_size`), held to what Shards boots as asked
  (`shards_api::envelope`, refused `Invalid`), as a box's are. A box boots through the pool kept for its image and size
  where there is one, and on the class's own boot (`ImageShards`) otherwise: a box of no named pool makes none, where
  an `ImagePools` would make one from its first run.
- **One of Shards' `ImagePools` for each kept pool**, its two threads (Shards' pools and room threads) its cost while it
  holds no host. Shards gives no call that ends one pool's hosts (`ImagePools::release` ends the coldest of every pool),
  so a pool let go is its own `ImagePools` dropped, which ends its hosts alone. At most `MAX_BOXES` pools, one for each
  box a node holds, as kiln's simulator keeps one for each run it holds (`SIM_RUNS`): past them a keep is refused `Full`.
  Within one, Shards keys its pools by a run's whole microVM (`wire::Request::exemplar`: image, size, command, ports,
  network, shares, nesting, credential grants and GPUs), so a kept pool warms boxes alike in all of those: two boxes of
  one image and size whose shares differ are two of Shards' pools within it, each warming from its own first run.
- **The room** is its owner's, bound once (`ImageClass::bind_room`, `ImageRuntime::bind_room`): for the image runtime
  kiln embeds, kiln's runner's ledger, the one its sessions are admitted from. A runtime given none keeps no pool: a keep
  or a let-go is refused `Unconfigured` naming `WARM_ROOM` (`Refusal::no_warm_room`), which kiln maps to its typed
  `NO_POOLS`, and it lists none. So does every other runtime (the trait's default), the full node's guest runtime and
  `hex-client` among them.
- **Its keep-alive** is Shards' `ImagePools::KEEP`, D26's (10 minutes, as AWS keeps an idle function: Shahrad et al.,
  "Serverless in the Wild", USENIX ATC 2020, §1), the window kiln names a pool for after its last start (kiln's
  `pools::KEEP`), so neither outlives the other.
- **Its first start is cold.** Nothing is warm until a pool's first run teaches it its microVM (D253). kiln keeps a pool
  as its first start comes, so that start boots on its own, the next teaches the pool, and the starts after it are warm.
  The simulator keeps pools by name alone, none of its hosts warm or starting.

Tests: HEX-IMG-9 (`hex_api::conformance`) on the simulator, on the class given a room and given none, and on hex-node's
image runtime before and after its owner binds its room; a box booting through the pool kept for its image and size
alone, and the room bound once (`image_tests.rs`, `p3_1_*`). On real VMs, owed (ROOT's), on KVM, HVF and WHP:
`crates/hex-cli/tests/image_pools_real.rs`, a kept pool's second start `Warm`, a node holding a warm host admitting no
box past its room, and n warm starts end to end (AC-T7.6.2, the plan's p50 <= 300 ms).

Owed: those runs; the full node's room (its placement's ledger bound as the class's `WarmRoom`) and the node's wire
carrying the three calls; and a box that needs room a warm host holds winning it back (`ImagePools::release`) as its
owner asks, which its owner does today by letting the pool go.

What would falsify it: a warm host started with no claim of its room, or a room held past its budget while hosts
start (then the room isn't its admitter's); a box of a kept pool booting on the class's own boot, or a box of no kept
pool making one.

## 11. A class of its own (kiln plan 01a1dd2e…, P2.2)

The Hex lead's note, approved in kiln plan `01a1dd2e195c7328b7a9bf2b12fb923a`'s P2.2: `Class::ShardsImage` appended,
listed only where the image backend boots and Shards' check of the host says it runs microVMs. Merged from
`root/hex-p22`; its tests are written and owed to ROOT's runs.

- **The class.** `hex_api::Class::ShardsImage`, named `shards-image`, the wire's class tag 4 after every older one: an
  image box's class (`ClassSpec::ShardsImage`), as its inspect and a node's listing say it. Its backend is still the
  Shards guest runtime, whose image class boots it (`BoxSpec::backend_class` answers `Shards` for it), so a node
  dispatches, preflights and recovers an image box as it did, and its record still takes a native scope
  (`Store::put_lifecycle`).
- **Listed where it boots.** A node lists the class (`Runtime::classes`, its hello's classes) only where
  `ImageClass::serves`: its image boot configured, Shards' boot given and its waits given. `host_images` gives the
  boot only where Shards' own check of the host (`shards_api::host_check`, D262), asked once of the `shards` its run
  hosts are as the node starts, says it runs microVMs, and each answer is mapped (`runs_microvms`): `Usable` serves;
  `Unusable` is refused `Platform` (`Shards image VMs`) in Shards' own words, a fact of the host; a check that didn't
  start, that the OS failed or that ended without its answer is refused `Os`, in its words; an answer this Hex doesn't
  read (`Version`, `Wire`: another Shards' `shards`) is refused `Platform`, naming it. A node that serves none lists
  none and refuses an image box's create, and the class's capacity, with that refusal; `ImageClass::serves` gives it to
  a caller that asks before any create (kiln's runner, for P2.4's `node_images`, through `ImageRuntime::class`).
- **Its own accounting.** The class's capacity answer is its host's plane, on which the record authority books each
  image VM at its receipt, with no backing or device of the class's own, and its one route (below); the charge learner
  learns its VMs in a
  class of their own (`hex_sim::model::Class::ShardsImage`), so their priors, and its capacity answer's learned part
  (C8), are theirs alone, while each keeps what it awaited as a Shards VM: its room (`GuestRoom`, C7) and its move
  (`LiveMove`, C5). The client times its box kinds in a table of their own (`client.answer.<kind>.shards-image`), and
  the scheduler counts its starts apart (`start_cost`).
- **Placed only where listed.** hex-sim's model has the class, its bit 8 in `Classes`: the index admits an image VM's
  demand only on a node whose classes hold it (`Verdict::NoClass` elsewhere). A node's facts hold it only where its
  runtime was initialized for it, Shards' check among that, and on the hypervisor Shards boots it on, as Shards' own
  class is held (KVM, Hypervisor.framework; on Windows Shards' check is the one fact of it). Peer rows, launch
  observations, traces and needs name it `shards-image`. The generator draws none, as no recorded trace carries one.
- **Admitted by the scheduler** (merged from `root/p2-finish`, on `root/hex-p22`). Before it, Hex's
  scheduler never admitted an image box: `placement::validate` checked a payload with `BoxSpec::check`, which an image
  spec passes only with its allocation, and then refused a payload naming one, the scheduler deriving it; and the
  initialized admission takes a class only through its routes, which none carried. Now:
  - **Its route.** `RouteKey::check` takes `(shards-image, ShardsDedicated)` of a Linux image with no GPU alone: a
    microVM of its own that Shards boots, of a Linux image as every Shards guest is, attaching no device (D231). Its
    backend class is Shards', as its box's is (`BoxSpec::backend_class`), so a node dispatches and its scheduler checks
    its endpoint as before. Its architecture is the host's (`image::route_key`); no backing.
  - **Its per-box cost** (`image::cost`), offered on that route: Shards' envelope, as `shards_api::envelope` gives
    it, so Hex states none of Shards' numbers: at least Shards' least VM on the host's architecture, in whole vCPUs
    (1,000 CPU milli each, `Allocation::vm_size`'s unit), Shards' memory unit and bytes of disk, so every allocation
    under it is a size Shards boots as asked; at most the node's whole plane, its one physical bound; census-free
    (decision 9): no VMM or guest allowance, what a VM takes past its envelope learned from its windows (C1), its
    priors keyed by the route's cost (C2, `attach_learned`). Its identity is a SHA-256 of the numbers that define it.
  - **Its payload** is checked by `BoxSpec::check_payload`: every check of `BoxSpec::check` but the size an admitted
    allocation gives. The index derives the allocation from the route's cost and the original demand, and the box is
    made with it and its route, as a dedicated Shards guest's is.
  - **Its preflight.** A node preflights an image VM's record as the image class's (`image::preflight`): its spec, its
    size as Shards boots it, and its route, where one was committed, the class's own here. The dedicated guest's checks
    (its configured size and cost identity, its selected receipt) were never an image VM's; an image VM admitted at
    its class's cost could not pass them.
  - **Counted among its node's routes** (`GuestRuntime::project_routes`) where it serves, so a node's offers stay within
    their 64-route eligibility word.

  kiln's runner still sizes its image VMs itself and places by its own balancer, reading the class from its node's
  listing.

Tests (written, not run here): hex-core's `the_shards_image_class_is_tag_4_and_reads_back_identical`; hex-sim's
`initialized_classes_obey_platform_and_feature_filters`, `p2_2_an_image_box_lands_only_on_the_node_listing_the_image_class`
and `p2_2_an_image_vms_placement_row_names_its_route_and_reads_back`; hex-api's
`p2_2_an_image_payload_is_checked_without_the_allocation_its_scheduler_derives` and
`p2_2_an_image_vms_route_is_its_own_microvm_charged_its_allocation`; hex-runtime's
`p2_2_each_host_check_answer_maps_to_the_class_or_its_refusal`,
`p2_2_a_guest_runtime_lists_the_image_class_only_where_it_boots_image_vms`,
`p2_2_the_image_class_cost_is_shards_envelope_on_its_nodes_plane`,
`p2_2_a_node_preflights_an_image_record_as_its_class_admitted_it` and
`p2_2_a_node_counts_its_image_route_where_it_boots_image_vms`; hex-node's
`p2_2_an_image_runtime_lists_its_class_only_where_it_boots_image_vms`,
`p2_2_the_scheduler_admits_an_image_vm_where_its_node_lists_the_class_and_has_room` and
`p2_2_an_image_class_route_admits_its_vm_at_the_class_cost_where_there_is_room`.

What would falsify it:
- A host whose `shards api-check` says usable where an image VM can't boot (then Shards' check is the wrong gate).
- A node whose image VMs' priors move a Shards guest's, or the other way (then the classes aren't apart).
- A run host that isn't the `shards` the check was asked of (then the check vouches for another binary).
- An allocation under the class's cost that Shards boots at another size (then `image::cost` misreads Shards' rule).
- A node whose plane total moves while it runs (then a receipt's cost differs from the offer's at create, refused
  `validate_route`, and the ceiling wants another bound).

## 12. Pause and resume (kiln plan P5.5b)

Decided 2026-11-15, merged from `cp/t7-p5-5b-pause-v2`. Kiln's sessions pause and resume a running agent
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
  while paused, its usage its paused time, its refusals the class's; HEX-IMG-10 holds every runtime of the class to them.

**Alternatives not taken.** Shards' idempotent answer at Hex's seam (the brief names a typed refusal; a caller still
reads it as done). A lock held across Shards' move (a stuck move would stall each next call of the run behind it;
`Busy` tells the caller instead). The paused time taken off `HostUse::window` (D267's suggestion: it changes an existing
field's meaning under kiln's meter). The cgroup freezer for process and oci boxes (P5.5b pauses image boxes; D267
refused SIGSTOP and a freezer for a VM, whose device threads and link they freeze).

**Tests.** HEX-IMG-10 (`hex_api::conformance`) through the simulator in process, through a node's socket
(`the_image_cases_hold_through_a_nodes_socket`) and, ROOT's, on real VMs (`image_real`); over QUIC
(`p5_5b_a_vms_pause_and_resume_go_through_quic`); the class over a model of D267 (`image_tests.rs`, `p5_5b_*`: its
moves, `Busy`, a move its VM didn't make, its end while paused, a mute paused run's usage); the wire's round trips; and
the learner holding a paused VM's charge and booking through SEEDABLE paused windows
(`p5_5b_a_paused_image_vm_keeps_its_charge_and_booking`), which taking those windows as samples turns red.

**What would falsify it.** A VM whose guest time grows while Hex says it paused; a resumed VM whose use passes the
charge its pre-pause windows made more often than one window in SEEDABLE, the windows around its pause left out; a
caller of kiln's that needs a move it lost the answer of answered as done rather than refused typed.

## 13. Component swaps and a share's exec (Shards' D270 slices 3b and 4, D279)

Decided 2026-11-16, built on `cp/t7-v41-swaps` and merged on Hex's local main (not pushed), AC-T7.4.1. The Hex lead's
brief (2026-11-16): bring Hex to Shards' vendor point `f17b8fb77` (image channel v41), expose a member's component swaps
through Hex as P5.5b exposed pause and resume (§12), and carry a volume's `exec` through the image class.

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
  answering nothing (§12); `components` is answered as ever, from what its node holds. Shards' `ImageRefusal::Restart`
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
  generation's streams are takeable anew; its refusals the class's. HEX-IMG-11 holds every runtime of the class to them,
  the fixture naming its component (`Fixture::component`; `image_real` takes one from `HEX_IMAGE_COMPONENT`).

**Alternatives not taken.** Shards' `Now` in one ask (above). Each unread reader dropped rather than read (Shards' pump
then lets its bytes go uncounted; read, they are told). A finite patience (above). A `Runtime::restart` of its own (no
caller asked one; a `NextStart` swap is laid by a later `Now`). `exec` on `Mount` (the OCI class would carry a field it
honours nowhere yet).

**Tests.** HEX-IMG-11 (`hex_api::conformance`) through the simulator in process, through a node's socket
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
