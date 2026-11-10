#!/usr/bin/env python3
"""ROOT-only exact-identity absence/owned-artifact observer. Never signals or removes anything."""

import argparse
import json
import os
import sys
from pathlib import Path

from evidence import InvalidEvidence, load_json, require
from native_probe import linux_process, mac_process, windows_process


def absent_process(pid, started, timeout, byte_limit):
    try:
        if sys.platform.startswith("linux"):
            # Check start before executable/fds: an owned zombie is still unreaped.
            path=Path("/proc")/str(pid)/"stat"
            if not path.exists():
                return True
            with path.open("rb") as source:
                raw=source.read(byte_limit+1)
            require(len(raw)<=byte_limit,"proc stat exceeded budget")
            text=raw.decode()
            fields=text[text.rfind(")")+2:].split()
            require(len(fields)>=20,"short process stat")
            return fields[19]!=str(started)
        if sys.platform=="darwin":
            row=mac_process(pid,timeout,byte_limit)
        elif os.name=="nt":
            row=windows_process(pid,timeout,byte_limit)
        else:
            raise InvalidEvidence("unverified absence observer platform")
        return row["start_token"]!=str(started)
    except ProcessLookupError:
        return True
    # A denied observation is uncertainty, not physical absence.


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root-authorized",action="store_true")
    parser.add_argument("--scope",type=Path,required=True)
    parser.add_argument("--identity",type=Path,action="append",required=True)
    parser.add_argument("--absent",type=Path,action="append",default=[])
    parser.add_argument("--timeout",type=float,required=True)
    parser.add_argument("--byte-limit",type=int,required=True)
    args=parser.parse_args()
    require(args.root_authorized and args.timeout>0 and args.byte_limit>0,"ROOT finite envelope required")
    scope=args.scope.resolve(strict=True)
    living=[]
    for path in args.identity:
        require(path.resolve(strict=True).is_relative_to(scope),"foreign identity artifact")
        # Host witness stdout is retained JSON, optionally with line-framed test diagnostics.
        with path.open("rb") as source:
            raw=source.read(args.byte_limit+1)
        require(len(raw)<=args.byte_limit,"identity output exceeded budget")
        rows=[]
        for line in raw.decode().splitlines():
            try:
                row=json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(row,dict) and "pid" in row and "start_token" in row:
                rows.append(row)
        require(len(rows)==1,"missing/ambiguous native process identity")
        row=rows[0]
        if not absent_process(row["pid"],row["start_token"],args.timeout,args.byte_limit):
            living.append({"pid":row["pid"],"start_token":row["start_token"]})
    artifacts=[]
    for path in args.absent:
        require(path.absolute().is_relative_to(scope),"foreign artifact path")
        if path.exists() or path.is_symlink():
            artifacts.append(str(path))
    print(json.dumps({"owned_native_processes":len(living),"owned_artifacts":len(artifacts),
                      "remaining_processes":living,"remaining_artifacts":artifacts}))
    return 0 if not living and not artifacts else 1


if __name__=="__main__":
    try:
        sys.exit(main())
    except (InvalidEvidence,OSError,KeyError) as error:
        print(f"UNVERIFIED cleanup: {error}",file=sys.stderr)
        sys.exit(1)
