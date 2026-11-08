#!/usr/bin/env python3
"""Goal 17's record: runtimes interleaved round by round, so every arm meets the host as it is at that moment (Ada's
rule: a p99 claim stands on at least 299 interleaved rounds, the load recorded each round).

  rounds.py <out dir> <rounds> <warmup> <lifecycles> <concurrency> <arm>...

An arm is `hex:<way>`, a node in a seam_oci of its own (its `serve` phase) asked for that way of making a pruned
create's mount namespace (HEX_OCI_MNTNS: copy, empty or tree); `hex`, a node taking its own default; or `libcrun`,
libcrun in process, one libcrun-driver per concurrency slot (libcrun/driver.c, as oci_bench.py runs it). Each arm is a
process of its own, as a node is one process on a host.

Each round reads the host's load once (the 1-minute load average, the runnable and blocked tasks, CPU pressure's
`some avg10`), then runs one burst on each arm, in an order that rotates by one arm a round, so each arm leads as
often as the next. A burst is <lifecycles> of the throughput lifecycle (create, start and delete of a `true` container)
at <concurrency> in flight, each create and delete timed alone. Its window on CLOCK_MONOTONIC, from the request's
write to the reply's read, is recorded, so a trace on the mono clock (create_breakdown_inside.sh's) can be cut by arm.
The first <warmup> rounds are marked and summarized apart: each arm's first burst meets its driver cold.

Env: ROUNDS_SEAM, ROUNDS_HEX, ROUNDS_SLEEP and ROUNDS_TRUE (seam_oci's binary, the hex binary, and the `sleep` and
`true` bundles) for hex arms, and ROUNDS_CGROUP, a cgroup with controllers to hand down, under which each hex arm's
node gets a cgroup of its own for its box root; ROUNDS_LIBCRUN and ROUNDS_LIBCRUN_ROOT (the driver and its state root)
and ROUNDS_TRUE for libcrun. Every other variable reaches each arm as it is (the node's HEXBENCH_* settings among them).

Writes into <out dir>: rounds.jsonl, a line a round; arms.json, what each arm said of itself at its end (a hex arm's
seam_oci result, the node's counts over the phase among it); summary.json and summary.md, each arm's creates and
deletes over the timed rounds and each arm against the first named. Exits 3 when an arm fails, with what it said.
"""
import fcntl
import json
import math
import os
import select
import signal
import subprocess
import sys
import threading
import time

# Each call's bound, as oci_bench.py's CALL_TIMEOUT_S: a burst's is three calls (create, start, delete) for each of
# its lifecycles a slot runs in turn.
CALL_TIMEOUT_S = 60


class ArmError(Exception):
    pass


def pct(samples, q):
    """Nearest rank, as summarize.py's: the smallest sample with at least q of them at or below it."""
    s = sorted(samples)
    return s[max(0, math.ceil(q * len(s)) - 1)] if s else None


def dist(samples):
    return {"n": len(samples), "p50": pct(samples, 0.5), "p99": pct(samples, 0.99), "p99.9": pct(samples, 0.999),
            "max": max(samples) if samples else None}


def host_load():
    """The host's load as one round meets it: the 1-minute load average, the tasks runnable and blocked now
    (/proc/stat), and the share of the last 10 s some task waited for a CPU (/proc/pressure/cpu's `some avg10`)."""
    out = {"load1": os.getloadavg()[0], "procs_running": None, "procs_blocked": None, "cpu_some_avg10": None}
    with open("/proc/stat") as f:
        for line in f:
            key, _, value = line.partition(" ")
            if key in ("procs_running", "procs_blocked"):
                out[key] = int(value)
    try:
        with open("/proc/pressure/cpu") as f:
            fields = dict(kv.split("=", 1) for kv in f.readline().split()[1:])
        out["cpu_some_avg10"] = float(fields["avg10"])
    except (OSError, KeyError, ValueError):
        pass
    return out


def read_line(fd, deadline, what):
    """One line from `fd` by `deadline` (time.monotonic), or ArmError naming `what`."""
    got = b""
    while not got.endswith(b"\n"):
        left = deadline - time.monotonic()
        if left <= 0 or not select.select([fd], [], [], left)[0]:
            raise ArmError(f"{what}: no reply within its bound")
        chunk = os.read(fd, 65536)
        if not chunk:
            raise ArmError(f"{what}: its process closed its replies")
        got += chunk
    return got.decode(errors="replace")


def burst_bound(lifecycles, conc):
    return CALL_TIMEOUT_S * 3 * math.ceil(lifecycles / conc)


