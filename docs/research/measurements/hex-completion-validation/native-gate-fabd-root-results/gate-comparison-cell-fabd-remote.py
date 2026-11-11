import hashlib,importlib.util,json,os,re,sys,time
from pathlib import Path
name=sys.argv[1]
if name not in ('baseline','bypass','restored'):raise ValueError('one fixed comparison cell required')
root=Path.home()/'.cache/hex-lead/gc5688v1';plan=root/'sealed.json'
expected='06ba2ce2d7e577487e7e2df8ab8298aee2fe3f529e6422abc7ab24712a094fde'
tools=Path.home()/'.cache/hex-lead/gc5688-build-v1/source/docs/research/measurements/hex-completion-validation/native-gate-r05'
driver=tools/'root_native_gate_comparison.py';helper=tools/'root_native_gate_baseline.py'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
if sha(plan)!=expected or sha(driver)!='780e113dfed6063e6d1a67f23d8cdb4568e49f0ec0a9796c9303b1fe842ff6b2' or sha(helper)!='d1b42b03247b336c47a6724d6640c114c89c9ceca81d521db637220585759531':raise ValueError('exact reviewed plan/tools required')
spec=importlib.util.spec_from_file_location('root_fixed_a128_primitives',helper);base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base)
cell=root/name;lab=cell/'l';dispatch=cell/'root-dispatch-v1';dispatch.mkdir(mode=0o700)
evidence=base.Evidence(dispatch);end=time.monotonic()+1780
unit='hex-root-gate-'+name+'-fabd-v1.scope'
argv=['/usr/bin/systemd-run','--user','--scope','--collect','--quiet','--unit='+unit,'--property=Delegate=yes','--property=CPUQuota=100%','--property=MemoryMax=4G','--property=MemorySwapMax=0','--property=TasksMax=256','--property=RuntimeMaxSec=1800','/usr/bin/nice','-n','10','/usr/bin/python3','-B',str(driver),'cell','--plan',str(plan),'--plan-sha256',expected,'--cell',name,'--unit',unit,'--lab',str(lab)]
result={'source':'fabd73020cdd1c6e201d8028fcca5d85f28ef18c','cell':name,'plan_sha256':expected,'outer_owner':base.birth(os.getpid()),'unit':unit,'lab':str(lab),'outer_seconds':1780,'hard_scope_seconds':1800,'original_native_phase_seconds':180,'VM_GPU':False}
base.save(dispatch/'inputs.json',result)
errors=[]
try:
 _,observation=base.run_owned(argv,dict(os.environ),dispatch/'scope.stdout',dispatch/'scope.stderr',end,evidence)
 result['scope_launcher']=observation
 if not (observation['error'] is None and observation['both_eof'] and observation['reaped'] and observation.get('returncode')==0):errors.append('cell scope launcher incomplete/nonzero')
 outcome=json.loads((lab/'comparison-result.json').read_text());result['comparison']=outcome
 if outcome.get('expected_outcome_witnessed') is not True:errors.append('expected qualified comparison not witnessed')
 inputs=json.loads((lab/'inputs.json').read_text());owner=inputs['owner']
 try:
  observed=base.birth(owner['pid']);absent=observed['start_ticks']!=owner['start_ticks'];native={'observed':observed,'reused':absent}
 except FileNotFoundError:absent=True;native={'absent':True}
 scope=Path('/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/app.slice')/unit
 properties=dict(line.split('=',1) for line in (lab/'unit.stdout').read_text().splitlines())
 if properties.get('ControlGroup')!='/'+str(scope.relative_to('/sys/fs/cgroup')):raise ValueError('external owned scope differs from actual delegated query')
 result['external_closure']={'driver':owner,'owned_driver_birth_absent':absent,'native_observation':native,'scope':str(scope),'scope_absent':not scope.exists()}
 if not absent or scope.exists():errors.append('whole owned scope/driver absence unverified')
 _,query=base.run_owned(['/usr/bin/systemctl','--user','show',unit,'--property=ActiveState,SubState,ControlGroup'],dict(os.environ),dispatch/'terminal-unit.stdout',dispatch/'terminal-unit.stderr',end,evidence)
 result['terminal_unit_query']=query
 if query['error'] is not None or not query['reaped'] or not query['both_eof']:errors.append('external terminal query incomplete')
except Exception as error:errors.append(type(error).__name__+': '+str(error))
finally:
 errors.extend(evidence.failures);result['errors']=errors;result['exit']=0 if not errors else 1
 base.save(dispatch/'result.json',result);evidence.journal.close()
 print('HEX_GATE_CELL_RESULT '+json.dumps(result),flush=True)
sys.exit(result['exit'])
