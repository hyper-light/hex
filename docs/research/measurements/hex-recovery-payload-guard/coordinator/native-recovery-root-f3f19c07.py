from pathlib import Path
import os,json,hashlib,subprocess,traceback
h=Path.home()/'.cache/hex-lead';proof=h/'recovery-f3f19c07-native';proof.mkdir(exist_ok=False)
built=json.loads((h/'capacity-8767f0d9-build/build.json').read_text());new=json.loads((h/'recovery-f3f19c07-root/result.json').read_text());assert built['exit']==new['exit']==0
driver=h/'native-recovery-payload-guard-f3.py';assert hashlib.sha256(driver.read_bytes()).hexdigest()=='dc38e77aea5b6d80ca75ecf14971a6fa321b0939281fa63af43ce71f97d345b1'
for name,digest in built['wire_inputs'].items():
 assert hashlib.sha256((h/'snapshots/recovery-f3f19c07'/name).read_bytes()).hexdigest()==digest
policy={'receipt':'POLICY_READ','revision':'f9030566bc227d50770bba879bd1387ebffe410f','rules_sha256':'d634fcb944959ce1e40a9277d12b0905fcfcebf82d9da9832e826f790a349242','role':'coordinator','task':'ROOT actual native OCI recovery payload consistency red/green','constraints':['all root owned exact processes and guest','no laptop test','no runner changes','Windows ARM paused','R01-R16']}
pp=proof/'policy.json';pp.write_text(json.dumps(policy,indent=2)+'\n')
status=1;cases=[];env=dict(os.environ)
for k in list(env):
 if k.startswith('DYLD_') or k.startswith('HEX_WAITS_CENSUS') or k.startswith('HEX_DEBUG_'):env.pop(k,None)
try:
 for label,source,host,expected in [('before','capacity-8767f0d9',built['artifacts']['hex_debug'],'started'),('after','recovery-f3f19c07',new['host'],'refused')]:
  assert hashlib.sha256(Path(host['path']).read_bytes()).hexdigest()==host['sha256']
  bp=proof/('build-'+label+'.json');bp.write_text(json.dumps({'host_source':source,'host':host,'retained_guest_source':'8767f0d9c03f37c64c9c86301bceaa878f7eb495','retained_guest_and_probe_artifacts':built['artifacts'],'all_eight_wire_inputs_byte_identical':True,'scope':'Only host immutable recovery guard changed. Guest and probe intentionally retained source87; Shards and kernel unchanged.'},indent=2)+'\n')
  argv=['nice','-n','10',str(h/'python-3.13.18-20261026/python/bin/python3'),'-I',str(driver),'--source',str(h/'snapshots'/source),'--inputs',str(h/'capacity-8767f0d9-physical-smoke/argv-debug-physical.json'),'--host',host['path'],'--expected',expected,'--policy',str(pp),'--build',str(bp)]
  log=proof/(label+'.log')
  with log.open('w') as out:
   child=subprocess.Popen(argv,env=env,stdout=out,stderr=out);(proof/'child.pid').write_text(str(child.pid));rc=child.wait()
  results=[x[7:] for x in log.read_text().splitlines() if x.startswith('result ')];assert results,'no result: check owned native guest before any next case'
  p=Path(results[-1]);data=json.loads(p.read_text())
  row={'label':label,'argv':argv,'exit':rc,'result':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'observed':data.get('contradiction'),'controls':data.get('controls'),'retirement':data.get('owned_guest_retired'),'status':data['status']}
  cases.append(row);(proof/'cases.json').write_text(json.dumps(cases,indent=2)+'\n');print(json.dumps(row),flush=True)
  assert rc==0 and data['exit']==0 and data['status']=='passed',data.get('error')
  assert not data.get('backend_cleanup_error') and not data.get('cleanup_error')
  assert data['owned_guest_retired']['end']=='Stopped'
 status=0
except BaseException:(proof/'driver-error.txt').write_text(traceback.format_exc());print(traceback.format_exc(),flush=True)
(proof/'driver.exit').write_text(str(status)+'\n')
raise SystemExit(status)
