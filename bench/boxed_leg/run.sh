#!/usr/bin/env bash
# Consumer #1 dispatched-leg smoke (CP-T7.1, AC-T7.1.1, the machine-protection half).
#
# FINDING (2026-10-22, from the first slot run): this "here"-mode vehicle does NOT box the leg, BY
# DESIGN. kiln boxes only in a process that called `os::contain::hold_agents()` — done by `Cmd::Server`
# (main.rs:459) and `Cmd::Agent` (:468), NOT by `Cmd::Workflow`/`kiln workflow run` (:785). The guard
# exists so a tool that merely links kiln (a `kiln workflow run` invoked on a box that is itself a live
# runner) does not box the runner's cgroup it happens to sit in. So the leg here runs unboxed in the
# scope root (its cgroup is `<scope>`, not `<scope>/hex-legs/leg.*`), which this script now DOCUMENTS as
# the guard working. The BOXED smoke needs a real `kiln server` (its local_agent boxes) or `kiln agent`
# under the Delegate=yes scope, dispatched to via `kiln workflow run --server`. See the server variant.
#
# As-is it stands up a scratch server + one runner = this machine via `kiln workflow run` in "here" mode
# inside `systemd-run --user --scope -p Delegate=yes` with KILN_HEX_BOX=1, and shows (via evidence.sh)
# that the leg's cgroup is the scope root (unboxed) — the hold_agents guard in effect.
# The heavy job shows (1) it runs in a Hex box with its bounds, (2) the machine is protected (legs'
# parent memory.high, runner/agents memory.low) and a build storm doesn't starve the host, (3) no leg
# box is left behind. A scaled memory.high throttle demo proves the cap safely.
#   run.sh <kiln binary> [full|dry]     dry = mechanism only, no heavy storm
set -uo pipefail
KILN=$(realpath "${1:?usage: run.sh <kiln binary> [full|dry]}")
MODE=${2:-full}
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/hex-t7"
mkdir -p "$CACHE"
LAB=$(mktemp -d "$CACHE/boxed-leg.XXXXXX")
trap 'rm -rf "${LAB:?}"' EXIT
REPO="$LAB/repo"
mkdir -p "$REPO/.github/workflows"

# A self-contained workflow: the two step scripts inlined into run: blocks (no checkout needed).
indent() { sed 's/^/          /' "$1"; }
{
  printf 'name: boxed-leg-smoke\non: push\njobs:\n  heavy:\n    runs-on: ubuntu-latest\n    steps:\n'
  printf '      - name: box-bounds\n        run: |\n'; indent "$HERE/evidence.sh"
  printf '      - name: build-storm\n        run: |\n'; indent "$HERE/storm.sh"
  printf '      - name: done\n        run: echo "smoke leg complete"\n'
} > "$REPO/.github/workflows/smoke.yml"

( cd "$REPO" && git init -q && git add -A \
  && git -c user.email=smoke@localhost -c user.name=smoke commit -qm smoke ) || { echo "git setup failed"; exit 1; }

# The in-scope logic: run the workflow on a scratch server+runner, then (still in the delegated scope,
# after the runner's cleanup) check no leg box is left and prove memory.high throttles safely.
cat > "$LAB/inner.sh" <<INNER
set -uo pipefail
ROOT=/sys/fs/cgroup
cd "$REPO"
echo "--- pre-kiln scope diagnostics ---"
mycg=\$ROOT\$(awk -F: '\$1=="0"{print \$3}' /proc/self/cgroup)
echo "  my cgroup: \${mycg#\$ROOT}"
echo "  controllers=[\$(cat "\$mycg/cgroup.controllers" 2>/dev/null)] subtree_control=[\$(cat "\$mycg/cgroup.subtree_control" 2>/dev/null)]"
echo "  procs in my scope root=\$(wc -l < "\$mycg/cgroup.procs" 2>/dev/null)  KILN_HEX_BOX=\${KILN_HEX_BOX:-unset}"
echo "=== kiln workflow run (scratch server + one runner = this machine) ==="
SMOKE_STORM=$([ "$MODE" = dry ] && echo 0 || echo 1) RUST_LOG="\${RUST_LOG:-warn,kiln::os=debug}" \
  "$KILN" workflow run --executor host --report "$LAB/report.json" 2>&1; code=\$?
echo "=== kiln workflow run exit \$code ==="

# This process was vacated to OWN/supervisor by the runner; find OWN (holds 'supervisor').
own=\$ROOT\$(awk -F: '\$1=="0"{print \$3}' /proc/self/cgroup)
while [ -n "\$own" ] && [ "\$own" != "\$ROOT" ] && [ "\$own" != "/" ]; do
  [ -d "\$own/supervisor" ] && break; own=\$(dirname "\$own"); done
echo "=== after the run: leftover leg boxes under \${own#\$ROOT} (expect none) ==="
left=\$( (ls -d "\$own"/hex-legs/leg.* "\$own"/hex/leg.* 2>/dev/null) | wc -l )
ls -d "\$own"/hex-legs/leg.* "\$own"/hex/leg.* 2>/dev/null || echo "  none"
echo "leftover-leg-boxes: \$left (0 = sweep/cleanup clean)"

echo "=== scaled memory.high throttle demo (a hog in a 64 MiB-high box is throttled, not host-OOM) ==="
demo="\$own/smoke-mem"
if mkdir -p "\$demo" 2>/dev/null && echo 67108864 > "\$demo/memory.high" 2>/dev/null; then
  ( echo \$BASHPID > "\$demo/cgroup.procs" 2>/dev/null
    timeout 10 python3 -I -c 'b=bytearray(512*1024*1024)
for i in range(0,len(b),4096): b[i]=1' 2>/dev/null; echo "  hog exit \$? (throttled under 64MiB-high, host alive)" )
  high=\$(cat "\$demo/memory.high" 2>/dev/null); cur=\$(cat "\$demo/memory.current" 2>/dev/null)
  echo "  demo box: memory.high=\$high memory.current=\$cur"
  rmdir "\$demo" 2>/dev/null && echo "  demo box removed" || echo "  demo box left (will sweep)"
else
  echo "  (could not make a sub-box; memory controller not delegated here)"
fi
exit \$code
INNER

echo "=== running the smoke under systemd-run --user --scope -p Delegate=yes (KILN_HEX_BOX=1) ==="
systemd-run --user --scope --quiet -p Delegate=yes \
  --setenv=KILN_HEX_BOX=1 --setenv=XDG_CACHE_HOME="${XDG_CACHE_HOME:-$HOME/.cache}" \
  -- bash "$LAB/inner.sh"
rc=$?
echo "=== smoke exit $rc ==="
if [ -f "$LAB/report.json" ]; then echo "=== report.json ==="; cat "$LAB/report.json"; fi
exit "$rc"