class HexArm:
    """A node in a seam_oci of its own, serving bursts (seam_oci's `serve` phase): a request a line on its stdin, a
    JSON reply a line on its stdout. `way` asks for that way of making a pruned create's mount namespace; None leaves
    the node its default. Its result, written as the phase ends, is what it says of itself."""

    def __init__(self, name, way, out_dir, env):
        self.name, self.out = name, os.path.join(out_dir, f"arm-{name}")
        os.makedirs(self.out)
        arm_env = dict(env, HEXBENCH_PHASES="serve")
        arm_env.pop("HEX_OCI_MNTNS", None)
        if way is not None:
            arm_env["HEX_OCI_MNTNS"] = way
        # Each node its own box root, as each node on a host has: under ROUNDS_CGROUP, a cgroup of the arm's with the
        # parent's controllers handed down (seam_oci makes its box root beneath HEXBENCH_CGROUP).
        self.cgroup = None
        if env.get("ROUNDS_CGROUP"):
            self.cgroup = os.path.join(env["ROUNDS_CGROUP"], f"arm-{name}")
            os.mkdir(self.cgroup)
            with open(os.path.join(env["ROUNDS_CGROUP"], "cgroup.subtree_control")) as f:
                controllers = f.read().split()
            with open(os.path.join(self.cgroup, "cgroup.subtree_control"), "w") as f:
                f.write(" ".join("+" + c for c in controllers))
            arm_env["HEXBENCH_CGROUP"] = self.cgroup
        argv = [env["ROUNDS_SEAM"], os.path.join(self.out, "seam.json"), "0", "0", "0", env["ROUNDS_HEX"],
                env["ROUNDS_SLEEP"], env["ROUNDS_TRUE"]]
        with open(os.path.join(self.out, "stderr.log"), "w") as err:
            self.p = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=err, env=arm_env)

    def burst(self, lifecycles, conc):
        try:
            self.p.stdin.write(f"burst {lifecycles} {conc}\n".encode())
            self.p.stdin.flush()
        except BrokenPipeError:
            raise ArmError(f"{self.name}: its seam_oci exited {self.p.poll()}") from None
        line = read_line(self.p.stdout.fileno(), time.monotonic() + burst_bound(lifecycles, conc), self.name)
        reply = json.loads(line)
        if "error" in reply:
            raise ArmError(f"{self.name}: {reply['error']}")
        return reply["create_ms"], reply["delete_ms"]

    def close(self):
        """Ends the phase (its stdin's end) and the process, at most CALL_TIMEOUT_S, then killed; what it said."""
        try:
            self.p.stdin.close()
        except BrokenPipeError:
            pass
        try:
            code = self.p.wait(timeout=CALL_TIMEOUT_S)
        except subprocess.TimeoutExpired:
            self.p.kill()
            code = self.p.wait()
        self.p.stdout.close()
        try:
            with open(os.path.join(self.out, "seam.json")) as f:
                said = json.load(f)
        except (OSError, ValueError) as e:
            said = {"error": f"no result: {e}"}
        out = {"exit": code, "result": said}
        if self.cgroup:
            # seam_oci removed its box root as it ended; a cgroup it left is said, not removed by force.
            try:
                os.rmdir(self.cgroup)
            except OSError as e:
                out["cgroup_left"] = f"{self.cgroup}: {e}"
        return out


