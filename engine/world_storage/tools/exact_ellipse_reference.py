"""Reference values for the elliptical orbits of `docs/orbits-and-thrust-plan.md` (section A, step A1), worked out with `mpmath` at 120 digits.

    python exact_ellipse_reference.py selfcheck   # checks this program against mathematics, and against the circle that is the ellipse with ratio 1.0
    python exact_ellipse_reference.py kepler      # rows for the test of the Kepler solver: (ratio, mean anomaly, eccentric anomaly, sine, cosine, x, y, amplification)
    python exact_ellipse_reference.py orbits      # rows for the test of `Orbit`: the offset and the velocity at a tick, from the elements as the program keeps them

Run it with `mpmath` installed (`pip install mpmath`), from this directory. Nothing here is read by the build: the rows are pasted into the Rust tests.

What an elliptical orbit is (the definition the Rust must reproduce; `exact_circle_reference.py` defines the circle it contains):

- The elements are the ones of a circle (`normal`, `radius`, `period`, `phase`) and two more: `ratio` = 1 - e, with e the eccentricity (so 1.0 is a circle and a small number is a
  very long ellipse; an `f64` holds 1 - e to full precision where it holds e to 1e-16 of the whole), and `periapsis`, an angle in radians. `radius` is the semi-major axis a.
- The plane has the two directions `first` and `second` of the circle. The nearest point of the orbit, the periapsis, is in the direction p = cos(periapsis) first + sin(periapsis) second;
  q = -sin(periapsis) first + cos(periapsis) second is a quarter turn on from it, counter-clockwise looking down the normal.
- The mean anomaly is M = phase + 2 pi (turns gone), the turns gone being time / period taken modulo one in whole numbers, as for the circle; a negative period goes the other way round.
- Kepler's equation E - e sin E = M gives the eccentric anomaly E, and the place is x p + y q with x = a (cos E - e), y = a sqrt(1 - e^2) sin E.
- The speed is (n a / (1 - e cos E)) (-sin E p + sqrt(1 - e^2) cos E q) with n = 2 pi / period (negative for a negative period).
- With ratio 1.0, E = M and the place is the circle at the angle M + periapsis.
"""
import math
import random
import sys

from mpmath import floor, mp, mpf, pi, sin, cos, sqrt

import exact_circle_reference as circle

mp.dps = 120
TWO_PI = 2 * pi
ONE = 1 << 126


def fixed(value):
    """`value` with 126 bits after the point, rounded to the nearest."""
    return int(floor(value * ONE + mpf(1) / 2))


def solve_eccentric_anomaly(mean_anomaly, e):
    """E - e sin E = M for M in [0, 2 pi): bisection to the working precision, then Newton. (The conditioning is 1 / (1 - e cos E): up to 1e18 here, of 1e120.)"""
    if mean_anomaly == 0:
        return mpf(0)
    if e == 0:
        return mean_anomaly
    if mean_anomaly > pi:
        return TWO_PI - solve_eccentric_anomaly(TWO_PI - mean_anomaly, e)
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


def directions(normal, periapsis):
    """p and q: the direction of periapsis and the one a quarter turn on, from the normal as the program keeps it."""
    first, second = circle.basis(circle.stored(normal))
    c, s = cos(mpf(periapsis)), sin(mpf(periapsis))
    p = [c * first[i] + s * second[i] for i in range(3)]
    q = [-s * first[i] + c * second[i] for i in range(3)]
    return p, q


def mean_anomaly_at(period, phase, tick, ticks_per_second):
    """M in [0, 2 pi): the phase and the turns gone, as the circle takes them."""
    turns = mpf(tick) / (ticks_per_second * mpf(period))
    turns -= floor(turns)
    total = turns + mpf(phase) / TWO_PI
    total -= floor(total)
    return TWO_PI * total


