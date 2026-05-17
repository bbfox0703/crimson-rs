#!/usr/bin/env python3
"""Affine fit using the CE TP marker coords (global frame).

The user's first round used in-game CE-read positions which are
*field-local* — when you fast-travel between regions, the engine
resets the coordinate origin to that sublevel's local frame. The TP
marker values, by contrast, are global (same frame as the save's
`_position`), so a single affine should fit cleanly across all
fields.

Hernand Town's TP value was a copy/paste of Abyss Nexus HH in the
user's data — skipped here pending re-verification.
"""

# (label, TP_X, TP_Z, map_px, map_py)  — Y (height) dropped.
POINTS = [
    ("Char Howling Hill",          -10503.16602, -4375.878418, 1400, 3759),
    ("Abyss Nexus, Howling Hill",  -10531.5918,  -4329.918457, 1381, 3744),
    # ("Abyss Nexus, Hernand Town" — TP value was copy-pasted from Abyss HH; skip)
    ("Abyss Nexus, Witch Woods",   -11517.12207, -4628.442383,  960, 3868),
    ("Abyss Cresset, Five Finger Mtn (NW)", -11746.07227, -153.2326965,  864, 1932),
    ("Abyss Nexus, Coast Windmill (E)",     -4261.412598,  -4629.567383, 4099, 3869),
    ("Abyss Cresset, Trivana Sound (NE)",   -6196.891602,   1010.535034, 3262, 1427),
    ("Abyss Cresset, Mtn of Frozen Souls (SW)", -11610.14746, -6272.822266,  939, 4582),
    ("Abyss Nexus, Vellua",        -10623.20215, -6109.266602, 1344, 4511),
    ("Abyss Nexus, Three Brother's Cliff", -6926.178711, -5265.687988, 2947, 4146),
]


def mat3_inv(m):
    a, b, c = m[0]; d, e, f = m[1]; g, h, i = m[2]
    det = a*(e*i - f*h) - b*(d*i - f*g) + c*(d*h - e*g)
    if abs(det) < 1e-12: return None
    idet = 1.0 / det
    return [
        [(e*i-f*h)*idet, (c*h-b*i)*idet, (b*f-c*e)*idet],
        [(f*g-d*i)*idet, (a*i-c*g)*idet, (c*d-a*f)*idet],
        [(d*h-e*g)*idet, (b*g-a*h)*idet, (a*e-b*d)*idet],
    ]

def mat3_vec(m, v):
    return [sum(m[r][c]*v[c] for c in range(3)) for r in range(3)]

def solve_lstsq_3(rows, rhs):
    n = len(rows)
    ata = [[sum(rows[k][r]*rows[k][c] for k in range(n)) for c in range(3)] for r in range(3)]
    atb = [sum(rows[k][r]*rhs[k] for k in range(n)) for r in range(3)]
    inv = mat3_inv(ata)
    return mat3_vec(inv, atb) if inv else None


def fit(points):
    rows = [[p[1], p[2], 1.0] for p in points]
    mx = [p[3] for p in points]
    my = [p[4] for p in points]
    ax = solve_lstsq_3(rows, mx); ay = solve_lstsq_3(rows, my)
    a, b, tx = ax; c, d, ty = ay
    sum_e2 = 0.0
    residuals = []
    for label, x, z, pmx, pmy in points:
        ex = a*x + b*z + tx - pmx
        ey = c*x + d*z + ty - pmy
        residuals.append((label, x, z, pmx, pmy, a*x+b*z+tx, c*x+d*z+ty, (ex*ex+ey*ey)**0.5))
        sum_e2 += ex*ex + ey*ey
    rmse = (sum_e2 / len(points))**0.5
    return (a, b, c, d, tx, ty), rmse, residuals


print(f"=== Affine fit on {len(POINTS)} TP-marker points (global frame) ===\n")
(a, b, c, d, tx, ty), rmse, residuals = fit(POINTS)
print(f"  map_px = {a:>10.6f}*X + {b:>10.6f}*Z + {tx:>10.2f}")
print(f"  map_py = {c:>10.6f}*X + {d:>10.6f}*Z + {ty:>10.2f}\n")
print(f"  {'point':<48s} {'predicted':>14s} {'observed':>13s} {'err_px':>7s}")
print(f"  {'-'*48} {'-'*14} {'-'*13} {'-'*7}")
residuals.sort(key=lambda r: -r[7])
for label, x, z, mx, my, pmx, pmy, err in residuals:
    print(f"  {label:<48s} ({pmx:>5.0f},{pmy:>5.0f})  ({mx:>5d},{my:>5d}) {err:>7.1f}")
print(f"\n  RMSE: {rmse:.1f} px,  max: {max(r[7] for r in residuals):.1f} px")

# Inverse
det = a*d - b*c
if abs(det) > 1e-12:
    idet = 1.0/det
    ia, ib = d*idet, -b*idet
    ic, idd = -c*idet, a*idet
    itx = -(ia*tx + ib*ty)
    ity = -(ic*tx + idd*ty)
    print(f"\n=== Inverse: pixel -> world (TP frame) ===\n")
    print(f"  X = {ia:>10.6f}*px + {ib:>10.6f}*py + {itx:>10.2f}")
    print(f"  Z = {ic:>10.6f}*px + {idd:>10.6f}*py + {ity:>10.2f}")

# Per-pair scale check (should be uniform now if frame is consistent)
print(f"\n=== Per-pair scale sanity (should be tightly clustered if global frame is consistent) ===")
scales = []
for i, p in enumerate(POINTS):
    for j, q in enumerate(POINTS):
        if i >= j: continue
        wx = p[1] - q[1]; wz = p[2] - q[2]
        wd = (wx*wx + wz*wz)**0.5
        mx_d = p[3] - q[3]; my_d = p[4] - q[4]
        md = (mx_d*mx_d + my_d*my_d)**0.5
        if wd < 0.1: continue
        scales.append(md/wd)
ss = sorted(scales)
print(f"  scale (px/world-unit): min={ss[0]:.3f}, median={ss[len(ss)//2]:.3f}, max={ss[-1]:.3f}, ratio={ss[-1]/ss[0]:.2f}x")

# Per-field offset extraction (in-game CE vs TP marker) — verify these
# are multiples of 1000.
print(f"\n=== In-game CE vs TP marker: per-landmark offset (TP - in_game) ===")
INGAME = [
    ("Char HH",                      -502.6116638,  -373.8612671),
    ("Abyss Nexus, Howling Hill",    -531.5917969,  -329.918457),
    ("Abyss Nexus, Witch Woods",     -517.1220703,  -628.4423828),
    ("Abyss Cresset, Five Finger",   -746.4730225,  -154.1475372),
    ("Abyss Nexus, Coast Windmill",  -261.4125977,  -629.5673828),
    ("Abyss Cresset, Trivana Sound", -196.4550781,    9.634738922),
    ("Abyss Cresset, Frozen Souls",  -610.9794922,  -273.3764648),
    ("Abyss Nexus, Vellua",          -623.2021484,  -109.2666016),
    ("Abyss Nexus, Three Brother's", -926.1787109,  -265.6879883),
]
for (lbl, ix, iz), (_, tx_w, tz_w, _, _) in zip(INGAME, POINTS):
    ox = tx_w - ix; oz = tz_w - iz
    print(f"  {lbl:<35s}  X_offset={ox:>10.2f}  Z_offset={oz:>10.2f}  (rounded to nearest 1000: {round(ox/1000)*1000:>6d}, {round(oz/1000)*1000:>6d})")
