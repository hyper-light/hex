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
result={'source':revision,'archive_sha256':expected,'scope':'ROOT5688 new shared native fixture2 only plus affected Runtime/Node/CLI strict incl new parentdeath observations compile; no oldnative/producer5 replay, no process/VM/GPU native.','target':target,'platform':platform.platform(),'rustc':version,'driver_pid':os.getpid(),'phases':[]}
phases=[
 ('native-fixed-fixture-new2',['test','-p','hex-runtime','--lib','--locked','guest::native_fixture::tests::','--','--nocapture','--test-threads=1'],2),
 ('fixture-and-observations-strict',['clippy','-p','hex-runtime','-p','hex-node','-p','hex-cli','--all-targets','--locked','--no-deps','--','-D','warnings'],None)]
for name,args,selected in phases:
 command=(['nice','-n','10',sys.executable,*args[1:]] if args[0]=='__python__' else ['nice','-n','10',str(cargo),*args]);phase={'name':name,'command':command};result['phases'].append(phase)
 with (root/(name+'.log')).open('xb') as log:
  child=subprocess.Popen(command,cwd=source,env=env,stdout=log,stderr=subprocess.STDOUT);phase['pid']=child.pid;(root/'running.json').write_text(json.dumps(result,indent=2)+'\n')
  try:phase['exit']=child.wait(timeout=900)
  except subprocess.TimeoutExpired:
   phase['timed_out']=True;child.terminate()
   try:child.wait(timeout=10)
   except subprocess.TimeoutExpired:child.kill();child.wait()
   phase['exit']=child.returncode
  phase['reaped']=child.returncode is not None
 raw=(root/(name+'.log')).read_bytes();phase.update(sha256=hashlib.sha256(raw).hexdigest(),bytes=len(raw))
 if selected is not None:
  counts=re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',raw.decode(errors='replace'))
  if args[0]=='__python__':
   matched=re.search(r'Ran (\d+) tests',raw.decode(errors='replace'));counts=[(matched.group(1),'0','0')] if matched and phase['exit']==0 and '\nOK\n' in raw.decode(errors='replace') else []
  phase['expected_selected']=selected
  phase['actual_counts']=[{'passed':int(p),'failed':int(f),'ignored':int(i)} for p,f,i in counts]
  phase['selection_verified']=len(counts)==1 and sum(map(int,counts[0]))==selected and int(counts[0][2])==0
 phase['test_outcomes'] = [{'name': name, 'outcome': outcome} for name, outcome in re.findall(r'test (\S+) \.\.\. (ok|FAILED|ignored)', raw.decode(errors='replace'))] if selected is not None else []
 if phase.get('timed_out') or (selected is not None and phase['exit']!=0 and not phase.get('actual_counts')):break
post={str(p.relative_to(source)):sha(p) for p in sorted(source.rglob('*')) if p.is_file()};(root/'source-after.json').write_text(json.dumps(post,indent=2)+'\n')
result.update(source_changes=sorted(k for k in set(manifest)|set(post) if manifest.get(k)!=post.get(k)),temp_entries=len(list((root/'temp').iterdir())),native_invocations=0)
result['exit']=0 if len(result['phases'])==len(phases) and all(p['exit']==0 and not p.get('timed_out') and p.get('selection_verified',True) for p in result['phases']) and manifest==post else 1
(root/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2),flush=True)
if result['exit']:print(raw.decode(errors='replace')[-18000:],flush=True)
sys.exit(result['exit'])
