"""ROOT-only actual OCI recovery proof; existing owned Harness and binary inputs only."""
from pathlib import Path
from types import SimpleNamespace
import argparse, importlib.util, json, hashlib, signal, sys, time, traceback

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--source',required=True)
parser.add_argument('--inputs',required=True,help='Exact prior smoke argv JSON, retaining caller budgets/artifacts')
parser.add_argument('--host',required=True)
parser.add_argument('--expected',choices=['refused','started'],required=True)
parser.add_argument('--policy',required=True)
parser.add_argument('--build',required=True)
args=parser.parse_args()
original=json.loads(Path(args.inputs).read_text())
values={};i=original.index('--source')
while i<len(original):
 key=original[i];assert key.startswith('--')
 if key=='--validate-seeds': values['validate_seeds']=True;i+=1
 else: values[key[2:].replace('-','_')]=original[i+1];i+=2
for k in ['n','max_census_lines','cpu_milli','box_memory_mib','guest_cpus','guest_memory_mib']:values[k]=int(values[k])
values['deadline_seconds']=float(values['deadline_seconds'])
values.update(source=args.source,hex=args.host,policy_receipt=args.policy,build_receipt=args.build,sshd=None,cgroup_parent=None,guest_shares=None)
source=Path(args.source).resolve()
spec=importlib.util.spec_from_file_location('owned_census',source/'bench/sched/runtime-pools/census/scheduler_census.py')
c=importlib.util.module_from_spec(spec);spec.loader.exec_module(c)
h=c.Harness(SimpleNamespace(**values))
def interrupted(number,_frame):raise c.Failed('native recovery fixture interrupted '+str(number))
for number in [signal.SIGTERM,signal.SIGINT,signal.SIGHUP]:signal.signal(number,interrupted)
def durable(path):
 return {p.name:p.read_bytes() for p in sorted(path.glob('*.rec'))}
def hashes(rows):return {k:hashlib.sha256(v).hexdigest() for k,v in rows.items()}
def create_only(sock,label):
 cpu,memory=h.args.cpu_milli,h.args.box_memory_mib*(1<<20)
 payload={'spec':{'id':label,'bounds':{'cpu_max_milli':cpu,'memory_max':memory},'class':{'oci':{'rootfs':{'dir':str(h.rootfs)},'init':{'args':['/hex','selftest-box','wait'],'env':[],'cwd':None,'user':None},'mounts':[],'hostname':None,'network':'private','hosts':[]}},'owner':None},'start':False}
 reply=h.call(sock,{'Submit':{'demand':f'cpus={cpu}m,mem={memory}b,class:shards,linux/arm64','payload':payload}})
 key=reply['key'];ready=h.status(sock,key,lambda row:row.get('created'));assert ready['at']==0
 info=h.snapshot(inspect=ready['physical'])['inspected'];assert info['state'].lower()=='created'
 return key,ready['physical'],payload
