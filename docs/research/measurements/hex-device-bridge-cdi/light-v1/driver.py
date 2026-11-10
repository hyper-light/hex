import hashlib,json,os,pathlib,platform,subprocess
root=pathlib.Path.home()/'.cache/hex-cdi-01a1c640-9cb5953c-v1'
source=root/'source';proof=root/'proof';target=root/'target';cargo_home=root/'cargo'
assert not target.exists() and not cargo_home.exists()
target.mkdir();cargo_home.mkdir()
copy=subprocess.run(['cp','-cR',str(pathlib.Path.home()/'.cargo/registry'),str(cargo_home/'registry')],capture_output=True,text=True)
(proof/'cache-copy.stdout').write_text(copy.stdout);(proof/'cache-copy.stderr').write_text(copy.stderr);assert copy.returncode==0
toolchain=pathlib.Path.home()/'.cache/hex-lead/rustup-scheduler-copy/toolchains/1.99.1-aarch64-apple-darwin/bin'
cargo=toolchain/'cargo';assert cargo.is_file()
env=os.environ.copy()
for key in list(env):
    if key.startswith('DYLD_'):env.pop(key,None)
env.update(CARGO_HOME=str(cargo_home),CARGO_TARGET_DIR=str(target),CARGO_BUILD_JOBS='1',RUST_TEST_THREADS='1',RUSTC=str(toolchain/'rustc'),RUSTDOC=str(toolchain/'rustdoc'),RUSTUP_HOME=str(toolchain.parent.parent),RUSTUP_TOOLCHAIN='1.99.1')
env['PATH']=str(toolchain)+':'+env.get('PATH','')
versions={key:subprocess.check_output([str(toolchain/key),'--version'],cwd=source,env=env,text=True).strip() for key in ['cargo','rustc']}
(proof/'environment.json').write_text(json.dumps({'platform':platform.platform(),'machine':platform.machine(),'versions':versions,'needs':os.environ.get('KILN_NEEDS'),'nice':os.getpriority(os.PRIO_PROCESS,0),'jobs':1,'threads':1},indent=2)+'\n')
build=[str(cargo),'test','-p','hex-core','--lib','--locked','--offline','--no-run','--message-format=json']
(proof/'build-command.json').write_text(json.dumps(build)+'\n')
with (proof/'build.jsonl').open('w') as out,(proof/'build.stderr').open('w') as err:
    built=subprocess.run(build,cwd=source,env=env,stdout=out,stderr=err)
(proof/'build.exit').write_text(str(built.returncode)+'\n')
if built.returncode:
    errors=[]
    for line in (proof/'build.jsonl').read_text().splitlines():
        row=json.loads(line)
        if row.get('reason')=='compiler-message' and row.get('message',{}).get('level')=='error':
            errors.append(row['message'].get('rendered',''))
    print(json.dumps({'build_exit':built.returncode,'errors':errors,'stderr_tail':(proof/'build.stderr').read_text()[-2000:]}))
    raise SystemExit(built.returncode)
binaries=[row['executable'] for line in (proof/'build.jsonl').read_text().splitlines() if (row:=json.loads(line)).get('reason')=='compiler-artifact' and row.get('target',{}).get('name')=='hex_core' and row.get('executable')]
assert len(binaries)==1,binaries
binary=pathlib.Path(binaries[0])
listed=subprocess.check_output([str(binary),'--list'],cwd=source,env=env,text=True)
(proof/'tests.list').write_text(listed)
names=[line.removesuffix(': test') for line in listed.splitlines() if line.startswith('cdi::tests::') and line.endswith(': test')]
assert len(names)==8,names
rows=[]
for number,name in enumerate(names):
    command=[str(binary),'--exact',name,'--nocapture','--test-threads=1']
    path=proof/('test-'+str(number)+'.log')
    with path.open('w') as log:
        child=subprocess.Popen(command,cwd=source,env=env,stdout=log,stderr=subprocess.STDOUT)
        (proof/('test-'+str(number)+'.pid')).write_text(str(child.pid)+'\n')
        try:child.wait(timeout=120)
        except subprocess.TimeoutExpired:
            child.kill();child.wait();raise
    raw=path.read_bytes()
    ran_one=b'running 1 test' in raw and (b'test result: ok. 1 passed;' if child.returncode==0 else b'1 failed;') in raw
    rows.append({'name':name,'command':command,'exit_code':child.returncode,'pid':child.pid,'reaped':True,'ran_one':ran_one,'log':path.name,'log_sha256':hashlib.sha256(raw).hexdigest()})
checks={}
for label,flags in [('clippy-lib',['--lib']),('clippy-tests',['--lib','--tests'])]:
    command=[str(cargo),'clippy','-p','hex-core',*flags,'--locked','--offline','--no-deps','--','-D','warnings']
    (proof/(label+'-command.json')).write_text(json.dumps(command)+'\n')
    with (proof/(label+'.log')).open('w') as log:
        result=subprocess.run(command,cwd=source,env=env,stdout=log,stderr=subprocess.STDOUT)
    checks[label]=result.returncode
    (proof/(label+'.exit')).write_text(str(result.returncode)+'\n')
fmt=[str(toolchain/'rustfmt'),'--edition','2024','--check',str(source/'crates/hex-core/src/cdi.rs'),str(source/'crates/hex-core/src/cdi/tests.rs')]
with (proof/'rustfmt.log').open('w') as log:
    result=subprocess.run(fmt,cwd=source,env=env,stdout=log,stderr=subprocess.STDOUT)
checks['rustfmt']=result.returncode
(proof/'rustfmt.exit').write_text(str(result.returncode)+'\n')
manifest=json.loads((proof/'source.json').read_text())
hashes={key:hashlib.sha256((source/path).read_bytes()).hexdigest() for key,path in [('cdi_sha256','crates/hex-core/src/cdi.rs'),('tests_sha256','crates/hex-core/src/cdi/tests.rs')]}
assert all(manifest[key]==value for key,value in hashes.items())
summary={'build_exit':0,'binary':str(binary),'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'tests':rows,'selected':len(rows),'passes':sum(row['exit_code']==0 and row['ran_one'] for row in rows),'nonzero':sum(row['exit_code']!=0 for row in rows),'one_test_per_command':all(row['ran_one'] for row in rows),'checks':checks,'hashes':hashes,'scope':'Pure CDI parser/select tests in native arm64 macOS harness; no IO/registry/OCI/native device effects'}
(proof/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps(summary))
raise SystemExit(0 if summary['passes']==8 and summary['nonzero']==0 and all(value==0 for value in checks.values()) else 1)
