"""Pure/light behavior checks only; drivers run local stand-in programs alone. Never invokes a runtime,
GPU, VM, mutation or profiler."""

import argparse
import contextlib
import copy
import importlib.util
import io
import json
import os
import platform
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

import build
import load_probe
import origin
import run
from evidence import (BACKENDS, POLICY, RULES, STAGES, TARGETS, InvalidEvidence, artifact,
                      check_events, check_run, libtest_result, load_json, matrix, sha256)
from mutation import command, replacement

compare_path=Path(__file__).resolve().parents[2]/"bench/scheduler-completion/compare.py"
spec=importlib.util.spec_from_file_location("completion_compare",compare_path)
comparison=importlib.util.module_from_spec(spec)
spec.loader.exec_module(comparison)


def full_events():
    selected=[{"kind":"gpu","key":256}]
    data=[
        {"runtime_class":"oci","native_process_verified":True,"native_executable_sha256":"a"*64},
        {"selected":selected,"holder_count":1,"durable_before_execution":True},
        {"uuid":"b"*32,"selected":selected,"native_join_verified":True,"words":3,
         "native_inventory":[{"uuid":"b"*32,"pci":[0,1,0,0]}],
         "mismatches":0,"prefill_mismatches":3,"hardware":True,"dispatch_completed":True},
        {"host_accessible":1,"guest_accessible":0},
        {"dispatches":0,"refused":True},
        {"selected":selected,"holder_count":1,"compute_verified":True,"same_physical_box":True},
        {"selected":selected,"holder_count":1,"compute_verified":True,"same_physical_box":True,
         "catalog_reordered":True,"load_order_changed":True},
        {"boxes":0,"holders":0,"owned_native_processes":0,"owned_artifacts":0},
        {"selected_stages":9},
    ]
    return [dict(row,stage=stage,backend="oci",topology="laptop",outcome="verified")
            for row,stage in zip(data,STAGES)]


