#!/usr/bin/env python3
"""bench/competitive: a sandbox platform end to end through its own API (SDK, CLI or Kubernetes API).

  platform_bench.py <e2b|daytona|docker|k8s|agentsandbox|agentsandbox-warm|cli> <out.json> <n> <warmup>
                    <lifecycles> <density cap>

Run on the bench host against a self-hosted stack (E2B `embed/` compose, Daytona `docker/` compose), or
against a hosted platform, which the result then labels hosted (its numbers include the network).
  e2b:      E2B_API_KEY, E2B_DOMAIN or E2B_API_URL as `docker compose exec ready cat /run/e2b/sdk.env`
            prints them; template "base".
  daytona:  DAYTONA_API_URL, DAYTONA_API_KEY; the default snapshot.
  docker:   the Docker Engine CLI; DOCKER_IMAGE (the OCI bundles' rootfs, `docker import`ed), host /usr bound;
            DOCKER_CGROUP_PARENT, a slice listed in STACK_CGROUPS that holds the sandboxes (an idle keeper
            container holds the slice open for the run, so its counters live as long as the run).
  k8s:      a Pod through the Kubernetes API (kubeconfig; K8S_NAMESPACE, K8S_IMAGE).
  agentsandbox / agentsandbox-warm: kubernetes-sigs/agent-sandbox, a Sandbox (agents.x-k8s.io/v1beta1), or a
            SandboxClaim from the warm pool AS_WARMPOOL (extensions.agents.x-k8s.io/v1beta1).
  cli:      any platform with a CLI (Agent Substrate's `kubectl ate`, AX's `ax`): CLI_CREATE, CLI_EXEC,
            CLI_DELETE, optional CLI_READY (polled until it exits 0), CLI_SUSPEND and CLI_RESUME, each a
            command with {name}; CLI_NAME names the platform in the result.
Phases: lifecycle (create until the sandbox runs a command, exec of `true`, delete; n sequential),
throughput (whole lifecycles at concurrency 1, 8, 32), density (idle sandboxes up to the cap), and, for a
platform that can suspend, resume (n suspend/resume cycles of one sandbox, each resume timed until the
sandbox runs a command again). HEXBENCH_PHASES names the phases to run (all by default).
A platform's cost lives in many services, so its CPU and memory are those of its stack's own cgroups,
STACK_CGROUPS (paths under /sys/fs/cgroup, none inside another, each alive for the whole run: the stack's
containers, docker.service, containerd.service), plus this client and the CLIs it runs, as Hex's bench
cgroup holds its harness too. Host-wide figures would charge the platform with whatever else the host
runs. The stack's own idle CPU is measured first and taken out of each phase's CPU, so CPU per lifecycle
is what a lifecycle adds; the idle cost is recorded beside it. Memory is what the cgroups hold, page
cache aside (cgmem.py). Without STACK_CGROUPS (a hosted platform) CPU and memory are not measured.
"""
import collections
import itertools
import json
import os
import resource
import shlex
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))  # cgmem.py, beside this file even under -I
import cgmem  # noqa: E402

CG_ROOT = "/sys/fs/cgroup"
# The idle window must hold the stack's periodic work: Daytona's runner long-polls its API with a 30 s
# timeout and the API runs a 10-second cron (docs/research/runtime/agent-sandbox-platforms.md, Daytona),
# and a kubelet reports node status every 10 s (KubeletConfiguration's nodeStatusUpdateFrequency default).
# Cited, not measured: two of the longest period.
IDLE_S = 60


class E2B:
    def __init__(self):
        from e2b import Sandbox  # e2b==2.46.0, as embed/compose/README.md pins
        self.Sandbox = Sandbox

    def create(self):
        return self.Sandbox.create("base")

    def exec_true(self, s):
        s.commands.run("true")

    def delete(self, s):
        s.kill()


