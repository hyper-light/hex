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
prior=Path.home()/'.cache/hex-lead/cohort-25d98e88-linux-producer-recovery-25d9/cargo'
locked=tomllib.loads((source/'Cargo.lock').read_text())
checks={(p['name'],p['version']):p['checksum'] for p in locked['package'] if p.get('source','').startswith('registry+')}
seeded=[]
if (prior/'registry/cache').exists():
 for file in sorted((prior/'registry/cache').glob('*/*.crate')):
  if file.is_symlink() or not file.is_file():raise ValueError('nonregular package cache member')
  name=file.name[:-6];key=next((key for key in checks if name==key[0]+'-'+key[1]),None)
  if key is None:continue
  digest=sha(file)
  if digest!=checks[key]:raise ValueError('cached package checksum differs from exact root lock')
  into=root/'cargo/registry/cache'/file.parent.name/file.name;into.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(file,into)
  seeded.append({'file':str(file.relative_to(prior)),'sha256':digest,'bytes':file.stat().st_size})
indexhash={}
if (prior/'registry/index').exists():
 for file in sorted((prior/'registry/index').rglob('*')):
  if file.is_symlink():raise ValueError('symlink in ROOT sparse registry cache')
  if not file.is_file():continue
  rel=file.relative_to(prior);into=root/'cargo'/rel;into.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(file,into);indexhash[str(rel)]=sha(file)
(root/'public-registry-seed.json').write_text(json.dumps({'source':str(prior),'packages':seeded,'index_hashes':indexhash,'extracted_source_or_binary_or_credentials_copied':False},indent=2)+'\n')
env={k:v for k,v in os.environ.items() if not k.startswith(('DYLD_','HEX_DEBUG_','SHARDS_DEBUG_'))}
env.update(CARGO_HOME=str(root/'cargo'),CARGO_TARGET_DIR=str(root/'target'),CARGO_BUILD_JOBS='1',RUST_TEST_THREADS='1',RAYON_NUM_THREADS='1',TMPDIR=str(root/'temp'),RUSTUP_AUTO_INSTALL='0')
if target=='mac':
 rustup=Path.home()/'.cache/hex-lead/rustup-scheduler-copy';tc=rustup/'toolchains/1.99.1-aarch64-apple-darwin/bin';cargo=tc/'cargo';rustc=tc/'rustc';rustdoc=tc/'rustdoc';env.update(PATH=str(tc)+os.pathsep+env['PATH'],RUSTUP_HOME=str(rustup))
elif target=='linux':
 tc=Path.home()/'.rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin';cargo=tc/'cargo';rustc=tc/'rustc';rustdoc=tc/'rustdoc';env.update(PATH=str(tc)+os.pathsep+env['PATH'],RUSTUP_TOOLCHAIN='1.99.1')
else:raise ValueError('unknown target')
env.update(RUSTC=str(rustc),RUSTDOC=str(rustdoc))
scope=Path('/sys/fs/cgroup')/Path(next(line.split(':',2)[2] for line in Path('/proc/self/cgroup').read_text().splitlines() if line.startswith('0::'))).relative_to('/')
limits={name:(scope/name).read_text().strip() for name in ['cpu.max','memory.max','memory.swap.max','pids.max']}
if limits!={'cpu.max':'100000 100000','memory.max':'4294967296','memory.swap.max':'0','pids.max':'256'}:raise ValueError('owned scope resource envelope differs: '+str(limits))
(root/'owned-scope-limits.json').write_text(json.dumps({'cgroup':str(scope),'limits':limits},indent=2)+'\n')
version=subprocess.check_output([str(rustc),'-Vv'],cwd=source,env=env,text=True)
if 'release: 1.99.1\n' not in version:raise ValueError('wrong Rust release')

comparison=source/'docs/research/measurements/hex-completion-validation/native-gate-r05/root_native_gate_comparison.py'
if sha(comparison)!='780e113dfed6063e6d1a67f23d8cdb4568e49f0ec0a9796c9303b1fe842ff6b2':raise ValueError('reviewed comparison driver changed')
output=Path.home()/'.cache/hex-lead/gc5688v1'
if output.exists():raise ValueError('comparison output is not new')
birth=lambda pid:Path('/proc')/str(pid)/'stat'
def owned_birth(pid):
 raw=birth(pid).read_text();tail=raw.rsplit(')',1)[1].split()
 if len(tail)<20:raise ValueError('short native process identity')
 return int(tail[19])
result={'source':revision,'archive_sha256':expected,'target':target,'platform':platform.platform(),'rustc':version,'driver_pid':os.getpid(),'driver_birth':owned_birth(os.getpid()),'scope':'ROOT only prepare/build/seal three fresh exact sources/targets; no tests/native/VM/GPU invocation','output':str(output),'phases':[]}
def record():
 (root/'running.json').write_text(json.dumps(result,indent=2)+'\n')
