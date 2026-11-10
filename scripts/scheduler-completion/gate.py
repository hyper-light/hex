#!/usr/bin/env python3
"""AC-T7.3.1: join retained run receipts into the supported native/routed matrix."""

import argparse
import json
import sys
from pathlib import Path

from evidence import (InvalidEvidence, TARGETS, check_run, load_json, matrix, require)


def assess(manifest, byte_limit):
    rows = []
    source = manifest["source"]
    for cell in manifest["cells"]:
        key = {name:cell[name] for name in ("target","backend","topology")}
        require(key["target"] in TARGETS, "unsupported origin target")
        if not cell.get("receipt"):
            rows.append({**key,"outcome":"unverified","reason":cell.get("reason","no native receipt")})
            continue
        path = Path(cell["receipt"]).resolve(strict=True)
        receipt = load_json(path, byte_limit)
        require(receipt["source"] == source, "matrix mixes source revisions")
        verified = check_run(path.parent, receipt, byte_limit)
        require(verified["backend"] == key["backend"] and verified["topology"] == key["topology"],
                "receipt belongs to a different backend/topology")
        require(verified["proof"] == "full", "selected-only proof cannot fill a full matrix cell")
        execution = receipt["target"]
        require(execution in TARGETS, "missing native execution target")
        routed = False
        if execution != key["target"]:
            origin_path = Path(cell["origin_receipt"]).resolve(strict=True)
            origin = load_json(origin_path, byte_limit)
            # This is a second actual control-path receipt, not a build or a requested target flag.
            require(origin.get("source") == source and origin.get("target") == key["target"] and
                    origin.get("scope") == "root-peer-origin" and origin.get("outcome") == "verified",
                    "no actual peer-origin control-path receipt")
            require(origin.get("physical") == cell.get("physical") and
                    origin.get("runner_receipt_sha256") == cell.get("runner_receipt_sha256"),
                    "origin/runner did not witness the same physical placement")
            from evidence import sha256
            require(sha256(path) == origin["runner_receipt_sha256"], "origin used another runner result")
            routed = True
        rows.append({**key,"outcome":"verified","native_receipt_verified":True,
                     "execution_target":execution,"routed_to_capable_peer":routed})
    return {"source":source, **matrix(rows)}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest",type=Path,required=True)
    parser.add_argument("--byte-limit",type=int,required=True)
    parser.add_argument("--output",type=Path,required=True)
    args=parser.parse_args()
    result=assess(load_json(args.manifest,args.byte_limit),args.byte_limit)
    args.output.write_text(json.dumps(result,indent=2)+"\n")
    print(json.dumps({"required":result["required_cells"],"verified":result["verified_cells"],
                      "complete":result["complete"],"report":str(args.output)}))
    return 0 if result["complete"] else 1


if __name__=="__main__":
    try:
        sys.exit(main())
    except (InvalidEvidence,OSError,KeyError) as error:
        print(f"UNVERIFIED matrix: {error}",file=sys.stderr)
        sys.exit(1)