class Daytona:
    def __init__(self):
        from daytona import Daytona as Client, DaytonaConfig, DaytonaNotFoundError
        self.client = Client(DaytonaConfig(api_key=os.environ["DAYTONA_API_KEY"], api_url=os.environ["DAYTONA_API_URL"]))
        self.NotFound = DaytonaNotFoundError

    def create(self):
        return self.client.create()

    def exec_true(self, s):
        s.process.exec("true")

    def delete(self, s):
        # Waiting until the sandbox is gone is what every other row's delete measures. SDK 0.187.0, which
        # matches the v0.187.0 server, returns once the API accepts the delete and has no option to wait,
        # so the sandbox is polled until it is destroyed or the API no longer knows it. Every 20 ms: a
        # fifth of the SDK's own 100 ms state-wait interval (sandbox.py, wait_for_sandbox_stop), so a
        # delete reads at most one poll and its request late.
        self.client.delete(s)
        deadline = time.monotonic() + 60
        while True:
            try:
                s.refresh_data()
            except self.NotFound:
                return
            if str(getattr(s.state, "value", s.state)) == "destroyed":
                return
            if time.monotonic() > deadline:
                raise RuntimeError(f"sandbox {s.id}: not destroyed within 60 s of delete (state {s.state})")
            time.sleep(0.02)


def run_cmd(argv, timeout=300):
    r = subprocess.run(argv, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=timeout)
    if r.returncode:
        raise RuntimeError(f"{argv[0]} {argv[1] if len(argv) > 1 else ''} exit {r.returncode}: {r.stderr.strip()[:300]}")
    return r.stdout.strip()


class Docker:
    def __init__(self):
        self.image = os.environ.get("DOCKER_IMAGE", "hexbench/rootfs:1")
        parent = os.environ.get("DOCKER_CGROUP_PARENT")
        self.parent = ["--cgroup-parent", parent] if parent else []
        # Sandboxes come and go in the slice; the keeper holds it, and the counters, for the whole run.
        self.keeper = self.create() if parent else None

    def create(self):
        return run_cmd(["docker", "run", "-d", "--network", "none", "-v", "/usr:/usr:ro", "--label",
                        "hex.bench=competitive", *self.parent, self.image, "/usr/bin/sleep", "infinity"])

    def exec_true(self, s):
        run_cmd(["docker", "exec", s, "/usr/bin/true"])

    def delete(self, s):
        run_cmd(["docker", "rm", "-f", s])

    def close(self):
        if self.keeper:
            self.delete(self.keeper)


