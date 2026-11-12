#!/usr/bin/env python3
"""Create a concrete immutable fixture config from ROOT's actual backend/image/resource inputs."""

import argparse
import json
from pathlib import Path

from evidence import BACKENDS, TARGETS, artifact, finite_positive, hex_digest, load_json, natural, require, sha256


def case_inputs(lab, source):
    """Validate caller inputs without choosing an adapter, device or operation budget."""
    require(hex_digest(source, 40) and lab.get("backend") in BACKENDS, "fixed source/backend required")
    require(lab.get("topology") in ("laptop", "peer") and lab.get("proof") in ("full", "selected"), "case identity")
    require(lab.get("origin_target") in TARGETS and lab.get("execution_target") in TARGETS,
            "explicit origin and native execution targets required")
    require(lab["topology"] == "peer" or lab["origin_target"] == lab["execution_target"],
            "local fixture cannot substitute another execution target")
    if lab["topology"] == "peer":
        require(lab.get("require_remote_origin") is True, "peer topology requires an actual origin barrier")
    for name in ("wait_ms", "byte_limit", "max_items", "max_shader_bytes", "words", "rounds", "gpu_wait_ns",
                 "creating_entry_limit", "creating_event_limit", "creating_notification_bytes"):
        require(natural(lab.get(name), positive=True), "explicit positive integer input required: " + name)
    require(natural(lab.get("challenge")), "explicit non-boolean challenge required")
    require(finite_positive(lab.get("observer_timeout_seconds")), "finite native observer budget required")
    contract = lab.get("factory_contract")
    require(isinstance(contract, dict) and isinstance(contract.get("path"), str), "retained factory contract required")
    path = Path(contract["path"])
    require(path.is_absolute(), "explicit absolute factory contract path required")
    checked = artifact(path.parent, {**contract, "path": path.name})
    require(checked.stat().st_size > 0, "empty factory contract")
    return checked


