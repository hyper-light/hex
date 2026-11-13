# Decision (proposed 2026-11-12): a microVM is charged its learned use, its envelope the ceiling; a contention rule; budgets per fleet

Joint plan step 14 (kiln plan `01a1cd8e862e7169a074db3d5fa33f3d`, "[Hex + Shards] Learned microVM charge, a
contention rule, and budgets per fleet"), serving AC-T7.4.1 (placement) and AC-T7.5.2 (shares under
contention). Status: the charge by measured use is the person's decision (below); the plan has the research shape
the rule without holding the step up, and ROOT took the rule (C1 to C7) and the interface with step 13 (C8) on
2026-11-12, correcting C1's memory and C2's prior to the forms below. ROOT measures density, fairness and the
violation rates once it is built.

Sources: kiln research `01a1cd8f5b8b77649ccb95ad26875842` (findings cited `f<n>` with kiln's grade; raw texts in
`~/.cache/hex-lead/research/hexres-c14`); Linux `Documentation/admin-guide/cgroup-v2.rst` at `1590cf03`; Shards
`docs/design/architecture.md` at `04abb68d` (D183, D188, D191, D230) and `31b3f106` (D231); Hex's
`scheduler-architecture.md` (B3, B4, B16), `hex-sim-constants.md` and decision 9 (`hex_core::patience`).

## The person's choice

