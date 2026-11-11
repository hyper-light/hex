import hashlib,io,json,os,platform,re,subprocess,sys,tarfile
from pathlib import Path,PurePosixPath
root=Path(sys.argv[1]);expected,revision=sys.argv[2:4]
root.mkdir(parents=True,exist_ok=False)
data=sys.stdin.buffer.read()
if hashlib.sha256(data).hexdigest()!=expected:raise ValueError('source archive mismatch')
(root/'source.tar.gz').write_bytes(data)
source=root/'source';source.mkdir()
with tarfile.open(fileobj=io.BytesIO(data),mode='r:gz') as bundle:
 for member in bundle.getmembers():
  p=PurePosixPath(member.name)
  if p.is_absolute() or '..' in p.parts or not(member.isfile() or member.isdir()):raise ValueError('nonregular archive member')
 bundle.extractall(source)
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
manifest={str(p.relative_to(source)):sha(p) for p in sorted(source.rglob('*')) if p.is_file()}
(root/'source-hashes.json').write_text(json.dumps(manifest,indent=2)+'\n')
for name in ['target','cache/cargo','temp']:(root/name).mkdir(parents=True)
rustup=Path.home()/'.cache/hex-lead/rustup-scheduler-copy';toolchain=rustup/'toolchains/1.99.1-aarch64-apple-darwin/bin'
env={k:v for k,v in os.environ.items() if not k.startswith(('DYLD_','HEX_DEBUG_','HEX_WAITS_CENSUS','SHARDS_DEBUG_'))}
env.update(PATH=str(toolchain)+os.pathsep+env['PATH'],RUSTUP_HOME=str(rustup),RUSTC=str(toolchain/'rustc'),RUSTDOC=str(toolchain/'rustdoc'),CARGO_HOME=str(root/'cache/cargo'),CARGO_TARGET_DIR=str(root/'target'),CARGO_BUILD_JOBS='1',RUST_TEST_THREADS='1',RAYON_NUM_THREADS='1',TMPDIR=str(root/'temp'))
result={'source':revision,'archive_sha256':expected,'platform':platform.platform(),'arch':platform.machine(),'scope':'ROOT combined Windows completion/core markers with current API/native GPU model: both Windows targets CLI/runtime all-target Clippy including combined ignored native fixtures, no test execution or native HCS/GPU attachment effect','phases':[]}

for name,args in [('cli-runtime-winboth-clippy',['clippy','-p','hex-cli','-p','hex-runtime','--all-targets','--locked','--target','x86_64-pc-windows-msvc','--target','aarch64-pc-windows-msvc','--','-D','warnings'])]:
 command=['nice','-n','10',str(toolchain/'cargo')]+args;phase={'name':name,'command':command};result['phases'].append(phase)
 with (root/(name+'.log')).open('xb') as log:
  child=subprocess.Popen(command,cwd=source,env=env,stdout=log,stderr=subprocess.STDOUT);phase['pid']=child.pid
  (root/'run.json').write_text(json.dumps(result,indent=2)+'\n')
  try:phase['exit']=child.wait(timeout=2*max(int(row.split('\t')[2]) for row in (source/'bench/native/guest/records.tsv').read_text().splitlines() if row.startswith('validation_run\t'))/1_000_000)
  except subprocess.TimeoutExpired:
   child.terminate()
   try:child.wait(timeout=10)
   except subprocess.TimeoutExpired:child.kill();child.wait()
   phase['exit']=child.returncode;phase['timed_out']=True
 phase['reaped']=child.returncode is not None
 raw=(root/(name+'.log')).read_bytes();phase['sha256']=hashlib.sha256(raw).hexdigest();phase['bytes']=len(raw)
 if name=='hcs-core':phase['required_selected']=b'51 passed; 0 failed; 0 ignored;' in raw and b'panicked at' not in raw
 if name=='device-offer':phase['required_selected']=b'6 passed; 0 failed; 0 ignored;' in raw and b'panicked at' not in raw
 (root/'run.json').write_text(json.dumps(result,indent=2)+'\n')
 if phase['exit']!=0 or phase.get('timed_out'):break
result['exit']=0 if len(result['phases'])==1 and all(p['exit']==0 and p.get('required_selected',True) and not p.get('timed_out') for p in result['phases']) else 1
result['temp_entries']=len(list((root/'temp').iterdir()))
(root/'result.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2),flush=True)
if result['exit']:print((root/(result['phases'][-1]['name']+'.log')).read_text(errors='replace')[-14000:],flush=True)
sys.exit(result['exit'])