def place_and_velocity(normal, radius, ratio, periapsis, period, mean_anomaly):
    """`(voxels, velocity)` from the centre, as lists of mpf, at the mean anomaly (in radians, any value)."""
    e = 1 - mpf(ratio)
    a = mpf(radius)
    reduced = mean_anomaly - TWO_PI * floor(mean_anomaly / TWO_PI)
    eccentric = solve_eccentric_anomaly(reduced, e)
    root = sqrt(1 - e * e)
    x = a * (cos(eccentric) - e)
    y = a * root * sin(eccentric)
    p, q = directions(normal, periapsis)
    voxels = [x * p[i] + y * q[i] for i in range(3)]
    factor = (TWO_PI / mpf(period)) * a / (1 - e * cos(eccentric))
    vx = -factor * sin(eccentric)
    vy = factor * root * cos(eccentric)
    velocity = [vx * p[i] + vy * q[i] for i in range(3)]
    return voxels, velocity


def offset(normal, radius, ratio, periapsis, period, phase, tick, ticks_per_second):
    """As `exact_circle_reference.offset`: whole bricks and a rest, and the velocity."""
    mean_anomaly = mean_anomaly_at(period, phase, tick, ticks_per_second)
    voxels, velocity = place_and_velocity(normal, radius, ratio, periapsis, period, mean_anomaly)
    bricks = [int(floor(c / 32)) for c in voxels]
    return bricks, [c - 32 * b for c, b in zip(voxels, bricks)], velocity


def in_voxels(bricks, rest):
    return [32 * b + r for b, r in zip(bricks, rest)]


# ---- the cases

RATIOS = [1.0, 0.99, 0.9, 0.5, 0.1, 1e-3, 1e-6, 1e-12, 1e-18]


def mean_anomaly_turns():
    """Mean anomalies as turns with 128 bits after the point: the corners, the neighbourhood of periapsis (the hard place for a near parabola) and random ones."""
    random.seed(20261101)
    turns = [0, 1, (1 << 128) - 1, 1 << 127, (1 << 127) - 1, (1 << 127) + 1, 1 << 126, 3 << 126, (1 << 125) + 12345]
    turns += [1 << shift for shift in (100, 80, 60, 40)]  # just after periapsis
    turns += [(1 << 128) - (1 << shift) for shift in (100, 60)]  # just before it
    turns += [random.getrandbits(128) for _ in range(8)]
    return turns


def orbit_cases():
    """(normal, radius, ratio, periapsis, period, phase, tick, ticks a second): the circles of `exact_circle_reference.py` as ellipses, and some that are not circles."""
    cases = []
    circles = [
        ([0.0, 1.0, 0.0], 6.0e26, 3.156e7, 0.7, 12_345),
        ([1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0], 2.5e20, 7.0e15, 2.0, 7_600_000_000_037),
        ([0.6, 0.8, 0.0], 1.0e18, 5.0e9, -1.25, 333),
        ([0.0, 1.0, 0.0], 1.5e11, 3.156e7, 0.0, 0),
        ([0.2672612419124244, -0.5345224838248488, 0.8017837257372732], 8.8e26, -6.3e12, 3.0, 99_999_999_999),
        ([0.0, 0.0, 1.0], 3.84e8, 2.36e6, 5.5, 18_446_744_073_709_551_615),
    ]
    for normal, radius, period, phase, tick in circles:
        cases.append((normal, radius, 1.0, 0.0, period, phase, tick, 20))
    ellipses = [
        # normal, radius a, ratio, periapsis, period, phase, tick
        ([0.0, 1.0, 0.0], 1.5e11, 0.0167, 1.8, 3.156e7, 0.3, 4_000_000),
        ([0.0, 1.0, 0.0], 1.5e11, 0.0167, 1.8, 3.156e7, 0.3, 0),
        ([0.0, 0.0, 1.0], 3.84e8, 0.945, 0.0, 2.36e6, 5.5, 18_446_744_073_709_551_615),
        ([0.6, 0.8, 0.0], 1.0e12, 0.1, -2.5, -5.0e8, -1.25, 333_333_333),
        ([0.2672612419124244, -0.5345224838248488, 0.8017837257372732], 8.8e26, 1e-6, 3.0, 6.3e12, 1.0, 99_999_999_999),
        ([0.0, 1.0, 0.0], 1.0e22, 1e-12, 0.5, 1.0e22 ** 1.5 / 1.0e10, 0.0, 1),  # a very long ellipse, one tick after the start
        ([0.0, 1.0, 0.0], 1.0e22, 1e-12, 0.5, 1.0e22 ** 1.5 / 1.0e10, 0.0, 987_654_321_012_345),
        ([0.95, 0.1, 0.2], 7.0e6, 0.3, 2.0, 5_400.0, 6.28, 123_456_789),
        ([-0.3, 0.5, -0.8], 2.2e13, 0.02, -3.0, -4.4e8, -2.5, 55_555_555_555),
    ]
    for normal, radius, ratio, periapsis, period, phase, tick in ellipses:
        cases.append((normal, radius, ratio, periapsis, period, phase, tick, 20))
    return cases


