import re
import hashlib,io,json,os,platform,subprocess,sys,tarfile,time,shutil,tomllib
from pathlib import Path,PurePosixPath
root=Path(sys.argv[1]).expanduser();expected,revision,target=sys.argv[2:5]
root.mkdir(parents=True,exist_ok=False);data=sys.stdin.buffer.read()
if hashlib.sha256(data).hexdigest()!=expected:raise ValueError('archive SHA mismatch')
(root/'source.tar.gz').write_bytes(data);source=root/'source';source.mkdir()
with tarfile.open(fileobj=io.BytesIO(data),mode='r:gz') as bundle:
 for m in bundle.getmembers():
  p=PurePosixPath(m.name)
  if p.is_absolute() or '..' in p.parts or not (m.isfile() or m.isdir()):raise ValueError('nonregular member')
 bundle.extractall(source,filter='data')
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
manifest={str(p.relative_to(source)):sha(p) for p in sorted(source.rglob('*')) if p.is_file()}
(root/'source-before.json').write_text(json.dumps(manifest,indent=2)+'\n')
for file in source.rglob('*'):
 if file.is_file():file.chmod(0o444 | (file.stat().st_mode & 0o111))
for directory in sorted((p for p in source.rglob('*') if p.is_dir()),key=lambda p:len(p.parts),reverse=True):directory.chmod(0o555)
source.chmod(0o555)
for name in ['target','cargo','temp']:(root/name).mkdir()
# Seed only public immutable package archives and sparse-index metadata from ROOT's prior
# same-platform private job. The destination source, target and cargo home are all new.
# No old extracted source, target, binary, credentials or configuration are copied.
env={k:v for k,v in os.environ.items() if not k.startswith(('DYLD_','HEX_DEBUG_','SHARDS_DEBUG_'))}
env.update(CARGO_HOME=str(root/'cargo'),CARGO_TARGET_DIR=str(root/'target'),CARGO_BUILD_JOBS='1',RUST_TEST_THREADS='1',RAYON_NUM_THREADS='1',TMPDIR=str(root/'temp'),RUSTUP_AUTO_INSTALL='0')
if target!='linux':raise ValueError('bounded vendor source check is Linux only')
env['KILN_VENDOR_CACHE']=str(root/'vendor-cache')
scope=Path('/sys/fs/cgroup')/Path(next(line.split(':',2)[2] for line in Path('/proc/self/cgroup').read_text().splitlines() if line.startswith('0::'))).relative_to('/')
limits={name:(scope/name).read_text().strip() for name in ['cpu.max','memory.max','memory.swap.max','pids.max']}
if limits!={'cpu.max':'100000 100000','memory.max':'4294967296','memory.swap.max':'0','pids.max':'256'}:raise ValueError('owned scope resource envelope differs: '+str(limits))
(root/'owned-scope-limits.json').write_text(json.dumps({'cgroup':str(scope),'limits':limits},indent=2)+'\n')
import importlib.util
helper=source/'docs/research/measurements/hex-completion-validation/native-gate-r05/root_native_gate_baseline.py'
if sha(helper)!='d1b42b03247b336c47a6724d6640c114c89c9ceca81d521db637220585759531':raise ValueError('immutable event/reap primitives changed')
spec=importlib.util.spec_from_file_location('root_a128_event_primitives',helper);base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base)
os.chdir(source)  # The shared event/reap primitive inherits this exact archived source directory.


result={'source':revision,'archive_sha256':expected,'scope':'ROOT vendored exact-source/pin/patch check only, private cache; no Cargo, gates, native runtime, VM or GPU','platform':platform.platform(),'driver':base.birth(os.getpid()),'phases':[]}
(root/'event-evidence').mkdir();evidence=base.Evidence(root/'event-evidence')
try:
 command=['nice','-n','10','/usr/bin/python3','-B','scripts/vendor.py','check']
 _,observed=base.run_owned(command,env,root/'vendor.stdout',root/'vendor.stderr',time.monotonic()+900,evidence)
 row={'name':'vendor-source-consistency','command':command,'observation':observed}
 for ending in ['stdout','stderr']:
  p=root/('vendor.'+ending);row[ending]={'bytes':p.stat().st_size,'sha256':sha(p)}
 result['phases'].append(row)
finally:
 post={str(p.relative_to(source)):sha(p) for p in sorted(source.rglob('*')) if p.is_file()};(root/'source-after.json').write_text(json.dumps(post,indent=2)+'\n');result.update(source_changes=sorted(k for k in set(manifest)|set(post) if manifest.get(k)!=post.get(k)),temp_entries=len(list((root/'temp').iterdir())),native_invocations=0,event_errors=evidence.failures)
 result['exit']=0 if len(result['phases'])==1 and result['phases'][0]['observation']['returncode']==0 and not result['phases'][0]['observation']['error'] and result['phases'][0]['observation']['reaped'] and result['phases'][0]['observation']['both_eof'] and manifest==post and not evidence.failures else 1
 evidence.journal.close();(root/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2),flush=True)
sys.exit(result['exit'])
