#!/usr/bin/env python3
"""Lead-only diagnostic: existing real capacity controls under declared extra Status traffic.

Prepared source only. Do not use its census rows as timing seeds. This imports the existing
boundary/census fixture; it changes neither the SUT nor the primary readiness predicate.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import signal
import subprocess
import sys
import threading
import time


def load_boundary():
    source = Path(__file__).with_name("scheduler_capacity_e2e.py")
    spec = importlib.util.spec_from_file_location("status_pressure_capacity", source)
    if spec is None or spec.loader is None:
        raise RuntimeError("the existing boundary fixture could not be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


# The capacity module loads scheduler_census.py by importlib too. Use that same module instance so
# Boundary, its inherited Harness methods and census.main all refer to the same original globals.
boundary = load_boundary()
census = boundary.census
PUBLICATION_BUSY = b"hex schedule: the node holds its most scheduler command publication (1)\n"


class Cancelled(census.Failed):
    """Only this diagnostic's explicitly signalled child cancellation, never a valid reply."""


def validate_reply(reply, value):
    census.require(isinstance(reply, dict), "scheduler CLI reply is not a JSON object")
    fields = {"key", "at", "tracked", "physical", "created", "ended", "offer", "observations", "refusals"}
    census.require(set(reply) == fields, "scheduler CLI reply has an unexpected schema")
    for field in ("tracked", "created", "ended"):
        census.require(type(reply[field]) is bool, "scheduler CLI boolean field is invalid")
    census.require(reply["at"] is None or (type(reply["at"]) is int and 0 <= reply["at"] <= (1 << 32) - 1),
                   "scheduler CLI placement is invalid")
    census.require(reply["physical"] is None or (isinstance(reply["physical"], str) and bool(reply["physical"])),
                   "scheduler CLI physical identity is invalid")
    census.require(reply["offer"] is None or isinstance(reply["offer"], dict), "scheduler CLI offer is invalid")
    census.require(isinstance(reply["observations"], list), "scheduler CLI observations are invalid")
    census.require(isinstance(reply["refusals"], list) and not reply["refusals"],
                   "scheduler returned an actual typed runtime refusal")
    key = reply["key"]
    if key is not None:
        census.require(isinstance(key, dict) and set(key) == {"owner", "epoch", "seq"}, "reply key is malformed")
        census.require(all(type(key[name]) is int and 0 <= key[name] <= (1 << bits) - 1
                           for name, bits in (("owner", 32), ("epoch", 64), ("seq", 64))), "reply key is out of range")
    if isinstance(value, dict) and set(value) == {"Status"}:
        census.require(key == value["Status"]["key"], "Status replied about a different key")


