from pathlib import Path
import hashlib,json,os,platform,re,subprocess
base=Path.home()/'.cache/hex-lead/lanes/device-recovery-max-fixed-01a1c640-9628f7da-v4'
source=base/'source'; proof=base/'proof'
assert not (proof/'light-result.json').exists()
env=os.environ.copy()
for name in list(env):
 if name.startswith('DYLD_') or name.startswith('HEX_DEBUG_') or name=='HEX_SKIPS': env.pop(name,None)
env.update(CARGO_HOME=str(base/'cargo-home'),CARGO_TARGET_DIR=str(base/'target'),CARGO_BUILD_JOBS='1',RUST_TEST_THREADS='1',RUSTUP_HOME=str(Path.home()/'.cache/hex-lead/rustup-scheduler-copy'),TMPDIR=str(base/'tmp'))
def digest(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def hashes(): return {str(p.relative_to(source)):digest(p) for p in sorted(source.rglob('*')) if p.is_file()}
before=hashes(); (proof/'source-hashes-before.json').write_text(json.dumps(before,indent=2)+'\n')
receipt=json.loads((proof/'input-receipt.json').read_text())
cargo=str(Path.home()/'.cargo/bin/cargo'); rustc=str(Path.home()/'.cargo/bin/rustc')
result={'source_revision':receipt['source_revision'],'fixed_base':receipt['fixed_base'],'runner':'d1323898b255a3ed435eb83fa0f7b0cf','platform':platform.platform(),'architecture':platform.machine(),'driver_pid':os.getpid(),'nice':subprocess.check_output(['ps','-p',str(os.getpid()),'-o','nice='],text=True).strip(),'environment':{k:env[k] for k in ['CARGO_HOME','CARGO_TARGET_DIR','CARGO_BUILD_JOBS','RUST_TEST_THREADS','RUSTUP_HOME','TMPDIR']},'native_gpu_effect':False,'phases':[]}
for binary in [cargo,rustc]: result[Path(binary).name]=subprocess.check_output([binary,'--version'],cwd=source,env=env,text=True).strip()
phases=[
 ('max-index',[cargo,'test','-p','hex-sim','--lib','--locked','peer::adversarial_device_recovery_tests::last_legal_device_index_roundtrips_and_releases_without_iterator_overflow','--','--exact','--test-threads=1','--nocapture'],1),
]
exit_code=0
for name,args,expected in phases:
 log=proof/(name+'.log')
 with log.open('wb') as out:
  proc=subprocess.Popen(args,cwd=source,env=env,stdout=out,stderr=subprocess.STDOUT)
  identity={'pid':proc.pid,'start':subprocess.check_output(['ps','-p',str(proc.pid),'-o','lstart='],text=True).strip(),'argv':args,'driver_pid':os.getpid()}
  (proof/(name+'-owned-process.json')).write_text(json.dumps(identity,indent=2)+'\n')
  print(json.dumps({'phase':name,'started':True,'owned':identity}),flush=True)
  code=proc.wait()
 text=log.read_text(errors='replace')
 parsed=re.search(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;',text)
 counts=dict(zip(['passed','failed','ignored','measured','filtered_out'],map(int,parsed.groups()))) if parsed else None
 phase={'name':name,'exit_code':code,'counts':counts,'expected_selected_count':expected,'log':str(log),'log_bytes':log.stat().st_size,'log_sha256':digest(log),'child_reaped':True,'owned':identity,'witnesses':[line for line in text.splitlines() if any(word in line for word in ['device-recovery PURE','retained-transient','binding-put','binding-refusal','binding-overlap','legacy-cpu','physical-reindex','physical-refusal','reserve-refusal','reserve-overlap','last-index'])]}
 result['phases'].append(phase)
 print(json.dumps(phase),flush=True)
 (proof/'phases.json').write_text(json.dumps(result['phases'],indent=2)+'\n')
 if code or (name!='lib-clippy' and (counts is None or counts['passed']+counts['failed']==0)) or (expected is not None and counts['passed']+counts['failed']!=expected):
  exit_code=code or 2
  print(text[-9000:],flush=True)
  break
after=hashes(); (proof/'source-hashes-after.json').write_text(json.dumps(after,indent=2)+'\n')
result.update(exit_code=exit_code,source_unchanged=before==after,child_reaped=all(p['child_reaped'] for p in result['phases']),source_sha256=receipt['source_sha256'],parent_registered_sha256=receipt['parent_registered_sha256'],private_compatibility=receipt['private_compatibility'])
result['binaries']=[{'path':str(p),'sha256':digest(p),'bytes':p.stat().st_size} for p in sorted((base/'target/debug/deps').glob('hex_sim-*')) if p.is_file() and os.access(p,os.X_OK) and p.suffix=='']
if before!=after: result['exit_code']=exit_code=2
(proof/'light-result.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:v for k,v in result.items() if k not in ['phases','environment']}),flush=True)
raise SystemExit(exit_code)
