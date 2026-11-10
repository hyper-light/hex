import hashlib,json,os,pathlib,platform,subprocess,time
root=pathlib.Path.home()/'.cache/hex-adversarial-relay-01a1c640-dc29-v1'
source=root/'source'; proof=root/'proof'; cargo_home=root/'cargo'; target=root/'target'
assert not cargo_home.exists() and not target.exists()
cargo_home.mkdir(); target.mkdir()
copy=subprocess.run(['cp','-cR',str(pathlib.Path.home()/'.cargo/registry'),str(cargo_home/'registry')],capture_output=True,text=True)
(proof/'cache-copy.stdout').write_text(copy.stdout); (proof/'cache-copy.stderr').write_text(copy.stderr)
assert copy.returncode==0
toolchain=pathlib.Path.home()/'.cache/hex-lead/rustup-scheduler-copy/toolchains/1.99.1-aarch64-apple-darwin/bin'
cargo=toolchain/'cargo'; rustc=toolchain/'rustc'
assert cargo.is_file() and rustc.is_file(),str(toolchain)
env=os.environ.copy()
for key in list(env):
    if key.startswith('DYLD_') or key.startswith('HEX_DEBUG') or key.startswith('HEX_CENSUS'):
        env.pop(key,None)
env.update(CARGO_HOME=str(cargo_home),CARGO_TARGET_DIR=str(target),CARGO_BUILD_JOBS='1',RUST_TEST_THREADS='1',RUSTC=str(rustc),RUSTDOC=str(toolchain/'rustdoc'),RUSTUP_HOME=str(toolchain.parent.parent),RUSTUP_TOOLCHAIN='1.99.1')
env['PATH']=str(toolchain)+':'+env.get('PATH','')
versions={name:subprocess.check_output([str(toolchain/name),'--version'],cwd=source,env=env,text=True).strip() for name in ['cargo','rustc']}
(proof/'environment.json').write_text(json.dumps({'platform':platform.platform(),'machine':platform.machine(),'versions':versions,'needs':os.environ.get('KILN_NEEDS'),'nice':os.getpriority(os.PRIO_PROCESS,0),'jobs':1,'threads':1},indent=2)+'\n')
cmd=[str(cargo),'test','-p','hex-node','--lib','--locked','--offline','--no-run','--message-format=json']
(proof/'build-command.json').write_text(json.dumps(cmd)+'\n')
with (proof/'build.jsonl').open('w') as out,(proof/'build.stderr').open('w') as err:
    result=subprocess.run(cmd,cwd=source,env=env,stdout=out,stderr=err)
(proof/'build.exit').write_text(str(result.returncode)+'\n')
if result.returncode:
    print(json.dumps({'build_exit':result.returncode,'stderr_tail':(proof/'build.stderr').read_text()[-6000:]}))
    raise SystemExit(result.returncode)
artifacts=[]
for line in (proof/'build.jsonl').read_text().splitlines():
    row=json.loads(line)
    if row.get('reason')=='compiler-artifact' and row.get('target',{}).get('name')=='hex_node' and row.get('executable'):
        artifacts.append(row['executable'])
assert len(artifacts)==1,artifacts
binary=pathlib.Path(artifacts[0]); binary_sha=hashlib.sha256(binary.read_bytes()).hexdigest()
listed=subprocess.check_output([str(binary),'--list'],cwd=source,env=env,text=True)
(proof/'tests.list').write_text(listed)
names=[line.removesuffix(': test') for line in listed.splitlines() if line.startswith('scheduler::tests::adversarial_relay_tests::') and line.endswith(': test')]
assert len(names)==2,names
rows=[]
for number,name in enumerate(names):
    command=[str(binary),'--exact',name,'--nocapture','--test-threads=1']
    with (proof/('test-'+str(number)+'.log')).open('w') as log:
        result=subprocess.run(command,cwd=source,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=120)
    raw=(proof/('test-'+str(number)+'.log')).read_bytes()
    rows.append({'name':name,'command':command,'exit_code':result.returncode,'log':'test-'+str(number)+'.log','log_sha256':hashlib.sha256(raw).hexdigest(),'log_bytes':len(raw)})
summary={'build_exit':0,'binary':str(binary),'binary_sha256':binary_sha,'tests':rows,'selected':len(names),'passes':sum(row['exit_code']==0 for row in rows),'nonzero':sum(row['exit_code']!=0 for row in rows),'scope':'actual native macOS test binary; Service/Order/Driver with SimRuntime effects; no native runtime box, SSH or VM','candidate_note':'wrong-owner unchanged-order/same-stamp correction expectation is a spec candidate, not a production defect; ROOT noted commit-before-effects transport fencing contract'}
(proof/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps(summary))