class Driver:
    """One libcrun-driver (libcrun/driver.c): requests a line on its fd 3, replies a line on its fd 4, each call
    timed inside it, as oci_bench.py's Driver runs it."""

    def __init__(self, path, root, err_path):
        req_r, self.req_w = os.pipe()
        self.rep_r, rep_w = os.pipe()
        # Its ends above 4, so neither dup2 onto 3 or 4 overwrites the other.
        ends = [fcntl.fcntl(fd, fcntl.F_DUPFD_CLOEXEC, 10) for fd in (req_r, rep_w)]
        os.close(req_r)
        os.close(rep_w)
        errfd = os.open(err_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644)
        try:
            actions = [(os.POSIX_SPAWN_OPEN, 0, os.devnull, os.O_RDONLY, 0),
                       (os.POSIX_SPAWN_OPEN, 1, os.devnull, os.O_WRONLY, 0),
                       (os.POSIX_SPAWN_DUP2, errfd, 2), (os.POSIX_SPAWN_DUP2, ends[0], 3),
                       (os.POSIX_SPAWN_DUP2, ends[1], 4)]
            self.pid = os.posix_spawn(path, [path, root], dict(os.environ), file_actions=actions)
        finally:
            for fd in ends + [errfd]:
                os.close(fd)

    def ask(self, *words):
        """One call's wall time in ms, as the driver timed it."""
        try:
            os.write(self.req_w, (" ".join(words) + "\n").encode())
        except BrokenPipeError:
            raise ArmError(f"libcrun {words[0]}: the driver exited") from None
        kind, _, rest = read_line(self.rep_r, time.monotonic() + CALL_TIMEOUT_S, f"libcrun {words[0]}").strip() \
            .partition(" ")
        if kind != "ok":
            raise ArmError(f"libcrun {words[0]} {words[-1]}: {rest}")
        return int(rest.split()[0]) / 1e6

    def end(self):
        """EOF on its requests, then its end, at most CALL_TIMEOUT_S before it's killed: its exit."""
        os.close(self.req_w)
        os.close(self.rep_r)
        pidfd = os.pidfd_open(self.pid)
        try:
            if not select.select([pidfd], [], [], CALL_TIMEOUT_S)[0]:
                os.kill(self.pid, signal.SIGKILL)
        finally:
            os.close(pidfd)
        return os.waitstatus_to_exitcode(os.waitpid(self.pid, 0)[1])


