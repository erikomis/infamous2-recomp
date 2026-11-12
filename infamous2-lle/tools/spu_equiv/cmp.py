import sys
a=[l.split() for l in open(sys.argv[1])]; b=[l.split() for l in open(sys.argv[2])]
if len(a)!=len(b): print("LEN", len(a), len(b)); sys.exit()
d=sk=0
for x,y in zip(a,b):
    if x[:2]!=y[:2]: print("ORDER", x, y); sys.exit()
    if x[2].startswith("SKIP") or y[2].startswith("SKIP"): sk+=1; continue
    if x!=y: d+=1
print(f"lines={len(a)} skip={sk} diff={d}")
