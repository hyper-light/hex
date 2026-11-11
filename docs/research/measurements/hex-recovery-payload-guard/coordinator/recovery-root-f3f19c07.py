from pathlib import Path
import os,subprocess,json,hashlib,traceback,shutil,platform
h=Path.home()/'.cache/hex-lead';canonical=h/'snapshots/recovery-f3f19c07';proof=h/'recovery-f3f19c07-root'
assert (canonical/'SOURCE_REVISION').read_text().strip()=='f3f19c07e658dcd8097bba3dbb2bcbc484d9d8d7'
proof.mkdir(exist_ok=False);src=proof/'source';shutil.copytree(canonical,src)
env=dict(os.environ,PATH=str(h/'python-3.13.18-20261026/python/bin')+':'+str(Path.home()/'.cargo/bin')+':'+os.environ['PATH'],RUSTUP_HOME=str(h/'rustup-scheduler-copy'),RUSTUP_TOOLCHAIN='1.99.1',CARGO_BUILD_JOBS='4',RUST_TEST_THREADS='4',CARGO_TARGET_DIR=str(h/'target-recovery-f3f19c07'))
for k in list(env):
 if k.startswith('DYLD_') or k.startswith('HEX_WAITS_CENSUS') or k.startswith('HEX_DEBUG_'):env.pop(k,None)
phases=[];status=1;(proof/'driver.pid').write_text(str(os.getpid()))
rows=src/'crates/hex-node/src/placement/rows.rs';original=rows.read_bytes()
def digest(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def run(label,argv,expected=0,marker=None):
 log=proof/(label+'.log')
 with log.open('w') as out:
  child=subprocess.Popen(['nice','-n','10']+argv,cwd=src,env=env,stdout=out,stderr=out)
  (proof/'child.pid').write_text(str(child.pid));rc=child.wait()
 row={'label':label,'argv':['nice','-n','10']+argv,'exit':rc,'expected':expected,'log_sha256':digest(log),'pid':child.pid,'reaped':True,'rows_sha256':digest(rows)}
 phases.append(row);(proof/'phases.json').write_text(json.dumps(phases,indent=2)+'\n');print(json.dumps(row),flush=True)
 assert rc==expected,(label,rc)
 if expected==101:
  text=log.read_text();assert 'test result: FAILED. 0 passed; 1 failed;' in text and marker in text,label
def test(label,name,expected=0,marker=None):
 run(label,['cargo','test','-p','hex-node','--lib','--locked','--target','aarch64-apple-darwin',name,'--','--exact','--nocapture','--test-threads=1'],expected,marker)
try:
 run('combined-node',['cargo','test','-p','hex-node','--lib','--locked','--target','aarch64-apple-darwin'])
 guard='''                // Recovery must validate the immutable local payload before any runtime callback.
                if self
                    .held
                    .get(&name("own", key))
                    .is_some_and(|own| own.payload != s.payload)
                {
                    return Err(bad(format!(
                        "owned and runner rows disagree on payload for {key:?}"
                    )));
                }
'''
 assert original.decode().count(guard)==1
 mutated=original.decode().replace(guard,'',1).encode();(proof/'rows.original.rs').write_bytes(original);(proof/'rows.guard-removed.rs').write_bytes(mutated)
 for label,name,marker in [
 ('start','contradictory_recovered_start_intent_refuses_without_executing_init','contradictory recovery executed init'),
 ('spec','equal_start_spec_disagreement_refuses_before_callbacks_without_rendering_payload','equal-start BoxSpec disagreement was not typed Damaged')]:
  try:
   rows.write_bytes(mutated);test('guard-removed-'+label,'placement::tests::adversarial_bounds_tests::'+name,101,marker)
  finally:
   rows.write_bytes(original);assert rows.read_bytes()==(canonical/'crates/hex-node/src/placement/rows.rs').read_bytes()
  test('restored-'+label,'placement::tests::adversarial_bounds_tests::'+name)
 run('node-clippy',['cargo','clippy','-p','hex-node','--all-targets','--locked','--target','aarch64-apple-darwin','--','-D','warnings'])
 run('host-debug',['cargo','build','-p','hex-cli','--bin','hex','--locked','--target','aarch64-apple-darwin'])
 status=0
except BaseException:(proof/'driver-error.txt').write_text(traceback.format_exc());print(traceback.format_exc(),flush=True)
finally:
 assert rows.read_bytes()==original
 host=Path(env['CARGO_TARGET_DIR'])/'aarch64-apple-darwin/debug/hex'
 meta={'source_revision':'f3f19c07e658dcd8097bba3dbb2bcbc484d9d8d7','tree':(canonical/'SOURCE_TREE').read_text().strip(),'archive_sha256':(canonical/'SOURCE_ARCHIVE_SHA256').read_text().strip(),'platform':platform.platform(),'exit':status,'phases':phases,'host':None if not host.is_file() else {'path':str(host),'sha256':digest(host),'bytes':host.stat().st_size},'scope':'ROOT combined native unit/Clippy, exact guard-removal effect controls and debug host build; native OCI proof and final full gates remain separate.'}
 (proof/'result.json').write_text(json.dumps(meta,indent=2)+'\n');(proof/'driver.exit').write_text(str(status)+'\n')
raise SystemExit(status)
