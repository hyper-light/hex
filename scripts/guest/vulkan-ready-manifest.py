#!/usr/bin/env python3
"""The node's GPU observer manifest (HEX_GUEST_GPU_OBSERVER_FILE, AC-T7.4.1) for a closure
scripts/guest/vulkan-ready.sh built, each bound in it from a record, and the calibration that records them:

  vulkan-ready-manifest.py calibrate OUT SELECTOR... [--runs N]
  vulkan-ready-manifest.py write OUT --guest-root DIR
  vulkan-ready-manifest.py first-dial-ns

`calibrate` measures the built Program through its closure on each GPU SELECTOR names (a Shards
--gpu-device selector, as pci:0000:01:00.0), one Shards VM per GPU and round, booted as the node boots its
GPU pool VM (SHARDS_BIN, HEX_GUEST_KERNEL of the `gpu` variant, HEX_GUEST_CPUS, HEX_GUEST_MEMORY_MIB), whose
init is vulkan-ready.sh's calibrator (vulkan-ready-calibrate.c): each run in a fresh cgroup, its caches
dropped first, its receipt the kernel's own cpu.stat, memory.peak, memory.events, pids.peak and
pids.events. Its rounds, in OUT/calibration:
- probe: one run a GPU with one queue family, the decoder's least: the device's own count comes back in
  the frame or in the Program's `queue-families N` refusal. max_queue_families is the largest: what the
  Program needs, a fact of each device. Neither Vulkan, Venus nor the frame bounds the count under a u32.
- measure: N runs a GPU (default SEEDABLE), unarmed.
- armed: N runs a GPU bounded as Hex bounds the observer's box (hex_core::cgroup::writes), each bound
  TIMES_SLOWEST times its census record. A run that ran out (cpu.stat nr_throttled, memory.events max, oom
  or oom_kill, pids.events max) records the bound it hit, so that bound doubles, and another armed
  round follows, until one runs nothing out (decision 9, waits_census.py's seed).
calibration/record.txt is the SHA-256 of every file the rounds wrote.

`write` derives the manifest from that record alone, re-reading every receipt, and refuses one that
would rest on less: a GPU with fewer than SEEDABLE samples, a run that failed, a last armed round that
ran out or held tighter bounds than the manifest's, or a closure or allocation file other than the one
calibrated. Its bounds:
- cpu_max_milli, memory_max, pids_max: TIMES_SLOWEST times each census record (its largest per-period
  CPU, memory.peak, pids.peak), raised to Hex's least (hex_api::MIN_CPU_MILLI, MIN_MEMORY);
- max_queue_families: the probe's;
- max_allocation_bytes: the Program's three storage() calls, GPUs x (physical-device + observation) +
  max_queue_families x queue-family, its own sizes on its ABI (allocation.txt);
- stderr: not configured. The node sizes it, as the Firecracker class sizes its VMM's output: what the
  refusal carrying it holds in one wire answer, and its receipt beside all else it keeps.
DIR is where the guest sees OUT/closure (the node's HEX_GUEST_SHARES mounts its i-th root, read-only, at
/run/hex/shares/i). The manifest's evidence is the SHA-256 of observer-evidence.txt, which cites each
bound's record and derivation. Writes OUT/observer.json and prints the two variables the node reads.

`first-dial-ns` prints a Linux node's first dial bound, the most its GPU pool start gives the Program.

Every number here is read from the source that states it: SEEDABLE (waits_census.py), TIMES_SLOWEST
(hex_core::patience), CPU_PERIOD_US (hex_core::cgroup), MIN_CPU_MILLI and MIN_MEMORY (hex_api), and the
boot records (bench/native/guest/records.tsv).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys

HEX = pathlib.Path(__file__).resolve().parent.parent.parent
LOADER = "lib/libvulkan.so.1"
DRIVER = "lib/libvulkan_virtio.so"
ICD = "icd/virtio_icd.json"
PROGRAM = "bin/hex-vulkan-guest-ready"
CALIBRATOR = "calibrator/hex-vulkan-ready-calibrate"
# hex_runtime::guest::CLASS on Linux: the records' name for the hypervisor Shards boots with there.
CLASS = "Linux-KVM"
RESOURCES = ("cpu", "memory", "pids")


class Refused(Exception):
    pass


def constant(path: str, name: str) -> int:
    """`name`'s value where `path` states it (a decimal, `_` grouped, or `a << b`)."""
    text = (HEX / path).read_text()
    m = re.search(rf"^(?:pub const {name}: \w+|{name}) = ([0-9_]+(?: << [0-9_]+)?);?$", text, re.M)
    if m is None:
        raise Refused(f"{path} no longer states {name}")
    value = m.group(1).replace("_", "")
    if "<<" in value:
        left, right = value.split("<<")
        return int(left) << int(right)
    return int(value)


