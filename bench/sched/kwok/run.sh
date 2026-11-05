#!/usr/bin/env bash
# AC-T7.11.3's kube-scheduler side: kube-scheduler v1.37.1 binding hex-sim's generated cluster on
# kwok's fake nodes, timed from the API server's watch (bench/sched/kube_scheduler_kwok.md).
#
#   run.sh prep             pinned binaries (sha256 checked), the sched_kwok example built, manifests written
#   run.sh one <cfg> <i>    one timed run: a fresh cluster, the nodes, every pod created with no scheduler
#                           running, then kube-scheduler started and timed to its last bind
#   run.sh series           best of RUNS for each config, each run under LOCK when it is set
#   run.sh hex              Hex's side: the sched_kwok example's `hex` mode, RUNS runs at HEX_SHARDS
#
# <cfg> is `defaults` (kwokctl's cluster as it comes; kube-scheduler with its client defaults, 50 QPS,
# burst 100) or `raised` (kwokctl --disable-qps-limits: the API server's in-flight limits off; and
# kube-scheduler's client at 5000 QPS, burst 10000).
# Environment: NODES (10000) and PODS (50000), which only check the example's counts; RUNS (3);
# FILES (64, manifest files created side by side); STALL_S (120); HEX_SHARDS (1,12); REPO (the
# checkout, for prep's build); LOCK (empty: no lock, for smoke runs).
set -euo pipefail

ROOT="${HOME:?}/.cache/hex-t7/sched-g6/kwok"
BIN="$ROOT/bin"
NODES="${NODES:-10000}"
PODS="${PODS:-50000}"
RUNS="${RUNS:-3}"
FILES="${FILES:-64}"
HEX_SHARDS="${HEX_SHARDS:-1,12}"
STALL_S="${STALL_S:-120}"
LOCK="${LOCK-}"
HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
MANIFESTS="$ROOT/manifests/h13-f$FILES"
NAME=hexg6
export KWOK_WORKDIR="$ROOT/work"
KUBECONFIG_FILE="$ROOT/kubeconfig"

K8S=v1.37.1
KWOK=v0.8.0
ETCD=v3.6.10
# Pinned digests, read 2026-11-04 from dl.k8s.io's .sha256 files, kwok's release assets and etcd's
# SHA256SUMS; a download that doesn't match is refused.
declare -A SUM=(
  [kube-apiserver]=de9b1b43c10143f448408b3aaf15f44fd5248b97a56301417a1fe2f1fc47a691
  [kube-controller-manager]=acd67c8f8871d4594f307d2816bf11aa972f47617ee6e1f84b0ce92a091f7290
  [kube-scheduler]=8d10dfc64de70b1670cc1ce52701eb6a219c0905a95bdde608edc5432f9eca43
  [kubectl]=65691ff77eb6fa44c908b77a1082c9f092c3b9733b5cefabec0d1104890e21a8
  [kwok]=f5a4385a2ae1b9dd7f8acae18e002a75c0ebba124ae9d7259aa8dadb50711f46
  [kwokctl]=d5743166c657283d6e827c7c4c20cacfd9009fd94830f06718ca7b2c7099461f
  [etcd.tar.gz]=ed579fafab5701e3aaa95509969e7bc74776a4ae5269d32e3928408b406456ec
)
declare -A URL=(
  [kube-apiserver]=https://dl.k8s.io/release/$K8S/bin/linux/amd64/kube-apiserver
  [kube-controller-manager]=https://dl.k8s.io/release/$K8S/bin/linux/amd64/kube-controller-manager
  [kube-scheduler]=https://dl.k8s.io/release/$K8S/bin/linux/amd64/kube-scheduler
  [kubectl]=https://dl.k8s.io/release/$K8S/bin/linux/amd64/kubectl
  [kwok]=https://github.com/kubernetes-sigs/kwok/releases/download/$KWOK/kwok-linux-amd64
  [kwokctl]=https://github.com/kubernetes-sigs/kwok/releases/download/$KWOK/kwokctl-linux-amd64
  [etcd.tar.gz]=https://github.com/etcd-io/etcd/releases/download/$ETCD/etcd-$ETCD-linux-amd64.tar.gz
)

log() { echo "[$(date -u +%H:%M:%S)] $*"; }

fetch() {
  local name="$1" dl="$ROOT/dl/$1"
  mkdir -p "$ROOT/dl"
  if [ ! -f "$dl" ] || ! echo "${SUM[$name]}  $dl" | sha256sum -c --quiet; then
    curl -fsSL -o "$dl.part" "${URL[$name]}"
    echo "${SUM[$name]}  $dl.part" | sha256sum -c --quiet || { echo "sha256 mismatch: $name" >&2; exit 1; }
    mv "$dl.part" "$dl"
  fi
}

