"""1.13 partprefabdyeslotinfo per-slot RE / validator.

1.13's "expanded dyeable equipment" grew the table 968 -> 1,538 rows and
drifted the per-slot record layout for the new gear: the 5-byte field the
1.12 parser blindly skips (`u8 + u32`, uniformly `(0xFF, 0)` in 1.12) is
actually `u8 + u32(extra_layer_count)`. New gear rows set the count to 1,
adding a second material/dye layer before the slot's tail name.

Enhanced slot model tested here:
    Slot = mat_indices[3] + cstr*3 + mask[3] + u8 + u32(N)
           + N * ExtraLayer + tail_cstr
    ExtraLayer = cstr*3 + mask[3] + u8

The script parses EVERY row of the live 1.13 table under this model and
reports exact-consume coverage (target 1,538/1,538), so the Rust change
can be made with confidence.

HISTORICAL: the model above is 1.13's. **2.01 widened `mask` from 3 bytes
to 12** in both the slot and the extra layer, so against a 2.01 install
this reports 0/1,626 — that is the script working as written, not a
regression. `crate::part_prefab_dye_slot_info` carries the current model
(and still reads the 1.13 one); update `mask[3]` here to `mask[12]` if you
need this validator against a live 2.01+ table.
"""
from __future__ import annotations
import struct, sys
from pathlib import Path
REPO = Path('.').resolve(); sys.path.insert(0, str(REPO)); import crimson_rs

sys.path.insert(0, str(REPO / "scripts"))
from gamedata_layout import resolve_bin_layout

GAME = r"D:\SteamLibrary\steamapps\common\Crimson Desert"
GROUP = "0008"
# 2.01 renamed the directory and the .pabgb/.pabgh extensions; `stem` here
# is the bare table name and the layout supplies the rest.
LAYOUT = resolve_bin_layout(GAME, GROUP)

def extract(stem, header=False):
    name = LAYOUT.header(stem) if header else LAYOUT.body(stem)
    return bytes(crimson_rs.extract_file(GAME, GROUP, LAYOUT.dir, name))

def parse_pabgh(pabgh):
    """u16 count + (u32 key, u32 offset)* -> list[(key, offset)]."""
    count = struct.unpack_from("<H", pabgh, 0)[0]
    assert 2 + 8 * count == len(pabgh), f"unexpected pabgh shape: {len(pabgh)} vs count {count}"
    return [struct.unpack_from("<II", pabgh, 2 + i * 8) for i in range(count)]

class R:
    def __init__(s, b): s.b = b; s.o = 0
    def u8(s): v = s.b[s.o]; s.o += 1; return v
    def u32(s): v = struct.unpack_from("<I", s.b, s.o)[0]; s.o += 4; return v
    def m3(s): v = s.b[s.o:s.o+3]; s.o += 3; return v
    def cstr(s):
        l = s.u32()
        if s.o + l > len(s.b) or l > 256:
            raise ValueError("bad cstr len")
        v = s.b[s.o:s.o+l]; s.o += l
        return v.decode("utf-8")

def read_slot(r):
    r.m3()                       # mat_indices
    r.cstr(); r.cstr(); r.cstr() # 3 default materials
    r.m3()                       # mask
    r.u8()                       # 0xff marker
    n = r.u32()                  # extra_layer_count (0 in 1.07-1.12; 1 for new 1.13 gear)
    if n > 8:
        raise ValueError(f"implausible extra_layer_count {n}")
    for _ in range(n):
        r.cstr(); r.cstr(); r.cstr()  # extra-layer materials
        r.m3()                        # extra-layer mask
        r.u8()                        # extra-layer flag
    r.cstr()                     # tail name (mesh / .pac path)

def try_row(body, expected_key):
    if len(body) < 17:
        return False
    key = struct.unpack_from("<I", body, 0)[0]
    if key != expected_key:
        return False
    slot_count = struct.unpack_from("<I", body, 9)[0]
    if slot_count == 0 or slot_count > 64:
        return False
    r = R(body); r.o = 13
    try:
        r.cstr()                 # prefab_name
        for _ in range(slot_count):
            read_slot(r)
    except (ValueError, IndexError, struct.error):
        return False
    return r.o == len(body)

def main():
    pabgb = extract("partprefabdyeslotinfo")
    pabgh = extract("partprefabdyeslotinfo", header=True)
    index = parse_pabgh(pabgh)
    offsets = [off for _, off in index] + [len(pabgb)]
    ok = 0; fail = []
    for i, (key, off) in enumerate(index):
        body = pabgb[off:offsets[i+1]]
        if try_row(body, key):
            ok += 1
        else:
            fail.append(key)
    print(f"rows: {len(index)}  enhanced-model exact-consume: {ok}  fail: {len(fail)}")
    if fail:
        print("failing keys:", [f"0x{k:08x}" for k in fail[:20]])
    # confirm the 9 previously-dropped keys now parse
    prev = [0x54534e48,0xe0bffb36,0xb2cc6efa,0x625369c0,0x8cba6493,0x199ceacd,0xac8a6ab6,0xbffdd4e0,0x5ed0a80e]
    idx = {k: off for k, off in index}
    off_by_next = {off: offsets[i+1] for i, (_, off) in enumerate(index)}
    for k in prev:
        if k in idx:
            body = pabgb[idx[k]:off_by_next[idx[k]]]
            print(f"  prev-drop 0x{k:08x}: {'OK' if try_row(body, k) else 'STILL FAILS'}")

if __name__ == "__main__":
    main()
