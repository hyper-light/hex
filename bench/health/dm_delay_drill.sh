#!/usr/bin/env bash
# AC-T7.10.3's drill (docs/design/failure-isolation.md F3): a node whose disk stalls is marked and
# drained before its work fails. As root in a privileged container: a dm-delay device over a loop file
# holds the node's state disk (ext4); `disk_drill` runs work units on it that fail when a write isn't
# done by its deadline, and the node's disk probe beside them; the device is stalled (reloaded with a
# 60 s delay on every I/O, dm-delay's documented use, kernel Documentation/admin-guide/device-mapper/
# delay.rst) and the drill records when the node was marked, when it was drained and whether any work
# failed. Run twice: the probe on (expect 0 failed), and off (expect failures: non-vacuity).
# Usage: dm_delay_drill.sh <disk_drill binary> <record file>
# Env: BENCH_MEMORY (8g), BENCH_OOM (1000), BENCH_CPUS (4-15), IMAGE (fedora:44), LAB (~/.cache dir).
set -euo pipefail
DRILL=$(realpath "${1:?disk_drill binary}")
RECORD=${2:?record file}
IMAGE=${IMAGE:-registry.fedoraproject.org/fedora:44}
LAB=${LAB:-$HOME/.cache/hex-t7/sched-g5/drill-lab}
mkdir -p "${LAB:?}"
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
COMMIT=$(git -C "$HERE" rev-parse --short HEAD)
DIRTY=$(git -C "$HERE" status --porcelain | wc -l)

out=$(docker run --rm --privileged --security-opt label=disable --network=none \
  --memory "${BENCH_MEMORY:-8g}" --oom-score-adj "${BENCH_OOM:-1000}" --cpuset-cpus "${BENCH_CPUS:-4-15}" \
  -v /usr:/usr:ro -v /dev:/dev -v "$DRILL:/opt/disk_drill:ro" -v "$LAB:/lab" \
  -e DM_DISABLE_UDEV=1 "$IMAGE" bash -c '
set -euo pipefail
name=hexdrill-$$
img=/lab/disk-$$.img
truncate -s 512M "$img"
loop=$(losetup -f --show "$img")
sectors=$(blockdev --getsz "$loop")
cleanup() {
  dmsetup suspend --noflush "$name" 2>/dev/null || true
  dmsetup reload "$name" --table "0 $sectors delay $loop 0 0" 2>/dev/null || true
  dmsetup resume "$name" 2>/dev/null || true
  umount /mnt/state 2>/dev/null || true
  dmsetup remove "$name" 2>/dev/null || true
  losetup -d "$loop" 2>/dev/null || true
  rm -f "${img:?}"
}
trap cleanup EXIT
dmsetup create "$name" --table "0 $sectors delay $loop 0 0"
mkfs.ext4 -q "/dev/mapper/$name"
mkdir -p /mnt/state /lab/spare
mount "/dev/mapper/$name" /mnt/state
cat >/tmp/set_delay <<EOF
#!/usr/bin/bash
set -e
dmsetup reload $name --table "0 $sectors delay $loop 0 \$1"
dmsetup suspend --noflush --nolockfs $name
dmsetup resume $name
EOF
chmod +x /tmp/set_delay
for probe in on off; do
  find /mnt/state /lab/spare -mindepth 1 -delete
  /opt/disk_drill --state /mnt/state --spare /lab/spare --probe "$probe" \
    --stall-cmd "/tmp/set_delay 60000" --resume-cmd "/tmp/set_delay 0" \
    --units 8 --deadline-ms 10000 --period-ms 100 --warm-s 10 --after-s 25
done
')
echo "$out"
on=$(grep "probe=on" <<<"$out")
off=$(grep "probe=off" <<<"$out")
failed_on=$(sed -n "s/.* failed=\([0-9]*\).*/\1/p" <<<"$on")
failed_off=$(sed -n "s/.* failed=\([0-9]*\).*/\1/p" <<<"$off")
drained_on=$(sed -n "s/.* drained_ms=\([0-9a-z]*\).*/\1/p" <<<"$on")
{
  echo "# AC-T7.10.3 dm-delay drill, $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo
  echo "- Commit: $COMMIT (dirty files: $DIRTY); host: $(hostname), $(uname -r)"
  echo "- Hardware: $(lscpu | sed -n 's/^Model name: *//p'), $(nproc) CPUs; drill on CPUs ${BENCH_CPUS:-4-15}"
  echo "- Load: $(uptime)"
  echo "- Co-tenants: $(docker ps --format '{{.Names}}' | tr '\n' ' ')"
  echo "- Device: dm-delay over a 512 MiB loop file, ext4; stall: every I/O delayed 60 s"
  echo "- Work: 8 units, a 4 KiB write and fdatasync each 50 ms, each write due in 10,000 ms"
  echo
  echo '```'
  echo "$on"
  echo "$off"
  echo '```'
  echo
  echo "Probe on: failed=$failed_on, drained ${drained_on} ms after the stall (deadline 10000 ms)."
  echo "Probe off: failed=$failed_off (non-vacuity: the same stall fails work)."
} >"$RECORD"
[ "$failed_on" = 0 ] && [ "$drained_on" != none ] && [ "$drained_on" -lt 10000 ] && [ "$failed_off" -gt 0 ]
