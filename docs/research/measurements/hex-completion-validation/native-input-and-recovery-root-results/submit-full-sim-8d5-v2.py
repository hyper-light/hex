from pathlib import Path
import hashlib,json,os,re,shlex,subprocess,sys
b=Path('/Users/adalundhe/.cache/hex-lead/scheduler-completion-20261110');local=b/'full-sim-8d5-v2';local.mkdir(exist_ok=False)
remote=r"""from pathlib import Path
import hashlib,json,os,re,subprocess,time
root=Path.home()/'.cache/hex-lead/full-sim-8d5-v2';root.mkdir(exist_ok=False);(root/'temp').mkdir()
source=Path.home()/'.cache/hex-lead/cohort-8d5bb0b4-linux-remote-pending-after/source'
binary=source.parent/'target/debug/deps/hex_sim-1a26fbe8e182b00f'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
expected='79200981438237179e4e808d6b52bdbe0742489b22526e0db6e49b7532d5f0fe'
if binary.stat().st_size!=93763432 or sha(binary)!=expected:raise ValueError('exact compiled8d5 ELF required')
if sha(source/'crates/hex-sim/src/index/routes.rs')!='cda46b5fee827aaab61a9e2fcf7f1e9b7085c596017cbd7de3d3d2ebabb399fc':raise ValueError('source mismatch')
cg=Path('/sys/fs/cgroup')/Path('/proc/self/cgroup').read_text().strip().split('0::',1)[1].lstrip('/')
limits={n:(cg/n).read_text().strip() for n in ['cpu.max','memory.max','memory.swap.max','pids.max']}
if limits!={'cpu.max':'100000 100000','memory.max':'4294967296','memory.swap.max':'0','pids.max':'256'}:raise ValueError('hardprivatebounds required')
env={k:v for k,v in os.environ.items() if not k.startswith(('HEX_DEBUG_','SHARDS_DEBUG_'))};env.update(TMPDIR=str(root/'temp'),RUST_TEST_THREADS='1',RAYON_NUM_THREADS='1')
cmd=['nice','-n','10',str(binary),'--nocapture','--test-threads=1'];result={'source':'8d5bb0b4eb6a175532d73d889dc472b721660b41','binary_sha256':expected,'binary_bytes':93763432,'driver_pid':os.getpid(),'limits':limits,'command':cmd,'vm_gpu':False,'fresh_build':False}
with (root/'full-sim.log').open('xb') as out:
 child=subprocess.Popen(cmd,cwd=source,env=env,stdout=out,stderr=subprocess.STDOUT);result['pid']=child.pid;(root/'running.json').write_text(json.dumps(result)+'\n')
 try:result['exit']=child.wait(timeout=900)
 except subprocess.TimeoutExpired:
  result['timed_out']=True;child.terminate()
  try:child.wait(timeout=10)
  except subprocess.TimeoutExpired:child.kill();child.wait()
  result['exit']=child.returncode
 result['reaped']=child.returncode is not None
raw=(root/'full-sim.log').read_bytes();counts=re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',raw.decode(errors='replace'))
result.update(log_bytes=len(raw),log_sha256=hashlib.sha256(raw).hexdigest(),counts=counts,binary_unchanged=sha(binary)==expected,temp_entries=len(list((root/'temp').iterdir())))
result['verified']=result['exit']==0 and counts==[('198','0','0')] and result['reaped'] and result['binary_unchanged'] and not result.get('timed_out')
(root/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2),flush=True)
if not result['verified']:print(raw.decode(errors='replace')[-6000:],flush=True)
raise SystemExit(0 if result['verified'] else 1)
"""
cmd=['/usr/bin/timeout','--signal=TERM','--kill-after=10s','980s','/usr/bin/systemd-run','--user','--scope','--collect','--quiet','--unit=hex-root-full-sim-8d5-v2','--property=CPUQuota=100%','--property=MemoryMax=4G','--property=MemorySwapMax=0','--property=TasksMax=256','--property=RuntimeMaxSec=1000','/usr/bin/nice','-n','10','/usr/bin/python3','-B','-c',remote]
argv=[str(Path.home()/'.local/bin/kiln'),'run','--on','00aad8daff2fa3c9daa65d53c425d450','--needs','linux/amd64,cpus=1,mem=4g,disk=12g','--no-sync','--quiet','--',shlex.join(cmd)]
with (local/'cli.log').open('xb') as out:
 child=subprocess.Popen(argv,stdout=out,stderr=subprocess.STDOUT);(local/'submission.json').write_text(json.dumps({'pid':child.pid,'argv':argv,'driver_sha256':hashlib.sha256(remote.encode()).hexdigest()},indent=2)+'\n');print(json.dumps({'pid':child.pid}),flush=True);code=child.wait()
(local/'cli.exit').write_text(str(code)+'\n');print((local/'cli.log').read_text()[-5000:],flush=True);raise SystemExit(code)
