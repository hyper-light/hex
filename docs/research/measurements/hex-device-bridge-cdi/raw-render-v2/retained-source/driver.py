import hashlib,json,os,pathlib,platform,subprocess
root=pathlib.Path.home()/'.cache/hex-cdi-render-01a1c640-74f889be-v2'
previous=pathlib.Path.home()/'.cache/hex-cdi-render-01a1c640-661cbd81-v1'
source=root/'source';proof=root/'proof';baseline=previous/'baseline'
toolchain=pathlib.Path.home()/'.cache/hex-lead/rustup-scheduler-copy/toolchains/1.99.1-aarch64-apple-darwin/bin'
cargo=toolchain/'cargo'
assert cargo.is_file() and baseline.exists()
manifest=json.loads((proof/'source.json').read_text())
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
for name,sha in manifest['files'].items():assert digest(source/name)==sha,(name,digest(source/name),sha)
(proof/'source-before').mkdir()
for name in ['crates/hex-core/src/lib.rs','crates/hex-core/src/bundle.rs']:
    (proof/'source-before'/pathlib.Path(name).name).write_bytes((source/name).read_bytes())
assert digest(root/'baseline-bundle.rs')==manifest['baseline_bundle_sha256']
capture='''
    #[test]
    fn render_legacy_capture() {
        let output = PathBuf::from(std::env::var_os("HEX_RENDER_CAPTURE").unwrap());
        let masked = ["/sys/host-private-mask".to_owned()];
        let mut rootless_dir = in_place();
        rootless_dir.init.user = None;
        let cases = [
            ("layers-root", layered(), at(Owner::Root, &[])),
            ("layers-rootless", layered(), at(ROOTLESS, &[])),
            ("dir-root", in_place(), at(Owner::Root, &masked)),
            ("dir-rootless", rootless_dir, at(ROOTLESS, &[])),
        ];
        for (name, spec, place) in cases {
            let rendered = render(&spec, &place).unwrap();
            std::fs::write(output.join(format!("{name}.config")), rendered.config).unwrap();
            std::fs::write(output.join(format!("{name}.hosts")), rendered.hosts).unwrap();
            std::fs::write(
                output.join(format!("{name}.dirs")),
                serde_json::to_vec(&rendered.dirs).unwrap(),
            )
            .unwrap();
        }
    }
'''
(proof/'private-capture.rs').write_text(capture)
baseenv=os.environ.copy()
for key in list(baseenv):
    if key.startswith('DYLD_'):baseenv.pop(key,None)
baseenv.update(CARGO_BUILD_JOBS='1',RUST_TEST_THREADS='1',RUSTC=str(toolchain/'rustc'),RUSTDOC=str(toolchain/'rustdoc'),RUSTUP_HOME=str(toolchain.parent.parent),RUSTUP_TOOLCHAIN='1.99.1')
baseenv['PATH']=str(toolchain)+':'+baseenv.get('PATH','')
(proof/'environment.json').write_text(json.dumps({'platform':platform.platform(),'machine':platform.machine(),'needs':os.environ.get('KILN_NEEDS'),'nice':os.getpriority(os.PRIO_PROCESS,0),'jobs':1,'threads':1,'versions':{key:subprocess.check_output([str(toolchain/key),'--version'],env=baseenv,text=True).strip() for key in ['cargo','rustc']}},indent=2)+'\n')
rows=[];checks={};binaries={};registered={}
def run(command,cwd,env,label,jsonout=False):
    (proof/(label+'-command.json')).write_text(json.dumps(command)+'\n')
    with (proof/(label+('.jsonl' if jsonout else '.log'))).open('w') as out,(proof/(label+'.stderr')).open('w') as err:
        child=subprocess.Popen(command,cwd=cwd,env=env,stdout=out,stderr=err)
        (proof/(label+'.pid')).write_text(str(child.pid)+'\n')
        try:child.wait(timeout=600)
        except subprocess.TimeoutExpired:
            child.kill();child.wait();raise
    (proof/(label+'.exit')).write_text(str(child.returncode)+'\n')
    return child.returncode

