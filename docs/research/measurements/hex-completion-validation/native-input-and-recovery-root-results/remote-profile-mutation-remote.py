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
result={'source':revision,'archive_sha256':expected,'scope':'ROOT single new exact peer remote-owner committed pending original; production unchanged, no previous pass replay or remedy; no native/VM/GPU.','target':target,'platform':platform.platform(),'rustc':version,'driver_pid':os.getpid(),'phases':[]}
patch=b'diff --git a/crates/hex-sim/src/index/routes.rs b/crates/hex-sim/src/index/routes.rs\n--- a/crates/hex-sim/src/index/routes.rs\n+++ b/crates/hex-sim/src/index/routes.rs\n@@ -515,8 +515,8 @@\n                 cost: allocation.map(|allocation| allocation.cost),\n                 ..choice.offer\n             };\n-            return original.check_backing(backing).is_ok()\n-                && self.holds_dedicated_inner(key, backing, demand);\n+            let _profile_checked = original.check_backing(backing).is_ok();\n+            return self.holds_dedicated_inner(key, backing, demand);\n         }\n         self.holds_inner(choice, demand)\n     }\n'
patch_file=root/'profile-enforcement.patch';patch_file.write_bytes(patch)
assert hashlib.sha256(patch).hexdigest()=='af92e293ddf6b7434c3a4b9e1cbc775586f09c1d8277438c1ec7a55266b7798d'
route=source/'crates/hex-sim/src/index/routes.rs';unit=source/'crates/hex-sim/src/index/route_offer_tests.rs'
original='cda46b5fee827aaab61a9e2fcf7f1e9b7085c596017cbd7de3d3d2ebabb399fc';mutant='e9fbf5d8aafdf9b12755a1d07f91ce21842a61a4beb53a9cd098ba267033b1c6';test_sha='795f0d8ee79eaa8226905c5dc22bdc142942be1f8f1ab03cbfb3a48142a9a614'
assert sha(route)==original and sha(unit)==test_sha
result['purpose']='R10 one exact existing profile-enforcement guard omitted, same unmodified pure unit; restored original source/test then separate empty target. No native/VM/GPU/benchmark.'
def phase(name):
 target_dir=root/('target-'+name);target_dir.mkdir(exist_ok=False);env['CARGO_TARGET_DIR']=str(target_dir)
 cmd=['nice','-n','10',str(cargo),'test','-p','hex-sim','--lib','--locked','index::route_offer_tests::committed_pending_recovery_keeps_receipt_charge_and_claim_bounds','--','--exact','--nocapture','--test-threads=1']
 row={'name':name,'command':cmd,'source_routes_sha256':sha(route),'test_sha256':sha(unit),'target':str(target_dir)};result['phases'].append(row)
 try:
  with (root/(name+'.log')).open('xb') as log:
   child=subprocess.Popen(cmd,cwd=source,env=env,stdout=log,stderr=subprocess.STDOUT);row['pid']=child.pid;(root/'running.json').write_text(json.dumps(result,indent=2)+'\n')
   try:row['exit']=child.wait(timeout=900)
   except subprocess.TimeoutExpired:
    row['timed_out']=True;child.terminate()
    try:child.wait(timeout=10)
    except subprocess.TimeoutExpired:child.kill();child.wait()
    row['exit']=child.returncode
   row['reaped']=child.returncode is not None
  raw=(root/(name+'.log')).read_bytes();row.update(bytes=len(raw),sha256=hashlib.sha256(raw).hexdigest())
  counts=re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',raw.decode(errors='replace'))
  row['actual_counts']=[{'passed':int(p),'failed':int(f),'ignored':int(i)} for p,f,i in counts];row['selection_verified']=len(counts)==1 and sum(map(int,counts[0]))==1 and int(counts[0][2])==0
  row['foreign_profile_diagnostic']='foreign profile' in raw.decode(errors='replace')
 except Exception as error:row['error']=str(error)
 return row
result['restoration']={'attempted':False,'verified':False};error=None
route.parent.chmod(0o755);route.chmod(0o644)
try:
 subprocess.run(['git','apply','--no-index','--check','-'],input=patch,cwd=source,check=True)
 subprocess.run(['git','apply','--no-index','-'],input=patch,cwd=source,check=True)
 assert sha(route)==mutant and sha(unit)==test_sha
 (root/'mutant-source-pin.json').write_text(json.dumps({'routes':sha(route),'unit':sha(unit)})+'\n')
 phase('mutant')
except Exception as why:error=str(why)
finally:
 result['restoration']['attempted']=True
 try:
  actual=sha(route)
  if actual==mutant:
   subprocess.run(['git','apply','--no-index','--reverse','--check','-'],input=patch,cwd=source,check=True)
   subprocess.run(['git','apply','--no-index','--reverse','-'],input=patch,cwd=source,check=True)
  elif actual!=original:raise ValueError('unexpected routes state; refuse blind restoration')
  assert sha(route)==original and sha(unit)==test_sha
  route.chmod(0o444);route.parent.chmod(0o555)
  result['restoration']['verified']=True
 except Exception as why:result['restoration']['error']=str(why)
if result['restoration']['verified']:phase('restored')
result['setup_error']=error
phases=result['phases']
post={str(p.relative_to(source)):sha(p) for p in sorted(source.rglob('*')) if p.is_file()};(root/'source-after.json').write_text(json.dumps(post,indent=2)+'\n')
result.update(source_changes=sorted(k for k in set(manifest)|set(post) if manifest.get(k)!=post.get(k)),temp_entries=len(list((root/'temp').iterdir())),native_invocations=0)
good=(len(phases)==2 and phases[0].get('actual_counts')==[{'passed':0,'failed':1,'ignored':0}] and phases[0].get('foreign_profile_diagnostic') and phases[0].get('exit')!=0 and phases[1].get('actual_counts')==[{'passed':1,'failed':0,'ignored':0}] and phases[1].get('exit')==0 and all(p.get('reaped') and not p.get('timed_out') and p.get('selection_verified') for p in phases) and result['restoration']['verified'] and error is None and manifest==post)
result['mutation_relevant']=bool(good);result['exit']=0 if good else 1
(root/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2),flush=True)
if result['exit']:print('R10 incomplete: inspect exact retained phase logs',flush=True)
sys.exit(result['exit'])
