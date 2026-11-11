# Original unallocated cancellation ACK without keeper

Fixed exact ROOT ff765c021b842e02ea872abdec0809d802703d0b. SOURCE ONLY,
UNCOMPILED/UNRUN: one new actual WinRuntime::preflight_managed control, with no
production change and no mutation to ROOT's immutable32 cohort.

The exact native owner fixture has no local slot and no configured keeper; no
native constructor, API load, HCS, WMI, job, adoption, process, or keeper start is
called. Its cost is checked through the real native cost validator. A valid
Prepared Pool row is decoded into the same key/cost/scope and legally accepts an
unallocated/no-init/no-exit cancellation Released row through the actual core
transition validator. This declares fixture metadata, never a native ScopeReleased
result, native termination observation or synthesized exit.

Prepared metadata preflight must succeed first. The cancelled row is then checked
by the same concrete WinRuntime function. Its in-memory bytes, absent local owner,
absent keeper, empty private directory and zero feature reads are verified BEFORE
the final oracle requires that metadata-only check to remain readable for the
explicit later Index ACK. Common Store/Tx canonical authority/key validation is
covered separately; this unit isolates the native half of the new feature edge.

The current source no-keeper branch accepts present||Prepared, so the unallocated
Released case is a source hypothesis. No failure is claimed until ROOT actually
runs this exact unit and confirms the intended refusal after the preconditions.
Existing CLI outliving-with-keeper behavior is distinct. All preexisting test bodies
and production bytes are byte-identical to ff765. No remedy, fallback or weaker
scope/identity/cleanup rule is included. If the original is relevant red, ROOT
reviews the smallest exact unallocated-shape change; allocated/unknown ownership
and presence/error refusals must remain strict.

ROOT selection: windows::preflight::tests::original_unallocated_released_without_keeper_checks_same_submission_metadata
No L3 test/native/job dispatch. Scoped rustfmt and git diff --check only. Policy7d7e
full read/rulesdf2 implementer receipt persists; ff765 Hex CLAUDE is byte-equal to
fully read112297 CLAUDE. R01–R16 remaining native/platform/full gates/profile work
stays ROOT-owned; this is neither a current native failure nor a completion claim.
