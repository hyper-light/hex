from pathlib import Path
from collections import Counter
import hashlib, importlib.util, json, stat, sys

h = Path.home() / '.cache/hex-lead'
parent = Path.home() / '.cache/hx'
revision = '8767f0d9c03f37c64c9c86301bceaa878f7eb495'
profile = sys.argv[1]
assert profile in ('debug', 'release')
summary_file = h / f'capacity-8767f0d9-physical-smoke/physical-{profile}-n1000.json'
summary_row = json.loads(summary_file.read_text())
assert summary_row['exit'] == 0 and summary_row['status'] == 'passed'
dirname = Path(summary_row['result']).parent.name
root = parent / dirname
spec = importlib.util.spec_from_file_location('root_cold', h / 'snapshots/cold-verifier-root-1bb73f74/verify_cold_census.py')
c = importlib.util.module_from_spec(spec)
spec.loader.exec_module(c)
v = c.shared
maximum, _ = v.protocol(h / 'snapshots/capacity-8767f0d9')
reader = c.Reader(root)
r = reader.json(root / 'result.json', maximum)
assert r['kind'] == 'actual-scheduler-seeded-validation' and r['status'] == 'passed'
assert hashlib.sha256((root / 'result.json').read_bytes()).hexdigest() == summary_row['sha256']
assert r['source_revision'] == revision and r['profile'] == profile and r['wait_mode'] == 'armed' and r['n'] == 1000
assert r['verified_physical_created'] == r['verified_physical_deleted'] == 2000 and r['backend_baseline_restored']
assert r['comparison_or_performance_claim'] is False
assert not any(r.get(k) for k in ('error', 'cleanup_error', 'backend_cleanup_error'))
assert r['census']['enabled'] is False and r['census']['files'] == 0
folder = root / 'census'
assert stat.S_ISDIR(folder.lstat().st_mode) and not list(folder.iterdir())
runtime = reader.json(root / 'runtime.json', maximum)
assert runtime['root'] == str(root) and runtime['source_revision'] == revision and runtime['wait_mode'] == 'armed'
assert runtime['max_frame'] == maximum and runtime['ledger'] == str(root / 'pids.jsonl')
retirement = v.retirement(root, r, maximum)
assert retirement['recorded_terminal'] == 'Stopped' and not r['owned_guest_retired'].get('already_gone')
receipt = reader.json(root / 'owned-guest.json', maximum)
record = reader.read(Path(receipt['record']), maximum)
assert receipt['record'] == str(root / 'real.node-state/@guest') and receipt.get('owned_vsock_root') is None
assert hashlib.sha256(record).hexdigest() == retirement['record_sha256']
env, artifacts = r['environment'], r['binary_and_artifact_hashes']
config = receipt['declared_config']
assert config == {'binary': artifacts['shards']['path'], 'kernel': artifacts['kernel']['path'], 'initrd': artifacts['initrd']['path'], 'vcpus': env['guest_cpus'], 'memory_mib': env['guest_memory_mib'], 'shares': [{'dir': str(root / 'guest-shares'), 'read_only': True}]}
assert env['guest_shares_sha256'] == hashlib.sha256((str(root / 'guest-shares') + ':ro').encode()).hexdigest()
rows, controls, kinds, producers, reaped = 0, [], Counter(), [], []
for rows, line in c.bounded_lines(reader, root / 'pids.jsonl', 1048576, maximum):
    assert line.endswith(b'\n')
    row = v.decode_json(line)
    assert row['run'] == str(root) and row.get('census') is None
    kinds[row['kind']] += 1
    if row['kind'] == 'verified-physical-lifecycle':
        controls.append(row)
    elif row['kind'] == 'real':
        producers.append((rows, row))
    elif row['kind'] == 'reaped':
        reaped.append((rows, row))
