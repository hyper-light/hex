from pathlib import Path
import json,hashlib,tarfile,io,base64,re
h=Path.home()/'.cache/hex-lead'
files={}
def take(path):
 path=Path(path); assert path.is_file() and not path.is_symlink(),path
 name=str(path.relative_to(h)) if h in path.parents else 'external-hx/'+str(path.relative_to(h.parent/'hx'))
 files[name]=path.read_bytes()
g=h/'capacity-16b0a5ae-seven-gates'
assert (g/'gates.exit').read_text().strip()=='0'
record=json.loads((g/'gates.json').read_text());raw=(g/'gates.log').read_bytes()
assert record['exit']==0 and hashlib.sha256(raw).hexdigest()==record['log_sha256']=='2e3ffc3c4e5f8dee33b2aca2d4a5948611f0b059f2f96864de40e9ee039c0e8b'
counts={'passed':0,'failed':0,'ignored':0};summaries=[]
for line in raw.decode().splitlines():
 if not line.startswith('test result: '):continue
 m=re.search(r'(\d+) passed; (\d+) failed; (\d+) ignored;',line);assert m,line
 for k,n in zip(counts,m.groups()):counts[k]+=int(n)
 summaries.append(line)
assert counts['failed']==0 and counts['passed']>0
for name in ['gates.log','gates.json','gates.exit']:take(g/name)
s=h/'capacity-06e6dbed-focused-v2';assert (s/'checks.exit').read_text().strip()=='0'
for name in ['checks.log','checks.json','checks.exit']:take(s/name)
smoke=h/'capacity-8767f0d9-physical-smoke'
for profile in ['debug','release']:
 stem='physical-'+profile+'-n1000'
 assert (smoke/(stem+'.exit')).read_text().strip()=='0'
 for name in [stem+'.json',stem+'.log',stem+'.exit','root-'+stem+'-verified.json']:take(smoke/name)
for name in ['summary.json','driver.exit','smoke.log']:take(smoke/name)
assert (smoke/'driver.exit').read_text().strip()=='0'
for row in json.loads((smoke/'summary.json').read_text()):
 p=Path(row['result']);assert hashlib.sha256(p.read_bytes()).hexdigest()==row['sha256'];take(p)
for name in ['build.json','build.log','build.exit']:take(h/'capacity-8767f0d9-build'/name)
readback={'kind':'ROOT terminal proof collection','gate_source':record['source_revision'],'gate_counts':counts,'gate_summaries':summaries,'limits':'Exact old16/06/87 proof only; not pending new recovery guard source or Linux native rootlab. Full physical ledger reconstructions retained, original remote raw ledgers not duplicated.'}
files['terminal-readback.json']=(json.dumps(readback,indent=2)+'\n').encode()
manifest={name:{'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()} for name,data in files.items()}
files['MEMBERS.json']=(json.dumps(manifest,indent=2)+'\n').encode()
b=io.BytesIO()
with tarfile.open(fileobj=b,mode='w:gz') as t:
 for name,data in files.items():
  info=tarfile.TarInfo(name);info.size=len(data);info.mode=0o600;t.addfile(info,io.BytesIO(data))
raw=b.getvalue();print(json.dumps({'sha256':hashlib.sha256(raw).hexdigest(),'bytes':len(raw),'count':len(files),'readback':readback,'archive':base64.b64encode(raw).decode()}))