"For 14 - of course go" (the plan header's decision 3): Hex charges a microVM by its measured use, its envelope
the ceiling. Published schedulers and production fleets, from a laptop to multi-datacenter scale, shape the
contention rule and the predictors; they do not hold the step up.

## What is charged today (Hex main `0f28f75e`)

- guest_cost's census (`crates/hex-cli/examples/guest_cost`) measures a VM class once per host class and writes a
  `RuntimeCost` whose floor and ceiling are both the configured envelope; `overhead` is the VMM's own CPU in one
  enforcement window from readiness on (the boot's burst recorded apart), its own memory and its folders' bytes
  (f39 A).
- The node admits a VM at `RuntimeCost::allocate`, the envelope plus that overhead, and the ledger
  (`hex_sim::lb::Runner`, `hex_sim::peer`) books it from admission to deletion; recovery books it again from the
  record. Shards gives a run's microVM every host CPU (D191), so one default VM fills a host's CPU.

## C1. A VM's charge is learned from its windows; its allocation is the ceiling

**The choice.**
- **The window** is the period the node enforces a VM's CPU over, guest_cost's: on Linux `cpu.max`'s period
  (`hex_core::cgroup::CPU_PERIOD_US`); on macOS the managed control's longest cycle, its cycle at Hex's least CPU
  bound (`hex_api::MIN_CPU_MILLI`, the least the learner caps a VM at) on its host class's CPU-response row
  (`MacRuntime::charge_window`), so every VM's rate holds on average over one window. On Windows a job's hard cap holds
  per scheduling interval, which Windows doesn't document (`JOBOBJECT_CPU_RATE_CONTROL_INFORMATION`), so no window
  derives from it yet: the learner there is refused, typed, until ROOT measures the interval on the host. A charge and
  the census's overhead are per the same window.
- **A window's sample** is the VMM's whole CPU over it, which KVM and HVF count with its guest's (guest_cost's
  `Use`): every vCPU's guest time plus the VMM's own. The per-vCPU split is typed-absent
  (`Awaits(GuestCpuTimePerVcpu)`) until Shards' p/guest-cpu-time-v1 and goal 12 put it in `Usage`; the charge
  needs only the sum, and checks the split against it once it comes. Memory is the most the VMM held in the window
  as the census reads it (Linux `memory.stat` anon plus kernel; macOS its physical footprint), its guest's touched
  RAM included; disk is its folders' bytes.
- **The charge**, each of CPU, memory and disk: the nearest-rank p99.9 of its last SEEDABLE windows (of CPU, and of
  memory's and disk's high-water marks), the second largest of 1,000.
- **Ceiling**: each at most its allocation (envelope and VMM overhead). Learning never raises authority (R03).
- **Cold**: until it has SEEDABLE windows its charge is no less than its booking (C2), as its p99.9 is not yet an
  observed sample.
- Use that grows past the charge is the contention rule's (C4, C5), not a margin's.

**Sources.** A per-task percentile, summed per machine, is the per-task half of Google's production peak
predictor (f27 A), taken here at Hex's tail of record: p99.9 is the bar's (`hex-sim-constants.md`, "the quantiles
reported"), and decision 9's census rule makes SEEDABLE the fewest samples whose nearest-rank p99.9 is an observed
sample, not the maximum (`bench/competitive/probes/waits_census.py`). Memory follows its high-water marks, not its
mean (f82 B; Autopilot's memory signal is the peak, f24 A). CPU past its share only throttles and memory past it is
reclaimed or killed (f23 A, f43 A, f44 A), which C4 and C5 handle. With no history every production system charges
the limit (f60 A).

**What would falsify it.** On ROOT's recorded Shards agent traces, a VM's windows past its charge more often than
one in SEEDABLE (the violation rate, which tracks scheduling latency: f28 A). The history is SEEDABLE windows of the
enforcement window, 100 s on Linux and a few seconds on macOS, whose cycle is milliseconds; if it is too short
there, the history becomes the VM's life.

## C2. A new VM is booked at its class's learned prior

**The choice.** A VM's class is its runtime class, its cost profile's id and its ceiling: VMs of one Agentfile
image and size. A new VM is booked at its class's prior: by the same sample rule, the nearest-rank p99.9 of the
class's last SEEDABLE windows of its warmed VMs (each warmed VM's window a sample, as its own charge's is), each
dimension at most the allocation. It falls when the evidence does: an outlier's windows leave the sample as newer
ones come. With no warmed VM of the class on record, its allocation.

**Sources.** Resource Central predicts a new VM from its class, as a subscription's VMs behave alike, 81% accurate
for average CPU (f62 A), and counts a VM it can't predict as using all it was allocated (f29 A); an empirical-Bayes
prior by class is kiln's (f64 C).

**What would falsify it.** New VMs of a class exceeding the prior they were booked at more often than warmed ones
exceed their own charge.

## C3. Bookings: what the ledger holds of each VM, within the fleet's budget

**The choice.** The ledger holds each VM at its booking. Every booking fits the fleet's budget (C7):
`sum(booking) <= budget`, B3's invariant with learned amounts. After each window a booking follows its VM's
charge: down at once, freeing room; up only into room no booking holds. A VM whose charge exceeds its booking for
want of room is **over** by the difference, and it alone bears the contention (C4, C5).

**Sources.** Borg books a task's reservation, which rises fast and decays to measured use, and places work into
what it reclaims (f21 A). Kubernetes evicts first the pods using more than they asked, never one within its ask
while another isn't (f45 A).

## C4. CPU under contention: shares by booking, the ceiling a cap

**The choice.** Each VM's `cpu.weight` is proportional to its booking: the largest booking takes the kernel's top
weight (10,000), each other its share of it rounded up, at least 1. Its `cpu.max` stays its ceiling.

**The guarantee.** The kernel gives each active group "the fraction matching the ratio of its weight against the
sum", work-conserving (cgroup-v2.rst, "Weights"). With `sum(booking) <= capacity`, every VM receives at least
`min(its demand, its booking)` in every window: were one short, every VM still wanting would hold less than its
booking, so less than capacity would be handed out, and the share would rise (weighted water-filling). An over VM
takes only CPU the others leave idle; its neighbours are never starved. Were bookings ever past capacity, each VM
would keep the same fraction of its booking. The weight's range quantizes a share by at most one part in 10,000 of
the largest booking (c).

**The placement.** A weight is a share only among its cgroup's siblings (cgroup-v2.rst, "Weights"), so the
guarantee needs a VM's siblings to be VMs booked by this rule, and their parent to receive the fleet's budget:
- Every Hex microVM's cgroup sits under its fleet's parent, `<node root>/.vm/<node key>/<VM scope>`, which holds VMs
  alone (process and OCI boxes are `<node root>/<box>`).
- The parent's `cpu.max` is the fleet's CPU budget (C7), at least the kernel's least quota. However the VMs' weights
  fall, the fleet together never takes more than its budget.
- The parent's `cpu.weight` is that budget on the machine's scale, `ceil(10,000 × budget / the machine's CPU)`
  (`charge::fleet_weight`): 10,000 is the whole machine, so against a sibling weighed on the same scale (another
  node's fleet, a box weighed by its booking) the fleet receives its budget. `.vm`, the parent of the fleets of the
  nodes sharing one box root, weighs the sum of their budgets on the same scale, each read from its fleet's
  `cpu.max`.
- The parent has no memory bound of its own. Its VMs' `memory.high` budgets already sum to the fleet's budget (C5);
  a parent's `memory.high` would reclaim from VMs within their budgets whenever one passed its own, where C5 holds
  that one alone.

`charge::Node::placement` gives these bounds: the parent's, and each VM's `cpu.weight` by booking, `cpu.max` and
`memory.max` its ceiling, `memory.high` its budget at most its ceiling.

**One scale for every child of the node's cgroup** (ROOT, 2026-11-12). A process or OCI box is booked at its
allocation until it is learned: the native classes offer no runtime cost (`facts::capacity`), so a native box's
receipt books its caps, which its cgroup holds as `cpu.max` and `memory.max` (a reused slot's as its box's create
wrote them). It weighs `ceil(10,000 × booking / machine CPU)` like a fleet, and `.oci` its boxes' weights together.
A box booked nothing weighs 1: it takes what booked work leaves idle. A reuse slot no box holds runs nothing and
keeps the caps its last box left (`Cgroups::give_back` writes no bound): it books nothing and weighs 1, apart from
the scale. The fleets take the units the boxes leave, and a fleet's CPU budget is at most the CPU those units carry,
rounded down (`charge::fleet_room`), besides the room its machine leaves after the boxes' bookings: each box's weight
rounded up comes off the fleet's, never past the scale's top, so the weights of every child that may run sum to at
most 10,000 and each child, box or fleet, receives at least `min(its need, its booking or budget)` under contention
(`charge::tests`: a process box under contention, and the property over any bookings; `node::charges::tests`: the
weights the node writes, shared as the kernel shares them). Its owner's own `cpu_weight` is not the node's scale. The
node writes the boxes' weights each window, so a box made between two windows keeps its owner's weight until the next.

**Sources.** Kubernetes makes a request a CPU weight and the limit a throttle (f35 A, f45 A); Borg gives batch
work tiny shares beside latency-sensitive work (f44 A); weight by charge, the envelope the cap (f83 B). On macOS a box's
CPU follows its weight only as QoS tiers, and the managed control caps (f56 A, f38 A); a Windows job holds a weight of
1 to 9 or a hard cap. Where the platform caps, each VM is capped each window at its water-filled share
(`charge::Node::rates`): its booking, then what the fleet's CPU budget leaves past the bookings, shared by booking over
what each asked past its booking in its last window, one that used its whole cap or ran none yet asking its ceiling.
So each keeps at least its booking whatever it asks next, the caps fit the budget, and CPU the others leave goes to the
VMs asking past their bookings, a window late.

**What would falsify it.** A VM within its booking receiving less than `min(demand, booking)` in a contended window
on hardware (ROOT's fairness run).

## C5. Memory and disk under contention: budgets by booking, the over VM held, ended last

**The choice.**
- **Each VM's budget** is its booking plus a share of the fleet's slack (`budget - sum(booking)`) in proportion to
  its booking, so the budgets sum to the fleet's budget exactly and no two VMs grow into one byte of slack in one
  window. Its room, what D188 tells its guest, is its budget less what it holds. On Linux its VMM's cgroup
  `memory.high` is its budget (throttled past it, never OOM-killed: cgroup-v2.rst, `memory.high`) and `memory.max`
  its ceiling.
- **An over VM** is held at its budget; its booking rises into slack as other VMs' bookings fall or end.
- **Last**, when what the fleet's VMs hold plus the fastest growth on record in one window passes the fleet's
  budget, the node ends the VM holding most past its own budget, lowest band first; never a VM within its budget,
  so never one within its booking, nor one `memory.high` holds at its budget (growth is no fact yet). The fastest
  growth is learned from the node's own windows, only rising. Moving it instead of ending it waits on Shards' D176
  (`Awaits(LiveMove)`): its saved and live moves are the `shards` command's, which shards-api's image run (D231)
  doesn't offer at 66113379, whose D235 plugs a directory into a running member, not the member into a machine.
- **In the node** (`node::charges`), once a window after the placement: the learner kills the chosen VM's cgroup
  through the descriptor its native owner pinned, so its VMM ends as the kernel's OOM killer would end it and is reaped
  as any other end; it records why (`hex_api::LearnedEnd`: what the VM held against its budget, its band, and its
  fleet's held, growth and budget), and ends no other while that VM's VMM still runs, so one end is never two. Its
  state is explicit (`Resort`: `Held`, `Ending`, `Refused`). Where the fleet passes its budget with every VM within its
  own, the node can't act and says so, typed (`Refusal::Exhausted`); a refused kill is kept typed too, and the next
  window asks the rule again. Where it would move the VM rather than end it, the move is refused typed: a Shards VM's
  awaits D176 (`Awaits(LiveMove)`, the end's `unmoved`), a Firecracker VM's would be Hex's own, through Firecracker's
  snapshot, which Hex doesn't drive yet. The why is on the box itself, where the person looks (ROOT, 2026-11-12): the node
  writes it into the VM's record before it ends the VM (`BoxRecord::last_resort`, through the box's record turn and only
  into the record of that VM's lifetime), and the VM's inspect and list carry it (`BoxInfo::last_resort`, last on the
  node's wire), across the node's restarts. Each class's capacity answer carries
  the VMs of the class it ended too, latest first (`Learned::ended`, fitted after the holdings), of the at most the
  machine's VM population it keeps, the oldest forgotten first.
- **Intent first** (ROOT, 2026-11-12: "if the model allows it"; decided so). The learner writes why it ends a VM into
  the VM's record before it ends it (`recorded::charges::LastResort` through `Reach::intend`), so a node that crashes
  between the end and the window's writes still says why on the box. The model allows it on three conditions, each
  held: (1) the end waits on nothing unbounded: the write waits at most the store's record turn, held only by another
  write of that record and bounded by its learned patience (`store.turns`, twice the slowest record change, 3.136 ms
  seeded for a release build: `crates/hex-node/src/store.rs`), and one durable write, within the window whose growth
  the rule's margin covers (a record turn is never a lifecycle turn: those are separate); (2) the end never waits on a
  write that fails: where the intent isn't written (its turn refused, its write failed, the record not that VM's
  lifetime), the VM is ended all the same and why is written after the window, as before, a write that fails kept for
  the next; (3) no record says a VM was ended when it wasn't: an end refused withdraws its intent (written after the
  window, the latest write of a VM replacing any kept before it), and a node that restarts and meets an intent on a VM
  that still runs carries that end out first, as it was decided, before asking the rule again (`LastResort::met`,
  `due`), as a delete marked before a crash is finished by recovery. The VMM may still be dying when the restarted node
  looks, which ending it again only confirms; carrying the end out after a restart acts on the evidence of the window
  that decided it, which is why the rule decides nothing anew for it. The one case where a refused end is carried out
  after a restart is a crash before its withdrawal is written. The cost is one record write before each end, on a path
  that ends at most one VM a window. Not taken: ending first and writing after, as before, which loses the why to a crash
  between the two; writing the intent somewhere of its own (a journal of the learner's), a second record the box's
  inspect would have to read, which the box's own record already is.
- **Bands.** A VM's band is its spec's (`BoxSpec::band`, as its submitter gives it, 0 the lowest; a spec without one
  is band 0, its record and native commitment unchanged), carried whole from the submission to the node's record:
  the scheduler's payload holds the spec, the node's wire carries the band last on a spec, and the learner tracks each
  VM in the band its record names. So the room goes to the highest band first (C3), and the last resort ends the
  lowest band first, of a band the VM most past its budget. The placement's demand doesn't carry it: the spec rides
  every hop whole with it, and placement neither orders nor preempts by band on a live node, so a band there would
  change nothing (R13).

**Sources.** Admit within room, take back cold pages first, then end or move the lowest-priority VM past its
charge, never one within it (f84 B: Kubernetes' order f45 A, ESX f47 A, far memory f49 A); D188's give-back is
Shards' own once a guest has its room (f53 B). A kill loses an agent's context (f58 B, f76 A), so it is last, and a
move replaces it once there is one.

**What would falsify it.** A VM within its booking ended, or held below its booking.

## C6. Admission and back-off

**The choice.** A start is admitted only if its booking (C2) fits the fleet's budget less every booking and less
what VMs hold past their bookings (memory and disk). A refusal carries the room (B3), so the next pick doesn't
repeat it, and the node's word carries its room when it changes: no timer. A VM whose use rises past its charge
raises its booking or holds memory past it, which shrinks the room at once: the node backs off until bookings
fall or VMs end.

**Sources.** B3 (the node admits against its measured room, a refusal carries it), Omega's one rule for a full
machine checked at commit (f68 A), Sparrow's late binding (f69 A).

## C7. Budgets per fleet, and each guest's D188 room

**The choice.** A fleet is the Hex VMs on one machine, the group a budget holds as a Shards slice holds a group of
microVMs (D183, D188). Its budget is the combined view's room for Hex on that machine (step 13): the machine's room
less kiln's reservations and less what neither books. So it follows fleet scale (a laptop's is what the person's
own work and kiln's runs leave; a dedicated machine's is the machine's) and hardware (the machine's facts). Each
VM's budget (C5) is its share; D188 turns it into its guest's room, and everything the VM holds comes out of it,
its D230 ring pins included (pinned guest RAM, so in what it holds). Delivering the room to a Hex-booted guest
waits on Shards: shards-api has no call that sends init's `ROOM` (D231 has `STATS` alone), so Hex refuses it typed
(`Awaits(GuestRoom)`). The pinned share is Shards' own since ring-pins-v1 (D230, vendored at 66113379): an engine
ring's slot is made only where the room init is told covers it past the room the run needs to go on (where it is told
none, the guest's available memory), idle slots go back as a told room shrinks, and the relay that pins them is in the
run's cgroup, so once the room is told the pins are within it (`Upstream::RingPinsWithinRoom`, awaited no more).

**Sources.** The plan's architecture ("Hex sets budgets per fleet, and Shards' D188 tells each guest its room.
Everything a VM holds, ring pins included, comes out of that room"); D188 (f53 B); Borg admits quota by the limit and
places by reservation (f87 B, f21 A). Per-owner budgets across cells are B16's escrowed rights, not this step.

## C8. The interface with step 13 (goal 13)

Taken by ROOT on 2026-11-12 and passed to goal 13; `Allocation`'s booked amount approved; how a VM's learned use
reaches the view settled with goal 13 the same day:
- The room in the machine's view stays physical, the machine's capacity. Nothing shifts it by a learned amount.
- Hex's own local reservation in the view counts each running box at its holding, or at its receipt where it has
  none. A VM's holding is its booking plus the memory and disk it holds past it. A VM that ends leaves the view
  with its release, so no aggregate lags behind an end.
- The node agent learns, so it reports. Its capacity answer for each VM class carries `Capacity::learned`: the
  class's priors, its learned use for placement across nodes (`LearnedUse`: what the receipts booked, what their VMs
  are learned to take), and each running VM's holding by box id. On each refresh the scheduler maps the box ids to
  its keys and calls goal 13's `Driver::rebook` with them, right after it applies the answers.
- The holdings are as fresh as the refreshes, not each window. Between two refreshes a VM growing past its booking
  is held physically by C5's rule: its `memory.high` is its budget. The view lags, but the machine isn't overrun.
- A capacity answer keeps to one message (`hex_core::wire::fit_learned`): the holdings go most past their receipts
  first, and any left out is one least past its receipt, counted at its receipt.
- Hex admits a VM when its booking fits the combined view's free room for Hex, refused typed with that room.
- Hex's own ledger books a VM's booking: `hex_api::Allocation` gains an optional booked amount (append-only, absent
  meaning today's whole allocation) that admission, records and recovery all use.

## How it is wired

- `hex_sim::charge` is the pure core: `Node` learns, books, shares and budgets; `sim` runs it window by window.
- `hex_api::Allocation::booked` is what a receipt booked; `Allocation::booking()` is what the ledger holds, and
  `RuntimeCost::allocate` books it whole. The node socket's wire carries it after the cost, and the peers' text
  receipt as `a2:` (an unbooked one stays `a1:`, so records written before still read).
- `NodeSpec::priors` carries each VM class's learned prior in a node's facts and offers. The placement index books a
  new VM's receipt at its node's prior (`model::booked_at`); a receiver admits an owner's receipt as it was booked,
  comparing charges with the booking aside (`model::same_charge`), so a stale view is never refused for it.
- The learner is the node agent's (`hex-node`'s `node::charges`), the one authority, as the owner of its VMs'
  cgroups. A monotonic timer wakes it once a window, `cpu.max`'s period. It reads each running VM's cgroup (the
  supervisor's `running_vms`), learns, and writes the placement (C4, C5), each file only when its value changes
  (`BoxCgroup::set`; a VM's create already wrote its ceiling): each VM's `cpu.weight` and `memory.high`, the fleet's
  parent's `cpu.max` and `cpu.weight`, `.vm`'s `cpu.weight`, and each process and OCI box's and `.oci`'s `cpu.weight`
  on the same scale (C4). Each Shards guest's room is refused `Awaits(GuestRoom)` (C7). A failed window is kept, and the
  capacity answers then carry nothing learned, so the class's VMs are booked whole until a window passes.
- `hex_api::Capacity::learned` crosses the node socket's wire last on a VM class's capacity answer (append-only).
  Its priors are bounded by a class's profiles, its holdings by a node's boxes, and the answer is fitted to one
  message. On each refresh the scheduler (`scheduler::charges`) gives its node's facts the priors it carries, then,
  right after applying the answers, maps the holdings' box ids to its keys (`Driver::running_ids`) and books each
  VM at its holding (goal 13's `Driver::rebook`). Its capacity stays the machine's (C8).
- On macOS and Windows the learner is `hex_node::recorded::charges`, started by `Recorded::start_charges` (hex-cli
  on both): a thread that waits on the node's end beside the window, and once a window reads each running managed
  VM's usage through its native owner (`Recover::managed_usage`), learns as Linux's does, and caps each VM at its
  water-filled share (`Recover::set_managed_rate`): on macOS a native-helper `rate` request (kind 0x0014, the scope's
  whole key and its rate) sets its control's rate, its debt kept and what came before charged at the old rate; on
  Windows the job's hard cap, at least the machine's least. Its capacity answers carry what it learned
  (`recorded::charges::learned`, shared with Linux's). Neither platform holds a VM's memory at a budget below its
  ceiling (macOS ends past a footprint limit, Windows refuses past a commit limit, neither throttles at a level), so
  each VM's memory budget is refused, typed (`Platform`, `memory_high`), which the node says as it starts. On Windows
  the learner is refused, typed, until its window is measured (C1). Their last resort is Linux's, in the state every
  learner shares (`recorded::charges::LastResort`: one end at a time, why kept for the class's capacity answers and the
  box's record), and ends the VM it chooses through its managed receipt (`Recover::stop_managed`): on macOS the native
  helper's stop of the VM's scope, the stop its receipt sends; on Windows the termination of the job the node holds, as
  its receipt's stop terminates it. Why goes into the VM's record before its end (intent first, C5), its end is then
  observed as any other, and its inspect says it (`Books::counted`), as Linux's does.

## Every number

| Number | Value | Kind | Derivation | Pinned by |
|---|---|---|---|---|
| The window | Linux 100 ms; macOS the control's cycle at Hex's least CPU bound; Windows none until measured | (c) Linux's `cpu.max` default period, Hex's; (b) macOS's CPU-response row | guest_cost's window; macOS's longest cycle at any rate the learner sets | `charge::tests`, `macos::cpu::tests` |
| A capped VM's least | `hex_api::MIN_CPU_MILLI` | (c) | Hex's least CPU bound, the kernel's least quota | `charge::tests` |
| SEEDABLE | 1,000 | (b) rule | the fewest samples whose nearest-rank p99.9 is observed (decision 9) | doc-truth test against `waits_census.py` |
| The charge's tail | p99.9 of CPU and of memory's and disk's high-water marks | requirement | the bar's tail (`hex-sim-constants.md`) | `charge::tests` |
| The ceiling | the allocation | (b) | guest_cost's census of the class | `charge::tests` |
| The cold booking | the class's prior, else the allocation | (a) | p99.9 of the class's last SEEDABLE warmed windows | `charge::tests` |
| `cpu.weight`'s range | 1 to 10,000 | (c) | cgroup-v2.rst | `charge::tests` |
| The fleet's weight | `ceil(10,000 × budget / machine CPU)`, at least 1 | derived | its budget's share of the machine on the kernel's range | `charge::tests` (exact and property) |
| The fleet's CPU cap | its CPU budget, at least 10 milli-CPU | (c) | the kernel's least quota, 1 ms a 100 ms period (`hex_api::MIN_CPU_MILLI`) | `charge::tests` |
| A box's weight | `ceil(10,000 × booking / machine CPU)`, its booking its caps; an idle slot 1 | derived | its booking's share of the machine on the kernel's range | `node::charges::tests` |
| The fleet's CPU budget | at most the CPU the units the boxes leave carry, rounded down | derived | each box's weight rounded up comes off the fleet's, so the children's weights stay within the top | `charge::tests` (exact and property), `node::charges::tests` |
| The slack's split | in proportion to bookings | derived | budgets sum to the budget, no race in a window | `charge::tests` |
| The fastest growth | learned per window | (a) | the node's own windows, only rising | `charge::tests` |
| The eviction order | band, then excess | rule, cited | Kubernetes' documented order (f45 A) | `charge::tests` |
| The ends a node keeps | its machine's VM population, at most a node's boxes | derived | a machine never holds more VMs at once | `node::charges::tests` |

## Alternatives not taken

- **Borg's reservation decay and margin, Autopilot's half-lives and 10-15% margin, ESX's 75% idle tax, VPA's
  multipliers** (f21, f24, f46, f63): each a picked number (f86 B, R03).
- **Pooling CPU across a node's VMs** (the N-sigma half of Google's predictor; Cohen's chance constraint, f27 A,
  f31 A, f32 A): the gain is real for uncorrelated VMs (f33 A, f71 B), but a Gaussian tail is optimistic (Google
  took 5 sigma for an acceptable violation rate where a Gaussian puts the 99th percentile at 3: f27's source, §5.3),
  distribution-free bounds (Hoeffding's, f32) are void when one VM's range is the host, and one owner's VMs behave as
  one (f33). Taken once ROOT measures the violation rate on recorded traces (f28, f85).
- **Charging CPU at its mean** (Autopilot's batch limit, f24): packs bursts that collide; a burst could then get only
  its mean.
- **Memory overcommit by pooling peaks** (f71, f84): needs f85's four measurements first.
- **A history of hours** (f61): a picked length; SEEDABLE windows are derived.

## Boundaries (typed, never a stubbed success)

| What | Waits on | Hex meanwhile |
|---|---|---|
| per-vCPU guest time | Shards p/guest-cpu-time-v1, goal 12's `Usage` | charges the VMM's whole CPU; `Awaits(GuestCpuTimePerVcpu)` |
| a Hex-booted guest told its room | a shards-api call for init's `ROOM` | Linux holds the VMM at `memory.high`; `Awaits(GuestRoom)` |
| moving an over VM instead of ending it | Shards D176 in shards-api's image run (66113379 has none; D235 plugs directories) | ends it, last; `Awaits(LiveMove)` |
| class priors across the fleet | step 13's view, hyper-swim's gossip (f67 A) | each node's own record |

## Measured in hex-sim, and what ROOT measures

hex-sim (`hex_sim::charge::sim`) runs nodes window by window: VMs with an agent's use, admitted and booked by this
rule, CPU shared by water-filling on bookings (the kernel's weights), memory budgets and the last resort as above.
Its VMs are drawn from the published agent studies: tool time 4.8% to 28.5% of a session (f73 A, f75 A), bursts
of 1 to 2 s (f75), a core's tenth on average (f75, f78 A), memory peaks of 0.8 to 4 GB (f74 A, f75). These are the
generator's input space, as `generate.rs`'s draws are, until ROOT's recorded Shards traces replace them. It
computes, per seed:
- **density**: VMs admitted per node before the first refusal for room, against today's envelope charge;
- **fairness**: the least, over windows and VMs, of what a VM received over `min(demand, booking)` (at least 1 by
  C4), and Jain's index of VMs' shares in contended windows;
- **the riser**: a VM whose use rises past its charge: its neighbours' guarantees kept, the node refusing new VMs
  while short, only it held or ended.

ROOT measures on hardware: density and fairness under this rule with real harness VMs (Linux on 00aad, the Mac
Studio), the CPU violation rate per OS on recorded agent traces, and f85's memory measurements before memory pools.
The driver is hex-cli's `charge_fleet` (ignored, ROOT's run): a real `hex node` and a fleet of VMs of one class
(`shards-image`, Agentfile images, Linux, macOS and Windows awaiting `RunHostStdioOnly`; `shards`, Shards guests whose
box plays the trace; `firecracker`, Linux), each replaying one recorded trace (`millis threads mib` steps, played by
the guest's shell) and deleted once it has played, the first admitted the riser (its trace until its charge has
warmed, SEEDABLE windows, then every vCPU and the most any trace holds, to the run's end), beside a process box asking
its whole cap. It admits as the scheduler's ledger books (each VM at its class's prior on the node's capacity answer,
else its receipt; C2, C6), a start retried only once the room has changed, and reads each window, the node's learner's
as the node says it on stderr (`the charge learner's window is N ns`, or why it doesn't run, which refuses the run),
each read at least a window after the last, each VM's CPU given (its usage over its own span), asked (its trace's over
the same span) and booked (its holding). `hex_sim::charge::trial` reckons the windows as `sim` reckons its own: ratios
of given over min(asked, booked) and misses in the contended windows (C4's falsifier's), Jain's index, the riser's
windows over its booking, its neighbours' and the box's misses while it rises, and each VM's windows past its charge
(C1's falsifier); with the most VMs at once against envelope density, the starts refused while the riser rises, the
riser's inspect (its exit and the last resort's why), and the learner's own CPU (Linux: its `hex-charges` thread;
macOS and Windows: the node's whole CPU, an upper bound). Its windows go to `windows.tsv`; its figures are a record,
not a verdict.
