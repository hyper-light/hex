# Completed wire refusals are separate census outcomes

AC-T7.4.1, root coordinator record, 2026-11-10. Source bba3d814cfa8373d589482dfbaa38b9bfa792d03.

The common synchronous client previously called `Patience::answered` for both an
ordinary wire answer and `Answer::Refused`. The timing and existing online maximum
were intentional, but the census file could not separate those categories in a
multiplexed connection. The earlier935f captures therefore retain their original
classification limits. Their old unmarked rows are not retrospectively declared
successful calls or converted into new seed inputs.

The smallest change is at that same completion point: a typed wire refusal calls
`Patience::refused` with the same single elapsed measurement. It performs the same
`seen.fetch_max(stored(took))` update and emits `Sample::Refused` only to an installed
census sink. No deadline, seed, retry, protocol message or transport behavior changes.
The optional file uses `name !nanos`; old `name nanos` and `name >nanos` remain
answers and run-outs. Both parser consumers preserve refusals separately from
answer distributions. No refusal text is matched.

This is a wire outcome, not a workload verdict. `Answer::Scheduled` may describe
pending work or contain a refusal notice. Actual physical lifecycle controls and
per-call/process provenance still establish which observations are eligible for a
later calibration decision. The new marker alone does not establish that fact.

## Focused and actual controls

The implementer returned signedf6fc7d3f with two focused Rust cases, eight
competitive-parser and three scheduler-report cases, and touched-crate clippy
passing. Root reviewed the source and integrated it as signedbba3d814. The Rust
tests exercise the actual common call with a typed refusal and verify identical
seeded/measuring maximum and deadline behavior after equal observations.

Root r461 built fresh host and observer binaries in both profiles. All eight wire
identity source inputs remained byte-identical to the retainedcc static guest, so
the guest binary/initrd were reused with their original identities explicitly
recorded. No Shards API/vendor/kernel source or candidate pin changed.

Root r465 ran ordinary nodes, real Shards guests and forced-key OpenSSH in both
debug and release. Each passed two oversized633MiB/632MiB refusals, two valid64MiB
physical creates/deletes, baseline restoration and identity-checked guest shutdown.
The new files contain explicit refusals in native and guest client rows and the
multiplexed peer control path. Debug VM16783/start12216828351394 and release
VM17124/start12216919165564 both ended Stopped. The input/report hashes, emitted
categories and physical controls are retained in wire-marker-guest-bba3d814.json.
These N1 controls are not timing seeds. Root subsequently reconstructed every
raw SHA/count/typed marker and filename attribution with the fixed verifier;
both reports were consistent and their hashes are retained separately from
the harness's own summaries.

Root r467 built the native Windows CLI and ran a real private scheduler with no
runtime backend or VM. A Status call returned an ordinary reply with an unmarked
row; a wrong-realm Describe returned exit1 and exactly one `!` row for that call.
The fixture reaped the exact owned server and both CLI processes. Source and result
are census/native_wire_marker.ps1 and wire-marker-windows-bba3d814.json. This is
native wire/transport evidence, not Windows HCS, WHP or GPU workload validation.

The PowerShell adapter is benchmark tooling because this runner's Python entry is
a WindowsApps placeholder. It invokes the ordinary production CLI; it does not
implement another protocol client. Its120s outer cancellation budget and explicit
row allowance are the same existing administrative/diagnostic inputs, never runtime
timing seeds. It changes no user SSH configuration, service or runner installation.

Root r463 also passed 300 native Windows core and 45 client library cases, with
two ignored cases, plus workspace all-target clippy. Root r462 passed the full
seven-target/workspace/vendor gate at bba3d814; gates-bba3d814.json preserves its
commands, ignored cases and log hash. Root r476 removed the common-call refusal
classifier in an isolated Windows copy: the real marker control failed for the
expected missing `!`, with all exact processes reaped. Restoring the classifier
passed; wire-marker-mutation-bba3d814.json retains both source/binary identities.

The debug and release N1000 captures r468/r469 each completed 1,000 local and
1,000 SSH physical create/delete cycles, restored the empty baseline and stopped
their exact guests. Root independently verified every SHA and reconstructed
every row/outcome in 84,610 release and 110,302 debug census files (194,912 total).
The new summaries are census-{release,debug}-bba3d814.json. These are loaded,
unarmed measuring captures, not a test of installed timeout seeds or a quiet
performance comparison. No timing seed has been installed.

Root r489/r490 then used separate fresh owned guests for 1,000 actual SDK Hellos
per profile, followed by the unchanged physical boundary controls. All Hellos
advertised the requested API class; after all borrower drops, a live capacity RPC
still named the same PID/start identity and the physical baseline was unchanged.
Each guest subsequently ran/deleted both valid boxes and ended Stopped. Root
also reconstructed all raw files. guest-hello-{debug,release}-fa5c7e4e.json retains
the helper's separate source/binary/profile identity and the exact SDK rows.
These prove the stated wire/lifetime controls, not FD/task/allocator leak freedom.
The helper's census timing point measures the SDK Hello answer after dial/token
setup; it is not total CONNECT or VM boot latency.

All records retain their original revision/profile/receipt identities. Remaining
calibration, native Linux/root-lab, resource-return and unsupported-platform checks
are unrun; no final landing or whole-goal completion is claimed.

## Resource and scope limits

The census retains its existing explicit line bound and full-record marker. There
is no new queue, cache, clock, ownership authority or allocation claim. The same
maximum learner remains authoritative. Wire identity is unchanged, and parser
tests retain legacy answer/run-out behavior. Live failures and cancellations remain
ineligible for automatic successful-call attribution. Native Linux/root-lab and
unsupported/unavailable hardware checks remain unrun, not inferred from cross lint.
The Windows arm64 VM remains paused.

## Verifier nonregular input correction

Root found that the new verifier opened a leaf for reading before checking its
file type. A private FIFO with no writer therefore blocked at open. Root r477
reproduced this against exact implementer source53451b17, cancelled it at the
existing 120-second administrative boundary and reaped only its recorded child.
No VM or capture input was used. Opening the leaf with O_NONBLOCK lets fstat reject
the FIFO before any read; ordinary regular-file reconstruction is unchanged.
The same FIFO then returned the expected typed verifier error, and nine synthetic
tests passed. verifier-nonregular-controls.json records the red/fixed controls.
Root subsequently used this fixed reader for every actual verification above.
