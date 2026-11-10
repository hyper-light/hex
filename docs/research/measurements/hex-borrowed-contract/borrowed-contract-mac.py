import hashlib,io,json,os,platform,signal,subprocess,sys,tarfile,tomllib
from pathlib import Path,PurePosixPath
root=Path(sys.argv[1]);transport_sha,source_sha,seed_sha,revision=sys.argv[2:6]
root.mkdir(parents=True,exist_ok=False);data=sys.stdin.buffer.read();assert hashlib.sha256(data).hexdigest()==transport_sha
(root/'input.tar.gz').write_bytes(data)
with tarfile.open(fileobj=io.BytesIO(data),mode='r:gz') as outer:
 assert {m.name for m in outer.getmembers()}=={'source.tar.gz','upstream.lock'}
 assert all(m.isfile() for m in outer.getmembers())
 archived=outer.extractfile('source.tar.gz').read();seed=outer.extractfile('upstream.lock').read()
assert hashlib.sha256(archived).hexdigest()==source_sha and hashlib.sha256(seed).hexdigest()==seed_sha
(root/'upstream.lock').write_bytes(seed);(root/'source.tar.gz').write_bytes(archived)
source=root/'source';source.mkdir()
with tarfile.open(fileobj=io.BytesIO(archived),mode='r:gz') as bundle:
 for m in bundle.getmembers():
  p=PurePosixPath(m.name);assert not p.is_absolute() and '..' not in p.parts and(m.isfile() or m.isdir())
 bundle.extractall(source)
def hashes():return {str(p.relative_to(source)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(source.rglob('*')) if p.is_file() and str(p.relative_to(source))!='vendor/shards/Cargo.lock'}
before=hashes();(root/'source-hashes.json').write_text(json.dumps(before,indent=2)+'\n')
(source/'vendor/shards/Cargo.lock').write_bytes(seed)
short=Path.home()/('.cache/hbm-'+revision[:8]);short.mkdir(exist_ok=False)
for d in ['cache/cargo','target']:(root/d).mkdir(parents=True)
rustup=Path.home()/'.cache/hex-lead/rustup-scheduler-copy';tc=rustup/'toolchains/1.99.1-aarch64-apple-darwin/bin'
env={k:v for k,v in os.environ.items() if not k.startswith(('DYLD_','HEX_DEBUG_','SHARDS_DEBUG_','HEX_BORROWED_'))}
env.update(PATH=str(tc)+os.pathsep+env['PATH'],RUSTUP_HOME=str(rustup),RUSTC=str(tc/'rustc'),RUSTDOC=str(tc/'rustdoc'),CARGO_HOME=str(root/'cache/cargo'),CARGO_TARGET_DIR=str(root/'target'),CARGO_BUILD_JOBS='1',RUST_TEST_THREADS='1',RAYON_NUM_THREADS='1',TMPDIR=str(short))
rows=(source/'bench/native/guest/records.tsv').read_text().splitlines();guard=2*max(int(row.split('\t')[2]) for row in rows if row.startswith('validation_run\t'))/1_000_000
r={'source':revision,'transport_sha256':transport_sha,'archive_sha256':source_sha,'seed_lock_sha256':seed_sha,'platform':platform.platform(),'temp':str(short),'guard_seconds':guard,'scope':'ROOT actual borrowed process/socket contract, no VM or device attachment','phases':[]}
def run(name,cmd,required):
 phase={'name':name,'command':cmd};r['phases'].append(phase)
 with (root/(name+'.log')).open('xb') as out:
  child=subprocess.Popen(['nice','-n','10',*cmd],cwd=source,env=env,stdout=out,stderr=subprocess.STDOUT,start_new_session=True);phase['pid']=child.pid
  (root/'run.json').write_text(json.dumps(r,indent=2)+'\n')
  try:phase['exit']=child.wait(timeout=guard)
  except subprocess.TimeoutExpired:
   phase['timed_out']=True;os.killpg(child.pid,signal.SIGKILL);child.wait(timeout=guard);phase['exit']=child.returncode
 phase['reaped']=child.returncode is not None;raw=(root/(name+'.log')).read_bytes();phase.update(sha256=hashlib.sha256(raw).hexdigest(),bytes=len(raw),required=all(x in raw for x in required))
 (root/'run.json').write_text(json.dumps(r,indent=2)+'\n')
 if phase['exit']!=0 or not phase['required'] or phase.get('timed_out'):raise RuntimeError(name+' failed')
 return raw
try:
 raw=run('build',[str(tc/'cargo'),'test','--manifest-path','vendor/shards/Cargo.toml','-p','shards-api','--lib','--test','hex_borrowed_contract','--no-run','--message-format=json'],[])
 lock=(source/'vendor/shards/Cargo.lock').read_bytes();(root/'resolved.lock').write_bytes(lock)
 registry=lambda b:{(p['name'],p['version'],p['checksum']) for p in tomllib.loads(b.decode())['package'] if p.get('source','').startswith('registry+')}
 assert registry(lock)<=registry(seed);r['registry_versions_equal_seed']=True
 bins={}
 for line in raw.splitlines():
  try:item=json.loads(line)
  except(ValueError,UnicodeDecodeError):continue
  if item.get('reason')=='compiler-artifact' and item.get('executable'):bins[item['target']['name']]=item['executable']
 assert set(bins)=={'shards_api','hex_borrowed_contract'},bins
 r['binaries']={k:{'path':v,'sha256':hashlib.sha256(Path(v).read_bytes()).hexdigest()} for k,v in bins.items()}
 run('canonical-borrowed',[bins['shards_api'],'client::borrowed_tests::','--nocapture','--test-threads=1'],[b'9 passed; 0 failed; 0 ignored;'])
 for name in ['zero','max']:
  run('owned-'+name,[bins['shards_api'],'--exact','client::tests::a_patience_of_'+name+('_dials_once' if name=='zero' else '_dials_until_the_guest_listens'),'--nocapture','--test-threads=1'],[b'1 passed; 0 failed; 0 ignored;'])
 run('public-lifetime',[bins['hex_borrowed_contract'],'--ignored','--exact','root_borrowed_drop_preserves_target_and_socket_and_dead_target_refuses','--nocapture','--test-threads=1'],[b'1 passed; 0 failed; 0 ignored;',b'borrowed_lifetime live_after_both_drops=true'])
 r['exit']=0
except Exception as e:r.update(exit=1,error=str(e))
finally:
 r['source_unchanged']=hashes()==before
 parent=short/'shards-tests'
 if parent.is_dir() and not list(parent.iterdir()):parent.rmdir()
 r['temp_entries']=len(list(short.iterdir()))
 if not r['source_unchanged'] or r['temp_entries']:r['exit']=1
 (root/'result.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r,indent=2),flush=True)
 if r['exit'] and r['phases']:print((root/(r['phases'][-1]['name']+'.log')).read_text(errors='replace')[-7000:],flush=True)
sys.exit(r['exit'])