# ---- the output

def kepler():
    turns_list = mean_anomaly_turns()
    print("// (ratio, mean anomaly in turns x 2^128, eccentric anomaly in turns x 2^128, sin E, cos E, x / a = cos E - e, y / a = sqrt(1 - e^2) sin E: all x 2^126, and 1 / (1 - e cos E))")
    print("const KEPLER: [(f64, u128, u128, i128, i128, i128, i128, f64); %d] = [" % (len(RATIOS) * len(turns_list)))
    for ratio in RATIOS:
        e = 1 - mpf(ratio)
        for turns in turns_list:
            mean_anomaly = TWO_PI * mpf(turns) / mpf(2) ** 128
            eccentric = solve_eccentric_anomaly(mean_anomaly, e)
            eccentric_turns = int(floor(eccentric / TWO_PI * mpf(2) ** 128 + mpf(1) / 2)) % (1 << 128)
            print("    (%r, 0x%032x, 0x%032x, %d, %d, %d, %d, %r)," % (
                ratio, turns, eccentric_turns,
                fixed(sin(eccentric)), fixed(cos(eccentric)),
                fixed(cos(eccentric) - e), fixed(sqrt(1 - e * e) * sin(eccentric)),
                float(1 / (1 - e * cos(eccentric))),
            ))
    print("];")


def orbits():
    cases = orbit_cases()
    print("// (normal, radius, ratio, periapsis, period, phase, tick, ticks a second, bricks, rest, velocity)")
    print("const ELLIPSES: [PinnedEllipse; %d] = [" % len(cases))
    for normal, radius, ratio, periapsis, period, phase, tick, ticks_per_second in cases:
        bricks, rest, velocity = offset(normal, radius, ratio, periapsis, period, phase, tick, ticks_per_second)
        print("    (%r, %r, %r, %r, %r, %r, %d, %d, %r, [%s], [%s])," % (
            normal, radius, ratio, periapsis, period, phase, tick, ticks_per_second, bricks,
            ", ".join(repr(float(x)) for x in rest), ", ".join(repr(float(x)) for x in velocity)))
    print("];")


# ---- the program checked against mathematics

