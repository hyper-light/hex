# File-backed scheduler (CP-T7.4, LB03)

`hex schedule serve --config scheduler.json` runs the adapted peer core against initialized
Runtime endpoints. One node is a laptop deployment: its messages are drained locally, through the
same driver that handles remote messages. Peers each keep their own files; no central service or
database is required.

Start the ordinary runtime node first. A minimal Linux scheduler configuration is:

```json
{
  "cluster": "my-hex-laptop",
  "state": "/var/lib/hex/scheduler",
  "socket": "/run/hex/scheduler.sock",
  "backends": [{"class": "oci", "socket": "/run/hex/node.sock"}]
}
```

Every node of one cluster must declare the same exact `cluster` identity. Use a distinct identity
for independent clusters sharing a backend. Its SHA256 scopes physical box IDs; the durable epoch
envelope rejects rebinding existing records to a different namespace before runtime inspection.
Peer messages from another cluster are refused before their ordering or runtime effects.

The backend class describes the runtime the administrator actually initialized. A Shards endpoint
must have a working configured guest; selecting its name does not install or enable a hypervisor.
Hex currently has no built-in Firecracker Runtime adapter. A registered Firecracker endpoint must
implement the Runtime seam. Backend hello capabilities and live platform facts further restrict
what can be admitted. The scheduler offers no device attachments through the generic Runtime
endpoint, because that seam currently cannot carry selected device identities; such requests are
refused explicitly. The core still models and tests GPU load, model, memory and device filters.

The scheduler socket serves control through the ordinary bounded node wire and common client
reader. It refuses direct Runtime calls, which belong on the backend socket. Send a JSON control
request with `hex schedule call --socket /run/hex/scheduler.sock --request request.json`:

```json
{
  "Submit": {
    "demand": "cpus=1,mem=64m,class:oci,linux/amd64",
    "payload": {
      "spec": {
        "id": "my-box",
        "bounds": {"cpu_max_milli": 1000, "memory_max": 67108864},
        "class": "process",
        "owner": null
      },
      "start": false
    }
  }
}
```

These CPU and memory values are the example caller's requested reservation and enforced caps,
not scheduler tuning. Production submission requires finite CPU and memory caps covered by its
reservation. OCI specs may request `start: true`; a process box has no init to start. Rootfs and
mount paths name files on the selected backend; peer control does not copy image files. Callers
must stage those files before submitting a spec that names them.

The reply includes its durable `{owner, epoch, seq}` key and, once known, its selected node and
namespaced physical box ID. `tracked` distinguishes a retained record from an unknown/retired
key. A pending placement has no confirmed creation; a supplied key is not itself proof that a
record exists. After accepted deletion, the owner's record is retired only after the runner proves
physical deletion/absence. A pending placement has no confirmed creation. A `Status` or `Delete`
request carries that key, for example `{"Status":{"key":{"owner":0,"epoch":1,"seq":1}}}`.
`"Retry"` reconsiders pending work; `{"LinkUp":{"peer":1}}` reconciles a restored peer link.
Attempt refusals remain typed wire refusals in the reply; one rejected receiver does not mean
that another compatible receiver cannot accept the box.

For peers, give every node a dense ID from zero and configure one `peers` entry per remote node.
The receiver's live description supplies its facts; `nodes` and `models` may optionally seed known
facts. Applied peer facts are cached in files for crash recovery. A peer entry is:

```json
{"node": 1, "host": "hex-worker", "command": "hex schedule tunnel --socket /run/hex/scheduler.sock --from 0"}
```

The entry goes inside the configuration's `peers` array. `host` names an SSH configuration with
the correct user and identity. SSH runs in batch mode with strict host-key checking; trust and
keys must already be configured. Give each peer a distinct authorized key with an SSH forced
command fixing `--from` to that peer's ID, for example
`restrict,command="hex schedule tunnel --socket /run/hex/scheduler.sock --from 0"` before its
public key in `authorized_keys`. The CLI requires this forced-command form; the tunnel permits
only that sender's peer controls, never local administration calls. The remote command is administrator configuration, never text
from a submitted box. Each node re-probes its own current room before new admission; a stale
remote offer cannot force the receiving node to over-admit. Box reservations last until physical
deletion, including when an OCI init exits or a process box is empty.

