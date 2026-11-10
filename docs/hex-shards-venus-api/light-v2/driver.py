import hashlib,json,os,pathlib,platform,subprocess,tomllib
root=pathlib.Path.home()/'.cache/hex-shards-venus-api-01a1c640-3f2e9fb1-v2'
source=root/'source';proof=root/'proof';target=root/'target';cargo_home=root/'cargo'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
manifest=json.loads((proof/'source.json').read_text())
for path,digest in manifest['files'].items():assert sha(source/path)==digest,path
assert not target.exists() and not cargo_home.exists();target.mkdir();cargo_home.mkdir()
copied=subprocess.run(['cp','-cR',str(pathlib.Path.home()/'.cargo/registry'),str(cargo_home/'registry')],capture_output=True,text=True)
(proof/'registry-copy.stdout').write_text(copied.stdout);(proof/'registry-copy.stderr').write_text(copied.stderr);assert copied.returncode==0
# The vendored four-crate scaffold has no checked-in lock. Seed ONLY this private workspace
# with exact upstream33df's lock; permit pruning, then verify every registry version/checksum.
lock=source/'vendor/shards/Cargo.lock';assert not lock.exists()
lock.write_bytes((proof/'upstream-Cargo.lock').read_bytes())
toolchain=pathlib.Path.home()/'.cache/hex-lead/rustup-scheduler-copy/toolchains/1.99.1-aarch64-apple-darwin/bin'
env=os.environ.copy()
for key in list(env):
    if key.startswith(('DYLD_','HEX_DEBUG_','HEX_WAITS_CENSUS')):env.pop(key,None)
env.update(CARGO_HOME=str(cargo_home),CARGO_TARGET_DIR=str(target),CARGO_BUILD_JOBS='1',RUST_TEST_THREADS='1',RUSTC=str(toolchain/'rustc'),RUSTDOC=str(toolchain/'rustdoc'),RUSTUP_HOME=str(toolchain.parent.parent),RUSTUP_TOOLCHAIN='1.99.1')
env['PATH']=str(toolchain)+':'+env.get('PATH','')
(proof/'environment.json').write_text(json.dumps({'platform':platform.platform(),'arch':platform.machine(),'needs':os.environ.get('KILN_NEEDS'),'nice':os.getpriority(os.PRIO_PROCESS,0),'jobs':1,'threads':1,'versions':{key:subprocess.check_output([str(toolchain/key),'--version'],env=env,text=True).strip() for key in ['cargo','rustc']}},indent=2)+'\n')
command=[str(toolchain/'cargo'),'test','--manifest-path','vendor/shards/Cargo.toml','-p','shards-api','--lib','--tests','--no-run','--message-format=json']
(proof/'build-command.json').write_text(json.dumps(command)+'\n')
with (proof/'build.jsonl').open('w') as out,(proof/'build.stderr').open('w') as err:
    child=subprocess.Popen(command,cwd=source,env=env,stdout=out,stderr=err)
    (proof/'build.pid').write_text(str(child.pid)+'\n')
    try:child.wait(timeout=900)
    except subprocess.TimeoutExpired:child.kill();child.wait();raise
(proof/'build.exit').write_text(str(child.returncode)+'\n')
if child.returncode:
    errors=[row['message'].get('rendered','') for line in (proof/'build.jsonl').read_text().splitlines() if (row:=json.loads(line)).get('reason')=='compiler-message' and row.get('message',{}).get('level')=='error']
    print(json.dumps({'build_exit':child.returncode,'errors':errors[:4],'stderr_tail':(proof/'build.stderr').read_text()[-4000:]}),flush=True)
    raise SystemExit(child.returncode)
seed=tomllib.loads((proof/'upstream-Cargo.lock').read_text())
locked=tomllib.loads(lock.read_text())
packages={(p['name'],p['version'],p.get('source')):p for p in seed['package']}
for package in locked['package']:
    if 'source' in package:
        old=packages[(package['name'],package['version'],package['source'])]
        assert old.get('checksum')==package.get('checksum'),package['name']
(proof/'private-Cargo.lock').write_bytes(lock.read_bytes())
(proof/'lock-receipt.json').write_text(json.dumps({'seed_sha256':sha(proof/'upstream-Cargo.lock'),'private_sha256':sha(lock),'all_registry_versions_and_checksums_match_exact_upstream_seed':True,'private_only':True},indent=2)+'\n')
artifacts={}
for line in (proof/'build.jsonl').read_text().splitlines():
    row=json.loads(line)
    if row.get('reason')=='compiler-artifact' and row.get('executable'):
        name=row['target']['name']
        if name in ['shards_api','sim','real']:artifacts[name]=pathlib.Path(row['executable'])
assert set(artifacts)=={'shards_api','sim','real'},artifacts
selected=[('shards_api','client::tests::a_spec_is_the_flags_shards_run_reads'),('shards_api','client::tests::shared_venus_is_enabled_only_when_requested'),('shards_api','tests::bounds_are_refused_by_name'),('sim','the_simulator_retains_shared_gpu_enable')]
for name,binary in artifacts.items():
    if name=='real':continue
    listing=subprocess.check_output([str(binary),'--list'],cwd=source,env=env,text=True)
    (proof/(name+'-tests.list')).write_text(listing)
    for target,test in selected:
        if target==name:assert listing.splitlines().count(test+': test')==1,test
print(json.dumps({'built':True,'selected':len(selected),'real_harness':'compiled only; executable never invoked'}),flush=True)
results=[]
for number,(target_name,test) in enumerate(selected):
    command=[str(artifacts[target_name]),'--exact',test,'--nocapture','--test-threads=1'];path=proof/('test-'+str(number)+'.log')
    with path.open('w') as log:
        child=subprocess.Popen(command,cwd=source,env=env,stdout=log,stderr=subprocess.STDOUT)
        (proof/('test-'+str(number)+'.pid')).write_text(str(child.pid)+'\n')
        try:child.wait(timeout=120)
        except subprocess.TimeoutExpired:child.kill();child.wait();raise
    raw=path.read_bytes();passed=child.returncode==0 and b'running 1 test' in raw and b'test result: ok. 1 passed; 0 failed; 0 ignored;' in raw
    results.append({'name':test,'target':target_name,'command':command,'pid':child.pid,'reaped':True,'exit':child.returncode,'passed':passed,'log':path.name,'log_sha256':sha(path)})
    print(json.dumps({'test':test,'exit':child.returncode,'passed':passed,'log_tail':raw.decode(errors='replace')[-1800:]}),flush=True)
after={path:sha(source/path) for path in manifest['files']}
assert after==manifest['files']
extra=set(str(p.relative_to(source)) for p in source.rglob('*') if p.is_file())-set(after)
assert extra=={'vendor/shards/Cargo.lock'},extra
summary={'source':manifest['commit'],'production_pin':manifest['production_pin'],'build_exit':0,'binaries':{name:{'path':str(binary),'sha256':sha(binary)} for name,binary in artifacts.items()},'selected':len(results),'passes':sum(row['passed'] for row in results),'failed':sum(not row['passed'] for row in results),'ignored':0,'tests':results,'source_after':after,'only_private_generated_source_file':'vendor/shards/Cargo.lock','scope':'Pure CLI argument/bounds checks plus one SimGuests spec-retention model test. No physical GPU/VM/native start/attach/conformance/effects or performance claim.'}
(proof/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({key:value for key,value in summary.items() if key not in ['source_after','tests']}),flush=True)
raise SystemExit(0 if summary['passes']==4 and summary['failed']==0 else 1)
