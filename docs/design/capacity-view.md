# One capacity view per machine: Hex and kiln's load balancer, peer to peer

Step 13 of the joint plan (kiln plan `01a1cd8e862e7169a074db3d5fa33f3d`, version 9: "One capacity view
per machine, peer to peer"), joint with kiln's Load Balancing agent (kiln agent
`01a198e7a285772298e00399c5c7d5de`); it serves AC-T7.4.1 (placement by Hex's scheduler). Hex's
scheduler and kiln's load balancer each decide where work goes at the node that asks, and each has its
own admitter on the machine the work lands on: Hex's node agent admits boxes against its ledger (B3 of
`scheduler-architecture.md`), kiln's runner admits runs against its own (kiln `docs/lb.md` "The
runner"). On a machine that runs both, each admitter checks only its own books. This record decides
how both admit against one view, so neither oversubscribes the other, and how a dropped link or a crash
on either side reconciles without double-booking. Each decision has its sources, the alternatives and
what would falsify it (CLAUDE.md §5; Shards R02).

Acceptance (the plan's): on one machine, Hex placements and kiln runs never together exceed its room; a
dropped link or a crash on either side reconciles without double-booking. Tests: hex-sim and kiln-ci's
simulator, then ROOT on two real machines; passes when no oversubscription across crash seeds and the
counts the two sides keep agree after a partition heals.

## What it is

- `crates/hex-api/src/capacity.rs`: the protocol as two pure cores (CLAUDE.md §9; sans-IO): the
  [`Keeper`](../../crates/hex-api/src/capacity/keeper.rs), the machine's one ledger of reservations,
  and the [`Holder`](../../crates/hex-api/src/capacity/holder.rs), the side that reserves through it.
  They are in `hex-api` because kiln reaches Hex only through it (CLAUDE.md §9), and kiln's runner
  uses the same holder core (Shards R12: one implementation).
- The seam (CLAUDE.md §9): `CapacityView`, the keeper as a transport serves it, and `Host`, its
  host's side; the simulator `capacity::sim::SimView`, which kiln's simulator drives as the machine's
  Hex node; and `capacity::conformance::view` (HEX-CV-1 to 8), which the simulator and the node's own
  view both pass, and a keeper kiln hosts would.
- The node's view (`crates/hex-node/src/placement/view.rs`): the keeper's shell in the placement
  `Driver`, under the lock its own admissions take. Its records are a folder of their own in the
  node's record files; its room is the node's facts' capacity (`Driver::refresh_node`); the node
  admits its own boxes against that room less every holder's reservations, given to its admission at
  once whenever they change; and `local` is what the node holds itself (`peer::Node::held`: the larger,
  per dimension, of its ledger's admissions and its admission index's, which charges backings too).
- hex-sim scenarios: crashes on either side, dropped links, partitions and heals.
- The shell over hyper-transport, once Hex step 4's transport is on main: the scheduler opens the
  view (`Driver::open_view`) with the holders its configuration authorizes, and carries the messages.

## The defect today

- Hex's room for a node is every hardware thread's CPU and the measured free memory and disk plus its
  own boxes' measured use (`crates/hex-sim/src/facts.rs`, `Facts::node_with_owned`); its ledger admits
  against that (`crates/hex-sim/src/peer.rs`, `Node::start`, `refresh_node`'s `set_budget`). kiln's
  runs on the same machine are invisible to it until their use shows in measured free memory, and never
  in CPU.
- kiln's runner admits against its budget less `outside`, its measured use beyond what it admitted
  (kiln `docs/lb.md` "Room the ledger doesn't hold"; `crates/kiln-ci/src/place/runner.rs` `sync_fast`,
  kiln origin/main `b2dfa3f2c`). Hex's boxes reach it only as measured use, never as reservations.
- So a reservation one side holds but hasn't used yet (a microVM's memory before its guest plugs it,
  any CPU) is free room to the other: both admit into it, and the machine is oversubscribed by
  construction. Omega names the failure: when schedulers compete, "multiple schedulers may attempt to
  claim the same resource simultaneously" (Schwarzkopf et al., EuroSys 2013, §3).

## Decisions

### CV1. One keeper per machine; every other admitter reserves through it

**The choice.** A machine's room has one authoritative ledger, the **keeper**, hosted by the agent that
measures the room: Hex's node agent wherever one runs. Hex's own admissions are the keeper's **local**
held; every other admitter on the machine (kiln's runner) is a **holder** that reserves through the
keeper before it starts work. Deciders stay where they are and stay peer to peer: Hex's owners and
kiln's owners decide at the node that asks, on the view each machine publishes; the keeper validates at
commit. The protocol and both cores are symmetric, in `hex-api`: on a machine with no Hex node, kiln's
runner could host the keeper, and its implementation passes the same suite.

**Sources.**
- Omega (Schwarzkopf et al., EuroSys 2013, §3.4): schedulers in parallel on shared state need "a common
  notion of whether a machine is full", and the allocator is "simplified to a persistent data store with
  validation code that enforces these common rules"; a scheduler "updates the shared copy of cell state
  in an atomic commit. At most one such commit will succeed in the case of conflict", then resyncs and
  retries. (Research finding f68 of the joint plan, grade A.)
- Kubernetes runs several schedulers at once with "no builtin mechanism to prevent such race
  conditions", and is safe because "Kubelet however will not admit a pod unless it has enough resources
  to run it" (kubernetes-dev, A. Gharaibeh, 2020-06-21): one admitter per node, whichever scheduler
  decided.
- Nomad: "Multiple schedulers are running in parallel without locking or reservations ... schedulers
  might overlap work on the same node and cause resource over-subscription. The plan queue allows the
  leader node to protect against this and do partial or complete rejections of a plan"
  (developer.hashicorp.com/nomad/docs/concepts/scheduling/scheduling).
- kiln's runner ledger is "the one place a run is admitted, against the runner's own room, never past
  its budget" (kiln `crates/kiln-ci/src/lb/runner.rs:1-2`); Hex's B3 has the node admit against its
  measured room. Both are one admitter per room; this decision keeps that with two products on one room.

**Alternatives.**
- *Each side subtracts what the other publishes* (optimistic, no commit point): both admit into the same
  free room before they hear each other, and undoing means stopping work already started. Rejected:
  Omega's optimistic schedulers are safe only because one commit point validates.
- *Escrow with demarcation* (O'Neil, ACM TODS 11(4), 1986; Barbará and Garcia-Molina, VLDB Journal 3(3),
  1994; Balegas et al., SRDS 2015; kiln's own `place/budget.rs`): each side holds a share and admits
  within it without a round trip; shares move by the giver lowering first. Safe under any partition,
  but rejected for a machine's room: the room is measured and moves (B3: memory outside Hex's books),
  so someone must cut the shares when it shrinks, which is an owner again; room left in the wrong share
  strands while a link is down; and how much each side keeps needs a learned demand model, for a round
  trip that stays on one machine. It is the falsifier's alternative below.
- *Mesos-style offers* (pessimistic: "only offering a given resource to one framework at a time ...
  concurrency control is pessimistic", Omega §3.3): serialises the two sides' decisions behind the
  offer, and reclaiming a hoarded offer needs a timeout. Rejected.

**What would falsify it.** The keeper's round trip showing in kiln's run-start latency at the measured
p99 on the two-machine run (ROOT's), against the same runs admitted by kiln's runner alone; then the
escrow alternative, with each side's share learned from its measured arrival rate times the round trip
(kiln's `place/budget.rs` rule), is measured against it.

### CV2. What a reservation is, and what the keeper writes

- **Holder**: a name (the identity its link authenticates; for kiln's runner, its runner identity) and
  an **epoch** that grows each time the holder's process starts, durable on the holder's side (kiln's
  runner has one: kiln `docs/lb.md` "Fences"; Hex's node has one: `peer::Node`).
- **Reservation**: `(holder, id, amount)`. The id is `(epoch, seq)`, the holder's epoch when it made it
  and its count in that epoch, so an id is never reused and the keeper needs no tombstones (as
  `peer::Key` names boxes). The amount is CPU in thousandths, memory and disk in bytes, and GPUs, the
  dimensions both kernels already count (`hex_sim::lb::Amounts`, kiln `lb` `Amounts`).
- **Durable before said**: the keeper writes a reservation before it answers `Granted`, removes it before
  `Released`, writes a holder's new epoch before it answers that holder, and its own epoch grows at each
  start before it serves (the order `peer.rs` keeps: "a node ... writes its record of a box before the
  box is created"). A keeper that crashes after a write and before its answer holds a reservation its
  holder never heard granted; the holder's next sync releases it (CV5). Nothing is ever answered that
  isn't kept.
- **Its records are a folder of its own**: `epoch`, `holder-<n>` and `res-<n>-<epoch>-<seq>`, `n` a u32, so
  at full width a name is as long as the node's own `own-u32-u64-u64` and the node's record files hold
  it (`crates/hex-node/src/records.rs`); a name it didn't write is damage, refused at boot.
- **The host's own admissions are not keeper records.** Hex keeps each box it admits in its own record
  (`peer::Node`'s `ran` record, written before the box is created) and passes their sum, `local`, into
  every keeper decision. One durable write per box, not two; and the keeper still decides on one sum.

### CV3. One rule for full, both sides

A charge `c` is admitted iff, in every dimension, `c <= room - local - foreign`, with checked
arithmetic (Omega's "common notion of whether a machine is full"). The host's ledger admits against a
budget of `room - foreign` (`Keeper::room_for_local`), so its own check is the same inequality; a
holder's reserve is checked by the keeper against it. The charge is what each side's admission charges:
on Hex's side goal 14's learned charge, its envelope the ceiling; on kiln's, its run's ask. The room is
the host's measured room (Hex: `Facts::node_with_owned`). When the room shrinks below what is held, the
view is **over**: nothing new is admitted until it fits, and running work keeps its reservations (kiln's
`set_budget` rule, `docs/lb.md`); use past a charge is goal 14's contention rule, not this view's.

GPUs are counted (the host's GPUs are in its room and in `local`), but a holder's reservation that asks
any is refused, typed (`Why::Devices`): a GPU is held by its identity through Hex's device holder
(`hex-api` devices), and a count can't say which one. Device grants for holders are an open item.

### CV4. Fencing by epochs and sessions

Every message a holder sends after its hello carries a **fence**: the keeper's epoch, the session the
keeper opened for it, and the holder's epoch. The keeper answers anything else `Closed` and acts on
none of it, so a process older than the holder's current one, or a late message from a link that has
since dropped, can neither reserve nor release.

**Sources.** Chubby's sequencers (Burrows, OSDI 2006, §2.4): a server checks a request's sequencer
"against the most recent sequencer that the server has observed" and, if it isn't valid, "should reject
the request"; "the sequencer mechanism requires only the addition of a string to affected messages".
Chubby's lock-delay, which frees a lock some time after its holder becomes inaccessible, is by its
authors "imperfect" and is not taken (CV6).

### CV5. Reconciliation: the holder's whole live set, and never a release on silence

- On every hello the holder answers `Welcome` with a `Sync` listing every reservation whose work may be
  running, those of earlier epochs too. The keeper releases the holder's reservations not listed,
  adopts any listed it lacks (they run, so they are held, even past the room: the view is then over),
  takes the larger amount where the two disagree, and answers `Synced` with its count and sum of the
  holder's reservations. The holder compares them with its own.
- A dropped link or a holder's crash releases nothing: the keeper keeps the holder's reservations until
  that holder's next sync says otherwise. This is `peer.rs`'s rule (a box is placed again "only on that
  node's word that it doesn't hold it ... never on its silence, so no box starts twice") and kiln's
  (`docs/lb.md` "A dropped link": at most once, no per-run log).
- A release is idempotent: releasing an id the keeper doesn't hold answers `Released`.
- A reserve the holder heard no answer to (its link dropped, or its wait ran out) is **abandoned**: its
  work never starts; the holder releases it at once if its link is up (the keeper may have granted it),
  or leaves it out of its next sync.

**What "the counts agree after a partition heals" means.** Once a holder's sync is answered and no
reserve or release of its is in flight, the keeper's set of that holder's reservations, ids and
amounts, equals the holder's set of reservations granted and not yet released; and the view the keeper
answers (room, and everything held: `local` plus every holder's sum) is the one the holder holds. The
holder checks the first on every `Synced` (`Event::Synced { agree }`); hex-sim checks both at every
quiescent step after a heal.

### CV6. No leases on reservations; the only learned time is a holder's wait

**The choice.** No term ends a reservation. A holder's wait for an answer (reserve, release, sync) is
learned, per call kind and class, by hex-core's Patience on kiln's side as on Hex's (decision 9 of
`runtime-architecture.md`): seeded from the waits census, learned from answers, never a picked
constant. A wait that runs out abandons its reserve (CV5); it never ends anything the keeper holds.

**Why.** A lease (Gray and Cheriton, SOSP 1989) lets the grantor take back what it granted when the term
ends without the holder's word, because the holder stops using it at the term. A reservation here is
CPU and memory that running work uses. Taking it back at a term either double-books it (the work still
runs) or requires the holder to stop its work at the term (a keeper restart longer than the term would
end every kiln run on the machine). Chubby's lock-delay is the same trade, by its authors "imperfect".
Fencing (CV4) already keeps a stale holder from acting, which is what a lease's expiry would buy.

**A holder gone for good: the person's release.** A holder that never comes back would keep its
reservations for ever: under-use, never double-booking. The person releases them by name
(`Keeper::forget`): refused, typed, while the holder has a session (`NotForgotten::Connected`: its own
sync says what it holds) and for a name never seen (`Unknown`); otherwise every reservation of it is
removed from the records, the other holders are hinted, and what was released (its epoch, count and
sum) is returned for the shell to log on the node's capacity event target. Its epoch stays, so a process
of it from before is still fenced; and if the person was wrong and some of its work still runs, the
holder's own next sync adopts it (CV5), so the release never hides running work from the view for
longer than the holder is away.

### CV7. The view reaches every decider, by hints

Every keeper answer carries the **view**: the room, and everything held (`local` plus every holder's
reservations). When the view changes for a reason a holder didn't cause (the host admitted or ended a
box, the room was measured anew, another holder reserved or released), the keeper owes each synced
holder one `Hint`, coalesced while unsent and sent when the shell drains (backpressure, no timer; B3,
kiln `docs/lb.md` "Hints"). Each side folds the view into what its own deciders see:
- Hex: its ledger's budget is `room - foreign`, so its words to Hex's owners carry the combined free room;
- kiln: its kernel ledger's budget and `outside` (kiln's integration, its Load Balancing agent's).

Deciders still decide on a copy, and the keeper still validates each admission: a refusal carries the
view, so the next decision is made on the room there is (Omega §3.4: resync, then retry).

### CV8. Bounds

| Bound | Value | Derivation | Overflow |
|---|---|---|---|
| Holders a keeper serves | `Budget::holders` | the holders the host's configuration authorizes on this machine (its declared peers) | `Closed { HoldersFull }` |
| Reservations per holder | `Budget::reservations` | the host's own record bound: what its records directory is sized to hold (the shell's, checkpoint 2) | `Refused { Full }`; a sync longer is `Closed { Invalid }` |
| A holder's name | `Budget::name_bytes` | the host's wire message bound (`hex_core::wire::MAX_MESSAGE` on Hex's shell), so a hello always fits one message | `Closed { Invalid }` |
| Hints owed | one per holder | coalesced: the newest view replaces the unsent one | none: a flag |

Every amount is an integer with checked arithmetic; a sum that would overflow is refused, typed.

### CV9. Carried over hyper-transport, after Hex step 4

The messages become variants of `hex_core::wire` (append-only, as every wire change) carried on step 4's
control lane, TLS 1.3 with `TLS13_AES_256_GCM_SHA384` alone and identities from kiln's CA (the plan's
header). Until step 4 is on main the cores run in process and over hex-sim's simulated links only; a
remote binding asked for before then is refused, typed, naming the transport it waits on.

### CV10. Where a keeper is, and where there is none

A holder admits on its own, as kiln's runner does today, on a machine where no keeper is known.
A keeper becomes known to a holder in one of two ways:
- **configured**: the holder's configuration names the machine's keeper (its endpoint and the identity
  its link authenticates);
- **advertised**: the machine's Hex node says it keeps the room where the holder already reaches it, its
  node socket's answer to the holder (and, across machines, its hyper-swim membership entry, Hex's
  gossip, S4), naming the keeper's endpoint.
Once a keeper is known for a machine, the holder records it durably and rule 6 of "What a holder owes"
holds there from then on: no admission there without a sync. A keeper that stops answering is not
taken for no keeper (that would double-book against the reservations it still holds, CV5); only the
person removing it from the holder's configuration returns the holder to admitting on its own there.

### CV11. A keeper that is down, and one that restarts

- **Down**: a holder whose keeper doesn't answer admits nothing new on that machine; its deciders see no
  room there (kiln's runner says none in its words, as a cordon does), so the load balancer places the
  work elsewhere; runs already started go on, their reservations held in the keeper's durable ledger.
  Hex's own admissions stop with it, since the keeper is in Hex's node agent.
- **Restart**: the keeper boots with a new epoch (`epoch` grows by one, written before it serves), its
  ledger read whole from its records (every reservation it granted, CV2), every holder away and every
  earlier fence closed (CV4). Each holder says hello again and syncs (CV5): the keeper releases what the
  holder no longer lists and adopts what it lacks, and the counts agree again before the holder reserves.

## The protocol

**Holder to keeper** (`ToKeeper`): `Hello { holder, epoch }`; `Sync { fence, held: [(id, amount)] }`;
`Reserve { fence, id, amount }`; `Release { fence, id }`.

**Keeper to holder** (`ToHolder`): `Welcome { fence, view }`; `Synced { fence, view, count, sum,
released, adopted, disagreed }`; `Granted { fence, id, amount, view }`; `Refused { fence, id, why,
view }` (`why`: `Room`, `Full`, `Empty`, `Devices`, `Disagrees`); `Released { fence, id, view }`;
`Hint { fence, view }`; `Closed { epoch, why }` (`why`: `OlderEpoch`, `Session`, `NotSynced`,
`HoldersFull`, `Invalid`).

**The keeper, per holder** (R04): `Away` (no session; its reservations kept) → hello → `Joined
{ session }` (a sync is owed) → sync → `Synced { session }` (reserves and releases served) → link down →
`Away`. A hello under an older epoch is `Closed { OlderEpoch }` and changes nothing; under a newer one
the new epoch is written before the welcome; a hello on a live session supersedes it.

**The holder** (R04): `Down` → hello sent → `Joining` → welcome → `Syncing` (sync sent) → synced →
`Up`; link down from any → `Down`. Each reservation: `Asking` (reserve sent) → `Held` (granted) →
`Releasing` (release sent) → gone; `Asking` → gone on a refusal, or abandoned on a link down or a wait
run out.

**What a holder owes** (kiln's runner, through the holder core):
1. Start work only on `Granted` for its reservation's id.
2. Its epoch grows at every start of its process and is durable.
3. Its sync lists every reservation whose work may still run, of any of its epochs.
4. Release a reservation once its work can no longer run.
5. Never reuse an id; treat a reserve it heard no answer to as not granted.
6. Where the machine's room has a keeper (configured, or advertised by its Hex node: CV10) and it
   isn't synced with it, admit nothing new there (CV11).

**What the keeper owes** (Hex's node agent): grant only what fits (CV3), write before it answers (CV2),
fence (CV4), release only on its holder's word (CV5), and tell each holder the view (CV7).

## The interface with goal 14 (learned charge)

Goal 14 owns what a VM is charged, its booking and the contention rule (`learned-charge.md`); this step
owns the room and the combined view. Agreed through ROOT (2026-11-12, `learned-charge.md` C6-C8):
- **What Hex publishes** is the keeper's `local`: per dimension the sum of its VMs' bookings (a booking
  is the learned charge), and for memory and disk what VMs hold past their bookings too
  (`hex_sim::charge::Node::reservation`). It rises without an admission (a booking rising into free
  room, a VM holding memory past its booking): the host calls `Keeper::local_changed`, and every synced
  holder (kiln's balancer) is owed the view (CV7), so both sides back off.
- **Hex's admission**: a new VM is admitted only if its booking fits the combined view's free room for
  Hex, `Keeper::view(local).free()` (`Node::admit(.., free)`); refused, typed, with that room (B3).
  Bookings follow charges: down at once, up only into that free room.
- **Hex's budget** on a machine (goal 14's fleet budget, each guest's D188 room) is the view's room for
  Hex, `Keeper::room_for_local()`: the machine's room less every holder's reservations.
- **A holder's reservation** keeps its amount for its life; a holder releases it and reserves anew to
  change it.

Goal 14 adds an optional `Allocation.booked` to Hex's runtime wire (append-only, absent meaning the whole
allocation); the capacity protocol carries amounts only and is unchanged by it.

## Tests

- Unit tests of both cores (`hex-api`, `capacity::tests`): each transition, each refusal, each fence,
  sync's three outcomes (released, adopted, disagreed), idempotent release, the person's release, the
  keeper crashed at every write of a conversation before and after the write lands, hostile record text.
- The conformance suite (`capacity::conformance::view`, HEX-CV-1 to 8) against the simulator, and
  against the node's view over its record files, restarted from them (`hex-node`,
  `placement::view::tests`); and the node's own admission and a holder's reserve checking one view
  through the placement `Driver`.
- hex-sim (`hex_sim::capacity`): one machine of sixteen CPUs and 64 GiB, Hex's own boxes and kiln's
  runners on its keeper, under a nemesis that crashes the keeper between messages and at its writes
  (before and after a write lands), crashes runners (their runs outliving them or not), drops links with
  what was on them, partitions and heals them, lets a runner's wait run out, and retires a runner whose
  reservations the person then releases. After every step: the work actually running fits the room, and
  every running run is in the keeper's books while it is up. After settling (every link healed, every
  message delivered, every hint drained): the keeper's books of each runner, the runner's own, and what it
  runs are the same, and no sync said the counts disagree. Planted shell mistakes, each caught on some of
  64 seeds: release on silence, a run started before its grant, a restart forgetting its survivors, a
  keeper answering before its write lands, Hex admitting against its own boxes alone. Bounded seeds here
  (64 of 2,000 steps with two runners, 32 each with one and with three); ROOT runs the sweeps:
  `hex_sim::capacity::run(seed, runners, steps, None)` over its seeds, which must each return `Ok`.
- ROOT: kiln-ci's simulator with kiln's binding of the holder (`hex_api::capacity::Holder` against
  `capacity::sim::SimView`, and the conformance suite for a keeper kiln hosts), then two real machines,
  once the transport binding lands.

## Open items

- The person's release (CV6) is in the core (`Keeper::forget`); its command on the node, and the log
  event's name (agreed with kiln's Observe owner, CLAUDE.md §10), come with the node's shell.
- A holder's GPU reservations need device grants by identity (CV3).
- The room counts a holder's used memory twice: the host's measured free memory already lacks it, and
  its reservation is held besides. Never an oversubscription, but under-use up to that holder's use,
  until the host can measure a holder's use itself (its cgroup on Linux, its process tree elsewhere).
- The transport binding waits on Hex step 4 (CV9).
