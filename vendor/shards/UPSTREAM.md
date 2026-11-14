# Shards API seam

Repository: https://github.com/hyper-light/shards
Revision: `16451e19f172cfb29882efcab6502cc43b8bd061` (main).

The API, ABI, IPC, DNS-name, remote-control, AF_UNIX transport and test-directory crates, license,
formatting, lint configuration and binding policy are copied exactly from this revision, with
the measurement fixtures the API's and its transport's tests include by path. Cargo.toml is Hex's
seven-member API-only workspace scaffold (listed as own in vendor/MANIFEST.toml),
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
orchestrator (D231, `shards_api::image`, image channel version 27), with its nesting grant (D234)
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
host's (D253, `ImageGuest::started_from`, versions 19 and 21; `ImagePools`, which Hex does not use
yet), its credential grants' values leased by the orchestrator's lessor (D242, `ImageSpec::lessor`,
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
`crates/afunix/src/lib.rs`).

## Recorded changes

None. Hex's two changes at e82b9403 are upstream at this revision: its run host started holding
its three standard streams alone as D248 (9bdd4a7ec, through `shards_ipc::spawn_with` rather than
Hex's `image/spawn.rs`), and each member's reach of the destinations kept as D246 (7a4ef5c02,
image channel version 12, where Hex's had carried it as version 9).

Every change Hex carried at 33df95b0 (patches 0001-0031) is upstream too: Shards'
hex-upstream-v1 maps each to its commit (docs/research/measurements/hex-upstream-v1/
README.md), but 0004, 0005, 0007 and 0029, Hex's own campaign over the borrowed connector,
which Shards classed as Hex-only. That campaign is now a test of Hex's on its dependency, its
body unchanged: crates/hex-client/tests/shards_borrowed_contract.rs.
