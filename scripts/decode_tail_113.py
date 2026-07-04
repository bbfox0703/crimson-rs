"""Decode the 1.13 relocated tail block (merged prefab+gimmick list) using the
CURRENT parser's exact consumed offset as a reliable boundary (leftover =
chunk[consumed:]). Tests the unified-element hypothesis against all 6508 items
and reports count-vs-(prefab+gimmick) agreement + remainder distribution.

Unified element hypothesis:
  { scale:[f32;3], prefab_names:CArray<u32>, animation_path_list:CArray<u32>,
    equip_slot_list:CArray<u16>, tribe_gender_list:CArray<u32>, flag:u8 }
tail = CArray<Unified> then <remainder> (characterised).
"""
from __future__ import annotations
import os, struct, sys, json
from collections import Counter
from pathlib import Path
REPO = Path('.').resolve(); sys.path.insert(0, str(REPO)); import crimson_rs
new = (REPO/"out"/"iteminfo.pabgb").read_bytes()
ref = json.loads((REPO/"out"/"ref_112.json").read_text())

def _id(b): return b in (0x5f,0x20) or 48<=b<=57 or 65<=b<=90 or 97<=b<=122 or b>=0x80
def _st(d,o,k):
    if o+12>len(d): return False
    kk,sl=struct.unpack_from('<II',d,o)
    if kk!=k or not(2<=sl<=128) or o+8+sl+1>len(d): return False
    if any(not _id(b) for b in d[o+8:o+8+sl]): return False
    return d[o+8+sl]==0
keys=[int(x) for x in (REPO/'data'/'keys.txt').read_text().split() if x.strip()]
na=[0]; last=0
for i in range(1,len(keys)):
    cur=last+60; tgt=struct.pack('<I',keys[i]); f=-1
    while cur+12<=len(new):
        idx=new.find(tgt,cur)
        if idx<0: break
        if _st(new,idx,keys[i]): f=idx; break
        cur=idx+1
    na.append(f if f>=0 else None)
    if f>=0: last=f
nx=[None]*len(na); L=len(new)
for i in range(len(na)-1,-1,-1):
    if na[i] is not None: nx[i]=L; L=na[i]
npos={keys[i]:(na[i],nx[i]) for i in range(len(keys)) if na[i] is not None}

class R:
    def __init__(s,b): s.b=b; s.o=0
    def u8(s): v=s.b[s.o]; s.o+=1; return v
    def u16(s): v=struct.unpack_from('<H',s.b,s.o)[0]; s.o+=2; return v
    def u32(s): v=struct.unpack_from('<I',s.b,s.o)[0]; s.o+=4; return v
    def arru32(s): return [s.u32() for _ in range(s.u32())]
    def arru16(s): return [s.u16() for _ in range(s.u32())]
def read_unified(r):
    r.o+=12; r.arru32(); r.arru32(); r.arru16(); r.arru32(); r.o+=3  # 3 trailing bytes

def refcount(key):
    r=ref.get(str(key))
    if not r: return None
    pc=sum(1 for x in r['ranges'] if x[0].endswith('.is_craft_material'))
    gc=sum(1 for x in r['ranges'] if x[0].endswith('.use_gimmick_prefab'))
    return pc,gc

ok=0; cntok=0; fail=0; rem=Counter(); mism=[]; failex=[]
for k in keys:
    if k not in npos: continue
    nlo,nhi=npos[k]; chunk=new[nlo:nhi]
    res=crimson_rs.parse_iteminfo_tracked(chunk)
    if not res['spans']: continue
    consumed=res['spans'][0]['end']
    tail=chunk[consumed:]
    rc=refcount(k)
    try:
        r=R(tail); c=r.u32()
        for _ in range(c): read_unified(r)
        rem[len(tail)-r.o]+=1
        ok+=1
        if rc and c==rc[0]+rc[1]: cntok+=1
        elif rc and len(mism)<8: mism.append((k,c,rc))
    except Exception as e:
        fail+=1
        if len(failex)<6: failex.append((k,rc,len(tail),str(e)[:40],tail[:48].hex()))
print(f"tail merged-decode ok={ok} fail={fail}  count==prefab+gimmick: {cntok}/{ok}")
print("remainder after merged_list:", dict(sorted(rem.items())))
print("count mismatches:", mism)
print("decode failures:")
for e in failex: print("  ",e)