class StatusScope:
    """At most status_readers+1 active owned Status children, under the existing host child bound.

    Threads share the primary status loop's absolute operation deadline. Cancellation/reaping/joins
    share one cleanup deadline made from the same existing operator budget; it does not extend the
    primary readiness wait. A missing join/reap is always a failed diagnostic.
    """
    def __init__(self, harness, socket_path, key, number):
        self.harness, self.socket, self.key, self.number = harness, socket_path, key, number
        self.deadline = None
        self.cleanup_deadline = None
        self.stop = threading.Event()
        self.lock = threading.Lock()
        self.active = {}
        self.signalled = set()
        self.errors = []
        self.threads = []
        self.first = [threading.Event() for _ in range(harness.status_readers)]
        self.counts = [{"started": 0, "valid": 0, "publication_busy": 0, "cancelled": 0}
                       for _ in range(harness.status_readers)]
        self.launched = self.reaped = 0

    def start(self, deadline):
        census.remaining(deadline)
        if self.deadline is not None:
            census.require(self.deadline == deadline, "primary Status deadline changed inside its scope")
            return
        self.deadline = deadline
        with self.harness.children_lock:
            live = sum(process.poll() is None for _, process in self.harness.children)
            census.require(live + self.harness.status_readers + 1 <= self.harness.child_bound,
                           "declared Status readers plus primary exceed the existing host child budget")
        # No lock is held across Popen, subprocess waits or thread joins.
        for reader in range(self.harness.status_readers):
            thread = threading.Thread(target=self.reader, args=(reader,), daemon=True,
                                      name=f"status-pressure-{self.number}-{reader}")
            self.threads.append(thread)
            thread.start()
        for started in self.first:
            census.require(started.wait(timeout=census.remaining(deadline)),
                           "Status reader did not launch before the primary operation deadline")
        self.check()

    def check(self):
        with self.lock:
            error = self.errors[0] if self.errors else None
        census.require(error is None, "Status pressure reader failed: " + str(error))
        if self.stop.is_set():
            raise Cancelled("this Status scope was cancelled")

    def count(self, reader, field):
        if reader is not None:
            with self.lock:
                self.counts[reader][field] += 1

    def registered(self, process, reader):
        with self.lock:
            self.active[process] = reader
            self.launched += 1
        if reader is not None:
            self.count(reader, "started")
            self.first[reader].set()
        # A close can race Popen before registration. Such a child is cancelled immediately here,
        # and close's joins cannot succeed until that late child is also reaped.
        if self.stop.is_set():
            self.cancel_child(process)

    def cancel_child(self, process):
        if process.poll() is None:
            with self.lock:
                self.signalled.add(process)
            try:
                process.terminate()
            except ProcessLookupError:
                pass
            # No new grace timer: these are disposable read-only CLI clients. Escalate immediately
            # when still live, then reap/join within the inherited cleanup budget.
            if process.poll() is None:
                try:
                    process.kill()
                except ProcessLookupError:
                    pass

    def cancelled_exit(self, process):
        with self.lock:
            ours = process in self.signalled
        return ours and process.returncode in (-signal.SIGTERM, -signal.SIGKILL)

    def reaped_child(self, process):
        with self.lock:
            census.require(process.returncode is not None, "Status child is not reaped")
            census.require(process in self.active, "Status child lost its owned registration")
            del self.active[process]
            self.signalled.discard(process)
            self.reaped += 1

    def cancel(self):
        with self.lock:
            if self.cleanup_deadline is None:
                self.cleanup_deadline = time.monotonic() + self.harness.runtime["wait"]
            self.stop.set()
            active = list(self.active)
        for process in active:
            self.cancel_child(process)

    def fault(self, why):
        with self.lock:
            self.errors.append(f"{type(why).__name__}: {why}")
        self.cancel()

    def reader(self, reader):
        try:
            while not self.stop.is_set():
                self.harness.unique_call(self.socket, {"Status": {"key": self.key}},
                                         deadline=self.deadline, scope=self, reader=reader)
        except Cancelled:
            pass
        except Exception as why:
            self.fault(why)
        finally:
            # Startup failures wake the primary too; they are faults, never successful launches.
            self.first[reader].set()

    def finish(self):
        self.cancel()
        for thread in self.threads:
            thread.join(timeout=max(0, self.cleanup_deadline - time.monotonic()))
        with self.lock:
            joined = all(not thread.is_alive() for thread in self.threads)
            clean = not self.active and self.launched == self.reaped
            row = {"scope": self.number, "key": self.key, "socket": str(self.socket),
                   "phase": self.harness.phase, "status_readers": self.harness.status_readers,
                   "operation_deadline_seconds": self.harness.runtime["wait"],
                   "cleanup_budget_seconds": self.harness.runtime["wait"],
                   "all_readers_joined": joined, "all_status_children_reaped": clean,
                   "launched": self.launched, "reaped": self.reaped,
                   "readers": [dict(counts) for counts in self.counts], "errors": list(self.errors)}
        self.harness.report["status_pressure"]["scopes"].append(row)
        census.record(self.harness.runtime, "status-pressure-scope", **row)
        census.require(joined and clean, "Status pressure cancellation did not prove every join/reap")
        census.require(not row["errors"], "Status pressure reader failed; see scope report")
        census.require(self.deadline is not None and all(counts["started"] > 0 for counts in row["readers"]),
                       "declared additional Status callers were not all launched")


