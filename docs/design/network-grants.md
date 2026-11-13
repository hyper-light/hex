# Network grants for Hex's microVMs (joint plan step 15; CP-T7.7)

Each harness VM Hex launches reaches what its grants name and nothing else. It publishes nothing they don't
name, and Hex's record of the VM shows every grant. This record states the grant model, who enforces it, what
Hex can program through Shards today (the revision vendor/MANIFEST.toml pins, `1389dc0b` as this was reviewed, which
holds Shards' step 9: destinations, D236, and each published port's receivers, D231's `Bound::to`; each member held
apart, D246; a run's network kept, D250) and what still waits on Shards. Decided by goal 15 with Hex's lead,
2026-11-12. The lead's ruling on enforcement (N3) follows the person's decision: "Hex owns TCP/UDP for Shards
VMs" (joint plan kiln `01a1cd8e862e7169a074db3d5fa33f3d`, "Decided by the person" 2); its ruling on step 9's
destinations (N4) and receivers (N6) followed the same day.

Status, 2026-11-13 (built, merged and live apart; live is GitHub's main, `025aae6d`). Live: the grants, their one check
and the image class's boot programmed from them (`944ab292`, `9bdcc666`, `7767d4db`), each destination by TCP kept and
every member held to its own reach (`25237f5e`, `6027470c`), on Shards `1389dc0b`. Merged on Hex's main, on Shards
`740fa372`: check 2 waiting for its probe while its box settles, a run's own end recorded, and the VM granted nothing
booted both undriven and driven (`4b1b31c2`, `c56fcfbb`, `8b65e191`). `image_grants_real` passed on Linux KVM at
`8aa7c028` and on macOS HVF at `cf535152`, where its host readings say `GRANTS-OWED`, as macOS keeps no /proc. Built:
readers for those on macOS and Windows (`66cb1b1a`). Owed: those readings on HVF, every case on Windows WHP, and N2's
enforcer cases (Tests).

Requirements served: `AC-T7.7.2` (flows allowed that no grant names: 0), `T-T7.7.1` (no DNS answer and no packet
for a workload without a DNS grant), and the joint plan's step 15 acceptance: "a Hex-launched harness reaches
exactly its granted providers and kiln's endpoint, publishes nothing ungranted, and every grant is visible in
Hex's record of the VM".

## N1. The grant: who, to whom, which ports, which way

Default deny is Shards' rule as well as this step's: "a network flow that no directive explicitly grants does
not exist ... Every grant names who, to whom, on which ports, which way" (Shards `CLAUDE.md` at `1389dc0b`).
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
  - `localhost` and names under it, which resolve to loopback (RFC 6761 §6.3);
  - `host.docker.internal`, the name Shards' destinations give the VM's own host (D236), which its network
    process takes to the host's loopback: the host is a `Host` grant's, never an egress one's.
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
  rebinding). Shards `66113379`'s network process refuses a learned address that is its gateway, loopback,
  unspecified, link-local, multicast or broadcast (`Stack::allows`, `crates/net/src/lib.rs`), but no rule there
  names the host's other own addresses (its LAN address, where kiln listens on `0.0.0.0`): ROOT's real-VM case,
  and a question for Shards.
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

## N4. What Hex programs, through Shards' image boot (D231) and its hold of each member apart (D246)

Hex boots harness images through `shards_api::image::ImageShards::boot`. The run host, `shards api-run`, gives
the VM a NIC and its network process where something passes the microVM (a published port, or a narrowing that
keeps something) or where the image links its members past the microVM (D250: `Agentfile::linked_past`, its agents
granted what lies past it); otherwise the run's network is `none`, so the VM has no NIC (`crates/shards/src/api_run.rs`,
`network`, `plan`, `launch`). The network process starts at `Policy::DenyAll` and is told a policy only for what the
request keeps (`tell_net`'s `NET_POLICY`): the image's grants as the orchestrator narrows them, "egress, remote MCP
hosts, destinations (D236) and DNS the image grants, kept or dropped by the orchestrator, never added" (D231). A
widening is refused (`Cause::Widening`). So a run keeping nothing of an image that links its members past the
microVM keeps its network, and its network process lets nothing past (D250): its members are linked as the image
grants them (init's netplan, which kills a member whose link has no address), and every question and flow past the
microVM is refused.

Hex expresses everything through two fields of `shards_api::image::ImageSpec`:

- `publish: Vec<Publish { guest_port, proto, host: Option<IpAddr>, host_port }>`;
- `egress: Narrowing { egress: Keep, mcp: Keep, destinations: Keep, dns: bool, members: Option<Vec<MemberReach>> }`.

`Narrowing::default()` keeps the image's grants whole, its destinations among them, so Hex never uses it: every
narrowing Hex sends keeps no port egress, no remote MCP server and no name past the VM but its destinations'
(`Keep::Only([])`, `dns: false`), with no grant no destination either, and names every member's reach (`Some`,
a member in no entry reaching nothing); tests pin each (`no_grant_is_programmed_as_no_network`, the property test,
the boot-spec tests).

**Destinations (D236).** An Agentfile names a member's destinations as `NETWORK --egress=HOST:PORT`, beside the
ports it opens outward (`EXPOSE … AS egress`): TCP to a host's name, an IPv4 address, or `host.docker.internal`,
the host itself at that port alone, which the network process takes to the host's loopback there. They are exactly
the forms of Hex's TCP `Egress` and `Host` grants: `<name>:<port>`, `<a.b.c.d>:<port>` and
`host.docker.internal:<port>`, one for each port of a grant's range. Kiln's endpoint beside the VM is a `Host` grant
of its port, so it is held the same way as a provider is.

What a member reaches of the names kept, `K`, is `named(X) ∩ K`, where `named(X)` is what the image lets it reach by
name, its networks' destinations and its remote MCP servers' (the agentfile crate's `named_reach`), and `K` the
destinations and remote MCP servers kept alike (D246: the network process holds the microVM to both). Hex keeps no
remote MCP server, so its `K` is the destinations its grants name; a member whose remote MCP server is one of them
reaches it too, and is held to it as to a destination.

- its init lets it out on those ports alone, and its agents' resolver answers it those names alone
  (`crates/init/src/netplan.rs`, `destinations_of`, `Link::named`; the host itself by the name in its hosts file,
  never asked);
- the network process holds the whole VM to the names kept, refusing every other flow and question (D192, D236);
- D236's `held` refuses, at build and as the run host reads the image, any image in which a member reaching a port
  by name shares it with a member reaching other names there, or any address by a port grant.

D236 alone keeps `K` image-wide (`Narrowing.destinations`), and shards-api named no member's destinations before or
after a boot, so it gave Hex no way to keep a destination for some members and not others. The lead's ruling (R02,
2026-11-12) was (B): refuse rather than leave a window, and close it in Shards. Its alternatives:

1. **Rejected: keep, then hold the run as its boot returns**, stopping a run where some member the grants don't
   name is let out on a kept destination's port, as N6 holds publishes. In an image of several members an ungranted
   one could reach a kept destination between its start and its stop.
2. **Not possible through shards-api alone: read an image's members before its boot.** Their destinations live in
   the image's normalized Agentfile, `/.agentfile.json` inside its EROFS root in Shards' store
   (`crates/shards/src/agentfile.rs`, `agentfile`), and shards-api holds no store, EROFS or Agentfile code; any route
   to it is a change to the `shards` binary.
3. **Chosen: each member's reach in the narrowing, held by the run host.** Built first as Hex's recorded change
   0002 to its vendored Shards (Ada, 2026-11-12: an upstream blocker is fixed in Hex's vendored copy and carried to
   Shards' ROOT), it is upstream as D246 (`7a4ef5c02`), which holds the remote MCP servers kept beside the
   destinations, and Hex vendors it with its change dropped. The orchestrator names what each member may
   reach (`Narrowing::members`, `MemberReach`). The run host, from the Agentfile it already reads, refuses the boot
   before the VM starts unless every member the image declares, named or not, reaches exactly what it is let:
   `named(X) ∩ K` equal to the member's entry, sorted, each once, a member in no entry nothing (`api_run`'s
   `held_apart`): else a widening of what was asked (`Cause::Widening`), naming the member and both reaches; an entry
   for a member the image declares not is the image's refusal (`Cause::Image`), and a member's second entry the
   request's (`Cause::Request`). Neither init nor the network process changes.

So Hex keeps `K`, every destination its grants name, and names each member's reach, its own grants', on every boot.
The image's grants bound it from above (a destination the image grants no one is refused as a widening), and the run
host from both sides (a member that would reach more than its grants, or less, refuses the boot). Each refusal
reaches Hex's start typed, through shards-api's `ImageRefusal`: a widening `Invalid` past the image's grants, the
image's `Invalid` naming the image (a grant for a member it doesn't declare), the request's `Invalid` naming the
request (Hex names each member once, which the property test pins). The channel is at version 13 for every request,
the hold a flag in it (D246's version 12, D247's rate 13), so the run host must speak version 13, as Hex's `shards`
built by `scripts/shards-gpu/build.sh` at the vendored revision does: an older one refuses the request by its
version (`Cause::Request`, naming both versions) and never runs it.

