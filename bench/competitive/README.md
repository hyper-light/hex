# Competitive baseline (goal 3)

Hex's runtime classes against named competitors on the same hardware, per CLAUDE.md §7 and the research
bar in [`docs/research/README.md`](../../docs/research/README.md): Hex must beat the best competitor by
at least 4-5x on throughput, latency (p50 through p99.9 and max) and resource use. This directory
measures where Hex stands today. `RESULT.md` records each run of record.

## What is compared

| Hex class | Competitors | Where |
|---|---|---|
| `oci` (`hex oci create/start/state/kill/delete`) | runc, crun, youki, gVisor `runsc` (each project's latest release, fetched and verified by `fetch.py`) | one privileged bench container, private cgroup namespace, no network |
| `process` (`hex box run`) | systemd transient scopes (`systemd-run --user --scope`) and services (`systemd-run --user --wait`) | a delegated systemd user service on the host |
| sandbox platforms | Daytona (self-hosted, v0.190.0 tree, Docker-based) and E2B (`embed/`, Firecracker on one KVM host); hosted platforms end to end, labelled hosted | see "Platforms" below |

## What is measured

- **Lifecycle latency**: create, start, exec (`true` in the running container) and delete --force of a
  `sleep infinity` container, each operation timed alone; and create+start of a `true` container
  (AC-T7.1.3's measure). n = 10,000 per round after 100 warm-ups (runsc 3,000, as each lifecycle takes far
  longer); p50, p99, p99.9 (nearest rank) and max.
- **Throughput**: create+start+delete of `true` containers at concurrency 1, 8 and 32; lifecycles per second.
- **CPU per workload**: the whole bench cgroup's `cpu.stat` usage per lifecycle (CLI, shims, sandboxes and
  workload together); per-call CLI CPU from `wait4` rusage is also kept.
- **Memory and density**: `sleep` containers started until a cap (1,000) or a memory budget (8 GiB), memory
  per container from the bench cgroup's `memory.current`, and containers per GiB.
- **Process boxes**: the same axes for running `true` (latency, throughput) or `sleep` (density) in a box
  bounded at 256 MiB and 64 pids. systemd's CPU includes the user manager's and the user bus's.

Every call's stdio goes to `/dev/null` or an error file, never a pipe (a detached `create` holds the
caller's stdio; commit 4c76c13). Calls are spawned with `posix_spawn` (OCI) or `fork`+`execve` (boxes,
identically for every method) and waited with a blocking `wait4`, with a timeout on each. Rounds run in
a shuffled runtime order, and the best of 3 rounds is reported per metric.

## Running it (fedora, in a bench-calendar slot)

```
cargo build --release -p hex-cli            # nice -n 10, -j 8, CARGO_TARGET_DIR under ~/.cache
systemd-run --user --unit=hexbench --collect bash bench/competitive/host.sh "$CARGO_TARGET_DIR/release/hex"
```

`host.sh` refuses any host but fedora, waits up to 15 minutes for a load average under 2 and refuses if it
doesn't get one, records the environment and the competitors' versions, and writes everything under
`~/.cache/hex-bench/out/<UTC time>/`, including `table.md` from `summarize.py`.

### Checking the drivers off the bench host

`HEXBENCH_BIN`, `HEXBENCH_CGROUP` and `HEXBENCH_ERR` (oci_bench.py) and `HEXBENCH_SELF_CGROUP` and
`HEXBENCH_USER_CGROUP` (box_bench.py) point the drivers at stand-in binaries and a plain directory in
place of the cgroup tree. That runs every phase's control flow (warm-ups, error stops, teardown, the
summary) without containers, cgroups or load. The numbers it produces are meaningless. On 2026-10-23 it
found three bugs before any slot was spent: a container that failed to start counted as running, a dead
worker that could hang the box driver, and a ratio that divided by zero.

## Platforms

