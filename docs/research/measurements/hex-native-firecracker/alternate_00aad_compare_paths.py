import gzip,hashlib,json,os,sys,time
from pathlib import Path
packed=sys.stdin.buffer.read(1048577);assert len(packed)<=1048576
data=gzip.decompress(packed);assert len(data)<=16777216
manifest=json.loads(data);rows=manifest['rows'];assert len(rows)<=8192
deadline=time.monotonic()+40
out={'mode':'read-only exact4a SDK/payload path comparison; no materialization/compile/VM/GPU','cohort_sha256':manifest['cohort_sha256'],'results':[]}
for row in rows:
 assert time.monotonic()<deadline
 item={**row}
 p=Path(row['source'])
 try:
  before=p.stat()
  if not p.is_file():raise ValueError('not_regular')
  if before.st_size!=row['bytes']:
   item.update(state='different_extent',actual_bytes=before.st_size)
  else:
   h=hashlib.sha256();n=0
   with p.open('rb') as f:
    while True:
     b=f.read(1048576)
     if not b:break
     n+=len(b);assert n<=row['bytes']+1048576;h.update(b)
   item.update(state='exact_hash_match' if n==row['bytes'] and h.hexdigest()==row['sha256'] else 'different_digest',actual_bytes=n,actual_sha256=h.hexdigest())
 except FileNotFoundError:item['state']='absent'
 except (OSError,ValueError) as e:item.update(state='unreadable',error=str(e))
 out['results'].append(item)
out['metadata_input_sha256']=hashlib.sha256(data).hexdigest()
print(json.dumps(out,sort_keys=True))
