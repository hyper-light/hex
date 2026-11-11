import os,pathlib,subprocess,json,hashlib,platform
base=pathlib.Path.home()/'.cache/hex-lead'
proof=base/'root-queue-allocation-09ef773b'
proof.mkdir(exist_ok=False)
binary=base/'queue-allocation-probe-target/debug/deps/hex_node-1028f62b4e94535b'
meta={'source':'09ef773b04161d1431737c6868d631a0aff2b218','binary':str(binary),'sha256':hashlib.file_digest(binary.open('rb'),'sha256').hexdigest(),'platform':platform.platform(),'uname':list(platform.uname()),'driver_pid':os.getpid(),'samples':3,'scope':'actual Queued empty channel constructor; Rust layout and native process time/faults/RSS; not full Service or kernel charge'}
(proof/'meta.json').write_text(json.dumps(meta,indent=2))
test='scheduler::tests::cp_t7_3_actual_queued_channel_allocation_probe'
rows=[]
for peers in [1,3]:
 for sample in range(3):
  for mode in ['bounded','linked']:
   stem=f'{peers}-{sample}-{mode}'
   env=dict(os.environ,HEX_QUEUE_ALLOCATION_MODE=mode,HEX_QUEUE_ALLOCATION_PEERS=str(peers))
   for k in list(env):
    if k.startswith('DYLD_') or k.startswith('HEX_DEBUG_'): env.pop(k,None)
   with (proof/(stem+'.log')).open('w') as out,(proof/(stem+'.time')).open('w') as err:
    p=subprocess.Popen(['/usr/bin/nice','-n','10','/usr/bin/time','-l',str(binary),test,'--exact','--ignored','--test-threads=1','--nocapture'],env=env,stdout=out,stderr=err)
    (proof/(stem+'.pid')).write_text(str(p.pid))
    status=p.wait()
   (proof/(stem+'.exit')).write_text(str(status))
   if status: raise RuntimeError(stem+' exit '+str(status))
   lines=(proof/(stem+'.log')).read_text().splitlines()
   obj=next(json.loads(x[x.index('{"'):]) for x in lines if '{"after_drop"' in x or '{"before_construct"' in x)
   rows.append(dict(peers=peers,sample=sample,mode=mode,result=obj))
(proof/'results.json').write_text(json.dumps(rows,indent=2))
(proof/'driver.exit').write_text('0')
print(json.dumps({'proof':str(proof),'cases':len(rows),'exit':0,'deltas':[{'peers':r['peers'],'sample':r['sample'],'mode':r['mode'],'requested':r['result']['while_held']['requested_bytes']-r['result']['before_construct']['requested_bytes'],'retained':r['result']['after_drop']['live_bytes']-r['result']['before_construct']['live_bytes']} for r in rows]}))
