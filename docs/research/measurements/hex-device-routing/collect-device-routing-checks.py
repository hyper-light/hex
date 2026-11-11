import base64,hashlib,io,json,os,shlex,subprocess,sys,tarfile
from pathlib import Path,PurePosixPath
lead=Path.home()/'.cache/hex-lead'
repo=lead/'worktrees/scheduler-live'
proof=lead/'device-routing-independent-proof-v1';proof.mkdir(exist_ok=False)
remote=r'''
import base64,hashlib,io,json,tarfile
from pathlib import Path
lead=Path.home()/'.cache/hex-lead'
roots=['device-current-40d5c644-compile-v1','device-routing-0aa59e26-red-v1','device-routing-8fd2572b-green-v1']
files={};readbacks={}
for name in roots:
 p=lead/name;r=json.loads((p/'result.json').read_text());assert r['exit']==(1 if '-red-' in name else 0) and r['temp_entries']==0
 manifest=json.loads((p/'source-hashes.json').read_text())
 actual={str(q.relative_to(p/'source')):hashlib.sha256(q.read_bytes()).hexdigest() for q in sorted((p/'source').rglob('*')) if q.is_file()}
 assert actual==manifest
 for phase in r['phases']:
  assert phase['exit']==(101 if '-red-' in name else 0) and phase['reaped'] and not phase.get('timed_out')
  raw=(p/(phase['name']+'.log')).read_bytes();assert hashlib.sha256(raw).hexdigest()==phase['sha256']
  if '-red-' in name:
   assert b'0 passed; 1 failed; 0 ignored;' in raw and b'actual route was bypassed: Some(Platform' in raw
  elif phase['name']=='device-routing': assert b'1 passed; 0 failed; 0 ignored;' in raw
  elif phase['name']=='device-driver-model': assert b'3 passed; 0 failed; 0 ignored;' in raw
  files[name+'/'+phase['name']+'.log']=raw
 for leaf in ['result.json','run.json','source-hashes.json']: files[name+'/'+leaf]=(p/leaf).read_bytes()
 readbacks[name]={'revision':r['source'],'source_files':len(manifest),'source_unchanged':True,'result':r}
files['READBACK.json']=(json.dumps(readbacks,indent=2)+'\n').encode()
files['MEMBERS.json']=(json.dumps({n:{'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()} for n,b in files.items()},indent=2)+'\n').encode()
b=io.BytesIO()
with tarfile.open(fileobj=b,mode='w:gz') as t:
 for n,raw in files.items():
  m=tarfile.TarInfo(n);m.size=len(raw);m.mtime=0;t.addfile(m,io.BytesIO(raw))
raw=b.getvalue();print(json.dumps({'sha256':hashlib.sha256(raw).hexdigest(),'bytes':len(raw),'archive':base64.b64encode(raw).decode()}))
'''
command=shlex.join(['/Users/adalundhe/.cache/hex-lead/python-3.13.18-20261026/python/bin/python3','-c',remote])
args=[str(Path.home()/'.local/bin/kiln'),'run','--on','d1323898b255a3ed435eb83fa0f7b0cf','--needs','cpus=1,mem=4g,disk=12g','--no-sync','--quiet','--',command]
with (proof/'cli.log').open('xb') as out:
 child=subprocess.Popen(args,stdout=out,stderr=subprocess.STDOUT)
 (proof/'submission.json').write_text(json.dumps({'pid':child.pid,'argv':args},indent=2)+'\n')
 code=child.wait()
(proof/'cli.exit').write_text(str(code)+'\n');assert code==0
obj=json.loads((proof/'cli.log').read_text());raw=base64.b64decode(obj['archive'],validate=True)
assert len(raw)==obj['bytes'] and hashlib.sha256(raw).hexdigest()==obj['sha256']
(proof/'proof.tar.gz').write_bytes(raw)
with tarfile.open(fileobj=io.BytesIO(raw),mode='r:gz') as archive:
 for m in archive.getmembers():
  path=PurePosixPath(m.name);assert m.isfile() and not path.is_absolute() and '..' not in path.parts
 archive.extractall(proof/'raw')
manifest=json.loads((proof/'raw/MEMBERS.json').read_text())
for n,m in manifest.items():
 b=(proof/'raw'/n).read_bytes();assert len(b)==m['bytes'] and hashlib.sha256(b).hexdigest()==m['sha256']
readback=json.loads((proof/'raw/READBACK.json').read_text())
for n,row in readback.items():
 contents=subprocess.check_output(['git','archive','--format=tar',row['revision']],cwd=repo)
 with tarfile.open(fileobj=io.BytesIO(contents),mode='r:') as source:
  signed={m.name:hashlib.sha256(source.extractfile(m).read()).hexdigest() for m in source.getmembers() if m.isfile()}
 actual=json.loads((proof/'raw'/n/'source-hashes.json').read_text());assert actual==signed,n
 row['signed_source_equal']=True
receipt={'exit':code,'reaped':child.returncode is not None,'archive_sha256':obj['sha256'],'bytes':obj['bytes'],'readback':readback}
(proof/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps({'proof':str(proof),'archive_sha256':obj['sha256'],'phases':{n:r['result']['phases'] for n,r in readback.items()}},indent=2))
