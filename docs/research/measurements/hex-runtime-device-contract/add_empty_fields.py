#!/usr/bin/env python3
"""Mechanical empty fields for pre-device literals; never change struct updates or model types."""
import json
from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[4]
# Hide literals/comments while retaining character offsets and brace nesting.
tokens = re.compile(r'//[^\n]*|/\*.*?\*/|r(?P<hashes>\#*)".*?"(?P=hashes)|"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])\'', re.S)
manifest = []
for path in sorted((root / 'crates').rglob('*.rs')):
    rel = str(path.relative_to(root))
    if rel.startswith('crates/hex-sim/') or rel == 'crates/hex-api/src/devices.rs':
        continue
    source = path.read_text()
    masked = tokens.sub(lambda m: re.sub(r'[^\n]', ' ', m.group()), source)
    changes = []
    for match in re.finditer(r'\b(BoxSpec|Capacity)\s*\{', masked):
        prefix = masked[max(0, match.start()-100):match.start()]
        if re.search(r'(?:\bstruct|\bimpl|->)\s*(?:(?:\w+)::)*\s*$', prefix):
            continue
        opening = match.end()-1
        depth, at = 1, opening+1
        top = []
        while depth:
            c = masked[at]
            if depth == 1:
                top.append(c)
            if c == '{':
                depth += 1
            elif c == '}':
                depth -= 1
            at += 1
        fields = ''.join(top)
        if re.search(r'\bdevices\b|\.\.', fields):
            continue
        if rel == 'crates/hex-runtime/src/guest/rewrite.rs':
            raise ValueError('rewrite must preserve device claims explicitly')
        indent = re.match(r'\s*', source[source.rfind('\n',0,match.start())+1:match.start()]).group()
        insertion = '\n' + indent + '    devices: Vec::new(),'
        changes.append((opening+1,insertion))
        manifest.append({'path':rel,'type':match.group(1),'original_line':source.count('\n',0,match.start())+1})
    if '--write' in sys.argv:
        for at, insertion in reversed(changes):
            source = source[:at] + insertion + source[at:]
        if changes:
            path.write_text(source)
if '--write' in sys.argv:
    (Path(__file__).parent / 'empty-literals.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps({'literal_count':len(manifest),'paths':sorted({m['path'] for m in manifest})},indent=2))
