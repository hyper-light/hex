# Shards API seam

Repository: https://github.com/hyper-light/shards
Revision: `04481e36be379b1c0a26575e66ef9c254be87865` (main).

The API, ABI, IPC, DNS-name, remote-control, AF_UNIX transport, Apple-frameworks and test-directory crates, license,
formatting, lint configuration and binding policy are copied exactly from this revision, with
the measurement fixtures the API's and its transport's tests include by path. Cargo.toml is Hex's
eight-member API-only workspace scaffold (listed as own in vendor/MANIFEST.toml),
retaining upstream package/dependency/lint settings, and patching shards-remote's hyper-quic, which
upstream takes from hyper-raft by git at the commit Hex vendors (e0613f52, patch Q35 on hyper-raft's
branch p/q35-handshake-progress-v1, bound for its main: Q34's `Connection::idle_timeout`, which
shards-remote's keep-alive reads, and Q35's `Connection::handshake_progress`, by which its listeners
let a handshake that answers but never advances give its place, D180), to Hex's one vendored copy
(vendor/hyper-raft): one hyper-quic is built, and nothing is fetched. No VMM/kernel
binary or pin is changed here: the `shards` binary Hex boots GPU guests with is vendor/shards-gpu's.

This revision holds what Hex's node speaks to: the direct boot (D215), its guest taken back on every
host, Windows' among them (b4548fcf), and its use read on its vsock socket (D260, 867053aa1:
`ManagedGuest::host_use`, its microVM's use in an image run's stats' form, `Microvm::vmm_ns` its VMM's
own CPU, `Refusal::HostUse` an answer none of the protocol's; Hex reads it nowhere yet); launches
through a client's native owner (D233); GPU selection
by exact identity (D232), a VM process's refusal of its selection typed in place of a receipt
(`GpuProbeError::Refused`, `Unreported`; 18f59d497); and an image booted under shards-init for an
orchestrator (D231, `shards_api::image`, image channel version 30), with its nesting grant (D234)
and, under it, the run's GPUs by identity (`ImageSpec::gpu_devices`, `gpus=`, version 16, e46299c07;
its workload's cgroup sized by its microVM's CPUs, a853a8138), plugs (D235), destinations by name
and the receivers a published port reaches (D236), a member's stdin, the room its orchestrator's
budget leaves the run and its microVM's cost, each vCPU's guest time among it (D241), each vCPU's
unread cause typed (`shards_abi::guest_time::Unread`, version 14, 2fb8e9ba3), credential grants by
reference (D242), each with the placeholder its member sends where the value goes, the injector's
decision and the grants' check in `shards_dns::credential` (D242's step 3, version 22, 51e674604), a
member's terminal (D244), each member held to its reach of the names kept
(D246), the run's CPU rate (D247) and the period its governor holds that rate over
(`ImageGuest::rate_period`, version 17, 1acbf2cb9), a member's run-time arguments (D256,
`MemberSpec::args`, version 18), how its microVM started, from its image's template or a warm run
host's (D253, `ImageGuest::started_from`, versions 19 and 21; `ImagePools`, through which Hex's image
class boots the boxes of each warm pool its owner keeps, P3.1), its credential grants' values leased by the orchestrator's lessor (D242, `ImageSpec::lessor`,
version 20), the setup keys a run's spec may name (D257), a run's end saying why and its run host's
diagnostics kept whole in a file its orchestrator names (D258, `ImageGuest::keep_diagnostics`), the
run's microVM saved whole for its owner (D259, `ImageGuest::save`, version 23, 8a9fa413d; Hex asks
none yet), a stop for a boot under way (D231, `BootStop` and `Images::boot_stoppable`, ed1136305: the
run host killed with its process group, or on Windows its job, and the boot answering
`Ended(Stopped)` once that host is reaped, on Windows once every process of the run has ended
(9a65eb9bb), and every holder of its stderr has closed it), the sign-ins
an orchestrator's injector captures and refreshes for a run (D242's step 8 slice 2b, version 24,
`ImageSpec::sign_ins`, the lessor's `capture`, `refresh` and `revoked`, defaulted; Hex grants none
yet, its pass-through goal 13's), its
run host started holding its three standard streams alone through `shards_ipc`'s spawn (D248), a run
whose own command never started ending failed in its VM's words (D249), a run that keeps nothing
past the microVM keeping the network its image links its members to, nothing let past (D250), a lane
to a socket of the run's own command (D251), and the run's own agent-control endpoint, bound by its
run host, whose controller shards-api carries from shards-remote (D252; `ImageSpec::control`,
`ImageGuest::control`, version 15), with its members' own lanes and its log lanes (D238, 8bb81584d,
638c05244; `Execs::log`, which Hex does not use yet), an exec's output done once its end has reached
its controller's lane (f24a4644e, 644ed4f64), and an orchestrator's member lanes: its run host
serving each member's own lane (running, its signals steered through init, its end told), with the
controller's handles (`Execs`, `Member`, `MemberControl`, `MemberSteering`, `MemberEnd`,
`Unsteered`) and its one source of exec admissions (`exec_admissions`) re-exported by
`shards_api::control` (c1773281f, 38ca48283); a member's MCP bridge, its socket relayed to its init's
port and its connections paired with its controller's asks (D254, 6f786c91c, af8f1ebe5;
`MemberSpec::mcp`, version 25), which needs the run's agent-control endpoint, so that Hex, asking
none of an image VM's run, refuses a member asking one by its field, as Shards' own spec check does;
each credential and sign-in grant's field held to the two-byte length the network process's wire
carries (`shards_dns::credential::WIRE_MOST`, 17fc36ecd), refused by field as a spec's
(`ImageRefusal::Spec`), which Hex maps as it maps every spec refusal and bounds no further; a run
host that ends before its microVM holds its layer leaving no layer file behind (D253, 712f791b3);
the roots a run's injector trusts toward its credentials' hosts, its host's and those its
orchestrator names (D242, `ImageSpec::upstream_roots`, version 26, b6814785d, fd61d46da), each
checked at request and refused as a spec (`an upstream root`), its environment's
`SSL_CERT_FILE` and `SSL_CERT_DIR` trusted on no OS (Hex names none);
a member's end typed, `Signaled(n)` beside its exit, from its namespace's reaper to both ends (#72,
7aab838e0: `shards_abi::run::members::End`, `MemberEnd::Signaled`, version 27; its lane's
`exec::End`, `Member::Ended` and `MemberEnd::wait`), which Hex maps to `Exit::Signal(n)`, never
folded into the status `docker run` reports; a member's MCP connections' handles re-exported by
`shards_api::control` (`McpConnection`, `Output`, `Input`, `Got`, `ExecControl`; D254);
a run's end carrying what its microVM used in all (D260 amended, 1e2d403ec, version 28:
`ImageGuest::wait_for` and `ImageRefusal::Ended` give a `RunEnded`, its `FinalUse` read or why not,
`Unused`), of which Hex reads the end alone yet; a member's channel's end told typed
(`ImageGuest::wait_channel`, `ChannelEnd`; control-01, a3352f573), which Hex calls nowhere; a managed
guest's final use recorded beside its vsock socket (D260 amended, 4d86af2c4: `ManagedGuest::host_use`
answering `ManagedUsed`, its end with its final use once it has ended, `wait_ended`, `final_use_file`),
which Hex reads as it read that end before; the agent-control lanes' asks sent once, each answer waited
for within its caller's patience on a handle (155b5ee2e: `PendingMember`, `PendingExec`, `PendingMcp`,
`PendingLog`, `PendingSocket`, `MemberSteering::signal`'s patience and `steered`, `Fault::Awaiting` and
`Unawaited`), on which Hex's member signals wait within their W* and follow on past it, told, and its
lanes' dials within their deadlines; an image run's GPU report (D233, fad757af4, 925434277, version 29:
`ImageSpec::gpu_report`, `ImageGuest::take_gpu_report`, `ImageRefusal::GpuProbe`), which Hex asks of no
run; and a borrowed connect with no clock, ending at its OK, its connection's end or its VM process's
(9156058fb: `connect_borrowed_before`), Shards' answer to the review's D-3; and, past d91b5ce6b,
each lane's reader holding its frames to its own grammar's bound (remote-08, 37b1b165e: `MAX_FRAME`
gone, a reader's most held by the host's memory, `shards_ipc::host_memory`), an invitation living as
long as its owner says (30645fa69), the pause between looks learned where they run (2275e3829,
`shards_abi::pace`, `shards_api::pause` its re-export) and its crates rustfmt-clean again (06388950f):
at d91b5ce6b `crates/dns/src/lib.rs` was not, and Hex's `cargo fmt --all --check` holds its vendored
crates to it, so Hex takes a later revision than d91b5ce6b; and past ce80cbb37, a run's stop
returning released, as a boot's stop does (D231, c133bfb56: `ImageGuest::stop` returns once its end
is told, its run host exited and reaped, and every process the host started, its VM, network and
share processes, gone, their diagnostics closed; on Windows the host's group ended too, a failure
there `ImageRefusal::Os`), so Hex's delete frees nothing a run still holds (the review's B-4), and the
agent-control driver's waker and wake connected to each other, so the wake takes datagrams from the
waker alone (b6727996c); and past 2737fc4ec, a run's endpoint telling every socket it holds (D252
amended, d3950c7d1, image channel version 30: `ControlAt::waker`, `wake` and `sockets`), which Hex's
inspect carries (`BoxInfo::control_wake`) and its grants check holds a run's sockets to, Windows' UDP
table showing no socket's peer; a controller's socket lanes driven from one thread on one wake (Hex's
D-10, 9f5b1f75); a killed managed VM's AppContainer profiles released (D141, 16bbdf34:
`release_app_containers`, `Refusal::Release`), which Hex calls nowhere yet; and a stream shared without
an inheritable duplicate (D215, Hex's D-5, a168c2a3: `Stream::read_shared`, `write_shared` and
`into_socket`, `&dyn Stream`'s `Read` and `Write`, a `try_clone` no child inherits), which Hex's test
streams implement; and a member's stdio read and written without waiting, on the controller's one
wake, and an exec's hangup (Hex's asks after D-10, 5142c6a7: `Output::try_next_record`,
`Input::try_write`, `ExecControl::hangup`; an `ExecControl` let go before its exit was heard now hangs
its exec or MCP connection up), Hex's MCP connections keeping theirs until they read their exit;
a guest's size held to `check`'s rule from one source,
`envelope` (672b6fb47); judged waits in place of recorded ones (ca7a04d0d: `Refusal::Stalled`, no
`shards_api::record`, no conformance patiences); shards-ipc's start test holding a child to its
parent's start tick (743c5b32) and its read test holding its wait from below alone (308895ef4);
shards-remote's handshake judged as a send is, a closed port's refusal read from a send too
(233c715f0, D180), so its loopback test of a closed port no longer waits on a refusal it never reads,
a handshake that answers but never advances giving its place after a whole window without progress
(4fa48def2, D180, by Q35's `handshake_progress`), and its loopback test's waits judged by their
subjects' progress (9ea642f0f, 0fdc1ffec), its window's wait unix's alone, as its one caller is
(824653415); the host endpoint resetting a cut lane (D251 amended,
8754b451f), on Windows too (e1973ee50), and the guest legs' cure (shards-init's relay, the vsock
muxer's host writes, Windows' warm pass; 275b4e178); and
an AF_UNIX read on Windows that waits in WSAPoll, which sees a peer's end that a waiting recv can
sleep through (shards-afunix, 7ea3ce27, D140, PM M536). shards-api depends on `crates/ipc` (the run
host's spawn), `crates/dns` (a credential grant's host names, checked as DNS names, D242) and
`crates/remote` (the endpoint's controller, D252), so each is taken.
The take list adds `docs/research/measurements/api-flakes/spawns.rs` and `half-close.rs`, which
the API's and shards-afunix's tests include by path at this revision (`crates/api/src/client.rs`,
`crates/afunix/src/lib.rs`), and `docs/research/measurements/borrowed-no-clock/focused.rs`, which
`managed-borrowed-nowait/focused.rs` includes for the tests of the borrowed connect with no clock
(9156058fb).

Past 9975d1f55 (image channel 36) this revision speaks image channel 51, and Hex's node takes what it
adds: a share given to members as they start, beside the run's own root (D277, 325457a88:
`ImageShare::members`, version 37), which Hex passes from a share's own member scope
(`hex_api::Mount::members`); a member's working directory its guest could not enter refused typed as
its members start (D277 amended, f84089c9f, 806841601: `ImageRefusal::Workdir`, versions 42 and 43,
told to `ImageGuest::wait_member`); a member's generations, started again in its running microVM, and
a component swapped into it by digest or its image's back (D270 slices 3b and 4, 5be1ca21a,
c0aee1b38, 600ab556d, c91fe3614: `ImageGuest::restart`, `swap`, `restore` and `components`,
`RestartRefusal`, `SwapRefusal`, `SwapWhen`, `Swapped`, `MemberEnd::Killed`, versions 38, 40, 45, 46,
48 and 49), every member stream and end now named by its generation, 0 its first: Hex restarts and
swaps none, so its members' streams and ends are their first generation's; each member's own reach
past its microVM within its ceiling (D278, f6e0b057b, 3f6734697: `ImageGuest::member_egress`,
`ImageSpec::egress_ceiling`, version 39) and a member's grants on a network shared by runs (D285
slices 4 to 6, 94b5276d8, 57f1e6a39, 4348c8c1b: `ImageGuest::connect`, `disconnect`,
`shards_api::networks`, version 50), neither of which Hex asks yet, their refusals typed
(`NetRefusal::Unheld`, `NoSuchNetwork`, `NameTaken`, `Exhausted`, `PairedElsewhere`, `Ipv6`, `Grant`,
`Overlaps`); whether a share's or a plug's files may be run (D279, f17b8fb77: `ImageShare::exec`,
`Plug::exec`, version 41), which Hex leaves as every share was before, run from; its members' stdio
served as lanes of its agent-control endpoint where its control asks it (D252, 10c82ef2c:
`ControlAsk::stdio`, version 44, `shards_api::control::member_output` and `member_input`), which Hex
does not ask, taking its driven members' stdio from the run as before; its run host's budget holding
its endpoint's link and its members' first stdio lanes, a run it has no room for refused before its
members start (D252, d8935aca1, 32b27e6c3, 3c76f6c05: `ImageRefusal::Capacity`, version 51); a volume
plugged with a bound, and a home's volumes listed and cleared (D281, 4a5178d9a, d704727d5, a3025e473:
`PlugSource::Volume { name, bound }`, `PlugRefusal::Bound` and `Clearing`, versions 47 and 49,
`shards_api::volumes`), which Hex's image class plugs into a box's members with their bounds, lists and
clears (P6.2); the agent-control endpoint's report of
every lane its controller was granted, MCP bridges among them (D254 amended, f99a3900b:
`Linked::report`, `LinkReport`, ALPN `shards-control/2`; `Dialed::lanes` gone), which Hex's
`Runtime::link` answers (kiln's step 35); a name no grant reaches answered NXDOMAIN at once (D46, 4348c8c1b: `shards_dns::denied`,
which was `refused`), which Hex calls nowhere; and on Windows a guest's vsock connection on two named
pipes, its way down opened first (D272 S4, 0bef06525: `Stream::handle` the way down's pipe,
`OwnedSocket` both ways' handles), which Hex's Windows client bridges to a local socket pair, as it
bridges a node VM's lanes, since its readiness waits and its node's completion port wait on sockets.

Past f1e8a097b this revision still speaks image channel 51 and adds nothing Hex calls. shards-api now
takes Shards' Apple-frameworks crate on macOS (`crates/apple`, `shards-apple`, bound when first used:
its host's block storage's completed I/O, read through IOKit in `storage.rs`), and on Windows reads its
host's physical disks' completed I/O (`IOCTL_DISK_PERFORMANCE`, windows-sys's `Win32_Security` and
`Win32_System_Ioctl`), so that D215's judge holds a wait in a kernel its host's only while those disks
complete I/O (D215 amended, 927ba0119, 2726a8deb). So the take list and this workspace's members add
`crates/apple`, whose only dependency is libc. Beside it: a daemon's deadline files read by name
(D209 Part 10 and D163, 03c0b8603, adc48df03: `shards_ipc::deadline_text` and `deadline_in` in place
of `grace_text` and `grace_until`, `RETRY` beside `GRACE`), and the demand rule's refill window made
public (D26, e80ae3775: `Demand::window`), neither of which Hex calls.

