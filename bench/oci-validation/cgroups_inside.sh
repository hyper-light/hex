#!/usr/bin/env bash
# Inside the lab container (see cgroups.sh). Each check prints PASS or FAIL with what it saw.
set -uo pipefail
export HEX_OCI_ROOT=/run/hex-oci
CG=/sys/fs/cgroup
W=/run/lab; mkdir -p "$W/out"
fails=0
pass() { echo "PASS $*"; }
fail() { echo "FAIL $*"; fails=$((fails + 1)); }
check() { # check <what> <want> <got>
  if [ "$2" = "$3" ]; then pass "$1 = $3"; else fail "$1: want $2, got $3"; fi
}

# cgroup v2's no-internal-processes rule: the namespace root hands controllers down only once it
# holds no process, so this shell and everything else moves to a leaf, as the validation lab does.
mkdir -p "$CG/lab"
for p in $(cat "$CG/cgroup.procs"); do echo "$p" > "$CG/lab/cgroup.procs" 2>/dev/null || true; done

# bundle <dir> <process args json> <extra linux json> <extra mounts json>
bundle() {
  local B=$1; mkdir -p "$B/rootfs"/{usr,proc,dev,tmp,out}
  ( cd "$B/rootfs" && ln -sfn usr/bin bin && ln -sfn usr/lib lib && ln -sfn usr/lib64 lib64 && ln -sfn usr/sbin sbin )
  cat > "$B/config.json" <<JSON
{ "ociVersion": "1.3.0", "root": {"path": "rootfs"}, "hostname": "lab",
  "process": {"cwd": "/", "args": $2, "env": ["PATH=/usr/bin:/usr/sbin"], "user": {"uid": 0, "gid": 0},
              "capabilities": {"bounding": ["CAP_MKNOD","CAP_KILL","CAP_DAC_OVERRIDE"],
                               "effective": ["CAP_MKNOD","CAP_KILL","CAP_DAC_OVERRIDE"],
                               "permitted": ["CAP_MKNOD","CAP_KILL","CAP_DAC_OVERRIDE"]}},
  "mounts": [{"destination": "/usr", "type": "bind", "source": "/usr", "options": ["rbind","ro"]},
             {"destination": "/proc", "type": "proc", "source": "proc"},
             {"destination": "/dev", "type": "tmpfs", "source": "tmpfs", "options": ["nosuid","mode=755","size=65536k"]},
             {"destination": "/tmp", "type": "tmpfs", "source": "tmpfs"}
             $4],
  "linux": {"namespaces": [{"type": "pid"}, {"type": "mount"}, {"type": "ipc"}, {"type": "uts"}, {"type": "network"}, {"type": "cgroup"}]
            $3} }
JSON
}
OUT=", {\"destination\": \"/out\", \"type\": \"bind\", \"source\": \"$W/out\", \"options\": [\"rbind\",\"rw\"]}"
DEVICES='"devices": [{"allow": false, "access": "rwm"},
  {"allow": true, "type": "c", "major": 1, "minor": 3, "access": "rwm"},
  {"allow": true, "type": "c", "major": 1, "minor": 5, "access": "rwm"},
  {"allow": true, "type": "c", "major": 1, "minor": 7, "access": "rwm"},
  {"allow": true, "type": "c", "major": 1, "minor": 8, "access": "rwm"},
  {"allow": true, "type": "c", "major": 1, "minor": 9, "access": "rwm"},
  {"allow": true, "type": "c", "major": 5, "minor": 0, "access": "rwm"}]'

# 1. Limits in force, the container inside its cgroup and its namespace rooted there, devices enforced.
bundle "$W/limits" '["sh", "-c", "cat /proc/self/cgroup > /out/cg; head -c 4 /dev/zero | wc -c > /out/zero; (mknod /tmp/disk b 8 0 && head -c 1 /tmp/disk) > /out/disk 2>&1; echo $? > /out/disk.rc; exec sleep infinity"]' \
  ", \"resources\": {\"memory\": {\"limit\": 268435456}, \"pids\": {\"limit\": 64},
     \"cpu\": {\"shares\": 512, \"quota\": 50000, \"period\": 100000}, $DEVICES}" "$OUT"
