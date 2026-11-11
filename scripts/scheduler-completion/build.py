#!/usr/bin/env python3
"""ROOT-only focused fixture build with actual clean source/command/binary provenance."""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

from evidence import (InvalidEvidence, TARGETS, end_group, finite_positive, hex_digest, require, sha256,
                      start_group, unwind_on_sigterm)


def git(repo,*argv):
    return subprocess.check_output(["git","-C",str(repo),*argv]).decode().strip()


def source_state(repo, source, overlay=None):
    """One declared guard overlay is the only admitted dirty-source state."""
    require(git(repo, "rev-parse", "HEAD") == source, "build checkout is not the fixed source")
    status = subprocess.check_output(["git", "-C", str(repo), "status", "--porcelain=v1", "-z"])
    if overlay is None:
        require(status == b"", "build source must be clean and signed before execution")
    else:
        filename = overlay["file"]
        relative = Path(filename)
        require(filename and not relative.is_absolute() and ".." not in relative.parts,
                "guard overlay escaped private source")
        require(status == b" M " + os.fsencode(filename) + b"\0",
                "only the exact unstaged guard overlay may differ from signed source")
        cursor = repo
        for part in relative.parts:
            cursor /= part
            require(not cursor.is_symlink(), "guard overlay cannot traverse a symlink")
        original = subprocess.check_output(["git", "-C", str(repo), "show", source + ":" + filename])
        require(hashlib.sha256(original).hexdigest() == overlay["original_sha256"]
                and sha256(cursor) == overlay["sha256"], "guard overlay original/current hashes disagree")
    paths = subprocess.check_output(["git", "-C", str(repo), "ls-files", "-z"]).split(b"\0")
    manifest = {}
    for name in paths:
        if not name:
            continue
        path = repo / os.fsdecode(name)
        manifest[os.fsdecode(name)] = (hashlib.sha256(os.fsencode(os.readlink(path))).hexdigest()
                                      if path.is_symlink() else sha256(path))
    return manifest


def fixture_artifact(row, repo, target, package, test, before):
    """Bind a newly compiled integration test to this source and empty target."""
    require(isinstance(row, dict) and row.get("reason") == "compiler-artifact",
            "fixture requires a Cargo compiler-artifact message")
    descriptor, profile = row.get("target"), row.get("profile")
    require(isinstance(descriptor, dict) and descriptor.get("name") == test
            and descriptor.get("kind") == ["test"] and descriptor.get("crate_types") == ["bin"]
            and isinstance(profile, dict) and profile.get("test") is True,
            "artifact is not the intended integration-test harness")
    # Cargo's JSON protocol: fresh:true means rustc did not run. An empty private
    # target must not accept a copied cross-cohort fixture as current build proof.
    require(row.get("fresh") is False, "cached fixture artifact is not current compilation proof")
    inputs = (("manifest_path", Path("crates") / package / "Cargo.toml"),
              ("src_path", Path("crates") / package / "tests" / (test + ".rs")))
    provenance = {"fresh": False}
    for field, relative in inputs:
        value = row.get(field) if field == "manifest_path" else descriptor.get(field)
        require(isinstance(value, str) and value, "artifact source path is missing")
        emitted, expected = Path(value), repo / relative
        require(emitted.is_absolute() and emitted.is_relative_to(repo)
                and emitted.resolve(strict=True) == expected
                and expected.is_file(), "artifact refers to another source checkout")
        require(before.get(relative.as_posix()) == sha256(expected),
                "artifact source differs from the pinned source manifest")
        provenance[field] = relative.as_posix()
    value = row.get("executable")
    require(isinstance(value, str) and value, "artifact executable is missing")
    emitted = Path(value)
    require(emitted.is_absolute() and emitted.is_relative_to(target),
            "artifact executable is outside the private target")
    executable = emitted.resolve(strict=True)
    require(executable.is_relative_to(target) and executable.is_file(),
            "artifact executable escapes the private target")
    filenames = row.get("filenames")
    require(isinstance(filenames, list) and value in filenames,
            "artifact executable is absent from Cargo's generated files")
    provenance["executable"] = executable.relative_to(target).as_posix()
    return executable, provenance


