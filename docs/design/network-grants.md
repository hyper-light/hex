# Network grants for Hex's microVMs (joint plan step 15; CP-T7.7)

Each harness VM Hex launches reaches what its grants name and nothing else. It publishes nothing they don't
name, and Hex's record of the VM shows every grant. This record states the grant model, who enforces it, what
Hex can program through Shards today, and what Shards' step 9 gave it (D236, on Shards' main at `edbada623`; Hex
vendors Shards at `2365d784`, which holds it). Decided by goal 15 with Hex's lead, 2026-11-12. The lead's ruling on enforcement (N3) follows the person's decision: "Hex owns TCP/UDP for Shards
VMs" (joint plan kiln `01a1cd8e862e7169a074db3d5fa33f3d`, "Decided by the person" 2).

Requirements served: `AC-T7.7.2` (flows allowed that no grant names: 0), `T-T7.7.1` (no DNS answer and no packet
for a workload without a DNS grant), and the joint plan's step 15 acceptance: "a Hex-launched harness reaches
exactly its granted providers and kiln's endpoint, publishes nothing ungranted, and every grant is visible in
Hex's record of the VM".

## N1. The grant: who, to whom, which ports, which way

Default deny is Shards' rule as well as this step's: "a network flow that no directive explicitly grants does
not exist ... Every grant names who, to whom, on which ports, which way" (Shards `CLAUDE.md` at `2365d784`).
A box's grants are a list of `hex_api::Grant` (`crates/hex-api/src/grants.rs`). The variant is the way:

| Grant | Who | To whom | Ports | Way |
|---|---|---|---|---|
| `Egress` | a member | a DNS name, or one address | one protocol's range | out; replies of those flows back |
| `Host` | a member | the VM's own host, on its loopback (kiln's endpoint when kiln runs beside the VM) | one protocol's range | out |
| `Publish` | whoever reaches the host address it names | the member | the member's port, and the host's port | in |

- **Who** is a member of the box's image, `{kind, name}`, as its Agentfile declares it (`AGENT` or `HARNESS`;
  Shards' `shards_api::image::MemberName`). A box running an OCI spec (Hex's own guest path, `process`, `oci`,
  `firecracker`) has no members, so it has nothing to grant. Grants ride in the spec of the class that boots
  Agentfile images (goal 12, plan step 12), never in `BoxSpec` beside every class.
- **To whom** is named exactly. A name is one fully qualified host name, with no pattern. An address is one
  unicast address. There is no "any destination" grant: Shards' port-only egress (`NETWORK --egress=443/tcp`)
  reaches every address on the port, which is the gap Shards' bring-up recorded (`harness-bringup-v1`, H2), so
  Hex never asks for it.
- **The host** is its own grant. Every egress grant excludes the VM's host, its loopback and link-local
  addresses, as Shards' `Policy::AllowAll` does (`crates/net/src/lib.rs`). A service on the host is reached only
  through a `Host` grant.
- **Ports** are TCP or UDP, the protocols a VM's network process carries (Shards D31), with port 0 refused
  (RFC 6335 §6).
- **Replies.** A flow a grant allows carries its replies. Nothing else comes back. This is Kubernetes'
  semantics: "Reply traffic for those allowed connections will also be implicitly allowed" (NetworkPolicy, v1.37).

What the check refuses, by field (`check_grants`):

- **Member names.** BuildKit's stage-name grammar `^[a-z][a-z0-9._-]*$`, as Shards' Agentfile parser takes it
  (`valid_stage_name`, `crates/agentfile/src/instructions.rs`). At most `NAME_MAX`, 255 bytes
  (`include/uapi/linux/limits.h`), because a member's name is one directory of its image (`/harness/<name>`).
- **DNS names.** RFC 1123 §2.1 host names: labels of letters, digits and hyphens, 1 to 63 octets, not starting
  or ending with a hyphen. At most 253 characters in all, the text form of RFC 1035 §2.3.4's 255 octets. The
  check also refuses:
  - upper case: RFC 4343 makes DNS compare names case-insensitively, so Hex keeps one spelling and a duplicate
    can't hide;
  - a trailing dot;
  - a pattern;
  - a single label, which the host's resolver completes from its search list (RFC 1535), so it names no one host;
  - an all-numeric last label (RFC 3696 §2), which would read as an address;
  - `localhost` and names under it, which resolve to loopback (RFC 6761 §6.3).