def prepare(lab, source):
    contract = case_inputs(lab, source)
    for name in ("hex","python","vulkan","native_executable"):
        row=lab[name]
        path=Path(row["path"]).resolve(strict=True)
        require(sha256(path)==row["sha256"],f"{name} binary pin mismatch")
    require(lab["services"] and lab["payload"]["spec"].get("devices",[])==[],
            "ordinary services and scheduler-owned physical selection required")
    names=[row["name"] for row in lab["services"]]
    require(names.count("backend")==1 and names.count("scheduler")==1,"exact owned service roles")
    root=Path(__file__).resolve().parent
    value={key:lab[key] for key in ("backend","topology","proof","scratch","wait_ms","byte_limit",
        "hex","services","scheduler_socket","backend_socket","runner","record_root","demand","payload")}
    value.update(source=source,files=list(lab["files"]))
    if "native_resources" in lab:
        row = lab["native_resources"]
        require(isinstance(row, dict) and Path(row["path"]).is_absolute()
                and sha256(Path(row["path"])) == row["sha256"], "explicit pinned native resource producer required")
        value["native_resources"] = row
    value.update(origin_target=lab["origin_target"], execution_target=lab["execution_target"],
                 factory_contract_sha256=sha256(contract), factory_contract_path="@SCOPE@/factory-contract")
    for name in ("creating_entry_limit", "creating_event_limit", "creating_notification_bytes"):
        value[name] = lab[name]
    value["files"].append({"path": "@SCOPE@/factory-contract", "copy_from": str(contract), "sha256": sha256(contract)})
    scoped_hex="@SCOPE@/helpers/hex"+(".exe" if lab.get("host_windows") else "")
    value["files"].append({"path":scoped_hex,"copy_from":lab["hex"]["path"],"sha256":lab["hex"]["sha256"]})
    value["hex"]={"path":scoped_hex,"sha256":lab["hex"]["sha256"]}
    for name in ("evidence.py","native_probe.py","cleanup_probe.py","load_probe.py","firecracker_probe.py","origin.py",
                 "creating_events.py", "creating_events.ps1"):
        value["files"].append({"path":f"@SCOPE@/helpers/{name}","text":(root/name).read_text()})
    witness="@SCOPE@/helpers/vulkan-witness"+(".exe" if lab.get("host_windows") else "")
    value["files"].append({"path":witness,"copy_from":lab["vulkan"]["path"],"sha256":lab["vulkan"]["sha256"]})
    def python_command(script,args):
        return {"path":lab["python"]["path"],"sha256":lab["python"]["sha256"],
                "args":[f"@SCOPE@/helpers/{script}",*args]}
    if lab["execution_target"].endswith("apple-darwin"):
        value["creating_notifications"] = python_command("creating_events.py", ["--root-authorized",
            "--root", lab["record_root"], "--timeout", str(lab["wait_ms"] / 1000),
            "--entry-limit", str(lab["creating_entry_limit"]), "--event-limit", str(lab["creating_event_limit"])])
    elif "windows" in lab["execution_target"]:
        shell = lab["powershell"]
        require(Path(shell["path"]).is_absolute() and sha256(Path(shell["path"])) == shell["sha256"],
                "explicit pinned existing PowerShell executable required for filesystem notifications")
        value["creating_notifications"] = {"path": shell["path"], "sha256": shell["sha256"],
            "args": ["-NoProfile", "-NonInteractive", "-File", "@SCOPE@/helpers/creating_events.ps1",
                "-RecordRoot", lab["record_root"], "-TimeoutSeconds", str(lab["wait_ms"] / 1000),
                "-EventLimit", str(lab["creating_event_limit"]), "-BufferBytes", str(lab["creating_notification_bytes"])]}
    common=["--root-authorized","--timeout",str(lab["observer_timeout_seconds"]),
            "--byte-limit",str(lab["byte_limit"])]
    if lab.get("require_remote_origin"):
        value["origin_barrier"]=python_command("origin.py",["barrier",*common,
            "--placement","@SCOPE@/placement.json","--address","@SCOPE@/origin-address.json",
            "--output","@SCOPE@/origin-observed.json"])
    native=lab["native_identity"]
    value["native_observer"]=python_command("native_probe.py",[
        *common,"--identity",native["path"],"--format",native["format"],
        "--pointer",native.get("pointer","/init"),"--backend",lab["backend"],
        "--executable",lab["native_executable"]["path"],
        "--executable-sha256",lab["native_executable"]["sha256"]])
    cleanup=[*common,"--scope","@SCOPE@","--identity","@SCOPE@/native-proof.json"]
    for path in lab["cleanup_absent"]:
        cleanup.extend(["--absent",path])
    value["cleanup_observer"]=python_command("cleanup_probe.py",cleanup)
    value["host_inventory"]={"path":witness,"sha256":lab["vulkan"]["sha256"],
                             "args":["--mode","inventory","--max-items",str(lab["max_items"])]}
    program=lab["guest_vulkan_program"]
    shader=lab["guest_shader"]
    work=["--max-items",str(lab["max_items"]),"--uuid","@UUID@","--shader",shader,
          "--max-shader-bytes",str(lab["max_shader_bytes"]),"--words",str(lab["words"]),
          "--rounds",str(lab["rounds"]),"--challenge",str(lab["challenge"]),
          "--wait-ns",str(lab["gpu_wait_ns"])]
    # Program uses the ordinary public serde contract. Its paths belong inside the staged root/image.
    value["compute"]={"args":[program,"--mode","compute",*work],"env":lab.get("guest_env",[]),"cwd":None,"user":None}
    value["reject"]={"args":[program,"--mode","reject","--max-items",str(lab["max_items"]),
                              "--uuid","@UUID@"],"env":lab.get("guest_env",[]),"cwd":None,"user":None}
    value["inventory"]={"args":[program,"--mode","inventory","--max-items",str(lab["max_items"])],
                        "env":lab.get("guest_env",[]),"cwd":None,"user":None}
    if lab["backend"]=="firecracker":
        value["physical_translation"]=python_command("firecracker_probe.py",[
            *common,"--api",lab["firecracker_api"],"--host-sbdf",lab["selected_host_sbdf"],
            "--native-key","@KEY@"])
    if lab["proof"]=="full":
        host_work=["--mode","host-compute","--max-items",str(lab["max_items"]),
            "--uuid","@UUID@","--shader",lab["host_shader"],"--max-shader-bytes",str(lab["max_shader_bytes"]),
            "--words",str(lab["words"]),"--rounds",str(lab["rounds"]),"--challenge",str(lab["challenge"]),
            "--wait-ns",str(lab["gpu_wait_ns"])]
        value["foreign_host_compute"]={"path":witness,"sha256":lab["vulkan"]["sha256"],"args":host_work}
        value["wrong_uuid"]=lab["wrong_uuid"]
        require(hex_digest(lab["wrong_uuid"],32),"wrong-ID control must supply an explicit UUID")
        value["catalog"]=lab["catalog"]
        value["load_actor"]=lab["load_actor"]
        telemetry=lab["telemetry"]
        require(sha256(telemetry["path"])==telemetry["sha256"],"telemetry binary pin mismatch")
        for name in ("load_before","load_change"):
            value[name]=python_command("load_probe.py",[*common,"--binary",telemetry["path"],
                "--sha256",telemetry["sha256"],"--output",f"@SCOPE@/{name}.log"])
    return value


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lab",type=Path,required=True)
    parser.add_argument("--source",required=True)
    parser.add_argument("--byte-limit",type=int,required=True)
    parser.add_argument("--output",type=Path,required=True)
    args=parser.parse_args()
    result=prepare(load_json(args.lab,args.byte_limit),args.source)
    with args.output.open("x") as output:
        json.dump(result,output,indent=2,allow_nan=False)
        output.write("\n")


if __name__=="__main__":
    main()
