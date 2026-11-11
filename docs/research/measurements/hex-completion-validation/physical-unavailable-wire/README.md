# Qualified VM observer diagnostics on the existing physical report

Implementer source only, based on signed `ac01b71abece322305b9ee7d753338a3de87e436` (snapshot `01fe011d`, fixed ROOT `7197ed5e`). Policy `7d7e0855`, rules `df2acf47`; full-read receipts retained. Exact dependencies, unchanged paths and source hashes are in `source.json`.

The required API is L1 `c772f964f683880142eeed1c6d904c3938a09f03`. This private branch does not overlay API files. ROOT must join that API, reexport `BackendUnavailable`, and port outside literals. Apply the owned wire/test delta; current client/consumer source remains ROOT-owned.

A nonempty `PhysicalReport.unavailable` appends a u32 count and the existing class/refusal bytes. Each qualified entry is exactly 51 bytes: class (1), Backing refusal tag (1), canonical BackingKey (48), lifecycle cause (1). The API allows only the two VM classes, unique, with Pending/Busy/Uncertain Backing refusals. No Call/Answer/refusal tags change. `RuntimeRoutes.admit` is internal and adds no public wire field. Source-derived wire identity updates normally.

Empty reports retain the old payload exactly; omitted and explicit-zero tails decode to empty. A partially present new tail is refused. Before body writes the encoder checks the API and total byte bound, including the count and scalar trailers. The decoder checks count <=2 and minimum bytes before parsing/reserving entries. It rejects non-VM classes and non-Backing refusal tags before variable refusal strings or the static-name interner; existing codecs parse the accepted fixed body. API checks still reject duplicate backends, invalid authority, holders and invalid metadata.

Three focused controls are defined for qualified cause round trips and canonical refusal bytes; hostile counts/tags/causes/duplicates and pre-interner rejection; legacy/explicit-empty bodies, truncation, trailing data and fatal invalid authority. Existing tests change only one owned report literal to add the empty field.

No builds or tests were executed, and no native/GPU/VM dispatch occurred. Scoped formatting and source-integrity inspection are the only local checks. ROOT owns matched compilation, execution and intake. The prior snapshot3 actual passes remain attributed to ROOT50758; they were not repeated. Settle original2212 remains INCOMPLETE, with its separate fixture-only successor5de88 unexecuted.

These diagnostics do not replace actual provider data, grant holders, refresh retained offers, report zero usage, or prove native readiness/physical absence. The ordinary Writer's inherited allocation gaps remain separate from the checked snapshot helper strategy.