class PressureBoundary(boundary.Boundary):
    status_readers = None

    def __init__(self, args):
        census.require(args.deadline_seconds == 120, "this diagnostic preserves the original 120s operator budget")
        census.require(type(self.status_readers) is int and self.status_readers > 0, "--status-readers must be positive")
        super().__init__(args)
        self.call_lock = threading.Lock()
        self.call_serial = 0
        self.scope_serial = 0
        self.pressure_scope = None
        self.report["kind"] = "actual-scheduler-status-pressure-diagnostic"
        self.report["seed_generation"] = "forbidden: diagnostic pressure only; never timing seeds"
        self.report["status_pressure"] = {"status_readers": self.status_readers,
                                           "operator_declared_diagnostic_concurrency": True, "scopes": []}

    def status(self, socket_path, key, fact):
        census.require(self.pressure_scope is None, "primary Status scopes must not overlap")
        self.scope_serial += 1
        scope = StatusScope(self, socket_path, key, self.scope_serial)
        self.pressure_scope = scope
        try:
            # Keep the original Harness.status fact/deadline loop authoritative. The call override
            # starts readers only when this loop supplies its existing absolute deadline.
            return super().status(socket_path, key, fact)
        finally:
            try:
                scope.finish()
            finally:
                self.pressure_scope = None

    def call(self, socket_path, value, deadline=None):
        scope = self.pressure_scope
        if scope is not None:
            census.require(socket_path == scope.socket and value == {"Status": {"key": scope.key}},
                           "pressure scope may run only Status about its original key/socket")
            census.require(deadline is not None, "primary Harness.status omitted its original deadline")
            scope.start(deadline)
        return self.unique_call(socket_path, value, deadline=deadline, scope=scope)

    def unique_call(self, socket_path, value, deadline=None, scope=None, reader=None):
        if deadline is None:
            deadline = time.monotonic() + self.runtime["wait"]
        while True:
            if scope is not None:
                scope.check()
            census.remaining(deadline)
            with self.call_lock:
                self.call_serial += 1
                name = f"pressure-call-{self.call_serial}"
            request = self.root / (name + ".request.json")
            output_path = self.root / (name + ".reply.json")
            census.atomic_json(request, value)
            with output_path.open("xb") as output:
                process = self.spawn(name, [self.runtime["hex"], "schedule", "call", "--socket", str(socket_path),
                                           "--request", str(request)], stdout=output)
            if scope is not None:
                scope.registered(process, reader)
            # Preserve the imported census reporter's real CLI producer classification despite
            # unique log/request names. Its existing census_environment already gives a unique file.
            census.record(self.runtime, "call", process.pid, census=process.hex_census_path,
                          operation=census.operation(value), pressure_scope=None if scope is None else scope.number,
                          reader=reader, request=str(request), output=str(output_path))
            outcome = "failed"
            try:
                try:
                    process.wait(timeout=census.remaining(deadline))
                except subprocess.TimeoutExpired:
                    raise census.Failed("scheduler call exceeded the original operator deadline")
                with output_path.open("rb") as output:
                    data = output.read(self.runtime["max_frame"] + 1)
                census.require(len(data) <= self.runtime["max_frame"], "call output exceeds the common-wire bound")
                if scope is not None and scope.cancelled_exit(process):
                    outcome = "cancelled"
                    scope.count(reader, "cancelled")
                    raise Cancelled("owned Status child cancelled by this scope")
                if process.returncode != 0:
                    with (self.root / (name + ".log")).open("rb") as errors:
                        why = errors.read(self.runtime["max_frame"] + 1)
                    if process.returncode == 1 and not data and why == PUBLICATION_BUSY:
                        outcome = "publication_busy"
                        if scope is not None:
                            scope.count(reader, "publication_busy")
                        continue
                    raise census.Failed(f"call failed; see {name}.log in the private run directory")
                reply = json.loads(data)
                validate_reply(reply, value)
                with self.call_lock:
                    kind = census.operation(value)
                    self.success_controls[kind] = self.success_controls.get(kind, 0) + 1
                outcome = "valid"
                if scope is not None:
                    scope.count(reader, "valid")
                return reply
            finally:
                if process.poll() is None:
                    if scope is None:
                        self.stop(process, crash=True)
                    else:
                        scope.cancel()
                        process.wait(timeout=max(0, scope.cleanup_deadline - time.monotonic()))
                census.record(self.runtime, "census-call-exit", process.pid, census=process.hex_census_path,
                              returncode=process.returncode, operation=census.operation(value), success=outcome == "valid",
                              diagnostic=True, outcome=outcome, reader=reader,
                              pressure_scope=None if scope is None else scope.number)
                if scope is not None:
                    scope.reaped_child(process)

    def census_report(self):
        try:
            super().census_report()
        finally:
            if "census" in self.report:
                self.report["census"]["seed_generation"] = "forbidden: diagnostic pressure only; never timing seeds"


def main():
    parser = argparse.ArgumentParser(add_help=False, allow_abbrev=False)
    parser.add_argument("--status-readers", type=int, required=True,
                        help="operator-declared additional Status-only callers; diagnostic, never a timing seed")
    extension, rest = parser.parse_known_args()
    census.require(extension.status_readers > 0, "--status-readers must be positive")
    PressureBoundary.status_readers = extension.status_readers
    original_arguments, original_harness = sys.argv, census.Harness
    try:
        sys.argv = [sys.argv[0], *rest]
        census.Harness = PressureBoundary
        census.main()
    finally:
        sys.argv, census.Harness = original_arguments, original_harness


if __name__ == "__main__":
    main()
