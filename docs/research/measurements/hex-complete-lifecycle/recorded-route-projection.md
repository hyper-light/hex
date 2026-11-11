# Common Recorded route encoding barrier

SOURCE ONLY: this joins the real L1/L2 family producer interfaces. No compiler,
tests, service query or native job was run by L3. It does not complete native VM,
resource or cold-scope proof.

The existing Recorded Books `prepare_pool` wraps its configured pure validator.
After that validator succeeds, but before SAME Store publication, it checks one
global route encoding union. `introduced` is Some only for expectedCurrent=None;
the Store has already strictly proved that target absent. Same-generation reuse
and changed-R draining/ACK handling do not invent a new lifetime. The existing
8b exclusive metadata snapshot/publication fence and full-family Turns remain the
only authority. No native I/O or recursive Books/Store call occurs in this wrapper.

Recover has a component `project_routes(pools, introduced, &mut RouteProjection)`
and top-level `check_route_projection(pools, introduced)`. The top-level bounds and
checks rows, allocates the existing fallible transient L1 collector, appends all
components and obtains its one final count. The collector itself refuses before
growth past MAX_RUNTIME_ROUTES. A generic unimplemented component returns a typed
refusal; it cannot silently drop VM tuples. No route DTO, generation, row, picker,
inventory, epoch, holder cache or admission ledger is added.

WithGuest rejects retained/introduced executions outside its actual Shards
adapter. It appends its host's native component and L2's real configured guest
forms to the SAME collector. Guest projection retains the exact tuple including
BackingKey and actual aliases; introduced forms describe only the one absent
lifetime. The native-only Mac/Win component refuses VM owners passed directly
without the initialized WithGuest producer. No classes(), capacity(), Prober,
WMI, receipt read or OS call occurs under the family fence.

MacRuntime currently inherits Runtime::capacity=None. Its native component
therefore contributes no invented explicit Process prefix. This preserves current
routing and the independently supplied same-host budget behavior; it is not a
claim that an explicit native Mac capacity producer was implemented or validated.
WithGuest still contributes its actual initialized guest forms.

## Windows: conservative encoding bound, actual eligibility unchanged

The existing `windows/oci/sys.rs` API OnceLock stores only Api::load: DLL/export
pointers or the exact initialization failure. It does not store the separate
HcsGetServiceProperties(Basic) result. A loaded DLL must not be described as a
service answer. No new service-answer flag, cache, refresh period or native
provider is introduced.

The SAME API declaration is moved to module scope. A pure getter reads only
API.get(). WinRuntime's existing constructor warms that owner with Api::load
outside the metadata fence, without calling HcsGetServiceProperties. The existing
features_off OnceLock is also read only through get() in projection. Unknown API
or Containers initialization returns a typed refusal before collector mutation.
No error is converted to guessed free capacity or successful eligibility.

`project_native_encoding_upper_bound` explicitly reserves possible fixed forms:
Process, plus OCI when the DLL/export initialization succeeded and the initialized
Containers configuration enables it. This reservation includes OCI even if the
existing current service availability query would fail. It is an encoding upper
bound, not an advertised route, placement grant or VM boot permission. Reserving
that extra possible slot prevents a later service recovery from causing protocol
overflow after a new backing was published. It may conservatively refuse a new
backing earlier than today's currently advertised service forms would require.

The actual Runtime::capacity eligibility predicate still calls oci::available()
and features_off() exactly as before; oci::available() still makes its existing
HcsGetServiceProperties query. Their bodies are byte-identical to the parent
source. The actual offer constructor and projection use the SAME L1
native_route_keys function with the SAME native Windows platform; a false actual
initialized predicate still publishes no OCI offer. Encoding reservation never
widens current service or GPU eligibility.