| A box's grants | What Shards holds (D236, D246, D250) | Hex |
|---|---|---|
| none | no NIC where the image links no member past the microVM (`network none`); else a NIC whose network process, told nothing kept, lets nothing past (D250) | programmed: nothing kept, no publish, every member reaching nothing |
| `Publish` of a member's port at a host address, on any run | `Publish { guest_port, proto, host: Some(addr), host_port }`, for a port the image gives a member coming in, or (a run with a command of its own) any port the image doesn't declare as egress, which then reaches the command (D122) | programmed; held to the receiver Shards names as the boot returns (N6) |
| `Egress` by TCP to a name or an IPv4 address | a destination the image grants, kept, and each member held to its reach of the names kept (D246) | programmed: kept, its member held to it alone |
| `Host` by TCP (kiln's endpoint beside the VM) | `host.docker.internal:<port>`, the host's loopback at that port alone, held alike | programmed alike |
| `Egress` by UDP to a name | no UDP destination (D236 is TCP) | refused `Awaits(MemberGrantsByName)` |
| `Egress` to an IPv6 address, or by UDP to an address | no IPv6 or UDP destination | refused `Awaits(MemberGrantsByAddress)` |
| `Host` by UDP | the host itself by TCP alone | refused `Awaits(MemberHostService)` |
| `Egress` to `host.docker.internal` | the host itself | invalid: the host is a `Host` grant's (N1) |

`hex_api::unheld` is the one rule of what awaits Shards, the class's and the simulator's: the first grant D236
can't express at all names its capability (`Refusal::Awaits { needs: Upstream }`), never a plan step's number, and
before anything is made. `Awaits(MemberDestinations)` and `Awaits(PublishReceiver)` are no longer refused; their
tags stay on the wire.

## N5. One check, wherever a grant enters

`check_grants` is the one check of a grant's form, and `unheld` the one rule of what Shards can hold (N4), which
the class and the simulator share. The check runs:

- in the class spec's own check, which the caller runs and every runtime's create runs again;
- on the node's wire as a spec is read;
- in the program a class makes of the grants.

So every point refuses alike, as Shards checks a grant at build and again at run (D109: "every check twice, at
build and at run").

Shards' run host checks the image's own grants again as it reads them from the image, and refuses whatever Hex's
program would widen: Hex keeps only what its grants name, and a grant the image's own grants don't give is refused
there, typed, before anything starts.

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
  (`ImageGuest::published()`);
- each such port must reach the grant's member alone, as Shards names its receivers (D231's `Bound::to`: each
  member the image gives the port coming in, by declared index, at the member's own port; several where several
  claim it, the one listening taking each connection; none where it reaches the run's own command).

A run that breaks any is stopped, and its start is refused. A member the image doesn't run, or a port the image
gives anyone else, is the spec's fault, `Invalid` naming the grant and whom the port reaches. A socket bound past
or short of the grants, or a receiver that is no member of the image, is Shards', `Broken`. So no port stays
published for anyone its grant doesn't name. Each `Published` records its receiver as `Receiver::Named { port }`:
the grant's member, at its own port there (the published port, or the one an `EXPOSE` maps it to). A publish is
granted on a run with a command of its own too, since its receiver is held the same way: `Awaits(PublishReceiver)`
is lifted, its tag kept for the wire, and `Receiver::Declared`, the unverified receiver of Shards `482c5d78`, is
reported no more.

**The residual.** D231 binds a publish before the members start and names its receivers in the boot's answer, so
Hex holds a run to them once the boot has returned: a connection to a port that reaches someone else could arrive
between the bind and Hex's stop. What closes it is N4's route for the receiver: the boot request names each publish's
receiver and the run host, which reads whom each port reaches from the Agentfile before it binds anything
(`agentfile::receiving`), refuses the run before it starts; then the check here becomes Shards' refusal. Hex's lead
raises it with the person beside N4's change.

## N7. Bounds and literals

| Literal | Value | Source |
|---|---|---|
| member name bytes | 255 | Linux `NAME_MAX`: a member's name is one directory of its image |
| DNS name characters, label octets | 253, 63 | RFC 1035 §2.3.4, in text form |
| lowest port | 1 | RFC 6335 §6 reserves port 0 |
| the host's name | `host.docker.internal` | Shards D236, Docker Desktop's name for the host, under `.internal`, the top-level domain ICANN reserved for private use |

There is no count constant. A box's grants cross the node's wire in one message (`hex_core::wire::MAX_MESSAGE`)
and live in its record (`hex_core::record::MAX_RECORD_BYTES`), so the count is bounded by bytes. Each grant
encodes in a fixed minimum. The check's one working copy is reserved fallibly and refused `Exhausted` past what
memory gives. It sorts that copy (O(n log n)), with no recursion (CLAUDE.md §11).

## Tests

- `crates/hex-api/src/grants.rs`:
  - each malformed field is refused by name, well-formed grants pass, and every overlap is refused;
  - the VM's host by its Shards name is refused as an egress name;
  - an oracle: the sorted check agrees with a pairwise model over every pair and triple of an 84-grant universe;
  - the record's JSON form round-trips;
  - a report of published ports is held to the grants it names.
- `crates/hex-core/src/wire/tests.rs`:
  - fixed tags, a named receiver's (1, then the member's port) and a declared one's (0);
  - refusals both written and read, every capability by its place in `Upstream::ALL`;
  - property tests: round trip, and any bytes read or refused;
  - a Shards image VM's grants on the wire;
  - an inspect's grants and bound ports.
- `crates/hex-core/src/record.rs`: a Shards image VM's record keeps every way a grant takes, whole.
- `crates/hex-runtime/src/grants/tests.rs`:
  - no grant keeps nothing past the microVM (`Narrowing::default()` keeps it all) and holds every member to
    reaching nothing;
  - each destination by TCP kept as D236 writes it and its member held to it alone, several members' and ports'
    each kept once and sorted, and what D236 can't express refused naming its own capability, first;
  - a publish is programmed alike on a run with a command of its own and on one of the members alone;
  - the program's property test (`AC-T7.7.2`), over runs with a command of their own and without: every publish it
    makes is one a grant names, its narrowing keeps exactly the list's destinations and no remote MCP server (D246's
    names kept) and holds each member to its own in one entry (a model apart from the program's), and every list it
    can't program exactly is refused by a model's reading of N4;
  - a run's report is held to its grants: its receivers named at their own ports; a port reaching the run's own
    command, another member or several refused; a receiver its image doesn't declare `Broken`.
- `crates/hex-runtime/src/guest/image_tests.rs`, over a model of D231 that names each port's receivers, refuses a
  port no member is given on a run of the members alone, and holds every member apart as Shards' run host does
  (D246's `held_apart`: an entry for a member the image declares not the image's refusal, a member's second entry
  the request's, and every member's reach of the names kept, destinations and remote MCP servers alike, exactly its
  entry's):
  - create refuses what awaits Shards before any record;
  - a provider and kiln's endpoint reach the boot's narrowing kept and held to the harness alone;
  - a destination another member of the image would reach too refuses the boot before anything starts;
  - a destination for a member the image doesn't declare refuses the boot before anything starts, naming the image;
  - a publish reaches the boot's `ImageSpec` exactly, and inspect shows it bound, its receiver named;
  - a publish on a run with a command of its own runs where its port reaches the grant's member, and stops the VM
    where it reaches the run's own command;
  - a publish to a member the image doesn't run stops the VM.
- Conformance `HEX-IMG-7`, which the simulator passes.
- Shards' own tests, upstream at the vendored revision: D246's (the agentfile crate's `each_member_reaches_its_own_names`, the
  wire's `each_members_reach_rides_the_request_last`, `api_run::tests::each_member_is_held_to_its_reach_of_the_names_kept`)
  and D250's (`api_run::tests::a_run_keeping_nothing_past_the_microvm_has_its_members_network_and_no_policy`).
- `crates/hex-cli/tests/image_grants_real.rs`, without a VM, on Linux, macOS and Windows: the grants probe
  (`image_grants_probe/probe.rs`) answers a connection, and each line on its stdin, with its targets' outcomes,
  typed (connected, refused, unresolved), waiting for each try's own answer; its report and arguments read back as
  written, each malformed one refused, a wait of its own among them.
- **Owed to Hex's lead, real VMs**: `image_grants_real`'s ignored test, on a node booting the probe image, a harness
  whose program tries its egress targets and reports each typed on every connection to its ports and every line on
  its stdin (`scripts/grants-probe/build.sh` makes its context on each host: the probe built static for the guest's
  arch, `<arch>-unknown-linux-musl`, by rust-lld), with `SHARDS_BIN` and `SHARDS_INIT` from one build of Hex's
  `shards` at the vendored revision (`scripts/shards-gpu/build.sh`). On Linux KVM, macOS HVF and Windows WHP, once
  the image class boots there (goal 12):
  - a destination the image grants not refuses the boot, past the image's grants, before anything starts;
  - a harness image booted with no grants, its probe asked on its stdin: its names unresolved (no DNS answer) and
    none of its targets connected (`T-T7.7.1`), the host's service taking nothing; its image links its harness past
    the microVM, so its run keeps a network whose network process lets nothing past (D250);
  - a `Publish` grant: the probe answers at the reported socket, which inspect shows reaching the probe at its port;
    its own reach with destinations dropped (its provider and an ungranted name, the host's service and a port
    egress) connected nowhere, and the host's service took nothing;
  - granted its provider and the host's service (kiln's endpoint), the probe reaches both, its ungranted name and the
    image's port egress nowhere, and the host's service takes its connection: the acceptance's reach;
  - a publish reaching the run's own command: the start refused, the VM stopped;
  - on Linux alone, from /proc, the run's host side: with no grant its one network process (D250), listening on
    nothing; with a publish its one published socket the only one it listens on; no flow past the host's loopback;
    and no run host left. macOS and Windows keep no /proc, so those readings are owed there (the run says
    `GRANTS-OWED`);
  - each N2 enforcer case (expiry, rebinding, address families), and a two-member image whose members share a
    destination: the boot refused unless both are granted it.

## Sources

- Shards at `1389dc0b`:
  - `docs/design/architecture.md`: D31, D59 parts seven, ten and eleven, D109, D122, D124, D192, D231, D236, D246,
    D247, D250;
  - `docs/research/measurements/s9-grants-v1/README.md`, step 9's record;
  - `crates/api/src/image.rs` and `image/wire.rs` (`Narrowing.destinations`, `Narrowing.members`, `Bound::to`,
    channel version 13);
  - `crates/shards/src/api_run.rs` (`kept`, `egress`, `held_apart`, `network`, `ports_granted`, `published`);
  - `crates/shards/src/agentfile.rs` (`Agentfile::named`, `linked_past`);
  - `crates/init/src/netplan.rs` (`destinations_of`; the host's name in a member's hosts file);
  - `crates/net/src/lib.rs` (`Stack::allows`, `host_granted`);
  - `crates/dns/src/lib.rs` (`destination`, `HOST`);
  - `crates/agentfile/src/agentfile.rs` (`destinations_on`, `held`) and `instructions.rs`;
  - `CLAUDE.md`.
- Shards at `482c5d78`: the boot N4 and N6 first programmed, which named no receiver; at `66113379`, Hex's change
  0002 (each member's reach, channel version 5) that D246 took upstream.
- Shards at `04abb68d`: `docs/research/measurements/harness-bringup-v1/README.md`, findings H2 and H6.
- Kubernetes NetworkPolicy, v1.37, kubernetes.io/docs/concepts/services-networking/network-policies: isolation,
  additive allow rules, reply traffic, `endPort`, and "What you can't do".
- Cilium, `Documentation/security/policy/layer3.rst` at `cb21a81b`: DNS based policy and `toFQDNs`.
- Firecracker, `docs/network-setup.md`: a tap device per microVM, filtered on the host with `nft`.
- Claude Code, code.claude.com/docs/en/sandboxing: network isolation.
- D. Fifield, C. Lan, R. Hynes, P. Wegmann, V. Paxson, "Blocking-resistant communication through domain
  fronting", PoPETs 2015(2):46-64.
- RFC 1035, 1123, 1535, 2181, 3696, 4291, 4343, 6066 §3 (SNI), 6335, 6761, 6890, 9849.
