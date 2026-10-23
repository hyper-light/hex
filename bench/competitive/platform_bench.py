#!/usr/bin/env python3
"""bench/competitive: a sandbox platform end to end through its own SDK (E2B or Daytona).

  platform_bench.py <e2b|daytona> <out.json> <n> <warmup> <lifecycles> <density cap>

Run on the bench host against a self-hosted stack (E2B `embed/` compose, Daytona `docker/` compose), or
against a hosted platform, which the result then labels hosted (its numbers include the network).
  e2b:      E2B_API_KEY, E2B_DOMAIN or E2B_API_URL as `docker compose exec ready cat /run/e2b/sdk.env`
            prints them; template "base".
  daytona:  DAYTONA_API_URL, DAYTONA_API_KEY; the default snapshot.
Phases: lifecycle (create until the sandbox runs a command, exec of `true`, delete; n sequential),
throughput (whole lifecycles at concurrency 1, 8, 32), and density (idle sandboxes up to the cap).
A platform's cost lives in many services, so CPU and memory are host-wide here: /proc/stat CPU time
per lifecycle and the MemAvailable drop per idle sandbox, on an otherwise idle bench host.
"""
import json
import os
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
    try:
        _, e = timed(p.exec_true, s)
    finally:
        _, d = timed(p.delete, s)
    return c, e, d


def main():
    kind, out_path = sys.argv[1], sys.argv[2]
    n, warmup, lifecycles, cap = (int(x) for x in sys.argv[3:7])
    p = {"e2b": E2B, "daytona": Daytona}[kind]()
    res = {"platform": kind, "hosted": os.environ.get("HOSTED") == "1", "n": n, "phases": {}, "errors": {}}
    try:
        ms = {"create": [], "exec": [], "delete": []}
        cpu0 = host_cpu_s()
        for i in range(warmup + n):
            c, e, d = lifecycle(p)
            if i >= warmup:
                ms["create"].append(c)
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
    try:
        base = mem_available()
        t0 = time.perf_counter()
        for _ in range(cap):
            held.append(p.create())
        wall = time.perf_counter() - t0
        time.sleep(5)
        used = base - mem_available()
        res["phases"]["density"] = {"count": len(held), "start_wall_s": wall, "memory_bytes": used,
                                    "memory_per_sandbox_bytes": used / len(held) if held else None,
                                    "sandboxes_per_gib": len(held) / (used / 2**30) if used > 0 else None}
    except Exception as e:
        res["errors"]["density"] = f"{type(e).__name__} after {len(held)}: {e}"
    finally:
        for s in held:
            try:
                p.delete(s)
            except Exception:
                pass
    with open(out_path, "w") as f:
        json.dump(res, f)
    print(json.dumps({k: v for k, v in res.items() if k != "phases"}))


if __name__ == "__main__":
    main()
