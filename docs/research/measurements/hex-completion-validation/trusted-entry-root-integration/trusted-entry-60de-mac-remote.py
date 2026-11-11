import re
import hashlib,io,json,os,platform,subprocess,sys,tarfile,time,shutil,tomllib
from pathlib import Path,PurePosixPath
root=Path(sys.argv[1]).expanduser();expected,revision,target=sys.argv[2:5]
root.mkdir(parents=True,exist_ok=False);
if target!='mac' or shutil.disk_usage(root).free<30*1024**3:raise ValueError('Mac arm64 cohort needs preserved30GiB free floor')
if not all(hasattr(os,k) for k in ['waitid','P_PID','WEXITED','WNOWAIT','WNOHANG']):raise ValueError('retained native child observation unavailable')
data=sys.stdin.buffer.read()
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
prior=Path.home()/'.cache/hex-lead/cohort-786775e3-mac-portability-native-image-786/cargo'
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
version=subprocess.check_output([str(rustc),'-Vv'],cwd=source,env=env,text=True)
if 'release: 1.99.1\n' not in version:raise ValueError('wrong Rust release')

import select,signal,errno
absolute=time.monotonic()+3280
result={'source':revision,'archive_sha256':expected,'scope':'ROOT opt-in trusted-entry new8 pure controls and affected feature/default strict; no dispatch/run/native launch or old passes replayed','platform':platform.platform(),'rustc':version,'driver_pid':os.getpid(),'phases':[]}
def present(group):
 try:os.killpg(group,0);return True
 except ProcessLookupError:return False
phases=[
 ('entry-feature-strict',['clippy','-p','hex-runtime','-p','hex-node','-p','hex-cli','--all-targets','--locked','--offline','--features','hex-runtime/native-trusted-entry-measurement,hex-cli/native-trusted-entry-measurement','--','-D','warnings'],None),
 ('native-component-pure3',['test','-p','hex-runtime','--lib','--locked','--offline','--features','native-trusted-entry-measurement','macos::sys::trusted_entry_native::controls::','--','--nocapture','--test-threads=1'],3),
 ('entry-caller-pure5',['test','-p','hex-runtime','--lib','--locked','--offline','--features','native-trusted-entry-measurement','macos::process::trusted_entry::tests::','--','--nocapture','--test-threads=1'],5),
 ('ordinary-mac-runtime-strict',['clippy','-p','hex-runtime','--all-targets','--locked','--offline','--','-D','warnings'],None)]
try:
 for name,args,count in phases:
  row={'name':name,'command':['nice','-n','10',str(cargo),*args]};result['phases'].append(row);deadline=min(absolute,time.monotonic()+900)
  with (root/(name+'.log')).open('xb') as log:
   queue=select.kqueue();child=subprocess.Popen(row['command'],cwd=source,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True);row.update(pid=child.pid,pgid=child.pid)
   (root/'running.json').write_text(json.dumps(result,indent=2)+'\n')
   identity=subprocess.run(['/bin/ps','-p',str(child.pid),'-o','pid=,pgid=,lstart='],stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=min(5,max(0.001,deadline-time.monotonic())),text=True)
   row['native_identity']={'query_exit':identity.returncode,'row':identity.stdout.strip(),'error':identity.stderr.strip()}
   try:
    queue.control([select.kevent(child.pid,filter=select.KQ_FILTER_PROC,flags=select.KQ_EV_ADD|select.KQ_EV_ENABLE|select.KQ_EV_ONESHOT,fflags=select.KQ_NOTE_EXIT)],0,0)
   except OSError as e:
    if e.errno!=errno.ESRCH:row['event_registration_error']=str(e)
   observed=None
   while True:
    observed=os.waitid(os.P_PID,child.pid,os.WEXITED|os.WNOWAIT|os.WNOHANG)
    if observed is not None:break
    left=deadline-time.monotonic()
    if left<=0:row['timed_out']=True;break
    queue.control([],1,left)
   if observed is None:
    # Leader remains unreaped, so this exact recorded process group cannot be reused.
    os.killpg(child.pid,signal.SIGTERM)
    try:child.wait(timeout=10)
    except subprocess.TimeoutExpired:
     os.killpg(child.pid,signal.SIGKILL)
     try:child.wait(timeout=10)
     except subprocess.TimeoutExpired:row['cleanup_uncertain']='direct child not reaped within finite cleanup'
   else:
    row['native_wait']={'pid':observed.si_pid,'code':observed.si_code,'status':observed.si_status};child.wait(timeout=10)
   queue.close();row.update(exit=child.returncode,reaped=child.returncode is not None,owned_group_absent=not present(child.pid))
  raw=(root/(name+'.log')).read_bytes();row.update(bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest())
  if count is not None:
   counts=re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',raw.decode(errors='replace'));row['actual_counts']=counts;row['selection_verified']=len(counts)==1 and counts[0]==(str(count),'0','0')
  row['qualified']=row['exit']==0 and row['reaped'] and row['owned_group_absent'] and not row.get('timed_out') and not row.get('cleanup_uncertain') and not row.get('event_registration_error') and row.get('selection_verified',True)
  (root/'running.json').write_text(json.dumps(result,indent=2)+'\n')
  if not row['qualified']:break
finally:
 post={str(p.relative_to(source)):sha(p) for p in sorted(source.rglob('*')) if p.is_file()};(root/'source-after.json').write_text(json.dumps(post,indent=2)+'\n');result.update(source_changes=sorted(k for k in set(manifest)|set(post) if manifest.get(k)!=post.get(k)),temp_entries=len(list((root/'temp').iterdir())),native_test_invocations=0,VM_GPU_invocations=0)
 result['exit']=0 if len(result['phases'])==len(phases) and all(p.get('qualified') for p in result['phases']) and manifest==post and result['temp_entries']==0 else 1
 (root/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2),flush=True)
sys.exit(result['exit'])