def phase(name,args,cwd,runenv,seconds,stdout,stderr):
 row={'name':name,'command':args,'deadline_seconds':seconds,'start_ns':time.monotonic_ns()};result['phases'].append(row)
 with stdout.open('xb') as out,stderr.open('xb') as err:
  child=subprocess.Popen(args,cwd=cwd,env=runenv,stdout=out,stderr=err,start_new_session=True)
  row.update(pid=child.pid,process_group=child.pid)
  try:
   row['birth']=owned_birth(child.pid);record()
   row['exit']=child.wait(timeout=seconds)
  except subprocess.TimeoutExpired:
   row['timed_out']=True
   # The sole Popen waiter has not reaped the session leader; this exact group is ours.
   os.killpg(child.pid,15)
   try:child.wait(timeout=10)
   except subprocess.TimeoutExpired:os.killpg(child.pid,9);child.wait()
   row['exit']=child.returncode
  except BaseException:
   if child.poll() is None:
    os.killpg(child.pid,15)
    try:child.wait(timeout=10)
    except subprocess.TimeoutExpired:os.killpg(child.pid,9);child.wait()
   else:child.wait()
   row.update(exit=child.returncode,reaped=True);record();raise
  row['reaped']=child.returncode is not None
 row.update(end_ns=time.monotonic_ns(),stdout={'path':str(stdout),'bytes':stdout.stat().st_size,'sha256':sha(stdout)},stderr={'path':str(stderr),'bytes':stderr.stat().st_size,'sha256':sha(stderr)})
 record()
 if row['exit']!=0 or row.get('timed_out'):raise ValueError('phase did not complete0: '+name)
 return row
try:
 phase('comparison-prepare',['nice','-n','10',sys.executable,'-B',str(comparison),'prepare','--source',str(source),'--source-pin',revision,'--manifest',str(root/'source-before.json'),'--manifest-sha256',sha(root/'source-before.json'),'--output',str(output)],source,env,180,root/'prepare.stdout',root/'prepare.stderr')
 plan=output/'prepared.json';prepared=json.loads(plan.read_text());result['prepared_sha256']=sha(plan)
 for name in ('baseline','bypass','restored'):
  cell=Path(prepared['cells'][name]['root'])
  if any((cell/'target').iterdir()) or any((cell/'cargo').iterdir()):raise ValueError('target/cargo must start EMPTY')
  for rel in ('registry/cache','registry/index'):
   origin=root/'cargo'/rel
   if origin.exists():shutil.copytree(origin,cell/'cargo'/rel)
  (cell/'public-registry-seed.json').write_text((root/'public-registry-seed.json').read_text())
  runenv=dict(env);runenv.update(CARGO_HOME=str(cell/'cargo'),CARGO_TARGET_DIR=str(cell/'target'),TMPDIR=str(cell/'temp'))
  command=['nice','-n','10',str(cargo),'test','-p','hex-node','--lib','--locked','--no-run','--message-format=json','--target','x86_64-unknown-linux-gnu','-j1']
  try:row=phase(name+'-compile',command,Path(prepared['cells'][name]['source']),runenv,900,cell/'cargo.jsonl',cell/'cargo.stderr')
  finally:
   row=result['phases'][-1]
   (cell/'cargo.exit').write_text(str(row.get('exit','UNKNOWN'))+'\n')
  row['target_created_empty']=True;row['target_reused']=False
  row['temp_entries']=len(list((cell/'temp').iterdir()));record()
 phase('comparison-seal',['nice','-n','10',sys.executable,'-B',str(comparison),'seal','--plan',str(plan),'--plan-sha256',result['prepared_sha256'],'--output',str(output/'sealed.json')],source,env,180,root/'seal.stdout',root/'seal.stderr')
 result['sealed_sha256']=sha(output/'sealed.json');result['exit']=0
except BaseException as error:
 result['error']=type(error).__name__+': '+str(error);result['exit']=1
finally:
 post={str(p.relative_to(source)):sha(p) for p in sorted(source.rglob('*')) if p.is_file()}
 (root/'source-after.json').write_text(json.dumps(post,indent=2)+'\n')
 result.update(source_changes=sorted(k for k in set(manifest)|set(post) if manifest.get(k)!=post.get(k)),temp_entries=len(list((root/'temp').iterdir())),native_invocations=0)
 if manifest!=post:result['exit']=1
 for name,path in [('build_root',root),('comparison_root',output)]:
  if path.exists():
   measured=subprocess.run(['/usr/bin/du','-sk',str(path)],capture_output=True,text=True,check=False,timeout=30)
   result[name+'_du']={'exit':measured.returncode,'stdout':measured.stdout,'stderr':measured.stderr}
 (root/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2),flush=True)
sys.exit(result['exit'])
