# hex
Agentic runtime and compute provisioning, scheduling, and management.

Hex is the scheduler and runtime: agent scheduling orchestration for infinite scale. It runs process boxes, OCI containers, Firecracker microVMs and Shards microVMs, and places them
from a laptop to millions of agents in one cluster, built on what Kubernetes, Borg and the rest
learned. It replaces Shards' own host-side scheduler and runtime.

**Status:** the repository has its bar (`CLAUDE.md`, the gate, the requirement registry); the
`hex` binary says what it is, and nothing more yet. `STATUS.md` says what works and what's next;
`docs/design/` is the design.

```sh
cargo build --release      # target/release/hex
bash scripts/gates.sh      # every gate, as kiln CI runs them
```

MIT licensed (`LICENSE`).