class Kubernetes:
    """A Pod: create until Running and Ready, exec of `true`, delete until it is gone."""
    def __init__(self):
        from kubernetes import client, config, watch
        from kubernetes.stream import stream
        config.load_kube_config()
        cfg = client.Configuration.get_default_copy()
        cfg.connection_pool_maxsize = 64
        api = client.ApiClient(cfg)
        self.v1, self.co = client.CoreV1Api(api), client.CustomObjectsApi(api)
        self.watch, self.stream, self.ApiException = watch, stream, client.exceptions.ApiException
        self.exec_api = lambda: client.CoreV1Api(client.ApiClient(cfg))
        self.local = threading.local()
        self.ns = os.environ.get("K8S_NAMESPACE", "hexbench")
        self.image = os.environ.get("K8S_IMAGE", "busybox:1.37")
        self.seq = itertools.count()

    def name(self):
        return f"hb-{os.getpid()}-{next(self.seq)}"

    def wait(self, lister, args, name, done, timeout=300):
        w = self.watch.Watch()
        for ev in w.stream(lister, *args, field_selector=f"metadata.name={name}", timeout_seconds=timeout):
            if done(ev):
                w.stop()
                return
        raise RuntimeError(f"{name}: not ready in {timeout}s")

    def until_ready(self, name, wait, remove):
        """Waits for an object the API accepted. One that never gets ready is removed, never left to hold a
        slot: a Pending pod past the node's pod limit took one from each following density try."""
        try:
            wait()
        except Exception as e:
            try:
                remove()
            except Exception as r:
                raise RuntimeError(f"{e}; removing {name} failed too: {r}") from e
            raise

    def pod_ready(self, ev):
        o = ev["object"]
        st = o.status if hasattr(o, "status") else None
        return bool(st and st.phase == "Running" and st.container_statuses and all(c.ready for c in st.container_statuses))

    def pod_spec(self):
        return {"containers": [{"name": "c", "image": self.image, "imagePullPolicy": "IfNotPresent",
                                "command": ["sleep", "infinity"]}],
                "terminationGracePeriodSeconds": 0, "automountServiceAccountToken": False}

    def create(self):
        name = self.name()
        self.v1.create_namespaced_pod(self.ns, {"apiVersion": "v1", "kind": "Pod", "metadata": {"name": name},
                                                "spec": self.pod_spec()})
        self.until_ready(name, lambda: self.wait(self.v1.list_namespaced_pod, (self.ns,), name, self.pod_ready),
                         lambda: self.delete(name))
        return name

    def exec_in(self, pod):
        # stream() swaps its ApiClient's call_api for a websocket call while it runs (kubernetes 37.0.1,
        # stream/stream.py). Every other request on that client meanwhile goes out as a websocket handshake
        # ("Handshake status 200 OK"), and two overlapping execs can leave it swapped for good. So an exec
        # never shares a client: each thread makes its own at its first exec (in the lifecycle phase, during
        # warm-up).
        if not hasattr(self.local, "v1"):
            self.local.v1 = self.exec_api()
        self.stream(self.local.v1.connect_get_namespaced_pod_exec, pod, self.ns, command=["true"],
                    stderr=True, stdin=False, stdout=True, tty=False)

    def exec_true(self, s):
        self.exec_in(s)

    def gone(self, pod):
        try:
            self.v1.read_namespaced_pod(pod, self.ns)
        except self.ApiException as e:
            if e.status == 404:
                return
            raise
        self.wait(self.v1.list_namespaced_pod, (self.ns,), pod, lambda ev: ev["type"] == "DELETED")

    def delete(self, s):
        self.v1.delete_namespaced_pod(s, self.ns, grace_period_seconds=0)
        self.gone(s)


def cr_ready(ev):
    conds = (ev["object"].get("status") or {}).get("conditions") or []
    return any(c.get("type") == "Ready" and c.get("status") == "True" for c in conds)


class AgentSandbox(Kubernetes):
    """A Sandbox (agents.x-k8s.io/v1beta1): create until its Ready condition, exec in its pod (same name), delete."""
    G, V, P = "agents.x-k8s.io", "v1beta1", "sandboxes"

    def create(self):
        name = self.name()
        self.co.create_namespaced_custom_object(self.G, self.V, self.ns, self.P, {
            "apiVersion": f"{self.G}/{self.V}", "kind": "Sandbox", "metadata": {"name": name},
            "spec": {"podTemplate": {"spec": self.pod_spec()}}})
        self.until_ready(name, lambda: self.wait(self.co.list_namespaced_custom_object, (self.G, self.V, self.ns, self.P),
                                                 name, cr_ready),
                         lambda: self.delete(name))
        return name

    def delete(self, s):
        self.co.delete_namespaced_custom_object(self.G, self.V, self.ns, self.P, s)
        self.gone(s)