- **Addresses.** The special-purpose addresses that can't name a peer past the VM (RFC 6890): unspecified and
  `0.0.0.0/8`, loopback, link-local, multicast and limited broadcast. An IPv4-mapped IPv6 address (RFC 4291
  §2.5.5.2) is refused too, so an IPv4 address has one spelling.
- **Publish addresses.** A `Publish` host address must name a socket the host can bind for its own addresses.
  The unspecified address is refused because "every address" names no one, and so are multicast, broadcast,
  link-local and IPv4-mapped.
- **Flows named twice.** Two grants may not name one flow: one member, destination and protocol with
  overlapping ranges; one published `(protocol, port)` for two members, since Shards gives each incoming port one
  receiver (D122); or one host socket twice. So every allowed flow maps to exactly one grant, in the record and in
  the property test.

## N2. How a name grant is held without trusting the guest

The enforcer resolves the name itself and pins the grant to what it learned:

- it answers the guest's DNS question for a granted name from its own resolver;
- it refuses every other question at once, without forwarding it, so no packet leaves for it (`T-T7.7.1`);
- it admits a flow only to an address its own resolver answered for the granted name, on the granted ports.

The guest's resolver, `/etc/hosts` and the addresses it claims are never trusted. This is how Shards already
holds an agent's remote MCP grant and a network's D192 egress: "a flow passed only to an address its own
resolver answered for a destination's name, or the address a destination is, on that destination's port; and of
names, only the destinations' asked of the host's resolver ... every other question REFUSED" (D192; D59 part
seven). Cilium's `toFQDNs` holds names the same way: "Cilium intercepts DNS responses per-Endpoint using a DNS
Proxy", and the IPs those responses carry are allowed (`Documentation/security/policy/layer3.rst` at
`cb21a81b`).

What the enforcer must also do for a grant to stay exact. Shards' step 9 built it (D236: "a flow to its port
reaches only an address its resolver answered for the name through it, or the address it is"), and its real-VM
evidence is owed (D236, "Evidence owed"). Hex's conformance cases for the binding are these:

- **Expiry.** A learned address stays pinned for its answer's TTL (RFC 1035 §3.2.1, RFC 2181 §8). A flow
  already admitted outlives its address's expiry, as Cilium's do ("live connections are not expired until they
  terminate").
- **Bound.** The learned set is bounded by expiry, never by a chosen count. Cilium's per-name cap of 50 is an
  upstream default and no derivation (R03).
- **Rebinding.** A learned address that no `Egress` address grant could name is refused: loopback, link-local,
  the host's own. Otherwise a name resolving to `127.0.0.1` would reach the host without a `Host` grant (DNS
  rebinding).
- **Address families.** IPv4 and IPv6 are both learned. Shards' network process learns A records alone today
  (D192, "Next"), so until it learns AAAA a name grant reaches its IPv4 addresses only, and Hex records that.

Pinning holds addresses, not names. Several names share a CDN's addresses: the bring-up's `api.openai.com` was
answered from Cloudflare's edge (`cf-ray` in `harness-bringup-v1`). A flow to a pinned address can therefore
carry another name in its TLS SNI, or in its HTTP authority inside TLS: domain fronting (Fifield, Lan, Hynes,
Wegmann and Paxson, "Blocking-resistant communication through domain fronting", PoPETs 2015(2), doi
10.1515/popets-2015-0009). A grant's exactness has three levels, and the record states which one holds:

1. the addresses learned for the name (D236 holds this one);
2. the ClientHello's SNI equals the granted name. Encrypted Client Hello (RFC 9849, March 2026) hides the inner
   name, so an ECH hello whose outer name isn't granted is refused. D236 does not hold this level: it rejected
   names held by what a flow says, as a client chooses what it says;
3. the HTTP authority equals the granted name. Only an enforcer that terminates TLS sees it: step 33's injector,
   which "terminates TLS only for granted hosts".

Claude Code's own sandbox checks names at level 2 alone: its proxy "enforces the allowlist based on the
requested hostname and, by default, does not terminate or inspect TLS traffic"
(code.claude.com/docs/en/sandboxing).

## N3. Hex decides each VM's TCP/UDP policy; Shards' per-VM network process enforces it

"Hex owns TCP/UDP" means two things:

- Hex decides each VM's policy, from the grants of the spec it was given;
- Hex configures Shards' per-VM network process with that policy.