The API OnceLock retains its existing process-lifetime initialization semantics:
no invalidation/reset or refresh is added, and a failed DLL initialization needs a
new process as before. Containers configuration retains its existing runtime-lifetime
OnceLock semantics; a failed initial read remains unknown and may be retried by its
existing outside-fence path. A changed initialized configuration requires runtime
reinitialization, not a new periodic refresh. Service state is deliberately not
cached, so its existing query can recover and offer the already-reserved form.

## Exact dependency and intake boundary

- L1 shared collector/native actual keys: `ec0a0ab6f2fe6266696979d8df2fdf6032178ff5`.
- L2 real configured guest projector: `b3c05c9e58c86170c0c41d5c2322f173238bd7cd`,
  preserved by the current family consumer lineage `3bdb5707b806304df0a367401a328a7c5d8926ef`.
- Required Books family boundary: `b924aec5b7dfb98e4ce6ce11a767c51c7b3a0d48`;
  common real Store fence/producer: `8b5534be5277e422c7cc6c949e6c3019fb8ecab0`.
- L1 Node equivalent projector/guard: `59363a0f5e8d669b2a0e7c86601f2d60c5dd0eaf`
  plus plain-if source correction `9b9d5932ecb84fa4b417fd265a1f1e532fa62a50`.

Apply this named commit delta to the matched ROOT cohort. Do not copy this branch's
unrelated ancestors wholesale. Preserve ROOT's seven route_for calls, canonical
authority fixtures, Box allocation/route projection and holder/normalized metadata
ports. The independent Prepared/ACK producer pins0597523/70b364 remain intact.
No full2ef Mac helper, new manifest dependency, SDK patch, CLI registration or
shared wire change is included. ROOT owns CLI/startup integration and all outside
mechanical fixture ports for custom Recover implementations.

## Focused controls and limitations

Two new Recorded controls are UNRUN. The first uses actual Recorded/Store
publication with a counted sans-I/O model producer. Its two native forms plus
valid canonical model Pool tuples fit independently but reach exactly the global
limit when combined with one absent lifetime. That publication succeeds in the
specified oracle; reusing the same lifetime must introduce no extra form; the
next new lifetime must refuse before any row/sync change. Callback counts and
panic-if-called native/capacity/classes methods challenge ordering. These are model
configuration identities for the common barrier, not actual configured VM family
proof or a passing result. Actual L1/L2 configured producers have separate controls.
The second refuses VM owners/introductions on a native-only component.

One new pure Windows unit is UNRUN. It challenges unknown initialization before
mutation, enabled possible forms, disabled/missing-DLL bounds, and a currently
unavailable service predicate through the actual native offer constructor: possible
encoding OCI remains reserved, but no fresh OCI offer/device route/backing appears.
This injects the native predicate, never calls HCS, and establishes no live service
or eligibility result. No prior passing unit or original failure control is changed.

ROOT must match and compile the actual constructors/callers, run these three units
and the appropriate changed producer controls, validate supported-target lints,
and later exercise native admission/cleanup/fault/resource behavior. Source-only
composition cannot establish any of those results.

## R01–R16 receipt

Policy7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506, rules SHA-256
df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3; all four binding
files byte-equal0342, FULL reread with Hex charter/brief/CLAUDE after resume. L3 is
implementer; no extra agents/goals or runner changes. R01/R04/R06/R07/R13: borrowed
components, bounded fallible collector, typed unknown/errors, no new unsafe/state
owner, fallible existing diagnostics. Api::load's existing initialization allocations
and other legacy constructors/Store encoders remain acknowledged gaps, not new
allocation/leak/performance claims. R02/R05/R12: actual source constructors and
existing initialization owners, no second path/cache/query under the fence.
R03: no production bound or cadence is invented; MAX is the existing wire limit.
R09/R10: required feature controls only, UNRUN; prior reproductions/raw unchanged.
R08/R14: no timing, allocation profile or competitor claim. R11: matching seven-target
build/lint and actual native effects remain ROOT-owned. R15/R16: signed named source,
scoped rustfmt/git diff checks only, no job/temp actor/foreign effect or main merge.
