# Decision (proposed 2026-11-12): a microVM is charged its learned use, its envelope the ceiling; a contention rule; budgets per fleet

Joint plan step 14 (kiln plan `01a1cd8e862e7169a074db3d5fa33f3d`, "[Hex + Shards] Learned microVM charge, a
contention rule, and budgets per fleet"), serving AC-T7.4.1 (placement) and AC-T7.5.2 (shares under
contention). Status: the charge by measured use is the person's decision; the contention rule (C4 to C6) and
the admission interface with step 13 (C8) are proposed for ROOT and, where it needs her choice, the person.

Sources: kiln research `01a1cd8f5b8b77649ccb95ad26875842` (findings cited `f<n>` with kiln's grade; raw texts in
`~/.cache/hex-lead/research/hexres-c14`); Linux `Documentation/admin-guide/cgroup-v2.rst` at `1590cf03`; Shards
`docs/design/architecture.md` at `04abb68d` (D183, D188, D191, D230) and `31b3f106` (D231); Hex's
`scheduler-architecture.md` (B3, B4, B16), `hex-sim-constants.md` and decision 9 (`hex_core::patience`).

## The person's choice

"For 14 - of course go" (the plan header's decision 3): Hex charges a microVM by its measured use, its envelope
the ceiling.

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
  (`hex_core::cgroup::CPU_PERIOD_US`), on macOS the managed control's cycle at the VM's rate (its host class's
  CPU-response row). A charge and the census's overhead are per the same window.
- **A window's sample** is the VMM's whole CPU over it, which KVM and HVF count with its guest's (guest_cost's
  `Use`): every vCPU's guest time plus the VMM's own. The per-vCPU split is typed-absent
  (`Awaits(GuestCpuTimePerVcpu)`) until Shards' p/guest-cpu-time-v1 and goal 12 put it in `Usage`; the charge
  needs only the sum, and checks the split against it once it comes. Memory is what the VMM holds as the census
  reads it (Linux `memory.stat` anon plus kernel; macOS its physical footprint), its guest's touched RAM
  included; disk is its folders' bytes.
- **CPU charge**: the nearest-rank p99.9 of its last SEEDABLE windows' CPU, the second largest of 1,000.
- **Memory and disk charge**: `INPUT_MARGIN` (2) times the most it has held.
- **Ceiling**: each at most its allocation (envelope and VMM overhead). Learning never raises authority (R03).
- **Cold**: until it has SEEDABLE windows its charge is no less than its booking (C2), as its p99.9 is not yet an
  observed sample.

**Sources.** CPU past its share only throttles; memory past it is killed (f23 A, f43 A, f44 A), so CPU takes a
tail of its use and memory a margin over its peak (f82 B). A per-task percentile, summed per machine, is the
per-task half of Google's production peak predictor (f27 A), taken here at Hex's tail of record: p99.9 is the
bar's (`hex-sim-constants.md`, "the quantiles reported"), and decision 9's census rule makes SEEDABLE the fewest
samples whose nearest-rank p99.9 is an observed sample, not the maximum (`bench/competitive/probes/waits_census.py`).
The memory margin is the lead's input-bound rule (`hex_sim::census::INPUT_MARGIN`); one agent task's runs vary
1.8x (f76 A), inside it. With no history every production system charges the limit (f60 A).

**What would falsify it.** On ROOT's recorded Shards agent traces, a VM's windows past its CPU charge more often
than one in SEEDABLE (the violation rate, which tracks scheduling latency: f28 A). The history is SEEDABLE windows
of the enforcement window, 100 s on Linux and a few seconds on macOS, whose cycle is milliseconds; if it is too
short there, the history becomes the VM's life. Or a VM's memory past twice its peak within one session.

## C2. A new VM is booked at its class's learned prior

**The choice.** A VM's class is its runtime class, its cost profile's id and its ceiling: VMs of one Agentfile
image and size. A new VM is booked at its class's prior: the most any VM of the class was charged on this node's
record once it had SEEDABLE windows, only rising, as `Patience` learns a wait; with none on record, its allocation.

