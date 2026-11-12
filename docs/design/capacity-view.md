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
  Hex node; and `capacity::conformance::view` (HEX-CV-1 to 10), which the simulator and the node's own
  view both pass, and a keeper kiln hosts would.
- The node's view (`crates/hex-node/src/placement/view.rs`): the keeper's shell in the placement
  `Driver`, under the lock its own admissions take. Its records are a folder of their own in the
  node's record files; its room is the node's facts' capacity (`Driver::refresh_node`); the node
  admits its own boxes against that room less every holder's reservations, given to its admission at
  once whenever they change; and `local` is what the node holds itself (`peer::Node::held`: what its
  ledger and admission index book, the larger per dimension, which charges backings too, each box the
  learner rebooked counted at its holding instead of its receipt).
- hex-sim scenarios: crashes on either side, dropped links, partitions and heals.
- The lane over Hex's QUIC transport (CV9): `hex_core::capacity`'s frames on `CAPACITY_PORT`,
  `hex_quic::capacity::Server` serving a keeper (the node's as `placement::DriverView`) and
  `hex_quic::capacity::Link`, a holder's end.

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
holder's reserve is checked by the keeper against it. Each check is made where it commits, under one hold of
the host's lock: the node re-derives its ledger's room and whether the view is over at each admission's commit,
never before a backend call it makes with the lock let go, where a holder's reserve or sync, or a rebook,
could move the room unseen (`placement::view::race_tests`). The charge is what each side's admission charges:
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
sum) is returned for the shell to log on the node's capacity event target: the person says
`hex schedule forget --socket <path> --holder <name>` (`schedule::Command::Forget`, its own wait, a typed
`Reply::forget`), and the scheduler logs `hex.capacity event=forgotten holder="<name>" epoch= count= cpu_milli=
mem_bytes= disk_bytes= gpus=`. Its epoch stays, so a process
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

### CV9. Carried over hyper-transport, on a lane of its own

A holder reaches its keeper over Hex's QUIC transport (Hex step 4, `hex-quic.md`): TLS 1.3 with
`TLS13_AES_256_GCM_SHA384` alone, both ends asserting it, each peer named by the certificate kiln's CA
issued it by CSR. The view's messages are not variants of the node's wire: they have a port of their own,
`hex_core::lanes::CAPACITY_PORT` (`0x4845_5805`, the port after the log's, of the control class: small
messages, each answered), so a holder that speaks only this protocol needs none of the node agent's.
- **Framing** (`hex_core::capacity`): each `ToKeeper` and `ToHolder` is one JSON message framed by its
  length (`hex_core::stream`'s 4-byte big-endian prefix), at most the wire's message bound (128 KiB).
- **A connection is a link**: the holder says its messages in order and the keeper answers each in
  order; the hints owed to a link come on it between answers. When the connection ends the holder is
  away (CV5); a connection that breaks the framing or says a message that isn't one is reset as breaking
  the protocol.
- **A sync fits one message**: the keeper holds each holder to the most reservations one sync lists at
  their widest (`hex_core::capacity::most_held`), beside the node's own record bound (CV8).
- **The shells**: `hex_quic::capacity::Server` serves any `CapacityView` on the port's acceptor, a thread
  a link within the endpoint's connection bound, to the holders its host authorizes alone, each by the name its
  certificate carries (CV8: any other word is closed `NotAuthorized` and logged); it delivers each hint to the
  link it is owed to after each message and when its host calls `Server::hint`; `hex_quic::capacity::Link` is a
  holder's end.
- **In production**: `hex schedule serve` serves the node's keeper as `placement::DriverView` where its
  configuration names a lane (`capacity`: `bind`, `credentials`, the enrollment folder `hex node enroll` began and
  kiln's CA's answer completed, read whole before anything binds, and `holders`, each `name` and `user`). The view
  opens before any admission the scheduler makes, its endpoint serves `CAPACITY_PORT` alone, two connections a
  holder (one, and the one replacing it: `hex-quic.md` Q8), and every command it takes ends with the hints owed.
  It says `capacity <address>` before `ready`. Without a lane it keeps no view and binds nothing for one, and a
  release is refused, typed.

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

- **Lag**: the view counts a VM's growth past its holding from the next refresh that rebooks it, so it
  lags that growth by up to one refresh; goal 14's contention rule holds the growth physically meanwhile
  (the VM is held at its budget), so the lag is never a double booking of room the view granted.

### CV12. A holder's memory is counted once: its scope, read with the free memory

**The defect.** The room is measured: free memory plus what Hex's own boxes use (`Facts::node_with_owned`). What a
holder's work uses is missing from that free memory, and the holder's reservation is held besides, so each byte its
work uses was counted twice: never an oversubscription, but room lost up to the holder's use
(`placement::view::credit_tests`, red before this decision).

**The choice** (ROOT, 2026-11-12, Option A with its conditions). A holder's hello may name where its reserved work
runs, a `Scope` (an optional field, append-only): a cgroup v2 directory on Linux, a process group on macOS, a job
object on Windows. The host reads each synced holder's scope in the same sample as its free memory, once on either
side of that read, and keeps the smaller (`hex_node::scopes::around`): a use that moved while free memory was read
is credited at its least. The room every admission is decided in, the keeper's and the node's ledger alike, is the
measured room plus each synced holder's credit, `min(use, its reservations)` (`Keeper::credit`). Use past a holder's
reservations stays counted against the room. The credit is the sample's and the session's: a holder's next sample
replaces it, a hello replaces its session's, and a dropped link ends it at once (the node's admission given the room
without it before the lock is let go). The room and the credit of one sample are set together, under the lock
every admission commits under (`Driver::refresh_node_sampled`).

