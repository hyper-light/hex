#!/usr/bin/env python3
"""Create a concrete immutable fixture config from ROOT's actual backend/image/resource inputs."""

import argparse
import json
from pathlib import Path

from evidence import BACKENDS, hex_digest, load_json, require, sha256


def prepare(lab, source):
    require(hex_digest(source,40) and lab["backend"] in BACKENDS,"fixed source/backend required")
    require(lab["topology"] in ("laptop","peer") and lab["proof"] in ("full","selected"),"case identity")
    require(lab["wait_ms"]>0 and lab["byte_limit"]>0,"explicit finite bounds required")
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
    scoped_hex="@SCOPE@/helpers/hex"+(".exe" if lab.get("host_windows") else "")
    value["files"].append({"path":scoped_hex,"copy_from":lab["hex"]["path"],"sha256":lab["hex"]["sha256"]})
    value["hex"]={"path":scoped_hex,"sha256":lab["hex"]["sha256"]}
    for name in ("evidence.py","native_probe.py","cleanup_probe.py","load_probe.py","firecracker_probe.py"):
        value["files"].append({"path":f"@SCOPE@/helpers/{name}","text":(root/name).read_text()})
    witness="@SCOPE@/helpers/vulkan-witness"+(".exe" if lab.get("host_windows") else "")
    value["files"].append({"path":witness,"copy_from":lab["vulkan"]["path"],"sha256":lab["vulkan"]["sha256"]})
    def python_command(script,args):
        return {"path":lab["python"]["path"],"sha256":lab["python"]["sha256"],
                "args":[f"@SCOPE@/helpers/{script}",*args]}
    common=["--root-authorized","--timeout",str(lab["observer_timeout_seconds"]),
            "--byte-limit",str(lab["byte_limit"])]
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