class AgentSandboxWarm(Kubernetes):
    """A SandboxClaim from the warm pool AS_WARMPOOL: create until Ready, exec in the claimed sandbox's pod, delete."""
    G, V, P = "extensions.agents.x-k8s.io", "v1beta1", "sandboxclaims"

    def create(self):
        name = self.name()
        self.co.create_namespaced_custom_object(self.G, self.V, self.ns, self.P, {
            "apiVersion": f"{self.G}/{self.V}", "kind": "SandboxClaim", "metadata": {"name": name},
            "spec": {"warmPoolRef": {"name": os.environ["AS_WARMPOOL"]}}})
        found = {}

        def ready(ev):
            st = ev["object"].get("status") or {}
            if cr_ready(ev) and (st.get("sandbox") or {}).get("name"):
                found["pod"] = st["sandbox"]["name"]
                return True
            return False
        # A claim owns the sandbox it adopts or makes (sandboxclaim_controller.go sets itself as its controller
        # reference), so deleting the claim takes that sandbox too; its pod, once known, is waited out.
        def remove():
            self.co.delete_namespaced_custom_object(self.G, self.V, self.ns, self.P, name)
            if found.get("pod"):
                self.gone(found["pod"])
        self.until_ready(name, lambda: self.wait(self.co.list_namespaced_custom_object, (self.G, self.V, self.ns, self.P),
                                                 name, ready),
                         remove)
        return (name, found["pod"])

    def exec_true(self, s):
        self.exec_in(s[1])

    def delete(self, s):
        self.co.delete_namespaced_custom_object(self.G, self.V, self.ns, self.P, s[0])
        self.gone(s[1])


class Cli:
    """Commands from the environment, each with {name}; CLI_READY is polled until it exits 0 (5 minutes at most)."""
    def __init__(self):
        self.cmd = {k: os.environ.get("CLI_" + k.upper()) for k in ("create", "ready", "exec", "delete", "suspend", "resume")}
        self.seq = itertools.count()
        if not self.cmd["exec"]:  # a platform without an exec verb: its exec rows read n/a, never a no-op's time
            self.exec_true = None
        if self.cmd["suspend"] and self.cmd["resume"]:
            self.suspend = lambda s: self.run("suspend", s)
            self.resume = lambda s: (self.run("resume", s), self.until_ready(s))

    def run(self, key, name):
        run_cmd(shlex.split(self.cmd[key].format(name=name)))

    def until_ready(self, name):
        if not self.cmd["ready"]:
            return
        deadline = time.monotonic() + 300
        while subprocess.run(shlex.split(self.cmd["ready"].format(name=name)), stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL).returncode:
            if time.monotonic() > deadline:
                raise RuntimeError(f"{name}: not ready in 300s")
            time.sleep(0.02)

    def create(self):
        name = f"hb{os.getpid()}x{next(self.seq)}"
        self.run("create", name)
        self.until_ready(name)
        return name

    def exec_true(self, s):
        self.run("exec", s)

    def delete(self, s):
        self.run("delete", s)


def own_cgroup(pid):
    with open(f"/proc/{pid}/cgroup") as f:
        rel = f.read().split("::", 1)[-1].strip().strip("/")
    return os.path.join(CG_ROOT, rel) if rel else CG_ROOT


