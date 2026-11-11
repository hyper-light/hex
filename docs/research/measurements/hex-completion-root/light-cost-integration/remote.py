import hashlib,io,json,os,platform,re,subprocess,sys,tarfile,time,tomllib
from pathlib import Path,PurePosixPath
root=Path(sys.argv[1]);expected,revision,seedhash=sys.argv[2:5]
root.mkdir(parents=True,exist_ok=False)
data=sys.stdin.buffer.read()
if hashlib.sha256(data).hexdigest()!=expected:raise ValueError("archive SHA mismatch")
(root/"source.tar.gz").write_bytes(data)
source=root/"source";source.mkdir()
with tarfile.open(fileobj=io.BytesIO(data),mode="r:gz") as bundle:
 for m in bundle.getmembers():
  p=PurePosixPath(m.name)
  if p.is_absolute() or ".." in p.parts or not (m.isfile() or m.isdir()):raise ValueError("nonregular member")
 bundle.extractall(source,filter="data")
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
seed=source/".cohort-inputs/shards-Cargo.lock"
if sha(seed)!=seedhash:raise ValueError("seed SHA mismatch")
manifest={str(p.relative_to(source)):sha(p) for p in sorted(source.rglob("*")) if p.is_file()}
(root/"source-before.json").write_text(json.dumps(manifest,indent=2)+"\n")
for name in ["target","cargo","temp"]:(root/name).mkdir()
rustup=Path.home()/".cache/hex-lead/rustup-scheduler-copy";tc=rustup/"toolchains/1.99.1-aarch64-apple-darwin/bin"
env={k:v for k,v in os.environ.items() if not k.startswith(("DYLD_","HEX_DEBUG_","SHARDS_DEBUG_"))}
env.update(PATH=str(tc)+os.pathsep+env["PATH"],RUSTUP_HOME=str(rustup),RUSTC=str(tc/"rustc"),RUSTDOC=str(tc/"rustdoc"),CARGO_HOME=str(root/"cargo"),CARGO_TARGET_DIR=str(root/"target"),CARGO_BUILD_JOBS="1",RUST_TEST_THREADS="1",RAYON_NUM_THREADS="1",TMPDIR=str(root/"temp"))
phases=[('fit-cost', ['test', '-p', 'hex-sim', '--lib', '--locked', 'index::tests::costed_vm_candidates_and_admission_charge_the_same_original_demand', '--', '--exact', '--nocapture', '--test-threads=1']), ('fit-recovery', ['test', '-p', 'hex-sim', '--lib', '--locked', 'index::tests::persisted_allocation_survives_cost_refresh_without_recalculation', '--', '--exact', '--nocapture', '--test-threads=1']), ('peer-cost', ['test', '-p', 'hex-sim', '--lib', '--locked', 'peer::tests::runtime_allocation_survives_owner_receiver_and_profile_change', '--', '--exact', '--nocapture', '--test-threads=1']), ('peer-codec', ['test', '-p', 'hex-sim', '--lib', '--locked', 'peer::tests::allocation_message_and_record_codec_rejects_forged_charge', '--', '--exact', '--nocapture', '--test-threads=1']), ('client-link', ['test', '-p', 'hex-client', '--lib', '--locked', 'guest::tests::the_first_dial_waits_for_the_guest_and_later_ones_try_once', '--', '--exact', '--nocapture', '--test-threads=1']), ('clippy', ['clippy', '-p', 'hex-api', '-p', 'hex-core', '-p', 'hex-client', '-p', 'hex-sim', '--all-targets', '--locked', '--', '-D', 'warnings'])]

result={"source":revision,"archive_sha256":expected,"seed_sha256":seedhash,"scope":"ROOT one stale wire fixture correction plus previously unrun fit/peer original charge and managed client adapter integration; no native backend/VM/GPU invocation; no budget-suite or new SDK unit rerun","platform":platform.platform(),"driver_pid":os.getpid(),"phases":[]}
for name,args in phases:
 if name=="physical":(source/"vendor/shards/Cargo.lock").write_bytes(seed.read_bytes())
 command=["nice","-n","10",str(tc/"cargo")]+args
 phase={"name":name,"command":command};result["phases"].append(phase)
 with (root/(name+".log")).open("xb") as log:
  child=subprocess.Popen(command,cwd=source,env=env,stdout=log,stderr=subprocess.STDOUT);phase["pid"]=child.pid
  (root/"running.json").write_text(json.dumps(result,indent=2)+"\n")
  try:phase["exit"]=child.wait(timeout=900)
  except subprocess.TimeoutExpired:
   phase["timed_out"]=True;child.terminate()
   try:child.wait(timeout=10)
   except subprocess.TimeoutExpired:child.kill();child.wait()
   phase["exit"]=child.returncode
 phase["reaped"]=child.returncode is not None
 raw=(root/(name+".log")).read_bytes();phase.update(sha256=hashlib.sha256(raw).hexdigest(),bytes=len(raw))
 summaries=re.findall(rb"test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; .*?(\d+) filtered out",raw)
 if name!="clippy":
  phase["summaries"]=[dict(zip(["passed","failed","ignored","filtered"],map(int,row))) for row in summaries]
  phase["nonzero"]=bool(summaries) and sum(int(row[0]) for row in summaries)>0
 print(json.dumps(phase),flush=True)
 if phase["exit"]!=0 or phase.get("timed_out") or not phase.get("nonzero",True):break
post={str(p.relative_to(source)):sha(p) for p in sorted(source.rglob("*")) if p.is_file()}
(root/"source-after.json").write_text(json.dumps(post,indent=2)+"\n")
changed=[p for p in set(manifest)|set(post) if manifest.get(p)!=post.get(p)]
result["source_changes"]=sorted(changed)
lock=source/"vendor/shards/Cargo.lock"
if lock.exists():
 registry=lambda data:{(p["name"],p["version"],p.get("checksum")) for p in tomllib.loads(data).get("package",[]) if p.get("source","").startswith("registry+")}
 result["registry_subset_seed"]=registry(lock.read_text())<=registry(seed.read_text())
 result["resolved_lock_sha256"]=sha(lock)
result["temp_entries"]=len(list((root/"temp").iterdir()))
result["exit"]=0 if len(result["phases"])==len(phases) and all(p["exit"]==0 and not p.get("timed_out") and p.get("nonzero",True) for p in result["phases"]) and set(changed)<={"vendor/shards/Cargo.lock"} and result.get("registry_subset_seed",True) else 1
(root/"result.json").write_text(json.dumps(result,indent=2)+"\n")
print(json.dumps(result,indent=2),flush=True)
if result["exit"]:print((root/(result["phases"][-1]["name"]+".log")).read_text(errors="replace")[-12000:],flush=True)
sys.exit(result["exit"])
