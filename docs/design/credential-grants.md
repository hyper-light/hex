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
- **A placeholder no request could carry.** Its member sends it where the credential goes, as a bearer token is sent,
  and Shards' injector fills the value in only where it finds it there (D242's step 3): so it is RFC 6750 §2.1's
  `b64token` (`1*( ALPHA / DIGIT / "-" / "." / "_" / "~" / "+" / "/" ) *"="`), which an `x-api-key` field's value
  holds too, as `shards_dns::credential::placeholder` takes it. Shards' run host would refuse the run's request at its
  boot; refused at create instead. The refusal names the field, never the placeholder: holding it is the capability.
- **One placeholder for two credentials.** The injector could not tell which credential a request carrying it asks,
  so it could stand for neither alone (`shards_dns::credential::check`'s `Shared`). Grants of one credential may share
  one, as each member's stands for the same value; kiln's broker mints one for each credential it stages.
- **A member's credential granted twice.**
- **A host a member reaches ungranted.** Shards' network process holds the whole VM to the union of its members'
  grants and can't tell one member's flow from another's, so its injector ends the TLS of every flow to a credential's
  host, whichever member's (D242, "Who"), and every member whose grants reach it trusts the run's CA. A member that
  reaches the host by its own egress grant (by name, by TCP) without a credential would have its flows ended there
  with no grant of Hex's record saying so. So each such member must be granted a credential toward that host too, or
  not reach it. This is N4's ruling carried to credentials: refuse rather than leave a window, and record exactly what
  the run does.

Two credentials toward one host are taken, one member's or two members': Shards' injector at `db4168345` fills each in
only where its own grant's placeholder stands (D242's step 3, `decide` taking the request's token), so neither stands in
the other's way. At `740fa372` it filled in the first credential a host's grants named, and C2 refused a second toward
one host as a grant that could never act; the re-vendor to `db4168345` dropped that rule.

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
| a placeholder's characters | 1 or more of A-Z a-z 0-9 - . _ ~ + /, then any = | RFC 6750 §2.1's `b64token`, as Shards' `shards_dns::credential::placeholder` takes it |

There is no count constant: a box's credential grants cross the node's wire in one message (`MAX_MESSAGE`) and live in
its record (`MAX_RECORD_BYTES`). Nor is there a field's length of Hex's own: since Shards `17fc36ecd` (at `097ec0047`)
its boot refuses a credential or sign-in grant's field past what its network process's wire carries behind a two-byte
length (`shards_dns::credential::WIRE_MOST`, 65,535 bytes or hosts), by field and never quoting it, as a spec's
(`ImageRefusal::Spec`), which the image class says as `Invalid` naming `shards image` as it says every spec refusal. The roots a run's
injector trusts toward a credential's host are its host's (Shards' D242 upstream roots, `37ddeb080`): Hex names none
of its own (`ImageSpec::upstream_roots` empty), and since then no `SSL_CERT_FILE` or `SSL_CERT_DIR` adds one. The check's three working lists (each grant's hosts, its member and credential, its
placeholder and credential) are reserved fallibly (refused `Exhausted` past what memory gives), sorted, O(n log n) in
the hosts named, with no recursion (§11). A lease's value is held by the orchestrator's lessor, Hex's adapter and
Shards' run host only as long as each passes it on, wiped as each lets go of it, and never printed.

## C6. What would falsify it

- A flow to a host no credential grant of the run names that has a credential filled in (then the lowering or Shards'
  decision widens, and C2's exactness fails); ROOT's real runs hold it.
- A lease Shards' session was given and never gave back at the run's end (then Hex needs a release path of its own,
  C3's alternative 1).
- A placeholder Hex takes that Shards' run host refuses, or the other way (then C2's placeholder rules and
  `shards_dns::credential::check` part, and one follows the other); a boot refused `Invalid` naming `shards image` for
  its credential grants would show it.

## C7. Leases over the node's wire: from the connection that created the box

Joint plan step 33 (kiln #74), Hex's lead's ruling of 2026-11-13. A node reached over its wire (`hex node serve`, on its
local socket or over QUIC) leases a box's credentials from the orchestrator that created the box, over the connection it
created the box on. Hex's seam is still `hex_api::lease` (C3): the node's `Lessors` is the socket's
(`hex_node::serve::WireLessors`, which `hex node serve` builds its guest runtime's image class with), and the client's
are its caller's (`hex_client::Core::with_lessors`).

- **Saying so.** A connection that leases sends `Call::Lessors` (kind `0x0019`), answered `Done`. A node whose runtime
  leases through no connection's lessors refuses it `Unconfigured`. `with_lessors(lessors)` sends it and gives the
  connection its caller's lessors.
- **Default deny.** A create granting a credential on a connection that sent no `Lessors` is refused `Unconfigured`
  before the runtime is called, so before any record: a node never leases for a box whose creator gave no lessors.
  Each box's route is its latest create's, kept in memory before that create is answered. It goes to the creating
  connection where the box grants a credential and the connection leases, else nowhere. A delete over the wire, or the
  connection's end, takes it away. Nothing else of a box is kept for its leases, and nothing of a lease is written.
- **The asks.** As a run of the box boots, the node asks the connection for the run's lessor: `Answer::LessorAsk(id)`
  (`0x8011`), its header's id the run's from then on. The client makes the lessor through its `Lessors::lessor(id)` and
  answers `Request::Lessor` (`0x001a`). Each `Lessor::lease` is `Answer::LeaseAsk { run, reference, key }` (`0x8012`),
  answered `Request::Leased { run, answer }` (`0x001b`): the lease, or a refusal of Shards' kinds in its words. Each
  give-back is `Answer::LeaseGiven(lease)` (`0x8013`), and the run's end, as the node lets its lessor go,
  `Answer::LessorEnded` (`0x8014`), neither answered. Each kind is appended after every one before it, and the wire's
  `IDENTITY` moves.
- **The value.** It crosses in its `Leased` message alone (`wire::carries_secret`), read straight into a `lease::Value`,
  wiped as it drops and never printed. The node wipes its read buffer once that message is read. The client wipes its
  unframed copy as it frames it, and each word it owes the node once it has gone or its connection has ended. The
  wire's writer wipes what it held of a message that fails, and reserves the message's room before the value goes in,
  so no copy is left as it grows. The buffers of a transport below the wire (the kernel's, and TLS's in hyper-quic) are
  not Hex's to wipe.
- **Waits.** Every ask waits for its answer while its connection stands (decision 9): the answer, or the connection's
  end, ends it, however long that takes, and no ask is bounded by a time of its own. From `with_lessors` on, the client
  reads its connection with a thread of its own (the event form's loop), so the node's asks are read whatever its calls
  are doing, and it answers each on a thread of its own, since a broker may take its time.
- **Bounds.** A connection's lease asks waiting are held to its in-flight limit (`Limits::in_flight`), refused past it,
  typed. The routes are held to `MAX_BOXES` boxes, refused past it at create. The words the client may owe the node grow
  by the same in-flight limit, one answer for each ask that may wait.
- **Ends.** When the connection ends, from either side, the node fills every ask waiting on it with the end: a lessor's
  ask is refused `Unconfigured`, and a lease's `Unavailable`, naming the end. It drops the connection's routes and gives
  later give-backs to no one. The client gives back every lease it handed over and wasn't given back, and lets every
  run's lessor go. A run that holds a value past then holds it until its lease's life or its run ends, as a run of an
  in-process orchestrator that crashed would.
- **Not built: composing a node's in-process lessors with its connections'.** A node built in process with its
  orchestrator's lessors (kiln's runner) leases its in-process boxes through them, and its runtime doesn't lease through
  the socket's `WireLessors`. So a wire create granting a credential there is default-denied, since its connection
  can't send `Lessors` to that node.

Alternatives considered:
1. **A lease connection of its own** (a second connection per orchestrator, or a QUIC port of its own). Rejected: the
   box's creator is its connection. A second one would have to prove it speaks for the first, another identity to
   check, and the two could end apart.
2. **A lessor given in the create's call.** Rejected, as C3's alternative 4: the wire can't carry a trait object, and a
   run asks for its lessor as it boots, so the ask goes back to whoever holds the lessors.
3. **The node asking its orchestrator's broker itself** (kiln's server, over kiln's API). Rejected: kiln reaches Hex
   through `hex-api` alone (CLAUDE.md §9), and the broker leases to the connection that proves the run's orchestrator,
   which the node can't.

What would falsify it:
- A lease handed over a connection that neither its run nor the connection's end gave back, so its broker holds it after
  both (`s33_a_connection_that_drops_gives_back_its_leases`).
- A leased value in a node's files, records or answers (`s33_a_leased_value_is_kept_nowhere_on_the_node`).
- A run of a box created on a connection that gave no lessors given any lease.

## C8. Sign-ins: a CLI's own sign-in passed through to its orchestrator

Joint plan step 33 (kiln #74 item 2), on Shards `3114e082f` (image channel v24, D242's step 8). A harness's CLI (Claude
Code, Codex) may sign in as itself in its VM. Shards' injector takes the CLI's token response outside the microVM, hands
its tokens to the run's orchestrator, and gives the CLI placeholders in their places; refreshes and revocations are
answered outside too. Hex passes each through, adding none:

- **The grant.** `ShardsImage::sign_ins` (after the credentials, append-only): each a `hex_api::SignInGrant` of its
  member, the sign-in's name in its orchestrator's store, its CLI (`SignInCli`), the hosts its access token is filled in
  toward, and an earlier capture's placeholders (`Captured`), so a later run's CLI holds the same ones. None by default.
- **One check** (`hex_api::check_sign_ins`, beside `check_credentials`, wherever a spec enters: the class spec's check,
  the node's wire both ways, the lowering): each field refused by name. A name or a captured reference no store could
  hold; a sign-in named twice, or a second sign-in of one CLI (Shards: one sign-in of a CLI a run); no host, a host
  twice, or one its member's egress doesn't reach by name, by TCP (C2's rule); a captured placeholder that isn't a
  b64token; and a placeholder standing for two tokens, its own two, two sign-ins', or a sign-in's and a credential's.
  A placeholder is a capability: `Captured`'s `Debug` says its lengths alone.
- **The lessor.** `hex_api::lease::Lessor` gains `capture(sign_in, tokens, key)` (the placeholders and the access
  token's reference), `refresh(sign_in, key)` (the token response the CLI is to have) and `revoked(sign_in, key)` (told).
  Each defaults to `SignInRefused::Unanswered`, which Shards' guest sees as a 503, as Shards' own defaults do. Tokens
  are `SignInTokens` of `lease::Value`s, wiped as they drop and never printed.
- **The image class** lowers each grant run-wide into its boot's `ImageSpec::sign_ins`, and a run with any sign-in has a
  lessor, as one with a credential does (C3): a node given no lessors refuses its create `Unconfigured`. Its `RunLeases`
  forwards each ask, its tokens moved whole into `Value`s, each refusal kind for kind.
- **Over the node's wire** (C7's connection): `Answer::CaptureAsk { run, sign_in, tokens, key }` (`0x8015`),
  `RefreshAsk` (`0x8016`) and `SignInRevoked` (`0x8017`, not answered), answered `Request::Captured` (`0x001c`) and
  `Request::Refreshed` (`0x001d`); appended, the wire's `IDENTITY` moving. A capture's ask, and the captured and
  refreshed answers, carry secrets (`wire::carries_secret`): wiped where Hex held them, on both sides, as a lease is.
  A create granting a sign-in on a connection that sent no `Lessors` is refused `Unconfigured` (default deny).

Not built: an inspect's report of a box's sign-ins (its record holds them); kiln's broker answering captures and
refreshes (kiln's).

## Tests

- `crates/hex-api/src/credentials/tests.rs`: a harness's credential toward its own egress hosts taken; each malformed
  field refused by name; a host its member's egress doesn't reach by name, by TCP (none, UDP, an address, another
  member's, another case) refused; a credential granted twice refused; two credentials toward one host, one member's or
  two members', each by its own placeholder, taken; a placeholder no request could carry (none, a space, a colon, an
  `=` before its end, a byte past ASCII) refused naming its field, never its bytes, and kiln's and every `b64token`
  character taken; one placeholder for two credentials refused, one member's or two members', and two members' grants
  of one credential sharing one taken; a member reaching a credential's host ungranted refused, then taken granted, and
  one reaching it by UDP alone no such member; the record's JSON round trip, a record kept before placeholders read with
  none; a grant's `Debug` showing its placeholder's length alone.
- `crates/hex-api/src/lease/tests.rs`: the simulator passes HEX-LEASE-1 to 4, leaving no lease held; lessors that give
  nothing back and refuse a missing credential as unavailable fail exactly HEX-LEASE-2 and 3; a value prints its length
  alone and is handed on whole.
- `crates/hex-api/src/conformance.rs` HEX-IMG-8, which the simulator passes: two credentials toward one host, each by
  its own placeholder, made and inspected; a placeholder with a space, and another credential's placeholder, refused.
- `crates/hex-core/src/wire/tests.rs`: a Shards image VM's credential grants cross the wire after its shares, refused
  written and read where the check refuses them; an inspect's credential grants are its last bytes; `Unconfigured`
  round-trips with every refusal (and in the property test's).
- `crates/hex-runtime/src/guest/leases/tests.rs`: a box's credentials lowered to one grant of each credential and
  placeholder toward its hosts, each one the box names, none from none, and a member's own placeholder for a credential
  lowered to a grant of its own toward its own hosts; a credential the check refuses never lowered; the run's leases
  answer Shards' run host from its own lessor (its key passed on, the value whole, a life of none living past the
  channel's most, each refusal kind for kind, each give-back passed on).
- `crates/hex-runtime/src/guest/image_tests.rs`, over the model of D231 (its run host now asking the run's lessor and
  giving back every lease at the run's end, as Shards' session does):
  `w33_a_harness_vms_credentials_reach_its_boot_and_record_and_lease_from_its_lessors` (the boot's credentials and
  lessor, inspect and the record showing every credential grant, Shards' decision filling the granted host where its
  placeholder stands and passing an ungranted one, or a request without it, bare, the ask answered from the simulated
  broker and held there, an unheld credential `Denied`, and the lease given back as the VM stops) and
  `w33_a_credential_no_one_here_can_lease_or_beyond_its_egress_is_refused_before_any_record`.
- C7, `crates/hex-core/src/wire/tests.rs`: `Lessors`, a lessor's and a lease's answers of every kind, and the node's four
  asks and words read back as written, through every truncation and bit flip of the hostile tests;
  `s33_a_leases_value_crosses_marked_and_never_printed` (only a leased message says it carries a value, and neither
  side prints it); `s33_a_lease_answer_of_no_defined_kind_is_refused`.
- C7, `crates/hex-node/src/serve/leases/tests.rs`: a routed box's run leases over its creator's line and gives back there,
  each told under the run's id; a box no leasing connection created leases from no one, as does one whose route a later
  create or its delete took away; a lease asked of a line that ends is refused by its end.
- C7, `crates/hex-node/src/serve/unix_tests.rs` (Linux's socket and macOS's framed stream, a live server reached through
  `hex-client`, red at `8508ed34`):
  `s33_a_remote_harnesss_lease_is_answered_through_the_wire_and_given_back_at_its_end`,
  `s33_a_connection_that_drops_gives_back_its_leases`,
  `s33_a_credential_granting_create_without_lessors_is_refused_before_any_record`, and
  `s33_a_leased_value_is_kept_nowhere_on_the_node` (every file under a recorded node's state and run folders, and every
  answer about the box, read after a run to its stop and delete: the value nowhere; the census holds wait names and
  times alone, by its type).

- C8, `crates/hex-api/src/credentials/tests.rs`: `s33_a_sign_in_is_checked_by_its_fields_beside_the_runs_credentials`.
  `crates/hex-runtime/src/guest/leases/tests.rs`: `s33_a_runs_sign_ins_answer_shards_run_host_from_its_own_lessor`
  (red at `9e3617b6`). `crates/hex-runtime/src/guest/image_tests.rs`:
  `s33_a_harness_vms_sign_in_reaches_its_boot_beside_its_lessor` (red at `9e3617b6`). `crates/hex-core/src/wire/tests.rs`:
  the capture's and refresh's asks and answers read back as written, and only those carrying secrets say so.
  `crates/hex-node/src/serve/unix_tests.rs`:
  `s33_a_remote_harnesss_sign_in_is_captured_refreshed_and_revoked_through_the_wire`, and the default-deny test refusing
  a sign-in too.

## Owed

- **kiln's wiring** (kiln, once it vendors this Hex): `hex_api::lease::Lessors` over its run-lease broker (kiln's
  `secrets::lessor`, built on kiln main beside its broker's per-lease give-back), given to the node it runs its
  harness microVMs on; and its sandbox's credential grants (`kiln_seams::sandbox::Launch::credentials`, SANDBOX-13)
  carried to `ShardsImage::credentials` for the launch's member, each with the placeholder its broker minted where it
  staged the credential into that member.
- **Leases over the wire (C7), beyond the light tests**: the same over QUIC and natively on Windows, and a remote
  harness VM's real lease on KVM, HVF and WHP (ROOT's); kiln's runner calling `with_lessors` on the connections it
  creates harness VMs over; and composing a node's in-process lessors with its connections' (not built).
- **Shards**: the injector (step 33's step 3) that asks the first lease; until it lands, nothing in a run asks.
- **ROOT's real runs**: a harness VM on KVM, HVF and WHP whose granted provider gets its credential filled in at egress
  and an ungranted host gets none, its lease given back at its end, no value in the VM (step 33's step 10).
