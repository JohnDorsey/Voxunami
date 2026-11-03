"""Makes `hyperbola.txt`, the places and speeds of bodies on hyperbolas that the tests of `wide_kepler` are compared with.

    python generate_hyperbola.py > hyperbola.txt

A hyperbola has a `ratio` (1 - e) **below nothing** (e above 1), and its `radius` is the size of its semi-major axis, `|a|`. Its `period` is 2 pi over the mean motion n (a time of
nothing in particular: a body goes round no turn), negative for the other way round, and its `phase` is the mean anomaly at time 0 as it is, **not** taken modulo a turn: the mean anomaly at
a time is `phase + 2 pi t / period`. Kepler's equation is `M = e sinh H - H`.

This is worked out with mpmath at 700 bits more than the width of each case, from the elements exactly as written in the line, and **by a way the Rust does not take**: the Rust goes
from `H` to the place by the hyperbolic functions of `H`; this goes by the true anomaly (`tan(nu / 2) = sqrt((e + 1) / (e - 1)) tanh(H / 2)`), the distance `p / (1 + e cos nu)` of the
conic's own equation, and the speed from the angular momentum (`sqrt(mu / p)`, with `mu = n^2 |a|^3`), so a formula that is wrong in both would still show. Needs `mpmath` and `generate.py`
beside it (the helpers); the file is committed, so the tests need only Rust. The cases are the same on every run (a seeded generator).

Each line is that of `orbits.txt`: `orbit <bits>`, the elements (the normal's three components, radius, ratio, periapsis, period, phase), the time (units, units a second) and the answer
(the place's three components in voxels, the speed's three in voxels a second), each number as `[-]0x<hex digits of the odd mantissa>p<decimal exponent>`, the answer rounded to `bits + 64` bits.
"""
import random
import sys

from mpmath import atan, cosh, floor, log, mp, mpf, pi, sinh, sqrt, tanh
from mpmath import cos, sin

from generate import SPARE, WIDTHS, basis, number, round_mpf, text, value


def solve_hyperbolic_anomaly(mean, e):
    """e sinh H - H = M: H is odd in M, so for M above nothing: bisection to the working precision on [0, ln(4 M) + 3] (the end is above the root), then Newton."""
    if mean == 0:
        return mpf(0)
    if mean < 0:
        return -solve_hyperbolic_anomaly(-mean, e)
    low, high = mpf(0), max(mpf(3), log(4 * mean) + 3)
    for _ in range(mp.prec + 8):
        middle = (low + high) / 2
        if e * sinh(middle) - middle > mean:
            high = middle
        else:
            low = middle
    anomaly = (low + high) / 2
    for _ in range(3):
        anomaly -= (e * sinh(anomaly) - anomaly - mean) / (e * cosh(anomaly) - 1)
    return anomaly


def place_and_speed(normal, radius, ratio, periapsis, period, phase, units, per_second):
    rate = 2 * pi / period
    mean = phase + rate * (mpf(units) / per_second)
    e = 1 - ratio
    anomaly = solve_hyperbolic_anomaly(mean, e)
    # The true anomaly, the distance from the conic's equation, and the speed from the angular momentum.
    true_anomaly = 2 * atan(sqrt((e + 1) / (e - 1)) * tanh(anomaly / 2))
    parameter = radius * (e * e - 1)
    distance = parameter / (1 + e * cos(true_anomaly))
    x, y = distance * cos(true_anomaly), distance * sin(true_anomaly)
    mu = (rate * rate) * radius ** 3
    unit = sqrt(mu / parameter)
    vx, vy = -unit * sin(true_anomaly), unit * (e + cos(true_anomaly))
    if rate < 0:
        vx, vy = -vx, -vy
    # The generator checks itself: the other way (the one the Rust takes, by the hyperbolic functions of H) gives the same place and speed to the working precision less 200 bits (a row keeps 636 bits fewer than the working precision has, and a hyperbola of an eccentricity of 2^64 loses some in the cancellation of e - cosh H).
    minor = sqrt(e * e - 1)
    factor = rate * radius / (e * cosh(anomaly) - 1)
    other = [radius * (e - cosh(anomaly)), radius * minor * sinh(anomaly), -factor * sinh(anomaly), factor * minor * cosh(anomaly)]
    scale = max(abs(c) for c in [x, y, vx, vy]) + radius
    for mine, theirs in zip([x, y, vx, vy], other):
        assert abs(mine - theirs) <= scale * mpf(2) ** (-(mp.prec - 200)), (mine, theirs)
    first, second = basis(normal)
    c, s = cos(periapsis), sin(periapsis)
    p = [c * first[i] + s * second[i] for i in range(3)]
    q = [-s * first[i] + c * second[i] for i in range(3)]
    place = [x * p[i] + y * q[i] for i in range(3)]
    speed = [vx * p[i] + vy * q[i] for i in range(3)]
    return place, speed


def row(bits, normal, radius, ratio, periapsis, period, phase, units, per_second):
    args = [value(pair) for pair in normal]
    place, speed = place_and_speed(args, value(radius), value(ratio), value(periapsis), value(period), value(phase), units, per_second)
    fields = [text(*pair) for pair in normal + [radius, ratio, periapsis, period, phase]]
    answer = [text(*round_mpf(c, bits + 64)) for c in place + speed]
    return f"orbit {bits} " + " ".join(fields) + f" {units} {per_second} " + " ".join(answer)


