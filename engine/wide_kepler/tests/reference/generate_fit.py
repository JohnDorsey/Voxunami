"""Makes `fit.txt`, the states from which the tests of `Elements::from_state` (the orbit a body is on) are made.

    python generate_fit.py > fit.txt

Each row is an orbit from known elements, worked out forward by `generate.py` (the definition of `engine/world_storage/tools/exact_ellipse_reference.py` in mpmath, 700 bits more than
the width of the case) to the place and the speed of the body at a time, and the mass parameter that gives the orbit its period (mu = (2 pi / period)^2 radius^3). The Rust is given
the state and mu and must find the orbit again: so it is held to the truth the elements were, and not to the forward evaluator it is a partner of. The state is rounded to the width
of the case and 256 bits more, so that it is as exact as the width asked for needs (a state rounded to 64 bits more would give a ratio of 2^-64 only 2^-bits of itself).

Each line: `fit <bits>`, the elements (the normal's three components, a unit vector rounded to the width, radius, ratio, periapsis, period, phase), the time (units, units a second), mu, and the state (the place's three
components, the speed's three), each number as `[-]0x<hex digits of the odd mantissa>p<decimal exponent>`. Periods are above nothing: the orbit found goes counter-clockwise about
the normal it finds, which is the given normal when the period is positive.
"""
import importlib.util
import os
import random
import sys

from mpmath import floor, mp, mpf, pi

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("generate", os.path.join(HERE, "generate.py"))
generate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(generate)

WIDTHS = {128: 16, 192: 8, 256: 14, 512: 6}
EXTRA = 256


def unit_normal(rng, bits):
    """A direction as three numbers of `bits` bits: a random vector made a unit vector in mpmath and rounded, as the Rust keeps the normal it finds."""
    parts = [generate.value(generate.number(rng, bits, -3, 1, True)) for _ in range(3)]
    size = mp.sqrt(sum(part * part for part in parts))
    return [generate.round_mpf(part / size, bits) for part in parts]


def row(bits, normal, radius, ratio, periapsis, period, phase, units, per_second):
    args = [generate.value(pair) for pair in normal]
    r, rt, pe, pd, ph = (generate.value(x) for x in (radius, ratio, periapsis, period, phase))
    place, speed = generate.place_and_speed(args, r, rt, pe, pd, ph, units, per_second)
    mu = (2 * pi / pd) ** 2 * r ** 3
    fields = [generate.text(*pair) for pair in normal + [radius, ratio, periapsis, period, phase]]
    state = [generate.text(*generate.round_mpf(c, bits + EXTRA)) for c in place + speed]
    mu_text = generate.text(*generate.round_mpf(mu, bits + EXTRA))
    return f"fit {bits} " + " ".join(fields) + f" {units} {per_second} {mu_text} " + " ".join(state)


def main():
    rng = random.Random(20261103)
    lines = []
    for bits, count in WIDTHS.items():
        mp.prec = bits + EXTRA + generate.SPARE
        for case in range(count):
            normal = unit_normal(rng, bits)
            if all(m == 0 for m, _ in normal):
                continue
            radius = generate.number(rng, bits, 0, rng.choice([10, 40, 80, 120]))
            kind = case % 8
            if kind == 0:
                ratio = generate.number(rng, bits, -64, -63)  # the thinnest
            elif kind == 1:
                ratio = generate.number(rng, bits, -40, -39)
            elif kind == 2:
                ratio = generate.number(rng, bits, -20, -19)
            elif kind == 3:
                ratio = ((1 << bits) - (1 << (bits // 2)), -bits)  # barely not a circle: e = 2^-(bits/2)
            elif kind == 4:
                ratio = generate.number(rng, bits, -1, 0)  # between 1/2 and 1
            else:
                ratio = generate.number(rng, bits, -3, 0)
            periapsis = generate.number(rng, bits, -4, rng.choice([2, 20, 60]), True)
            low, high = rng.choice([(-5, 2), (0, 14), (6, 34), (12, 40), (30, 80)])
            period = generate.number(rng, bits, low, high)
            phase = generate.number(rng, bits, -3, 63, True)
            units = rng.randint(0, (1 << rng.choice([10, 30, 62])) - 1)
            per_second = rng.choice([20, 40, 1 << 20])
            lines.append(row(bits, normal, radius, ratio, periapsis, period, phase, units, per_second))
        # The body at the periapsis and the apoapsis of a thin ellipse, where the state is nearly along the radius or across it.
        for phase in [(0, 0), generate.round_mpf(+pi, bits)]:
            normal = unit_normal(rng, bits)
            if all(m == 0 for m, _ in normal):
                continue
            radius = generate.number(rng, bits, 0, 40)
            ratio = generate.number(rng, bits, -64, -63)
            periapsis = generate.number(rng, bits, -4, 2, True)
            period = generate.number(rng, bits, 6, 34)
            lines.append(row(bits, normal, radius, ratio, periapsis, period, phase, 0, 20))
    print("\n".join(lines))


if __name__ == "__main__":
    sys.stdout.reconfigure(newline="\n")
    main()
