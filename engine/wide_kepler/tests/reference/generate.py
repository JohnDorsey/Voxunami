"""Makes `orbits.txt`, the places and speeds the tests of `wide_kepler` are compared with.

    python generate.py > orbits.txt

The definition is the one of `engine/world_storage/tools/exact_ellipse_reference.py` (read that for the words), worked out here with mpmath at 700 bits more than the width of the
case, with the elements exactly as written in the line (nothing is rounded to a double), so the rows are as near the truth as the working precision lets them be and the Rust is
held to the truth and not to another program. Needs `mpmath`; the file is committed, so the tests need only Rust. The cases are the same on every run (a seeded generator).

Each line: `orbit <bits>`, the elements (the normal's three components, radius, ratio, periapsis, period, phase), the time (units, units a second) and the answer (the place's three
components in voxels, the speed's three in voxels a second), each number as `[-]0x<hex digits of the odd mantissa>p<decimal exponent>`, the answer rounded to `bits + 64` bits.
"""
import random
import sys
from fractions import Fraction

from mpmath import cos, floor, mp, mpf, pi, sin, sqrt

WIDTHS = {128: 24, 192: 12, 256: 24, 512: 8, 1024: 3}
SPARE = 700


def text(mantissa, exponent):
    if mantissa == 0:
        return "0x0p0"
    sign = "-" if mantissa < 0 else ""
    magnitude = abs(mantissa)
    while magnitude % 2 == 0:
        magnitude //= 2
        exponent += 1
    return f"{sign}0x{magnitude:x}p{exponent}"


def round_mpf(value, bits):
    """An mpf rounded to `bits` bits, to the nearest, ties to even, as (mantissa, exponent)."""
    sign, mantissa, exponent, _ = value._mpf_
    if mantissa == 0:
        return 0, 0
    fraction = Fraction(mantissa) * Fraction(2) ** exponent
    exponent = mantissa.bit_length() + exponent - bits
    scaled = fraction / Fraction(2) ** exponent
    quotient = scaled.numerator // scaled.denominator
    rest = scaled - quotient
    if rest > Fraction(1, 2) or (rest == Fraction(1, 2) and quotient % 2 == 1):
        quotient += 1
    return (-quotient if sign else quotient), exponent


def number(rng, bits, low, high, signed=False):
    """A number of at most `bits` bits with its leading bit in [2^low, 2^high)."""
    mantissa = rng.getrandbits(bits - 1) | (1 << (bits - 1))
    if rng.random() < 0.3:
        mantissa = rng.getrandbits(rng.randint(2, 53)) | 1
        mantissa <<= bits - mantissa.bit_length()
    exponent = rng.randint(low, high - 1) - (bits - 1)
    if signed and rng.random() < 0.5:
        mantissa = -mantissa
    return mantissa, exponent


def value(pair):
    return mpf(pair)


def solve_eccentric_anomaly(mean_anomaly, e):
    """E - e sin E = M for M in [0, 2 pi): bisection to the working precision, then Newton."""
    two_pi = 2 * pi
    if mean_anomaly == 0:
        return mpf(0)
    if e == 0:
        return mean_anomaly
    if mean_anomaly > pi:
        return two_pi - solve_eccentric_anomaly(two_pi - mean_anomaly, e)
    low, high = mpf(0), pi
    for _ in range(mp.prec + 8):
        middle = (low + high) / 2
        if middle - e * sin(middle) > mean_anomaly:
            high = middle
        else:
            low = middle
    eccentric = (low + high) / 2
    for _ in range(3):
        eccentric -= (eccentric - e * sin(eccentric) - mean_anomaly) / (1 - e * cos(eccentric))
    return eccentric


def basis(normal):
    """The two directions of the plane: the +x axis (the +z axis where the normal's x is 0.9 or more, as written) with its part along the normal taken out, and the normal crossed with it."""
    size = sqrt(sum(c * c for c in normal))
    n = [c / size for c in normal]
    # (The threshold is the double 0.9, exactly, and the component is the one given, not the unit vector's: the program decides by the number it keeps, as it always did.)
    axis = 0 if abs(normal[0]) < mpf(0.9) else 2
    reference = [mpf(0)] * 3
    reference[axis] = mpf(1)
    along = n[axis]
    inside = [reference[i] - n[i] * along for i in range(3)]
    inside_size = sqrt(sum(c * c for c in inside))
    first = [c / inside_size for c in inside]
    second = [n[1] * first[2] - n[2] * first[1], n[2] * first[0] - n[0] * first[2], n[0] * first[1] - n[1] * first[0]]
    return first, second


def place_and_speed(normal, radius, ratio, periapsis, period, phase, units, per_second):
    two_pi = 2 * pi
    turns = mpf(units) / (per_second * period)
    turns -= floor(turns)
    total = turns + phase / two_pi
    total -= floor(total)
    mean = two_pi * total
    e = 1 - ratio
    eccentric = solve_eccentric_anomaly(mean, e)
    root = sqrt(1 - e * e)
    x = radius * (cos(eccentric) - e)
    y = radius * root * sin(eccentric)
    first, second = basis(normal)
    c, s = cos(periapsis), sin(periapsis)
    p = [c * first[i] + s * second[i] for i in range(3)]
    q = [-s * first[i] + c * second[i] for i in range(3)]
    place = [x * p[i] + y * q[i] for i in range(3)]
    factor = (two_pi / period) * radius / (1 - e * cos(eccentric))
    vx, vy = -factor * sin(eccentric), factor * root * cos(eccentric)
    speed = [vx * p[i] + vy * q[i] for i in range(3)]
    return place, speed