SEEDABLE = constant("bench/competitive/probes/waits_census.py", "SEEDABLE")
TIMES = constant("crates/hex-core/src/patience.rs", "TIMES_SLOWEST")
PERIOD_US = constant("crates/hex-core/src/cgroup.rs", "CPU_PERIOD_US")
MIN_CPU_MILLI = constant("crates/hex-api/src/bounds.rs", "MIN_CPU_MILLI")
MIN_MEMORY = constant("crates/hex-api/src/bounds.rs", "MIN_MEMORY")


def first_dial_ns() -> int:
    """hex_runtime::guest::recorded_dial_bound, a node's first dial bound before it has seen a boot: twice
    its seed (dial_bound), which hex_client::guest::shards::seed_in gives as its class's recorded boot to
    ready, or with none, twice the slowest of any class."""
    rows = [line.split("\t") for line in (HEX / "bench/native/guest/records.tsv").read_text().splitlines()
            if line and not line.startswith("#")]
    boots = [(row[1], int(row[2])) for row in rows if len(row) >= 3 and row[0] == "boot_ready"]
    if not boots:
        raise Refused("bench/native/guest/records.tsv records no boot_ready")
    own = [micros for cls, micros in boots if cls == CLASS]
    seed = own[0] if own else 2 * max(micros for _, micros in boots)
    return 2 * seed * 1000


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def role(relative: str) -> str:
    if relative == PROGRAM:
        return "program"
    if relative == LOADER:
        return "loader"
    if relative == DRIVER:
        return "driver"
    if relative == ICD:
        return "icd"
    if relative.startswith("lib/ld-linux"):
        return "interpreter"
    return "dependency"


def closure(out: pathlib.Path) -> list[tuple[str, str]]:
    """closure.sha256's rows, each file checked against it."""
    rows = []
    for line in (out / "closure.sha256").read_text().splitlines():
        digest, relative = line.split(maxsplit=1)
        if sha256(out / "closure" / relative) != digest:
            raise Refused(f"{out / 'closure' / relative} is not the file closure.sha256 names")
        rows.append((digest, relative))
    return rows


def units(out: pathlib.Path) -> dict[str, int]:
    """allocation.txt: the Program's allocation units on its ABI (vulkan-ready-sizes.c)."""
    found = dict(line.split() for line in (out / "allocation.txt").read_text().splitlines())
    need = ("physical-device", "observation", "queue-family", "size-max")
    if set(found) != set(need):
        raise Refused(f"{out / 'allocation.txt'} names {sorted(found)}, not {list(need)}")
    return {k: int(v) for k, v in found.items()}


def allocation(sizes: dict[str, int], gpus: int, queues: int) -> int:
    """The Program's three storage() calls (hex-vulkan-guest-ready.c main): its devices' handles and
    observations, and one device's queue families at a time."""
    return gpus * (sizes["physical-device"] + sizes["observation"]) + queues * sizes["queue-family"]


def seed(samples: list[int], ran_out: list[int], seedable: int) -> int | None:
    """waits_census.py's seed: the largest bound the row ran out at, and its samples' max where it has at
    least `seedable` of them, whichever is larger."""
    candidates = list(ran_out)
    if len(samples) >= seedable:
        candidates.append(max(samples))
    return max(candidates) if candidates else None


