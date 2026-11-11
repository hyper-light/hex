import re,json
from pathlib import Path

def masked(text):
 out=list(text);i=0;n=len(text)
 def hide(a,b):
  for k in range(a,b):
   if out[k]!='\n':out[k]=' '
 while i<n:
  if text.startswith('//',i):
   end=text.find('\n',i);end=n if end<0 else end;hide(i,end);i=end;continue
  if text.startswith('/*',i):
   start=i;i+=2;depth=1
   while i<n and depth:
    if text.startswith('/*',i):depth+=1;i+=2
    elif text.startswith('*/',i):depth-=1;i+=2
    else:i+=1
   hide(start,i);continue
  raw=re.match(r'(?:br|r)(#*)"',text[i:]) if i==0 or not(text[i-1].isalnum() or text[i-1]=='_') else None
  if raw:
   end=text.find('"'+raw.group(1),i+raw.end());end=n if end<0 else end+1+len(raw.group(1));hide(i,end);i=end;continue
  if text[i]=='"':
   start=i;i+=1
   while i<n:
    if text[i]=='\\':i+=2
    elif text[i]=='"':i+=1;break
    else:i+=1
   hide(start,i);continue
  char=re.match(r"'(?:\\u\{[0-9a-fA-F_]+\}|\\.|[^'\\\n])'",text[i:])
  if char:end=i+char.end();hide(i,end);i=end;continue
  i+=1
 return ''.join(out)

entries=[]
for path in sorted(Path('crates').rglob('*.rs')):
 text=path.read_text();mask=masked(text);patches=[]
 for match in re.finditer(r'\b(NodeSpec|Msg::Start)\s*\{',mask):
  kind=match.group(1);op=match.end()-1;prefix=mask[:match.start()].rstrip()
  if re.search(r'\b(?:struct|impl|for)\s*$',prefix) or re.search(r'->\s*(?:\w+::)*$',prefix):continue
  depth=1;i=op+1;top=[]
  while i<len(mask) and depth:
   ch=mask[i]
   if ch in '{([':depth+=1
   elif ch in '})]':depth-=1
   elif depth==1:top.append((i,ch))
   i+=1
  if depth:raise ValueError('unbalanced '+str(path))
  body=''.join(ch for _,ch in top);after=mask[i:].lstrip()
  if after.startswith('=>') or after.startswith('=') or '..' in body:continue
  field='costs' if kind=='NodeSpec' else 'allocation'
  if re.search(r'\b'+field+r'\b\s*(?::|,|$)',body):continue
  if kind=='NodeSpec' and not re.search(r'\bcapacity\s*[:,]',body):continue
  count='crate::model::Class::COUNT' if str(path).startswith('crates/hex-sim/') else 'hex_sim::model::Class::COUNT'
  value='[None; '+count+']' if kind=='NodeSpec' else 'None'
  raw=text[op+1:i-1];indent=re.match(r'\n([ \t]*)\S',raw)
  insertion=('\n'+indent.group(1)+field+': '+value+',') if indent else (' '+field+': '+value+',')
  patches.append((op+1,insertion,kind,text.count('\n',0,match.start())+1))
 if patches:
  for pos,add,_,_ in reversed(patches):text=text[:pos]+add+text[pos:]
  path.write_text(text);entries.append(dict(path=str(path),node_spec=sum(k=='NodeSpec' for _,_,k,_ in patches),start=sum(k=='Msg::Start' for _,_,k,_ in patches),lines=[line for _,_,_,line in patches]))
Path('docs/research/measurements/hex-completion-root/cost-source/admission-literals.json').write_text(json.dumps(entries,indent=2)+'\n')
print(json.dumps(entries,indent=2))
