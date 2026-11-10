from pathlib import Path
import os,subprocess,json,hashlib,traceback
h=Path.home()/'.cache/hex-lead'
src=h/'snapshots/recovery-f3f19c07'
proof=h/'recovery-f3f19c07-seven-gates'
assert (src/'SOURCE_REVISION').read_text().strip()=='f3f19c07e658dcd8097bba3dbb2bcbc484d9d8d7'
proof.mkdir(exist_ok=False)
env=dict(os.environ,PATH=str(h/'python-3.13.18-20261026/python/bin')+':'+str(Path.home()/'.cargo/bin')+':'+os.environ['PATH'],RUSTUP_HOME=str(h/'rustup-scheduler-copy'),RUSTUP_TOOLCHAIN='1.99.1',CARGO_BUILD_JOBS='8',RUST_TEST_THREADS='8',CARGO_TARGET_DIR=str(h/'target-recovery-f3f19c07-seven'))
for k in list(env):
 if k.startswith('DYLD_') or k.startswith('HEX_WAITS_CENSUS') or k.startswith('HEX_DEBUG_'):env.pop(k,None)
argv=['nice','-n','10','bash','scripts/gates.sh']
(proof/'driver.pid').write_text(str(os.getpid()))
log=proof/'gates.log'
with log.open('w',buffering=1) as out:
 out.write(json.dumps(argv)+'\n')
 child=subprocess.Popen(argv,cwd=src,env=env,stdout=out,stderr=out)
 (proof/'child.pid').write_text(str(child.pid))
 rc=child.wait()
(proof/'gates.exit').write_text(str(rc)+'\n')
data={'source_revision':'f3f19c07e658dcd8097bba3dbb2bcbc484d9d8d7','source_tree':(src/'SOURCE_TREE').read_text().strip(),'argv':argv,'exit':rc,'resources':'8cpu 24gmem 30gdisk','log_sha256':hashlib.file_digest(log.open('rb'),'sha256').hexdigest(),'scope':'fmt/seven-target workspace lints/production/native Mac workspace/vendor; not native AMD64 Linux rootlab or Windows execution'}
(proof/'gates.json').write_text(json.dumps(data,indent=2)+'\n')
print(json.dumps(data));raise SystemExit(rc)
