#!/usr/bin/env bash
# bench/competitive/host.sh: Hex's competitive baseline (goal 3). Runs on the bench host (fedora) only,
# in its bench-calendar slot, under a systemd user unit so it outlives the shell that starts it:
#   systemd-run --user --unit=hexbench --collect bash bench/competitive/host.sh <hex release binary>
# Env (defaults): CPUSET 4-31, ROUNDS 3, N 10000, WARMUP 100, N_OVERRIDE "runsc:3000", LIFECYCLES 2000,
# CAP 1000, BUDGET_MIB 8192 (OCI density), BOX_BUDGET_MIB 4096 (box, sandbox and VM density), PHASES "oci box"
# (add "sandbox" for the process-sandbox pass and "vm" for the microVM pass; METHODS picks their methods; add
# "boxseam" for the process class through the Runtime seam, with SEAM_BIN naming seam_box's release binary).
# A later pass in the same run directory: OUT_DIR names it, ROUND_FIRST (1) numbers its rounds after the
# ones there, HEXBENCH_PHASES picks the drivers' phases (all), and FETCH=0 keeps the competitors' binaries
# already in $BASE/bin (their sha256 is recorded) rather than resolving each project's latest release again.
# Another host only by name (BENCH_HOST_OK=<its hostname>): Ada's call for E2B's same-hardware rounds on adacachy
# (2026-10-24). LOAD_GATE=0 skips the wait for load under 2: for those rounds, and for a density-only pass
# (HEXBENCH_PHASES=density), whose held memory doesn't depend on load. env.txt records which, and the load.
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
HEX=$(realpath "${1:?hex release binary}")
case "$(hostname)" in
  fedora*) ;;
  "${BENCH_HOST_OK:-}") ;;
  *) echo "refusing: the bench host is fedora, not $(hostname) (BENCH_HOST_OK=$(hostname) runs it here)" >&2; exit 2 ;;
esac
export CPUSET=${CPUSET:-4-31} ROUNDS=${ROUNDS:-3} N=${N:-10000} WARMUP=${WARMUP:-100}
export N_OVERRIDE=${N_OVERRIDE:-runsc:3000} LIFECYCLES=${LIFECYCLES:-2000} CAP=${CAP:-1000} BUDGET_MIB=${BUDGET_MIB:-8192}
BOX_BUDGET_MIB=${BOX_BUDGET_MIB:-4096}; PHASES=${PHASES:-"oci box"}
export ROUND_FIRST=${ROUND_FIRST:-1} HEXBENCH_PHASES=${HEXBENCH_PHASES:-}
BASE=${HEX_BENCH:-$HOME/.cache/hex-bench}
if [[ " $PHASES " == *" boxseam "* ]]; then
  SEAM=$(realpath "${SEAM_BIN:?SEAM_BIN: the seam_box binary, from cargo bench --no-run -p hex-bench --bench seam_box}")
fi
# OUT_DIR adds a pass (e.g. RUNTIMES=runsc PHASES=oci) to an existing run directory.
OUT=${OUT_DIR:-"$BASE/out/$(date -u +%Y%m%dT%H%M%SZ)"}
mkdir -p "$OUT" "$BASE/bin"

# CLAUDE.md §7: load average under 2. Wait up to 15 minutes for it; refuse rather than measure noise.
[ "${LOAD_GATE:-1}" = 0 ] || for i in $(seq 1 90); do
  load=$(cut -d' ' -f1 /proc/loadavg)
  awk -v l="$load" 'BEGIN { exit !(l < 2) }' && break
  [ "$i" = 90 ] && { echo "refusing: 1-minute load average $load is not under 2" >&2; exit 3; }
  sleep 10
done