**Ownership.** A scope earns credit only once proved its holder's user's, the user the person declares for that
holder in the host's configuration (a uid, or a SID on Windows; nothing the holder says can name it):
- Linux: some directory from the cgroup up to the hierarchy's root, the root not counted, is owned by the uid: a tree
  the user owns, or one delegated to it (the kernel's `cgroup-v2.rst`, "Delegation": the delegatee is given the
  subtree's directory and its `cgroup.procs`, `cgroup.threads` and `cgroup.subtree_control`). Its use is its
  `memory.current` ("the total amount of memory currently being used by the cgroup and its descendants").
- macOS: every member of the process group runs as the uid (`proc_pidinfo(PROC_PIDTBSDINFO)`'s `pbi_uid`); its use is
  their footprints summed (`proc_pid_rusage`'s `ri_phys_footprint`, what the node already reads of its own boxes).
- Windows: the job's owner, read from its security descriptor (`GetKernelObjectSecurity`,
  `GetSecurityDescriptorOwner`), is the declared SID, compared byte for byte; its use is its processes' private
  working sets summed (`PROCESS_MEMORY_COUNTERS_EX2.PrivateWorkingSetSize`), which the machine's available physical
  memory (`ullAvailPhys`, the node's free memory there) lacks, shared pages not counted, so never more than the
  work takes of it. An elevated runner makes jobs that BUILTIN\Administrators owns (an elevated token's default
  owner): such a job fails the proof against the runner's own SID and earns no credit, the safe outcome
  (`scopes::tests::cv12_a_job_is_its_holders_only_when_its_owner_is_the_declared_sid`).
A scope the host can't prove or read earns nothing, and the holder's session is closed, typed, with the hints:
`Close::Scope(NoUser | NotOwned | Unreadable | Invalid | NotThisPlatform)`; its holder drops the scope and may say
hello again without it. Ownership doesn't widen what a holder could already do by releasing early; it keeps a
mistaken scope from crediting someone else's memory.

**Alternatives.**
- *The holder reports its own use* (no platform code): rejected. A report goes stale when the use drops; credited
  then, it lets others into the holder's reserved slack, which the work grows back into.
- *The holder's whole process tree, or its runner's cgroup*: credits the runner's own memory, which no reservation
  holds, so others admit into it.
- *Crediting the whole reservation*: free room the work hasn't used yet becomes others' while it is still promised.

**What would falsify it.** On the two-machine run, a holder whose work uses `U` under reservations `R >= U`, its use
moving between samples: the free room the view says ever exceeding the room minus everything held at the larger of
its use and its reservation, or a sample's credit exceeding the use read on either side of its free memory.

## The protocol

**Holder to keeper** (`ToKeeper`): `Hello { holder, epoch, scope? }` (CV12); `Sync { fence, held: [(id, amount)] }`;
`Reserve { fence, id, amount }`; `Release { fence, id }`.

**Keeper to holder** (`ToHolder`): `Welcome { fence, view }`; `Synced { fence, view, count, sum,
released, adopted, disagreed }`; `Granted { fence, id, amount, view }`; `Refused { fence, id, why,
view }` (`why`: `Room`, `Full`, `Empty`, `Devices`, `Disagrees`); `Released { fence, id, view }`;
`Hint { fence, view }`; `Closed { rejected, why }` (`rejected`: the fence of the message closed, none for a
hello; `why`: `OlderEpoch`, `Session`, `NotSynced`, `HoldersFull`, `Invalid`, `Scope` (CV12), `NotAuthorized`
(CV8, CV9)).

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

## What kiln's runner implements as a holder on `CAPACITY_PORT`

kiln's runner holds room on a machine whose Hex scheduler keeps the view (CV1). This is the whole of its side: the
wire, the order of words and what each answer asks of it. The order itself is `hex_api::capacity::Holder`'s, a pure
core kiln drives through hex-api (Shards R12: one implementation), feeding it what comes and saying what it sends.

1. **Identity.** Enroll with kiln's CA by CSR (`hex_quic::enroll`: its key made where it runs and never sent). Its
   certificate's DNS name is its holder name: the keeper hears only the names its configuration lists
   (`capacity.holders[].name` in `hex schedule serve`'s configuration), each only as itself; any other word is
   answered `Closed { NotAuthorized }` and logged (`hex.capacity event=refused`).
2. **The link.** Dial the keeper's endpoint (the configured `capacity.bind`, which `hex schedule serve` prints as
   `capacity <address>` before `ready`), checking the certificate for the scheduler's name, and assert
   `TLS13_AES_256_GCM_SHA384` negotiated. Open one connection to `CAPACITY_PORT` (`0x4845_5805`): a connection is
   a link (CV9); its fences are good on it alone (`Close::Session` on another), and a hello on a new connection
   supersedes the old session.
3. **Framing.** Each message is a 4-byte big-endian length, then the JSON of one `hex_api::capacity::ToKeeper`
   (holder to keeper) or `ToHolder` (keeper to holder) in its serde form, at most `hex_core::wire::MAX_MESSAGE`
   (128 KiB): `hex_core::capacity::{encode, decode}`. Bytes that aren't one reset the connection (`Why::Protocol`).
   The protocol only appends: a variant or optional field it doesn't know is a newer keeper's.
4. **The order of words** (CV4, CV5):
   - `Hello { holder, epoch, scope? }`: its certificate's name, and an epoch one more at every start of its process,
     kept durably;
   - on `Welcome`, `Sync { fence, held }` listing every reservation whose work may still run, of any of its epochs;
     on `Synced`, its own count and sum must be the keeper's (`Event::Synced { agree }`), and where they aren't, the
     keeper's books are the machine's and it reports the difference;
   - only then `Reserve { fence, id, amount }`, `id` its epoch and a count never reused, and the work starts on
     `Granted` for that id alone; a reserve with no answer within its learned wait (hex-core's Patience, CV6) is
     abandoned: never started, and released at once where the link is up;
   - `Release { fence, id }` once that work can no longer run.
5. **What each answer asks.** `Refused { why }`: `Room` (decide again on the view it carries), `Full` (it holds the
   keeper's bound), `Empty`, `Devices` (a GPU by count, refused until device grants by identity), `Disagrees`.
   `Released`: its release is done (said for an id never held too: releasing is idempotent, CV5). `Hint { view }`:
   the machine changed (CV7): fold it into kiln's own ledger (its budget and `outside`). `Closed`:
   `OlderEpoch` (a newer process of it is there: this one admits nothing more here), `Session` or `NotSynced` (say
   hello again), `HoldersFull` or `NotAuthorized` (this keeper doesn't serve it: admit nothing here and tell the
   person), `Scope(_)` (say hello again without the scope; the core drops it), `Invalid(_)` (its own defect).
6. **Down.** With no synced session to a keeper it knows of (CV10), it admits nothing new on that machine (CV11);
   work already started runs on, held in the keeper's ledger, and its next sync says what still runs.
7. **Its scope** (CV12, optional): where its reserved work runs and nothing else of it, its own process not
   included: Linux, a cgroup v2 path relative to `/sys/fs/cgroup` under a tree its user owns or was delegated
   (`kiln-jobs-<pid>`, the parent of its jobs' cgroups); macOS, one process group whose members all run as its
   user; Windows, a job object it names, owned by its account (an elevated runner's jobs are BUILTIN\Administrators'
   and earn nothing). The person declares that user beside its name (`capacity.holders[].user`: a uid, or a SID).
8. **Gone for good.** The person releases what it held with `hex schedule forget --socket <scheduler socket>
   --holder <name>` (CV6), refused while it is connected, logged as `hex.capacity event=forgotten`.

## The interface with goal 14 (learned charge)

Goal 14 owns what a VM is charged, its booking and the contention rule (`learned-charge.md`); this step
owns the room and the combined view. Agreed through ROOT (2026-11-12, `learned-charge.md` C6-C8):
- **The room is physical.** The keeper's room is the machine's capacity as the node's facts measure it;
  no learned amount moves it.
- **What Hex holds is per box.** The keeper's `local` is `peer::Node::held`: each box the node runs at
  its holding where the learner said one, else at its receipt (`Allocation::booking`, the whole allocation
  where it books none). A holding is the VM's learned booking plus the memory and disk it holds past it.
  The node agent reports each running VM's holding by box id in its capacity answers; on each refresh the
  scheduler maps box ids to keys and calls `Driver::rebook(&[(Key, CapacityResources)])` right after
  applying the answers, replacing what was said before. A box that ends leaves `held` at the moment its
  room is released, so the view never counts an ended VM's learned share, nor misses its receipt's.
- **Hex's own admission** is its ledger's, which books each box at its receipt: the Driver gives it the
  room `physical - foreign + (receipts - holdings)` (`admission_spec`), so its free room is the view's free
  room for Hex, and recomputes it before each admission it hears, so a box that ended between refreshes
  takes its credit with it. Where holdings pass the room (a VM holding memory past its booking), the
  ledger can't be given less than it books: the node admits nothing of its own until they fit, refused
  typed (`local admission`), and every holder is owed the view (CV7), so both sides back off.
- **Hex's budget** on a machine (goal 14's fleet budget, each guest's D188 room) is the view's room for
  Hex, `Keeper::room_for_local()`: the machine's room less every holder's reservations.
- **A holder's reservation** keeps its amount for its life; a holder releases it and reserves anew to
  change it.

Goal 14 adds an optional `Allocation.booked` to Hex's runtime wire (append-only, absent meaning the whole
allocation); the capacity protocol carries amounts only and is unchanged by it. Aggregates of learned use
on the capacity wire serve the scheduler's placement across nodes only: summed, they lag a VM's end by a
window, which would under-count Hex here.

## Tests

- Unit tests of both cores (`hex-api`, `capacity::tests`): each transition, each refusal, each fence,
  sync's three outcomes (released, adopted, disagreed), idempotent release, the person's release, the
  keeper crashed at every write of a conversation before and after the write lands, hostile record text.
- The conformance suite (`capacity::conformance::view`, HEX-CV-1 to 10) against the simulator, and
  against the node's view over its record files, restarted from them (`hex-node`,
  `placement::view::tests`); and the node's own admission and a holder's reserve checking one view
  through the placement `Driver`.
- hex-sim (`hex_sim::capacity`): one machine of sixteen CPUs and 64 GiB, Hex's own boxes and kiln's
  runners on its keeper, under a nemesis that crashes the keeper between messages and at its writes
  (before and after a write lands), crashes runners (their runs outliving them or not), drops links with
  what was on them, partitions and heals them, lets a runner's wait run out, retires a runner whose
  reservations the person then releases, and has the person release a runner wrongly: one away (its link
  down, or its process crashed with its runs outliving it) with work still running (CV6). The room is measured
  as the node measures it (CV12): every CPU, and the memory that is free or Hex's own boxes', which lacks what
  each runner's process and its runs use. Half the runners name their runs' scope; each run uses memory within
  its reservation, moved by the nemesis between samples; the host's sample reads each synced runner's scope on
  either side of its read of free memory, keeps the smaller, and gives the keeper the room and the reads together;
  one run of a runner it reads may move once in the window, before the free memory's read or after it; and the OS
  now and then won't read a scope, which closes its runner's session. After every step:
  every running run is in the keeper's books while it is up, but one so released, until its runner's next
  sync adopts it (CV5); the work running fits the room but for what such a release hid and its sync then
  adopted; what the keeper books past the room (only an adoption's), its view says it is over by; and the
  keeper credits each scoped runner at most what its work used when free memory was last read, up to its
  reservations (CV12). After
  settling (every link healed, every message delivered, every hint drained): the keeper's books of each
  runner, the runner's own, and what it runs are the same, and no sync said the counts disagree. Planted
  shell mistakes, each caught on some of 64 seeds: release on silence, a run started before its grant, a
  restart forgetting its survivors, a keeper answering before its write lands, Hex admitting against its own
  boxes alone, a sync that drops what the keeper lacks instead of adopting it; and CV12's: a host keeping the
  larger of a scope's two reads, a host crediting whole reservations, a scope holding its runner's own process.
  Bounded seeds here (64 of
  2,000 steps with two runners, 32 each with one and with three); ROOT's sweeps take
  `capacity::tests::the_record_sweep` (ignored; `HEX_CAPACITY_SEEDS`, `HEX_CAPACITY_STEPS`), every seed
  of which must hold, and each mistake caught on some.
- ROOT: kiln-ci's simulator with kiln's binding of the holder (`hex_api::capacity::Holder` against
  `capacity::sim::SimView`, and the conformance suite for a keeper kiln hosts), then two real machines,
  once the transport binding lands.

## Open items

- A holder's GPU reservations need device grants by identity (CV3).
- The lane's log events (`hex.capacity event=refused`, `forgotten`, `lane_ended`, `hint_failed`) are to be agreed
  with kiln's Observe owner (CLAUDE.md §10).
- The transport admits any peer kiln's CA enrolled: one the lane doesn't name is refused there (CV8) but holds one
  of the lane's connections while it stays. Admitting by name at the endpoint is hex-quic's (`Directory`).
- A scheduler with no runtime class keeps no holder reservation: CV8 bounds a holder's by the node's own record
  bound, which such a node derives as none.
- The credit's window (CV12): a scope read on either side of the free memory's read, the smaller kept, is at most
  what that read lacked when the scope's use moved one way in between. A use that falls and rises again inside the
  window is credited at its high, the fall too much, until the next sample. The window today is the scheduler's
  whole facts read (`runtime_observations`: its backend's physical report, then the machine's facts), not the free
  memory's read alone; hex-sim moves one run once a window, the case CV12 claims.
- kiln's runner implements its side (the section above); until it does, the lane serves no holder in production.
