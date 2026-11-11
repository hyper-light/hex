#!/usr/bin/env python3
"""ROOT-only actual peer-origin Status witness and event barrier; never a production relay."""

import argparse
import json
import os
import platform
import socket
import subprocess
import sys
import time
from pathlib import Path

from evidence import InvalidEvidence, artifact, finite_positive, load_json, require, sha256


def write(path,value):
    path.write_text(json.dumps(value,indent=2)+"\n")


def barrier(args):
    placement=load_json(args.placement,args.byte_limit)
    deadline=time.monotonic()+args.timeout
    listener=socket.socket(socket.AF_INET,socket.SOCK_STREAM)
    try:
        listener.bind(("127.0.0.1",0))
        listener.listen(1)
        listener.settimeout(args.timeout)
        address={"host":"127.0.0.1","port":listener.getsockname()[1],**placement}
        write(args.address,address)
        connection,_=listener.accept()
        with connection:
            connection.settimeout(max(0,deadline-time.monotonic()))
            source=connection.makefile("rb")
            raw=source.read(args.byte_limit+1)
            require(len(raw)<=args.byte_limit,"origin acknowledgment exceeded admitted bound")
            origin=json.loads(raw)
            reply=origin["reply"]
            require(origin["source"]==placement["source"] and reply.get("key")==placement["key"] and
                    reply.get("physical")==placement["physical"] and reply.get("at")==placement["runner"] and
                    reply.get("tracked") is True and reply.get("created") is True and reply.get("refusals")==[],
                    "actual origin did not observe the same live physical placement")
            write(args.output,origin)
        return {"origin_verified":True,"physical":placement["physical"],"origin_target":origin["target"]}
    finally:
        listener.close()


def observe(args):
    architecture={"arm64":"aarch64","aarch64":"aarch64","x86_64":"x86_64","amd64":"x86_64"}.get(platform.machine().lower())
    os_part="linux" if sys.platform.startswith("linux") else "apple-darwin" if sys.platform=="darwin" else "windows" if os.name=="nt" else None
    require(args.target and architecture and args.target.startswith(architecture+"-") and os_part and
            os_part in args.target,"origin target must match the actual native control host")
    placement=load_json(args.placement,args.byte_limit)
    require(sha256(args.hex)==args.hex_sha256,"source-pinned actual origin Hex required")
    require(not args.output.exists(),"new private origin output required")
    args.output.mkdir(parents=True)
    import shutil
    executable=args.output/("hex.exe" if os.name=="nt" else "hex")
    shutil.copy2(args.hex,executable)
    require(sha256(executable)==args.hex_sha256,"copied origin Hex changed")
    request=args.output/"request.json"
    write(request,{"Status":{"key":placement["key"]}})
    log=args.output/"reply.json";error=args.output/"stderr.log"
    argv=[str(executable),"schedule","call","--socket",str(args.socket),"--request",str(request)]
    with log.open("xb") as stdout,error.open("xb") as stderr:
        child=subprocess.Popen(argv,stdin=subprocess.DEVNULL,stdout=stdout,stderr=stderr)
        try:
            child.wait(timeout=args.timeout)
        except subprocess.TimeoutExpired:
            child.kill();child.wait(timeout=args.timeout)
            raise InvalidEvidence("actual origin Status timed out")
    require(child.returncode==0,"actual origin control call failed")
    reply=load_json(log,args.byte_limit)
    require(reply.get("physical")==placement["physical"] and reply.get("key")==placement["key"] and
            reply.get("created") is True and reply.get("tracked") is True and
            reply.get("at")==placement["runner"] and reply.get("refusals")==[],"origin Status mismatch")
    row={"schema":"hex-completion-origin-1","scope":"root-peer-origin","source":placement["source"],
         "target":args.target,"outcome":"verified","physical":reply["physical"],"reply":reply,
         "command":argv,"pid":child.pid,"exit":child.returncode,"reaped":True,
         "host":{"platform":platform.platform(),"machine":platform.machine()},
         "binary":{"path":executable.name,"sha256":sha256(executable),"bytes":executable.stat().st_size},
         "reply_artifact":{"path":log.name,"sha256":sha256(log),"bytes":log.stat().st_size}}
    write(args.output/"origin.json",row)
    if args.address:
        address=load_json(args.address,args.byte_limit)
        connection=socket.create_connection((args.forward_host or address["host"],
                                             args.forward_port or address["port"]),timeout=args.timeout)
        with connection:
            connection.sendall(json.dumps(row).encode())
            connection.shutdown(socket.SHUT_WR)
    return row


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode",choices=("barrier","observe","finalize"))
    parser.add_argument("--root-authorized",action="store_true")
    parser.add_argument("--placement",type=Path)
    parser.add_argument("--address",type=Path)
    parser.add_argument("--output",type=Path,required=True)
    parser.add_argument("--hex",type=Path)
    parser.add_argument("--hex-sha256")
    parser.add_argument("--socket",type=Path)
    parser.add_argument("--target")
    parser.add_argument("--forward-host")
    parser.add_argument("--forward-port",type=int)
    parser.add_argument("--runner-receipt",type=Path)
    parser.add_argument("--timeout",type=float,required=True)
    parser.add_argument("--byte-limit",type=int,required=True)
    args=parser.parse_args()
    require(args.root_authorized and finite_positive(args.timeout) and args.byte_limit>0,"ROOT finite envelope required")
    if args.mode=="barrier":
        result=barrier(args)
    elif args.mode=="observe":
        result=observe(args)
    else:
        result=load_json(args.output,args.byte_limit)
        artifact(args.output.parent,result["binary"])
        reply_path=artifact(args.output.parent,result["reply_artifact"])
        require(load_json(reply_path,args.byte_limit)==result["reply"],"origin raw reply changed")
        runner=load_json(args.runner_receipt,args.byte_limit)
        require(runner["source"]==result["source"],"origin/runner source mismatch")
        result["runner_receipt_sha256"]=sha256(args.runner_receipt)
        write(args.output,result)
    print(json.dumps(result))


if __name__=="__main__":
    try:
        main()
    except (InvalidEvidence,OSError,ValueError,KeyError,subprocess.SubprocessError) as error:
        print(f"UNVERIFIED peer origin: {error}",file=sys.stderr)
        sys.exit(1)