{ echo "--- pass started $(date -u +%FT%TZ): PHASES=$PHASES RUNTIMES=${RUNTIMES:-all}"; echo "host: $(hostname)"; echo "date: $(date -u +%FT%TZ)"; echo "kernel: $(uname -r)"
  if [ "${LOAD_GATE:-1}" = 0 ]; then
    if [ "$HEXBENCH_PHASES" = density ]; then
      echo "load gate: skipped (LOAD_GATE=0) for a density-only pass: held memory doesn't depend on other people's CPU load (platform-rounds/README.md, density-only rounds)"
    else
      echo "load gate: skipped (LOAD_GATE=0), so this pass is not of record by CLAUDE.md §7"
    fi
  fi
  echo "cpu: $(lscpu | sed -n 's/^Model name: *//p') ($(nproc) cpus), pinned to $CPUSET"
  echo "memory: $(free -g | awk '/^Mem/ {print $2}') GiB"; echo "load at start: $(cat /proc/loadavg)"
  echo "hex: $HEX ($(sha256sum "$HEX" | cut -c1-16))"; [ -z "${SEAM:-}" ] || echo "seam_box: $SEAM ($(sha256sum "$SEAM" | cut -c1-16))"
  echo "params: ROUNDS=$ROUNDS ROUND_FIRST=$ROUND_FIRST N=$N WARMUP=$WARMUP N_OVERRIDE=$N_OVERRIDE LIFECYCLES=$LIFECYCLES CAP=$CAP BUDGET_MIB=$BUDGET_MIB BOX_BUDGET_MIB=$BOX_BUDGET_MIB HEXBENCH_PHASES=${HEXBENCH_PHASES:-all}"
} >> "$OUT/env.txt"

if [ "${FETCH:-1}" = 0 ]; then
  # Every file, runsc's gvisor-bin/ Sentry set included: a bare glob would hand sha256sum a directory.
  (cd "$BASE/bin" && find . -type f -print0 | sort -z | xargs -0 sha256sum) > "$OUT/versions.$(date -u +%H%M%S).kept.sha256"
else
  python3 -I "$HERE/fetch.py" "$BASE" > "$OUT/versions.$(date -u +%H%M%S).json"
fi
cp "$HEX" "$BASE/bin/hex"
# The oci class through the Runtime seam (seam_oci): SEAM_OCI_BIN names its release binary, staged
# into the oci pass's container as /opt/bin/seam_oci, where oci_inside.sh runs it as root. Its sha256
# is recorded beside hex's.
if [ -n "${SEAM_OCI_BIN:-}" ]; then
  cp "$(realpath "$SEAM_OCI_BIN")" "$BASE/bin/seam_oci"
  echo "seam_oci: $BASE/bin/seam_oci ($(sha256sum "$BASE/bin/seam_oci" | cut -c1-16))" >> "$OUT/env.txt"
fi

NAMES=()  # the bench's containers, removed however this script ends
cleanup() { for n in "${NAMES[@]}"; do docker rm -f "$n" >/dev/null 2>&1 || true; done; }
trap cleanup EXIT