assert len(producers) == 1
position, producer = producers[0]
assert producer['parent'] == producer['recorded_by']
closure = [x for n, x in reaped if n > position and x['pid'] == producer['pid'] and x['parent'] == producer['parent'] and x['recorded_by'] == producer['recorded_by'] and type(x.get('returncode')) is int]
assert closure
counts, keys, physical = Counter(), set(), set()
for row in controls:
    phase, key = row['phase'], row['key']
    assert phase in ('local', 'remote') and set(key) == {'owner', 'epoch', 'seq'}
    assert all(type(x) is int and x >= 0 for x in key.values()) and key['owner'] == 0 and key['epoch'] == 1
    identity_key = (phase, key['owner'], key['epoch'], key['seq'])
    assert identity_key not in keys and row['physical'] not in physical
    keys.add(identity_key); physical.add(row['physical'])
    realm = root.name + ('-local' if phase == 'local' else '-peer')
    assert row['physical'] == f"hex-place-{hashlib.sha256(realm.encode()).hexdigest()}-{key['owner']}-{key['epoch']}-{key['seq']}"
    counts[phase] += 1
assert dict(counts) == {'local': 1000, 'remote': 1000} and len(keys) == len(physical) == 2000
assert all({key[3] for key in keys if key[0] == phase} == set(range(1000)) for phase in ('local', 'remote'))

def external_meta(path):
    path = Path(path); digest, total = hashlib.sha256(), 0
    with v.input_file(h, path) as (source, size):
        for chunk in iter(lambda: source.read(1024 * 1024), b''):
            digest.update(chunk); total += len(chunk)
    assert total == size
    return {'path': str(path), 'bytes': total, 'sha256': digest.hexdigest()}

external = []
for item in [*artifacts.values(), *r['receipts'].values()]:
    actual = external_meta(item['path']); assert actual['sha256'] == item['sha256']; external.append(actual)
logs, diagnostic_markers, expired = [], 0, 0
for path in sorted(root.glob('*.log')):
    digest, size = hashlib.sha256(), 0
    with v.input_file(root, path) as (source, expected_size):
        for raw in source:
            assert len(raw) <= maximum
            digest.update(raw); size += len(raw)
            diagnostic_markers += b'[DEBUG-submit-df534d2f]' in raw
            expired += raw.startswith(b'hex schedule: waiting')
    assert size == expected_size
    reader.remember(path, digest.hexdigest(), size)
    logs.append({'path': str(path), 'bytes': size, 'sha256': digest.hexdigest()})
assert diagnostic_markers == expired == 0
full = {'kind': 'root-capacity-derived-armed-physical-reconstruction', 'status': 'consistent', 'source_revision': revision, 'profile': profile, 'raw_result': str(root / 'result.json'), 'physical_phase_counts': dict(counts), 'unique_phase_keys': len(keys), 'unique_physical_ids': len(physical), 'backend_baseline_restored': True, 'ledger_rows': rows, 'ledger_kinds': dict(kinds), 'identity': {k: retirement[k] for k in ('pid','started','vsock')}, 'retirement': r['owned_guest_retired'], 'declared_config': config, 'ordered_backend_reap': closure, 'actual_census_directory_entries': 0, 'diagnostic_markers': diagnostic_markers, 'authority_inputs': reader.inputs, 'external_inputs': external, 'logs': logs, 'scope': 'All ledger rows, physical phase keys/IDs/counts/namespaces, exact receipt/record/Stopped, declared configuration, ordered saved backend-Popen reap, artifact/receipt/log hashes checked. No independent live API query, no leak/timing/seed/performance or original Submit cause claim.'}
output = h / f'capacity-8767f0d9-physical-smoke/root-physical-{profile}-n1000-verified.json'
with output.open('x') as target:
    json.dump(full, target, indent=2, sort_keys=True); target.write('\n')
summary = {k: full[k] for k in full if k not in ('authority_inputs','external_inputs','logs','declared_config')}
summary.update({'reconstruction': external_meta(output), 'raw_result_sha256': reader.inputs['result.json']['sha256'], 'authority_inputs_hashed': len(reader.inputs), 'external_inputs_hashed': len(external), 'logs_hashed': len(logs)})
summary_path = output.with_name(f'root-physical-{profile}-n1000-summary.json')
with summary_path.open('x') as target:
    json.dump(summary, target, indent=2, sort_keys=True); target.write('\n')
print(json.dumps(summary, sort_keys=True))
