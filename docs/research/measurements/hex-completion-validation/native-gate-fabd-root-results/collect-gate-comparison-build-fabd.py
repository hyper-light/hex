import base64,hashlib,io,json,shlex,subprocess,sys,tarfile
from pathlib import Path,PurePosixPath
base=Path('/Users/adalundhe/.cache/hex-lead/scheduler-completion-20261110')
remote=r'''import base64,hashlib,io,json,os,stat,tarfile
from pathlib import Path
root=Path.home()/'.cache/hex-lead/gc5688-build-v1';comparison=Path.home()/'.cache/hex-lead/gc5688v1'
result=json.loads((root/'result.json').read_text());scope=Path(json.loads((root/'owned-scope-limits.json').read_text())['cgroup'])
expected='/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/app.slice/hex-root-gate-comparison-build-fabd-v1.scope'
if str(scope)!=expected:raise ValueError('unexpected owned scope')
try:
 current=Path('/proc')/str(result['driver_pid'])/'stat'
 value=current.read_text();observed=int(value.rsplit(')',1)[1].split()[19])
 owned_driver_absent=observed!=result['driver_birth'];birth={'observed':observed,'reused':owned_driver_absent}
except FileNotFoundError:owned_driver_absent=True;birth={'absent':True}
members=[]
if scope.exists():
 for p in [scope,*scope.rglob('*')]:
  if p.is_dir():
   f=p/'cgroup.procs'
   if f.exists():
    ids=f.read_text().split()
    if len(ids)>256:raise ValueError('owned task envelope exceeded')
    members.extend({'group':str(p),'pid':int(v)} for v in ids)
closure={'driver_pid':result['driver_pid'],'driver_birth':result['driver_birth'],'owned_driver_absent':owned_driver_absent,'native_observation':birth,'scope':str(scope),'scope_absent':not scope.exists(),'members':members}
files={}
for p in root.iterdir():
 if p.is_file() and p.name!='source.tar.gz':files['build/'+p.name]=p
for p in comparison.iterdir() if comparison.exists() else []:
 if p.is_file():files['comparison/'+p.name]=p
for name in ('baseline','bypass','restored'):
 for p in (comparison/name).iterdir() if (comparison/name).exists() else []:
  if p.is_file():files['comparison/'+name+'/'+p.name]=p
manifest={};data=io.BytesIO()
with tarfile.open(fileobj=data,mode='w:gz') as tar:
 for arc,p in files.items():
  if p.is_symlink() or p.stat().st_size>16*1024*1024:raise ValueError('nonregular or oversized existing metadata')
  raw=p.read_bytes();manifest[arc]={'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()};m=tarfile.TarInfo(arc);m.size=len(raw);m.mode=0o644;tar.addfile(m,io.BytesIO(raw))
 raw=(json.dumps(closure,indent=2)+'\n').encode();arc='outer-closure.json';manifest[arc]={'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()};m=tarfile.TarInfo(arc);m.size=len(raw);m.mode=0o644;tar.addfile(m,io.BytesIO(raw))
raw=data.getvalue();print('HEX_GATE_BUILD_COLLECTION '+json.dumps({'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'members':manifest,'closure':closure,'base64':base64.b64encode(raw).decode()}),flush=True)
'''
argv=[str(Path.home()/'.local/bin/kiln'),'run','--on','00aad8daff2fa3c9daa65d53c425d450','--needs','linux/amd64,cpus=1,mem=4g,disk=12g','--no-sync','--quiet','--',shlex.join(['/usr/bin/nice','-n','10','/usr/bin/python3','-B','-c',remote])]
log=base/'gate-comparison-build-fabd-collection.cli.log'
with log.open('xb') as out:
 child=subprocess.Popen(argv,stdout=out,stderr=subprocess.STDOUT)
 (base/'gate-comparison-build-fabd-collection-submission.json').write_text(json.dumps({'pid':child.pid,'argv':argv,'collector_sha256':hashlib.sha256(remote.encode()).hexdigest()},indent=2)+'\n')
 print(json.dumps({'pid':child.pid,'collection_log':str(log)}),flush=True)
 try:ret=child.wait(timeout=180)
 except subprocess.TimeoutExpired:
  child.terminate()
  try:child.wait(timeout=5)
  except subprocess.TimeoutExpired:child.kill();child.wait()
  raise
if ret:print(log.read_text(errors='replace')[-5000:]);sys.exit(ret)
rows=[l.split('HEX_GATE_BUILD_COLLECTION ',1)[1] for l in log.read_text().splitlines() if 'HEX_GATE_BUILD_COLLECTION ' in l]
if len(rows)!=1:raise ValueError('one exact collection required')
record=json.loads(rows[0]);data=base64.b64decode(record.pop('base64'),validate=True)
if len(data)!=record['bytes'] or hashlib.sha256(data).hexdigest()!=record['sha256']:raise ValueError('collection changed')
proof=base/'gate-comparison-build-fabd-results-proof';proof.mkdir(exist_ok=False);(proof/'raw.tar.gz').write_bytes(data)
names=set()
with tarfile.open(fileobj=io.BytesIO(data),mode='r:gz') as tar:
 for m in tar:
  rel=PurePosixPath(m.name)
  if not m.isfile() or rel.is_absolute() or '..' in rel.parts or m.name in names:raise ValueError('unsafe collection member')
  raw=tar.extractfile(m).read()
  if record['members'][m.name]!={'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()}:raise ValueError(m.name)
  names.add(m.name);p=proof/m.name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(raw)
if names!=set(record['members']):raise ValueError('member set differs')
with tarfile.open(base/'gate-comparison-build-fabd-v1/source.tar.gz','r:gz') as tar:
 expected={m.name:hashlib.sha256(tar.extractfile(m).read()).hexdigest() for m in tar if m.isfile()}
if json.loads((proof/'build/source-before.json').read_text())!=expected or json.loads((proof/'build/source-after.json').read_text())!=expected:raise ValueError('source archive before/after mismatch')
record.update(collector_exit=ret,collector_reaped=child.returncode is not None,all_members_verified=True,source_archive_verified=True)
(proof/'collection.json').write_text(json.dumps(record,indent=2)+'\n')
print(json.dumps({k:v for k,v in record.items() if k!='members'},indent=2))
