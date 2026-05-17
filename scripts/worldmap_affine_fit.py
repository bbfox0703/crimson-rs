#!/usr/bin/env python3
"""Solve the affine fit world (X, Z) -> map (px, py) from the 10
user-provided calibration points. Outputs the 6 coefficients +
per-point residuals + the inverse transform (so the editor can also
map a clicked pixel back to a world coordinate)."""

from __future__ import annotations

# (label, world_X, world_Z, map_px, map_py)
# Y (height) is dropped — we project top-down onto X/Z.
# Silver Wolf Mountain was excluded per user request.
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


def mat3_inv(m):
    """Invert a 3x3 matrix (rows-as-lists). Returns None if singular."""
    a, b, c = m[0]
    d, e, f = m[1]
    g, h, i = m[2]
    det = a*(e*i - f*h) - b*(d*i - f*g) + c*(d*h - e*g)
    if abs(det) < 1e-12:
        return None
    inv_det = 1.0 / det
    return [
        [(e*i - f*h) * inv_det, (c*h - b*i) * inv_det, (b*f - c*e) * inv_det],
        [(f*g - d*i) * inv_det, (a*i - c*g) * inv_det, (c*d - a*f) * inv_det],
        [(d*h - e*g) * inv_det, (b*g - a*h) * inv_det, (a*e - b*d) * inv_det],
    ]


def mat3_vec(m, v):
    return [sum(m[r][c] * v[c] for c in range(3)) for r in range(3)]


def solve_lstsq_3(rows, rhs):
    """Least-squares solve for [a, b, c] in A @ [a,b,c]^T = rhs.
    rows: list of [X, Z, 1] triples. rhs: list of target values."""
    n = len(rows)
    # A^T A (3x3)
    ata = [[0.0]*3 for _ in range(3)]
    for r in range(3):
        for c in range(3):
            ata[r][c] = sum(rows[k][r] * rows[k][c] for k in range(n))
    # A^T b (3,)
    atb = [sum(rows[k][r] * rhs[k] for k in range(n)) for r in range(3)]
    inv = mat3_inv(ata)
    if inv is None:
        raise RuntimeError("singular A^T A — collinear points?")
    return mat3_vec(inv, atb)


def main():
    rows = [[p[1], p[2], 1.0] for p in POINTS]
    map_x = [p[3] for p in POINTS]
    map_y = [p[4] for p in POINTS]

    a, b, tx = solve_lstsq_3(rows, map_x)
    c, d, ty = solve_lstsq_3(rows, map_y)

    print("=== Affine fit: (world_X, world_Z) -> (map_px, map_py) ===\n")
    print(f"  map_px = {a:>12.6f} * X + {b:>12.6f} * Z + {tx:>12.6f}")
    print(f"  map_py = {c:>12.6f} * X + {d:>12.6f} * Z + {ty:>12.6f}\n")

    # Per-point residuals
    print(f"  {'point':<45s} {'predicted':>14s} {'observed':>12s} {'err_px':>10s}")
    print(f"  {'-'*45} {'-'*14} {'-'*12} {'-'*10}")
    max_err = 0.0
    sum_err2 = 0.0
    for label, x, z, mx, my in POINTS:
        pmx = a*x + b*z + tx
        pmy = c*x + d*z + ty
        ex = pmx - mx
        ey = pmy - my
        err = (ex*ex + ey*ey) ** 0.5
        max_err = max(max_err, err)
        sum_err2 += ex*ex + ey*ey
        print(f"  {label:<45s} ({pmx:>5.0f},{pmy:>5.0f})  ({mx:>5d},{my:>5d})  {err:>10.1f}")
    rmse = (sum_err2 / len(POINTS)) ** 0.5
    print(f"\n  RMSE: {rmse:.1f} px,  max residual: {max_err:.1f} px,  map size: 5178 x 5240")
    print(f"  fractional error: {rmse / max(5178, 5240) * 100:.2f}% of map dim")

    # Inverse transform (for clicks on map → world)
    # We have:  [map_x]   [a  b]   [X]    [tx]
    #           [map_y] = [c  d] · [Z]  + [ty]
    # Inverse: [X]   [det^-1] [ d  -b] [map_x - tx]
    #          [Z] =          [-c   a] [map_y - ty]
    det = a*d - b*c
    if abs(det) < 1e-12:
        print("\n  WARNING: 2x2 part is singular — cannot invert")
    else:
        idet = 1.0 / det
        ia, ib = d * idet, -b * idet
        ic, idd = -c * idet, a * idet
        itx = -(ia * tx + ib * ty)
        ity = -(ic * tx + idd * ty)
        print(f"\n=== Inverse: (map_px, map_py) -> (world_X, world_Z) ===\n")
        print(f"  X = {ia:>12.6f} * map_px + {ib:>12.6f} * map_py + {itx:>12.6f}")
        print(f"  Z = {ic:>12.6f} * map_px + {idd:>12.6f} * map_py + {ity:>12.6f}")

    # Also report what the X/Z range covered by the calibration points looks like
    print(f"\n=== Calibration coverage ===")
    xs = [p[1] for p in POINTS]
    zs = [p[2] for p in POINTS]
    print(f"  world X range: [{min(xs):.1f}, {max(xs):.1f}]  (span {max(xs)-min(xs):.1f})")
    print(f"  world Z range: [{min(zs):.1f}, {max(zs):.1f}]  (span {max(zs)-min(zs):.1f})")
    print(f"  map_px range: [{min(map_x)}, {max(map_x)}]")
    print(f"  map_py range: [{min(map_y)}, {max(map_y)}]")


if __name__ == "__main__":
    main()
