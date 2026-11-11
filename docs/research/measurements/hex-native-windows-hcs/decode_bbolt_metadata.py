"""Read-only metadata inspection of stopped owned containerd databases, not a build/test.

Format: https://github.com/etcd-io/bbolt/tree/v1.4.3/internal/common/{meta,page,bucket}.go
Containerd snapshot keys: https://github.com/containerd/containerd/blob/v2.2.0/core/snapshots/storage/bolt.go
The observed DB magic/version/checksums are verified; version2.2 is schema context,
not a claim that installed containerd2.4.2 source was inspected at its unavailable web pin.
"""
from collections import deque
import hashlib
import json
from pathlib import Path
import struct
import sys

path = Path(sys.argv[1])
data = path.read_bytes()
if len(data) > 8388608:
    raise ValueError('original metadata collection bound')

def extent(blob, start, size):
    if start < 0 or size < 0 or start + size > len(blob):
        raise ValueError('metadata extent outside retained bytes')
    return blob[start:start+size]

def fnv64a(blob):
    value = 14695981039346656037
    for byte in blob:
        value = ((value ^ byte) * 1099511628211) & ((1 << 64)-1)
    return value

page_size = struct.unpack('<I', extent(data, 24, 4))[0]
if page_size < 1024 or page_size & (page_size-1) or page_size > len(data)//2:
    raise ValueError('invalid measured page size')
metas = []
for page_id in (0, 1):
    meta = extent(data, page_id*page_size+16, 64)
    magic, version, size, flags, root, sequence, free, high, tx, checksum = struct.unpack('<IIIIQQQQQQ', meta)
    if magic == 0xed0cdaed and version == 2 and size == page_size and fnv64a(meta[:-8]) == checksum:
        metas.append({'page': page_id, 'root': root, 'high': high, 'transaction': tx, 'checksum': f'{checksum:016x}'})
if not metas:
    raise ValueError('no valid checksummed metadata page')
chosen = max(metas, key=lambda meta: meta['transaction'])
pending = deque([(chosen['root'], [], None)])
seen = set()
rows = []
visits = 0
while pending:
    page_id, names, inline = pending.popleft()
    visits += 1
    if visits > len(data):
        raise ValueError('file-derived metadata node budget')
    if inline is None:
        if page_id in seen or not 1 < page_id < chosen['high']:
            raise ValueError('cycle/repeated or out-of-range page')
        seen.add(page_id)
        header = extent(data, page_id*page_size, 16)
        identity, kind, count, overflow = struct.unpack('<QHHI', header)
        if identity != page_id:
            raise ValueError('page identity mismatch')
        page = extent(data, page_id*page_size, (overflow+1)*page_size)
    else:
        page = inline
        _, kind, count, overflow = struct.unpack('<QHHI', extent(page, 0, 16))
        if overflow:
            raise ValueError('inline overflow')
    for index in range(count):
        offset = 16+index*16
        item = extent(page, offset, 16)
        if kind == 1:
            position, size, child = struct.unpack('<IIQ', item)
            extent(page, offset+position, size)
            pending.append((child, names, None))
            continue
        if kind != 2:
            raise ValueError('not a leaf/branch page')
        flags, position, key_size, value_size = struct.unpack('<IIII', item)
        key = extent(page, offset+position, key_size)
        value = extent(page, offset+position+key_size, value_size)
        label = key.decode('utf-8', errors='backslashreplace')
        trail = names+[label]
        if flags == 1:
            child, sequence = struct.unpack('<QQ', extent(value, 0, 16))
            pending.append((child, trail, None if child else value[16:]))
        elif flags == 0:
            printable = all(32 <= byte < 127 for byte in value)
            rows.append({'path': trail, 'value': value.decode('ascii') if printable else None, 'hex': None if printable else value.hex(), 'bytes': len(value)})
        else:
            raise ValueError('unknown leaf flags')
result = {'file': str(path), 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest(), 'page_size': page_size, 'valid_metas': metas, 'chosen': chosen, 'rows': rows}
print(json.dumps(result, indent=2))
