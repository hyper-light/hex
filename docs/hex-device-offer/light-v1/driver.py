import hashlib,json,os,pathlib,platform,subprocess
root=pathlib.Path.home()/'.cache/hex-device-offer-01a1c640-c5e806ed-v1'
source=root/'source';proof=root/'proof';target=root/'target';cargo_home=root/'cargo'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
manifest=json.loads((proof/'source.json').read_text())
for path,digest in manifest['files'].items():assert sha(source/path)==digest,(path,sha(source/path),digest)
parent=source/'crates/hex-node/src/scheduler/tests.rs';before=parent.read_bytes()
registration=b'\n#[path = "device_offer_tests.rs"]\nmod device_offer_tests;\n'
assert b'mod device_offer_tests;' not in before
parent.write_bytes(before+registration)
(proof/'parent-before.rs').write_bytes(before);(proof/'parent-private.rs').write_bytes(parent.read_bytes())
(proof/'private-registration.json').write_text(json.dumps({'parent':'crates/hex-node/src/scheduler/tests.rs','before_sha256':hashlib.sha256(before).hexdigest(),'after_sha256':sha(parent),'only_appended':registration.decode(),'scope':'PRIVATE copy only; actual worktree parent unchanged'},indent=2)+'\n')
assert not target.exists() and not cargo_home.exists();target.mkdir();cargo_home.mkdir()
copied=subprocess.run(['cp','-cR',str(pathlib.Path.home()/'.cargo/registry'),str(cargo_home/'registry')],capture_output=True,text=True)
(proof/'registry-copy.stdout').write_text(copied.stdout);(proof/'registry-copy.stderr').write_text(copied.stderr);assert copied.returncode==0
toolchain=pathlib.Path.home()/'.cache/hex-lead/rustup-scheduler-copy/toolchains/1.99.1-aarch64-apple-darwin/bin'
env=os.environ.copy()
for key in list(env):
    if key.startswith(('DYLD_','HEX_DEBUG_','HEX_WAITS_CENSUS')):env.pop(key,None)
env.update(CARGO_HOME=str(cargo_home),CARGO_TARGET_DIR=str(target),CARGO_BUILD_JOBS='1',RUST_TEST_THREADS='1',RUSTC=str(toolchain/'rustc'),RUSTDOC=str(toolchain/'rustdoc'),RUSTUP_HOME=str(toolchain.parent.parent),RUSTUP_TOOLCHAIN='1.99.1')
env['PATH']=str(toolchain)+':'+env.get('PATH','')
cargo=toolchain/'cargo';assert cargo.is_file()
(proof/'environment.json').write_text(json.dumps({'platform':platform.platform(),'arch':platform.machine(),'needs':os.environ.get('KILN_NEEDS'),'nice':os.getpriority(os.PRIO_PROCESS,0),'jobs':1,'threads':1,'versions':{key:subprocess.check_output([str(toolchain/key),'--version'],env=env,text=True).strip() for key in ['cargo','rustc']}},indent=2)+'\n')
def run(label,command,jsonout=False):
    (proof/(label+'-command.json')).write_text(json.dumps(command)+'\n')
    with (proof/(label+('.jsonl' if jsonout else '.log'))).open('w') as out,(proof/(label+'.stderr')).open('w') as err:
        child=subprocess.Popen(command,cwd=source,env=env,stdout=out,stderr=err)
        (proof/(label+'.pid')).write_text(str(child.pid)+'\n')
        try:child.wait(timeout=900)
        except subprocess.TimeoutExpired:
            child.kill();child.wait();raise
    (proof/(label+'.exit')).write_text(str(child.returncode)+'\n')
    return child.returncode
