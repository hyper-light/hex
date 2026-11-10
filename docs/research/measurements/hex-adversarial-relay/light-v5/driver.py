import hashlib,json,os,pathlib,platform,subprocess,time
root=pathlib.Path.home()/'.cache/hex-adversarial-relay-01a1c640-v5'
source=root/'source'; proof=root/'proof'; cargo_home=root/'cargo'; target=root/'target'
assert cargo_home.is_dir() and target.is_dir()
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
assert len(names)==3 and all('candidate_dc29' not in name for name in names),names
rows=[]
for number,name in enumerate(names):
    command=[str(binary),'--exact',name,'--nocapture','--test-threads=1']
    with (proof/('test-'+str(number)+'.log')).open('w') as log:
        result=subprocess.Popen(command,cwd=source,env=env,stdout=log,stderr=subprocess.STDOUT)
        (proof/('test-'+str(number)+'.pid')).write_text(str(result.pid)+'\n')
        try:
            result.wait(timeout=120)
        except subprocess.TimeoutExpired:
            result.kill()
            result.wait()
            raise
    raw=(proof/('test-'+str(number)+'.log')).read_bytes()
    rows.append({'name':name,'command':command,'exit_code':result.returncode,'log':'test-'+str(number)+'.log','log_sha256':hashlib.sha256(raw).hexdigest(),'log_bytes':len(raw),'pid':result.pid,'reaped':result.returncode is not None,'ran_one':b'running 1 test' in raw and (b'test result: ok. 1 passed;' in raw if result.returncode==0 else b'1 failed;' in raw)})
summary={'build_exit':0,'binary':str(binary),'binary_sha256':binary_sha,'tests':rows,'selected':len(names),'passes':sum(row['exit_code']==0 for row in rows),'nonzero':sum(row['exit_code']!=0 for row in rows),'scope':'actual native macOS test binary; Service/Order/Driver with SimRuntime effects; no native runtime box, SSH or VM','candidate_note':'original dc29 candidate retained separately in v3; final main leaf has 3 contract fixtures only'}
clippy_cmd=[str(cargo),'clippy','-p','hex-node','--tests','--locked','--offline','--no-deps','--','-D','warnings']
(proof/'clippy-command.json').write_text(json.dumps(clippy_cmd)+'\n')
with (proof/'clippy.log').open('w') as log:
    clippy=subprocess.run(clippy_cmd,cwd=source,env=env,stdout=log,stderr=subprocess.STDOUT)
(proof/'clippy.exit').write_text(str(clippy.returncode)+'\n')
fmt_cmd=[str(toolchain/'rustfmt'),'--edition','2024','--check',str(source/'crates/hex-node/src/scheduler/adversarial_relay_tests.rs')]
with (proof/'rustfmt.log').open('w') as log:
    fmt=subprocess.run(fmt_cmd,cwd=source,env=env,stdout=log,stderr=subprocess.STDOUT)
(proof/'rustfmt.exit').write_text(str(fmt.returncode)+'\n')
expected=json.loads((proof/'source.json').read_text())['checked_in_leaf_sha256']
summary.update(scoped_clippy_exit=clippy.returncode,scoped_rustfmt_exit=fmt.returncode,final_leaf_sha256=hashlib.sha256((source/'crates/hex-node/src/scheduler/adversarial_relay_tests.rs').read_bytes()).hexdigest())
summary['one_test_per_command']=all(row['ran_one'] for row in rows)
assert summary['final_leaf_sha256']==expected
(proof/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps(summary))

raise SystemExit(0 if summary['nonzero']==0 and clippy.returncode==0 and fmt.returncode==0 and summary['one_test_per_command'] else 1)