The network process stays the enforcer: each VM's own process, sharing no memory with another VM's (Shards
`crates/net/src/lib.rs`, D31). It is also where Veil's injector goes: "the injector sits in each VM's egress
(Shards' per-VM network process), configured by Hex, which owns TCP/UDP" (joint plan, credentials).

Alternatives considered:

1. **Hex configures Shards' network process (chosen).** One enforcer per VM, where D31 put it. One policy,
   with no second copy to keep consistent. The injector's place stays the same.
2. **A Hex enforcer of its own around Shards' network process.**
   - It would need a different mechanism on each OS:
     - Linux: a network namespace with nftables and a DNS proxy, as Firecracker's docs leave filtering to the
       host's `nft` (`docs/network-setup.md`);
     - macOS: PF anchors, which need root;
     - Windows: WFP filters, which need administrator rights.
   - It duplicates D31's enforcement (R12) and needs privileges Shards' rootless design avoids.
   - It moves the injector's place out of the network process.
   - Rejected by the lead, 2026-11-12.
3. **Hex's own network process in place of Shards'**, Firecracker style: a tap device and a user-space stack per
   VM. That is a second network stack per VM beside Shards' (R12, R13). Rejected.
4. **Port-only egress through today's interface** (`Narrowing.egress`). It admits every address on the port and
   any name (H2). Ungranted flows would exist. Rejected.
5. **An HTTP proxy in the guest**, Claude Code's model. It holds names only for clients that use it, so the
   network process must still deny the rest, and step 33's injector is the TLS-terminating form of the same
   idea. Not a separate layer.

What would falsify N3:

- a flow the network process admits that Hex's policy does not name, in the enforcer's conformance cases (N2);
- a grant Hex can't express through the network process's interface without widening it.

Either would make Hex refuse that grant typed until the interface is exact. Neither would make Hex build its own
enforcer.

## N4. What Hex programs today, through Shards `2365d784`'s image boot (D231)

Hex boots harness images through `shards_api::image::ImageShards::boot`. The run host, `shards api-run`, gives
the VM a NIC and its network process only when something passes the microVM: a published port, or narrowed
egress that isn't empty. Otherwise the run's network is `none`, so the VM has no NIC (`crates/shards/src/api_run.rs`,
`as_run`, `plan`, `launch`). The network process starts at `Policy::DenyAll` and receives as its policy the
image's grants as the orchestrator narrows them: "egress, remote MCP hosts and DNS the image grants, kept or
dropped by the orchestrator, never added" (D231). A widening is refused (`Cause::Widening`).

Hex expresses everything through two fields of `shards_api::image::ImageSpec`:

- `publish: Vec<Publish { guest_port, proto, host: Option<IpAddr>, host_port }>`;
- `egress: Narrowing { egress: Keep, mcp: Keep, destinations: Keep, dns: bool }`.

`Narrowing::default()` keeps the image's grants whole, so Hex never uses it.

What Shards' step 9 added (D236): a network's destinations, `NETWORK --egress=HOST:PORT` in the image's
Agentfile, TCP alone, to a host's name, an IPv4 address (no IPv6, as D192), or `host.docker.internal`, the host
itself, which reaches the host's IPv4 loopback at that port alone. They are the image's grants: an orchestrator
keeps or drops them (`Narrowing.destinations`), never adds one. A destination reaches the network's members the
image lets out on its port, and the network process holds the whole microVM to the union of its members' names
on a port, so an image whose members that union can't hold apart is refused at build and at run
(`agentfile::held`). And each port the run host binds names whom it reaches (`Bound::to`, N6).

| A box's grants | What 2365d784 can express | Hex today |
|---|---|---|
| none | `network none`: no NIC, no network process | programmed: `Narrowing { Only([]), Only([]), Only([]), false }`, no publish |
| `Publish` of a member's port at a host address, on a run of the members alone (`command: None`) | `Publish { guest_port, proto, host: Some(addr), host_port }`, only for a port the image gives a member for ingress (D122); `Bound::to` names its receivers | programmed; its receiver is recorded as declared, unread from `to`, see N6 |
| `Publish`, on a run with a command of its own | the same, and also for a port the image gives no member, which then reaches that command (`to` empty) | refused, `Awaits(PublishReceiver)`, see N6 |
| `Egress` to a name, for a member | a TCP destination `name:port` the image declares for the member's network, kept or dropped, never added | refused, `Awaits(MemberGrantsByName)` |
| `Egress` to an address | the same, an IPv4 address alone | refused, `Awaits(MemberGrantsByAddress)` |
| `Host` (kiln beside the VM) | the destination `host.docker.internal:PORT`, the image's, kept or dropped | refused, `Awaits(MemberHostService)` |

So since step 9 these refusals no longer wait on Shards for what the image declares: a grant whose destination
the image declares for its member, on TCP and one port, can be programmed exactly by keeping that destination
alone; one the image doesn't declare would widen it, which D231 refuses, and is refused typed as a widening.
Hex's program doesn't do this yet: it keeps no destination (`Keep::Only([])`) and refuses each such grant as
above. **Owed to step 15:** programming them through `Narrowing.destinations`, reading `Bound::to` (N6), and the
capability names these refusals give, which name Shards' grants by name and its host service, both given now.

The exact egress form Shards had before, D192's `shards.egress=HOST:PORT`, lives on networks a daemon run joins;
D236 took the same grammar into the Agentfile, where the image boot reads it.

The refusal is the shared typed one goal 12 lands, `Refusal::Awaits { needs: Upstream }`. It names the missing
capability, never a plan step's number. It is never a success: a box whose grants can't be programmed exactly
isn't made.

## N5. One check, wherever a grant enters

`check_grants` is the one check. It runs:

- in the class spec's own check, which the caller runs and every runtime's create runs again;
- on the node's wire as a spec is read;
- in the program a class makes of the grants.

So every point refuses alike, as Shards checks a grant at build and again at run (D109: "every check twice, at
build and at run").

Shards' run host checks the image's own grants again as it reads them from the image, and refuses whatever Hex's
program would widen. Hex never sends a widening: its narrowing is empty, and its publishes are the grants'.

## N6. Hex's record of the VM

The grants live in the image class's spec, `ShardsImage.grants`, in the box's durable record
(`hex_core::record::BoxRecord`, JSON). `inspect` reports them as `BoxInfo.grants`, every grant as the record
holds it. While the VM runs, `inspect` also reports what its boot bound, `BoxInfo.published`: one
`Published { grant, at, receiver }` for each publish grant, at the host socket the host bound. That socket is
the grant's host address, on its host port or on the one the OS picked. A run doesn't outlive its owner in
D231's first version (`Awaits(AttachAfterCrash)`), so this report is the live run's, not the durable record's.

At start, Hex holds the run's own report to the grants (`hex_runtime::grants::bound`):

- each publish grant's member must be one the image runs (`ImageGuest::members()`);
- the host must have bound each publish once, where the grant names it, and nothing else
  (`ImageGuest::published()`).

A run that breaks either is stopped, and its start is refused. A member the image doesn't run is the spec's
fault, `Invalid` naming the grant's member. A socket bound past or short of the grants is Shards', `Broken`. So
no port stays published for a member or a socket no grant names.

**The receiver gap, as Hex's check still has it.** shards-api before D236 didn't say who receives a published
port: `MemberInfo` carries no ports, and `Bound` named no member. At `2365d784` it does: `Bound::to` (D231,
channel version 3) names each member the image gives the port coming in, by declared index and port, several
where several claim it, and none where the port reaches the run's own command. Hex's check doesn't read it yet
(owed to step 15), so what follows is still what Hex does. Shards' run host binds a publish for a port the image's
Agentfile gives a member coming in. For a run with a command of its own, it also binds one for a port the Agentfile gives no member and
doesn't declare as egress, and that port reaches the command (D122: "A published port no member claims reaches
the run's own command"; `ports_granted` in `crates/shards/src/api_run.rs`). There a publish grant's "to whom"
can't be held: Hex refuses a publish grant on a run with a command of its own at create, before any record,
`Awaits(PublishReceiver)` (Hex's lead, 2026-11-12: default deny refuses an inexact grant rather than let it
through). A run of the members alone (`command: None`, the harness VM) is refused a port no member is given, so
each of its publishes reaches a member, though not provably the grant's. So each `Published` records its receiver
as `Receiver::Declared`: the member the image's Agentfile gives the port, which Hex can't verify is the grant's.
Hex's lead asked Shards to name the receiving member, and D236's `Bound::to` does. Once Hex's check reads it, the
check becomes exact: a new `Receiver` variant, appended after `Declared`, records it, and a run with a command of
its own may be granted publishes.

## N7. Bounds and literals

| Literal | Value | Source |
|---|---|---|
| member name bytes | 255 | Linux `NAME_MAX`: a member's name is one directory of its image |
| DNS name characters, label octets | 253, 63 | RFC 1035 §2.3.4, in text form |
| lowest port | 1 | RFC 6335 §6 reserves port 0 |

There is no count constant. A box's grants cross the node's wire in one message (`hex_core::wire::MAX_MESSAGE`)
and live in its record (`hex_core::record::MAX_RECORD_BYTES`), so the count is bounded by bytes. Each grant
encodes in a fixed minimum. The check's one working copy is reserved fallibly and refused `Exhausted` past what
memory gives. It sorts that copy (O(n log n)), with no recursion (CLAUDE.md §11).

## Tests

- `crates/hex-api/src/grants.rs`:
  - each malformed field is refused by name, well-formed grants pass, and every overlap is refused;
  - an oracle: the sorted check agrees with a pairwise model over every pair and triple of an 84-grant universe;
  - the record's JSON form round-trips;
  - a report of published ports is held to the grants it names.
- `crates/hex-core/src/wire/tests.rs`:
  - fixed tags;
  - refusals both written and read;
  - property tests: round trip, and any bytes read or refused;
  - a Shards image VM's grants on the wire;
  - an inspect's grants and bound ports.
- `crates/hex-core/src/record.rs`: a Shards image VM's record keeps every way a grant takes, whole.
- `crates/hex-runtime/src/grants/tests.rs`:
  - no grant is no network;
  - each grant Hex doesn't program yet is refused naming what it awaits;
  - a publish on a run with a command of its own awaits its receiver's name, and one on a run of the members alone
    is programmed;
  - the program's property test (`AC-T7.7.2`), over runs with a command of their own and without: every publish it
    makes is one a grant names, its narrowing keeps nothing, and every grant it can't program exactly is refused
    typed;
  - a run's report is held to its grants.
- `crates/hex-runtime/src/guest/image_tests.rs`:
  - create refuses what awaits Shards before any record, a publish on a run with a command of its own among them;
  - a publish reaches the boot's `ImageSpec` exactly, and inspect shows it bound;
  - a publish to a member the image doesn't run stops the VM.
- Conformance `HEX-IMG-7`, which the simulator passes.
- **Owed to Hex's lead, real VMs.**
  - A harness image booted with no grants: no NIC in the guest (`/sys/class/net` holds `lo` alone), and no DNS
    answer or packet for any name (`T-T7.7.1`).
  - A `Publish` grant: the host connects at the reported socket, and nothing else is bound.
  - Once step 15 programs destinations (N4): the provider and kiln reach of the acceptance, and each N2 enforcer
    case, on D236's enforcer.

## Sources

- Shards at `2365d784`:
  - `docs/design/architecture.md`: D31, D59 parts seven, ten and eleven, D109, D122, D124, D192, D231, D236;
  - `crates/api/src/image.rs` and `image/wire.rs` (`Narrowing`, `Keep`, `Bound`, `Receiving`);
  - `crates/shards/src/api_run.rs`;
  - `crates/net/src/lib.rs`;
  - `crates/agentfile/src/agentfile.rs` and `instructions.rs`;
  - `CLAUDE.md`.
- Shards at `04abb68d`: `docs/research/measurements/harness-bringup-v1/README.md`, findings H2 and H6.
- Kubernetes NetworkPolicy, v1.37, kubernetes.io/docs/concepts/services-networking/network-policies: isolation,
  additive allow rules, reply traffic, `endPort`, and "What you can't do".
- Cilium, `Documentation/security/policy/layer3.rst` at `cb21a81b`: DNS based policy and `toFQDNs`.
- Firecracker, `docs/network-setup.md`: a tap device per microVM, filtered on the host with `nft`.
- Claude Code, code.claude.com/docs/en/sandboxing: network isolation.
- D. Fifield, C. Lan, R. Hynes, P. Wegmann, V. Paxson, "Blocking-resistant communication through domain
  fronting", PoPETs 2015(2):46-64.
- RFC 1035, 1123, 1535, 2181, 3696, 4291, 4343, 6066 §3 (SNI), 6335, 6761, 6890, 9849.
