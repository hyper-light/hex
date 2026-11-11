import hashlib,json,shlex,subprocess,sys
from pathlib import Path
base=Path('/Users/adalundhe/.cache/hex-lead/scheduler-completion-20261110')
cell=sys.argv[1]
if cell not in ('baseline','bypass','restored'):raise ValueError('fixed cell name')
# Every cell follows completed build and verified whole-build-scope closure.
proof=base/'gate-comparison-build-fabd-results-proof'
closure=json.loads((proof/'outer-closure.json').read_text())
if not closure['scope_absent'] or not closure['owned_driver_absent'] or closure['members']:raise ValueError('build custody incomplete')
if hashlib.sha256((proof/'comparison/sealed.json').read_bytes()).hexdigest()!='06ba2ce2d7e577487e7e2df8ab8298aee2fe3f529e6422abc7ab24712a094fde':raise ValueError('plan changed')
if cell!='baseline':
 prior='baseline' if cell=='bypass' else 'bypass'
 previous=base/('gate-comparison-'+prior+'-fabd-v1/result.json')
 if json.loads(previous.read_text())['exit']!=0:raise ValueError('previous cell not qualified/clean')
local=base/('gate-comparison-'+cell+'-fabd-v1');local.mkdir(exist_ok=False)
driver=base/'gate-comparison-cell-fabd-remote.py'
command=shlex.join(['/usr/bin/timeout','--signal=TERM','--kill-after=10s','1830s','/usr/bin/nice','-n','10','/usr/bin/python3','-B','-c',driver.read_text(),cell])
argv=[str(Path.home()/'.local/bin/kiln'),'run','--on','00aad8daff2fa3c9daa65d53c425d450','--needs','linux/amd64,cpus=1,mem=4g,disk=12g','--no-sync','--quiet','--',command]
with (local/'cli.log').open('xb') as out:
 child=subprocess.Popen(argv,stdout=out,stderr=subprocess.STDOUT)
 (local/'submission.json').write_text(json.dumps({'cell':cell,'source':'fabd73020cdd1c6e201d8028fcca5d85f28ef18c','pid':child.pid,'argv':argv,'remote_wrapper_sha256':hashlib.sha256(driver.read_bytes()).hexdigest()},indent=2)+'\n')
 print(json.dumps({'cell':cell,'pid':child.pid,'local':str(local)}),flush=True)
 try:ret=child.wait(timeout=1860)
 except subprocess.TimeoutExpired:
  child.terminate()
  try:child.wait(timeout=5)
  except subprocess.TimeoutExpired:child.kill();child.wait()
  (local/'local-timeout.json').write_text(json.dumps({'pid':child.pid,'reaped':child.returncode is not None})+'\n');raise
(local/'cli.exit').write_text(str(ret)+'\n')
rows=[line.split('HEX_GATE_CELL_RESULT ',1)[1] for line in (local/'cli.log').read_text().splitlines() if 'HEX_GATE_CELL_RESULT ' in line]
if len(rows)!=1:print((local/'cli.log').read_text(errors='replace')[-5000:]);raise ValueError('missing/ambiguous cell result')
result=json.loads(rows[0]);result['local_cli']={'pid':child.pid,'exit':ret,'reaped':child.returncode is not None}
(local/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2),flush=True)
sys.exit(0 if ret==0 and result['exit']==0 else 1)
