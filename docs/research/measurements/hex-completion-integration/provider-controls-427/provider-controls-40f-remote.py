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
for name in ['target','cargo','temp']:(root/name).mkdir()
# Seed only public immutable package archives and sparse-index metadata from ROOT's prior
# same-platform private job. The destination source, target and cargo home are all new.
# No old extracted source, target, binary, credentials or configuration are copied.
prior=Path.home()/('.cache/hex-lead/cohort-b6ad2be7-'+target+'-compile-v1/cargo')
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
 cargo=Path('/usr/bin/cargo');rustc=Path('/usr/bin/rustc');rustdoc=Path('/usr/bin/rustdoc');env['RUSTUP_TOOLCHAIN']='stable'
else:raise ValueError('unknown target')
env.update(RUSTC=str(rustc),RUSTDOC=str(rustdoc))
version=subprocess.check_output([str(rustc),'-Vv'],cwd=source,env=env,text=True)
if 'release: 1.99.1\n' not in version:raise ValueError('wrong Rust release')
result={'source':revision,'archive_sha256':expected,'scope':'ROOT 3 pure RPC/authority controls only after canonical fixture closure; strict Clippy held known Node warnings; no native backend execution','target':target,'platform':platform.platform(),'rustc':version,'driver_pid':os.getpid(),'phases':[]}
commands=[
('rpc-control',['test','-p','hex-node','--lib','--locked','scheduler::tests::device_offer_tests::physical_rpc_facts_keep_unknowns_and_empty_native_inventory_cannot_grant','--','--exact','--test-threads=1']),
('alias-control',['test','-p','hex-node','--lib','--locked','placement::physical::tests::native_aliases_keep_one_owner_independent_unknowns_and_observed_pressure','--','--exact','--test-threads=1']),
('empty-control',['test','-p','hex-node','--lib','--locked','placement::physical::tests::empty_native_inventory_preserves_its_authority','--','--exact','--test-threads=1']),
]
for name,args in commands:
 command=['nice','-n','10',str(cargo),*args];phase={'name':name,'command':command};result['phases'].append(phase)
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
 if name!='joined-clippy':phase['selected_one_passed']=b'1 passed; 0 failed; 0 ignored' in raw and b'running 1 test' in raw
 if phase['exit'] or phase.get('timed_out') or phase.get('selected_one_passed') is False:break
post={str(p.relative_to(source)):sha(p) for p in sorted(source.rglob('*')) if p.is_file()};(root/'source-after.json').write_text(json.dumps(post,indent=2)+'\n')
result.update(source_changes=sorted(k for k in set(manifest)|set(post) if manifest.get(k)!=post.get(k)),temp_entries=len(list((root/'temp').iterdir())),native_invocations=0)
result['exit']=0 if len(result['phases'])==3 and all(p['exit']==0 and not p.get('timed_out') and p.get('selected_one_passed',True) for p in result['phases']) and manifest==post else 1
(root/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2),flush=True)
if result['exit']:print(raw.decode(errors='replace')[-16000:],flush=True)
sys.exit(result['exit'])