class EvidenceBehavior(unittest.TestCase):
    def test_accepts_exact_real_case_shape(self):
        self.assertEqual(check_events(full_events(),"oci","laptop")["native_stages"],9)

    def test_rejects_zero_selected(self):
        with self.assertRaises(InvalidEvidence):
            libtest_result("test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 13 filtered out;",1)

    def test_rejects_ignored_only(self):
        with self.assertRaises(InvalidEvidence):
            libtest_result("test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out;",1)

    def test_rejects_missing_capability(self):
        value=full_events();value[0]["runtime_class"]="process"
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop")

    def test_rejects_missing_native_join(self):
        value=full_events();value[2]["native_join_verified"]=False
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop")

    def test_rejects_recovery_repick(self):
        value=full_events();value[6]["selected"]=[{"kind":"gpu","key":512}]
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop")

    def test_hex_uuid_without_matching_native_identity_is_not_proof(self):
        value=full_events();value[2]["native_inventory"][0]["pci"]=[0,2,0,0]
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop")

    def test_shared_luid_cannot_select_a_physical_linked_node(self):
        value=full_events()
        first=value[2]["native_inventory"][0]
        first.update(luid="c"*16,node_mask=1)
        value[2]["native_inventory"].append({"uuid":"d"*32,"pci":[0,2,0,0],"luid":"c"*16,"node_mask":2})
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop")

    def test_multibit_luid_node_mask_is_not_exact(self):
        value=full_events();value[2]["native_inventory"][0].update(luid="c"*16,node_mask=3)
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop")

    def test_rejects_foreign_access(self):
        value=full_events();value[3]["guest_accessible"]=1
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop")

    def test_rejects_vacuous_wrong_id(self):
        value=full_events();value[4]["dispatches"]=1
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop")

    def test_rejects_unpoisoned_outputs(self):
        value=full_events();value[2]["prefill_mismatches"]=0
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop")

    def test_rejects_native_cleanup_gap(self):
        value=full_events();value[7]["owned_native_processes"]=1
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop")

    def test_preserves_selected_only_gaps(self):
        value=full_events();value=[value[i] for i in (0,1,2,7,8)]
        value[-1]["selected_stages"]=5
        result=check_events(value,"oci","laptop","selected")
        self.assertIn("foreign-device",result["remaining"])
        with self.assertRaises(InvalidEvidence):check_events(value,"oci","laptop","full")

    def test_rejects_duplicate_or_nonfinite_json(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/"receipt.json"
            for raw in ('{"exit":0,"exit":1}','{"value":NaN}'):
                path.write_text(raw)
                with self.assertRaises(InvalidEvidence):load_json(path,100)

    def test_checks_artifact_bytes_and_hash(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);path=root/"log";path.write_bytes(b"checked")
            row={"path":"log","sha256":sha256(path),"bytes":7}
            self.assertEqual(artifact(root,row),path)
            path.write_bytes(b"changed")
            with self.assertRaises(InvalidEvidence):artifact(root,row)

    def test_rejects_symlink_or_path_escape(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);path=root/"log";path.write_bytes(b"checked")
            link=root/"link";link.symlink_to(path)
            for name in ("link","../log"):
                with self.assertRaises(InvalidEvidence):
                    artifact(root,{"path":name,"sha256":sha256(path),"bytes":7})

    def test_matrix_never_promotes_missing_hardware(self):
        value=matrix([])
        self.assertEqual(value["required_cells"],39)
        self.assertEqual(value["verified_cells"],0)
        self.assertFalse(value["complete"])

    def test_nonlinux_firecracker_requires_peer_route(self):
        row={"target":"aarch64-apple-darwin","backend":"firecracker","topology":"peer",
             "outcome":"verified","native_receipt_verified":True,"execution_target":TARGETS[0]}
        with self.assertRaises(InvalidEvidence):matrix([row])

    def test_guard_replacement_is_exact_and_restorable(self):
        before=b"prefix\nactual_guard();\nsuffix\n"
        after=replacement(before,b"actual_guard();",b"")
        self.assertEqual(after,b"prefix\n\nsuffix\n")
        self.assertEqual(before,b"prefix\nactual_guard();\nsuffix\n")
        with self.assertRaises(InvalidEvidence):replacement(before,b"missing",b"")
        with self.assertRaises(InvalidEvidence):replacement(before+before,b"actual_guard();",b"")

    def test_tail_distribution_preserves_observed_samples(self):
        result=comparison.distribution([1,2,3,100])
        self.assertEqual((result["n"],result["p50"],result["p99"],result["max"]),(4,2,100,100))
        self.assertIsNone(result["median_interval"]["lower"])

    def test_median_confidence_uses_binomial_bounds(self):
        result=comparison.distribution(list(range(20)))
        self.assertGreaterEqual(result["median_interval"]["coverage"],0.95)
        self.assertLessEqual(result["median_interval"]["lower"],result["p50"])
        self.assertGreaterEqual(result["median_interval"]["upper"],result["p50"])

    def test_profiles_reject_nan_and_negative_values(self):
        for values in ([float("nan")],[float("inf")],[-1],[True]):
            with self.assertRaises(ValueError):comparison.distribution(values)

    def test_light_scope_cannot_be_promoted_to_native_run(self):
        receipt={"schema":"hex-completion-run-1","source":"a"*40,"policy":POLICY,"rules_sha256":RULES,
                 "proof":"full","scope":"light-unit"}
        with self.assertRaises(InvalidEvidence):check_run(Path("."),receipt,1000)

    def profiles(self):
        candidate={field:"same" for field in comparison.EQUIVALENT}
        candidate.update(schema="hex-completion-profile-1",build="release",scope="root-profile",
            source="a"*40,intended_samples=8,coverage=["caller","runtime","guest","induced-kernel"])
        baseline=copy.deepcopy(candidate);baseline["source"]="b"*40
        for label,row in (("candidate",candidate),("baseline",baseline)):
            metrics={name:8 if (direction=="higher")== (label=="candidate") else 1
                     for name,direction in comparison.METRICS.items()}
            row["samples"]=[{"run":str(index),"verified":True,"metrics":dict(metrics)} for index in range(8)]
        return candidate,baseline

    def test_missing_copy_measurements_cannot_claim_a_profile_target(self):
        candidate,baseline=self.profiles()
        candidate["samples"][0]["metrics"]["copied_bytes"]=None
        report=comparison.compare(candidate,baseline,4)
        self.assertFalse(report["all_targets_achieved"])
        self.assertIsNone(report["metrics"]["copied_bytes"]["ratio"])
        self.assertTrue(any("copied_bytes" in gap for gap in report["gaps"]))

    def test_profile_refuses_different_hardware(self):
        candidate,baseline=self.profiles();baseline["hardware"]="other"
        with self.assertRaises(ValueError):comparison.compare(candidate,baseline,4)

    def test_profile_target_misses_are_retained(self):
        candidate,baseline=self.profiles()
        for row in baseline["samples"]:row["metrics"]["latency_seconds"]=2
        report=comparison.compare(candidate,baseline,4)
        self.assertEqual(report["metrics"]["latency_seconds"]["ratio"],2)
        self.assertFalse(report["metrics"]["latency_seconds"]["achieved"])
        self.assertFalse(report["all_targets_achieved"])


def alive(pid):
    """Signal 0: only ProcessLookupError says the PID is gone."""
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        pass
    return True


def until(condition, budget):
    """Probe `condition` until it holds or `budget` seconds pass, at Popen.wait(timeout)'s cadence."""
    deadline, delay = time.monotonic() + budget, 0.0005
    while not condition() and time.monotonic() < deadline:
        delay = min(delay * 2, 0.05)
        time.sleep(delay)
    return condition()


def tree_pids(program):
    """The stand-in's `$$ $!` record beside it: its own PID and its grandchild's, or None until written."""
    record = Path(f"{program}.tree")
    pids = record.read_text().split() if record.is_file() else []
    return tuple(int(pid) for pid in pids) if len(pids) == 2 else None


def host_target():
    machine = {"arm64": "aarch64", "aarch64": "aarch64", "x86_64": "x86_64", "amd64": "x86_64"}[platform.machine().lower()]
    system = "apple-darwin" if sys.platform == "darwin" else "linux"
    return next(target for target in TARGETS if target.startswith(machine + "-") and system in target)


class ProcessGroupEnding(unittest.TestCase):
    """AC-T7.4.1: each driver ends its child's whole process group, on timeout, cancellation or exit, and
    records what it observed. The child is a stand-in that starts a grandchild sleeping until signaled;
    the oracle is signal 0 to both PIDs, never the driver's own report."""

    # TIMEOUT expires on a stand-in that hangs; the stand-in records its tree within milliseconds, and one
    # ended before that fails as vacuous. CLEANUP bounds each cleanup phase and the oracle. BOUND only
    # bounds waits that a fact ends first (a cancellation, readiness, a driver's exit).
    TIMEOUT, CLEANUP, BOUND = 1.0, 1.0, 10.0

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()

    def stand_in(self, name, first="", last="wait"):
        path = self.root / name
        path.write_text(f"#!/bin/sh\n{first}\n{shlex.quote(sys.executable)} -I -c 'import signal; signal.pause()' &\n"
                        f'echo "$$ $!" >"$0.tree"\n{last}\n')
        path.chmod(0o755)
        return path

    def load_argv(self, program, timeout):
        return [str(Path(load_probe.__file__).resolve()), "--root-authorized", "--binary", str(program),
                "--sha256", sha256(program), "--timeout", str(timeout), "--output", str(self.root / "load.log"),
                "--byte-limit", str(1 << 20)]

    @contextlib.contextmanager
    def tree_gone(self, program):
        """After the block, both PIDs `program` recorded must be gone within CLEANUP. A grandchild still
        alive is this test's own: it is killed by its recorded PID."""
        try:
            yield
        finally:
            pids = tree_pids(program)
            self.assertIsNotNone(pids, "stand-in ended before recording its grandchild: vacuous")
            until(lambda: not any(alive(pid) for pid in pids), self.CLEANUP)
            left = [pid for pid in pids if alive(pid)]
            if pids[1] in left:
                os.kill(pids[1], signal.SIGKILL)
            self.assertEqual(left, [], "the driver left its child's process tree running")

    def test_mutation_phase_timeout_ends_the_whole_tree(self):
        """AC-T7.4.1: a mutation phase past its budget has its grandchild ended too; its row says so."""
        program = self.stand_in("phase")
        with self.tree_gone(program):
            row = command([str(program)], self.root, "phase", self.TIMEOUT, self.CLEANUP)
        self.assertEqual((row["timed_out"], row["reaped"], row["group_signaled"], row["group_gone"]),
                         (True, True, True, True))

    def test_tree_ignoring_term_is_killed_once_cleanup_runs_out(self):
        """AC-T7.4.1: a tree that ignores TERM is KILLed, grandchild too, once the cleanup budget passes."""
        program = self.stand_in("phase", first="trap '' TERM")
        with self.tree_gone(program):
            row = command([str(program)], self.root, "phase", self.TIMEOUT, self.CLEANUP)
        self.assertEqual((row["exit"], row["group_gone"]), (-signal.SIGKILL, True))

    def test_exit_leaving_a_grandchild_has_it_ended_and_recorded(self):
        """AC-T7.4.1: a child that exits leaving a grandchild in its group has it ended; the row says so."""
        program = self.stand_in("phase", last="exit 0")
        with self.tree_gone(program):
            row = command([str(program)], self.root, "phase", self.TIMEOUT, self.CLEANUP)
        self.assertEqual((row["exit"], row["timed_out"], row["group_signaled"], row["group_gone"]),
                         (0, False, True, True))

    def test_cancelled_phase_ends_the_whole_tree(self):
        """AC-T7.4.1: ROOT's cancellation mid-phase (the InterruptedError mutation.py makes of SIGTERM)
        ends the grandchild too before it goes on."""
        def cancel(*_):
            raise InterruptedError("ROOT cancellation")
        self.addCleanup(signal.signal, signal.SIGUSR1, signal.signal(signal.SIGUSR1, cancel))
        program = self.stand_in("phase", last="kill -USR1 $PPID\nwait")
        with self.tree_gone(program), self.assertRaises(InterruptedError):
            command([str(program)], self.root, "phase", self.BOUND, self.CLEANUP)

    def test_run_timeout_ends_the_fixture_tree(self):
        """AC-T7.4.1: run.py's timeout ends the fixture's grandchild too; its receipt says the group is gone."""
        program, source, target = self.stand_in("fixture"), "a" * 40, host_target()
        built = self.root / "build"
        built.mkdir()
        shutil.copy2(program, built / "fixture")
        binary = {"path": "fixture", "sha256": sha256(program), "bytes": program.stat().st_size}
        (built / "build.json").write_text(json.dumps({"schema": "hex-completion-build-1", "source": source,
            "target": target, "test": "completion_backends", "exit": 0, "timed_out": False, "reaped": True,
            "group_gone": True, "source_unchanged": True, "binary": binary}))
        (self.root / "fixture.json").write_text(json.dumps({"proof": "selected", "source": source,
                                                            "backend": "oci", "topology": "laptop"}))
        args = argparse.Namespace(root_authorized=True, source=source, binary=program, binary_sha256=binary["sha256"],
            build_receipt=built / "build.json", config=self.root / "fixture.json", backend="oci", topology="laptop",
            proof="selected", target=target, timeout=self.TIMEOUT, cleanup_timeout=self.CLEANUP,
            byte_limit=1 << 20, output=self.root / "run")
        with self.tree_gone(self.root / "run/test-binary"), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(run.run(args), 1)
        receipt = load_json(self.root / "run/run.json", 1 << 20)
        self.assertEqual((receipt["timed_out"], receipt["reaped"], receipt["group_gone"],
                          receipt["assessment"]["outcome"]), (True, True, True, "unverified"))

    def test_build_timeout_ends_cargos_tree(self):
        """AC-T7.4.1: build.py's timeout ends Cargo's grandchild too; its receipt says the group is gone."""
        repo = self.root / "repo"
        repo.mkdir()
        (repo / "tracked").write_text("pinned\n")
        git = ["git", "-C", str(repo), "-c", "init.defaultBranch=main", "-c", "user.name=t", "-c", "user.email=t@t",
               "-c", "commit.gpgsign=false"]
        own = dict(os.environ, GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1")
        for argv in (["init", "-q"], ["add", "tracked"], ["commit", "-q", "-m", "pinned"]):
            subprocess.run([*git, *argv], env=own, check=True)
        source = subprocess.check_output([*git, "rev-parse", "HEAD"], env=own).decode().strip()
        program = self.stand_in("cargo")
        args = argparse.Namespace(root_authorized=True, repo=repo, source=source, cargo=program, package="hex-cli",
            test="completion_backends", target=TARGETS[0], output=self.root / "build", timeout=self.TIMEOUT,
            cleanup_timeout=self.CLEANUP, byte_limit=1 << 20, overlay_file=None, overlay_original_sha256=None,
            overlay_sha256=None)
        with self.tree_gone(program), self.assertRaises(InvalidEvidence):
            build.build(args)
        receipt = load_json(self.root / "build/build.json", 1 << 20)
        self.assertEqual((receipt["timed_out"], receipt["reaped"], receipt["group_gone"]), (True, True, True))

    def test_origin_status_timeout_ends_its_tree(self):
        """AC-T7.4.1: origin.py's timed-out Status call has its grandchild ended too."""
        program, placement = self.stand_in("hex"), self.root / "placement.json"
        placement.write_text(json.dumps({"key": "box"}))
        args = argparse.Namespace(placement=placement, hex=program, hex_sha256=sha256(program),
            socket=self.root / "socket", target=host_target(), output=self.root / "origin", address=None,
            forward_host=None, forward_port=None, timeout=self.TIMEOUT, byte_limit=1 << 20)
        with self.tree_gone(self.root / "origin/hex"), self.assertRaises(InvalidEvidence):
            origin.observe(args)

    def test_load_probe_timeout_ends_its_tree(self):
        """AC-T7.4.1: load_probe.py's timed-out telemetry run has its grandchild ended too."""
        program = self.stand_in("telemetry")
        with self.tree_gone(program), mock.patch.object(sys, "argv", self.load_argv(program, self.TIMEOUT)), \
                self.assertRaises(InvalidEvidence):
            load_probe.main()

    def test_sigterm_to_a_driver_ends_its_childs_tree(self):
        """AC-T7.4.1: a guard's SIGTERM to a driver (GNU timeout signals its own group, which the child's
        session has left) unwinds the driver, which ends the child's tree before it exits."""
        program = self.stand_in("telemetry")
        with self.tree_gone(program):
            driver = subprocess.Popen([sys.executable, *self.load_argv(program, self.BOUND)],
                                      stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            self.addCleanup(driver.wait, self.BOUND)
            self.addCleanup(driver.kill)
            until(lambda: tree_pids(program) is not None or driver.poll() is not None, self.BOUND)
            driver.terminate()
            _, error = driver.communicate(timeout=self.BOUND)
        self.assertEqual((driver.returncode, b"ROOT cancellation" in error), (1, True), error.decode())


if __name__=="__main__":
    unittest.main()