build=run('build',[str(cargo),'test','-p','hex-node','--lib','--locked','--offline','--no-run','--message-format=json'],True)
if build:
    errors=[]
    for line in (proof/'build.jsonl').read_text().splitlines():
        row=json.loads(line)
        if row.get('reason')=='compiler-message' and row.get('message',{}).get('level')=='error':errors.append(row['message'].get('rendered',''))
    (proof/'build-failure.json').write_text(json.dumps({'source':manifest['commit'],'production_pin':manifest['production_pin'],'build_exit':build,'errors':errors},indent=2)+'\n')
    print(json.dumps({'build_exit':build,'source':manifest['commit'],'errors':errors[:4],'stderr_tail':(proof/'build.stderr').read_text()[-4000:]}),flush=True)
    raise SystemExit(build)
outputs=[row['executable'] for line in (proof/'build.jsonl').read_text().splitlines() if (row:=json.loads(line)).get('reason')=='compiler-artifact' and row.get('target',{}).get('name')=='hex_node' and row.get('executable')]
assert len(outputs)==1,outputs
binary=pathlib.Path(outputs[0]);listing=subprocess.check_output([str(binary),'--list'],cwd=source,env=env,text=True)
(proof/'tests.list').write_text(listing)
names=[line.removesuffix(': test') for line in listing.splitlines() if line.startswith('scheduler::tests::device_offer_tests::') and line.endswith(': test')]
assert len(names)==6,names
names.sort(key=lambda name: (not name.endswith("late_refusal_does_not_publish_partial_model_interning"), name))
print(json.dumps({'built':True,'selected':len(names),'binary_sha256':sha(binary)}),flush=True)
rows=[]
for number,name in enumerate(names):
    command=[str(binary),'--exact',name,'--nocapture','--test-threads=1'];path=proof/('test-'+str(number)+'.log')
    with path.open('w') as log:
        child=subprocess.Popen(command,cwd=source,env=env,stdout=log,stderr=subprocess.STDOUT)
        (proof/('test-'+str(number)+'.pid')).write_text(str(child.pid)+'\n')
        try:child.wait(timeout=120)
        except subprocess.TimeoutExpired:
            child.kill();child.wait();raise
    raw=path.read_bytes();passed=child.returncode==0 and b'running 1 test' in raw and b'test result: ok. 1 passed;' in raw
    row={'name':name,'command':command,'exit_code':child.returncode,'pid':child.pid,'reaped':True,'ran_one':b'running 1 test' in raw,'passed':passed,'log':path.name,'log_sha256':hashlib.sha256(raw).hexdigest()}
    rows.append(row)
    print(json.dumps({'test':name,'exit':child.returncode,'passed':passed,'log_tail':raw.decode(errors='replace')[-1400:]}),flush=True)
checks={}
for label,flags in [('clippy-lib',['--lib']),('clippy-tests',['--lib','--tests'])]:
    checks[label]=run(label,[str(cargo),'clippy','-p','hex-node',*flags,'--locked','--offline','--no-deps','--','-D','warnings'])
    if checks[label]:print(json.dumps({'check':label,'exit':checks[label],'stderr_tail':(proof/(label+'.stderr')).read_text()[-6500:]}),flush=True)
checks['rustfmt']=run('rustfmt',[str(toolchain/'rustfmt'),'--edition','2024','--check',str(source/'crates/hex-node/src/scheduler/device_offer_tests.rs')])
for path,digest in manifest['files'].items():
    if path!='crates/hex-node/src/scheduler/tests.rs':assert sha(source/path)==digest
assert parent.read_bytes()==before+registration
summary={'source':manifest['commit'],'production_pin':manifest['production_pin'],'build_exit':0,'binary':str(binary),'binary_sha256':sha(binary),'selected':len(rows),'passes':sum(r['passed'] for r in rows),'failed':sum(not r['passed'] for r in rows),'ignored':0,'tests':rows,'checks':checks,'scope':'Actual Service capacity::devices::offers with pure synthetic Facts/Capacity inputs and production physical_id/binder. No Service command/driver/native/hardware/VM/socket/SSH/hook execution; late-refusal expectation remains a candidate.'}
(proof/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({k:v for k,v in summary.items() if k!='tests'}),flush=True)
raise SystemExit(0 if summary['passes']==6 and summary['failed']==0 and all(v==0 for v in checks.values()) else 1)
