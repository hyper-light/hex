#!/usr/bin/env python3
"""Lead-only r329 regression: deny an oversized guest reservation, then run a valid one."""
import importlib.util
from pathlib import Path

source = Path(__file__).with_name("scheduler_census.py")
spec = importlib.util.spec_from_file_location("scheduler_census", source)
if spec is None or spec.loader is None:
    raise RuntimeError("the owned census module could not be loaded")
census = importlib.util.module_from_spec(spec)
spec.loader.exec_module(census)


class Boundary(census.Harness):
    def one_box(self, socket_path, label, target, link=False):
        requested = self.args.box_memory_mib
        census.require(self.mode == "guest" and requested > self.args.guest_memory_mib,
                       "boundary trial must request more than its configured backing guest")
        key = self.submit(socket_path, label + "-oversized")
        for operation in ({"Status": {"key": key}}, "Retry", {"Status": {"key": key}}):
            reply = self.call(socket_path, operation)
            if isinstance(operation, dict):
                census.require(reply.get("at") is None and not reply.get("created"),
                               "oversized reservation was assigned despite the backing pool")
        census.require(self.snapshot()["boxes"] == self.baseline,
                       "oversized reservation caused a forbidden physical creation")
        self.call(socket_path, {"Delete": {"key": key}})
        self.status(socket_path, key, lambda row: row.get("ended") or not row.get("tracked", True))
        census.record(self.runtime, "verified-oversized-refusal", key=key,
                      requested_mib=requested, backing_mib=self.args.guest_memory_mib,
                      phase=self.phase, physical_baseline_restored=True)
        self.report["verified_oversized_refusals"] = self.report.get("verified_oversized_refusals", 0) + 1
        # The positive control uses the valid workload from the lead's actual smoke record.
        self.args.box_memory_mib = 64
        self.report["positive_control_memory_mib"] = self.args.box_memory_mib
        try:
            super().one_box(socket_path, label + "-valid", target, link=link)
        finally:
            self.args.box_memory_mib = requested


if __name__ == "__main__":
    census.Harness = Boundary
    census.main()
