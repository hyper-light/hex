import hashlib,json,shlex,subprocess,sys
from pathlib import Path
lead=Path.home()/'.cache/hex-lead';repo=lead/'worktrees/scheduler-live';rev=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()
root=lead/('borrowed-contract-'+rev[:8]+'-mac-v1');root.mkdir(exist_ok=False)
archive=root/'source.tar.gz'
with archive.open('xb') as out: subprocess.run(['git','archive','--format=tar.gz',rev],cwd=repo,stdout=out,check=True)
sha=hashlib.sha256(archive.read_bytes()).hexdigest()
seed=subprocess.check_output(['git','--git-dir='+str(lead/'shards-source.git'),'show','33df95b08ad8f95bddeaa1c48acf0c64386e0eac:Cargo.lock'])
(root/'upstream.lock').write_bytes(seed);seed_sha=hashlib.sha256(seed).hexdigest()
transport=root/'input.tar.gz'
import tarfile,io
with tarfile.open(transport,mode='w:gz') as bundle:
 for name,data in [('source.tar.gz',archive.read_bytes()),('upstream.lock',seed)]:
  member=tarfile.TarInfo(name);member.size=len(data);member.mtime=0;bundle.addfile(member,io.BytesIO(data))
transport_sha=hashlib.sha256(transport.read_bytes()).hexdigest();driver=lead/'fixtures/root-drivers-fff9d0b9/borrowed-contract-mac.py'
remote='/Users/adalundhe/.cache/hex-lead/'+root.name
command=shlex.join(['/Users/adalundhe/.cache/hex-lead/python-3.13.18-20261026/python/bin/python3','-c',driver.read_text(),remote,transport_sha,sha,seed_sha,rev])
args=[str(Path.home()/'.local/bin/kiln'),'run','--on','d1323898b255a3ed435eb83fa0f7b0cf','--needs','cpus=1,mem=4g,disk=12g','--no-sync','--quiet','--stdin','--',command]
with transport.open('rb') as inp,(root/'cli.log').open('xb') as out:
 child=subprocess.Popen(args,stdin=inp,stdout=out,stderr=subprocess.STDOUT)
 (root/'submission.json').write_text(json.dumps({'pid':child.pid,'argv':args,'source':rev,'remote':remote,'archive_sha256':sha,'driver_sha256':hashlib.sha256(driver.read_bytes()).hexdigest()},indent=2)+'\n')
 print(json.dumps({'source':rev,'pid':child.pid,'proof':str(root)}),flush=True)
 code=child.wait()
(root/'cli.exit').write_text(str(code)+'\n');print((root/'cli.log').read_text()[-14000:]);sys.exit(code)