def build(label,src):
    cargo_home=root/(label+'-cargo');target=root/(label+'-target')
    assert not cargo_home.exists() and not target.exists()
    cargo_home.mkdir();target.mkdir()
    copied=subprocess.run(['cp','-cR',str(pathlib.Path.home()/'.cargo/registry'),str(cargo_home/'registry')],capture_output=True,text=True)
    (proof/(label+'-cache-copy.log')).write_text(copied.stdout+copied.stderr);assert copied.returncode==0
    lib=src/'crates/hex-core/src/lib.rs';bundle=src/'crates/hex-core/src/bundle.rs'
    before={'lib':digest(lib),'bundle':digest(bundle)}
    lib.write_text(lib.read_text()+'\npub mod cdi;\n')
    original=bundle.read_text();head,tail=original.rsplit('}',1);assert not tail.strip()
    bundle.write_text(head+capture+'}\n')
    env=baseenv|{'CARGO_HOME':str(cargo_home),'CARGO_TARGET_DIR':str(target)}
    assert run([str(toolchain/'rustfmt'),'--edition','2024',str(bundle)],src,env,label+'-private-format')==0
    registered[label]={'before':before,'after':{'lib':digest(lib),'bundle':digest(bundle)},'private_changes':'lib.rs pub mod cdi; bundle::tests::render_legacy_capture (source retained)'}
    result=run([str(cargo),'test','-p','hex-core','--lib','--locked','--offline','--no-run','--message-format=json'],src,env,label+'-build',True)
    checks[label+'-build']=result
    if result:
        errors=[]
        for line in (proof/(label+'-build.jsonl')).read_text().splitlines():
            row=json.loads(line)
            if row.get('reason')=='compiler-message' and row.get('message',{}).get('level')=='error':errors.append(row['message'].get('rendered',''))
        print(json.dumps({'phase':label,'exit':result,'errors':errors,'stderr_tail':(proof/(label+'-build.stderr')).read_text()[-3000:]}),flush=True)
        (proof/'private-registration.json').write_text(json.dumps(registered,indent=2)+'\n')
        raise SystemExit(result)
    outputs=[row['executable'] for line in (proof/(label+'-build.jsonl')).read_text().splitlines() if (row:=json.loads(line)).get('reason')=='compiler-artifact' and row.get('target',{}).get('name')=='hex_core' and row.get('executable')]
    assert len(outputs)==1,outputs
    binary=pathlib.Path(outputs[0]);binaries[label]={'path':str(binary),'sha256':digest(binary)}
    print(json.dumps({'built':label,'binary_sha256':digest(binary)}),flush=True)
    return binary,env

def test(label,binary,src,env,name):
    command=[str(binary),'--exact',name,'--nocapture','--test-threads=1']
    path=proof/(label+'.log')
    with path.open('w') as log:
        child=subprocess.Popen(command,cwd=src,env=env,stdout=log,stderr=subprocess.STDOUT)
        (proof/(label+'.pid')).write_text(str(child.pid)+'\n')
        try:child.wait(timeout=120)
        except subprocess.TimeoutExpired:
            child.kill();child.wait();raise
    raw=path.read_bytes();ran=b'running 1 test' in raw
    row={'name':name,'label':label,'command':command,'exit_code':child.returncode,'pid':child.pid,'reaped':True,'ran_one':ran,'log':path.name,'log_sha256':hashlib.sha256(raw).hexdigest(),'passed':ran and child.returncode==0 and b'test result: ok. 1 passed;' in raw}
    rows.append(row)
    if not row['passed']:print(json.dumps({'failed':row,'raw':raw.decode(errors='replace')[-5000:]}),flush=True)

