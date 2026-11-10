
import hashlib,json,os,subprocess,sys,tarfile
from pathlib import Path
lead=Path.home()/'.cache/hex-lead';root=lead/'vendor-check-d9-root-v2';root.mkdir(exist_ok=False)
source=root/'source';source.mkdir();archive=lead/'borrowed-contract-d9d547c7-mac-v1/source.tar.gz'
with tarfile.open(archive,mode='r:gz') as bundle:bundle.extractall(source,filter='data')
env=dict(os.environ);env['KILN_VENDOR_CACHE']=str(root/'cache')
before={str(p.relative_to(source)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(source.rglob('*')) if p.is_file()}
cmd=['nice','-n','10',str(lead/'python-3.13.18-20261026/python/bin/python3'),'scripts/vendor.py','check','shards']
with(root/'check.log').open('xb') as out:
 c=subprocess.Popen(cmd,cwd=source,env=env,stdout=out,stderr=subprocess.STDOUT);(root/'run.json').write_text(json.dumps({'pid':c.pid,'command':cmd}));code=c.wait()
after={str(p.relative_to(source)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(source.rglob('*')) if p.is_file()}
raw=(root/'check.log').read_bytes();r={'source':'d9d547c7d9211f6c42f1c2c884017a3981032889','exit':code,'pid':c.pid,'reaped':c.returncode is not None,'source_unchanged':before==after,'sha256':hashlib.sha256(raw).hexdigest(),'bytes':len(raw)}
(root/'result.json').write_text(json.dumps(r,indent=2));print(json.dumps(r));print(raw.decode(errors='replace'));sys.exit(code if before==after else 1)
