#!/usr/bin/env python3
"""Fit affine on just the Hernand-region cluster — points the user
collected without crossing field boundaries. Hypothesis: these all
live in FieldInfoKey=1 and their CE coords are in a single frame, so
an affine fit should converge cleanly here even though the cross-field
fit doesn't."""

# Same data, restricted to the Hernand area.
HERNAND_CLUSTER_5 = [
    ("Char Howling Hill",         -502.6116638, -373.8612671, 1400, 3759),
    ("Abyss Nexus, Howling Hill", -531.5917969, -329.918457,  1381, 3744),
    ("Abyss Nexus, Hernand Town", -626.4335938, -813.2607422, 1350, 3514),
    ("Abyss Nexus, Witch Woods",  -517.1220703, -628.4423828,  960, 3868),
    ("Abyss Nexus, Vellua",       -623.2021484, -109.2666016, 1344, 4511),
]

# Drop Witch Woods — biggest outlier in the 5-point fit, likely
# because the user's CE coord was read at a different sub-point of
# the woods than where they marked the pixel coord.
HERNAND_CLUSTER_4 = [p for p in HERNAND_CLUSTER_5 if "Witch" not in p[0]]


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


def run_fit(name, pts):
    print(f"\n\n=== {name} ({len(pts)} points) ===\n")
    rows = [[p[1], p[2], 1.0] for p in pts]
    mxs = [p[3] for p in pts]
    mys = [p[4] for p in pts]
    a, b, tx = solve_lstsq_3(rows, mxs)
    c, d, ty = solve_lstsq_3(rows, mys)
    print(f"  map_px = {a:>10.4f}*X + {b:>10.4f}*Z + {tx:>9.2f}")
    print(f"  map_py = {c:>10.4f}*X + {d:>10.4f}*Z + {ty:>9.2f}\n")
    print(f"  {'point':<32s} {'predicted':>16s} {'observed':>14s} {'err_px':>8s}")
    print(f"  {'-'*32} {'-'*16} {'-'*14} {'-'*8}")
    sum_e2 = 0.0
    max_e = 0.0
    for label, x, z, mx, my in pts:
        pmx = a*x + b*z + tx
        pmy = c*x + d*z + ty
        err = ((pmx-mx)**2 + (pmy-my)**2)**0.5
        sum_e2 += (pmx-mx)**2 + (pmy-my)**2
        max_e = max(max_e, err)
        print(f"  {label:<32s} ({pmx:>5.0f}, {pmy:>5.0f})  ({mx:>5d}, {my:>5d})  {err:>7.1f}")
    rmse = (sum_e2 / len(pts))**0.5
    print(f"\n  RMSE: {rmse:.1f} px,  max: {max_e:.1f} px")
    return a, b, c, d, tx, ty

run_fit("Hernand cluster (5 incl. Witch Woods)", HERNAND_CLUSTER_5)
a, b, c, d, tx, ty = run_fit("Hernand cluster (4 — Witch Woods dropped)", HERNAND_CLUSTER_4)

print(f"\n  scale (px / world unit) ~ {(a*a + c*c)**0.5:.3f} (along X), {(b*b + d*d)**0.5:.3f} (along Z)")

# Inverse for the 4-point fit
det = a*d - b*c
if abs(det) > 1e-12:
    idet = 1.0/det
    ia, ib = d*idet, -b*idet
    ic, idd = -c*idet, a*idet
    itx = -(ia*tx + ib*ty)
    ity = -(ic*tx + idd*ty)
    print(f"\n=== Inverse: pixel -> world ===\n")
    print(f"  X = {ia:>10.4f}*px + {ib:>10.4f}*py + {itx:>9.2f}")
    print(f"  Z = {ic:>10.4f}*px + {idd:>10.4f}*py + {ity:>9.2f}")
