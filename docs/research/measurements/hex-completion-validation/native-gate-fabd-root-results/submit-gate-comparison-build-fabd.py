import gzip,hashlib,json,shlex,subprocess,sys
from pathlib import Path
base=Path('/Users/adalundhe/.cache/hex-lead/scheduler-completion-20261110')
repo=Path('/Users/adalundhe/.cache/hex-lead/worktrees/scheduler-live')
rev='fabd73020cdd1c6e201d8028fcca5d85f28ef18c'
local=base/'gate-comparison-build-fabd-v1';local.mkdir(exist_ok=False)
raw=subprocess.check_output(['git','archive',rev],cwd=repo);data=gzip.compress(raw,mtime=0)
archive=local/'source.tar.gz';archive.write_bytes(data);digest=hashlib.sha256(data).hexdigest()
driver=base/'gate-comparison-build-fabd-remote.py'
command=shlex.join(['systemd-run','--user','--scope','--collect','--quiet','--unit=hex-root-gate-comparison-build-fabd-v1','--property=Delegate=yes','--property=CPUQuota=100%','--property=MemoryMax=4G','--property=MemorySwapMax=0','--property=TasksMax=256','--property=RuntimeMaxSec=3300','timeout','--signal=TERM','--kill-after=10s','3280s','nice','-n','10','/usr/bin/python3','-B','-c',driver.read_text(),'~/.cache/hex-lead/gc5688-build-v1',digest,rev,'linux'])
argv=[str(Path.home()/'.local/bin/kiln'),'run','--on','00aad8daff2fa3c9daa65d53c425d450','--needs','linux/amd64,cpus=1,mem=4g,disk=12g','--no-sync','--quiet','--stdin','--',command]
with archive.open('rb') as inp,(local/'cli.log').open('xb') as log:
 child=subprocess.Popen(argv,stdin=inp,stdout=log,stderr=subprocess.STDOUT)
 (local/'submission.json').write_text(json.dumps({'source':rev,'pid':child.pid,'archive_bytes':len(data),'archive_sha256':digest,'driver_sha256':hashlib.sha256(driver.read_bytes()).hexdigest(),'argv':argv,'native_invocations':0},indent=2)+'\n')
 print(json.dumps({'source':rev,'pid':child.pid,'local':str(local),'archive_bytes':len(data),'archive_sha256':digest}),flush=True)
 try:code=child.wait(timeout=3360)
 except subprocess.TimeoutExpired:
  child.terminate()
  try:child.wait(timeout=5)
  except subprocess.TimeoutExpired:child.kill();child.wait()
  code=124
  (local/'local-timeout.json').write_text(json.dumps({'pid':child.pid,'timeout_seconds':3360,'reaped':child.returncode is not None})+'\n')
(local/'cli.exit').write_text(str(code)+'\n');print((local/'cli.log').read_text(errors='replace')[-8000:],flush=True);sys.exit(code)
