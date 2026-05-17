#!/usr/bin/env python3
"""Diagnose why the affine fit on the 10 calibration points is so bad.

Steps:
  1. Per-pair scale (px / world_unit) — is the ratio consistent across pairs?
  2. Affine fit on subsets (Abyss Nexus only, nearby cluster only).
  3. Leave-one-out — drop each point in turn, see which one's removal
     most reduces RMSE (= the outlier).
"""
from __future__ import annotations

POINTS = [
    ("Char Howling Hill",          -502.6116638, -373.8612671, 1400, 3759),
    ("Abyss Nexus, Howling Hill",  -531.5917969, -329.918457,  1381, 3744),
    ("Abyss Nexus, Hernand Town",  -626.4335938, -813.2607422, 1350, 3514),
    ("Abyss Nexus, Witch Woods",   -517.1220703, -628.4423828,  960, 3868),
    ("Abyss Cresset, Five Finger Mtn (NW)", -746.4730225, -154.1475372,  864, 1932),
    ("Abyss Nexus, Coast Windmill (E)",     -261.4125977, -629.5673828, 4099, 3869),
    ("Abyss Cresset, Trivana Sound (NE)",   -196.4550781,    9.634738922, 3262, 1427),
    ("Abyss Cresset, Mtn of Frozen Souls (SW)", -610.9794922, -273.3764648,  939, 4582),
    ("Abyss Nexus, Vellua",        -623.2021484, -109.2666016, 1344, 4511),
    ("Abyss Nexus, Three Brother's Cliff", -926.1787109, -265.6879883, 2947, 4146),
]

ABYSS_NEXUS_ONLY = [p for p in POINTS if "Abyss Nexus" in p[0] or "Char" in p[0]]


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
    ata = [[sum(rows[k][r]*rows[k][c] for k in range(len(rows))) for c in range(3)] for r in range(3)]
    atb = [sum(rows[k][r]*rhs[k] for k in range(len(rows))) for r in range(3)]
    inv = mat3_inv(ata)
    if inv is None: return None
    return mat3_vec(inv, atb)


def fit_affine(points):
    rows = [[p[1], p[2], 1.0] for p in points]
    mx = [p[3] for p in points]
    my = [p[4] for p in points]
    ax = solve_lstsq_3(rows, mx)
    ay = solve_lstsq_3(rows, my)
    if ax is None or ay is None:
        return None, None, None, None
    a, b, tx = ax
    c, d, ty = ay
    # Compute per-point residuals + RMSE.
    sum_e2 = 0.0
    residuals = []
    for label, x, z, pmx, pmy in points:
        ex = a*x + b*z + tx - pmx
        ey = c*x + d*z + ty - pmy
        e2 = ex*ex + ey*ey
        residuals.append((label, e2**0.5))
        sum_e2 += e2
    rmse = (sum_e2 / len(points)) ** 0.5
    return (a, b, c, d, tx, ty), rmse, residuals, max(r[1] for r in residuals)


print("=" * 70)
print("ANALYSIS 1: Per-pair scale (px-distance / world-distance)")
print("=" * 70)
print("If world→map is a clean affine, all pairs should give the SAME scale.")
print()
print(f"  {'pair':<55s} {'world_d':>9s} {'map_d':>8s} {'scale':>7s}")
print(f"  {'-'*55} {'-'*9} {'-'*8} {'-'*7}")

scales = []
for i, p in enumerate(POINTS):
    for j, q in enumerate(POINTS):
        if i >= j: continue
        wx = p[1] - q[1]; wz = p[2] - q[2]
        wd = (wx*wx + wz*wz) ** 0.5
        mx = p[3] - q[3]; my = p[4] - q[4]
        md = (mx*mx + my*my) ** 0.5
        if wd < 0.1: continue
        scale = md / wd
        scales.append((scale, p[0], q[0], wd, md))
scales.sort()
print(f"  -- LOWEST scale pairs (likely 'real' world neighbors, tiny world dist) --")
for s, a, b, wd, md in scales[:6]:
    print(f"  {a[:25]:<25s}  vs  {b[:25]:<25s}  {wd:>8.1f}  {md:>7.1f}  {s:>6.2f}")
print(f"  -- HIGHEST scale pairs (suspect: one CE coord is wrong OR field offset differs) --")
for s, a, b, wd, md in scales[-6:]:
    print(f"  {a[:25]:<25s}  vs  {b[:25]:<25s}  {wd:>8.1f}  {md:>7.1f}  {s:>6.2f}")

# Median + extremes
all_s = [s[0] for s in scales]
print(f"\n  scale stats: min={min(all_s):.2f}, median={sorted(all_s)[len(all_s)//2]:.2f}, max={max(all_s):.2f}, ratio_max_min={max(all_s)/min(all_s):.1f}x")

print()
print("=" * 70)
print("ANALYSIS 2: Affine fits on different subsets")
print("=" * 70)
for label, subset in [
    ("All 10 points", POINTS),
    ("Abyss Nexus only (7 points)", ABYSS_NEXUS_ONLY),
]:
    print(f"\n--- {label} ---")
    coefs, rmse, residuals, max_e = fit_affine(subset)
    if coefs is None:
        print("  (singular — skipped)")
        continue
    a, b, c, d, tx, ty = coefs
    print(f"  map_px = {a:>10.4f}*X + {b:>10.4f}*Z + {tx:>9.2f}")
    print(f"  map_py = {c:>10.4f}*X + {d:>10.4f}*Z + {ty:>9.2f}")
    print(f"  RMSE = {rmse:.1f} px,  max residual = {max_e:.1f} px")
    print(f"  per-point residuals:")
    residuals.sort(key=lambda r: -r[1])
    for label2, err in residuals:
        print(f"    {label2:<48s}  {err:>8.1f} px")

print()
print("=" * 70)
print("ANALYSIS 3: Leave-one-out — which point most reduces RMSE when dropped?")
print("=" * 70)
loo = []
for i, drop in enumerate(POINTS):
    subset = [p for j, p in enumerate(POINTS) if j != i]
    coefs, rmse, _, max_e = fit_affine(subset)
    loo.append((rmse, drop[0]))
loo.sort()
print(f"\n  RMSE after dropping each point (ascending — first row = drop this for biggest improvement):")
for rmse, label in loo:
    print(f"    drop {label:<45s} -> RMSE = {rmse:.1f} px")