if [[ " $PHASES " == *" oci "* ]]; then
  NAME="hexbench-oci-$$"; NAMES+=("$NAME")
  KRUN=()  # krun (crun's libkrun handler) runs only when RUNTIMES names it; its libraries mount beside it
  if [[ " ${RUNTIMES:-} " == *" krun "* ]]; then
    [ -x "$BASE/bin/krun" ] || python3 -I "$HERE/fetch.py" "$BASE" krun > "$OUT/versions.$(date -u +%H%M%S).krun.json"
    [ -d "$BASE/krun/lib" ] || { echo "refusing: no $BASE/krun/lib (fetch.py's record is in $OUT)" >&2; exit 4; }
    KRUN=(-v "$BASE/krun:/opt/krun:ro")
  fi
  # libcrun in process (libcrun/) runs when RUNTIMES is oci_inside.sh's default or names it: its driver is
  # built from crun's pinned source in an image of its own (the one step here that uses the network), and
  # copied out to $BASE/bin, where the pass's container finds it.
  if [ -z "${RUNTIMES:-}" ] || [[ " $RUNTIMES " == *" libcrun "* ]]; then
    REC="$OUT/versions.$(date -u +%H%M%S).crun-src.json"
    python3 -I "$HERE/fetch.py" "$BASE" crun-src > "$REC"
    SRC=$(python3 -I -c 'import json,sys; r=json.load(open(sys.argv[1]))["crun-src"]; print(r.get("path", ""))' "$REC")
    [ -n "$SRC" ] || { echo "refusing: crun's source didn't fetch ($REC)" >&2; exit 4; }
    CTX=$(mktemp -d "$BASE/libcrun-image.XXXXXX")
    cp "$SRC" "$CTX/crun.tar.gz" && cp "$HERE/libcrun/Dockerfile" "$HERE/libcrun/driver.c" "$HERE/libcrun/build.sh" "$CTX/"
    docker build --progress=plain -t hexbench/libcrun:1 "$CTX" > "$OUT/libcrun-image.log" 2>&1 \
      || { rm -rf -- "${CTX:?}"; echo "refusing: the libcrun driver didn't build ($OUT/libcrun-image.log)" >&2; exit 5; }
    rm -rf -- "${CTX:?}"
    CID=$(docker create hexbench/libcrun:1)
    docker cp "$CID:/usr/local/bin/libcrun-driver" "$BASE/bin/libcrun-driver" \
      || { docker rm "$CID" >/dev/null; echo "refusing: the libcrun driver didn't copy out of its image" >&2; exit 5; }
    docker rm "$CID" >/dev/null
    echo "libcrun $(docker image inspect hexbench/libcrun:1 --format '{{.Id}}') driver $(sha256sum "$BASE/bin/libcrun-driver" | cut -c1-16)" \
      >> "$OUT/lab-image.ids"
  fi
  # One privileged container, private cgroup namespace, no network; host /usr read-only provides the
  # workloads and python3. --memory bounds the density phase on top of its own budget.
  SEAM_OCI_ENV=(); [ -x "$BASE/bin/seam_oci" ] && SEAM_OCI_ENV=(-e "SEAM_OCI=/opt/bin/seam_oci")
  timeout 36000 docker run --rm --name "$NAME" --label hex.bench=competitive --cpuset-cpus="$CPUSET" --memory=40g \
    --privileged --security-opt label=disable --cgroupns=private --network=none \
    -v /usr:/usr:ro -v "$BASE/bin:/opt/bin:ro" "${KRUN[@]}" -v "$HERE:/opt/lab:ro" -v "$OUT:/opt/out" \
    -e RUNTIMES -e ROUNDS -e ROUND_FIRST -e N -e WARMUP -e N_OVERRIDE -e LIFECYCLES -e CAP -e BUDGET_MIB -e HEXBENCH_PHASES \
    -e STANDIN "${SEAM_OCI_ENV[@]}" registry.fedoraproject.org/fedora:44 bash /opt/lab/oci_inside.sh 2>&1 | tee "$OUT/oci.log"
fi

# The lab image (sandboxes/Dockerfile): Fedora 44 with each process sandbox pinned, codex at a pinned tag
# (sha256-checked by fetch.py), and the compiler the VM pass builds its guest's init with. The build is the
# one step that uses the network; the passes have none, and no host /usr: the image's own tools are run.
lab_image() {
  [ -x "$BASE/bin/codex" ] || python3 -I "$HERE/fetch.py" "$BASE" codex > "$OUT/versions.$(date -u +%H%M%S).codex.json"
  CTX=$(mktemp -d "$BASE/lab-image.XXXXXX")
  cp "$HERE/sandboxes/Dockerfile" "$CTX/" && cp "$BASE/bin/codex" "$CTX/codex"
  # Its own command, so a failed build stops this script (set -e doesn't see one inside an echo's
  # substitution), with the whole build's output kept beside the results.
  docker build --progress=plain -t hexbench/sandboxes:1 "$CTX" > "$OUT/lab-image.log" 2>&1 \
    || { rm -rf -- "${CTX:?}"; echo "refusing: the lab image didn't build ($OUT/lab-image.log)" >&2; exit 5; }
  rm -rf -- "${CTX:?}"
  { echo "base $(docker image inspect registry.fedoraproject.org/fedora:44 --format '{{.Id}}')"
    echo "lab $(docker image inspect hexbench/sandboxes:1 --format '{{.Id}}')"; } >> "$OUT/lab-image.ids"
}

