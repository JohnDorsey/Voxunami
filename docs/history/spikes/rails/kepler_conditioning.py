"""THROWAWAY. How far does rounding the mean anomaly to 2^-126 (or 2^-254) of a turn move a body at periapsis, for ellipses of growing eccentricity?

Reference arithmetic is mpmath at 1200 bits (so the answer is the mathematics, not a rounding of ours). The rounding of M is the smallest error any design with that
many bits of angle can have: a real fixed-point solver adds a few ulps more.
"""
from mpmath import mp, mpf, sin, cos, sqrt, pi

mp.prec = 1200
TWO_NEG_32 = mpf(2) ** -32  # the grid of positions: 2^-32 voxel = 0.23 nm


def solve_eccentric_anomaly(mean_anomaly, e):
    """E - e sin E = M for M in [0, pi]: bisection to ~900 bits, then Newton polish."""
    low, high = mpf(0), pi
    for _ in range(900):
        middle = (low + high) / 2
        if middle - e * sin(middle) - mean_anomaly > 0:
            high = middle
        else:
            low = middle
    eccentric = (low + high) / 2
    for _ in range(4):
        eccentric -= (eccentric - e * sin(eccentric) - mean_anomaly) / (1 - e * cos(eccentric))
    return eccentric


def position(semi_major, e, mean_anomaly):
    eccentric = solve_eccentric_anomaly(mean_anomaly, e)
    return (
        semi_major * (cos(eccentric) - e),
        semi_major * sqrt(1 - e * e) * sin(eccentric),
    )


def displacement_for_bits(periapsis, apoapsis, bits):
    """Metres moved at periapsis by one unit in the last place (2^-bits of a turn) of the mean anomaly."""
    semi_major = (periapsis + apoapsis) / 2
    e = (apoapsis - periapsis) / (apoapsis + periapsis)
    one_ulp = 2 * pi * mpf(2) ** -bits
    x0, y0 = position(semi_major, e, mpf(0))
    x1, y1 = position(semi_major, e, one_ulp)
    return sqrt((x1 - x0) ** 2 + (y1 - y0) ** 2), e


def closed_form(periapsis, apoapsis, bits):
    delta = 2 * periapsis / (periapsis + apoapsis)  # 1 - e
    return 2 * pi * mpf(2) ** -bits * sqrt(2) * periapsis * delta ** mpf(-1.5)


print("q = periapsis in metres, ratio = apoapsis / periapsis. 'moved' = metres one ulp of M moves the body at periapsis.")
print(f"{'q':>8} {'ratio':>9} {'1-e':>10} {'moved @126':>12} {'closed form':>12} {'moved @254':>12}  vs grid 2.3e-10")
for periapsis in (mpf(10) ** 6, mpf(10) ** 9):
    for exponent in (1, 3, 6, 9, 12, 15, 18, 21):
        ratio = mpf(10) ** exponent
        apoapsis = periapsis * ratio
        moved126, e = displacement_for_bits(periapsis, apoapsis, 126)
        moved254, _ = displacement_for_bits(periapsis, apoapsis, 254)
        formula = closed_form(periapsis, apoapsis, 126)
        print(
            f"{float(periapsis):8.0e} {float(ratio):9.0e} {float(1 - e):10.2e} {float(moved126):12.3e} {float(formula):12.3e} {float(moved254):12.3e}"
        )

print()
print("Largest apoapsis / periapsis for which one ulp of M moves periapsis by no more than the grid (2^-32 voxel), from the closed form:")
for periapsis in (mpf(10) ** 3, mpf(10) ** 6, mpf(10) ** 9, mpf(10) ** 12):
    for bits in (126, 254):
        # moved = 2 pi 2^-bits sqrt2 q delta^-1.5 <= 2^-32, and ratio ~ 2 / delta
        delta = (2 * pi * mpf(2) ** -bits * sqrt(2) * periapsis / TWO_NEG_32) ** (mpf(2) / 3)
        print(f"  q = {float(periapsis):7.0e} m, {bits} bits: ratio up to {float(2 / delta):9.2e}  (apoapsis {float(periapsis * 2 / delta):9.2e} m)")
