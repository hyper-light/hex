#!/usr/bin/env python3
"""ROOT-only focused fixture build with actual clean source/command/binary provenance."""

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

from evidence import InvalidEvidence, TARGETS, hex_digest, require, sha256
from mutation import command


def git(repo,*argv):
    return subprocess.check_output(["git","-C",str(repo),*argv]).decode().strip()


def build(args):
    require(args.root_authorized,"ROOT owns final integrated builds")
    require(hex_digest(args.source,40) and args.target in TARGETS,"exact source/target required")
    repo=args.repo.resolve(strict=True)
    require(args.timeout>0 and args.cleanup_timeout>0 and args.byte_limit>0,"finite build envelope required")
    require(not args.output.resolve().is_relative_to(repo),"private build output must be outside the source checkout")
    require(git(repo,"rev-parse","HEAD")==args.source,"build checkout is not the fixed source")
    require(git(repo,"status","--porcelain")=="","build source must be clean and signed before execution")
    args.output.mkdir(parents=True,exist_ok=False)
    target=args.output/"target";cache=args.output/"cache/cargo";temporary=args.output/"temp"
    for path in (target,cache,temporary):path.mkdir(parents=True)
    environment=dict(os.environ,CARGO_TARGET_DIR=str(target),CARGO_HOME=str(cache),
                     CARGO_BUILD_JOBS="1",RUST_TEST_THREADS="1",RAYON_NUM_THREADS="1",TMPDIR=str(temporary))
    for name in list(environment):
        if name.startswith(("DYLD_","HEX_DEBUG_","SHARDS_DEBUG_")):del environment[name]
    argv=["nice","-n","10",str(args.cargo),"test","-p",args.package,"--test",args.test,
          "--locked","--no-run","--message-format=json","--target",args.target,"-j1"]
    log=args.output/"cargo.jsonl";stderr=args.output/"cargo.stderr"
    receipt={"schema":"hex-completion-build-1","source":args.source,"tree":git(repo,"rev-parse","HEAD^{tree}"),
             "target":args.target,"package":args.package,"test":args.test,"command":argv,
             "exit":None,"reaped":False,"timed_out":False}
    def persist():
        (args.output/"build.json").write_text(json.dumps(receipt,indent=2)+"\n")
    with log.open("xb") as out,stderr.open("xb") as err:
        child=subprocess.Popen(argv,cwd=repo,env=environment,stdin=subprocess.DEVNULL,stdout=out,stderr=err)
        receipt["pid"]=child.pid;persist()
        try:
            child.wait(timeout=args.timeout)
        except subprocess.TimeoutExpired:
            receipt["timed_out"]=True
            child.terminate()
            try:child.wait(timeout=args.cleanup_timeout)
            except subprocess.TimeoutExpired:
                child.kill();child.wait(timeout=args.cleanup_timeout)
        finally:
            receipt.update(exit=child.returncode,reaped=child.poll() is not None,
                           log_sha256=sha256(log),stderr_sha256=sha256(stderr))
            persist()
    require(child.returncode==0 and not receipt["timed_out"],"fixture build failed; retain exact source/log receipt")
    raw=log.read_bytes()
    require(len(raw)<=args.byte_limit,"Cargo artifact output exceeded admitted byte bound")
    artifacts=[]
    for line in raw.splitlines():
        row=json.loads(line)
        if row.get("reason")=="compiler-artifact" and row.get("target",{}).get("name")==args.test and row.get("executable"):
            artifacts.append(Path(row["executable"]))
    require(len(artifacts)==1,"Cargo did not emit exactly one intended test executable")
    binary=args.output/("fixture.exe" if "windows" in args.target else "fixture")
    shutil.copy2(artifacts[0],binary)
    receipt["binary"]={"path":binary.name,"sha256":sha256(binary),"bytes":binary.stat().st_size}
    receipt["source_unchanged"]=git(repo,"rev-parse","HEAD")==args.source and git(repo,"status","--porcelain")==""
    require(receipt["source_unchanged"],"source changed during build")
    persist()
    print(json.dumps({"receipt":str(args.output/"build.json"),"binary":str(binary),**receipt["binary"]}))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root-authorized",action="store_true")
    parser.add_argument("--repo",type=Path,required=True)
    parser.add_argument("--source",required=True)
    parser.add_argument("--cargo",type=Path,required=True)
    parser.add_argument("--package",choices=("hex-cli","hex-gpu"),required=True)
    parser.add_argument("--test",choices=("completion_backends","completion_telemetry","completion_windows_compute"),required=True)
    parser.add_argument("--target",choices=TARGETS,required=True)
    parser.add_argument("--output",type=Path,required=True)
    parser.add_argument("--timeout",type=float,required=True)
    parser.add_argument("--cleanup-timeout",type=float,required=True)
    parser.add_argument("--byte-limit",type=int,required=True)
    build(parser.parse_args())


if __name__=="__main__":
    try:main()
    except (InvalidEvidence,OSError,subprocess.SubprocessError) as error:
        print(f"UNVERIFIED build: {error}",file=sys.stderr)
        sys.exit(1)
