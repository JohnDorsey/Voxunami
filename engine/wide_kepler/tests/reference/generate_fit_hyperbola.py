"""Makes `fit_hyperbola.txt`, the states from which the tests of `Elements::from_state` for a hyperbola (the orbit a body is on when it is going faster than it would to escape) are made.

    python generate_fit_hyperbola.py > fit_hyperbola.txt

The same as `generate_fit.py` for ellipses: each row is a hyperbola from known elements, worked out forward by `generate_hyperbola.py` (by the true anomaly and the angular momentum, in
mpmath, 700 bits more than the width of the case) to the place and the speed of the body at a time, and the mass parameter that gives the orbit its mean motion (mu = (2 pi / period)^2
radius^3, the radius being the size of the semi-major axis). The Rust is given the state and mu and must find the orbit again: so it is held to the truth the elements were, and not
to the forward evaluator it is a partner of. The state is rounded to the width of the case and 256 bits more.

Each line: `fit <bits>`, the elements (the normal's three components, a unit vector rounded to the width, radius, ratio (below nothing), periapsis, period, phase), the time (units, units a
second), mu, and the state (the place's three components, the speed's three), each number as `[-]0x<hex digits of the odd mantissa>p<decimal exponent>`. Periods are above nothing: the
orbit found goes counter-clockwise about the normal it finds, which is the given normal when the period is positive. **The phase is the mean anomaly at time 0 as it is** (not
taken modulo a turn), and has as many bits as its size and the width need, which is how the Rust keeps it.
"""
import random
import sys

from mpmath import mp, mpf, pi

import generate
import generate_hyperbola

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
    place, speed = generate_hyperbola.place_and_speed(args, r, rt, pe, pd, ph, units, per_second)
    mu = (2 * pi / pd) ** 2 * r ** 3
    fields = [generate.text(*pair) for pair in normal + [radius, ratio, periapsis, period, phase]]
    state = [generate.text(*generate.round_mpf(c, bits + EXTRA)) for c in place + speed]
    mu_text = generate.text(*generate.round_mpf(mu, bits + EXTRA))
    return f"fit {bits} " + " ".join(fields) + f" {units} {per_second} {mu_text} " + " ".join(state)


def negative(pair):
    return (-pair[0], pair[1])


def main():
    rng = random.Random(20261104)
    lines = []
    for bits, count in WIDTHS.items():
        mp.prec = bits + EXTRA + generate.SPARE
        for case in range(count):
            normal = unit_normal(rng, bits)
            if all(m == 0 for m, _ in normal):
                continue
            radius = generate.number(rng, bits, 0, rng.choice([10, 40, 80, 120]))
            # |ratio|: barely a hyperbola (2^-64), thin, middling (e from 1.1 to 2), wide (e to 2^10) and extreme (2^40 to 2^64).
            kind = case % 8
            low, high = {0: (-64, -63), 1: (-40, -39), 2: (-20, -19), 3: (-3, 1), 4: (-1, 0), 5: (1, 10), 6: (40, 64), 7: (-3, 1)}[kind]
            ratio = negative(generate.number(rng, bits, low, high))
            periapsis = generate.number(rng, bits, -4, rng.choice([2, 20, 60]), True)
            low, high = rng.choice([(-5, 2), (0, 14), (6, 34), (12, 40), (30, 80)])
            period = generate.number(rng, bits, low, high)
            phase = generate.number(rng, bits, -3, rng.choice([8, 40, 90]), True)
            units = rng.randint(0, (1 << rng.choice([10, 30, 62])) - 1)
            per_second = rng.choice([20, 40, 1 << 20])
            lines.append(row(bits, normal, radius, ratio, periapsis, period, phase, units, per_second))
        # The body at the periapsis of the thinnest hyperbola (a state across the radius, the mean anomaly tiny or nothing), and a long way out on both sides.
        for ratio, phase in [((-1, -64), (0, 0)), ((-1, -64), (1, -100)), ((-1, -64), (1, 90)), ((-1, -64), (-1, 90)), ((-1, -20), (1, 5)), ((-3, -2), (1, -20))]:
            normal = unit_normal(rng, bits)
            if all(m == 0 for m, _ in normal):
                continue
            radius = generate.number(rng, bits, 0, 40)
            periapsis = generate.number(rng, bits, -4, 2, True)
            period = generate.number(rng, bits, 6, 34)
            lines.append(row(bits, normal, radius, ratio, periapsis, period, phase, 0, 20))
        # A late time and a phase that cancels the clock: the body at the periapsis 2^76 seconds after time 0, the mean anomaly the sum of two numbers of 99 bits.
        per_second = 1 << 20
        for ratio in [(-1, -64), (-1, -40)]:
            normal = unit_normal(rng, bits)
            if all(m == 0 for m, _ in normal):
                continue
            radius = generate.number(rng, bits, 0, 40)
            periapsis = generate.number(rng, bits, -4, 2, True)
            period = generate.number(rng, bits, -20, -17)
            units = rng.randint(1 << 90, (1 << 96) - 1)
            moved = 2 * pi / generate.value(period) * (mpf(units) / per_second)
            phase = generate.round_mpf(-moved + mpf(2) ** -(bits // 2 + 20), bits)
            lines.append(row(bits, normal, radius, ratio, periapsis, period, phase, units, per_second))
    print("\n".join(lines))


if __name__ == "__main__":
    sys.stdout.reconfigure(newline="\n")
    main()
