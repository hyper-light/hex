# Shards API seam

Repository: https://github.com/hyper-light/shards
Revision: `740fa37268c9200616f6fd543d96625953e23b81` (main).

The API, ABI, IPC, DNS-name, remote-control, AF_UNIX transport and test-directory crates, license,
formatting, lint configuration and binding policy are copied exactly from this revision, with
the measurement fixtures the API's and its transport's tests include by path. Cargo.toml is Hex's
seven-member API-only workspace scaffold (listed as own in vendor/MANIFEST.toml),
retaining upstream package/dependency/lint settings, and patching shards-remote's hyper-quic, which
upstream takes from hyper-raft by git at the commit Hex vendors (045cf3ff, hyper-raft's main, whose
patch Q34 is the `Connection::idle_timeout` shards-remote's keep-alive reads, D180), to Hex's one
vendored copy (vendor/hyper-raft): one hyper-quic is built, and nothing is fetched. No VMM/kernel
binary or pin is changed here: the `shards` binary Hex boots GPU guests with is vendor/shards-gpu's.

This revision holds what Hex's node speaks to: the direct boot (D215), its guest taken back on every
host, Windows' among them (b4548fcf); launches through a client's native owner (D233); GPU selection
by exact identity (D232), a VM process's refusal of its selection typed in place of a receipt
(`GpuProbeError::Refused`, `Unreported`; 18f59d497); and an image booted under shards-init for an
orchestrator (D231, `shards_api::image`, image channel version 21), with its nesting grant (D234)
and, under it, the run's GPUs by identity (`ImageSpec::gpu_devices`, `gpus=`, version 16, e46299c07;
its workload's cgroup sized by its microVM's CPUs, a853a8138), plugs (D235), destinations by name
and the receivers a published port reaches (D236), a member's stdin, the room its orchestrator's
budget leaves the run and its microVM's cost, each vCPU's guest time among it (D241), each vCPU's
unread cause typed (`shards_abi::guest_time::Unread`, version 14, 2fb8e9ba3), credential grants by
reference (D242), a member's terminal (D244), each member held to its reach of the names kept
(D246), the run's CPU rate (D247) and the period its governor holds that rate over
(`ImageGuest::rate_period`, version 17, 1acbf2cb9), a member's run-time arguments (D256,
`MemberSpec::args`, version 18), how its microVM started, from its image's template or a warm run
host's (D253, `ImageGuest::started_from`, versions 19 and 21; `ImagePools`, which Hex does not use
yet), its credential grants' values leased by the orchestrator's lessor (D242, `ImageSpec::lessor`,
version 20), the setup keys a run's spec may name (D257), a run's end saying why and its run host's
diagnostics kept whole in a file its orchestrator names (D258, `ImageGuest::keep_diagnostics`), its
run host started holding its three standard streams alone through `shards_ipc`'s spawn (D248), a run
whose own command never started ending failed in its VM's words (D249), a run that keeps nothing
past the microVM keeping the network its image links its members to, nothing let past (D250), a lane
to a socket of the run's own command (D251), and the run's own agent-control endpoint, bound by its
run host, whose controller shards-api carries from shards-remote (D252; `ImageSpec::control`,
`ImageGuest::control`, version 15); a guest's size held to `check`'s rule from one source,
`envelope` (672b6fb47); judged waits in place of recorded ones (ca7a04d0d: `Refusal::Stalled`, no
`shards_api::record`, no conformance patiences); shards-ipc's start test holding a child to its
parent's start tick (743c5b32) and its read test holding its wait from below alone (308895ef4); and
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