def selfcheck():
    failures = []

    def check(condition, message):
        if not condition:
            failures.append(message)

    tiny = mpf(10) ** -100

    # 1. Kepler's equation holds for every case of the table, whatever the conditioning.
    count = 0
    for ratio in RATIOS:
        e = 1 - mpf(ratio)
        for turns in mean_anomaly_turns():
            mean_anomaly = TWO_PI * mpf(turns) / mpf(2) ** 128
            eccentric = solve_eccentric_anomaly(mean_anomaly, e)
            residual = abs(eccentric - e * sin(eccentric) - mean_anomaly)
            check(residual < tiny, "Kepler residual %s for ratio %s, turns %x" % (mp.nstr(residual, 5), ratio, turns))
            check(0 <= eccentric < TWO_PI, "E out of range for ratio %s, turns %x" % (ratio, turns))
            count += 1
    print("1. Kepler's equation: %d cases, residual below 1e-100" % count)

    # 2. With ratio 1.0 and periapsis 0 an ellipse is the circle of exact_circle_reference.py, on the cases that file pins.
    worst = mpf(0)
    for normal, radius, ratio, periapsis, period, phase, tick, ticks_per_second in orbit_cases():
        if ratio != 1.0 or periapsis != 0.0:
            continue
        ellipse_bricks, ellipse_rest, _ = offset(normal, radius, ratio, periapsis, period, phase, tick, ticks_per_second)
        circle_bricks, circle_rest = circle.offset(normal, radius, period, phase, tick, ticks_per_second)
        apart = max(abs(x - y) for x, y in zip(in_voxels(ellipse_bricks, ellipse_rest), in_voxels(circle_bricks, circle_rest)))
        worst = max(worst, apart)
    check(worst < mpf(10) ** -90, "a circle is not the ellipse with ratio 1.0: %s voxels apart" % mp.nstr(worst, 5))
    print("2. ratio 1.0 is the circle: worst difference %s voxels (of radii up to 8.8e26)" % mp.nstr(worst, 3))

    # 3. With ratio 1.0 the periapsis angle only turns the circle: the same as a phase that much greater.
    for periapsis in (0.7, -2.0, 5.0):
        a = offset([0.6, 0.8, 0.0], 1.0e18, 1.0, periapsis, 5.0e9, 0.2, 333, 20)
        # (the sum in mpmath, not in f64: at a radius of 1e18 a rounding of 1e-16 radian is a hundred voxels)
        b = offset([0.6, 0.8, 0.0], 1.0e18, 1.0, 0.0, 5.0e9, mpf(0.2) + mpf(periapsis), 333, 20)
        apart = max(abs(x - y) for x, y in zip(in_voxels(a[0], a[1]), in_voxels(b[0], b[1])))
        check(apart < mpf(10) ** -90, "periapsis %s on a circle is not a phase: %s" % (periapsis, mp.nstr(apart, 5)))
    print("3. on a circle the periapsis angle is a phase")

    # 4. Periapsis and apoapsis: at M = 0 the body is at a (1 - e) toward p; at M = pi it is at a (1 + e) away from it.
    for ratio in (0.99, 0.5, 1e-6, 1e-18):
        e = 1 - mpf(ratio)
        normal, radius, periapsis = [0.6, 0.8, 0.0], 1.0e12, 0.7
        p, _ = directions(normal, periapsis)
        near, _ = place_and_velocity(normal, radius, ratio, periapsis, 5.0e8, mpf(0))
        far, _ = place_and_velocity(normal, radius, ratio, periapsis, 5.0e8, pi)
        for i in range(3):
            check(abs(near[i] - mpf(radius) * mpf(ratio) * p[i]) < mpf(10) ** -90 * mpf(radius), "periapsis point, ratio %s" % ratio)
            check(abs(far[i] + mpf(radius) * (1 + e) * p[i]) < mpf(10) ** -90 * mpf(radius), "apoapsis point, ratio %s" % ratio)
    print("4. periapsis at a (1 - e) toward p, apoapsis at a (1 + e) away from it")

    # 5. The velocity is the derivative of the place (central difference in the mean anomaly), vis-viva holds, and the angular momentum is constant and along the normal.
    random.seed(20261102)
    worst_difference = mpf(0)
    worst_vis_viva = mpf(0)
    worst_momentum = mpf(0)
    worst_sense = mpf(0)
    step = mpf(10) ** -40
    for _ in range(60):
        ratio = random.choice([0.9, 0.5, 0.1, 1e-3, 1e-6, 1e-12])
        e = 1 - mpf(ratio)
        normal = [random.uniform(-1, 1) for _ in range(3)]
        radius = 10 ** random.uniform(3, 20)
        period = random.choice([-1, 1]) * 10 ** random.uniform(2, 10)
        periapsis = random.uniform(-7, 7)
        mean_anomaly = mpf(random.uniform(0, 6.28))
        n = TWO_PI / mpf(period)
        place, velocity = place_and_velocity(normal, radius, ratio, periapsis, period, mean_anomaly)
        before, _ = place_and_velocity(normal, radius, ratio, periapsis, period, mean_anomaly - step)
        after, _ = place_and_velocity(normal, radius, ratio, periapsis, period, mean_anomaly + step)
        # d place / d t = d place / d M x n
        difference = [(after[i] - before[i]) / (2 * step) * n for i in range(3)]
        scale = sqrt(sum(v * v for v in velocity))
        worst_difference = max(worst_difference, max(abs(difference[i] - velocity[i]) for i in range(3)) / scale)
        distance = sqrt(sum(c * c for c in place))
        mu = n * n * mpf(radius) ** 3
        vis_viva = sum(v * v for v in velocity) - mu * (2 / distance - 1 / mpf(radius))
        worst_vis_viva = max(worst_vis_viva, abs(vis_viva) / sum(v * v for v in velocity))
        momentum = [
            place[1] * velocity[2] - place[2] * velocity[1],
            place[2] * velocity[0] - place[0] * velocity[2],
            place[0] * velocity[1] - place[1] * velocity[0],
        ]
        size = sqrt(sum(m * m for m in momentum))
        expected = abs(n) * mpf(radius) ** 2 * sqrt(1 - e * e)
        worst_momentum = max(worst_momentum, abs(size - expected) / expected)
        # counter-clockwise looking down the normal for a positive period, clockwise for a negative one
        # (the normal as the program keeps it is a unit vector to an f64, so it is made one again here, as `circle.basis` does)
        kept = [mpf(c) for c in circle.stored(normal)]
        length = sqrt(sum(c * c for c in kept))
        along = sum(momentum[i] * kept[i] / length for i in range(3)) / size
        worst_sense = max(worst_sense, abs(along - (1 if period > 0 else -1)))
    # (a central difference is wrong by (step / the scale the orbit changes on)^2: the scale is down to ratio^1.5 = 1e-18 in M for the sharpest, so 1e-44 of the speed)
    check(worst_difference < mpf(10) ** -35, "velocity is not the derivative of the place: %s" % mp.nstr(worst_difference, 5))
    check(worst_vis_viva < tiny, "vis-viva: %s" % mp.nstr(worst_vis_viva, 5))
    check(worst_momentum < tiny, "angular momentum: %s" % mp.nstr(worst_momentum, 5))
    check(worst_sense < tiny, "the sense of the orbit is not the normal's for the sign of the period: %s" % mp.nstr(worst_sense, 5))
    print("5. velocity = d place / d t (worst relative difference %s), vis-viva (%s), |r x v| = n a^2 sqrt(1 - e^2) (%s), r x v along the normal for a positive period and against it for a negative one (%s from +-1)"
          % (mp.nstr(worst_difference, 3), mp.nstr(worst_vis_viva, 3), mp.nstr(worst_momentum, 3), mp.nstr(worst_sense, 3)))

    # 6. Kepler's second law: equal areas in equal times, near periapsis and near apoapsis, for a very eccentric orbit (the sector swept by a step in M is a step x a^2 sqrt(1 - e^2) / 2).
    ratio, radius, normal = 1e-6, 1.0e15, [0.0, 1.0, 0.0]
    e = 1 - mpf(ratio)
    step = mpf(10) ** -30
    areas = []
    for mean_anomaly in (mpf("0.0000001"), mpf("1.0"), pi - mpf("0.0000001"), pi):
        before, _ = place_and_velocity(normal, radius, ratio, 0.0, 1.0e9, mean_anomaly)
        after, _ = place_and_velocity(normal, radius, ratio, 0.0, 1.0e9, mean_anomaly + step)
        # the triangle between the two places and the focus (the origin) is half |before x after|
        cross = [
            before[1] * after[2] - before[2] * after[1],
            before[2] * after[0] - before[0] * after[2],
            before[0] * after[1] - before[1] * after[0],
        ]
        areas.append(sqrt(sum(c * c for c in cross)) / 2 / step)
    expected = mpf(radius) ** 2 * sqrt(1 - e * e) / 2
    worst_area = max(abs(area - expected) / expected for area in areas)
    check(worst_area < mpf(10) ** -25, "areas swept: %s" % mp.nstr(worst_area, 5))
    print("6. equal areas in equal times, ratio 1e-6: worst relative difference %s" % mp.nstr(worst_area, 3))

    if failures:
        print("FAILED:")
        for failure in failures:
            print("  ", failure)
        sys.exit(1)
    print("all checks passed")


if __name__ == "__main__":
    command = sys.argv[1:]
    if command == ["selfcheck"]:
        selfcheck()
    elif command == ["kepler"]:
        kepler()
    elif command == ["orbits"]:
        orbits()
    else:
        sys.exit(__doc__)
