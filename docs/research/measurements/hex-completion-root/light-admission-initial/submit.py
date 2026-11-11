import hashlib,io,json,shlex,subprocess,sys,tarfile
from pathlib import Path
base=Path("/Users/adalundhe/.cache/hex-lead/scheduler-completion-20261110");repo=Path("/Users/adalundhe/.cache/hex-lead/worktrees/scheduler-live")
rev="516a370bf7dccc7caea6683b0576f4c0880f9057";root=base/("cohort-"+rev[:8]+"-light-v1");root.mkdir(exist_ok=False)
seed=subprocess.check_output(["git","--git-dir=/Users/adalundhe/.cache/hex-lead/shards-source.git","show","33df95b08ad8f95bddeaa1c48acf0c64386e0eac:Cargo.lock"]);seedsha=hashlib.sha256(seed).hexdigest()
archive=root/"source.tar.gz"
with archive.open("xb") as out:
 with tarfile.open(fileobj=out,mode="w:gz") as dest:
  raw=subprocess.check_output(["git","archive",rev],cwd=repo)
  with tarfile.open(fileobj=io.BytesIO(raw),mode="r:") as src:
   for m in src:
    dest.addfile(m,src.extractfile(m) if m.isfile() else None)
  m=tarfile.TarInfo(".cohort-inputs/shards-Cargo.lock");m.size=len(seed);m.mode=0o644;dest.addfile(m,io.BytesIO(seed))
sha=hashlib.sha256(archive.read_bytes()).hexdigest();driver=base/"admission-integration-light-remote.py";remote="/Users/adalundhe/.cache/hex-lead/"+root.name
command=shlex.join(["/Users/adalundhe/.cache/hex-lead/python-3.13.18-20261026/python/bin/python3","-c",driver.read_text(),remote,sha,rev,seedsha])
args=[str(Path.home()/".local/bin/kiln"),"run","--on","d1323898b255a3ed435eb83fa0f7b0cf","--needs","cpus=1,mem=4g,disk=12g","--no-sync","--quiet","--stdin","--",command]
with archive.open("rb") as inp,(root/"cli.log").open("xb") as log:
 child=subprocess.Popen(args,stdin=inp,stdout=log,stderr=subprocess.STDOUT)
 (root/"submission.json").write_text(json.dumps(dict(source=rev,pid=child.pid,remote=remote,archive_sha256=sha,archive_bytes=archive.stat().st_size,seed_sha256=seedsha,driver_sha256=hashlib.sha256(driver.read_bytes()).hexdigest(),argv=args),indent=2)+"\n")
 print(json.dumps(dict(source=rev,pid=child.pid,root=str(root),archive_bytes=archive.stat().st_size)),flush=True)
 code=child.wait()
(root/"cli.exit").write_text(str(code)+"\n");print((root/"cli.log").read_text(errors="replace")[-18000:],flush=True);sys.exit(code)
