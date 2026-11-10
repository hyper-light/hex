nice -n 10 python3 - <<'PY'
from pathlib import Path
import hashlib,json,os,platform,re,subprocess
base=Path.home()/'.cache/hex-lead/lanes/device-recovery-01a1c640-44a2df3b-v1'
source=base/'source'
proof=base/'proof'
assert not (proof/'light-result.json').exists()
env=os.environ.copy()
for name in list(env):
 if name.startswith('DYLD_') or name.startswith('HEX_DEBUG_') or name=='HEX_SKIPS':
  env.pop(name,None)
env.update(CARGO_HOME=str(base/'cargo-home'),CARGO_TARGET_DIR=str(base/'target'),CARGO_BUILD_JOBS='1',RUST_TEST_THREADS='1',RUSTUP_HOME=str(Path.home()/'.cache/hex-lead/rustup-scheduler-copy'),TMPDIR=str(base/'tmp'))
def digest(path):
 return hashlib.sha256(path.read_bytes()).hexdigest()
before={str(p.relative_to(source)):digest(p) for p in sorted(source.rglob('*')) if p.is_file()}
(proof/'source-hashes-before.json').write_text(json.dumps(before,indent=2)+'\n')
receipt=json.loads((proof/'input-receipt.json').read_text())
cargo=str(Path.home()/'.cargo/bin/cargo')
rustc=str(Path.home()/'.cargo/bin/rustc')
result={'source_revision':receipt['source_revision'],'fixed_base':receipt['fixed_base'],'runner':'d1323898b255a3ed435eb83fa0f7b0cf','platform':platform.platform(),'machine':platform.machine(),'driver_pid':os.getpid(),'nice':subprocess.check_output(['ps','-p',str(os.getpid()),'-o','nice='],text=True).strip(),'environment':{k:env[k] for k in ['CARGO_HOME','CARGO_TARGET_DIR','CARGO_BUILD_JOBS','RUST_TEST_THREADS','RUSTUP_HOME','TMPDIR']},'selected_test':receipt['selected_test'],'expected_selected_count':1,'native_gpu_effect':False}
for binary in [cargo,rustc]:
 result[Path(binary).name]=subprocess.check_output([binary,'--version'],cwd=source,env=env,text=True).strip()
args=[cargo,'test','-p','hex-sim','--lib','--locked',receipt['selected_test'],'--','--exact','--test-threads=1','--nocapture']
result['argv']=args
(proof/'driver.json').write_text(json.dumps(result,indent=2)+'\n')
log=proof/'selected-device-recovery.log'
with log.open('wb') as out:
 proc=subprocess.Popen(args,cwd=source,env=env,stdout=out,stderr=subprocess.STDOUT)
 identity={'pid':proc.pid,'start':subprocess.check_output(['ps','-p',str(proc.pid),'-o','lstart='],text=True).strip(),'argv':args,'driver_pid':os.getpid()}
 (proof/'owned-process.json').write_text(json.dumps(identity,indent=2)+'\n')
 print(json.dumps({'started':True,'owned':identity}),flush=True)
 code=proc.wait()
logtext=log.read_text(errors='replace')
parsed=re.search(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;',logtext)
counts=dict(zip(['passed','failed','ignored','measured','filtered_out'],map(int,parsed.groups()))) if parsed else None
after={str(p.relative_to(source)):digest(p) for p in sorted(source.rglob('*')) if p.is_file()}
(proof/'source-hashes-after.json').write_text(json.dumps(after,indent=2)+'\n')
result.update(exit_code=code,counts=counts,log=str(log),log_bytes=log.stat().st_size,log_sha256=digest(log),source_unchanged=before==after,child_reaped=True,owned=identity,witnesses=[line for line in logtext.splitlines() if 'device-recovery PURE' in line])
result['binaries']=[{'path':str(p),'sha256':digest(p),'bytes':p.stat().st_size} for p in sorted((base/'target/debug/deps').glob('hex_sim-*')) if p.is_file() and os.access(p,os.X_OK) and p.suffix=='']
(proof/'selected-device-recovery.exit').write_text(str(code)+'\n')
(proof/'light-result.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result),flush=True)
if code!=0: print(logtext[-9000:],flush=True)
raise SystemExit(code)
PY
