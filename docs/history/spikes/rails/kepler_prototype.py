"""Throwaway (kept for the numbers in docs/orbits-and-thrust-plan.md, section A2/A). The fixed-point Kepler solver of step A2 as plain integers, exactly as it is meant to be written in Rust, measured against the mpmath reference.

Types as in exact_circle.rs: turns are u128 with 128 bits after the point, unit-vector coordinates and ratios are i128 with 126 bits after the point (ONE = 2^126).
Start: a solution in f64 (cubic start, stable evaluation, a few Newton steps), then STEPS Newton steps in fixed point, then one more sine and cosine for the answer.
"""
import math
import random
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[4] / "engine" / "world_storage" / "tools"))

from mpmath import mp, mpf, floor, pi, atan, sqrt as msqrt
import exact_ellipse_reference as reference

mp.dps = 120
ONE = 1 << 126
HALF_TURN = 1 << 127
TURN = 1 << 128

# ---- CORDIC exactly as exact_circle.rs
ANGLES = [int(floor(atan(mpf(2) ** -step) / (2 * pi) * (mpf(2) ** 128) + mpf(1) / 2)) for step in range(127)]
gain = mpf(1)
for step in range(127):
    gain /= msqrt(1 + mpf(2) ** (-2 * step))
GAIN = int(floor(gain * ONE + mpf(1) / 2))
ONE_OVER_TAU = int(floor(ONE / (2 * pi) + mpf(1) / 2))


def halved(value, bits):
    return value if bits == 0 else (value + (1 << (bits - 1))) >> bits


def sine_and_cosine(turns):
    quadrant = turns >> 126
    angle = turns & ((1 << 126) - 1)
    cosine, sine = GAIN, 0
    for step, step_angle in enumerate(ANGLES):
        from_cosine, from_sine = halved(cosine, step), halved(sine, step)
        if angle >= 0:
            cosine -= from_sine
            sine += from_cosine
            angle -= step_angle
        else:
            cosine += from_sine
            sine -= from_cosine
            angle += step_angle
    return [(sine, cosine), (cosine, -sine), (-sine, -cosine), (-cosine, sine)][quadrant]


def multiplied(first, second, bits):
    """rounded to the nearest, sign by magnitude, as exact_circle::multiplied"""
    negative = (first < 0) != (second < 0)
    magnitude = (abs(first) * abs(second) + (1 << (bits - 1))) >> bits
    return -magnitude if negative else magnitude


def divided(numerator, denominator):
    """numerator / denominator, truncated toward zero (restoring division in Rust)."""
    quotient = abs(numerator) // abs(denominator)
    return -quotient if (numerator < 0) != (denominator < 0) else quotient


# ---- the f64 start
def stable_g_and_derivative(eccentric, ratio, e):
    """E - e sin E and its derivative, without the cancellation near E = 0."""
    if abs(eccentric) < 0.5:
        x2 = eccentric * eccentric
        # E - sin E = x^3/6 - x^5/120 + ...
        term = eccentric * x2 / 6.0
        series = 0.0
        for k in range(12):
            series += term
            term *= -x2 / ((2 * k + 4) * (2 * k + 5))
        g = ratio * eccentric + e * series
    else:
        g = eccentric - e * math.sin(eccentric)
    half = math.sin(eccentric / 2)
    return g, ratio + 2 * e * half * half


def float_start(ratio, m_turns):
    """E in radians, to about 1e-15 relative, for m_turns in [0, 2^127]."""
    e = 1.0 - ratio
    m = 2 * math.pi * (m_turns / TURN)
    if m == 0.0:
        return 0.0
    if e < 1e-300:
        return m
    if e < 0.3:
        start = m + e * math.sin(m)
    else:
        p, q = 6.0 * ratio / e, -6.0 * m / e
        disc = math.sqrt(q * q / 4 + p ** 3 / 27)
        start = math.cbrt(-q / 2 + disc) + (math.cbrt(-q / 2 - disc) if -q / 2 - disc != 0 else 0.0)
    start = min(max(start, 0.0), math.pi)
    for _ in range(8):
        g, d = stable_g_and_derivative(start, ratio, e)
        nxt = start - (g - m) / d
        start = min(max(nxt, 0.0), math.pi)
    return start


