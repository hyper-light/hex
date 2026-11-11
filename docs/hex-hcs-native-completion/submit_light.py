#!/usr/bin/env python3
"""Submit exactly one compile-only private d132 check; never execute a native fixture."""
import gzip
import hashlib
import json
from pathlib import Path
import shlex
import subprocess
import sys

requested_revision,tag=sys.argv[1:3]
revision=subprocess.check_output(['git','rev-parse',requested_revision+'^{commit}'],text=True).strip()
free=int(subprocess.check_output(['df','-k','.'],text=True).splitlines()[-1].split()[3])
if free<30*1024*1024: raise SystemExit('hold source transfer: local free below30GiB')
base=Path('docs/hex-hcs-native-completion')
local=Path('/Users/adalundhe/.cache/hex-lead/transfers')/('hcs-native-completion-01a1c643-'+tag)
local.mkdir(parents=True,exist_ok=False)
archive=gzip.compress(subprocess.check_output(['git','archive','--format=tar',revision]),mtime=0)
(local/'source.tar.gz').write_bytes(archive)
sha=hashlib.sha256(archive).hexdigest()
remote='/Users/adalundhe/.cache/hex-hcs-native-completion/01a1c643-'+tag
script=(base/'light_check.py').read_text()
command='python3 -c '+shlex.quote(script)+' '+shlex.quote(remote)+' '+sha+' '+revision
meta={'source_revision':revision,'archive_sha256':sha,'archive_bytes':len(archive),'runner':'d1323898b255a3ed435eb83fa0f7b0cf','needs':'cpus=1,mem=4g,disk=12g','remote_root':remote,'local_root':str(local),'free_kib_before':free,'driver_sha256':hashlib.sha256(script.encode()).hexdigest(),'command':command}
(local/'transfer.json').write_text(json.dumps(meta,indent=2)+'\n')
with (local/'cli.log').open('xb') as log:
 child=subprocess.Popen(['/Users/adalundhe/.local/bin/kiln','run','--on','d1323898b255a3ed435eb83fa0f7b0cf','--needs','cpus=1,mem=4g,disk=12g','--no-sync','--stdin','--timeout','1800','--',command],stdin=subprocess.PIPE,stdout=log,stderr=subprocess.STDOUT)
 meta['local_cli_pid']=child.pid;(local/'transfer.json').write_text(json.dumps(meta,indent=2)+'\n')
 child.communicate(archive);meta['local_cli_exit']=child.returncode
 meta['cli_log_sha256']=hashlib.sha256((local/'cli.log').read_bytes()).hexdigest()
 (local/'transfer.json').write_text(json.dumps(meta,indent=2)+'\n')
 print(json.dumps({k:v for k,v in meta.items() if k!='command'},indent=2),flush=True)
 print((local/'cli.log').read_text(errors='replace')[-23000:],flush=True)
 raise SystemExit(child.returncode)
