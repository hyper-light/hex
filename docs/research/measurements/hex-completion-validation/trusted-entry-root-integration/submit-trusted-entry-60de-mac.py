import gzip,hashlib,json,os,shlex,subprocess,sys
from pathlib import Path
base=Path('/Users/adalundhe/.cache/hex-lead/scheduler-completion-20261110');repo=Path('/Users/adalundhe/.cache/hex-lead/worktrees/scheduler-live');rev='60dedf289e232f993627c582ff9b8ba3bfcccaf6';target=sys.argv[1]
if target!='mac':raise ValueError('new trusted-entry pure source cohort is Mac arm64 only')
local=base/('cohort-'+rev[:8]+'-'+target+'-trusted-entry-60de-mac');local.mkdir(exist_ok=False);raw=subprocess.check_output(['git','archive',rev],cwd=repo);data=gzip.compress(raw,mtime=0);archive=local/'source.tar.gz';archive.write_bytes(data);sha=hashlib.sha256(data).hexdigest();remote='~/.cache/hex-lead/'+local.name;driver=base/'trusted-entry-60de-mac-remote.py'
python='/Users/adalundhe/.cache/hex-lead/python-3.13.18-20261026/python/bin/python3' if target=='mac' else '/usr/bin/python3';runner='d1323898b255a3ed435eb83fa0f7b0cf' if target=='mac' else '00aad8daff2fa3c9daa65d53c425d450';needs='cpus=1,mem=4g,disk=12g' if target=='mac' else 'linux/amd64,cpus=1,mem=4g,disk=12g';command=shlex.join(['nice','-n','10',python,'-B','-c',driver.read_text(),remote,sha,rev,target]);args=[str(Path.home()/'.local/bin/kiln'),'run','--on',runner,'--needs',needs,'--no-sync','--quiet','--timeout','3300','--stdin','--',command]
with archive.open('rb') as inp,(local/'cli.log').open('xb') as log:
 child=subprocess.Popen(args,stdin=inp,stdout=log,stderr=subprocess.STDOUT);(local/'submission.json').write_text(json.dumps(dict(source=rev,target=target,pid=child.pid,remote=remote,archive_sha256=sha,archive_bytes=len(data),driver_sha256=hashlib.sha256(driver.read_bytes()).hexdigest(),argv=args),indent=2)+'\n');print(json.dumps(dict(source=rev,target=target,pid=child.pid,root=str(local),archive_bytes=len(data))),flush=True)
 try:code=child.wait(timeout=3360)
 except subprocess.TimeoutExpired:
  child.terminate()
  try:child.wait(timeout=5)
  except subprocess.TimeoutExpired:child.kill();child.wait()
  code=124
(local/'cli.exit').write_text(str(code)+'\n');print((local/'cli.log').read_text(errors='replace')[-3000:],flush=True);sys.exit(code)