prep() {
  mkdir -p "$BIN"
  for b in kube-apiserver kube-controller-manager kube-scheduler kubectl kwok kwokctl; do
    fetch "$b"
    install -m 0755 "$ROOT/dl/$b" "$BIN/$b"
  done
  fetch etcd.tar.gz
  local x="$ROOT/dl/etcd-x"
  rm -rf -- "${x:?}"; mkdir -p "$x"
  tar -xzf "$ROOT/dl/etcd.tar.gz" -C "$x" --strip-components=1
  install -m 0755 "$x/etcd" "$BIN/etcd"
  "$BIN/kube-scheduler" --version; "$BIN/kwokctl" --version; "$BIN/etcd" --version | head -1
  : "${REPO:?prep needs REPO, the checkout to build the manifests example in}"
  (cd "$REPO" && nice -n 10 cargo build --release --locked -j 4 -p hex-bench --example sched_kwok)
  install -m 0755 "${CARGO_TARGET_DIR:?}/release/examples/sched_kwok" "$BIN/sched_kwok"
  echo "commit: $(git -C "$REPO" rev-parse HEAD)$(git -C "$REPO" diff --quiet HEAD -- crates/hex-bench crates/hex-sim || echo ' (+ uncommitted changes)')" >"$BIN/sched_kwok.commit"
  rm -rf -- "${MANIFESTS:?}"
  "$BIN/sched_kwok" manifests "$MANIFESTS" "$FILES"
  local n p
  n=$(cat "$MANIFESTS"/nodes-*.yaml | grep -c '^kind: Node$')
  p=$(cat "$MANIFESTS"/pods-*.yaml | grep -c '^kind: Pod$')
  log "manifests: $n nodes, $p pods"
  [ "$n" -eq "$NODES" ] && [ "$p" -eq "$PODS" ] || { echo "manifests hold $n nodes, $p pods" >&2; exit 1; }
}

kc() { "$BIN/kubectl" --kubeconfig "$KUBECONFIG_FILE" "$@"; }

# Creates every file matching $1 side by side, one kubectl each, and fails if any does.
create_all() {
  local pids=() f rc=0
  for f in $1; do
    kc create -f "$f" -o name >/dev/null &
    pids+=($!)
  done
  for p in "${pids[@]}"; do wait "$p" || rc=1; done
  return "$rc"
}

wait_for() { # <what> <timeout-s> <command...>: polls once a second for a fact, then refuses
  local what="$1" limit="$2"; shift 2
  local end=$(( $(date +%s) + limit ))
  until "$@"; do
    [ "$(date +%s)" -lt "$end" ] || { echo "timed out waiting for $what" >&2; return 1; }
    sleep 1
  done
}

nodes_ready() { [ "$(kc get nodes --no-headers 2>/dev/null | awk '$2=="Ready"' | wc -l)" -ge "$NODES" ]; }
sa_ready() { kc get serviceaccount default -n default >/dev/null 2>&1; }

host_state() {
  echo "uptime: $(uptime)"
  echo "docker_ps: $(docker ps -q 2>/dev/null | wc -l)"
  echo "hex_units: $(systemctl --user list-units 'hex-*' --state=active --no-legend --plain | awk '{print $1}' | tr '\n' ' ')"
  echo "cpus: $(cat /proc/self/status | awk '/Cpus_allowed_list/{print $2}')"
}

delete_cluster() { "$BIN/kwokctl" delete cluster --name "$NAME" --kubeconfig "$KUBECONFIG_FILE" >/dev/null 2>&1 || true; }

one() {
  local cfg="$1" i="$2"
  local out="$ROOT/results/$STAMP/$cfg-$i"
  mkdir -p "$out"
  local extra=() qps=""
  case "$cfg" in
    defaults) ;;
    raised) extra=(--disable-qps-limits); qps=$'  qps: 5000\n  burst: 10000' ;;
    *) echo "unknown config $cfg" >&2; return 2 ;;
  esac
  delete_cluster
  log "$cfg $i: create cluster"
  "$BIN/kwokctl" create cluster --name "$NAME" --runtime binary --kubeconfig "$KUBECONFIG_FILE" \
    --disable kube-scheduler \
    --kube-apiserver-binary "$BIN/kube-apiserver" \
    --kube-controller-manager-binary "$BIN/kube-controller-manager" \
    --kwok-controller-binary "$BIN/kwok" \
    --etcd-binary "$BIN/etcd" \
    --wait 5m "${extra[@]}" >"$out/kwokctl.log" 2>&1
  log "$cfg $i: $NODES nodes"
  create_all "$MANIFESTS/nodes-*.yaml"
  wait_for "nodes ready" 600 nodes_ready
  wait_for "the default service account" 120 sa_ready
  log "$cfg $i: $PODS pods, no scheduler running"
  create_all "$MANIFESTS/pods-*.yaml"
  [ "$(kc get pods -n default --no-headers | wc -l)" -eq "$PODS" ] || { echo "not every pod was created" >&2; return 1; }
  cat >"$out/scheduler.yaml" <<EOF
