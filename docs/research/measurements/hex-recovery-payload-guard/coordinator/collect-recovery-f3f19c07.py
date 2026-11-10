from pathlib import Path
import json,hashlib,tarfile,io,base64,re
h=Path.home()/'.cache/hex-lead';files={}
def take(p):
 p=Path(p);assert p.is_file() and not p.is_symlink()
 name=str(p.relative_to(h)) if h in p.parents else 'external-hx/'+str(p.relative_to(h.parent/'hx'))
 files[name]=p.read_bytes()
for name in ['recovery-f3f19c07-root','recovery-f3f19c07-native']:
 p=h/name;assert (p/'driver.exit').read_text().strip()=='0'
 for q in sorted(p.iterdir()):
  if q.is_file():take(q)
for case in json.loads((h/'recovery-f3f19c07-native/cases.json').read_text()):
 root=Path(case['result']).parent
 for q in sorted(root.iterdir()):
  if q.is_file() and q.suffix in ['.json','.jsonl','.log']:take(q)
 for name in ['target.state/placements','sibling.state/placements']:
  for q in sorted((root/name).glob('*.rec')):take(q)
 take(root/'real.node-state/@guest')
canonical=h/'snapshots/recovery-f3f19c07';actual=h/'recovery-f3f19c07-root/source'
manifest={};generated={}
for q in sorted(canonical.rglob('*')):
 if q.is_file() and not q.is_symlink():
  rel=q.relative_to(canonical);a=hashlib.sha256(q.read_bytes()).hexdigest()
  if '__pycache__' in rel.parts:
   generated[str(rel)]=a;continue
  b=hashlib.sha256((actual/rel).read_bytes()).hexdigest();assert a==b,str(rel);manifest[str(rel)]=a
files['generated-import-cache-hashes.json']=(json.dumps(generated,indent=2)+chr(10)).encode()
files['source-hashes.json']=(json.dumps(manifest,indent=2)+'\n').encode()
log=(h/'recovery-f3f19c07-root/combined-node.log').read_text();summaries=re.findall(r'^test result:.*$',log,re.M)
assert len(summaries)==1 and '139 passed; 0 failed; 0 ignored;' in summaries[0],summaries
readback={'source_revision':'f3f19c07e658dcd8097bba3dbb2bcbc484d9d8d7','combined_node':summaries[0],'source_files_verified':len(manifest),'native_ledger_rows':133,'scope':'Final restored ROOT source/unit/mutation/native proof. Full7target gate and final Linux rootlab separate.'}
files['collection-readback.json']=(json.dumps(readback,indent=2)+'\n').encode()
files['MEMBERS.json']=(json.dumps({k:{'bytes':len(v),'sha256':hashlib.sha256(v).hexdigest()} for k,v in files.items()},indent=2)+'\n').encode()
b=io.BytesIO()
with tarfile.open(fileobj=b,mode='w:gz') as t:
 for name,data in files.items():
  m=tarfile.TarInfo(name);m.size=len(data);m.mode=0o600;t.addfile(m,io.BytesIO(data))
raw=b.getvalue();print(json.dumps({'archive':base64.b64encode(raw).decode(),'sha256':hashlib.sha256(raw).hexdigest(),'bytes':len(raw),'files':len(files),'readback':readback}))