previous_summary=json.loads((previous/'proof/summary.json').read_text())
assert previous_summary['passes']==27 and previous_summary['failed']==0
assert previous_summary['checks']['legacy-byte-comparison']==0
assert previous_summary['source']=='661cbd8140bdcc5510b20e6535790690997ca33e'
binaries['baseline']=previous_summary['binaries']['baseline']
basecapture=previous/'proof/baseline-capture'
(proof/'baseline-reference.json').write_text(json.dumps({'run':'r17','summary_sha256':digest(previous/'proof/summary.json'),'binary':binaries['baseline'],'baseline_bundle_sha256':digest(previous/'baseline-bundle.rs'),'scope':'Read-only reuse of retained d4d capture bytes; baseline is not executed again'},indent=2)+'\n')
binary,env=build('current',source)
listed=subprocess.check_output([str(binary),'--list'],cwd=source,env=env,text=True)
(proof/'tests.list').write_text(listed)
names=[line.removesuffix(': test') for line in listed.splitlines() if line.startswith(('bundle::','cdi::tests::')) and line.endswith(': test')]
assert sum(n.startswith('bundle::device_edits::tests::') for n in names)==9,names
assert sum(n.startswith('cdi::tests::') for n in names)==8,names
assert len(names)==26,names
currentcapture=proof/'current-capture';currentcapture.mkdir()
for number,name in enumerate(names):
    test('current-test-'+str(number),binary,source,env|{'HEX_RENDER_CAPTURE':str(currentcapture)},name)
comparisons={}
for path in sorted(basecapture.iterdir()):
    other=currentcapture/path.name
    comparisons[path.name]={'same':other.is_file() and path.read_bytes()==other.read_bytes(),'baseline_sha256':digest(path),'current_sha256':digest(other) if other.is_file() else None,'bytes':path.stat().st_size}
checks['legacy-byte-comparison']=0 if len(comparisons)==12 and all(r['same'] for r in comparisons.values()) else 1
(proof/'legacy-comparison.json').write_text(json.dumps(comparisons,indent=2)+'\n')
for label,flags in [('clippy-lib',['--lib']),('clippy-tests',['--lib','--tests'])]:
    checks[label]=run([str(cargo),'clippy','-p','hex-core',*flags,'--locked','--offline','--no-deps','--','-D','warnings'],source,env,label)
    if checks[label]:print(json.dumps({'check':label,'exit':checks[label],'log_tail':(proof/(label+'.log')).read_text()[-7000:],'stderr_tail':(proof/(label+'.stderr')).read_text()[-7000:]}),flush=True)
checks['rustfmt']=run([str(toolchain/'rustfmt'),'--edition','2024','--check',*[str(source/p) for p in ['crates/hex-core/src/bundle.rs','crates/hex-core/src/bundle/device_edits.rs','crates/hex-core/src/bundle/device_edits/tests.rs']]],source,env,'rustfmt')
(proof/'private-registration.json').write_text(json.dumps(registered,indent=2)+'\n')
for label,src in [('current',source)]:
    for short in ['lib','bundle']:
        (proof/(label+'-private-'+short+'.rs')).write_bytes((src/('crates/hex-core/src/'+short+'.rs')).read_bytes())
for name,sha in manifest['files'].items():
    if name not in ['crates/hex-core/src/lib.rs','crates/hex-core/src/bundle.rs']:assert digest(source/name)==sha
summary={'source':manifest['commit'],'baseline':'d4d96083c83f88ff3e9b9447598a3b95f461b680','binaries':binaries,'tests':rows,'passes':sum(r['passed'] for r in rows),'failed':sum(not r['passed'] for r in rows),'checks':checks,'counts':{'new_renderer':9,'existing_renderer':8,'cdi':8,'private_capture_current':1,'private_capture_baseline':0},'legacy_files':len(comparisons),'scope':'Pure render/CDI; private proof-file writes only. No native stat/mount/device/hook/VM/SSH effects.'}
(proof/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({k:v for k,v in summary.items() if k!='tests'}),flush=True)
raise SystemExit(0 if summary['passes']==26 and summary['failed']==0 and all(v==0 for v in checks.values()) else 1)
