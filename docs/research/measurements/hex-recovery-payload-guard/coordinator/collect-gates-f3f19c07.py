from pathlib import Path
import json,hashlib,tarfile,io,base64,re
h=Path.home()/'.cache/hex-lead';p=h/'recovery-f3f19c07-seven-gates';assert (p/'gates.exit').read_text().strip()=='0'
r=json.loads((p/'gates.json').read_text());raw=(p/'gates.log').read_bytes();assert r['exit']==0 and r['source_revision']=='f3f19c07e658dcd8097bba3dbb2bcbc484d9d8d7'
assert hashlib.sha256(raw).hexdigest()==r['log_sha256']=='5ff129a0561bfefae7879a2cfdffe449d98797d63427ec69b35fa07a8a252652'
text=raw.decode();counts={'passed':0,'failed':0,'ignored':0};summaries=[]
for line in text.splitlines():
 if not line.startswith('test result: '):continue
 m=re.search(r'(\d+) passed; (\d+) failed; (\d+) ignored;',line);assert m,line
 for k,n in zip(counts,m.groups()):counts[k]+=int(n)
 summaries.append(line)
assert counts['failed']==0 and counts['passed']>0
names=['p2p_keeps_every_box_once_through_crashes_and_dropped_links','a_laptop_keeps_every_box_once_through_crashes','replacing_on_silence_is_caught','ten_thousand_crash_seeds_lose_and_double_nothing','the_hard_filters_keep_every_placement_in_bounds','boxes_land_only_where_their_platform_class_and_devices_are','the_recorded_workload_strands_at_least_30_percent_less_than_best_fit']
named={n:any(line.startswith('test ') and line.endswith(' ... ok') and line.split(' ... ')[0].split('::')[-1].removeprefix('test ')==n for line in text.splitlines()) for n in names};assert all(named.values()),named
readback={'source_revision':r['source_revision'],'tree':r['source_tree'],'exit':0,'counts':counts,'summaries':summaries,'named_assertion_tests':named,'scope':'Full seven-target lint/format/production/native Mac workspace/vendor at exactf3. Not actual AMD64 Linux/rootlab. Passing class/filter model assertions do not prove the live device attachment bridge.'}
files={n:(p/n).read_bytes() for n in ['gates.log','gates.json','gates.exit']};files['readback.json']=(json.dumps(readback,indent=2)+chr(10)).encode();files['MEMBERS.json']=(json.dumps({k:{'bytes':len(v),'sha256':hashlib.sha256(v).hexdigest()} for k,v in files.items()},indent=2)+chr(10)).encode()
b=io.BytesIO()
with tarfile.open(fileobj=b,mode='w:gz') as t:
 for name,data in files.items():
  q=tarfile.TarInfo(name);q.size=len(data);q.mode=0o600;t.addfile(q,io.BytesIO(data))
raw=b.getvalue();print(json.dumps({'archive':base64.b64encode(raw).decode(),'sha256':hashlib.sha256(raw).hexdigest(),'bytes':len(raw),'readback':readback}))
