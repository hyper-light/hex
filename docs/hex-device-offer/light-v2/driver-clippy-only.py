import hashlib,json,os,pathlib,platform,subprocess
root=pathlib.Path.home()/'.cache/hex-device-offer-01a1c640-a69d3426-v2'
source=root/'source';proof=root/'proof-clippy';proof.mkdir()
manifest=json.loads((root/'proof/source.json').read_text())
parent='crates/hex-node/src/scheduler/tests.rs'
registration=b'\n#[path = "device_offer_tests.rs"]\nmod device_offer_tests;\n'
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
for path,digest in manifest['files'].items():
    if path!=parent:assert sha(source/path)==digest,path
assert (source/parent).read_bytes()==(root/'proof/parent-before.rs').read_bytes()+registration
assert not list((root/'proof').glob('test-*.log'))
toolchain=pathlib.Path.home()/'.cache/hex-lead/rustup-scheduler-copy/toolchains/1.99.1-aarch64-apple-darwin/bin'
env=os.environ.copy()
for key in list(env):
    if key.startswith(('DYLD_','HEX_DEBUG_','HEX_WAITS_CENSUS')):env.pop(key,None)
env.update(CARGO_HOME=str(root/'cargo'),CARGO_TARGET_DIR=str(root/'target'),CARGO_BUILD_JOBS='1',RUST_TEST_THREADS='1',RUSTC=str(toolchain/'rustc'),RUSTDOC=str(toolchain/'rustdoc'),RUSTUP_HOME=str(toolchain.parent.parent),RUSTUP_TOOLCHAIN='1.99.1')
env['PATH']=str(toolchain)+':'+env.get('PATH','')
command=[str(toolchain/'cargo'),'clippy','-p','hex-node','--lib','--tests','--locked','--offline','--no-deps','--','-D','warnings']
(proof/'command.json').write_text(json.dumps(command)+'\n')
with (proof/'clippy.stdout').open('w') as out,(proof/'clippy.stderr').open('w') as err:
    child=subprocess.Popen(command,cwd=source,env=env,stdout=out,stderr=err)
    (proof/'clippy.pid').write_text(str(child.pid)+'\n')
    try:child.wait(timeout=900)
    except subprocess.TimeoutExpired:
        child.kill();child.wait();raise
after={path:sha(source/path) for path in manifest['files']}
for path,digest in manifest['files'].items():
    if path!=parent:assert after[path]==digest,path
assert (source/parent).read_bytes()==(root/'proof/parent-before.rs').read_bytes()+registration
assert not list((root/'proof').glob('test-*.log'))
row={'source':manifest['commit'],'production_pin':manifest['production_pin'],'command':command,'exit':child.returncode,'pid':child.pid,'reaped':True,'tests_executed':0,'source_after':after,'parent_private_sha256':after[parent],'stdout_sha256':sha(proof/'clippy.stdout'),'stderr_sha256':sha(proof/'clippy.stderr'),'platform':platform.platform(),'arch':platform.machine(),'needs':os.environ.get('KILN_NEEDS'),'nice':os.getpriority(os.PRIO_PROCESS,0),'jobs':1,'threads':1,'scope':'Test Clippy ONLY for Copy correction after r22 interrupted during --no-run; no test binary executed.'}
(proof/'summary.json').write_text(json.dumps(row,indent=2)+'\n')
print(json.dumps({k:v for k,v in row.items() if k!='source_after'}),flush=True)
if child.returncode:print((proof/'clippy.stderr').read_text()[-5000:],flush=True)
raise SystemExit(child.returncode)