/opt/hex oci create --bundle "$W/limits" --pid-file "$W/limits.pid" limits || fail "create limits"
check "memory.max" 268435456 "$(cat $CG/limits/memory.max 2>&1)"
check "pids.max" 64 "$(cat $CG/limits/pids.max 2>&1)"
check "cpu.weight (shares 512, crun's conversion)" 59 "$(cat $CG/limits/cpu.weight 2>&1)"
check "cpu.max" "50000 100000" "$(cat $CG/limits/cpu.max 2>&1)"
pid=$(cat "$W/limits.pid")
check "the container's cgroup, seen from outside" "0::/limits" "$(cat /proc/$pid/cgroup)"
check "the shim stays outside the container's cgroup" 1 "$(wc -l < $CG/limits/cgroup.procs)"
/opt/hex oci start limits || fail "start limits"
for _ in $(seq 1 50); do [ -s "$W/out/disk.rc" ] && break; sleep 0.1; done
check "the container's cgroup, seen from inside (its namespace's root)" "0::/" "$(cat $W/out/cg 2>&1)"
check "an allowed device (/dev/zero) reads" 4 "$(tr -d ' ' < $W/out/zero 2>&1)"
if [ "$(cat $W/out/disk.rc 2>&1)" != 0 ]; then pass "a denied device (block 8:0) is refused: $(head -c 120 $W/out/disk)"; else fail "block 8:0 was readable"; fi
/opt/hex oci delete --force limits || fail "delete limits"
[ -d "$CG/limits" ] && fail "delete left the cgroup" || pass "delete removed the cgroup"

# 1b. No resources at all: the default device policy, as under runc (crun attaches no device program
# to a spec without devices). The supplied devices work (the init made /dev/null and the rest), a
# disk doesn't.
rm -f "$W/out/"*
bundle "$W/plain" '["sh", "-c", "head -c 4 /dev/zero | wc -c > /out/zero; (mknod /tmp/disk b 8 0 && head -c 1 /tmp/disk) > /out/disk 2>&1; echo $? > /out/disk.rc"]' "" "$OUT"
/opt/hex oci create --bundle "$W/plain" plain && /opt/hex oci start plain || fail "create/start plain"
for _ in $(seq 1 50); do [ -s "$W/out/disk.rc" ] && break; sleep 0.1; done
check "no resources: /dev/zero reads" 4 "$(tr -d ' ' < $W/out/zero 2>&1)"
if [ "$(cat $W/out/disk.rc 2>&1)" != 0 ]; then pass "no resources: a disk is still refused: $(head -c 120 $W/out/disk)"; else fail "no resources: block 8:0 was readable"; fi
/opt/hex oci delete --force plain || fail "delete plain"

# 1c. Containers with one device policy share one device program (decision 2; oci/devshare.rs): the
# second takes the first's, so the hints keep the first's id (only a miss rewrites them). Once the
# first is deleted the second still refuses a disk and reads /dev/zero: the program lives on in the
# second's cgroup and decides for it as its own copy would.
hint_id() { grep -o '"id":[0-9]*' "$HEX_OCI_ROOT/.device-programs.json" 2>/dev/null | head -1 | cut -d: -f2; }
bundle "$W/share1" '["sleep", "infinity"]' "" ""
bundle "$W/share2" '["sleep", "infinity"]' "" ""
/opt/hex oci create --bundle "$W/share1" --pid-file "$W/share1.pid" share1 && /opt/hex oci start share1 || fail "create/start share1"
first=$(hint_id)
# The started program holds its stdio and no descriptor of the runtime's (crun's holds 0, 1 and 2 alone):
# read from outside, so the listing opens nothing of its own in the program.
check "the program's descriptors, once started" "0 1 2" "$(ls /proc/$(cat $W/share1.pid)/fd | sort -n | xargs)"
if [ -n "$first" ]; then pass "the first container's device program is remembered (id $first)"; else fail "no device program hint after the first container"; fi
/opt/hex oci create --bundle "$W/share2" share2 && /opt/hex oci start share2 || fail "create/start share2"
check "a second container with the same device policy takes the first's program" "$first" "$(hint_id)"
/opt/hex oci delete --force share1 || fail "delete share1"
# One exec reports all three, so an exec that never ran its program can't pass: /dev/zero read, the
# node made (the default policy allows mknod, as runc's), and reading it refused (head exits 1).
got=$(/opt/hex oci exec share2 sh -c 'echo "zero $(head -c 4 /dev/zero | wc -c | tr -d " ")"; mknod /tmp/disk b 8 0 && echo made; head -c 1 /tmp/disk > /dev/null 2>&1; echo "read $?"' 2>&1)
case "$got" in
  *"zero 4"*"made"*"read 1"*) pass "the sharing container, the first gone, reads /dev/zero and refuses a disk: $(echo $got)" ;;
  *) fail "the sharing container once the first was deleted: $(echo $got)" ;;
