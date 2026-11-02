"""Throwaway: how much does a flyby magnify an error in where a ship is aimed, and how many digits does a chain of ten need? Planar, patched conics, mpmath 60 digits.

A ship arrives with speed vinf and impact parameter b at a body of gravitational parameter mu; the hyperbola turns it by delta = 2 asin(1 / e), e = sqrt(1 + (b vinf^2 / mu)^2).
It leaves in a direction turned by delta, with a sideways offset after a leg of length L of about L * delta (to first order). So an error db in b gives an error in the aim
at the next body of dX = L * d(delta)/db * db. We take the derivative with mpmath (no first-order shortcut) and multiply the factors along a chain.
"""
from mpmath import mp, mpf, sqrt, asin, diff, log

mp.dps = 60


def turn(b, vinf, mu):
    e = sqrt(1 + (b * vinf**2 / mu) ** 2)
    return 2 * asin(1 / e)


def magnification(b, vinf, mu, length):
    """How many metres the aim at the next body moves for one metre of b here."""
    return abs(length * diff(lambda x: turn(x, vinf, mu), b))


# (name, mu, vinf, periapsis wanted, leg to the next body)
SOLAR = [
    ("Earth", mpf("3.986e14"), mpf("9e3"), mpf("7e6"), mpf("1.5e11")),
    ("Venus", mpf("3.249e14"), mpf("8e3"), mpf("7e6"), mpf("1.0e11")),
    ("Earth", mpf("3.986e14"), mpf("9e3"), mpf("7e6"), mpf("4.0e11")),
    ("Jupiter", mpf("1.267e17"), mpf("1.0e4"), mpf("1.0e8"), mpf("7.0e11")),
    ("Saturn", mpf("3.793e16"), mpf("1.0e4"), mpf("1.0e8"), mpf("1.4e12")),
    ("Uranus", mpf("5.794e15"), mpf("1.0e4"), mpf("3.0e7"), mpf("1.6e12")),
]
STELLAR = [
    ("star A", mpf("1.327e20"), mpf("3e4"), mpf("1.0e9"), mpf("1e16")),
    ("star B", mpf("1.327e20"), mpf("3e4"), mpf("1.0e9"), mpf("1e17")),
    ("star C", mpf("1.327e20"), mpf("3e4"), mpf("1.0e9"), mpf("1e18")),
]


def b_for_periapsis(rp, vinf, mu):
    # rp = (mu / vinf^2)(e - 1), b = (mu / vinf^2) sqrt(e^2 - 1)
    k = mu / vinf**2
    e = 1 + rp / k
    return k * sqrt(e * e - 1)


def chain(name, legs):
    total = mpf(1)
    print(name)
    for body, mu, vinf, rp, length in legs:
        b = b_for_periapsis(rp, vinf, mu)
        factor = magnification(b, vinf, mu, length)
        total *= factor
        print(f"  {body:8} periapsis {float(rp):.1e} m  b {float(b):.2e} m  leg {float(length):.1e} m: x{float(factor):.2e}  (running {float(total):.2e})")
    return total


solar = chain("in a system, six assists", SOLAR)
mixed = chain("three stellar assists with legs of 1e16 to 1e18 m", STELLAR)
# The control that is wanted at the end: 1 mm. The digits the first leg's numbers need, if nothing is corrected in between: the span over 1 mm times the magnification.
for label, total, span in [("six assists in a system", solar, mpf("1e13")), ("three stellar", mixed, mpf("1e18")), ("ten stellar (the three, repeated)", mixed**mpf(10) / mixed**mpf(7) * mixed**mpf(0), mpf("1e21"))]:
    pass
ten = mixed ** (mpf(10) / 3)
print(f"ten stellar assists, estimated as the three above raised to 10/3: magnification {float(ten):.2e}")
for label, total, span in [("six in a system", solar, mpf("1e13")), ("three stellar", mixed, mpf("1e18")), ("ten stellar (estimate)", ten, mpf("1e21"))]:
    needed = log(span / mpf("1e-3") * total, 2)
    print(f"{label}: bits for the numbers of the first leg if nothing is corrected in between: {float(needed):.0f}")
# With a correction after the last assist only the last leg's magnification counts.
last = magnification(b_for_periapsis(STELLAR[-1][3], STELLAR[-1][2], STELLAR[-1][1]), STELLAR[-1][2], STELLAR[-1][1], STELLAR[-1][4])
print(f"last leg only (a correction just before the last assist): magnification {float(last):.2e}, bits for a position of 1e18 m: {float(log(mpf('1e18') / mpf('1e-3') * last, 2)):.0f}")
# What a rail's position has: i128 bricks of 32 m and a rest of 2^-32 m (2.3e-10 m).
print("a position now has a step of 2.3e-10 m = 2^-32; for 1 mm after a magnification M it needs 1e-3 / M:")
for m in [last, solar, mixed]:
    print(f"  M = {float(m):.2e}: step {float(mpf('1e-3') / m):.2e} m")
