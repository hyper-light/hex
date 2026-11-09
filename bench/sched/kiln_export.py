#!/usr/bin/env python3
"""Export kiln's CI legs over a window, read-only through kiln's HTTP API, for the kiln-CI side
measurement beside AC-T7.4.3's workload of record (bench/sched/kiln_workload.md). kiln_trace.py turns
the export into hex-sim's trace.

It reads what the running kiln serves today: `GET /api/runs` (newest first, paged on created_at,
at most 500 a page), `GET /api/runs/:id` (a run's jobs: queued, started and finished times, the
runner, executor and labels, and the peak RSS and CPU seconds kiln measured) and `GET /api/agents`
(each runner's CPU threads, memory and GPUs). It writes nothing to kiln and opens no database.

The admin token comes from `kiln token` and stays in this process: it is never printed, logged or
written. Every page and request is bounded; past a bound, or at the first request that fails, the
export is refused and nothing is written.
"""

import argparse
import datetime
import json
import subprocess
import sys
import urllib.request

# The input-bound rule (the lead, 2026-11-06): twice the most runs an export on record holds, the 1,488 of
# kiln-ci-20261022-20261105.export.json.gz.
MAX_RUNS = 2 * 1_488
# kiln's own most a page (`GET /api/runs`).
PAGE = 500
# Each page goes on only with a fresh run in the window, so the window's runs and one last page.
MAX_PAGES = MAX_RUNS + 1
TIMEOUT_S = 60
RUN_KEYS = ("id", "repo_name", "number", "pipeline", "event", "branch", "status", "created_at",
            "started_at", "finished_at")
JOB_KEYS = ("id", "key", "display", "status", "labels", "needs", "agent", "executor", "attempt",
            "queued_at", "started_at", "finished_at", "exit_code", "peak_rss", "cpu_seconds")


class Refused(Exception):
    pass


def token():
    try:
        out = subprocess.run(["kiln", "token"], capture_output=True, text=True, timeout=TIMEOUT_S)
    except (OSError, subprocess.TimeoutExpired) as e:
        raise Refused(f"`kiln token` failed: {type(e).__name__}") from None
    first = out.stdout.strip().splitlines()[:1]
    if out.returncode != 0 or not first:
        raise Refused(f"`kiln token` exited {out.returncode} with no token")
    return first[0]


def getter(base, secret):
    def get(path):
        req = urllib.request.Request(base + path, headers={"Authorization": f"Bearer {secret}"})
        try:
            with urllib.request.urlopen(req, timeout=TIMEOUT_S) as resp:
                return json.load(resp)
        except Exception as e:  # any failure refuses the export, naming the request
            raise Refused(f"GET {path}: {type(e).__name__}: {e}") from None
    return get


def runs_in(get, since, until):
    """Runs created in [since, until), oldest first. Pages strictly before a created_at, so the
    next page starts one second after the oldest seen and ids already seen are skipped."""
    seen, out, before = set(), [], until
    for _ in range(MAX_PAGES):
        page = get(f"/api/runs?limit={PAGE}&before={before}")
        fresh = [r for r in page if r["id"] not in seen]
        if not page or not fresh:
            if len(page) == PAGE:
                raise Refused(f"more than {PAGE} runs share created_at {before - 1}")
            return sorted(out, key=lambda r: (r["created_at"], r["number"]))
        for r in fresh:
            seen.add(r["id"])
            if since <= r["created_at"] < until:
                out.append(r)
        if len(out) > MAX_RUNS:
            raise Refused(f"more than {MAX_RUNS} runs in the window")
        oldest = min(r["created_at"] for r in page)
        if oldest < since:
            return sorted(out, key=lambda r: (r["created_at"], r["number"]))
        before = oldest + 1 if len(page) == PAGE else oldest
    raise Refused(f"more than {MAX_PAGES} pages of runs")


def runner(a):
    facts = a.get("facts") or {}
    cpu, mem = facts.get("cpu") or {}, facts.get("mem") or {}
    return {"name": a["name"], "os": a.get("os"), "arch": a.get("arch"), "labels": a.get("labels"),
            "ephemeral": a.get("ephemeral"), "threads": cpu.get("threads"),
            "mem_total": mem.get("total"),
            "gpus": [{"vendor": g.get("vendor"), "memory": g.get("memory")} for g in facts.get("gpus") or []]}


def unix(text):
    return int(datetime.datetime.fromisoformat(text.replace("Z", "+00:00")).timestamp())


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("out")
    ap.add_argument("--base", default="http://127.0.0.1:7070")
    ap.add_argument("--since", required=True, help="ISO 8601 UTC, inclusive")
    ap.add_argument("--until", required=True, help="ISO 8601 UTC, exclusive")
    a = ap.parse_args()
    since, until = unix(a.since), unix(a.until)
    try:
        if until <= since:
            raise Refused("the window ends before it starts")
        get = getter(a.base, token())
        runs = runs_in(get, since, until)
        out_runs = []
        for r in runs:
            detail = get(f"/api/runs/{r['id']}")
            row = {k: r.get(k) for k in RUN_KEYS}
            row["jobs"] = [{k: j.get(k) for k in JOB_KEYS} for j in detail.get("jobs") or []]
            out_runs.append(row)
        runners = [runner(x) for x in get("/api/agents")]
    except Refused as e:
        print(f"kiln_export: refused: {e}", file=sys.stderr)
        return 1
    export = {
        "source": f"kiln at {a.base} (adacachy), GET /api/runs, /api/runs/:id and /api/agents, "
                  f"runs created {a.since} to {a.until}, exported "
                  f"{datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')}",
        "window": [since, until],
        "runs": out_runs,
        "runners": runners,
    }
    with open(a.out, "w") as fh:
        json.dump(export, fh, separators=(",", ":"))
    print(f"kiln_export: {len(out_runs)} runs, {sum(len(r['jobs']) for r in out_runs)} jobs, "
          f"{len(runners)} runners -> {a.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