**Sources.** Resource Central predicts a new VM from its class, as a subscription's VMs behave alike, 81% accurate
for average CPU (f62 A), and counts a VM it can't predict as using all it was allocated (f29 A); an empirical-Bayes
prior by class is kiln's (f64 C). Only rising is decision 9's rule for a learned bound.

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

**Sources.** Kubernetes makes a request a CPU weight and the limit a throttle (f35 A, f45 A); Borg gives batch
work tiny shares beside latency-sensitive work (f44 A); weight by charge, the envelope the cap (f83 B). On macOS a box's CPU follows its weight only as QoS tiers, and the
managed control caps (f56 A, f38 A): the same shares there are the control's rate set to the water-filled share each
window, from the same pure function (`hex_sim::charge::shares`).

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
  budget, the node ends the over VM with the largest excess past its booking, lowest band first; never a VM within
  its booking. The fastest growth is learned from the node's own windows, only rising. Moving it instead of ending it
  waits on Shards' D176 (`Awaits(LiveMove)`).

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
(`Awaits(GuestRoom)`); the pinned share waits on p/ring-pins-v1 (`Awaits(RingPinsWithinRoom)`).

**Sources.** The plan's architecture ("Hex sets budgets per fleet, and Shards' D188 tells each guest its room.
Everything a VM holds, ring pins included, comes out of that room"); D188 (f53 B); Borg admits quota by the limit and
places by reservation (f87 B, f21 A). Per-owner budgets across cells are B16's escrowed rights, not this step.

## C8. The interface with step 13 (goal 13)

Proposed:
- Hex publishes its reservation on a machine: per dimension, `sum(booking)`, and for memory and disk the excess held
  past bookings too (`hex_sim::charge::Node::reservation`).
- Hex admits a VM when its booking fits the combined view's free room for Hex (`Node::admit(.., free)`), refused
  typed with that room.
- The view carries Hex's reservation as a value that rises without an admission (a booking rising, memory held past
  a booking) and tells kiln's balancer when it changes, so both back off.
- Hex's own ledger books a VM's booking: `hex_api::Allocation` gains an optional booked amount (append-only, absent
  meaning today's whole allocation) that admission, records and recovery all use, and the node's facts reconcile the
  learned bookings since admission each refresh, as they reconcile measured use today (`Facts::node_with_owned`).

## Every number

| Number | Value | Kind | Derivation | Pinned by |
|---|---|---|---|---|
| The window | Linux 100 ms; macOS the control's cycle | (c) Linux's `cpu.max` default period, Hex's; (b) macOS's CPU-response row | guest_cost's window | `charge::tests` |
| SEEDABLE | 1,000 | (b) rule | the fewest samples whose nearest-rank p99.9 is observed (decision 9) | doc-truth test against `waits_census.py` |
| The CPU tail | p99.9 | requirement | the bar's tail (`hex-sim-constants.md`) | `charge::tests` |
| The memory and disk margin | 2 | rule | the lead's input-bound rule | `census::INPUT_MARGIN` |
| The ceiling | the allocation | (b) | guest_cost's census of the class | `charge::tests` |
| The cold booking | the class's prior, else the allocation | (a) | most a warmed VM of the class was charged | `charge::tests` |
| `cpu.weight`'s range | 1 to 10,000 | (c) | cgroup-v2.rst | `charge::tests` |
| The slack's split | in proportion to bookings | derived | budgets sum to the budget, no race in a window | `charge::tests` |
| The fastest growth | learned per window | (a) | the node's own windows, only rising | `charge::tests` |
| The eviction order | band, then excess | rule, cited | Kubernetes' documented order (f45 A) | `charge::tests` |

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
| the ring pins' share of a room | Shards p/ring-pins-v1's room-bounded pins | counts pins in what the VM holds; `Awaits(RingPinsWithinRoom)` |
| a Hex-booted guest told its room | a shards-api call for init's `ROOM` | Linux holds the VMM at `memory.high`; `Awaits(GuestRoom)` |
| moving an over VM instead of ending it | Shards D176 | ends it, last; `Awaits(LiveMove)` |
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