class Stack:
    """The platform's cgroups and this client. A cgroup's cpu.stat keeps the CPU of children it has
    removed, so sandboxes created and deleted inside a listed cgroup still count; a listed cgroup removed
    or made anew mid-run (a restarted container) loses its counters, so reading it then is an error."""

    def __init__(self, spec):
        self.paths = [os.path.join(CG_ROOT, p.strip("/")) for p in spec.split()]
        for a in self.paths:
            for b in self.paths:
                if a != b and b.startswith(a + "/"):
                    raise SystemExit(f"STACK_CGROUPS: {b} is inside {a}, so it would count twice")
        if self.holds(own_cgroup("self")):
            raise SystemExit("STACK_CGROUPS holds this client, so its CPU would count twice")
        self.ino = {p: os.stat(p).st_ino for p in self.paths}

    def holds(self, path):
        return any(path == p or path.startswith(p + "/") for p in self.paths)

    def check(self, p):
        if os.stat(p).st_ino != self.ino[p]:
            raise RuntimeError(f"{p} was made anew mid-run, so its usage since then is unknown")

    def read(self, p, name):
        self.check(p)
        with open(os.path.join(p, name)) as f:
            return f.read()

    def cpu_s(self):
        usec = 0
        for p in self.paths:
            v = [line.split()[1] for line in self.read(p, "cpu.stat").splitlines() if line.startswith("usage_usec ")]
            if not v:
                raise RuntimeError(f"{p}/cpu.stat has no usage_usec")
            usec += int(v[0])
        ru = [resource.getrusage(w) for w in (resource.RUSAGE_SELF, resource.RUSAGE_CHILDREN)]
        return usec / 1e6 + sum(r.ru_utime + r.ru_stime for r in ru)

    def held_bytes(self):
        """The memory the stack's cgroups hold (cgmem.py)."""
        total = 0
        for p in self.paths:
            self.check(p)
            total += cgmem.held_bytes(p)
        return total

    def outside(self):
        """{pid: comm} of every process outside the stack's cgroups."""
        out = {}
        for d in os.listdir("/proc"):
            if not d.isdigit():
                continue
            try:
                cg = own_cgroup(d)
                with open(f"/proc/{d}/comm") as f:
                    comm = f.read().strip()
            except OSError:  # it exited between the listing and the read
                continue
            if cg != CG_ROOT and not self.holds(cg):  # the root cgroup holds the kernel's threads
                out[int(d)] = comm
        return out


def timed(fn, *a):
    t0 = time.perf_counter_ns()
    r = fn(*a)
    return r, (time.perf_counter_ns() - t0) / 1e6


def lifecycle(p):
    s, c = timed(p.create)
    e = None
    try:
        if p.exec_true:
            _, e = timed(p.exec_true, s)
    finally:
        _, d = timed(p.delete, s)
    return c, e, d


def main():
    kind, out_path = sys.argv[1], sys.argv[2]
    n, warmup, lifecycles, cap = (int(x) for x in sys.argv[3:7])
    p = {"e2b": E2B, "daytona": Daytona, "docker": Docker, "k8s": Kubernetes, "agentsandbox": AgentSandbox,
         "agentsandbox-warm": AgentSandboxWarm, "cli": Cli}[kind]()
    if kind == "cli":
        kind = os.environ.get("CLI_NAME", "cli")
    res = {"platform": kind, "hosted": os.environ.get("HOSTED") == "1", "n": n, "phases": {}, "errors": {}}
    try:
        phases(p, res, n, warmup, lifecycles, cap)
    finally:
        if hasattr(p, "close"):
            p.close()
    with open(out_path, "w") as f:
        json.dump(res, f)
    print(json.dumps({k: v for k, v in res.items() if k != "phases"}))


def phases(p, res, n, warmup, lifecycles, cap):
    spec = os.environ.get("STACK_CGROUPS")
    stack = Stack(spec) if spec else None
    wanted = (os.environ.get("HEXBENCH_PHASES") or "lifecycle throughput density resume").split()
    idle = 0.0
    if stack:
        c0, t0 = stack.cpu_s(), time.perf_counter()
        time.sleep(IDLE_S)
        wall = time.perf_counter() - t0
        idle = (stack.cpu_s() - c0) / wall
        res["stack"] = {"cgroups": stack.paths, "idle_window_s": wall, "idle_cpu_s_per_s": idle,
                        "idle_held_bytes": stack.held_bytes()}
        outside0 = set(stack.outside())

    def cpu_per_lifecycle(c0, wall, count):
        # What the lifecycles added: the stack's CPU less its own idle CPU over the same wall time.
        return (stack.cpu_s() - c0 - idle * wall) / count
    if "lifecycle" in wanted:
        lifecycle_phase(p, res, n, warmup, stack, cpu_per_lifecycle)
    if "throughput" in wanted:
        throughput_phase(p, res, lifecycles, stack, cpu_per_lifecycle)
    if "density" in wanted:
        density_phase(p, res, cap, stack, outside0 if stack else None)
    if "resume" in wanted and hasattr(p, "suspend"):
        resume_phase(p, res, n, warmup)