esac
/opt/hex oci delete --force share2 || fail "delete share2"

# 1d. A seccomp profile binds the program, and no_new_privs is what the spec's noNewPrivileges says
# (config.md, "noNewPrivileges"). Without it, the filter is loaded while the init still holds
# CAP_SYS_ADMIN, as runc loads it (standard_init_linux.go), so the program runs filtered with
# no_new_privs unset. The profile refuses mknodat, so a refused mknod shows the filter in force.
SECCOMP='"seccomp": {"defaultAction": "SCMP_ACT_ALLOW", "syscalls": [{"names": ["mknodat"], "action": "SCMP_ACT_ERRNO", "errnoRet": 1}]}'
for nnp in false true; do
  rm -f "$W/out/"*
  bundle "$W/nnp-$nnp" '["sh", "-c", "grep -E \"^(NoNewPrivs|Seccomp):\" /proc/self/status | tr -s \"\\t\\n \" \"  \" > /out/status; mknod /tmp/null c 1 3 2> /dev/null; echo $? > /out/mknod.rc"]' ", $SECCOMP" "$OUT"
  python3 - "$W/nnp-$nnp/config.json" "$nnp" <<'PY'
import json, sys
path, nnp = sys.argv[1], sys.argv[2] == "true"
spec = json.load(open(path))
spec["process"]["noNewPrivileges"] = nnp
json.dump(spec, open(path, "w"))
PY
  /opt/hex oci create --bundle "$W/nnp-$nnp" "nnp-$nnp" && /opt/hex oci start "nnp-$nnp" || fail "create/start nnp-$nnp"
  for _ in $(seq 1 50); do [ -s "$W/out/mknod.rc" ] && break; sleep 0.1; done
  want=0; [ "$nnp" = true ] && want=1
  check "noNewPrivileges $nnp: the program's no_new_privs and seccomp mode" "NoNewPrivs: $want Seccomp: 2 " "$(cat $W/out/status 2>&1)"
  if [ "$(cat $W/out/mknod.rc 2>&1)" = 1 ]; then pass "noNewPrivileges $nnp: the filter refuses mknod"; else fail "noNewPrivileges $nnp: mknod returned $(cat $W/out/mknod.rc 2>&1), not refused"; fi
  /opt/hex oci delete --force "nnp-$nnp" || fail "delete nnp-$nnp"
done

# 2. kill --all reaches a process that left the init's process group (setsid).
bundle "$W/escape" '["sh", "-c", "setsid sleep infinity & exec sleep infinity"]' "" ""
/opt/hex oci create --bundle "$W/escape" escape && /opt/hex oci start escape || fail "create/start escape"
for _ in $(seq 1 50); do [ "$(wc -l < $CG/escape/cgroup.procs)" -ge 3 ] && break; sleep 0.1; done
/opt/hex oci kill --all escape KILL || fail "kill --all"
for _ in $(seq 1 50); do grep -q "populated 0" "$CG/escape/cgroup.events" && break; sleep 0.1; done
check "kill --all emptied the cgroup" "populated 0" "$(grep populated $CG/escape/cgroup.events)"
/opt/hex oci delete escape || fail "delete escape"
[ -d "$CG/escape" ] && fail "delete left escape's cgroup" || pass "delete removed escape's cgroup"

# 3. Refusals leave nothing behind: a field cgroup v2 can't hold, and an occupied cgroup.
bundle "$W/kernel" '["true"]' ', "resources": {"memory": {"kernel": 1048576}}' ""
if err=$(/opt/hex oci create --bundle "$W/kernel" kernel 2>&1); then fail "memory.kernel was accepted"
else case "$err" in *memory.kernel*) pass "memory.kernel refused by name: $err" ;; *) fail "memory.kernel refused without naming it: $err" ;; esac; fi
[ -d "$CG/kernel" ] && fail "a refused create left its cgroup" || pass "a refused create left no cgroup"
[ -e "$HEX_OCI_ROOT/kernel/state.json" ] && fail "a refused create left state" || pass "a refused create left no state"
bundle "$W/occupied" '["true"]' ', "cgroupsPath": "/lab"' ""
if err=$(/opt/hex oci create --bundle "$W/occupied" occupied 2>&1); then fail "an occupied cgroup was accepted"
else pass "an occupied cgroup refused: $err"; fi

