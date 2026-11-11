# AC-T7.4.1: admission-to-recovery allocation receipt

ROOT imports the five pure owner files from signed L1
`30e89d7298fc929620cd70908ba8cc480c407c65`, including model costs, Index charge
handling, Owned/Ran persistence and Msg::Start allocation. The node intake uses
only the10ef-parent-to30e89 delta in the nine placement/scheduler paths. All nine
three-way merges completed without conflicts, retaining ROOT integration changes;
old/base/new/composed hashes are in `consumer-node-merge.json`.

Candidate fit and receiver admission call the same checked API cost formula.
Owned/Ran rows and Start retain both the original Demand and the immutable
allocation; resource release and recovery use that receipt rather than repricing
against a refreshed runtime profile. The bounded a1 atom extends existing records
and messages, not a second resource ledger. Driver derives BoxSpec allocation from
the retained admission and compares inspect/create replies before start/adopt.
Mismatch reports typed lifecycle uncertainty and retains claims.

`admission-literals.json` lists explicit legacy None costs added to unchanged
NodeSpec literals outside the supplied owner files. No measured profile is
invented. `add-admission-literals.py` is the source-edit method; format was run on
the changed Rust paths only. This checkpoint has not compiled or executed the new
cost controls. Earlier wire/API proof is at its recorded source, not this one.

The shared CPU OCI VM's once-only backing reservation is a separate outstanding
integration: existing RuntimePool must retain its actual authority/generation and
backing charge, while inner work consumes guest capacity. Native scope enforcement,
profile provenance, observed-use normalization and complete release still require
the matched runtime adapters and actual ROOT validation. Legacy unknown GPU load
also remains unchanged pending the frozen original control.
