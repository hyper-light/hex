# Canonical retained exit/refusal snapshots (AC-T7.4.1)

Fixed ROOT parent: `7197ed5e86307e9db615f7ee7b9699b8a7cc7f98`.
Public core-wire functions are `snapshot_refusal(&Refusal, usize)` and
`snapshot_exit(Exit, usize)`, returning Result<Vec<u8>, Fault>. They encode
canonical Answer envelopes with id0, current IDENTITY and existing tags/body
codecs. No Refusal clone, new DTO, alternate codec or native authority is added.

The old Writer has NO global preappend guard: scalar/header/raw paths append
unchecked and done can notice a bound only after allocation growth. Merely
reserving the caller bound first is therefore not an allocation-safety proof.
The new snapshot-only modes use the SAME put_refusal / put_exit twice. Counting
starts with Vec::new and performs no output allocation; every byte append is
checked arithmetically against the caller bound, including scalar trailers after
strings. A failed count returns typed Fault before creating the output buffer.
The exact counted length is then reserved once with try_reserve_exact. Fixed
mode checks BOTH logical bound and actual capacity before EVERY append and never
calls a later reserve. Sticky scalar failures are returned by the next fallible
operation or finalization. Exact final length is required. Thus no guessed
padding or second layout-sizing implementation can miss a future codec field.

All ordinary body codecs and tags remain the same. Ordinary Writer::new keeps
its previous mode/behavior; this patch does not claim to fix its inherited
allocation gaps. The normal source-derived wire IDENTITY changes as definitions
change, and canonical equality means the SAME current identity, not historical
cross-cohort interoperability. Fault::Storage remains the existing typed
Exhausted mapping; no unsafe allocator, clone, waiver or manager is introduced.
Every direct output extension is inside Writer::raw; fixed mode validates it
before Vec can grow. Byte strings and header/scalars all use that path. Existing
fixed backing-key reservation/raw paths share these same modes.

Only core wire and new evidence are changed in this producer checkpoint.
Existing client calls/records, CPUdiag API, Cargo and old tests are protected.
L2 receives the exact helper signatures; its record/call interfaces remain its
own. CPUdiag report-tail work is a separate source increment. Three focused canonical
byte/all-bound/trailer/capacity controls are defined for ROOT's joined execution.
No build/test/native dispatch or allocator-fault acceptance was performed here.

Policy: implementer, full Shards7d7 / rulesdf2 receipt retained; exact parent's
Hex CLAUDE matches the fully reread9b source including current Mac rationale.
SOURCE ONLY: tests/builds/native executed by L4 are0; ROOT owns compilation,
allocator-fault/audit gates and native validation. This is no runtime readiness
or physical release claim.

The three new exact selectors are in source.json. They compare every existing
refusal variant plus all Backing causes and representative exit tags to ordinary
Answer id0 bytes without cloning Refusal; test every shorter bound; retain the
critical Exhausted string+usize trailer case with zero counting allocation; and
check fixed-mode scalar overrun and a deliberately absent capacity refuse
without growing. A Vec capacity-overflow refusal exercises typed Storage mapping
without requesting a huge allocation. These are defined unit controls, not an
executed allocator fault/mutation or broad wire campaign. Old test source is a
byte-identical prefix. No old wire suite or client case is requested again.
