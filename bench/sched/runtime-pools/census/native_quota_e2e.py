import argparse,importlib.util,json,sys,tarfile,time
from pathlib import Path
p=argparse.ArgumentParser(add_help=False,allow_abbrev=False);p.add_argument('--cold-driver',required=True);p.add_argument('--unit-artifacts',required=True);a,rest=p.parse_known_args()
spec=importlib.util.spec_from_file_location('root_quota_native',a.cold_driver);cold=importlib.util.module_from_spec(spec);spec.loader.exec_module(cold)
manifest=json.loads(Path(a.unit_artifacts).read_text());artifacts={x['label']:x for x in manifest['artifacts']};test=manifest['filtered_test']
class NativeQuota(cold.ColdHarness):
 def exercise(self):
  super().exercise();self.report['kind']='native-arm-linux-quota-unit-execution';self.report['unit_cases']=[]
  local,socket,_=self.scheduler('quota-owner',0,[{'class':'shards','socket':str(self.backend_socket)}],[],self.root.name+'-quota')
  for label,artifact,expected in [('fixed',artifacts['fixed'],0),('truncation-mutant',artifacts['whole-cpu-truncation-mutant'],101),('fixed-restored',artifacts['fixed'],0)]:
   binary=Path(artifact['path']);raw=binary.read_bytes()
   import hashlib
   cold.census.require(hashlib.sha256(raw).hexdigest()==artifact['sha256'] and raw[:4]==b'\x7fELF' and raw[4:6]==bytes([2,1]) and int.from_bytes(raw[18:20],'little')==183,'unit artifact identity/ELF AArch64 mismatch')
   archive=self.root/(label+'-unit-rootfs.tar')
   with tarfile.open(archive,'w') as output:
    member=tarfile.TarInfo('hex');member.size=len(raw);member.mode=0o755
    with binary.open('rb') as data:output.addfile(member,data)
   cpu=self.args.cpu_milli;memory=self.args.box_memory_mib*(1<<20)
   payload={'id':'quota-'+label,'bounds':{'cpu_max_milli':cpu,'memory_max':memory},'class':{'oci':{'rootfs':{'dir':str(archive)},'init':{'args':['/hex','--exact',test,'--test-threads','1'],'env':[],'cwd':None,'user':None},'mounts':[],'hostname':None,'network':'private','hosts':[]}},'owner':None}
   deadline=time.monotonic()+self.runtime['wait'];reply=self.call(socket,{'Submit':{'demand':f'cpus={cpu}m,mem={memory}b,class:shards,linux/arm64','payload':{'spec':payload,'start':True}}},deadline);key=reply.get('key');cold.census.require(key is not None,'unit submit lacks durable key')
   while True:
    ready=self.call(socket,{'Status':{'key':key}},deadline)
    if ready.get('created'):break
   physical=ready.get('physical');cold.census.require(isinstance(physical,str) and physical,'missing physical unit box')
   while True:
    cold.census.remaining(deadline)
    original=self.runtime['wait'];self.runtime['wait']=cold.census.remaining(deadline)
    try:actual=self.snapshot(inspect=physical)
    finally:self.runtime['wait']=original
    info=actual.get('inspected',{})
    if info.get('exit') is not None:break
   cold.census.require(info['exit']=={'Code':expected},'native Linux unit returned unexpected exit '+str(info.get('exit')))
   retained=self.call(socket,{'Status':{'key':key}},deadline)
   cold.census.require(retained.get('tracked') and retained.get('created'),'init exit released long-lived box before deletion')
   self.call(socket,{'Delete':{'key':key}},deadline)
   while True:
    gone=self.call(socket,{'Status':{'key':key}},deadline)
    if gone.get('ended') or not gone.get('tracked',True):break
   cold.census.require(self.snapshot()['boxes']==self.baseline,'physical unit box survived deletion')
   self.report['cold_boot']['boxes_created']=True;self.report['cold_boot']['workload_success']='fixed/mutant/fixed functional unit cases, never seed data'
   self.report['unit_cases'].append({'label':label,'artifact':artifact,'test':test,'expected_exit':expected,'actual_exit':info['exit'],'key':key,'physical':physical,'box_retained_after_init_exit':True,'physical_deleted':True,'baseline_restored':True})
  cold.census.record(self.runtime,'native-arm-quota-fixed-mutant-fixed',cases=self.report['unit_cases'])
cold.ColdHarness=NativeQuota
original=cold.campaign
def campaign(count,one,save):
 def tagged(report):
  report['kind']='native-arm-linux-quota-unit-execution';report['workload_success']='see fixed/mutant/fixed unit cases in each retained result; no seed or full-rootlab claim';save(report)
 return original(count,one,tagged)
cold.campaign=campaign;sys.argv=[sys.argv[0],*rest];raise SystemExit(cold.main())