def build(args):
    require(args.root_authorized,"ROOT owns final integrated builds")
    require(hex_digest(args.source,40) and args.target in TARGETS,"exact source/target required")
    repo=args.repo.resolve(strict=True)
    require(finite_positive(args.timeout) and finite_positive(args.cleanup_timeout) and args.byte_limit>0,
            "finite build envelope required")
    args.output = args.output.resolve()
    require(not args.output.is_relative_to(repo),"private build output must be outside the source checkout")
    provided = (args.overlay_file, args.overlay_original_sha256, args.overlay_sha256)
    require(all(value is None for value in provided) or all(value is not None for value in provided),
            "guard overlay requires path, original hash and changed hash together")
    overlay = None if args.overlay_file is None else {
        "file": args.overlay_file, "original_sha256": args.overlay_original_sha256, "sha256": args.overlay_sha256}
    if overlay is not None:
        require(hex_digest(overlay["original_sha256"]) and hex_digest(overlay["sha256"])
                and overlay["original_sha256"] != overlay["sha256"], "exact differing overlay hashes required")
    before = source_state(repo, args.source, overlay)
    args.output.mkdir(parents=True,exist_ok=False)
    target=args.output/"target";cache=args.output/"cache/cargo";temporary=args.output/"temp"
    for path in (target,cache,temporary):path.mkdir(parents=True)
    require(not any(target.iterdir()), "fixture target must be newly created and empty")
    environment=dict(os.environ,CARGO_TARGET_DIR=str(target),CARGO_HOME=str(cache),
                     CARGO_BUILD_JOBS="1",RUST_TEST_THREADS="1",RAYON_NUM_THREADS="1",TMPDIR=str(temporary))
    for name in list(environment):
        if name.startswith(("DYLD_","HEX_DEBUG_","SHARDS_DEBUG_")):del environment[name]
    argv=["nice","-n","10",str(args.cargo),"test","-p",args.package,"--test",args.test,
          "--locked","--no-run","--message-format=json","--target",args.target,"-j1"]
    log=args.output/"cargo.jsonl";stderr=args.output/"cargo.stderr"
    receipt={"schema":"hex-completion-build-1","source":args.source,"tree":git(repo,"rev-parse","HEAD^{tree}"),
             "target":args.target,"package":args.package,"test":args.test,"command":argv,
             "exit":None,"reaped":False,"group_signaled":False,"group_gone":False,"timed_out":False,
             "target_directory":{"path":str(target),"created_empty":True,"reused":False}}
    if overlay is not None:
        receipt["overlay"] = overlay
    (args.output / "source-before.json").write_text(json.dumps(before, indent=2) + "\n")
    receipt["source_before_sha256"] = sha256(args.output / "source-before.json")
    receipt["source_before"] = {"path": "source-before.json", "sha256": receipt["source_before_sha256"],
                                "bytes": (args.output / "source-before.json").stat().st_size}
    def persist():
        (args.output/"build.json").write_text(json.dumps(receipt,indent=2)+"\n")
    with log.open("xb") as out,stderr.open("xb") as err:
        child=start_group(argv,cwd=repo,env=environment,stdin=subprocess.DEVNULL,stdout=out,stderr=err)
        receipt["pid"]=child.pid;persist()
        try:
            child.wait(timeout=args.timeout)
        except subprocess.TimeoutExpired:
            receipt["timed_out"]=True
        finally:
            receipt.update(end_group(child,args.cleanup_timeout),exit=child.returncode,
                           log_sha256=sha256(log),stderr_sha256=sha256(stderr))
            persist()
    require(child.returncode==0 and not receipt["timed_out"],"fixture build failed; retain exact source/log receipt")
    require(receipt["group_gone"],"fixture build's process group is not gone; retain exact receipt")
    after = source_state(repo, args.source, overlay)
    (args.output / "source-after.json").write_text(json.dumps(after, indent=2) + "\n")
    receipt["source_after_sha256"] = sha256(args.output / "source-after.json")
    receipt["source_after"] = {"path": "source-after.json", "sha256": receipt["source_after_sha256"],
                               "bytes": (args.output / "source-after.json").stat().st_size}
    receipt["source_unchanged"] = before == after
    persist()
    require(receipt["source_unchanged"],"source changed during build")
    raw=log.read_bytes()
    require(len(raw)<=args.byte_limit,"Cargo artifact output exceeded admitted byte bound")
    artifacts=[]
    for line in raw.splitlines():
        row=json.loads(line)
        require(isinstance(row, dict), "Cargo message must be a JSON object")
        descriptor = row.get("target")
        if (row.get("reason")=="compiler-artifact" and isinstance(descriptor, dict)
                and descriptor.get("name")==args.test and row.get("executable")):
            artifacts.append(row)
    require(len(artifacts)==1,"Cargo did not emit exactly one intended test executable")
    executable, provenance = fixture_artifact(artifacts[0], repo, target, args.package, args.test, before)
    provenance["sha256"] = sha256(executable)
    receipt["compiler_artifact"] = provenance
    binary=args.output/("fixture.exe" if "windows" in args.target else "fixture")
    shutil.copy2(executable,binary)
    receipt["binary"]={"path":binary.name,"sha256":sha256(binary),"bytes":binary.stat().st_size}
    require(receipt["binary"]["sha256"] == provenance["sha256"], "copied fixture differs from Cargo artifact")
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
    parser.add_argument("--overlay-file")
    parser.add_argument("--overlay-original-sha256")
    parser.add_argument("--overlay-sha256")
    build(parser.parse_args())


if __name__=="__main__":
    unwind_on_sigterm()
    try:main()
    except (InvalidEvidence,OSError,subprocess.SubprocessError) as error:
        print(f"UNVERIFIED build: {error}",file=sys.stderr)
        sys.exit(1)