Core rows and immutable Runtime payloads share one atomic file replacement, under `placements`.
A state-directory file lease excludes a second writer. Transport ordering records live under
`transport`: a sender commits a new epoch before sending; each peer lane numbers its messages;
a receiver commits its high-water mark and holds its ordering lock across the physical effect.
An old connection's delayed Start is therefore fenced after Delete/Ended/Forget, including after
a receiver restart. Silence never releases a reservation or authorizes a second creation. Lost
responses can repeat the current idempotent message. Retry pacing uses observed SSH operation
time and a shutdown event; there is no picked reconnect interval.

The wire identity covers the scheduler schema, peer codec/model, and serde BoxSpec definitions
as well as the existing wire and stream modules. A schema mismatch is refused at hello.

The source anchors below are pinned kiln revisions. Hex anchors name the integrated source at
`7b85e29cf9cbd7fe84545e43a924b8bec0a6319e`; symbols identify the implementation if later edits move
the lines. The earlier port's source mappings are recorded in `docs/design/hex-sim.md`, "Ported
from kiln's load balancer". The finish-model pin is
`e2a330eb51ae7a197856e04f7e6ce8ab0431416c`.

| Pinned source file:line | Hex file:line | What was CI- or agent-specific, and the box adaptation |
| --- | --- | --- |
| kiln `a63dd98ef`, `crates/kiln-ci/src/place/mod.rs:1` (ownership and admission) | `crates/hex-sim/src/peer.rs:1055` (`Node::submit`), `:1140` (`place`) | The asking node owns the box and the receiving node admits it. kiln's two-offer, least-loaded choice for short CI legs becomes Hex's full-demand packing/finish choice for boxes that retain room until deletion. |
| kiln `a63dd98ef`, `crates/kiln-ci/src/place/owner.rs:121` (`Kept`), `crates/kiln-ci/src/place/durable_tests.rs:190` (`commit_owner`), `:204` (`publish`) | `crates/hex-sim/src/peer.rs:1077`, `:1161`, `:1232` (owner-row writes), `crates/hex-node/src/placement.rs:515` (`Driver::drive`), `:604` (`completed`) | Keep commit-before-publish. Persist intent before Runtime effects; publish creation only after Runtime success and durable feedback. Physical deletion, rather than CI job completion, releases the reservation. |
| kiln `a63dd98ef`, `crates/kiln-ci/src/place/runner.rs:149` (`Record`), `:526` (`Admission`); `crates/kiln-ci/src/place/owner.rs:1241` (`restarted`) | `crates/hex-sim/src/peer.rs:46` (`Key`), `:623` (`boot`), `:1376` (`start`), `:1522` (`end`), `:1558` (`link_up`), `:3556` (reused-ID regression) | Preserve owner/epoch/sequence identity and ask `Holds` after restart or reconnection. Agent sessions that outlived a kiln runner become boxes recovered through Runtime inspection. Silence never permits a replacement creation. |
| kiln `a63dd98ef`, `crates/kiln-ci/src/place/durable_tests.rs:1` (crash-at-write model) | `crates/hex-sim/src/peer.rs:1712` (`Crashing`), `:1793` (`World`), `:2299` (peer crash/link test), `:2336` (laptop crash test) | One owner and two runners become nodes that each own and run boxes; one node uses the same peer state machine. Crashes before/after writes and lost links must neither lose nor double a box. |
| kiln `f20858c9c`, `crates/kiln-ci/src/lb/index.rs:259` (`top`), `:272` (`below`); `crates/kiln-ci/src/lb/owner.rs:574` (`pick`), `:623` (`walk`) | `crates/hex-sim/src/lb/index.rs:355` (`at_or_above`), `:375` (`above`); `crates/hex-sim/src/lb/owner.rs:607` (`pick`), `:674` (`walk`); `crates/hex-sim/src/index.rs:720` (`fit_scored`) | kiln walks the most-free buckets to spread jobs. Hex walks upward from the requested CPU bucket and ranks admitted nodes by GPU load, learned finish when available, then packing. Full class/platform/device filters remain in the shared pick. |
| kiln `f20858c9c`, `crates/kiln-ci/src/lb/runner.rs:1` (ledger), `:727` (`restart`) | `crates/hex-sim/src/lb/runner.rs:308` (`outside`), `:505` (`start`), `:626` (`end`), `:836` (`recover`) | A kiln restart assumed jobs died. Hex recovers reservations for boxes that outlive the agent; outside use is host use beyond Hex's boxes. The receiver's ledger remains the admission authority. |
| kiln `f20858c9c`, `crates/kiln-ci/src/lb/slab.rs:1`, `crates/kiln-ci/src/lb/msg.rs:1` | `crates/hex-sim/src/lb/slab.rs:13` (`Handle`), `:37` (`Slab`); `crates/hex-sim/src/lb/msg.rs:12` (`Word`), `:40` (`Start`) | Keep generational handles and bounded control words. A job's start/end become a box's create/start and release; unused CI-only slab helpers and owner batch/reuse fast paths are omitted. |
| kiln `a63dd98ef`, `crates/kiln-ci/src/facts.rs:12` (`RunnerFacts`), `:285` (`Gpu`), `:295` (`Virt`) | `crates/hex-sim/src/facts.rs:20` (`Facts`), `:81` (`Gpu`), `:125` (`classes`), `:241` (`node_with_owned`) | Omit CI tools, executors, labels and binfmt facts. Retain hardware totals separately from free capacity; add back only measured own memory/disk consumption before charging reservations. Runtime classes require initialized backends plus platform support. |
| kiln `a63dd98ef`, `crates/kiln/src/runner/probe.rs:228` (`kvm`), `:236` (`volume`), `:267` (`machine`), `:314` (`mem`) | `crates/hex-node/src/facts.rs:56` (`with_classes`), `:62` (`probe`), `:96` (`gpu_facts`), `:121` (`machine`); `crates/hex-sim/src/facts.rs:113` (`within_cgroup`) | Replace sysinfo and GPU subprocess probes with native OS readers and retained hex-gpu telemetry. Linux cgroup room limits available memory. GPU facts support core filtering/scoring; they do not claim production device attachment through the Runtime endpoint. |
| kiln `e2a330eb`, `crates/kiln-ci/src/lb/owner/terms.rs:19` (`Warm`), `:35` (`Terms`); `crates/kiln-ci/src/lb/owner/orders.rs:27` (`Link`) | `crates/hex-sim/src/start_cost.rs:20` (`Link`), `:39` (`Warm`), `:64` (`Terms`), `:341` (`Costs::finish`) | Keep measured CPU/I/O/network/transfer and interactive-link terms with their integer units. Borrow per-request warm facts directly; do not invent image identities or transfer rates. |
| kiln `e2a330eb`, `crates/kiln-ci/src/lb/owner/terms.rs:395` (`finish`), `:485` (`Rates`), `:525` (`Mean`), `:576` (`cpu_time`), `:595` (`stability`), `:611` (`exp_neg_q32`) | `crates/hex-sim/src/start_cost.rs:30` (`Rates`), `:85` (`Mean`), `:341` (`finish`), `:437` (`cpu_time`), `:455` (`stability`), `:472` (`exp_neg_q32`); `crates/hex-node/src/placement.rs:549` (launch measurement) | Retain the empirical prior and fixed-point finish arithmetic. Hex additionally learns complete Runtime create/start service by node/class, including opaque staging. Explicit modeled components exclude that combined observation, so transfer is not counted twice; recovered boxes add no partial sample. |
| kiln `e2a330eb`, `crates/kiln-ci/src/lb/order.rs:18` (`class_of`); `crates/kiln-ci/src/lb/owner/terms.rs:108` (`within_bound`), `:129` (`ROUND`) | `crates/hex-sim/src/start_cost.rs:417` (`within_bound`), `:422` (`class_of`); `crates/hex-sim/src/index.rs:676` (`fit_with_terms_excluding`), `:806` (finish-band packing) | Keep the source's six-bit/1/64 resolution. Hex uses its complete bounded candidate walk rather than porting kiln's per-rate threshold-order machinery; packing decides within the finish band. With no cost signal it uses the original single walk. |
| Hex-native shell, using `crates/hex-client/src/calls.rs:1557` (`Core::schedule`) and `crates/hex-node/src/serve.rs:478` (`Server`), at the Hex revision above | `crates/hex-core/src/schedule.rs:56` (`Command`), `crates/hex-node/src/records.rs:132` (`Files::put`), `crates/hex-node/src/scheduler/order.rs:88` (`Order::accept`), `crates/hex-node/src/scheduler/tunnel.rs:217` (`serve`) | This integration has no kiln source counterpart: file-backed records, common bounded control reader, sender-bound SSH stdio and durable connection fencing drive the adapted core without a central service. It supplies neither a Firecracker adapter nor generic GPU attachment. |

Functional checks do not constitute new Firecracker/WHP boot records or performance measurements.
The lead runs the final platform gates, root lab, crash simulations and performance bars on the
integrated tree before publishing it.