def special_rows(rng, bits):
    """The places where the equation is hardest: the thinnest hyperbola (a ratio of -2^-64, a slope of the equation as small as that) with the body at the periapsis (a mean anomaly of 0, 2^-20,
    2^-84 and 2^-136), a long way out on either side (a mean anomaly of +-2^90), the other way round, and at a clock of 2^96 units with the phase chosen to cancel it."""
    mp.prec = bits + SPARE
    cases = [
        ((-1, -64), (0, 0)),
        ((-1, -64), (1, -20)),
        ((-1, -64), (1, -84)),
        ((-1, -64), (1, -136)),
        ((-1, -64), (-1, -84)),
        ((-3, -65), (1, -150)),
        ((-1, -64), (1, 90)),
        ((-1, -64), (-1, 90)),
        ((-1, -30), (1, -50)),
        ((-1, -10), (-1, -40)),
        ((-1, 20), (1, 90)),
        ((-1, 64), (1, -30)),
        ((-1, 64), (1, 60)),
        ((-1, 0), (1, 5)),
        # The thinnest hyperbolas at a mean anomaly of a tenth to twenty, where the root of the equation is highest above where the first guess of a program that only knew the
        # periapsis would put it (the bound above the root of the solver is what holds, and a bound that is too low is stuck against it).
        ((-1, -64), (3, -4)),
        ((-1, -64), (1, -2)),
        ((-1, -64), (5, -4)),
        ((-1, -64), (1, -1)),
        ((-1, -64), (1, 0)),
        ((-1, -64), (5, 0)),
        ((-1, -64), (5, 2)),
        ((-1, -40), (3, -4)),
        ((-1, -40), (1, 0)),
        ((-1, -40), (5, 0)),
    ]
    rows = []
    for ratio, phase in cases:
        normal = [number(rng, bits, -3, 1, True) for _ in range(3)]
        radius = number(rng, bits, 0, rng.choice([20, 60, 120]))
        periapsis = number(rng, bits, -4, 20, True)
        period = number(rng, bits, 6, 34, True)
        rows.append(row(bits, normal, radius, ratio, periapsis, period, phase, 0, 20))
    # The clock near 2^96 units and a period near 2^-19 seconds: rate x time is some 2^99, and the phase is chosen to cancel it, so that the mean anomaly is tiny and the body is at its periapsis,
    # where the equation magnifies a mean anomaly by 2^64 on the thinnest hyperbola. (What is cancelled is the sum of two numbers of 99 bits and the width of the case.)
    per_second = 1 << 20
    for ratio in [(-1, -64), (-3, -65), (-1, -40)]:
        normal = [number(rng, bits, -3, 1, True) for _ in range(3)]
        radius = number(rng, bits, 0, rng.choice([20, 60, 120]))
        periapsis = number(rng, bits, -4, 20, True)
        period = number(rng, bits, -20, -17)
        units = rng.randint(1 << 90, (1 << 96) - 1)
        moved = 2 * pi / value(period) * (mpf(units) / per_second)
        phase = round_mpf(-moved + mpf(2) ** -(bits // 2 + 20), bits)
        rows.append(row(bits, normal, radius, ratio, periapsis, period, phase, units, per_second))
    return rows


def main():
    rng = random.Random(20261103)
    lines = []
    normals = [
        [(1, 0), (0, 0), (0, 0)],
        [(0, 0), (1, 0), (0, 0)],
        [(0, 0), (0, 0), (1, 0)],
        [(-1, 0), (1, -3), (0, 0)],
        [(0x1CCCCCCCCCCCCD, -53), (0x1, -2), (0x1, -2)],
        [(0x1CCCCCCCCCCCCC, -53), (0x1, -2), (0x1, -2)],
    ]
    for bits, count in WIDTHS.items():
        mp.prec = bits + SPARE
        for case in range(count):
            if case < len(normals) and rng.random() < 0.5:
                normal = [(m, e) for m, e in normals[case]]
            else:
                normal = [number(rng, bits, -3, 1, True) for _ in range(3)]
            if all(m == 0 for m, _ in normal):
                continue
            radius = number(rng, bits, 0, rng.choice([10, 40, 80, 120]))
            # |ratio|: barely a hyperbola (2^-64), thin (2^-40, 2^-20), middling (e from 1.1 to 2), wide (e to 2^10), and extreme (2^40 to 2^64).
            kind = rng.choice(["parabolic", "thin", "thinner", "middling", "wide", "extreme"])
            low, high = {"parabolic": (-64, -63), "thin": (-20, -19), "thinner": (-40, -39), "middling": (-3, 1), "wide": (1, 10), "extreme": (40, 64)}[kind]
            magnitude = number(rng, bits, low, high)
            ratio = (-magnitude[0], magnitude[1])
            periapsis = number(rng, bits, -4, rng.choice([2, 20, 60]), True)
            low, high = rng.choice([(-5, 2), (0, 14), (6, 34), (12, 40), (30, 80)])
            period = number(rng, bits, low, high, True)
            phase = number(rng, bits, -3, rng.choice([8, 40, 90]), True)
            units = rng.randint(0, (1 << rng.choice([10, 30, 62])) - 1)
            per_second = rng.choice([20, 40, 1 << 20])
            lines.append(row(bits, normal, radius, ratio, periapsis, period, phase, units, per_second))
        lines.extend(special_rows(rng, bits))
    print("\n".join(lines))


if __name__ == "__main__":
    sys.stdout.reconfigure(newline="\n")
    main()
