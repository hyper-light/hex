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

## Platforms

- **E2B** is self-hostable on one Linux host with KVM through `embed/` (Docker Compose, real Firecracker
  sandboxes) in `e2b-dev/runtime`
  ([`agent-sandbox-platforms.md`](../../docs/research/runtime/agent-sandbox-platforms.md)).
- **Daytona**'s frozen v0.190.0 tree is AGPL-3.0 and Docker-based. It needs Postgres, Redis, a registry
  and S3-compatible storage.
- **Hosted-only platforms** (E2B Cloud, Daytona Cloud, Modal, Morph, CodeSandbox, Fly Machines, Vercel,
  Cloudflare) can be measured only end to end, from the bench host through their public APIs, with
  accounts and API keys that only Ada can provide. They are labelled hosted, as their numbers include the
  network.
