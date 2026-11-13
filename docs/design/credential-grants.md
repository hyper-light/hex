# Credential grants for Hex's Shards image VMs (joint plan step 33, its step 6)

Each harness VM Hex launches has its provider credentials filled in at its egress, by Shards' per-VM network process,
toward the hosts its grants name and no other, with no credential value anywhere in the VM. Hex's part is to populate
the launch spec's credential grants, keep every one in its record of the VM, add none an image's box didn't carry, and
answer the run's lease asks from its orchestrator's broker, giving each lease back as the run lets go of it. Decided
for kiln plan `01a1d2b88be57304b972761e2a5c6781` (step 33's plan), step 6, built on Shards at `740fa372` (vendored by
goal 12's re-vendor; at or after `b60dab848`) and carried to `db4168345` (D242's step 3: each grant's placeholder), whose
D242 is the contract. Requirements served: `AC-T7.7.2` (flows
allowed that no grant names: 0, here credentials filled in that no grant names: 0), and step 6's acceptance: "a
Hex-launched harness VM reaches exactly its granted providers with its credentials filled in at egress and nothing
else; every credential grant is in Hex's record of the VM; Hex adds no grant the image did not carry; Hex answers lease
asks from the broker and releases at run end; default deny holds".

Sources, read at the revisions named:
- Shards `740fa372`: `docs/design/architecture.md` D242 (credentials in Veil's shape: the contract, step 2's CA and
  trust, step 4's leases, step 5's broker in kiln), D236 (destinations by name), D238 and D252 (a run's own key), D246
  (each member held apart); `crates/api/src/image.rs` (`ImageSpec::credentials`, `ImageSpec::lessor`, `Lessor`,
  `Leased`, `RunLessor`; a spec with grants and no lessor refused), `image/wire.rs` (`CredentialGrant`, `Fill`, `decide`,
  `LeaseRefusal`, `Secret`), `image/session.rs` (`lease_asked`, `settle`, `release`: every lease given is given back
  once, as the host says `RELEASED` or as its answers end, and one given after the release or with none of its life left
  at once), `crates/shards/src/api_run.rs` (`credentials`: each grant's hosts held to the run's egress grants).
- Shards `db4168345`: D242's "Built: step 3's first slice, each grant's placeholder" (`crates/dns/src/credential.rs`:
  `CredentialGrant::placeholder`, never printed, its `Debug` showing its length alone; `placeholder`, `check`,
  `Refused::NotAToken`, `Refused::Shared`; `decide` taking the request's token), `crates/api/src/image.rs` (`request`
  checking them, `ImageRefusal::Spec`).
- RFC 6750 §2.1 (a bearer credential's `b64token`: `1*( ALPHA / DIGIT / "-" / "." / "_" / "~" / "+" / "/" ) *"="`).
- kiln `978e8a1a0`: `crates/kiln/src/secrets/broker.rs` (the run-lease broker: `lease(key, reference)`, `release(key)`,
  `moved`), `crates/kiln-seams/src/secrets.rs` (`check_name`, `NAME_MAX`), `crates/kiln-seams/src/sandbox.rs`.
- AWS Secrets Manager API Reference, `CreateSecret`, `Name` (1 to 512 characters of ASCII letters, digits and
  `/_+=.@-`), the store model kiln-seams' `Secrets` keeps.
- RFC 6066 §3 (TLS names a server by its DNS name alone; literal addresses are not permitted in `HostName`).
- Hex's own: `docs/design/network-grants.md` (N1 to N7: the grants a credential grant leans on), `shards-image-class.md`.

## C1. The grant: who, which credential, toward which hosts, which way

A box's credential grants are a list of `hex_api::CredentialGrant` in its image class's spec
(`ShardsImage::credentials`, after its shares):

| Field | Is |
|---|---|
| `member` | who: a member of the box's image, as a network grant names one (N1) |
| `reference` | which credential: a secret's name in its orchestrator's store, never its value |
| `toward` | the hosts the value may be filled in toward, each a DNS name its member's egress reaches |
| `placeholder` | what stands for the credential in its member's requests: minted by the orchestrator where it gives the credential to the member (kiln's broker: `kiln-placeholder-` and 256 random bits in hex), a token the member sends where the value would go; never printed |

Its way is out, as an egress grant's: a value is filled into what the member sends to those hosts where its placeholder
stands, never into what comes back (Shards' injector is a gateway that fills a request header, D242), and never into a
request that carries no placeholder of a grant toward its host. Holding the placeholder is the capability, so it never
prints: `CredentialGrant`'s `Debug` shows its length alone, as Shards' grant's does, and no refusal names it. None by
default: nothing is filled in toward any host, the run asks no lease, and its boot names no lessor.

## C2. One check, wherever a credential grant enters

`hex_api::check_credentials(credentials, grants)` is the one check, beside `check_grants` (N5). It runs in the class
spec's check (`ShardsImage::check`, which the caller and every create run), on the node's wire both ways (a request's
spec and an inspect's answer), and in the lowering the class makes for Shards' boot (C4). Each refusal is `Invalid`
naming the field (CLAUDE.md §3). It refuses:

- **A member's name** no Agentfile takes (N1's rule).
- **A reference no store could hold**: 1 to 512 of ASCII letters, digits and `/_+=.@-` (`REFERENCE_MAX`, C5). Its
  broker would refuse it at the run's first lease; refused at create instead.
- **No host, or a host named twice.**
- **A host its member's own egress grant doesn't reach by name, by TCP.** A credential grant grants no egress (D242):
  each host is one an `Egress` grant of the same member names, `Destination::Name`, over TCP. By name, as TLS names a
  server by its DNS name alone (RFC 6066 §3), so a grant toward an address could never be filled in; by TCP, as TLS rides
  it. Shards' run host holds the same (`api_run::credentials`, `shards_net::reaches`); Hex keeps no port egress, so
  its hold is by name alone.
- **A member's credential granted twice.**
- **A second credential toward one host.** Shards' injector at `740fa372` fills in the first credential a host's grants
  name (D242's `decide`), so a second one toward it would never be filled in: a grant that can never act is refused,
  never kept silently (D242's own rule for a grant toward an address). Shards' main since (step 33's step 3, the
  injector) decides by each grant's placeholder too, so the re-vendor past it drops this rule (Owed).
- **A host a member reaches ungranted.** Shards' network process holds the whole VM to the union of its members'
  grants and can't tell one member's flow from another's, so its injector ends the TLS of every flow to a credential's
  host, whichever member's (D242, "Who"), and every member whose grants reach it trusts the run's CA. A member that
  reaches the host by its own egress grant (by name, by TCP) without the credential would have its flows ended there
  with no grant of Hex's record saying so. So each such member must be granted the credential toward that host too, or
  not reach it. This is N4's ruling carried to credentials: refuse rather than leave a window, and record exactly what
  the run does.

Hex adds none: the lowering (C4) is the box's grants alone, and every host lowered is one a grant of the box names for
that credential (`w33_a_boxs_credentials_lower_to_one_grant_of_each_credential_toward_its_hosts`). The orchestrator
keeps or drops what its image's harness needs; Hex neither adds nor widens (D242, f5).

## C3. Leases: the orchestrator's lessors, given to the node

Shards' run host asks a credential's value just in time, by reference, through its orchestrator's `Lessor` for the
run (`ImageSpec::lessor`), and gives each lease back once, at the run's end or as soon as it is let go of. Hex is that
orchestrator for its image VMs, and answers from its own orchestrator's broker (kiln's). Hex's seam for it is
`hex_api::lease` (CLAUDE.md §9: kiln reaches Hex through its own -api crate, never through Shards'):

- `Lessors::lessor(id)`: what the node's orchestrator gives it (`GuestRuntime::with_lessors`), asked for one `Lessor` as
  each run of a VM with credential grants boots, for that run alone. Its leases are issued to the run's own key where
  it binds an agent-control endpoint (D252, told with each ask), else to its channel, which the orchestrator names for
  the run (kiln: 256 random bits in the form its broker keeps a key's SHA-256 in). Hex's image VMs bind no endpoint yet
  (`control: None`), so every ask comes with no key.
- `Lessor::lease(reference, key)`: the value, the lessor's name for the lease, and how long it lives from now (none:
  until the run ends); or a refusal of D242's kinds (`Denied`, `KeyReplaced`, `Unavailable`, `Ended`, `Lapsed`), carried
  to Shards kind for kind with its words. `Lessor::release(lease)`: that lease given back, once for each lease given.
- Hex's image class adapts it to Shards' `Lessor` (`hex_runtime::guest::leases::RunLeases`): the value is moved into
  Shards' `Secret` whole (Hex's `lease::Value` is wiped as it drops and never printed, so no copy is left behind); a
  life of none becomes the most Shards' run channel carries, a u64 of nanoseconds, which its run host and network
  process read as living until the run ends; and each lease Shards gives back is given back to the run's lessor. That is
  how the run's leases are released at its end: Shards' session gives back every lease it was given, once, by whatever
  end (its command's exit, a stop, its host's failure, its channel's end), and Hex passes each on.
- A run granting no credential has no lessor and asks nothing (default deny). A box granting one on a node given no
  lessors is refused as it is created, before any record: `Refusal::Unconfigured`, the node's configuration, not its
  platform or its revision. The same node given lessors serves it, so `Platform` (which §12 reads as a platform gap) and
  `Awaits` (a product's revision) would each misname it. Appended to `Refusal` and the wire (tag 15).
- `hex_api::lease::sim::SimLessors` is the simulator; `hex_api::lease::conformance` (HEX-LEASE-1 to 4) the contract it
  and every orchestrator's lessors pass.

Alternatives considered:
1. **Hex calls a run-level release of its own as the run ends** (its stop's thread, a refused boot), kiln's broker's
   `release(key)`. Rejected: Shards' session already gives back every lease of the run at every end, once; a second
   path in Hex's lifecycle would duplicate it (R12) and change the image class's stop paths, which goal 12 owns. The
   orchestrator keeps its own run-level end where it sees the run end (kiln's `RunLessor::ended`).
2. **Shards' `Lessor` as the seam kiln implements.** Rejected: kiln reaches Hex through `hex-api` alone (§9), and
   `hex-api` names no Shards type.
3. **Run-wide credential grants with no member**, as Shards' grant is. Rejected: a grant names who (N1, step 15's
   acceptance), and C2's last rule needs it to record whose flows the injector ends.
4. **A lessor given per call** (`Runtime::start` taking one). Rejected: the node's wire can't carry a trait object, and
   every runtime class would grow a call only one uses; the node's orchestrator gives its lessors once, as it builds
   the node in process (kiln's runner).

## C4. Hex's record of the VM

The credential grants live in the image class's spec, `ShardsImage::credentials`, in the box's durable record
(`hex_core::record::BoxRecord`, JSON, its fields by name; a grant a record kept before placeholders reads with none,
refused as the box boots), and `inspect` reports them as `BoxInfo::credentials` (after `ended`), every one as
the record holds it, its placeholder among it: the record keeps it because each boot of the box gives it to Shards, and
inspect answers the node's callers, who gave it. The node's wire carries both (a create's spec after its shares, an
inspect's answer after its run's words), each grant its member, reference, hosts and placeholder, each checked written
and read (C2); the wire's `IDENTITY` takes `credentials.rs` and moves. Shards' boot is given the box's grants lowered to
its run-wide form (`leases::lowered`): one `CredentialGrant` of each credential and placeholder toward every host its
members' grants of it with that placeholder name, each once and sorted, Hex adding none. Members given one credential
under placeholders of their own lower to a grant of it for each, so a member's placeholder is filled in toward its own
hosts alone (D242's `decide`).

## C5. Bounds and literals

| Literal | Value | Source |
|---|---|---|
| a reference's characters | 1 to 512 of A-Z a-z 0-9 / _ + = . @ - | Secrets Manager's `CreateSecret` `Name`, as kiln-seams' `Secrets` keeps it (`NAME_MAX`, `check_name`) |
| a lease's life with no end of its own | `u64::MAX` nanoseconds | Shards' run channel: a lease's life is a u64 of nanoseconds, its most read as the run's life (D242's literals) |
| the simulator's leases held at once | 10,000 | the simulator's own bound, as `SIM_BOXES` is the runtime simulator's |

There is no count constant: a box's credential grants cross the node's wire in one message (`MAX_MESSAGE`) and live in
its record (`MAX_RECORD_BYTES`). The check's two working lists are reserved fallibly (refused `Exhausted` past what
memory gives), sorted, O(n log n) in the hosts named, with no recursion (§11). A lease's value is held by the
orchestrator's lessor, Hex's adapter and Shards' run host only as long as each passes it on, wiped as each lets go of
it, and never printed.

## C6. What would falsify it

- A flow to a host no credential grant of the run names that has a credential filled in (then the lowering or Shards'
  decision widens, and C2's exactness fails); ROOT's real runs hold it.
- A lease Shards' session was given and never gave back at the run's end (then Hex needs a release path of its own,
  C3's alternative 1).
- An image harness that needs one host filled in with two credentials (then C2's one-credential-per-host rule waits on
  Shards' injector deciding by placeholder rather than by host).

## Tests

- `crates/hex-api/src/credentials/tests.rs`: a harness's credential toward its own egress hosts taken; each malformed
  field refused by name; a host its member's egress doesn't reach by name, by TCP (none, UDP, an address, another
  member's, another case) refused; a credential granted twice, and two toward one host, refused; a member reaching a
  credential's host ungranted refused, then taken granted, and one reaching it by UDP alone no such member; the record's
  JSON round trip.
- `crates/hex-api/src/lease/tests.rs`: the simulator passes HEX-LEASE-1 to 4, leaving no lease held; lessors that give
  nothing back and refuse a missing credential as unavailable fail exactly HEX-LEASE-2 and 3; a value prints its length
  alone and is handed on whole.
- `crates/hex-api/src/conformance.rs` HEX-IMG-8, which the simulator passes.
- `crates/hex-core/src/wire/tests.rs`: a Shards image VM's credential grants cross the wire after its shares, refused
  written and read where the check refuses them; an inspect's credential grants are its last bytes; `Unconfigured`
  round-trips with every refusal (and in the property test's).
- `crates/hex-runtime/src/guest/leases/tests.rs`: a box's credentials lowered to one grant of each credential toward its
  hosts, each one the box names, none from none; a credential the check refuses never lowered; the run's leases answer
  Shards' run host from its own lessor (its key passed on, the value whole, a life of none living past the channel's
  most, each refusal kind for kind, each give-back passed on).
- `crates/hex-runtime/src/guest/image_tests.rs`, over the model of D231 (its run host now asking the run's lessor and
  giving back every lease at the run's end, as Shards' session does):
  `w33_a_harness_vms_credentials_reach_its_boot_and_record_and_lease_from_its_lessors` (the boot's credentials and
  lessor, inspect and the record showing every credential grant, Shards' decision filling the granted host and passing
  an ungranted one bare, the ask answered from the simulated broker and held there, an unheld credential `Denied`, and
  the lease given back as the VM stops) and
  `w33_a_credential_no_one_here_can_lease_or_beyond_its_egress_is_refused_before_any_record`.

## Owed

- **The re-vendor past Shards `740fa372`**: step 3's injector moved the grant to `shards_dns::credential` and gave it
  its member's `placeholder`, filling a value only where that placeholder stands (D242). `CredentialGrant` then gains
  its placeholder (appended, never printed; kiln mints it where it stages the credential into its member), lowered
  with the grant, and C2's one-credential-per-host rule goes (Shards refuses a placeholder two grants share).
- **kiln's wiring** (kiln, once it vendors this Hex): `hex_api::lease::Lessors` over its run-lease broker (kiln's
  `secrets::lessor`, built on kiln main beside its broker's per-lease give-back), given to the node it runs its
  harness microVMs on; and its sandbox's credential grants (`kiln_seams::sandbox::Launch::credentials`, SANDBOX-13)
  carried to `ShardsImage::credentials` for the launch's member.
- **A remote orchestrator**: a node reached over its wire (`hex node`) is given no lessors and refuses credential grants
  `Unconfigured`; a lessor carried over the node's wire is a later change.
- **Shards**: the injector (step 33's step 3) that asks the first lease; until it lands, nothing in a run asks.
- **ROOT's real runs**: a harness VM on KVM, HVF and WHP whose granted provider gets its credential filled in at egress
  and an ungranted host gets none, its lease given back at its end, no value in the VM (step 33's step 10).