status=1
try:
 h.report['kind']='actual-native-OCI-recovery-payload-guard'
 h.report['scope']='Host scheduler recovery against actual OCI boxes inside one owned ARM Linux guest; no performance, exploit or generic backend claim'
 h.report['source_revision']=(source/'SOURCE_REVISION').read_text().strip()
 h.report['expected']=args.expected
 h.runtime['source_revision']=h.report['source_revision'];h.runtime['hex_sha256']=h.report['binary_and_artifact_hashes']['hex']['sha256'];c.atomic_json(h.runtime_path,h.runtime)
 backend=h.backend('real')
 target,sock,config=h.scheduler('target',0,[{'class':'shards','socket':str(backend)}],[],h.root.name+'-target')
 sibling,other_sock,other_config=h.scheduler('sibling',0,[{'class':'shards','socket':str(backend)}],[],h.root.name+'-sibling')
 key,physical,payload=create_only(sock,'native-create-only')
 other_key,other_physical,_=create_only(other_sock,'native-sibling')
 rows=h.root/'target.state/placements';other_rows=h.root/'sibling.state/placements'
 before_other=durable(other_rows);before_other_info=h.snapshot(inspect=other_physical)['inspected']
 h.stop(target)
 runner=rows/f"ran-{key['owner']}-{key['epoch']}-{key['seq']}.rec"
 own=rows/f"own-{key['owner']}-{key['epoch']}-{key['seq']}.rec"
 original_runner=runner.read_bytes();changed=json.loads(original_runner)
 assert not changed['payload']['start'] and json.loads(own.read_bytes())['payload']==changed['payload']
 changed['payload']['start']=True;c.atomic_json(runner,changed)
 before=durable(rows)
 restarted=h.spawn('target-conflicted',[h.runtime['hex'],'schedule','serve','--config',str(config)],ready=True)
 h.scheduler_processes.append(restarted)
 ready=False
 try:h.ready(restarted,sock);ready=True
 except c.Failed:
  rc=restarted.wait(timeout=h.runtime['wait']);c.record(h.runtime,'reaped',restarted.pid,returncode=rc)
 observed=h.snapshot(inspect=physical)['inspected']
 after=durable(rows)
 diagnostic=(h.root/'target-conflicted.log').read_text()
 sibling_unchanged=durable(other_rows)==before_other and h.snapshot(inspect=other_physical)['inspected']==before_other_info
 h.report['contradiction']={'key':key,'physical':physical,'owner_start':False,'runner_start':True,'boot_ready':ready,'process_exit':restarted.poll(),'physical_state':observed['state'],'records_before':hashes(before),'records_after':hashes(after),'sibling_unchanged':sibling_unchanged,'diagnostic':diagnostic}
 c.atomic_json(h.root/'observed-before-cleanup.json',h.report['contradiction'])
 # Keep physical control independent of the corrupted scheduler state and prove sibling progress.
 progress=h.submit(other_sock,'native-sibling-progress')
 progressed=h.status(other_sock,progress,lambda row:row.get('created'))
 assert h.snapshot(inspect=progressed['physical'])['inspected']['state'].lower()=='running'
 h.call(other_sock,{'Delete':{'key':progress}});h.status(other_sock,progress,lambda row:row.get('ended') or not row.get('tracked',True))
 assert h.snapshot(inspect=other_physical)['inspected']==before_other_info
 h.stop(restarted);runner.write_bytes(original_runner)
 repaired=h.spawn('target-repaired',[h.runtime['hex'],'schedule','serve','--config',str(config)],ready=True)
 h.scheduler_processes.append(repaired);h.ready(repaired,sock)
 repaired_info=h.snapshot(inspect=physical)['inspected']
 h.call(sock,{'Delete':{'key':key}});h.status(sock,key,lambda row:row.get('ended') or not row.get('tracked',True))
 h.call(other_sock,{'Delete':{'key':other_key}});h.status(other_sock,other_key,lambda row:row.get('ended') or not row.get('tracked',True))
 h.report['controls']={'actual_sibling_progress':True,'matching_rows_recovered':True,'repaired_physical_state':repaired_info['state'],'empty_baseline':h.snapshot()['boxes']==h.baseline}
 # Guard assertions happen after the captured effect and explicit box cleanup.
 assert sibling_unchanged and h.report['controls']['empty_baseline']
 if args.expected=='started':assert ready and observed['state'].lower()=='running'
 else:
  assert not ready and restarted.returncode!=0
  assert 'owned and runner rows disagree on payload' in diagnostic
  assert observed['state'].lower()=='created' and repaired_info['state'].lower()=='created'
  assert before==after
 h.report['status']='passed';status=0
except BaseException:
 h.report['status']='failed';h.report['error']=traceback.format_exc()
finally:
 try:h.finish_backend()
 except BaseException:h.report['backend_cleanup_error']=traceback.format_exc();status=1
 try:h.cleanup()
 except BaseException:h.report['cleanup_error']=traceback.format_exc();status=1
 h.report['exit']=status;c.atomic_json(h.root/'result.json',h.report)
 print('result '+str(h.root/'result.json'),flush=True)
raise SystemExit(status)
