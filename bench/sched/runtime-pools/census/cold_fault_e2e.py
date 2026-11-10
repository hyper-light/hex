"""ROOT-only injected faults; closure remains the reviewed ColdHarness implementation."""
import argparse
from contextlib import contextmanager
import importlib.util
import sys


@contextmanager
def inject_fault(cold, fault):
    cold.census.require(fault in ("pre-ready", "post-ready"), "unknown cold fault control")
    if fault == "pre-ready":
        original = cold.ColdHarness.ready

        def ready(self, process, path):
            original(self, process, path)
            if process is self.backend_process:
                self.report["root_injection"] = {"case": "pre-ready", "trigger": "actual ready line read; before parent snapshot/receipt",
                                                 "known_record": str(self.backend_state / "@guest")}
                raise cold.census.Failed("ROOT pre-ready parent receipt-boundary fault")

        cold.ColdHarness.ready = ready
        try:
            yield
        finally:
            cold.ColdHarness.ready = original
    else:
        original_snapshot, original_finish = cold.ColdHarness.snapshot, cold.ColdHarness.finish_backend

        def snapshot(self, *args, **kwargs):
            if getattr(self, "root_finishing", False) and not getattr(self, "root_fault_sent", False):
                self.root_fault_sent = True
                self.report["root_injection"] = {"case": "post-ready", "trigger": "first original finish snapshot after ready/class/live capacity verification"}
                raise cold.census.Failed("ROOT post-ready finish snapshot fault")
            return original_snapshot(self, *args, **kwargs)

        def finish(self):
            self.root_finishing = True
            try:
                return original_finish(self)
            finally:
                self.root_finishing = False

        cold.ColdHarness.snapshot, cold.ColdHarness.finish_backend = snapshot, finish
        try:
            yield
        finally:
            cold.ColdHarness.snapshot, cold.ColdHarness.finish_backend = original_snapshot, original_finish


def main():
    parser = argparse.ArgumentParser(add_help=False, allow_abbrev=False)
    parser.add_argument("--cold-driver", required=True)
    parser.add_argument("--fault", choices=("pre-ready", "post-ready"), required=True)
    options, rest = parser.parse_known_args()
    spec = importlib.util.spec_from_file_location("root_actual_cold_fault", options.cold_driver)
    if spec is None or spec.loader is None:
        raise RuntimeError("the supplied cold driver could not be loaded")
    cold = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(cold)
    original_arguments = sys.argv
    try:
        with inject_fault(cold, options.fault):
            sys.argv = [original_arguments[0], *rest]
            return cold.main()
    finally:
        sys.argv = original_arguments


if __name__ == "__main__":
    raise SystemExit(main())
