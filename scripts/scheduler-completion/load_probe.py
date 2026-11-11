#!/usr/bin/env python3
"""ROOT-only native telemetry test driver. Two event samples, no sleeps or missing-counter defaults."""

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

from evidence import InvalidEvidence, end_group, finite_positive, require, sha256, start_group, unwind_on_sigterm


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root-authorized",action="store_true")
    parser.add_argument("--binary",type=Path,required=True)
    parser.add_argument("--sha256",required=True)
    parser.add_argument("--timeout",type=float,required=True)
    parser.add_argument("--output",type=Path,required=True)
    parser.add_argument("--byte-limit",type=int,required=True)
    args=parser.parse_args()
    require(args.root_authorized and finite_positive(args.timeout) and args.byte_limit>0,"ROOT finite envelope required")
    require(sha256(args.binary)==args.sha256,"source-pinned telemetry binary required")
    env=dict(os.environ,HEX_COMPLETION_ROOT_AUTHORIZED="1",HEX_COMPLETION_LOAD_SAMPLES="2",RUST_TEST_THREADS="1")
    argv=[str(args.binary),"--ignored","--exact","completion_native_load_samples","--nocapture","--test-threads=1"]
    timed_out=False
    with args.output.open("xb") as log:
        child=start_group(argv,env=env,stdin=subprocess.PIPE,stdout=log,stderr=subprocess.STDOUT)
        try:
            child.communicate(input=b"ss",timeout=args.timeout)
        except subprocess.TimeoutExpired:
            timed_out=True
        finally:
            ended=end_group(child,args.timeout)
    require(not timed_out,f"native telemetry timed out; not a native pass; its process group gone: {ended['group_gone']}")
    require(ended["group_gone"],"native telemetry's process group is not gone")
    raw=args.output.read_bytes()
    require(len(raw)<=args.byte_limit and child.returncode==0,"native telemetry failed/oversized")
    from evidence import libtest_result
    libtest_result(raw.decode(),1)
    rows=[]
    for line in raw.decode().splitlines():
        # libtest may prefix the first JSON output with the current test name.
        at=line.find('{"round":')
        if at>=0:
            rows.append(json.loads(line[at:]))
    require(len(rows)==2 and all("order" in row and "loads" in row for row in rows),"missing native sample count")
    require(rows[1]["round"]==1,"native sample sequence mismatch")
    print(json.dumps(rows[1]))


if __name__=="__main__":
    unwind_on_sigterm()
    try:
        main()
    except (InvalidEvidence,OSError,subprocess.SubprocessError) as error:
        print(f"UNVERIFIED load: {error}",file=sys.stderr)
        sys.exit(1)