def lifecycle_phase(p, res, n, warmup, stack, cpu_per_lifecycle):
    try:
        ms = {"create": [], "exec": [], "delete": []}
        c0, t0 = stack.cpu_s() if stack else None, time.perf_counter()
        for i in range(warmup + n):
            c, e, d = lifecycle(p)
            if i >= warmup:
                ms["create"].append(c)
                if e is not None:
                    ms["exec"].append(e)
                ms["delete"].append(d)
        res["phases"]["lifecycle"] = {"ms": ms}
        if stack:
            res["phases"]["lifecycle"]["stack_cpu_s_per_lifecycle"] = cpu_per_lifecycle(
                c0, time.perf_counter() - t0, warmup + n)
    except Exception as e:
        res["errors"]["lifecycle"] = f"{type(e).__name__}: {e}"


def throughput_phase(p, res, lifecycles, stack, cpu_per_lifecycle):
    try:
        tp = {}
        for conc in (1, 8, 32):
            c0, t0 = stack.cpu_s() if stack else None, time.perf_counter()
            with ThreadPoolExecutor(max_workers=conc) as pool:
                list(pool.map(lambda _: lifecycle(p), range(lifecycles)))
            wall = time.perf_counter() - t0
            tp[str(conc)] = {"lifecycles": lifecycles, "wall_s": wall, "per_s": lifecycles / wall}
            if stack:
                tp[str(conc)]["stack_cpu_s_per_lifecycle"] = cpu_per_lifecycle(c0, wall, lifecycles)
        res["phases"]["throughput"] = tp
    except Exception as e:
        res["errors"]["throughput"] = f"{type(e).__name__}: {e}"


def density_phase(p, res, cap, stack, outside0):
    sandboxes, failed = [], []
    stop = "cap"
    try:
        base = stack.held_bytes() if stack else None
        t0 = time.perf_counter()
        for i in range(cap):
            try:
                sandboxes.append(p.create())
            except Exception as e:  # a platform at its limit refuses: that bound is the density result
                stop = f"refused at {i}: {type(e).__name__}: {str(e)[:200]}"
                break
        density = {"count": len(sandboxes), "stop": stop, "start_wall_s": time.perf_counter() - t0}
        if stack:
            time.sleep(5)  # let the sandboxes settle before reading memory
            new = collections.Counter(c for pid, c in stack.outside().items() if pid not in outside0)
            density.update({"held": cgmem.per_unit(stack.held_bytes() - base, len(sandboxes)),
                            # A sandbox's own processes among these mean STACK_CGROUPS misses part of the stack.
                            "new_processes_outside_stack": dict(new.most_common(20))})
        res["phases"]["density"] = density
    except Exception as e:
        res["errors"]["density"] = f"{type(e).__name__} after {len(sandboxes)}: {e}"
    finally:
        for s_ in sandboxes:
            try:
                p.delete(s_)
            except Exception as e:
                failed.append(f"{type(e).__name__}: {str(e)[:200]}")
        if failed:
            res["errors"]["density_teardown"] = f"{len(failed)} of {len(sandboxes)} deletes failed, the first: {failed[0]}"


def resume_phase(p, res, n, warmup):
    try:
        s_ = p.create()
        try:
            sus, resu = [], []
            for i in range(warmup + n):
                _, a = timed(p.suspend, s_)
                _, b = timed(p.resume, s_)
                if p.exec_true:
                    p.exec_true(s_)
                if i >= warmup:
                    sus.append(a)
                    resu.append(b)
            res["phases"]["resume"] = {"ms": {"suspend": sus, "resume": resu}}
        finally:
            p.delete(s_)
    except Exception as e:
        res["errors"]["resume"] = f"{type(e).__name__}: {e}"


if __name__ == "__main__":
    main()