# lab <pass> [docker run mounts]: one pass of lab_inside.sh in a privileged lab container.
lab() {
  local pass=$1; shift
  NAMES+=("hexbench-$pass-$$")
  timeout 36000 docker run --rm --name "hexbench-$pass-$$" --label hex.bench=competitive --cpuset-cpus="$CPUSET" \
    --memory=40g --privileged --security-opt label=disable --cgroupns=private --network=none \
    -v "$HERE:/opt/lab:ro" -v "$OUT:/opt/out" "$@" \
    -e ROUNDS -e ROUND_FIRST -e N -e WARMUP -e LIFECYCLES -e CAP -e BUDGET_MIB="$BOX_BUDGET_MIB" -e METHODS \
    -e HEXBENCH_PHASES hexbench/sandboxes:1 bash /opt/lab/lab_inside.sh "$pass" 2>&1 | tee "$OUT/$pass.log"
}

if [[ " $PHASES " == *" sandbox "* || " $PHASES " == *" vm "* ]]; then lab_image; fi
if [[ " $PHASES " == *" sandbox "* ]]; then lab sandbox -v "$BASE/bin/hex:/opt/bin/hex:ro"; fi
if [[ " $PHASES " == *" vm "* ]]; then
  # Firecracker and Cloud Hypervisor at pinned releases, and the guest kernel Firecracker's guide boots
  # (fetch.py). A missing one is refused here: docker would mount an empty directory in its place.
  [ -x "$BASE/bin/firecracker" ] && [ -x "$BASE/bin/cloud-hypervisor" ] && [ -f "$BASE/vm/vmlinux" ] \
    || python3 -I "$HERE/fetch.py" "$BASE" firecracker cloud-hypervisor guest-kernel > "$OUT/versions.$(date -u +%H%M%S).vm.json"
  for f in bin/firecracker bin/cloud-hypervisor vm/vmlinux; do
    [ -f "$BASE/$f" ] || { echo "refusing: no $BASE/$f (fetch.py's record is in $OUT)" >&2; exit 4; }
  done
  lab vm -v "$BASE/bin/firecracker:/opt/bin/firecracker:ro" -v "$BASE/bin/cloud-hypervisor:/opt/bin/cloud-hypervisor:ro" \
    -v "$BASE/vm:/opt/vm:ro"
fi

if [[ " $PHASES " == *" box "* ]]; then
  for round in $(seq "$ROUND_FIRST" $((ROUND_FIRST + ROUNDS - 1))); do
    # A delegated user service: box_bench.py makes its worker cgroups under it (Delegate=yes).
    timeout 36000 systemd-run --user --unit="hexbench-box-$$-$round" --property=Delegate=yes --wait --collect --pipe \
      --setenv=HEXBENCH_PHASES="$HEXBENCH_PHASES" \
      taskset -c "$CPUSET" python3 -I "$HERE/box_bench.py" "$OUT/box.r$round.json" "$N" "$WARMUP" "$LIFECYCLES" \
      "$CAP" "$BOX_BUDGET_MIB" "$BASE/bin/hex" 2>&1 | tee -a "$OUT/box.log"
  done
fi

if [[ " $PHASES " == *" boxseam "* ]]; then
  for round in $(seq "$ROUND_FIRST" $((ROUND_FIRST + ROUNDS - 1))); do
    # The node agent in seam_box's own process, in a delegated user service as box_bench.py runs; it makes
    # its agent leaf and box root under the service's cgroup.
    timeout 36000 systemd-run --user --unit="hexbench-boxseam-$$-$round" --property=Delegate=yes --wait --collect --pipe \
      --setenv=HEXBENCH_PHASES="$HEXBENCH_PHASES" \
      taskset -c "$CPUSET" "$SEAM" "$OUT/boxseam.r$round.json" "$N" "$WARMUP" "$LIFECYCLES" "$CAP" "$BOX_BUDGET_MIB" \
      2>&1 | tee -a "$OUT/boxseam.log"
  done
fi

echo "load at end: $(cat /proc/loadavg)" >> "$OUT/env.txt"
python3 -I "$HERE/summarize.py" "$OUT" > "$OUT/table.md"
echo "results: $OUT"
