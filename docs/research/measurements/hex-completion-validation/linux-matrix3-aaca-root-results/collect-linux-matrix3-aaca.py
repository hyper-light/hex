import base64,hashlib,io,json,shlex,subprocess,sys,tarfile
from pathlib import Path,PurePosixPath
base=Path('/Users/adalundhe/.cache/hex-lead/scheduler-completion-20261110')
remote=r'''import base64,hashlib,io,json,tarfile
from pathlib import Path
names=['cohort-aaca22b9-linux-linux-matrix3-aaca','cohort-aaca22b9-linux-linux-matrix3-aaca-v2'];files={};closures={}
for name in names:
 root=Path.home()/'.cache/hex-lead'/name;r=json.loads((root/'result.json').read_text());owner=r['driver'];scope=Path(json.loads((root/'owned-scope-limits.json').read_text())['cgroup'])
 rows=[]
 for kind,identity in [('driver',owner),*[('cargo',p['observation']['identity']) for p in r['phases']]]:
  try:
   actual=int((Path('/proc')/str(identity['pid'])/'stat').read_text().rsplit(')',1)[1].split()[19]);absent=actual!=identity['start_ticks'];state={'actual_birth':actual}
  except FileNotFoundError:absent=True;state={'absent':True}
  rows.append({'kind':kind,'recorded':identity,'owned_birth_absent':absent,**state})
 closures[name]={'scope':str(scope),'scope_absent':not scope.exists(),'owners':rows}
 for p in [*root.iterdir(),* (root/'event-evidence').iterdir()]:
  if p.is_file() and p.name!='source.tar.gz':files[name+'/'+str(p.relative_to(root))]=p
manifest={};data=io.BytesIO()
with tarfile.open(fileobj=data,mode='w:gz') as tar:
 for arc,p in files.items():
  if p.is_symlink() or p.stat().st_size>16*1024*1024:raise ValueError('bounded regular metadata required')
  raw=p.read_bytes();manifest[arc]={'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()};m=tarfile.TarInfo(arc);m.size=len(raw);m.mode=0o644;tar.addfile(m,io.BytesIO(raw))
 raw=(json.dumps(closures,indent=2)+'\n').encode();arc='outer-closures.json';manifest[arc]={'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()};m=tarfile.TarInfo(arc);m.size=len(raw);m.mode=0o644;tar.addfile(m,io.BytesIO(raw))
raw=data.getvalue();print('HEX_MATRIX_COLLECTION '+json.dumps({'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'members':manifest,'closures':closures,'base64':base64.b64encode(raw).decode()}),flush=True)
'''
argv=[str(Path.home()/'.local/bin/kiln'),'run','--on','00aad8daff2fa3c9daa65d53c425d450','--needs','linux/amd64,cpus=1,mem=4g,disk=12g','--no-sync','--quiet','--timeout','180','--',shlex.join(['/usr/bin/nice','-n','10','/usr/bin/python3','-B','-c',remote])]
log=base/'linux-matrix3-aaca-collection.cli.log'
with log.open('xb') as out:
 child=subprocess.Popen(argv,stdout=out,stderr=subprocess.STDOUT)
 (base/'linux-matrix3-aaca-collection-submission.json').write_text(json.dumps({'pid':child.pid,'argv':argv,'collector_sha256':hashlib.sha256(remote.encode()).hexdigest()},indent=2)+'\n')
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
proof=base/'linux-matrix3-aaca-results-proof';proof.mkdir(exist_ok=False);(proof/'raw.tar.gz').write_bytes(data);names=set()
with tarfile.open(fileobj=io.BytesIO(data),mode='r:gz') as tar:
 for m in tar:
  rel=PurePosixPath(m.name)
  if not m.isfile() or rel.is_absolute() or '..' in rel.parts or m.name in names:raise ValueError('unsafe member')
  raw=tar.extractfile(m).read()
  if r['members'][m.name]!={'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()}:raise ValueError(m.name)
  names.add(m.name);p=proof/m.name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(raw)
if names!=set(r['members']):raise ValueError('member set changed')
for name in ['cohort-aaca22b9-linux-linux-matrix3-aaca','cohort-aaca22b9-linux-linux-matrix3-aaca-v2']:
 with tarfile.open(base/name/'source.tar.gz','r:gz') as tar:expected={m.name:hashlib.sha256(tar.extractfile(m).read()).hexdigest() for m in tar if m.isfile()}
 for ending in ['before','after']:
  if json.loads((proof/name/('source-'+ending+'.json')).read_text())!=expected:raise ValueError('source archive differs')
r.update(collector_exit=ret,collector_reaped=child.returncode is not None,all_members_verified=True,source_archives_verified=True)
(proof/'collection.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({k:v for k,v in r.items() if k!='members'},indent=2))
