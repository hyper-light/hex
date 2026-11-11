from pathlib import Path,PurePosixPath
import subprocess,hashlib,io,tarfile,json,gzip
r=Path('/Users/adalundhe/.cache/hex-lead/worktrees/scheduler-live')
sha=lambda b:hashlib.sha256(b).hexdigest()
def read(rev,p):return subprocess.check_output(['git','show',rev+':'+p],cwd=r)
def unpack(raw):
 out={}
 with tarfile.open(fileobj=io.BytesIO(raw)) as tar:
  for m in tar:
   p=PurePosixPath(m.name)
   assert m.isfile() and not p.is_absolute() and '..' not in p.parts and m.name not in out
   out[m.name]=tar.extractfile(m).read()
 return out
prefix='docs/research/measurements/hex-complete-bindings/';qrev='30d588ca59bac86949828d44ef27eb52598eecdf';q=json.loads(read(qrev,prefix+'unread-quality-green.json'))
for name,digest in q['artifacts'].items():assert sha(read(qrev,prefix+name))==digest,name
raw=unpack(read(qrev,prefix+'unread-quality-green-raw.tar.gz'));sources=unpack(read(qrev,prefix+'unread-quality-green-source.tar.gz'));manifest=json.loads(sources['source-readback.json'])
for name,digest in manifest['sources'].items():assert sha(sources[name])==digest,name
for field,name in [('green','green.log'),('retained_cpu','retained-v3.log')]:
 assert sha(raw[name])==q[field]['log_sha256'];text=raw[name].decode();assert 'running 1 test' in text and '1 passed; 0 failed; 0 ignored' in text
assert q['green']['selected']=='None' and q['original_red']['selected']=='Some([0])'
refs=json.loads(read(qrev,prefix+'unread-quality-reference.json'))
for name,digest in refs['sha256'].items():assert sha(read(refs['signed_source'],name))==digest
# Verify the recorded exact source-function port against the signed production implementation.
def block(t,marker):
 a=t.index(marker);b=t.index('{',a);d=1;b+=1
 while d:
  if t[b]=='{':d+=1
  elif t[b]=='}':d-=1
  b+=1
 return t[a:b]
markers={'crates/hex-sim/src/model.rs':['pub struct GpuLoad {','impl Device {','pub fn bind(','pub fn matches('],'crates/hex-sim/src/index.rs':['fn gpu_busy(','fn gpu_idle('],'crates/hex-node/src/facts.rs':['pub fn gpu_facts(']}
ported={}
for name,names in markers.items():
 for marker in names:
  tested=block(sources[name].decode(),marker);signed=block(read(refs['signed_source'],name).decode(),marker);assert tested==signed,(name,marker);ported[name+':'+marker]=sha(tested.encode())
wp='docs/research/measurements/hex-complete-lifecycle/';wrev='057ddc2e917629922578eb9418b69113c800f09d';w=json.loads(read(wrev,wp+'watch-fallible-v3.json'));wraw=read(wrev,wp+'watch-fallible-v3.tar.gz');assert sha(wraw)==w['sha256'];wm=unpack(wraw);assert set(wm)==set(w['members'])
for name,entry in w['members'].items():assert {'bytes':len(wm[name]),'sha256':sha(wm[name])}==entry,name
run=json.loads(wm['result.json']);assert run==w['run']
for phase in run['phases']:
 assert phase['exit_code']==0 and phase['child_reaped'];assert sha(wm[phase['name']+'.log'])==phase['log_sha256']
assert '29 passed; 0 failed; 0 ignored' in wm['watch-fallible-core.log'].decode()
inputs=json.loads(wm['source-hashes.json']);base={}
with tarfile.open(fileobj=io.BytesIO(subprocess.check_output(['git','archive','c3bd27e84680ee4d587c5be374b361a567f2419f'],cwd=r))) as tar:
 for m in tar:
  if m.isfile():base[m.name]=sha(tar.extractfile(m).read())
for name in ['crates/hex-core/src/record.rs','crates/hex-core/src/watch.rs']:base[name]=sha(read('6efa2d63f0794243269b3fb9e9af62a536a66446',name));assert inputs[name]==base[name]
cohort='docs/research/measurements/hex-complete-lifecycle/cohort.json';base[cohort]=sha(wm['source/'+cohort]);assert inputs==base,'full Watch input differs from declared source cohort'
for name,digest in run['source_hashes'].items():assert digest==inputs[name]
assert run['remaining_temp_entries']==0 and run['native_backend_invocations']==0 and w['source_unchanged']
out=r/'docs/research/measurements/hex-completion-integration/quality-watch-owner-intake';out.mkdir(exist_ok=False)
for name,rev,path in [('quality',qrev,prefix),('watch',wrev,wp)]:
 files=['unread-quality-green.json','unread-quality-reference.json','unread-quality-port.py','unread-quality-compat.py','unread-quality-green-raw.tar.gz','unread-quality-green-source.tar.gz','UNREAD-QUALITY-GREEN.md'] if name=='quality' else ['watch-fallible-v3.json','watch-fallible-v3.tar.gz','watch-fallible-v3-results.md','watch_fallible_check.py','submit_watch_fallible.py']
 with tarfile.open(fileobj=(buffer:=io.BytesIO()),mode='w:gz') as tar:
  for item in files:
   payload=read(rev,path+item);member=tarfile.TarInfo(item);member.size=len(payload);member.mode=0o644;tar.addfile(member,io.BytesIO(payload))
 (out/(name+'.tar.gz')).write_bytes(buffer.getvalue())
record={'quality':{'revision':qrev,'ported_production_blocks':ported,'green':q['green'],'retained_cpu':q['retained_cpu'],'source_manifest_files':len(manifest['sources'])},'watch':{'revision':wrev,'cohort_root':'c3bd27e84680ee4d587c5be374b361a567f2419f plus exact6efa core record/watch and recorded cohort metadata','all_git_input_hashes_verified':len(inputs),'units':[29,0,0],'strict_core_lib_clippy':0,'remote_source_after_unchanged':'signed collector reports true; no retained independent after-manifest in this bundle','children_reaped':True,'temp_entries':0},'limit':'Original source-specific pure checks only. No new tests, native GPU/VM/HCS, fault injection, mutations or new joined source gate performed by intake.'}
(out/'verification.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps({'quality_blocks':len(ported),'quality_units':[1,1],'watch_inputs':len(inputs),'watch_units':[29,0,0],'result':'verified retained inputs/bytes/logs; no new execution'},indent=2))
