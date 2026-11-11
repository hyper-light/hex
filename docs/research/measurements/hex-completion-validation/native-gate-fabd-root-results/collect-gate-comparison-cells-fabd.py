import base64,hashlib,io,json,shlex,subprocess,sys,tarfile
from pathlib import Path,PurePosixPath
base=Path('/Users/adalundhe/.cache/hex-lead/scheduler-completion-20261110')
remote=r'''import base64,hashlib,io,json,os,stat,tarfile
from pathlib import Path
root=Path.home()/'.cache/hex-lead/gc5688v1';plan=root/'sealed.json'
expected='06ba2ce2d7e577487e7e2df8ab8298aee2fe3f529e6422abc7ab24712a094fde'
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  while b:=f.read(1048576):h.update(b)
 return h.hexdigest()
if sha(plan)!=expected:raise ValueError('sealed plan changed')
sealed=json.loads(plan.read_text());files={};links={};closures={};binaries={};sources={}
for name in ('baseline','bypass','restored'):
 cell=root/name
 r=json.loads((cell/'root-dispatch-v1/result.json').read_text())
 if r['exit']!=0 or r['comparison']['expected_outcome_witnessed'] is not True:raise ValueError('unqualified cell')
 scope=Path(r['external_closure']['scope'])
 if scope.exists():raise ValueError('owned scope persists')
 native=[]
 for role,owner in [('outer_owner',r['outer_owner']),('cell_driver',r['external_closure']['driver'])]:
  try:
   current=int((Path('/proc')/str(owner['pid'])/'stat').read_text().rsplit(')',1)[1].split()[19]);absent=current!=owner['start_ticks'];raw={'observed_birth':current}
  except FileNotFoundError:absent=True;raw={'absent':True}
  native.append({'role':role,'owner':owner,'owned_birth_absent':absent,**raw})
 if not all(x['owned_birth_absent'] for x in native):raise ValueError('owned native process persists')
 closures[name]={'scope':str(scope),'scope_absent':True,'native':native}
 row=sealed['cells'][name];binary=Path(row['build']['binary'])
 binaries[name]={'path':str(binary),'bytes':binary.stat().st_size,'sha256':sha(binary)}
 if any(binaries[name][k]!=row['build'][k] for k in ('bytes','sha256')):raise ValueError('ELF changed')
 actual={}
 for p in Path(row['source']).rglob('*'):
  if p.is_symlink():raise ValueError('source symlink')
  if p.is_file():actual[str(p.relative_to(row['source']))]=sha(p)
 if actual!=row['source_manifest']:raise ValueError('source changed')
 sources[name]={'files':len(actual),'whole_manifest_equal':True}
 for folder in ('l','root-dispatch-v1'):
  for p in (cell/folder).rglob('*'):
   arc=name+'/'+str(p.relative_to(cell))
   if p.is_symlink():links[arc]=os.readlink(p)
   elif p.is_file():files[arc]=p
manifest={};data=io.BytesIO()
summary={'closures':closures,'binaries':binaries,'sources':sources,'sealed_sha256':expected,'symlinks':links}
with tarfile.open(fileobj=data,mode='w:gz') as tar:
 for arc,p in files.items():
  if p.stat().st_size>16*1024*1024:raise ValueError('existing metadata bound')
  raw=p.read_bytes();manifest[arc]={'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()};m=tarfile.TarInfo(arc);m.size=len(raw);m.mode=0o644;tar.addfile(m,io.BytesIO(raw))
 raw=(json.dumps(summary,indent=2)+'\n').encode();arc='verified-post-state.json';manifest[arc]={'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()};m=tarfile.TarInfo(arc);m.size=len(raw);m.mode=0o644;tar.addfile(m,io.BytesIO(raw))
raw=data.getvalue();print('HEX_GATE_CELLS_COLLECTION '+json.dumps({'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'members':manifest,'summary':summary,'base64':base64.b64encode(raw).decode()}),flush=True)
'''
argv=[str(Path.home()/'.local/bin/kiln'),'run','--on','00aad8daff2fa3c9daa65d53c425d450','--needs','linux/amd64,cpus=1,mem=4g,disk=12g','--no-sync','--quiet','--',shlex.join(['/usr/bin/nice','-n','10','/usr/bin/python3','-B','-c',remote])]
log=base/'gate-comparison-cells-fabd-collection.cli.log'
with log.open('xb') as out:
 child=subprocess.Popen(argv,stdout=out,stderr=subprocess.STDOUT)
 (base/'gate-comparison-cells-fabd-collection-submission.json').write_text(json.dumps({'pid':child.pid,'argv':argv,'collector_sha256':hashlib.sha256(remote.encode()).hexdigest()},indent=2)+'\n')
 print(json.dumps({'pid':child.pid,'log':str(log)}),flush=True)
 try:ret=child.wait(timeout=180)
 except subprocess.TimeoutExpired:
  child.terminate()
  try:child.wait(timeout=5)
  except subprocess.TimeoutExpired:child.kill();child.wait()
  raise
if ret:print(log.read_text(errors='replace')[-5000:]);sys.exit(ret)
rows=[l.split('HEX_GATE_CELLS_COLLECTION ',1)[1] for l in log.read_text().splitlines() if 'HEX_GATE_CELLS_COLLECTION ' in l]
if len(rows)!=1:raise ValueError('one exact collection required')
result=json.loads(rows[0]);data=base64.b64decode(result.pop('base64'),validate=True)
if len(data)!=result['bytes'] or hashlib.sha256(data).hexdigest()!=result['sha256']:raise ValueError('archive changed')
proof=base/'gate-comparison-cells-fabd-results-proof';proof.mkdir(exist_ok=False);(proof/'raw.tar.gz').write_bytes(data);names=set()
with tarfile.open(fileobj=io.BytesIO(data),mode='r:gz') as tar:
 for m in tar:
  rel=PurePosixPath(m.name)
  if not m.isfile() or rel.is_absolute() or '..' in rel.parts or m.name in names:raise ValueError('unsafe member')
  raw=tar.extractfile(m).read()
  if result['members'][m.name]!={'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()}:raise ValueError(m.name)
  names.add(m.name);p=proof/m.name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(raw)
if names!=set(result['members']):raise ValueError('member set changed')
result.update(collector_exit=ret,collector_reaped=child.returncode is not None,all_members_verified=True)
(proof/'collection.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:v for k,v in result.items() if k!='members'},indent=2))
