# AC-T7.4.1: original allocation in native inspection

Source intake: L1 signed `34d9ce119d1d837aaae187ca4ba96cfa08854354` adds
`BoxInfo.allocation: Option<Allocation>`, serde default/omit None. The API source
is copied exactly. SimRuntime and the existing Linux Node project the original
BoxSpec receipt. ROOT applied only those projection hunks to Node, preserving its
full BoxSpec render signature and other integration changes.

The binary wire appends one option tag after every existing BoxInfo field, then
the admitted resources and original cost if present. Each list element includes
its own tag. Existing call/answer tags stay fixed; source-derived wire identity
changes, requiring a matching rebuilt peer. Both directions validate the receipt's
guest envelope using Allocation::guest; original-demand equality belongs to the
scheduler's retained row and is checked there, not reconstructed from today's
capacity or just the possibly narrower BoxInfo bounds.

The focused inspect/list test retains an allocation after exit, mixes charged
and uncharged list entries, checks every truncation and refuses an invalid
envelope. It is source-only at this checkpoint. The completed preceding696 report
check does not cover this schema addition.

`boxinfo-literals.json` records mechanical None additions outside the incoming
API/Node producers. Current ordinary native process paths advertise legacy cost
None; the in-progress native managed adapters must refuse forged allocations and
the validated guest route must project its original outer receipt. These literal
additions do not assert that the managed/native path is complete. That coherent
implementation, new wire tests, platform compilation and native recovery effects
remain outstanding. No new record, ledger or independent cost provider is added.