apiVersion: kubescheduler.config.k8s.io/v1
kind: KubeSchedulerConfiguration
clientConnection:
  kubeconfig: $KUBECONFIG_FILE
$qps
leaderElection:
  leaderElect: false
EOF
  local ready="$out/watch.ready" start="$out/scheduler.start"
  rm -f -- "$ready" "$start"
  local fifo="$out/watch.fifo"
  rm -f -- "$fifo"; mkfifo "$fifo"
  python3 -I "$HERE/watch_binds.py" "$PODS" "$ready" "$start" "$STALL_S" "$out/result.json" <"$fifo" >"$out/watch.log" &
  local watcher=$!
  kc get pods -n default -w --request-timeout=0 --no-headers -o custom-columns=NAME:.metadata.name,NODE:.spec.nodeName >"$fifo" 2>"$out/watch.err" &
  local watch=$!
  wait_for "the watch to list every pod" 600 test -f "$ready"
  { echo "before:"; host_state; } >"$out/host.txt"
  python3 -I -c 'import time; print(time.monotonic())' >"$start.part" && mv "$start.part" "$start"
  "$BIN/kube-scheduler" --config "$out/scheduler.yaml" --secure-port 0 -v 1 >"$out/scheduler.log" 2>&1 &
  local sched=$!
  echo "$sched" >"$out/scheduler.pid"
  wait "$watcher" || true
  { echo "after:"; host_state; } >>"$out/host.txt"
  kill "$sched" 2>/dev/null || true
  wait "$sched" 2>/dev/null || true
  kill "$watch" 2>/dev/null || true
  wait "$watch" 2>/dev/null || true
  rm -f -- "$fifo"
  echo "bound_by_api: $(kc get pods -n default --no-headers -o custom-columns=NODE:.spec.nodeName | grep -vc '<none>')" >>"$out/host.txt"
  delete_cluster
  log "$cfg $i: $(cat "$out/result.json")"
}

hex() {
  local out="$ROOT/results/$STAMP"
  mkdir -p "$out"
  cat "$BIN/sched_kwok.commit" >"$out/hex.txt"
  { echo "before:"; host_state; } >>"$out/hex.txt"
  if [ -n "$LOCK" ]; then
    flock -w 3600 "$LOCK" "$BIN/sched_kwok" hex "$RUNS" "$HEX_SHARDS" | tee -a "$out/hex.txt"
  else
    "$BIN/sched_kwok" hex "$RUNS" "$HEX_SHARDS" | tee -a "$out/hex.txt"
  fi
  { echo "after:"; host_state; } >>"$out/hex.txt"
}

series() {
  local cfg i
  for i in $(seq 1 "$RUNS"); do
    for cfg in defaults raised; do
      if [ -n "$LOCK" ]; then
        flock -w 3600 "$LOCK" "$0" one "$cfg" "$i"
      else
        "$0" one "$cfg" "$i"
      fi
    done
  done
  python3 -I - "$ROOT/results/$STAMP" <<'EOF'
import json, os, sys
d = sys.argv[1]
for cfg in ("defaults", "raised"):
    rows = []
    for e in sorted(os.listdir(d)):
        if e.startswith(cfg + "-"):
            try:
                rows.append((e, json.load(open(os.path.join(d, e, "result.json")))))
            except OSError:
                rows.append((e, None))
    for e, r in rows:
        print(e, r)
    done = [r for _, r in rows if r and r.get("bound") == r.get("expected")]
    for key in ("pods_per_s_first_to_last", "pods_per_s_from_start"):
        rates = [r.get(key, 0) for r in done]
        print(cfg, "best", key, max(rates) if rates else None, "of", len(done), "runs that bound every pod")
EOF
}

export STAMP="${STAMP:-$(date -u +%Y%m%dT%H%M%SZ)}"
case "${1:-}" in
  prep) prep ;;
  one) one "$2" "$3" ;;
  series) series ;;
  hex) hex ;;
  *) echo "usage: run.sh prep | one <defaults|raised> <i> | series | hex" >&2; exit 64 ;;
esac
