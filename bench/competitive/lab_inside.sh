#!/usr/bin/env bash
# bench/competitive: the process-sandbox and microVM passes, inside the lab container (started by host.sh).
#   lab_inside.sh sandbox|vm
# Makes what the pass's driver needs, vacates the cgroup root to a leaf, then runs the driver
# (sandbox_bench.py or vm_bench.py) alone in a cgroup of its own, once per round, with its methods in a
# shuffled order each round so drift over the run favours none.
# Env: ROUNDS (2), ROUND_FIRST (1), N (10000), WARMUP (100), LIFECYCLES (1000), CAP (1000), BUDGET_MIB
# (4096), METHODS (all of the pass's), HEXBENCH_PHASES (all).
set -uo pipefail
PASS=${1:?sandbox or vm}
ROUNDS=${ROUNDS:-2}; ROUND_FIRST=${ROUND_FIRST:-1}; N=${N:-10000}; WARMUP=${WARMUP:-100}
LIFECYCLES=${LIFECYCLES:-1000}; CAP=${CAP:-1000}; BUDGET_MIB=${BUDGET_MIB:-4096}
OUT=/opt/out; CG=/sys/fs/cgroup

case $PASS in
  sandbox)
    DRIVER=sandbox_bench.py
    METHODS=${METHODS:-"hex bwrap nsjail firejail minijail nspawn srt codex"}
    # The minimal rootfs the chrooting tools take: symlinks into the read-only /usr they bind.
    R=/run/rootfs; mkdir -p "$R"/{usr,proc,dev,tmp,etc,sys}
    for l in bin lib lib64 sbin; do ln -sfn "usr/$l" "$R/$l"; done
    version() {
      case $1 in
        hex) /opt/bin/hex --version 2>&1 | head -1 ;;
        bwrap) bwrap --version 2>&1 | head -1 ;;
        nsjail) echo "3.6 (source)" ;;
        firejail) firejail --version 2>&1 | head -1 ;;
        minijail) echo "linux-v2026.05.18 (source)" ;;
        nspawn) systemd-nspawn --version 2>&1 | head -1 ;;
        srt) npm ls -g @anthropic-ai/sandbox-runtime 2>&1 | tail -2 | head -1 ;;
        codex) codex --version 2>&1 | head -1 ;;
      esac
    } ;;
  vm)
    DRIVER=vm_bench.py
    METHODS=${METHODS:-"firecracker cloud-hypervisor"}
    [ -c /dev/kvm ] || { echo "refusing: the lab container has no /dev/kvm" >&2; exit 2; }
    # The guest: the kernel host.sh fetched, and an uncompressed initramfs holding the static init and
    # the console it writes to.
    V=/run/vm; mkdir -p "$V/root/dev"
    gcc -static -Os -o "$V/root/init" /opt/lab/vm/init.c || exit 2
    mknod -m 600 "$V/root/dev/console" c 5 1 || exit 2
    ( cd "$V/root" && printf '%s\n' init dev dev/console | cpio -o -H newc --quiet ) > "$V/init.cpio" || exit 2
    ln -sfn /opt/vm/vmlinux "$V/vmlinux"
    { echo "vmlinux $(sha256sum /opt/vm/vmlinux | cut -c1-64)"; echo "init.cpio $(sha256sum "$V/init.cpio" | cut -c1-64)"
    } >> "$OUT/vm_guest.txt"
    version() {
      case $1 in
        firecracker) /opt/bin/firecracker --version 2>&1 | head -1 ;;
        cloud-hypervisor) /opt/bin/cloud-hypervisor --version 2>&1 | head -1 ;;
      esac
    } ;;
  *) echo "refusing: no pass named $PASS" >&2; exit 2 ;;
esac

# cgroup v2's no-internal-processes rule: the namespace root hands controllers down only once it holds no
# process, so everything already here moves to a leaf.
mkdir -p "$CG/lab"
for p in $(cat "$CG/cgroup.procs"); do echo "$p" > "$CG/lab/cgroup.procs" 2>/dev/null || true; done
echo "+cpu +io +memory +pids" > "$CG/cgroup.subtree_control"

for m in $METHODS; do printf '%s\t%s\n' "$m" "$(version "$m")"; done >> "$OUT/${PASS}_versions.tsv"

for round in $(seq "$ROUND_FIRST" $((ROUND_FIRST + ROUNDS - 1))); do
  # A cgroup of its own per round: once a driver hands controllers down from it, cgroup v2 admits no
  # process to it again.
  mkdir "$CG/$PASS$round"
  # shellcheck disable=SC2046 # the shuffled methods are separate arguments
  ( echo "$BASHPID" > "$CG/$PASS$round/cgroup.procs" \
    && exec python3 -I "/opt/lab/$DRIVER" "$OUT/$PASS.r$round.json" "$N" "$WARMUP" "$LIFECYCLES" \
         "$CAP" "$BUDGET_MIB" $(printf '%s\n' $METHODS | shuf) )
done
echo done > "$OUT/$PASS.done"
