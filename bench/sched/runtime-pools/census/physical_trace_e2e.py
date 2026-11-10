#!/usr/bin/env python3
"""ROOT ONLY: armed physical diagnostic using the reviewed cold backend/cleanup.

Cold readiness precedes ordinary SSH setup, a deliberate diagnostic ordering difference.
No production fix, timing calibration, SDK census or seed eligibility is implied.
"""
import argparse
from contextlib import nullcontext
import importlib.util
from pathlib import Path
import sys


DEBUG = "HEX_DEBUG_SUBMIT_DF534D2F"


def load_module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError("the supplied fixture module could not be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_cold(path):
    return load_module(path, "physical_trace_cold_driver")


def trace_harness(cold, trace):
    # Capture the original class before main replaces the module's Harness binding.
    base = cold.census.Harness

    class PhysicalTrace(cold.ColdHarness):
        def __init__(self, args):
            cold.census.require(args.validate_seeds and args.mode == "guest" and args.deadline_seconds == 120,
                                "physical trace requires armed guest mode and the original explicit120s budget")
            super().__init__(args)
            for environment in (self.environment, self.backend_env):
                if trace:
                    environment[DEBUG] = "1"
                else:
                    environment.pop(DEBUG, None)
            self.report.update(kind="diagnostic-physical-armed-trace", calibration_eligibility=False,
                               trace_enabled=trace, startup_order="cold readiness before ordinary SSH setup",
                               physical_results="ordinary verified physical counters/controls and owned ledger")
            self.report["cold_boot"].pop("boxes_created")
            self.report["cold_boot"].pop("workload_success")

        def exercise(self):
            cold.ColdHarness.exercise(self)
            base.exercise(self)

        def backend(self, name):
            if name == "cold-node":
                cold.census.require(self.backend_process is None and self.cold_process is None
                                    and self.backend_state is None and self.backend_socket is None,
                                    "cold backend must be created exactly once in fresh state")
                return super().backend(name)
            cold.census.require(name == "real", "unexpected diagnostic backend name")
            ready = self.report["cold_boot"]
            cold.census.require(ready.get("backend_ready") and ready.get("ready_class_verified")
                                and self.cold_process is not None and self.backend_process is self.cold_process
                                and self.cold_process.poll() is None,
                                "physical cycles require the verified original live cold backend")
            cold.census.require(self.backend_state == self.root / "cold-node.node-state"
                                and self.backend_socket == self.root / "cold-node.sock"
                                and self.guest_receipt == self.root / "owned-guest.json"
                                and ready.get("receipt") == str(self.guest_receipt),
                                "cold backend state/socket/receipt was rebound")
            return self.backend_socket

    return PhysicalTrace


def main():
    parser = argparse.ArgumentParser(add_help=False, allow_abbrev=False)
    parser.add_argument("--cold-driver", type=Path, required=True, help="reviewed cold_boot_census.py path")
    parser.add_argument("--trace", action="store_true", help="enable the cache diagnostic binary's child trace")
    parser.add_argument("--fault", choices=("pre-ready", "post-ready"), help="ROOT-only existing cold cleanup fault control")
    options, rest = parser.parse_known_args()
    cold = load_cold(options.cold_driver.expanduser().resolve())
    original_arguments, original_harness = sys.argv, cold.census.Harness
    original_instances = cold.ColdHarness.instances
    try:
        faults = load_module(Path(__file__).with_name("cold_fault_e2e.py"), "physical_trace_fault_control") if options.fault else None
        with faults.inject_fault(cold, options.fault) if faults else nullcontext():
            cold.ColdHarness.instances = []
            cold.census.Harness = trace_harness(cold, options.trace)
            sys.argv = [original_arguments[0], *rest]
            cold.census.main()  # One backend; --n retains ordinary physical trials per phase.
    finally:
        sys.argv, cold.census.Harness = original_arguments, original_harness
        cold.ColdHarness.instances = original_instances
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