# 4. The container is born in its cgroup: create migrates no process into it. A write to cgroup.procs is
# a migration, which the kernel serializes on a lock whose writer can wait a whole RCU grace period
# (docs/design/runtime.md); clone3(CLONE_INTO_CGROUP) starts the init there instead. The oracle is the
# kernel's own cgroup_attach_task tracepoint, read in a trace instance of this lab's own.
T=/sys/kernel/tracing
mountpoint -q "$T" || mount -t tracefs nodev "$T"
I="$T/instances/hexlab-$$"
if mkdir "$I" 2>/dev/null; then
  echo 1 > "$I/events/cgroup/cgroup_attach_task/enable"
  bundle "$W/born" '["sleep", "infinity"]' "" ""
  /opt/hex oci create --bundle "$W/born" born || fail "create born"
  echo 0 > "$I/events/cgroup/cgroup_attach_task/enable"
  check "migrations into the new container's cgroup during create" 0 "$(grep -c 'dst_path=[^ ]*/born ' "$I/trace")"
  rmdir "$I"
  /opt/hex oci delete --force born || fail "delete born"
else
  fail "no trace instance under $T, so migrations can't be seen"
fi

# 5. `hex oci exec` runs a program inside a running container (docs/design/runtime.md): in its cgroup,
# born there, and in its namespaces; the program's status is exec's; the request's cwd, env and user
# override the spec's; a signal to exec reaches the program; a stopped container is refused.
bundle "$W/ex" '["sleep", "infinity"]' "" ""
/opt/hex oci create --bundle "$W/ex" --pid-file "$W/ex.pid" ex && /opt/hex oci start ex || fail "create/start ex"
ipid=$(cat "$W/ex.pid")
seen=$(/opt/hex oci exec ex sh -c 'cat /proc/self/cgroup; hostname; readlink /proc/self/ns/pid' 2>&1 | tr '\n' ' ')
check "exec's cgroup, hostname and pid namespace" "0::/ lab $(readlink /proc/$ipid/ns/pid) " "$seen"
/opt/hex oci exec ex sh -c 'exit 7'; check "exec passes the program's exit code on" 7 "$?"
/opt/hex oci exec ex sh -c 'kill -TERM $$'; check "exec reports a signal's death as 128 + its number" 143 "$?"
/opt/hex oci exec ex /no/such/program 2> /dev/null; check "exec of a program that isn't there" 127 "$?"
check "exec's --cwd, --env and --user" "bar /tmp 1000:1000" \
  "$(/opt/hex oci exec --cwd /tmp --env FOO=bar --user 1000:1000 ex sh -c 'echo "$FOO $(pwd) $(id -u):$(id -g)"' 2>&1)"
if mkdir "$I" 2>/dev/null; then
  echo 1 > "$I/events/cgroup/cgroup_attach_task/enable"
  /opt/hex oci exec ex true || fail "exec true"
  echo 0 > "$I/events/cgroup/cgroup_attach_task/enable"
  check "migrations into the container's cgroup during exec" 0 "$(grep -c 'dst_path=[^ ]*/ex ' "$I/trace")"
  rmdir "$I"
else
  fail "no trace instance under $T, so exec's migrations can't be seen"
fi
/opt/hex oci exec ex sleep 300 & xpid=$!
for _ in $(seq 1 50); do [ "$(wc -l < $CG/ex/cgroup.procs)" -ge 2 ] && break; sleep 0.1; done
kill -TERM "$xpid"; wait "$xpid"; check "a SIGTERM to exec ends the program, reported as 143" 143 "$?"
check "the program is gone from the container's cgroup" 1 "$(wc -l < $CG/ex/cgroup.procs)"
/opt/hex oci kill ex KILL
for _ in $(seq 1 50); do grep -q '"status": *"stopped"' <(/opt/hex oci state ex) && break; sleep 0.1; done
if err=$(/opt/hex oci exec ex true 2>&1); then fail "exec into a stopped container was accepted"
else case "$err" in *stopped*) pass "exec into a stopped container refused: $err" ;; *) fail "exec into a stopped container refused without saying why: $err" ;; esac; fi
/opt/hex oci delete --force ex || fail "delete ex"

echo "cgroups lab: $fails failure(s)"
exit $((fails > 0))