Past 8d6029e4d this revision, Shards' main as its version became 0.8.0, still speaks image channel 51,
and no file Hex takes changes a type, a signature or a dependency: D215's judge reads a Windows
process's runnable threads and learns no rate from an answer that read no work (c16719483); an
AF_UNIX path shards-afunix refuses says which and why, where a bare InvalidInput said neither
(a982c9be0, D276 amended); shards-testdir says each panic of a test process on stderr as it happens
(c0c852156); and remote's loopback test goes on past Winsock's WSAECONNRESET (d1b1e1412).

## Recorded changes

| File | Change | Why | Recorded as |
|---|---|---|---|
| `crates/api/src/image.rs`, `crates/api/src/image/{wire,pool,session}.rs`, `crates/ipc/src/lib.rs` | Shards' D286: an image run given shares another server serves (`ImageSpec::served`, `ServedShare`, `ServedStream`; image channel 52, `wire::Request::served`, `ServedAt`; the streams handed beside the request, on Unix on the run host's links socket, `shards_ipc::kind::SERVED_SHARES` (117), on Windows duplicated into its process; no pool's, no save) | kiln's microVM engine shares each agent's slates volume into its guest over slates' own virtio-fs device, a stream handed to Shards with no host mount between (kiln goal 01a1ec34599974448f6b7663ddb07001; docs/design/shards-image-class.md §2b). Upstream on Shards' branch `agents/g265-kiln-shards-microvm-served-image-share` (4ff40fc99, on its main 4776b2e56; e4e0118c8 the same change on this revision), which ROOT lands on Shards' main after its v0.8.0 tag. Dropped once Hex takes a Shards revision holding it. Its run host's side (`crates/shards`) is in no crate Hex takes: kiln's copy of Shards, which builds the `shards` kiln carries, holds it whole | `0001-shards-api-shards-ipc-an-image-run-takes-shares-another.patch` |

Hex's two changes at e82b9403 are upstream at this revision: its run host started holding
its three standard streams alone as D248 (9bdd4a7ec, through `shards_ipc::spawn_with` rather than
Hex's `image/spawn.rs`), and each member's reach of the destinations kept as D246 (7a4ef5c02,
image channel version 12, where Hex's had carried it as version 9).

Every change Hex carried at 33df95b0 (patches 0001-0031) is upstream too: Shards'
hex-upstream-v1 maps each to its commit (docs/research/measurements/hex-upstream-v1/
README.md), but 0004, 0005, 0007 and 0029, Hex's own campaign over the borrowed connector,
which Shards classed as Hex-only. That campaign is now a test of Hex's on its dependency, its
body unchanged: crates/hex-client/tests/shards_borrowed_contract.rs.
