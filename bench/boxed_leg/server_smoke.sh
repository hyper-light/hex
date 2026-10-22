#!/usr/bin/env bash
# Consumer #1 dispatched-leg BOXED smoke (CP-T7.1, AC-T7.1.1). Unlike the "here"-mode run.sh (which
# doesn't box — `kiln workflow run` never calls hold_agents), this stands up a real `kiln server`
# whose in-process local_agent BOXES, because Cmd::Server calls os::contain::hold_agents(). The server
# runs as a transient `systemd-run --user` unit with Delegate=yes + KILN_HEX_BOX=1, so its cgroup is a
# delegated subtree the agent vacates and boxes within. A scratch repo is registered (a bare mirror in
# the server's data dir); a leg is dispatched with `kiln workflow run --server`. The leg shows itself
# boxed with its bounds and the protected leaves (evidence.sh); it runs an adacachy-shaped build storm
# (storm.sh); the driver then checks the protected leaves and that no leg box is left; and tears it all
# down.
#   server_smoke.sh <kiln binary> [full|dry]     dry = mechanism only (no storm)
set -uo pipefail
KILN=$(realpath "${1:?usage: server_smoke.sh <kiln binary> [full|dry]}")
MODE=${2:-full}
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/hex-t7"; mkdir -p "$CACHE"
LAB=$(mktemp -d "$CACHE/srv-smoke.XXXXXX")
UNIT="hex-smoke-$$"
TOKEN="smoke-token-$$"
cleanup() {
  systemctl --user stop "$UNIT.service" >/dev/null 2>&1 || true
  systemctl --user reset-failed "$UNIT.service" >/dev/null 2>&1 || true
  rm -rf "${LAB:?}"
}
trap cleanup EXIT

freeport() { python3 -I -c "import socket; t='$1'; s=socket.socket(socket.AF_INET, socket.SOCK_DGRAM if t=='udp' else socket.SOCK_STREAM); s.bind(('127.0.0.1',0)); print(s.getsockname()[1]); s.close()"; }
HTTP=$(freeport tcp); QUIC=$(freeport udp)
DATA="$LAB/server"; mkdir -p "$DATA/git"
URL="http://127.0.0.1:$HTTP"

cat > "$LAB/server.toml" <<TOML
[server]
http_addr = "127.0.0.1:$HTTP"
quic_addr = "127.0.0.1:$QUIC"
public_url = "$URL"
quic_advertise = "127.0.0.1:$QUIC"
https = false
data_dir = "$DATA"

[local_agent]
enabled = true
slots = 4

[ci]
enabled = true
kiln_workflows = true
github_labels = true
routing = [{ executor = ["host"] }]

[discovery]
enabled = false

[cd]
enabled = false

[terminal]
enabled = false
TOML

# A scratch repo: a working tree with the self-contained workflow (the step scripts inlined, so no
# checkout is needed), and a bare mirror in the server's git dir so the server knows the repo.
REPO="local/boxed-smoke"
MIRROR="$DATA/git/$(printf '%s' "$REPO" | sed 's#/#__#g').git"
WT="$LAB/wt"; mkdir -p "$WT/.github/workflows"
indent() { sed 's/^/          /' "$1"; }
{
  printf 'name: boxed-leg-smoke\non: push\njobs:\n  heavy:\n    runs-on: ubuntu-latest\n    steps:\n'
  printf '      - name: box-bounds\n        run: |\n'; indent "$HERE/evidence.sh"
  printf '      - name: build-storm\n        run: |\n'; indent "$HERE/storm.sh"
  printf '      - name: done\n        run: echo "smoke leg complete"\n'
} > "$WT/.github/workflows/smoke.yml"
( cd "$WT" && git init -q && git add -A && git -c user.email=s@localhost -c user.name=smoke commit -qm smoke ) || { echo "git setup failed"; exit 1; }
git clone --quiet --bare --shared "$WT" "$MIRROR" || { echo "mirror failed"; exit 1; }

echo "=== starting kiln server (unit $UNIT.service) boxed: Delegate=yes + KILN_HEX_BOX=1 ==="
systemd-run --user --unit="$UNIT" -p Delegate=yes --quiet \
  --setenv=KILN_HEX_BOX=1 --setenv=KILN_ADMIN_TOKEN="$TOKEN" \
  --setenv=XDG_CACHE_HOME="${XDG_CACHE_HOME:-$HOME/.cache}" \
  -- "$KILN" server --config "$LAB/server.toml" || { echo "systemd-run failed"; exit 1; }

echo "=== waiting for the server to listen on 127.0.0.1:$HTTP ==="
upcheck() { python3 -I -c "
import socket, sys
s = socket.socket(); s.settimeout(0.5)
try:
    s.connect(('127.0.0.1', $HTTP)); sys.exit(0)
except OSError:
    sys.exit(1)
"; }
up=no
for _ in $(seq 1 150); do
  if upcheck >/dev/null 2>&1; then up=yes; break; fi
  systemctl --user is-active "$UNIT.service" >/dev/null 2>&1 || { echo "server unit exited early — logs:"; systemctl --user status "$UNIT.service" --no-pager -n 20 2>/dev/null || true; break; }
  sleep 0.2
done
echo "server listening: $up"
UNITCG="/sys/fs/cgroup$(systemctl --user show "$UNIT.service" -p ControlGroup --value 2>/dev/null)"
echo "unit cgroup: ${UNITCG#/sys/fs/cgroup}"

echo "=== dispatch: kiln workflow run --server (leg runs on the server's boxed local_agent) ==="
( cd "$WT" && SMOKE_STORM=$([ "$MODE" = dry ] && echo 0 || echo 1) \
  "$KILN" workflow run --server "$URL" --repo "$REPO" --token "$TOKEN" --report "$LAB/report.json" 2>&1 )

echo "=== post-run: protected leaves (memory.high on legs' parent, memory.low on runner/agents) ==="
find "$UNITCG" -maxdepth 5 -type d \( -name supervisor -o -name hex -o -name hex-legs \) 2>/dev/null | sort | while read -r d; do
  printf '  %-48s cpu.weight=%s memory.high=%s memory.low=%s\n' "${d#/sys/fs/cgroup}" \
    "$(cat "$d/cpu.weight" 2>/dev/null || echo -)" "$(cat "$d/memory.high" 2>/dev/null || echo -)" "$(cat "$d/memory.low" 2>/dev/null || echo -)"
done
left=$(find "$UNITCG" -type d -name 'leg.*' 2>/dev/null | wc -l)
echo "leftover leg boxes after the run: $left (0 = per-leg cleanup/sweep clean)"
[ -f "$LAB/report.json" ] && { echo "=== report.json ==="; cat "$LAB/report.json"; }
echo "=== tearing down $UNIT.service ==="
