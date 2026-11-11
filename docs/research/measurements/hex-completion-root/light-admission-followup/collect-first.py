import base64,hashlib,io,json,shlex,subprocess,sys,tarfile
from pathlib import Path,PurePosixPath
base=Path('/Users/adalundhe/.cache/hex-lead/scheduler-completion-20261110')
collector=r'''import base64,hashlib,io,json,os,stat,tarfile
from pathlib import Path
names=['cohort-9ba81af8-light-v2'];data=io.BytesIO();manifest={}
with tarfile.open(fileobj=data,mode='w:gz') as tar:
 for name in names:
  root=Path.home()/'.cache/hex-lead'/name
  result=json.loads((root/'result.json').read_text())
  bins=[]
  for pattern in ['hex_api-*','hex_core-*','hex_client-*','hex_sim-*']:
   for p in (root/'target/debug/deps').glob(pattern):
    if p.is_file() and p.stat().st_mode & stat.S_IXUSR:
     bins.append(dict(path=str(p.relative_to(root)),bytes=p.stat().st_size,sha256=hashlib.sha256(p.read_bytes()).hexdigest()))
  extra=json.dumps(dict(binary=bins,phases=result['phases'],recorded_children_reaped=all(p.get('reaped') for p in result['phases'])),indent=2).encode()+b'\n'
  files=[p for p in root.iterdir() if p.is_file() and p.suffix in ('.log','.json')]
  for p in files:
   payload=p.read_bytes();arc=name+'/'+p.name;manifest[arc]=dict(bytes=len(payload),sha256=hashlib.sha256(payload).hexdigest());m=tarfile.TarInfo(arc);m.size=len(payload);m.mode=0o644;tar.addfile(m,io.BytesIO(payload))
  arc=name+'/collected-binaries.json';manifest[arc]=dict(bytes=len(extra),sha256=hashlib.sha256(extra).hexdigest());m=tarfile.TarInfo(arc);m.size=len(extra);m.mode=0o644;tar.addfile(m,io.BytesIO(extra))
raw=data.getvalue();print('HEX_COMPLETION_COLLECTION '+json.dumps(dict(bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest(),members=manifest,base64=base64.b64encode(raw).decode())),flush=True)
'''
command=shlex.join(['/Users/adalundhe/.cache/hex-lead/python-3.13.18-20261026/python/bin/python3','-c',collector])
args=[str(Path.home()/'.local/bin/kiln'),'run','--on','d1323898b255a3ed435eb83fa0f7b0cf','--needs','cpus=1,mem=4g,disk=12g','--no-sync','--quiet','--',command]
rawlog=base/'allocation-followup-collection-9ba.cli.log'
with rawlog.open('xb') as out:
 child=subprocess.Popen(args,stdout=out,stderr=subprocess.STDOUT)
 (base/'allocation-followup-collection-9ba-submission.json').write_text(json.dumps(dict(pid=child.pid,command=args,collector_sha256=hashlib.sha256(collector.encode()).hexdigest()),indent=2)+'\n')
 print(json.dumps(dict(pid=child.pid,collection_log=str(rawlog))),flush=True)
 code=child.wait()
if code:print(rawlog.read_text()[-3000:]);sys.exit(code)
lines=[line.split('HEX_COMPLETION_COLLECTION ',1)[1] for line in rawlog.read_text().splitlines() if 'HEX_COMPLETION_COLLECTION ' in line]
if len(lines)!=1:raise ValueError('missing/ambiguous collection marker')
result=json.loads(lines[0]);data=base64.b64decode(result.pop('base64'),validate=True)
assert len(data)==result['bytes'] and hashlib.sha256(data).hexdigest()==result['sha256']
proof=base/'allocation-followup-proof-9ba';proof.mkdir(exist_ok=False)
(proof/'raw.tar.gz').write_bytes(data)
with tarfile.open(fileobj=io.BytesIO(data),mode='r:gz') as tar:
 names=set()
 for m in tar:
  p=PurePosixPath(m.name);assert m.isfile() and not p.is_absolute() and '..' not in p.parts
  payload=tar.extractfile(m).read();assert result['members'][m.name]==dict(bytes=len(payload),sha256=hashlib.sha256(payload).hexdigest());names.add(m.name)
  target=proof/m.name;target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(payload)
assert names==set(result['members'])
for name in ['cohort-9ba81af8-light-v2']:
 before=json.loads((proof/name/'source-before.json').read_text());after=json.loads((proof/name/'source-after.json').read_text())
 with tarfile.open(base/name/'source.tar.gz','r:gz') as tar:
  archived={m.name:hashlib.sha256(tar.extractfile(m).read()).hexdigest() for m in tar if m.isfile()}
 assert before==archived,(name,'archive mismatch')
 delta={p for p in set(before)|set(after) if before.get(p)!=after.get(p)}
 assert delta<= {'vendor/shards/Cargo.lock'},(name,delta)
result.update(local_cli_exit=code,local_cli_reaped=child.returncode is not None,all_members_verified=True,archive_source_before_verified=True,post_source_only_vendor_lock=True)
(proof/'collection.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:v for k,v in result.items() if k!='members'},indent=2))
