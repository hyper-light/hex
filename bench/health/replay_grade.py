#!/usr/bin/env python3
"""Ada's cost rule (2026-11-06) tried on the peer grade (F2), on the gray nemesis's model: does the
probe's form carry over? Each window, a node's ratio x is its median slowdown over its peers'. The
grade would choose, from the healthy ratios learned so far, the bar x and the run length k minimizing
(1 - pi) * S(x)^k * C + pi * k * W * (x - 1): a healthy node k windows in a row above x is a false drain
costing C per workload; a slow node waited on for k windows costs each workload its excess, at least
x - 1, per window W; pi the Jeffreys posterior of a slow node per window, 1/2 / (n + 1) over n
node-windows, since the fleet has no recorded gray incidents. Suspect past the largest healthy ratio.
A run that ends without a drain is learned back as healthy (Karn's rule).

Usage: replay_grade.py <seeds>"""
import bisect
import math
import random
import statistics as st
import sys


def best(healthy, n, C, W=1.0):
    N=len(healthy); pi=0.5/(n+1); best=None
    for i,x in enumerate(healthy):
        if x<=1.0: continue
        a=N-1-i; S=(a+0.5)/(N+1)
        A=(1-pi)*C; B=pi*W*(x-1)
        ks={1}
        if S<1:
            k=math.log(B/(A*-math.log(S)))/math.log(S) if A*-math.log(S)>B else 1
            ks|={max(1,math.floor(k)),max(1,math.ceil(k))}
        for k in ks:
            l=A*S**k+B*k
            if best is None or l<best[0]: best=(l,x,k)
    return best
def run(seed,C):
    r=random.Random(seed); nodes=r.randint(4,200); onset=r.randint(5,20); f=r.uniform(2.5,8)
    healthy=[]; n=0; run_={}; held={}; fs=fd=0; det=iso=None
    for w in range(onset+10):
        med=[st.median(r.uniform(0.7,1.5)*(f if (i==0 and w>=onset) else 1) for _ in range(10)) for i in range(nodes)]
        peers=st.median(med); b=best(healthy,n,C) if healthy else None
        top=healthy[-1] if healthy else None
        for i,m in enumerate(med):
            x=m/peers; bad=(i==0 and w>=onset)
            if b and x>b[1]:
                run_[i]=run_.get(i,0)+1; held.setdefault(i,[]).append(x)
                sus = x>top or run_[i]>=b[2]
                if sus and not bad and run_[i]==1 and x>top: fs+=1
                if bad and sus and det is None: det=w-onset+1
                if run_[i]==b[2]:
                    held[i]=[]
                    if bad and iso is None: iso=w-onset+1
                    elif not bad: fd+=1
            else:
                for y in held.pop(i,[]): bisect.insort(healthy,y)
                run_[i]=0; bisect.insort(healthy,x)
            n+=1
    return fs,fd,det,iso
for C in (0.002,0.194):
    t=[run(s,C) for s in range(int(sys.argv[1]))]
    dets=[x[2] for x in t]; isos=[x[3] for x in t]
    print(f"C={C*1e3:.0f} ms: false suspects={sum(x[0] for x in t)} false drains={sum(x[1] for x in t)} undetected={dets.count(None)} undrained={isos.count(None)} drained within 4 windows={sum(1 for i in isos if i and i<=4)}")