class Run:
    """One run's receipt: run-N.cgroup (its cpu.stat samples, exit and the cgroup's files) and its output."""

    def __init__(self, folder: pathlib.Path, index: int):
        self.index = index
        text = (folder / f"run-{index}.cgroup").read_text()
        head, _, rest = text.partition("== ")
        self.samples = []
        self.exit = None
        for line in head.splitlines():
            word, *values = line.split()
            if word == "sample":
                self.samples.append((int(values[0]), int(values[1])))
            elif word == "exit":
                self.exit = int(values[0])
        self.files = {}
        for part in ("== " + rest).split("\n== "):
            name, _, body = part.removeprefix("== ").partition("\n")
            self.files[name] = body
        if self.exit is None or not self.samples or "memory.peak" not in self.files:
            raise Refused(f"{folder / f'run-{index}.cgroup'} is not a complete receipt")
        self.out = (folder / f"run-{index}.out").read_bytes()
        self.err = (folder / f"run-{index}.err").read_bytes()

    def flat(self, name: str, key: str) -> int:
        for line in self.files.get(name, "").splitlines():
            k, _, v = line.partition(" ")
            if k == key:
                return int(v)
        raise Refused(f"run {self.index}: {name} has no {key}")

    def value(self, resource: str) -> int:
        if resource == "memory":
            return int(self.files["memory.peak"])
        if resource == "pids":
            return int(self.files["pids.peak"])
        # The most CPU any period took: each sample's usage since the last over the period (or the time
        # between, where longer), as cpu.max holds a quota each period (hex_core::cgroup::CPU_PERIOD_US).
        peak, last_t, last_u = 0, 0, 0
        for t, u in self.samples:
            span = max(t - last_t, PERIOD_US)
            peak = max(peak, -(-(u - last_u) * 1000 // span))
            last_t, last_u = t, u
        return peak

    def ran_out(self, resource: str) -> bool:
        if resource == "cpu":
            return self.flat("cpu.stat", "nr_throttled") > 0
        if resource == "memory":
            return any(self.flat("memory.events", key) > 0 for key in ("max", "oom", "oom_kill"))
        return self.flat("pids.events", "max") > 0

    def queues(self, expected: int) -> int:
        """The frame's device rows' queue-family counts, the frame checked as far as its shape (the node's
        decoder, observer/driver.rs, is the frame's judge: built_program_frame_is_work_ready)."""
        lines = self.out.decode("ascii").split("\n")
        if lines[0] != f"hex-vulkan-guest-readiness 1 {expected}" or lines[-1] != "" or lines[-2] != f"ready {expected}":
            raise Refused(f"run {self.index}'s frame is not complete")
        # A device row's thirteenth word is its queue-family count (observer.rs `device`).
        counts = [int(line.split()[12]) for line in lines if line.startswith("device ")]
        if len(counts) != expected:
            raise Refused(f"run {self.index}'s frame has {len(counts)} devices, not {expected}")
        return max(counts)


class Round:
    def __init__(self, folder: pathlib.Path):
        self.folder = folder
        _, self.phase, gpu = folder.name.split("-")
        self.gpu = int(gpu.removeprefix("gpu"))
        self.args = dict(line.split() for line in (folder / "args").read_text().splitlines())
        done = (folder / "done").read_text().split() if (folder / "done").exists() else []
        made = int(done[1]) if len(done) == 2 and done[0] == "runs" else 0
        self.runs = [Run(folder, i) for i in range(1, made + 1)]
        self.complete = made == int(self.args["runs"]) and all(r.exit == 0 for r in self.runs)
        # The guest the runs had (the calibrator's guest.txt): release, cpus, pid_max, MemTotal.
        self.guest = {}
        if (folder / "guest.txt").exists():
            for line in (folder / "guest.txt").read_text().splitlines():
                key, _, value = line.partition(" ")
                self.guest[key.rstrip(":")] = value.strip()

    def bound(self, resource: str) -> int | None:
        value = self.args[{"cpu": "cpu-max-milli", "memory": "memory-max", "pids": "pids-max"}[resource]]
        return None if value == "-" else int(value)


def rounds(out: pathlib.Path) -> list[Round]:
    folder = out / "calibration"
    return [Round(f) for f in sorted(folder.iterdir()) if f.is_dir()]


def census(found: list[Round], gpus: int, seedable: int):
    """Each resource's record (the largest of each GPU's seed) and bound, and every GPU's samples, from
    the measure and armed rounds: a run that ran out of a resource is that resource's run-out at its
    bound, else that resource's sample. A run that failed otherwise is no sample."""
    samples = {(r, g): [] for r in RESOURCES for g in range(gpus)}
    ran_out = {(r, g): [] for r in RESOURCES for g in range(gpus)}
    for rnd in found:
        if rnd.phase == "probe":
            continue
        for run in rnd.runs:
            for resource in RESOURCES:
                if rnd.bound(resource) is not None and run.ran_out(resource):
                    ran_out[(resource, rnd.gpu)].append(rnd.bound(resource))
                elif run.exit == 0:
                    samples[(resource, rnd.gpu)].append(run.value(resource))
    records, bounds = {}, {}
    for resource in RESOURCES:
        seeds = [seed(samples[(resource, g)], ran_out[(resource, g)], seedable) for g in range(gpus)]
        records[resource] = None if any(s is None for s in seeds) else max(seeds)
        # Hex's least of each (hex_api::Bounds::check: pids_max 0 leaves no room for the process).
        floor = {"cpu": MIN_CPU_MILLI, "memory": MIN_MEMORY, "pids": 1}[resource]
        bounds[resource] = None if records[resource] is None else max(TIMES * records[resource], floor)
    return records, bounds, samples, ran_out


def boot(plan: dict, selector: str, folder: pathlib.Path, out: pathlib.Path) -> None:
    """One VM: the calibrator as init, the GPU, the closure read-only and the round's folder. Bounded by
    the bound of its boot and of each run's Program, each its first dial's (first_dial_ns)."""
    command = [plan["shards"], "run", "vm", "--kernel", plan["kernel"], "--init", str(out / CALIBRATOR),
               "--cpus", str(plan["cpus"]), "--memory", str(plan["memory_mib"]), "--gpu", "--gpu-device", selector,
               "--share", f"{out / 'closure'}:ro", "--share", str(folder)]
    runs = int((folder / "args").read_text().split("\n")[0].split()[1])
    deadline_s = (runs + 1) * plan["wait_ns"] / 1e9
    with open(folder / "console.txt", "wb") as console:
        vm = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=console, stderr=subprocess.STDOUT)
        try:
            status = vm.wait(timeout=deadline_s)
        except subprocess.TimeoutExpired:
            vm.terminate()
            try:
                vm.wait(timeout=plan["wait_ns"] / 1e9)
            except subprocess.TimeoutExpired:
                vm.kill()
                vm.wait()
            raise Refused(f"{folder.name}: its VM outlived its bound, {deadline_s:.0f} s; see {folder / 'console.txt'}")
    if status != 0 or not (folder / "done").exists():
        tail = (folder / "console.txt").read_bytes()[-2000:].decode(errors="replace")
        raise Refused(f"{folder.name}: shards ended {status} without the round's end:\n{tail}")


def round_args(plan: dict, queues: int, runs: int, bounds: dict | None) -> str:
    lines = [("runs", runs), ("expected", 1), ("queues", queues),
             ("allocation", allocation(plan["sizes"], len(plan["selectors"]), queues)),
             ("output", plan["sizes"]["size-max"]), ("wait-ns", plan["wait_ns"]), ("period-us", PERIOD_US),
             ("cpu-max-milli", bounds["cpu"] if bounds else "-"), ("memory-max", bounds["memory"] if bounds else "-"),
             ("pids-max", bounds["pids"] if bounds else "-")]
    return "".join(f"{k} {v}\n" for k, v in lines)


def calibrate(out: pathlib.Path, selectors: list[str], runs: int) -> int:
    need = {name: os.environ.get(name, "") for name in ("SHARDS_BIN", "HEX_GUEST_KERNEL", "HEX_GUEST_CPUS", "HEX_GUEST_MEMORY_MIB")}
    missing = [name for name, value in need.items() if not value]
    if missing:
        raise Refused(f"{', '.join(missing)} unset: the calibration boots its VMs as the node boots its GPU pool VM")
    folder = out / "calibration"
    if folder.exists() and any(folder.iterdir()):
        raise Refused(f"{folder} is not empty: a calibration is one record")
    folder.mkdir(parents=True, exist_ok=True)
    closure(out)
    plan = {
        "selectors": selectors,
        "shards": need["SHARDS_BIN"], "shards_sha256": sha256(pathlib.Path(need["SHARDS_BIN"])),
        "kernel": need["HEX_GUEST_KERNEL"], "kernel_sha256": sha256(pathlib.Path(need["HEX_GUEST_KERNEL"])),
        "cpus": int(need["HEX_GUEST_CPUS"]), "memory_mib": int(need["HEX_GUEST_MEMORY_MIB"]),
        "calibrator_sha256": sha256(out / CALIBRATOR),
        "closure": sha256(out / "closure.sha256"), "allocation": sha256(out / "allocation.txt"),
        "sizes": units(out), "wait_ns": first_dial_ns(), "runs": runs,
    }
    (folder / "plan.json").write_text(json.dumps(plan, indent=2) + "\n")
    sequence = 0

    def go(phase: str, gpu: int, text: str) -> Round:
        nonlocal sequence
        sequence += 1
        rnd = folder / f"{sequence:02d}-{phase}-gpu{gpu}"
        rnd.mkdir()
        (rnd / "args").write_text(text)
        print(f"{rnd.name}: {selectors[gpu]}", flush=True)
        boot(plan, selectors[gpu], rnd, out)
        return Round(rnd)

    queues = 0
    for gpu in range(len(selectors)):
        probe = go("probe", gpu, round_args(plan, 1, 1, None))
        run = probe.runs[-1]
        said = re.fullmatch(rb"hex-vulkan-guest-ready: queue-families ([0-9]+)\n", run.err)
        if run.exit == 0:
            queues = max(queues, run.queues(1))
        elif said:
            queues = max(queues, int(said.group(1)))
        else:
            raise Refused(f"{probe.folder.name}: the Program exited {run.exit}: {run.err.decode(errors='replace')}")
    for gpu in range(len(selectors)):
        rnd = go("measure", gpu, round_args(plan, queues, runs, None))
        if not rnd.complete:
            raise Refused(f"{rnd.folder.name}: run {len(rnd.runs)} exited {rnd.runs[-1].exit}: {rnd.runs[-1].err.decode(errors='replace')}")
    while True:
        found = rounds(out)
        _, bounds, _, _ = census(found, len(selectors), runs)
        pid_max = min(int(rnd.guest["pid_max"]) for rnd in found)
        if bounds["cpu"] > plan["cpus"] * 1000 or bounds["memory"] > plan["memory_mib"] << 20 or bounds["pids"] > pid_max:
            raise Refused(f"the census's bounds {bounds} exceed the pool VM's {plan['cpus']} CPUs, "
                          f"{plan['memory_mib']} MiB or {pid_max} processes")
        armed = [go("armed", gpu, round_args(plan, queues, runs, bounds)) for gpu in range(len(selectors))]
        outs = [rnd for rnd in armed if any(run.ran_out(r) for run in rnd.runs for r in RESOURCES)]
        failed = [rnd for rnd in armed if not rnd.complete and rnd not in outs]
        if failed:
            rnd = failed[0]
            raise Refused(f"{rnd.folder.name}: run {len(rnd.runs)} exited {rnd.runs[-1].exit} with nothing run out: {rnd.runs[-1].err.decode(errors='replace')}")
        if not outs:
            break
        print(f"ran out under {bounds}: another armed round", flush=True)
    files = sorted(p for p in folder.rglob("*") if p.is_file() and p.name != "record.txt")
    (folder / "record.txt").write_text("".join(f"{sha256(p)}  {p.relative_to(folder)}\n" for p in files))
    print(f"{folder / 'record.txt'} {sha256(folder / 'record.txt')}")
    return 0


def write(out: pathlib.Path, root: str) -> int:
    if not root.startswith("/"):
        raise Refused("--guest-root must be an absolute guest path")
    root = root.rstrip("/")
    folder = out / "calibration"
    record = folder / "record.txt"
    if not record.exists():
        raise Refused(f"no calibration record at {record}: every bound needs one (vulkan-ready-manifest.py calibrate)")
    listed = {}
    for line in record.read_text().splitlines():
        digest, relative = line.split(maxsplit=1)
        listed[relative] = digest
    present = {str(p.relative_to(folder)) for p in folder.rglob("*") if p.is_file() and p.name != "record.txt"}
    if present != set(listed) or any(sha256(folder / rel) != d for rel, d in listed.items()):
        raise Refused(f"{folder} is not what {record} records")
    plan = json.loads((folder / "plan.json").read_text())
    rows = closure(out)
    if sha256(out / "closure.sha256") != plan["closure"] or sha256(out / "allocation.txt") != plan["allocation"]:
        raise Refused("the closure or allocation.txt is not the one calibrated")
    sizes = units(out)
    gpus = len(plan["selectors"])
    found = rounds(out)
    probes = [rnd for rnd in found if rnd.phase == "probe"]
    queues = 0
    for rnd in probes:
        run = rnd.runs[-1]
        said = re.fullmatch(rb"hex-vulkan-guest-ready: queue-families ([0-9]+)\n", run.err)
        queues = max(queues, run.queues(1) if run.exit == 0 else int(said.group(1)) if said else 0)
    measured = [rnd for rnd in found if rnd.phase != "probe"]
    if sorted({rnd.gpu for rnd in probes}) != list(range(gpus)) or not queues:
        raise Refused("the record has no probe of each GPU's queue families")
    for rnd in measured:
        if int(rnd.args["queues"]) != queues or int(rnd.args["allocation"]) != allocation(sizes, gpus, queues):
            raise Refused(f"{rnd.folder.name} ran the Program with other queue or allocation bounds than the probe's")
        for run in rnd.runs:
            if run.exit == 0 and run.queues(1) > queues:
                raise Refused(f"{rnd.folder.name} run {run.index} reports more queue families than the probe")
    records, bounds, samples, ran_out = census(found, gpus, SEEDABLE)
    short = [f"gpu{g} {r}: {len(samples[(r, g)])}" for r in RESOURCES for g in range(gpus) if len(samples[(r, g)]) < SEEDABLE]
    if short:
        raise Refused(f"fewer than SEEDABLE ({SEEDABLE}) samples, so no census record: {', '.join(short)}")
    for gpu in range(gpus):
        last = [rnd for rnd in found if rnd.phase == "armed" and rnd.gpu == gpu]
        if not last:
            raise Refused(f"gpu{gpu} ran no armed round")
        held = last[-1]
        if not held.complete or any(run.ran_out(r) for run in held.runs for r in RESOURCES):
            raise Refused(f"{held.folder.name}, gpu{gpu}'s last armed round, ran out or failed: another round is owed")
        if any(held.bound(r) > bounds[r] for r in RESOURCES):
            raise Refused(f"{held.folder.name} held tighter bounds than the census gives now")
    evidence = [
        "hex-vulkan-guest-ready observer evidence 1",
        f"closure {plan['closure']} (sha256 of closure.sha256)",
        f"allocation {plan['allocation']} (sha256 of allocation.txt): " + ", ".join(f"{k} {v}" for k, v in sizes.items()),
        f"calibration {sha256(record)} (sha256 of calibration/record.txt): shards {plan['shards_sha256']}, "
        f"kernel {plan['kernel_sha256']}, calibrator {plan['calibrator_sha256']}, {plan['cpus']} CPUs, "
        f"{plan['memory_mib']} MiB, guest kernel {found[0].guest.get('release', 'unrecorded')}, GPUs "
        + ", ".join(f"gpu{g} {s}" for g, s in enumerate(plan["selectors"])),
        f"max_queue_families {queues}: the Program's need, the most queue families a GPU's probe reports",
        f"max_allocation_bytes {allocation(sizes, gpus, queues)}: the Program's three storage() calls, "
        f"{gpus} x ({sizes['physical-device']} + {sizes['observation']}) + {queues} x {sizes['queue-family']}",
    ]
    names = {"cpu": "cpu_max_milli (largest per-period CPU, thousandths)", "memory": "memory_max (memory.peak, bytes)",
             "pids": "pids_max (pids.peak)"}
    for resource in RESOURCES:
        per_gpu = "; ".join(f"gpu{g} n={len(samples[(resource, g)])} max={max(samples[(resource, g)])} "
                            f"ran out at {sorted(set(ran_out[(resource, g)])) or 'none'}" for g in range(gpus))
        evidence.append(f"{names[resource]} {bounds[resource]}: {TIMES} (TIMES_SLOWEST) x record {records[resource]} ({per_gpu})")
    evidence.append("stderr: the node's own, what the refusal carrying it holds in one wire answer and its receipt beside all else; not configured")
    text = "\n".join(evidence) + "\n"
    (out / "observer-evidence.txt").write_text(text)
    artifacts, interpreter = [], None
    for digest, relative in rows:
        host = out / "closure" / relative
        kind = role(relative)
        if kind == "interpreter":
            interpreter = f"{root}/{relative}"
        artifacts.append({"role": kind, "host": str(host), "guest": f"{root}/{relative}",
                          "bytes": host.stat().st_size, "sha256": digest})
    if interpreter is None:
        raise Refused("the closure has no dynamic linker")
    manifest = {
        "evidence": hashlib.sha256(text.encode()).hexdigest(),
        "artifacts": artifacts,
        "launch": [interpreter, "--library-path", f"{root}/lib", f"{root}/{PROGRAM}"],
        "max_queue_families": queues,
        "max_allocation_bytes": allocation(sizes, gpus, queues),
        "bounds": {
            "cpu_weight": None,
            "cpu_max_milli": bounds["cpu"],
            "memory_max": bounds["memory"],
            "memory_high": None,
            "io_weight": None,
            "io_latency_us": None,
            "io_max": None,
            "pids_max": bounds["pids"],
        },
    }
    body = json.dumps(manifest, indent=2) + "\n"
    path = out / "observer.json"
    path.write_text(body)
    print(text, end="")
    print(f"HEX_GUEST_GPU_OBSERVER_FILE={path}")
    print(f"HEX_GUEST_GPU_OBSERVER_SHA256={hashlib.sha256(body.encode()).hexdigest()}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    c = commands.add_parser("calibrate")
    c.add_argument("out", type=pathlib.Path)
    c.add_argument("selectors", nargs="+")
    c.add_argument("--runs", type=int, default=SEEDABLE)
    w = commands.add_parser("write")
    w.add_argument("out", type=pathlib.Path)
    w.add_argument("--guest-root", required=True)
    commands.add_parser("first-dial-ns")
    args = parser.parse_args()
    try:
        if args.command == "first-dial-ns":
            print(first_dial_ns())
            return 0
        if args.command == "calibrate":
            if args.runs <= 0:
                raise Refused("--runs must be positive")
            return calibrate(args.out.resolve(), args.selectors, args.runs)
        return write(args.out.resolve(), args.guest_root)
    except Refused as why:
        print(f"vulkan-ready-manifest.py: {why}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
