Exact ROOT005 Mac original/control port, source only, 2026-11-11

This branch starts exactly `005f3a23bbfb173718a520160a48c83f66ca56d3`. It contains only
one cfg(test) registration, the new named test leaf, this guide and the source manifest.
No production body, unsafe, dependency, helper, ABI or manifest is changed. No compiler,
Clippy, test or native operation was run by L3. ROOT owns the matched build and admitted
bounded native execution; there is no semantic red/green yet for this original.

The preserved older original is `cc2bfd1445e414fe9f491fc5eb8fae251a4feb5e`, whose leaf
`macos_scope_completion_tests.rs` has SHA256
`80c74c8bdf7ccafc064a06874c61fed910e08ff25633c391a5021bb2b6b66a58`.
Its production baseline included the unintaken 2ef/fab producer. No original commit,
leaf, reference or raw result is modified here. This new original deliberately exercises
ROOT005's actual `Scope::new(record, Ownership::Child)` and unchanged
`ProcessBox::terminal`; it does not import the 6459-line producer to make a control run.

The API compatibility edits are confined to the test leaf:

- Construct the current Scope with Ownership::Child; omit the future CpuControl inputs.
- Use ROOT005's unchanged ProcessBox::prepared, Shared and sole BoxProc waiter.
- ROOT005 has no kernel peer-PID wrapper. Kernel peerPID is explicitly UNAVAILABLE in
  this record and raw facts. No 2ef sys wrapper or new unsafe is copied to supply it.
  Instead the parent joins its unreaped direct Child.id to native birth. That immutable
  controller sends its actual spawned Child.id over the same private control connection
  before the parent accepts the escaped child's handshake. The receiver requires that
  exact child PID, matching positive native birth, actual session/group and same UID.
  This is the user-ACKed private test compatibility fact, not cc2's stronger kernel
  connected-peer-PID proof or a product identity/authentication mechanism.

All substantive original mechanics remain: an owner-only tempfile socket, the explicit
controller permission to exit only after the escaped child is known live, a deterministic
watcher-not-started interleaving, the actual production snapshot, retained WNOWAIT
controller identity, native birth/group/escape facts, and cleanup before the semantic
oracle. The escaped child calls safe setsid, is outside the controller PGID, and stays
live on the exact private lifeline while the controller ends. The oracle refuses a
Released result in that state. This is not a VM, GPU, race-frequency or CPU-cap test.

The parent first commands the exact escaped peer to exit and observes its pre-registered
native exit. The same native BoxProc waiter reaps the direct controller. A final birth
query must show that escaped incarnation absent/replaced. If that is still unknown, the
fixture reports INCOMPLETE; it is neither the original semantic red nor a green. A
NOTE_EXIT event alone is not claimed as whole-scope absence or a reap. Setup/timeout/
socket/cleanup errors are separate failures, with their raw facts preserved. The
39.507-second existing macOS test hang guard, private control/lifeline sequencing and
deadline remain unchanged. ROOT supplies a short owned TMPDIR and finite whole-process
guard, source/executable hashes, and exact child cleanup receipts.

Default ignored selectors remain:

1. `macos::process::scope_completion_tests::unobserved_escaped_descendant_prevents_scope_release`
2. `macos::process::scope_completion_tests::native_nproc_zero_refuses_spawn_and_hard_raise_but_preserves_threads`

`native_scope_worker` is only the exact internal re-exec role started by those parents;
do not run all ignored tests. The independent NPROC test still lowers only its isolated
nonroot child's soft/hard limit, checks spawn EAGAIN and native hard-raise EPERM, and
starts/joins a thread. It is not the production remedy. The independent Python fork
control stays at preserved cc2 and is not part of this narrow four-path port.

Same-owner closure research continues separately: NPROC0 does not account for pre-seal
workers, prevent their own PGID changes, prove pre-main or GPU-service lifetime, or grant
scope release. A complete implementation must retain exact PID/birth/seal receipts and
positive exits at the existing owner. This port changes no release rule or canonical
charge. Current ROOT source/native outcomes for other subset/LUID/cgroup controls are
separate; they do not substitute for this unrun Mac original.