def solve(ratio, m_turns, steps):
    """(eccentric anomaly in turns u128, sine, cosine) of the ellipse of this ratio at the mean anomaly."""
    rho = int(mpf(ratio) * ONE)  # exact for ratio >= 2^-73
    assert mpf(rho) == mpf(ratio) * ONE
    e = ONE - rho
    reflected = m_turns > HALF_TURN
    m = (TURN - m_turns) % TURN if reflected else m_turns
    if ratio == 1.0:
        eccentric = m
    else:
        eccentric = min(max(int(float_start(ratio, m) / (2 * math.pi) * TURN), 0), HALF_TURN)
        k = multiplied(e, ONE_OVER_TAU, 126)
        for _ in range(steps):
            sine, cosine = sine_and_cosine(eccentric)
            f = eccentric - (multiplied(k, sine, 126) << 2) - m  # Q128 turns
            derivative = ONE - multiplied(e, cosine, 126)  # Q126, positive
            step = divided(f << 126, derivative)
            eccentric = min(max(eccentric - step, 0), HALF_TURN)
    if reflected:
        eccentric = (TURN - eccentric) % TURN
    sine, cosine = sine_and_cosine(eccentric)
    return eccentric, sine, cosine


def measure(ratios, turns_list, steps):
    worst_error, worst_scaled, worst_case = 0, 0.0, None
    for ratio in ratios:
        e = 1 - mpf(ratio)
        for turns in turns_list:
            found, sine, cosine = solve(ratio, turns, steps)
            mean_anomaly = 2 * pi * mpf(turns) / mpf(2) ** 128
            truth = reference.solve_eccentric_anomaly(mean_anomaly, e)
            truth_turns = truth / (2 * pi) * mpf(2) ** 128
            error = abs(mpf(found) - truth_turns)  # in units of 2^-128 turns (wrap-around is not an issue for these inputs)
            if error > mpf(2) ** 127:
                error = mpf(2) ** 128 - error
            amplification = float(1 / (1 - e * mp.cos(truth)))
            scaled = float(error) / amplification
            if scaled > worst_scaled:
                worst_scaled, worst_error, worst_case = scaled, float(error), (ratio, hex(turns), amplification)
    return worst_error, worst_scaled, worst_case


if __name__ == "__main__":
    random.seed(11)
    # log-spaced ratios from 1e-18 to just below 1, plus the corners; mean anomalies: random, tiny, near half, near a turn
    ratios = [1.0, 1.0 - 2.0 ** -53, 1.0 - 2.0 ** -40, 1.0 - 1e-9, 1.0 - 1e-6, 0.999, 0.9, 0.7, 0.5, 0.3, 0.1, 1e-2, 1e-3, 1e-4, 1e-6, 1e-8, 1e-10, 1e-12, 1e-14, 1e-16, 1e-18]
    cases = []
    for ratio in ratios:
        for _ in range(60):
            kind = random.random()
            if kind < 0.4:
                turns = random.getrandbits(128)
            elif kind < 0.6:
                turns = random.getrandbits(random.randint(1, 120))  # just after periapsis
            elif kind < 0.8:
                turns = TURN - random.getrandbits(random.randint(1, 120))  # just before it
            else:
                turns = HALF_TURN + random.randint(-(1 << 60), 1 << 60)  # round apoapsis
            cases.append((ratio, turns % TURN))
    print(len(cases), "cases; units 2^-128 turn")
    for steps in (2, 3, 4, 6):
        worst_scaled, worst = 0.0, None
        for ratio, turns in cases:
            found, sine, cosine = solve(ratio, turns, steps)
            e = 1 - mpf(ratio)
            truth = reference.solve_eccentric_anomaly(2 * pi * mpf(turns) / mpf(2) ** 128, e)
            error = abs(mpf(found) - truth / (2 * pi) * mpf(2) ** 128)
            if error > mpf(2) ** 127:
                error = mpf(2) ** 128 - error
            amplification = float(1 / (1 - e * mp.cos(truth)))
            scaled = float(error) / max(amplification, 1.0)
            if scaled > worst_scaled:
                worst_scaled, worst = scaled, (ratio, hex(turns), float(error), amplification)
        print(f"steps {steps}: worst error / max(amplification, 1) = {worst_scaled:.3g}  at {worst}")