def special_rows(rng, bits):
    """The places where the equation is hardest: the thinnest ellipses with the body at the periapsis (an anomaly of 0, 2^-20 and 2^-136, and so the slope of the equation as small as the ratio),
    at the apoapsis (the mean anomaly pi, and 2 pi), and an ellipse barely not a circle."""
    mp.prec = bits + SPARE
    pi_rounded = round_mpf(+pi, bits)
    two_pi_rounded = (pi_rounded[0], pi_rounded[1] + 1)
    nearly_one = ((1 << bits) - 1, -bits)
    cases = [
        ((1, -64), (0, 0)),
        ((1, -64), (1, -84)),
        ((1, -64), (1, -200)),
        ((3, -65), (1, -150)),
        ((1, -64), pi_rounded),
        ((1, -64), two_pi_rounded),
        ((1, -30), (1, -50)),
        ((1, -30), (1, -60)),
        ((1, -10), (-1, -40)),
        (nearly_one, (1, -30)),
        (nearly_one, pi_rounded),
        ((1, 0), pi_rounded),
    ]
    rows = []
    for ratio, phase in cases:
        normal = [number(rng, bits, -3, 1, True) for _ in range(3)]
        radius = number(rng, bits, 0, rng.choice([20, 60, 120]))
        periapsis = number(rng, bits, -4, 20, True)
        period = number(rng, bits, 6, 34, True)
        args = [value(pair) for pair in normal]
        place, speed = place_and_speed(args, value(radius), value(ratio), value(periapsis), value(period), value(phase), 0, 20)
        fields = [text(*pair) for pair in normal + [radius, ratio, periapsis, period, phase]]
        answer = [text(*round_mpf(c, bits + 64)) for c in place + speed]
        rows.append(f"orbit {bits} " + " ".join(fields) + " 0 20 " + " ".join(answer))
    # A clock near 2^96 units and a period near 2^-19 seconds: the turns gone have some 115 whole bits, the fraction after them has the rest of the width the arithmetic has, and the
    # phase is chosen to cancel it so that the mean anomaly is tiny (2^-64 or so) on a thin ellipse: the body is at its periapsis, where the equation magnifies a mean anomaly by 2^64 or so.
    per_second = 1 << 20
    for ratio in [(1, -64), (3, -65), (1, -40)]:
        normal = [number(rng, bits, -3, 1, True) for _ in range(3)]
        radius = number(rng, bits, 0, rng.choice([20, 60, 120]))
        periapsis = number(rng, bits, -4, 20, True)
        period = number(rng, bits, -20, -17)
        units = rng.randint(1 << 90, (1 << 96) - 1)
        turns = mpf(units) / (per_second * value(period))
        fraction = turns - floor(turns)
        phase = round_mpf(-2 * pi * fraction + mpf(2) ** -(bits // 2 + 20), bits)
        args = [value(pair) for pair in normal]
        place, speed = place_and_speed(args, value(radius), value(ratio), value(periapsis), value(period), value(phase), units, per_second)
        fields = [text(*pair) for pair in normal + [radius, ratio, periapsis, period, phase]]
        answer = [text(*round_mpf(c, bits + 64)) for c in place + speed]
        rows.append(f"orbit {bits} " + " ".join(fields) + f" {units} {per_second} " + " ".join(answer))
    return rows


def main():
    rng = random.Random(20261102)
    lines = []
    normals = [
        [(1, 0), (0, 0), (0, 0)],
        [(0, 0), (1, 0), (0, 0)],
        [(0, 0), (0, 0), (1, 0)],
        [(-1, 0), (1, -3), (0, 0)],
        [(0x1CCCCCCCCCCCCD, -53), (0x1, -2), (0x1, -2)],  # x is the double 0.9: the +z axis is the reference
        [(0x1CCCCCCCCCCCCC, -53), (0x1, -2), (0x1, -2)],  # x is the double just below 0.9: the +x axis is
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
            ratio_choice = rng.choice(["circle", "near", "mid", "thin", "thinner", "thinnest"])
            if ratio_choice == "circle":
                ratio = (1, 0)
            else:
                low = {"near": -6, "mid": -3, "thin": -20, "thinner": -40, "thinnest": -64}[ratio_choice]
                ratio = number(rng, bits, low, low + 1 if ratio_choice != "near" else 0)
            periapsis = number(rng, bits, -4, rng.choice([2, 20, 60]), True)
            low, high = rng.choice([(-5, 2), (0, 14), (6, 34), (12, 40), (30, 80)])
            period = number(rng, bits, low, high, True)
            phase = number(rng, bits, -3, 63, True)
            units = rng.randint(0, (1 << rng.choice([10, 30, 62])) - 1)
            per_second = rng.choice([20, 40, 1 << 20])
            args = [value(pair) for pair in normal]
            place, speed = place_and_speed(args, value(radius), value(ratio), value(periapsis), value(period), value(phase), units, per_second)
            fields = [text(*pair) for pair in normal + [radius, ratio, periapsis, period, phase]]
            answer = [text(*round_mpf(c, bits + 64)) for c in place + speed]
            lines.append(f"orbit {bits} " + " ".join(fields) + f" {units} {per_second} " + " ".join(answer))
        lines.extend(special_rows(rng, bits))
    print("\n".join(lines))


if __name__ == "__main__":
    sys.stdout.reconfigure(newline="\n")
    main()
