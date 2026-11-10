from pathlib import Path
import json,hashlib,importlib.util
h=Path.home()/'.cache/hex-lead';proof=h/'recovery-f3f19c07-native'
assert (proof/'driver.exit').read_text().strip()=='0'
spec=importlib.util.spec_from_file_location('owned_reader',h/'snapshots/cold-verifier-root-1bb73f74/verify_cold_census.py');c=importlib.util.module_from_spec(spec);spec.loader.exec_module(c);v=c.shared
maximum,_=v.protocol(h/'snapshots/recovery-f3f19c07')
checks=[]
for row in json.loads((proof/'cases.json').read_text()):
 root=Path(row['result']).parent;reader=c.Reader(root);r=reader.json(root/'result.json',maximum)
 assert hashlib.sha256((root/'result.json').read_bytes()).hexdigest()==row['sha256']
 assert r['kind']=='actual-native-OCI-recovery-payload-guard' and r['exit']==0 and r['status']=='passed'
 assert not any(r.get(k) for k in ['error','backend_cleanup_error','cleanup_error'])
 expected='started' if row['label']=='before' else 'refused';assert r['expected']==expected
 o=r['contradiction'];assert o['owner_start'] is False and o['runner_start'] is True and o['sibling_unchanged']
 assert o==reader.json(root/'observed-before-cleanup.json',maximum)
 assert r['controls']['actual_sibling_progress'] and r['controls']['matching_rows_recovered'] and r['controls']['empty_baseline']
 if expected=='started':assert o['boot_ready'] and o['physical_state']=='running'
 else:
  assert not o['boot_ready'] and o['process_exit']==1 and o['physical_state']=='created'
  assert o['records_before']==o['records_after'] and 'owned and runner rows disagree on payload' in o['diagnostic']
  assert r['controls']['repaired_physical_state']=='created'
 realm=json.loads((root/'target.json').read_text())['cluster'];key=o['key']
 assert o['physical']==f"hex-place-{hashlib.sha256(realm.encode()).hexdigest()}-{key['owner']}-{key['epoch']}-{key['seq']}"
 retirement=v.retirement(root,r,maximum);assert retirement['recorded_terminal']=='Stopped' and not r['owned_guest_retired'].get('already_gone')
 ledger=[]
 for n,line in c.bounded_lines(reader,root/'pids.jsonl',1048576,maximum):
  item=v.decode_json(line);assert item['run']==str(root) and item.get('census') is None;ledger.append(item)
 producers=[(n,x) for n,x in enumerate(ledger) if x['kind']=='real'];assert len(producers)==1
 n,backend=producers[0];assert any(x['kind']=='reaped' and x['pid']==backend['pid'] and x['parent']==backend['parent'] and type(x.get('returncode'))is int for x in ledger[n+1:])
 externals=[]
 for item in [*r['binary_and_artifact_hashes'].values(),*r['receipts'].values()]:
  p=Path(item['path']);digest=hashlib.sha256(p.read_bytes()).hexdigest();assert digest==item['sha256'];externals.append({'path':str(p),'sha256':digest})
 # Retain hashes for every private regular evidence/record file; no credentials/SSH keys exist in this local-only case.
 inputs={}
 for p in sorted(root.rglob('*')):
  if p.is_file() and not p.is_symlink() and ('rootfs' not in p.name) and 'guest-home' not in p.parts and 'guest-tmp' not in p.parts and 'docker-config' not in p.parts:
   inputs[str(p.relative_to(root))]={'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()}
 checks.append({'label':row['label'],'result_sha256':row['sha256'],'observed':o,'controls':r['controls'],'retirement':retirement,'ledger_rows':len(ledger),'owned_backend':backend,'external_inputs':externals,'private_evidence_hashes':inputs})
out={'kind':'ROOT independent actual OCI recovery readback','source_revision':'f3f19c07e658dcd8097bba3dbb2bcbc484d9d8d7','cases':checks,'scope':'Actual native OCI start/refusal and sibling progress inside owned ARM Linux guests, full ledger/record/config/artifact/retirement readback; no performance/security-exploit or native AMD64/rootlab claim'}
(proof/'root-readback.json').write_text(json.dumps(out,indent=2)+'\n')
print(json.dumps({'cases':[{'label':x['label'],'ledger_rows':x['ledger_rows'],'state':x['observed']['physical_state'],'terminal':x['retirement']['recorded_terminal']} for x in checks],'sha256':hashlib.sha256((proof/'root-readback.json').read_bytes()).hexdigest()}))
