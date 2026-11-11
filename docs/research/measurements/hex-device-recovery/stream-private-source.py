"""Stream an immutable lane revision to a NEW approved private light scope."""
from pathlib import Path
import argparse
import gzip
import hashlib
import json
import shlex
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument("revision")
parser.add_argument("remote_name")
args = parser.parse_args()
assert args.remote_name.startswith("device-recovery-")
assert "01a1c640" in args.remote_name and Path(args.remote_name).name == args.remote_name
revision = subprocess.check_output(["git", "rev-parse", args.revision], text=True).strip()
blob = gzip.compress(subprocess.check_output(["git", "archive", "--format=tar", revision]), mtime=0)
paths = ["crates/hex-sim/src/" + p for p in [
    "model.rs", "index.rs", "peer.rs", "peer/adversarial_device_recovery_tests.rs"
]]
sources = {p: subprocess.check_output(["git", "show", revision + ":" + p]) for p in paths}
available = b"pub available: bool" in sources[paths[0]]
receipt = {
    "source_revision": revision,
    "fixed_base": "37021c35e64b2a8bab1d35f20f92a0042d874a44",
    "archive_sha256": hashlib.sha256(blob).hexdigest(),
    "archive_bytes": len(blob),
    "source_sha256": {p: hashlib.sha256(data).hexdigest() for p, data in sources.items()},
    "source_stream": "git archive gzip in memory; no local bulk archive",
    "local_bulk_transfer": False,
}
remote = r'''
from pathlib import Path
import hashlib,io,json,sys,tarfile
base=Path.home()/'.cache/hex-lead/lanes'/REMOTE_NAME
base.mkdir(parents=True,exist_ok=False)
receipt=json.loads(RECEIPT)
blob=sys.stdin.buffer.read()
assert len(blob)==receipt['archive_bytes'] and hashlib.sha256(blob).hexdigest()==receipt['archive_sha256']
(base/'source.tar.gz').write_bytes(blob)
source=base/'source';source.mkdir()
with tarfile.open(fileobj=io.BytesIO(blob),mode='r:gz') as archive:
 members=archive.getmembers()
 for member in members:
  p=Path(member.name)
  assert not p.is_absolute() and '..' not in p.parts and (member.isfile() or member.isdir())
 archive.extractall(source,members=members)
for name in ['target','cargo-home','tmp','proof']: (base/name).mkdir()
for name,expected in receipt['source_sha256'].items(): assert hashlib.sha256((source/name).read_bytes()).hexdigest()==expected
parent=source/'crates/hex-sim/src/peer.rs'
addition=b'\n#[cfg(test)]\n#[path = "peer/adversarial_device_recovery_tests.rs"]\nmod adversarial_device_recovery_tests;\n'
parent.write_bytes(parent.read_bytes()+addition)
facts=source/'crates/hex-sim/src/facts.rs';original=facts.read_bytes()
needle=b'kind: f.kind,\n                model: ModelId::UNNAMED,'
assert original.count(needle)==1
replacement=b'kind: f.kind,\n                key: None,\n'+AVAILABILITY+b'                model: ModelId::UNNAMED,'
compat=original.replace(needle,replacement);facts.write_bytes(compat)
receipt.update(remote_base=str(base),source_members=len(members),temporary_registration=addition.decode(),parent_registered_sha256=hashlib.sha256(parent.read_bytes()).hexdigest(),private_compatibility={'file':'crates/hex-sim/src/facts.rs','change':'only add key: None and, when present, available: true to existing explicit Device literal; ROOT owns actual identity bridge','original_sha256':hashlib.sha256(original).hexdigest(),'private_sha256':hashlib.sha256(compat).hexdigest()})
(base/'proof/input-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt))
'''
# The transport script is a literal Python program, passed as one shell-quoted argument.
remote = remote.replace("REMOTE_NAME", repr(args.remote_name)).replace("RECEIPT", repr(json.dumps(receipt)))
remote = remote.replace("AVAILABILITY", repr(b"                available: true,\n" if available else b""))
# Unescape the inner script's byte/text literals once; the outer script is raw for readability.
remote = remote.replace("\\\\n", "\\n")
command = [
    "/Users/adalundhe/.local/bin/kiln", "run", "--on", "d1323898b255a3ed435eb83fa0f7b0cf",
    "--needs", "cpus=1,mem=4g,disk=12g", "--no-sync", "--stdin", "--timeout", "120", "--",
    "nice -n 10 python3 -c " + shlex.quote(remote),
]
result = subprocess.run(command, input=blob, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
print(result.stdout.decode(errors="replace"))
raise SystemExit(result.returncode)
