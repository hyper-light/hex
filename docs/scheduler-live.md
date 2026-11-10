# File-backed scheduler (CP-T7.4, LB03)

`hex schedule serve --config scheduler.json` runs the adapted peer core against initialized
Runtime endpoints. One node is a laptop deployment: its messages are drained locally, through the
same driver that handles remote messages. Peers each keep their own files; no central service or
database is required.

Start the ordinary runtime node first. A minimal Linux scheduler configuration is:

```json
{
  "state": "/var/lib/hex/scheduler",
  "socket": "/run/hex/scheduler.sock",
  "backends": [{"class": "oci", "socket": "/run/hex/node.sock"}]
}
```

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
namespaced physical box ID. A pending placement has no confirmed creation. A `Status` or `Delete`
request carries that key, for example `{"Status":{"key":{"owner":0,"epoch":1,"seq":1}}}`.
`"Retry"` reconsiders pending work; `{"LinkUp":{"peer":1}}` reconciles a restored peer link.
Attempt refusals remain typed wire refusals in the reply; one rejected receiver does not mean
that another compatible receiver cannot accept the box.

For peers, configure a dense `nodes` list of previously exchanged `NodeSpec` facts and its shared
`models` name table, and one `peers` entry per remote node:

```json
{"node": 1, "host": "hex-worker", "command": "hex schedule tunnel --socket /run/hex/scheduler.sock"}
```

The entry goes inside the configuration's `peers` array. `host` names an SSH configuration with
the correct user and identity. SSH runs in batch mode with strict host-key checking; trust and
keys must already be configured. The remote command is administrator configuration, never text
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

| Source | Hex adaptation | Change for boxes |
| --- | --- | --- |
| kiln `place/` at `a63dd98ef`, commit-before-publish and reused-ID tests | `hex-sim/src/peer.rs`; `hex-node/src/placement.rs` | Durable owner/runner state, full class/platform/device demand, actual Runtime effects before acknowledgement, deletion instead of job completion. |
| kiln `lb/` kernel at `f20858c9c` | `hex-sim/src/lb/`; `hex-sim/src/index.rs` | One bounded admission/pick core; reservations and selected devices remain held by long-lived boxes. |
| kiln `runner/probe.rs` and facts at `a63dd98ef` | `hex-node/src/facts.rs`; `hex-sim/src/facts.rs` | Current free room plus measured own use, initialized runtime classes, portable GPU inventory/load and cgroup headroom. |
| kiln `lb/owner/terms.rs`, `owner/orders.rs`, `order.rs` at `e2a330eb` | `hex-sim/src/start_cost.rs`; `hex-sim/src/index.rs` | Learned finish terms and original integer resolution; box-launch observations replace assumptions about CI jobs. |
| Hex's existing local node wire/client and stream framing | `hex-core/src/schedule.rs`; `hex-node/src/scheduler/` | Same control reader/admission; authenticated SSH stdio shell, durable connection fencing, no central control plane. |

Functional checks do not constitute new Firecracker/WHP boot records or performance measurements.
The lead runs the final platform gates, root lab, crash simulations and performance bars on the
integrated tree before publishing it.