`platform_bench.py <adapter> <out> <n> <warmup> <lifecycles> <cap>` drives a platform through its own
API: create until the sandbox runs a command, exec of `true`, delete; whole lifecycles per second at
concurrency 1, 8 and 32; idle sandboxes up to a cap. A platform's cost is spread over many services, so
its CPU and memory are its stack's own cgroups' (`STACK_CGROUPS`: the stack's containers,
`docker.service`, `containerd.service`) plus the client's, never host-wide figures, which on a shared host
charge the platform with everyone else's work. The stack's idle CPU is measured over a minute first and
taken out of each phase, so CPU per lifecycle is what a lifecycle adds; the idle cost is a row of its
own. A density phase lists the processes that appeared outside those cgroups, so a sandbox the list
misses shows up by name.
Results go in the run directory as `platform.<name>.r<round>.json`, and `summarize.py` sets them against
Hex's `oci` class (create+start). **That comparison crosses isolation classes**: E2B isolates each
sandbox in a Firecracker microVM, Daytona's container class shares the host kernel. The like-for-like
comparison for E2B is Hex's `firecracker` or `shards` class, which does not exist yet; the table says so.

- **E2B** is self-hostable on one Linux host with KVM through `embed/compose` in `e2b-dev/runtime`
  ([`agent-sandbox-platforms.md`](../../docs/research/runtime/agent-sandbox-platforms.md)): `docker
  compose up -d --wait`, then `eval "$(docker compose exec ready cat /run/e2b/sdk.env)"` and the SDK
  `e2b==2.46.0` in a venv under `~/.cache`. **It changes the host**: it loads `nbd`, `tun` and `kvm`, writes
  `/etc/modules-load.d/e2b.conf`, `/etc/modprobe.d/e2b-nbd.conf`, a udev rule and `/etc/sysctl.d/90-e2b.conf`
  (raising `net.ipv4.tcp_max_syn_backlog` and `vm.max_map_count`), adds one iptables mangle rule, and
  reserves hugepages (4 GiB by default). `docker compose down -v` then `docker compose --profile purge run
  --rm host-teardown` undoes it, except `kvm` and `tun` stay loaded and the two sysctls stay raised until a
  reboot. On fedora, a shared host, that needs the coordinator's and Ada's go-ahead first.
- **Daytona**: its `docker/docker-compose.yaml` (14 services: API, proxy, a privileged runner, SSH gateway,
  Dex, Postgres, Redis, a registry, MinIO, tracing) runs inside Docker without host changes. Its images are
  untagged (`latest`), so the run records each image's digest; the frozen v0.190.0 tree
  (`01c502bb`) is what the research read. An API key comes from its dashboard (Dex dev login).
- **Docker** (`docker` adapter): the Docker Engine stack most people run (dockerd, containerd, runc shim),
  through its CLI: `run -d` until it runs, `exec`, `rm -f`, with the OCI bundles' rootfs `docker import`ed
  and the host's `/usr` bound read-only, so the workload is the same.
- **Kubernetes** (`k8s` adapter): a Pod on a single-node kind cluster (inside Docker, no host changes)
  through the API: create until Running and Ready, exec, delete until it is gone.
- **Kubernetes Agent Sandbox** (`agentsandbox`, `agentsandbox-warm`): kubernetes-sigs/agent-sandbox on the
  same cluster, a `Sandbox` until Ready, or a `SandboxClaim` from a `SandboxWarmPool`.
- **Agent Substrate and Google AX** (`cli` adapter): agent-substrate/substrate (`kubectl ate`: actors from
  an actor template, with suspend and resume, its claimed sub-400 ms resume) and google/ax (`ax apply`)
  on top of it, on the kind cluster. Substrate runs actors only in gVisor or a microVM, so the kind node
  needs runsc as a containerd runtime. Hex has no suspend or resume yet, so those rows show Hex n/a.
- **Hosted-only platforms** (E2B Cloud, Daytona Cloud, Modal, Morph, CodeSandbox, Fly Machines, Vercel,
  Cloudflare) can be measured only end to end, from the bench host through their public APIs, with
  accounts and API keys that only Ada can provide. Their results are labelled hosted (`HOSTED=1`), as the
  numbers include the network.
