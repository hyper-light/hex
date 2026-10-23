#!/usr/bin/env python3
"""bench/competitive: a sandbox platform end to end through its own API (SDK, CLI or Kubernetes API).

  platform_bench.py <e2b|daytona|docker|k8s|agentsandbox|agentsandbox-warm|cli> <out.json> <n> <warmup>
                    <lifecycles> <density cap>

Run on the bench host against a self-hosted stack (E2B `embed/` compose, Daytona `docker/` compose), or
against a hosted platform, which the result then labels hosted (its numbers include the network).
  e2b:      E2B_API_KEY, E2B_DOMAIN or E2B_API_URL as `docker compose exec ready cat /run/e2b/sdk.env`
            prints them; template "base".
  daytona:  DAYTONA_API_URL, DAYTONA_API_KEY; the default snapshot.
  docker:   the Docker Engine CLI; DOCKER_IMAGE (the OCI bundles' rootfs, `docker import`ed), host /usr bound.
  k8s:      a Pod through the Kubernetes API (kubeconfig; K8S_NAMESPACE, K8S_IMAGE).
  agentsandbox / agentsandbox-warm: kubernetes-sigs/agent-sandbox, a Sandbox (agents.x-k8s.io/v1beta1), or a
            SandboxClaim from the warm pool AS_WARMPOOL (extensions.agents.x-k8s.io/v1beta1).
  cli:      any platform with a CLI (Agent Substrate's `kubectl ate`, AX's `ax`): CLI_CREATE, CLI_EXEC,
            CLI_DELETE, optional CLI_READY (polled until it exits 0), CLI_SUSPEND and CLI_RESUME, each a
            command with {name}; CLI_NAME names the platform in the result.
Phases: lifecycle (create until the sandbox runs a command, exec of `true`, delete; n sequential),
throughput (whole lifecycles at concurrency 1, 8, 32), density (idle sandboxes up to the cap), and, for a
platform that can suspend, resume (n suspend/resume cycles of one sandbox, each resume timed until the
sandbox runs a command again).
A platform's cost lives in many services, so CPU and memory are host-wide here: /proc/stat CPU time
per lifecycle and the MemAvailable drop per idle sandbox, on an otherwise idle bench host.
"""
import itertools
import json
import os
import shlex
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor


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
        from daytona import Daytona as Client, DaytonaConfig
        self.client = Client(DaytonaConfig(api_key=os.environ["DAYTONA_API_KEY"], api_url=os.environ["DAYTONA_API_URL"]))

    def create(self):
        return self.client.create()

    def exec_true(self, s):
        s.process.exec("true")

    def delete(self, s):
        self.client.delete(s)


def run_cmd(argv, timeout=300):
    r = subprocess.run(argv, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=timeout)
    if r.returncode:
        raise RuntimeError(f"{argv[0]} {argv[1] if len(argv) > 1 else ''} exit {r.returncode}: {r.stderr.strip()[:300]}")
    return r.stdout.strip()


class Docker:
    def __init__(self):
        self.image = os.environ.get("DOCKER_IMAGE", "hexbench/rootfs:1")

    def create(self):
        return run_cmd(["docker", "run", "-d", "--network", "none", "-v", "/usr:/usr:ro", "--label",
                        "hex.bench=competitive", self.image, "/usr/bin/sleep", "infinity"])

    def exec_true(self, s):
        run_cmd(["docker", "exec", s, "/usr/bin/true"])

    def delete(self, s):
        run_cmd(["docker", "rm", "-f", s])


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
        self.wait(self.v1.list_namespaced_pod, (self.ns,), name, self.pod_ready)
        return name

    def exec_in(self, pod):
        self.stream(self.v1.connect_get_namespaced_pod_exec, pod, self.ns, command=["true"],
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
        self.wait(self.co.list_namespaced_custom_object, (self.G, self.V, self.ns, self.P), name, cr_ready)
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
        self.wait(self.co.list_namespaced_custom_object, (self.G, self.V, self.ns, self.P), name, ready)
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


def host_cpu_s():
    with open("/proc/stat") as f:
        v = [int(x) for x in f.readline().split()[1:]]
    busy = v[0] + v[1] + v[2] + v[5] + v[6] + v[7]  # user nice system irq softirq steal
    return busy / os.sysconf("SC_CLK_TCK")


def mem_available():
    with open("/proc/meminfo") as f:
        for line in f:
            if line.startswith("MemAvailable:"):
                return int(line.split()[1]) * 1024
    raise SystemExit("no MemAvailable")


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
        ms = {"create": [], "exec": [], "delete": []}
        cpu0 = host_cpu_s()
        for i in range(warmup + n):
            c, e, d = lifecycle(p)
            if i >= warmup:
                ms["create"].append(c)
                if e is not None:
                    ms["exec"].append(e)
                ms["delete"].append(d)
        res["phases"]["lifecycle"] = {"ms": ms, "host_cpu_s_per_lifecycle": (host_cpu_s() - cpu0) / (warmup + n)}
    except Exception as e:
        res["errors"]["lifecycle"] = f"{type(e).__name__}: {e}"
    try:
        tp = {}
        for conc in (1, 8, 32):
            t0 = time.perf_counter()
            with ThreadPoolExecutor(max_workers=conc) as pool:
                list(pool.map(lambda _: lifecycle(p), range(lifecycles)))
            wall = time.perf_counter() - t0
            tp[str(conc)] = {"lifecycles": lifecycles, "wall_s": wall, "per_s": lifecycles / wall}
        res["phases"]["throughput"] = tp
    except Exception as e:
        res["errors"]["throughput"] = f"{type(e).__name__}: {e}"
    held = []
    stop = "cap"
    try:
        base = mem_available()
        t0 = time.perf_counter()
        for i in range(cap):
            try:
                held.append(p.create())
            except Exception as e:  # a platform at its limit refuses: that bound is the density result
                stop = f"refused at {i}: {type(e).__name__}: {str(e)[:200]}"
                break
        wall = time.perf_counter() - t0
        time.sleep(5)
        used = base - mem_available()
        res["phases"]["density"] = {"count": len(held), "stop": stop, "start_wall_s": wall, "memory_bytes": used,
                                    "memory_per_sandbox_bytes": used / len(held) if held else None,
                                    "sandboxes_per_gib": len(held) / (used / 2**30) if used > 0 else None}
    except Exception as e:
        res["errors"]["density"] = f"{type(e).__name__} after {len(held)}: {e}"
    finally:
        for s_ in held:
            try:
                p.delete(s_)
            except Exception:
                pass
    if hasattr(p, "suspend"):
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
    with open(out_path, "w") as f:
        json.dump(res, f)
    print(json.dumps({k: v for k, v in res.items() if k != "phases"}))


if __name__ == "__main__":
    main()
