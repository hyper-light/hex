"""Decode one bounded read-only collection, preserving remote raw bytes and hashes."""
import base64
import hashlib
import json
from pathlib import Path

here = Path(__file__).resolve().parent
run = here / 'runs' / 'inputstate1'
raw = (run / 'cli.log').read_bytes()
data = json.loads(next(line for line in raw.decode().splitlines() if line.startswith('{"utc":')))
out = run / 'raw'
out.mkdir(exist_ok=False)
for name, encoded in data.pop('files').items():
    if Path(name).is_absolute() or '..' in Path(name).parts:
        raise ValueError('unsafe raw member')
    content = base64.b64decode(encoded, validate=True)
    receipt = data['roots'][name.split('/')[0]]['index'][name]
    if len(content) != receipt['bytes'] or hashlib.sha256(content).hexdigest() != receipt['sha256']:
        raise ValueError('byte-exact original hash mismatch')
    target = out / name
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(content)
(run / 'native-index.json').write_text(json.dumps(data, indent=2)+'\n')
print(json.dumps({'raw_bytes': data['total_bytes'], 'roots': data['roots']}, indent=2))