class LibcrunArm:
    """libcrun in process: one driver per concurrency slot, each a slot's lifecycles in turn."""

    def __init__(self, conc, out_dir, env):
        self.name, self.out = "libcrun", os.path.join(out_dir, "arm-libcrun")
        os.makedirs(self.out)
        self.bundle, self.bursts = env["ROUNDS_TRUE"], 0
        self.drivers = [Driver(env["ROUNDS_LIBCRUN"], env["ROUNDS_LIBCRUN_ROOT"],
                               os.path.join(self.out, f"driver{i}.err")) for i in range(conc)]

    def burst(self, lifecycles, conc):
        if conc > len(self.drivers):
            raise ArmError(f"libcrun: a burst at {conc} in flight, past its {len(self.drivers)} drivers")
        claimed, lock, start = [0], threading.Lock(), threading.Barrier(conc)
        results, errors = [[] for _ in range(conc)], []

        def slot(i):
            start.wait()
            while not errors:
                with lock:
                    j = claimed[0]
                    if j >= lifecycles:
                        return
                    claimed[0] = j + 1
                cid = f"lc{self.bursts:06}-{j:03}"
                try:
                    create = self.drivers[i].ask("create", self.bundle, cid)
                    try:
                        self.drivers[i].ask("start", cid)
                    finally:
                        delete = self.drivers[i].ask("delete", cid)
                except ArmError as e:
                    errors.append(str(e))
                    return
                results[i].append((create, delete))

        threads = [threading.Thread(target=slot, args=(i,)) for i in range(conc)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        self.bursts += 1
        if errors:
            raise ArmError(errors[0])
        done = [r for rs in results for r in rs]
        return [c for c, _ in done], [d for _, d in done]

    def close(self):
        return {"exits": [d.end() for d in self.drivers]}


def make_arm(spec, conc, out_dir, env):
    if spec == "libcrun":
        return LibcrunArm(conc, out_dir, env)
    if spec == "hex":
        return HexArm("hex", None, out_dir, env)
    if spec.startswith("hex:") and spec[4:] in ("copy", "empty", "tree"):
        return HexArm(spec[4:], spec[4:], out_dir, env)
    raise SystemExit(f"refusing: an arm {spec!r} isn't hex, hex:copy, hex:empty, hex:tree or libcrun")


def run_rounds(arms, rounds, warmup, lifecycles, conc, out, clock=time.monotonic_ns, load=host_load):
    """The rounds, each a line of `out`: its number (warmup rounds below 0), the load it met, its order and each
    burst's window and times. Raises ArmError when an arm fails, after the rounds done so far are written."""
    k = len(arms)
    for r in range(warmup + rounds):
        row = {"round": r - warmup, "warmup": r < warmup, "load": load(),
               "order": [arms[(r + i) % k].name for i in range(k)], "bursts": []}
        try:
            for i in range(k):
                arm = arms[(r + i) % k]
                t0 = clock()
                create_ms, delete_ms = arm.burst(lifecycles, conc)
                t1 = clock()
                if len(create_ms) != lifecycles or len(delete_ms) != lifecycles:
                    raise ArmError(f"{arm.name}: a burst of {lifecycles} answered {len(create_ms)} creates and "
                                   f"{len(delete_ms)} deletes")
                row["bursts"].append({"arm": arm.name, "t0_ns": t0, "t1_ns": t1, "create_ms": create_ms,
                                      "delete_ms": delete_ms})
        finally:
            out.write(json.dumps(row) + "\n")
            out.flush()


def summarize(rows, names):
    """Each arm over the timed rounds: its creates and deletes, pooled, and each round's median create; each arm but
    the first against the first: the rounds whose median create is below the first's, and the pooled p50s' ratio."""
    timed = [r for r in rows if not r["warmup"] and len(r["bursts"]) == len(names)]
    per = {}
    for name in names:
        bursts = [b for r in timed for b in r["bursts"] if b["arm"] == name]
        creates = [ms for b in bursts for ms in b["create_ms"]]
        per[name] = {"rounds": len(bursts), "create_ms": dist(creates),
                     "delete_ms": dist([ms for b in bursts for ms in b["delete_ms"]]),
                     "round_medians": [pct(b["create_ms"], 0.5) for b in bursts]}
    first = names[0]
    for name in names[1:]:
        pairs = list(zip(per[name]["round_medians"], per[first]["round_medians"]))
        a, b = per[name]["create_ms"]["p50"], per[first]["create_ms"]["p50"]
        per[name]["against_" + first] = {
            "rounds_below": sum(1 for x, y in pairs if x < y), "rounds": len(pairs),
            "p50_ratio": a / b if a is not None and b else None,
            "p99_lower": (per[name]["create_ms"]["p99"] < per[first]["create_ms"]["p99"])
            if pairs else None}
    loads = [r["load"] for r in timed]
    return {"rounds": len(timed), "arms": names, "per_arm": per,
            "load1": dist([lo["load1"] for lo in loads]) if loads else None,
            "procs_running": dist([lo["procs_running"] for lo in loads if lo["procs_running"] is not None])}


def markdown(summary):
    names = summary["arms"]
    out = [f"{summary['rounds']} timed rounds; load1 over them p50 {summary['load1']['p50']:.2f}, max "
           f"{summary['load1']['max']:.2f}" if summary["load1"] else f"{summary['rounds']} timed rounds", "",
           "| arm | creates | create p50 / p99 / p99.9 / max, ms | delete p50 / p99, ms | against " + names[0] + " |",
           "|---|---|---|---|---|"]
    fmt = lambda v: "n/a" if v is None else f"{v:.3f}"
    for name in names:
        p = summary["per_arm"][name]
        c, d = p["create_ms"], p["delete_ms"]
        vs = p.get("against_" + names[0])
        against = (f"median below in {vs['rounds_below']} of {vs['rounds']} rounds, p50 ratio "
                   f"{fmt(vs['p50_ratio'])}, p99 {'lower' if vs['p99_lower'] else 'not lower'}") if vs else ""
        out.append(f"| {name} | {c['n']} | {fmt(c['p50'])} / {fmt(c['p99'])} / {fmt(c['p99.9'])} / {fmt(c['max'])} | "
                   f"{fmt(d['p50'])} / {fmt(d['p99'])} | {against} |")
    return "\n".join(out) + "\n"


def main(argv):
    if len(argv) < 6:
        raise SystemExit(__doc__)
    out_dir, (rounds, warmup, lifecycles, conc), specs = argv[0], map(int, argv[1:5]), argv[5:]
    if rounds < 1 or warmup < 0 or lifecycles < 1 or conc < 1:
        raise SystemExit("refusing: rounds, lifecycles and concurrency above 0, warmup at least 0")
    os.makedirs(out_dir)
    arms, said, failed = [], {}, None
    try:
        for spec in specs:
            arms.append(make_arm(spec, conc, out_dir, dict(os.environ)))
        with open(os.path.join(out_dir, "rounds.jsonl"), "w") as f:
            run_rounds(arms, rounds, warmup, lifecycles, conc, f)
    except ArmError as e:
        failed = str(e)
    finally:
        for arm in arms:
            said[arm.name] = arm.close()
        with open(os.path.join(out_dir, "arms.json"), "w") as f:
            json.dump(said, f, indent=1)
    with open(os.path.join(out_dir, "rounds.jsonl")) as f:
        rows = [json.loads(line) for line in f]
    summary = summarize(rows, [a.name for a in arms])
    summary["failed"] = failed
    with open(os.path.join(out_dir, "summary.json"), "w") as f:
        json.dump(summary, f, indent=1)
    with open(os.path.join(out_dir, "summary.md"), "w") as f:
        f.write(markdown(summary))
    if failed:
        print(f"refusing: {failed}", file=sys.stderr)
        return 3
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
