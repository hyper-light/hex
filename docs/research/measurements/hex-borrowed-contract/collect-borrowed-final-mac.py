import base64,hashlib,io,json,shlex,subprocess,tarfile
from pathlib import Path,PurePosixPath
lead=Path.home()/'.cache/hex-lead';repo=lead/'worktrees/scheduler-live';dest=lead/'borrowed-final-mac-independent-v1';dest.mkdir(exist_ok=False)
remote=r'''
import base64,hashlib,io,json,tarfile
from pathlib import Path
lead=Path.home()/'.cache/hex-lead';files={};reports={}
base=lead/'borrowed-contract-d9d547c7-mac-v1';manifest=json.loads((base/'source-hashes.json').read_text())
for name in ['device-native-fixtures-winboth-d9d547c7-checks-v1','borrowed-observed-mutation-d9-v2','vendor-check-d9-root-v1','vendor-check-d9-root-v2']:
 root=lead/name;r=json.loads((root/'result.json').read_text());expected=1 if name=='vendor-check-d9-root-v1' else 0;assert r['exit']==expected
 leaves=['result.json']
 if name.startswith('device-native-'):
  own=json.loads((root/'source-hashes.json').read_text());assert own==manifest;source=root/'source'
  assert {str(p.relative_to(source)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(source.rglob('*')) if p.is_file()}==manifest
  leaves+=['source-hashes.json','run.json']
  for phase in r['phases']:
   assert phase['exit']==0 and phase['reaped'] and not phase.get('timed_out');raw=(root/(phase['name']+'.log')).read_bytes();assert hashlib.sha256(raw).hexdigest()==phase['sha256'];leaves.append(phase['name']+'.log')
  assert r['temp_entries']==0
 elif name.startswith('borrowed-observed-'):
  assert r['forbidden_stream_return_observed'] and r['original_source_unchanged'] and r['temp_entries']==0
  source=root/'source';actual={str(p.relative_to(source)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(source.rglob('*')) if p.is_file() and str(p.relative_to(source))!='vendor/shards/Cargo.lock'}
  assert {k for k in actual if actual[k]!=manifest.get(k)}=={'vendor/shards/crates/api/src/client.rs'}
  leaf=source/'vendor/shards/crates/api/src/client.rs';assert hashlib.sha256(leaf.read_bytes()).hexdigest()==r['after_sha256'] and manifest['vendor/shards/crates/api/src/client.rs']==r['before_sha256']
  leaves+=['run.json','mutation-client.rs']
  for phase in r['phases']:
   assert phase['exit']==(101 if phase['name']=='missing-final-guard-observed' else 0) and phase['reaped'] and not phase.get('timed_out');raw=(root/(phase['name']+'.log')).read_bytes();assert hashlib.sha256(raw).hexdigest()==phase['sha256'];leaves.append(phase['name']+'.log')
   if phase['name']=='missing-final-guard-observed':assert b'ROOT_forbidden_effect borrowed_stream_returned_after_target_death=true' in raw and b'0 passed; 1 failed; 0 ignored;' in raw
  assert all(hashlib.sha256((base/'source'/k).read_bytes()).hexdigest()==v for k,v in manifest.items())
 else:
  assert r['reaped'] and r['source_unchanged'];raw=(root/'check.log').read_bytes();assert hashlib.sha256(raw).hexdigest()==r['sha256'];leaves+=['check.log','run.json']
  if name.endswith('v2'):
   assert b'7 recorded changes, 0 differences' in raw
   source=root/'source';assert {str(p.relative_to(source)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(source.rglob('*')) if p.is_file()}==manifest
  else:assert b'only here: Cargo.lock' in raw
 for leaf in leaves:files[name+'/'+leaf]=(root/leaf).read_bytes()
 reports[name]=r
files['source-hashes.json']=(base/'source-hashes.json').read_bytes();files['READBACK.json']=(json.dumps(reports,indent=2)+'\n').encode()
files['MEMBERS.json']=(json.dumps({n:{'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()} for n,b in files.items()},indent=2)+'\n').encode()
buffer=io.BytesIO()
with tarfile.open(fileobj=buffer,mode='w:gz') as t:
 for n,b in files.items():m=tarfile.TarInfo(n);m.size=len(b);m.mtime=0;t.addfile(m,io.BytesIO(b))
b=buffer.getvalue();print(json.dumps({'sha256':hashlib.sha256(b).hexdigest(),'bytes':len(b),'archive':base64.b64encode(b).decode()}))
'''
argv=[str(Path.home()/'.local/bin/kiln'),'run','--on','d1323898b255a3ed435eb83fa0f7b0cf','--needs','cpus=1,mem=1g','--no-sync','--quiet','--',shlex.join([str(lead/'python-3.13.18-20261026/python/bin/python3'),'-c',remote])]
with(dest/'cli.log').open('xb') as out:
 c=subprocess.Popen(argv,stdout=out,stderr=subprocess.STDOUT);(dest/'submission.json').write_text(json.dumps({'pid':c.pid,'argv':argv},indent=2));code=c.wait()
(dest/'exit').write_text(str(code));assert code==0
obj=json.loads((dest/'cli.log').read_text());b=base64.b64decode(obj['archive'],validate=True);assert len(b)==obj['bytes'] and hashlib.sha256(b).hexdigest()==obj['sha256'];(dest/'proof.tar.gz').write_bytes(b)
with tarfile.open(fileobj=io.BytesIO(b),mode='r:gz') as t:
 for m in t.getmembers():p=PurePosixPath(m.name);assert m.isfile() and not p.is_absolute() and '..' not in p.parts
 t.extractall(dest/'raw')
for n,m in json.loads((dest/'raw/MEMBERS.json').read_text()).items():
 b=(dest/'raw'/n).read_bytes();assert len(b)==m['bytes'] and hashlib.sha256(b).hexdigest()==m['sha256']
rev='d9d547c7d9211f6c42f1c2c884017a3981032889';data=subprocess.check_output(['git','archive','--format=tar',rev],cwd=repo)
with tarfile.open(fileobj=io.BytesIO(data),mode='r:') as t:signed={m.name:hashlib.sha256(t.extractfile(m).read()).hexdigest() for m in t.getmembers() if m.isfile()}
assert signed==json.loads((dest/'raw/source-hashes.json').read_text())
r={'source':rev,'proof_sha256':obj['sha256'],'proof_bytes':obj['bytes'],'signed_original_sources_equal':True,'source_files':len(signed),'collector_exit':code,'collector_reaped':c.returncode is not None,'readback':json.loads((dest/'raw/READBACK.json').read_text())}
(dest/'receipt.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({k:v for k,v in r.items() if k!='readback'},indent=2))
