import base64,hashlib,io,json,shlex,subprocess,sys,tarfile
from pathlib import Path,PurePosixPath
base=Path('/Users/adalundhe/.cache/hex-lead/scheduler-completion-20261110')
remote=r'''import base64,hashlib,io,json,tarfile,subprocess,os
from pathlib import Path
names=['cohort-60dedf28-mac-trusted-entry-60de-mac'];files={};closures={}
for name in names:
 root=Path.home()/'.cache/hex-lead'/name;r=json.loads((root/'result.json').read_text());rows=[]
 for kind,pid in [('driver',r['driver_pid']),*[('phase:'+p['name'],p['pid']) for p in r['phases']]]:
  q=subprocess.run(['/bin/ps','-p',str(pid),'-o','pid=,pgid=,lstart='],stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=5,text=True)
  rows.append({'kind':kind,'pid':pid,'pid_absent':q.returncode==1 and not q.stdout.strip(),'query_exit':q.returncode,'observed':q.stdout.strip(),'error':q.stderr.strip()})
 groups=[]
 for p in r['phases']:
  try:os.killpg(p['pgid'],0);absent=False
  except ProcessLookupError:absent=True
  groups.append({'phase':p['name'],'pgid':p['pgid'],'group_absent':absent})
 closures[name]={'owners':rows,'groups':groups,'scope_note':'Mac admitted build has no Linux hard Scope; exact owned Popen groups queried, no whole nativeScope proof'}
 for p in [*root.iterdir()]:
  if p.is_file() and p.name!='source.tar.gz':files[name+'/'+str(p.relative_to(root))]=p
manifest={};data=io.BytesIO()
with tarfile.open(fileobj=data,mode='w:gz') as tar:
 for arc,p in files.items():
  if p.is_symlink() or p.stat().st_size>16*1024*1024:raise ValueError('bounded regular metadata required')
  raw=p.read_bytes();manifest[arc]={'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()};m=tarfile.TarInfo(arc);m.size=len(raw);m.mode=0o644;tar.addfile(m,io.BytesIO(raw))
 raw=(json.dumps(closures,indent=2)+'\n').encode();arc='outer-closures.json';manifest[arc]={'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()};m=tarfile.TarInfo(arc);m.size=len(raw);m.mode=0o644;tar.addfile(m,io.BytesIO(raw))
raw=data.getvalue();print('HEX_MATRIX_COLLECTION '+json.dumps({'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'members':manifest,'closures':closures,'base64':base64.b64encode(raw).decode()}),flush=True)
'''
argv=[str(Path.home()/'.local/bin/kiln'),'run','--on','d1323898b255a3ed435eb83fa0f7b0cf','--needs','cpus=1,mem=4g,disk=12g','--no-sync','--quiet','--timeout','180','--',shlex.join(['/usr/bin/nice','-n','10','/Users/adalundhe/.cache/hex-lead/python-3.13.18-20261026/python/bin/python3','-B','-c',remote])]
log=base/'trusted-entry-60de-mac-collection.cli.log'
with log.open('xb') as out:
 child=subprocess.Popen(argv,stdout=out,stderr=subprocess.STDOUT)
 (base/'trusted-entry-60de-mac-collection-submission.json').write_text(json.dumps({'pid':child.pid,'argv':argv,'collector_sha256':hashlib.sha256(remote.encode()).hexdigest()},indent=2)+'\n')
 print(json.dumps({'pid':child.pid,'log':str(log)}),flush=True)
 try:ret=child.wait(timeout=190)
 except subprocess.TimeoutExpired:
  child.terminate()
  try:child.wait(timeout=5)
  except subprocess.TimeoutExpired:child.kill();child.wait()
  raise
if ret:print(log.read_text(errors='replace')[-5000:]);sys.exit(ret)
rows=[l.split('HEX_MATRIX_COLLECTION ',1)[1] for l in log.read_text().splitlines() if 'HEX_MATRIX_COLLECTION ' in l]
if len(rows)!=1:raise ValueError('one collection required')
r=json.loads(rows[0]);data=base64.b64decode(r.pop('base64'),validate=True)
if len(data)!=r['bytes'] or hashlib.sha256(data).hexdigest()!=r['sha256']:raise ValueError('archive changed')
proof=base/'trusted-entry-60de-mac-results-proof';proof.mkdir(exist_ok=False);(proof/'raw.tar.gz').write_bytes(data);names=set()
with tarfile.open(fileobj=io.BytesIO(data),mode='r:gz') as tar:
 for m in tar:
  rel=PurePosixPath(m.name)
  if not m.isfile() or rel.is_absolute() or '..' in rel.parts or m.name in names:raise ValueError('unsafe member')
  raw=tar.extractfile(m).read()
  if r['members'][m.name]!={'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()}:raise ValueError(m.name)
  names.add(m.name);p=proof/m.name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(raw)
if names!=set(r['members']):raise ValueError('member set changed')
for name in ['cohort-60dedf28-mac-trusted-entry-60de-mac']:
 with tarfile.open(base/name/'source.tar.gz','r:gz') as tar:expected={m.name:hashlib.sha256(tar.extractfile(m).read()).hexdigest() for m in tar if m.isfile()}
 for ending in ['before','after']:
  if json.loads((proof/name/('source-'+ending+'.json')).read_text())!=expected:raise ValueError('source archive differs')
r.update(collector_exit=ret,collector_reaped=child.returncode is not None,all_members_verified=True,source_archives_verified=True)
(proof/'collection.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({k:v for k,v in r.items() if k!='members'},indent=2))
