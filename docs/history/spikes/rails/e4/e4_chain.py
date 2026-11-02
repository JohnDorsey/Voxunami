"""E4b of `docs/orbits-and-thrust-plan.md`: how many bits does a chain of gravity assists need? THROWAWAY, kept to reproduce the numbers there.

    python e4_chain.py design N SEED FILE     # designs a chain of N flybys (each aimed at the periapsis wanted, from the path the ship really takes) and writes it to FILE (JSON)
    python e4_chain.py ladder FILE            # runs the chain at a ladder of working precisions and says what each gives for the periapsis of the last flyby, and whether comparing
                                              # a run with a run at twice the bits (the detector) says the right thing
    python e4_chain.py legs FILE              # the same for each leg alone (a pilot who corrects at every lens): the bits one leg needs
    python e4_chain.py adaptive FILE          # the driver the owner asked about: double the precision until a run and a run at twice the bits agree, for horizons K = 1, 2, 3, ...
    python e4_chain.py interval FILE          # the chain in interval arithmetic (mpmath.iv): the bits at which the range is under the tolerance
    python e4_chain.py export FILE OUT        # the chain as plain text for the Rust program in e4/; `compare FILE "RESULT line"` compares what it printed with the truth

The model is the one the owner chose for now (2026-11-02): patched conics, with straight lines between the spheres of influence. The bodies (stars, heavy stars, black holes) go
in straight lines at a constant speed (a proper motion) and each pulls inside its sphere of influence only, with its own `mu` (G M). Everything inside a sphere is closed
form: the ship goes in on a hyperbola, which is symmetric about its line of apsides, so it comes out where the mirror image of the place it went in is (a half turn about the
eccentricity vector) with the mirror velocity; the time inside is twice the time from the periapsis to the sphere, from the hyperbolic Kepler equation.

Inputs are exact numbers of at most 128 bits (positions are multiples of 2^-10 m, speeds and masses are doubles), so any working precision of 192 bits or more holds them exactly,
and the only difference between two runs is the rounding of the working: the sensitivity of the answer to it is what is measured.
"""
import json
import math
import random
import sys
from fractions import Fraction

from mpmath import acosh, mp, mpf, sinh, sqrt, workprec

GRID = Fraction(1, 1024)  # positions are given to a millimetre
TOLERANCE = mpf("1e-4")  # metres: a tenth of a millimetre, a tenth of the promise


def exact(value):
    """A number given as a Fraction or float, as the mpf that holds it exactly (at 192 bits or more)."""
    if isinstance(value, Fraction):
        return mpf(value.numerator) / mpf(value.denominator)
    return mpf(value)


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def add(a, b):
    return [a[0] + b[0], a[1] + b[1], a[2] + b[2]]


def sub(a, b):
    return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]


def scale(a, k):
    return [a[0] * k, a[1] * k, a[2] * k]


def norm(a):
    return sqrt(dot(a, a))


class Broken(Exception):
    """The ship does not go through the next sphere as a hyperbola (it misses it, or is bound): the precision of the run was not enough, or the chain was not one."""


def flyby(lens, x0, t0, v):
    """From the ship at `x0` at time `t0` with the velocity `v`: through the sphere of `lens`. Returns (exit place, exit time, exit velocity, periapsis)."""
    mu, rho = lens["mu"], lens["rho"]
    w = lens["w"]
    p0 = lens["p0"]
    u = sub(v, w)  # the velocity against the lens
    q = sub(x0, add(p0, scale(w, t0)))  # the place against the lens, at t0
    uu = dot(u, u)
    qu = dot(q, u)
    disc = qu * qu - uu * (dot(q, q) - rho * rho)
    if disc <= 0 or qu >= 0:
        raise Broken("misses the sphere")
    s = (-qu - sqrt(disc)) / uu
    r0 = add(q, scale(u, s))
    energy = uu / 2 - mu / rho
    if energy <= 0:
        raise Broken("bound")
    h = cross(r0, u)
    k = sub(scale(cross(u, h), 1 / mu), scale(r0, 1 / rho))
    e = norm(k)
    khat = scale(k, 1 / e)
    r1 = sub(scale(khat, 2 * dot(r0, khat)), r0)
    v1 = sub(u, scale(khat, 2 * dot(u, khat)))
    a = mu / (2 * energy)
    n = sqrt(mu / (a * a * a))
    big_h = acosh((1 + rho / a) / e)
    mean = e * sinh(big_h) - big_h
    t1 = t0 + s + 2 * mean / n
    place = add(add(p0, scale(w, t1)), r1)
    return place, t1, add(w, v1), a * (e - 1)


def run(chain, start, precision, only=None):
    """The periapsis of every flyby of the chain (a list), at the working precision; `only` runs the one lens with that index from the stated start."""
    with workprec(precision):
        x = [exact(c) for c in start["x"]]
        v = [exact(c) for c in start["v"]]
        t = exact(start["t"])
        periapsides = []
        lenses = []
        for lens in chain:
            lenses.append(
                {
                    "mu": exact(lens["mu"]),
                    "rho": exact(lens["rho"]),
                    "w": [exact(c) for c in lens["w"]],
                    "p0": [exact(c) for c in lens["p0"]],
                }
            )
        for index, lens in enumerate(lenses):
            if only is not None and index != only:
                continue
            x, t, v, periapsis = flyby(lens, x, t, v)
            periapsides.append(periapsis)
        return periapsides, (x, t, v)


KINDS = [
    # (weight, mu in m^3/s^2, sphere of influence in m, least periapsis, most periapsis)
    (0.70, 1.3e20, 3.0e15, 5.0e8, 5.0e9),  # a star
    (0.20, 5.0e21, 1.0e16, 2.0e9, 2.0e10),  # a heavy star
    (0.10, 5.3e26, 1.0e18, 1.0e11, 1.0e12),  # a black hole of four million suns
]


def design(count, seed, precision):
    """A chain of `count` flybys, each placed from the path the ship really takes (at `precision` bits) so that its periapsis is the one wanted."""
    rng = random.Random(seed)
    x = [Fraction(0)] * 3
    v = [48_000.0, 3_000.0, -2_000.0]
    start = {"x": [str(c) for c in x], "v": v, "t": "0"}
    chain = []
    with workprec(precision):
        xm = [exact(c) for c in x]
        vm = [exact(c) for c in v]
        tm = mpf(0)
        for _ in range(count):
            roll, total = rng.random(), 0.0
            for weight, mu, rho, low, high in KINDS:
                total += weight
                if roll <= total:
                    break
            length = 10 ** rng.uniform(16.0, 18.0)
            w = [rng.uniform(-15_000, 15_000) for _ in range(3)]
            wm = [mpf(c) for c in w]
            u = sub(vm, wm)
            uu = dot(u, u)
            vinf2 = uu
            rp = mpf(10 ** rng.uniform(math.log10(low), math.log10(high)))
            b = sqrt(rp * rp + 2 * mu * rp / vinf2)
            speed = norm(vm)
            t_arrive = mpf(length) / speed
            line = add(xm, scale(vm, t_arrive))
            # A direction across the relative velocity.
            helper = [mpf(0), mpf(0), mpf(1)] if abs(float(u[2])) < 0.9 * float(norm(u)) else [mpf(1), mpf(0), mpf(0)]
            e1 = cross(u, helper)
            e1 = scale(e1, 1 / norm(e1))
            e2 = cross(u, e1)
            e2 = scale(e2, 1 / norm(e2))
            angle = rng.uniform(0, 2 * math.pi)
            across = add(scale(e1, math.cos(angle)), scale(e2, math.sin(angle)))
            place = add(line, scale(across, -b))  # the lens, at the time of the nearest approach of the straight relative line, is b across from the ship
            p0 = sub(place, scale(wm, tm + t_arrive))
            p0 = [Fraction(int(mp.floor(c * 1024 + mpf(1) / 2)), 1024) for c in p0]
            lens = {"mu": mu, "rho": rho, "w": w, "p0": [str(c) for c in p0]}
            chain.append(lens)
            exact_lens = {"mu": mpf(mu), "rho": mpf(rho), "w": wm, "p0": [exact(c) for c in p0]}
            xm, tm, vm, _ = flyby(exact_lens, xm, tm, vm)
    return start, chain


def load(path):
    data = json.load(open(path))
    start = data["start"]
    start["x"] = [Fraction(c) for c in start["x"]]
    start["t"] = Fraction(start["t"])
    chain = data["chain"]
    for lens in chain:
        lens["p0"] = [Fraction(c) for c in lens["p0"]]
    return start, chain


def save(path, start, chain):
    json.dump({"start": start, "chain": chain}, open(path, "w"))


def ladder_precisions(top):
    precisions, p = [], 192
    while p < top:
        precisions.append(p)
        p = int(p * 1.25) // 16 * 16 + 16
    return precisions


def do_ladder(path):
    start, chain = load(path)
    count = len(chain)
    truth_precision = 192 + 90 * count + 400
    truth, _ = run(chain, start, truth_precision)
    check, _ = run(chain, start, truth_precision + 600)
    agreement = abs(truth[-1] - check[-1])
    print(f"chain of {count}: truth at {truth_precision} bits (and {truth_precision + 600}: they agree to {float(agreement):.2e} m); last periapsis {float(truth[-1]):.12e} m")
    # How the answer moves with the input: a part in 2^(truth - 300) of the first speed, far above the rounding of the truth and far inside the range where the answer is linear.
    shift = Fraction(1, 2 ** (truth_precision - 300))
    moved = {"x": start["x"], "t": start["t"], "v": [Fraction(start["v"][0]) * (1 + shift), start["v"][1], start["v"][2]]}
    shifted, _ = run(chain, moved, truth_precision)
    with workprec(truth_precision):
        slope = abs(shifted[-1] - truth[-1]) / (exact(Fraction(start["v"][0])) * exact(shift))
    print(f"  the last periapsis moves by {float(slope):.2e} m for 1 m/s more of the first speed (about 2^{float(mp.log(slope, 2)):.0f}, taken with a shift of a part in 2^{truth_precision - 300})")
    rows = []
    for precision in ladder_precisions(truth_precision):
        try:
            result, _ = run(chain, start, precision)
            error = abs(result[-1] - truth[-1])
            wide, _ = run(chain, start, precision * 2)
            detector = abs(result[-1] - wide[-1])
            rows.append((precision, float(error), float(detector), False))
        except Broken as reason:
            rows.append((precision, float("inf"), float("inf"), True))
    needed = None
    for index, (precision, error, detector, broken) in enumerate(rows):
        if all(not row[3] and row[1] < float(TOLERANCE) for row in rows[index:]):
            needed = precision
            break
    flagged = None
    for index, (precision, error, detector, broken) in enumerate(rows):
        if all(not row[3] and row[2] < float(TOLERANCE) for row in rows[index:]):
            flagged = precision
            break
    false_accepts = [precision for precision, error, detector, broken in rows if not broken and detector < float(TOLERANCE) and error >= float(TOLERANCE)]
    false_rejects = [precision for precision, error, detector, broken in rows if (broken or detector >= float(TOLERANCE)) and (not broken and error < float(TOLERANCE))]
    print(f"  the detector (a run against a run at twice the bits) said enough where it was not: at {false_accepts or 'no precision'}; said not enough where it was: at {false_rejects or 'no precision'}; of {len(rows)} precisions")
    for precision, error, detector, broken in rows:
        print(f"  {precision:>6} bits: last periapsis off by {error:.3e} m; the run at twice the bits differs by {detector:.3e} m{'  BROKEN' if broken else ''}")
    print(f"  => bits needed for {float(TOLERANCE)} m: {needed}; the first precision at which comparing with the run at twice the bits says it is enough, and stays so: {flagged}")


def do_legs(path):
    start, chain = load(path)
    count = len(chain)
    truth_precision = 192 + 90 * count + 400
    # The state at the start of each leg, from the truth.
    states = [start]
    with workprec(truth_precision):
        x = [exact(c) for c in start["x"]]
        v = [exact(c) for c in start["v"]]
        t = exact(start["t"])
        for lens in chain:
            exact_lens = {"mu": mpf(lens["mu"]), "rho": mpf(lens["rho"]), "w": [mpf(c) for c in lens["w"]], "p0": [exact(c) for c in lens["p0"]]}
            x, t, v, _ = flyby(exact_lens, x, t, v)
            # Round the state to what a ship stores here: a step of 2^-30 (a nanometre), about 100 bits at the largest distances (the same state for every run).
            states.append({"x": [Fraction(int(c * 2**30), 2**30) for c in x], "v": [Fraction(int(c * 2**30), 2**30) for c in v], "t": Fraction(int(t * 2**30), 2**30)})
    needs = []
    for index, lens in enumerate(chain):
        begin = states[index]
        begin = {"x": begin["x"], "v": [Fraction(c) if not isinstance(c, float) else c for c in begin["v"]], "t": begin["t"]}
        reference, _ = run([lens], begin, 1500)
        found = None
        for precision in [64, 72, 80, 88, 96, 104, 112, 120, 128, 136, 144, 160, 176, 192, 224, 256, 320, 384, 512, 640, 768, 1024]:
            try:
                result, _ = run([lens], begin, precision)
            except Broken:
                continue
            if abs(result[-1] - reference[-1]) < TOLERANCE:
                found = precision
                break
        needs.append(found)
    values = [n for n in needs if n is not None]
    print(f"legs of {count} flybys, each from the exact state at the exit of the one before: bits one leg needs: smallest {min(values)}, median {sorted(values)[len(values) // 2]}, largest {max(values)} (none found for {needs.count(None)})")


def do_export(path, out):
    """The chain as plain text for the Rust program: a line `start x0 x1 x2 v0 v1 v2 t` and a line `lens mu rho w0 w1 w2 p0 p1 p2` for each lens (floats as Python writes them, positions as fractions)."""
    start, chain = load(path)
    lines = ["start " + " ".join(str(c) for c in start["x"]) + " " + " ".join(repr(c) for c in start["v"]) + " " + str(start["t"])]
    for lens in chain:
        lines.append("lens " + repr(lens["mu"]) + " " + repr(lens["rho"]) + " " + " ".join(repr(c) for c in lens["w"]) + " " + " ".join(str(c) for c in lens["p0"]))
    open(out, "w").write(chr(10).join(lines) + chr(10))
    print(f"wrote {len(chain)} lenses to {out}")


def do_compare(path, line):
    """Compares a RESULT line of the Rust program with the truth of the chain: the relative difference of the last periapsis."""
    start, chain = load(path)
    count = len(chain)
    truth_precision = 192 + 90 * count + 400
    truth, _ = run(chain, start, truth_precision)
    text = line.split("periapsis")[1].strip()
    mantissa, exponent = text.split("*2^")
    negative = mantissa.startswith("-")
    with workprec(truth_precision):
        value = mpf(int(mantissa.lstrip("-"), 16)) * mpf(2) ** int(exponent)
        value = -value if negative else value
        difference = abs(value - truth[-1])
        print(f"  the last periapsis differs from the truth by {float(difference):.3e} m ({float(difference / truth[-1]):.3e} of it); truth {float(truth[-1]):.15e}")


def do_adaptive(path):
    """The driver the owner asked about: increase the precision until a run and a run at twice the precision agree about every periapsis of the first K flybys, for K = 1, 2, 3, ... (the
    button pressed K times). Says the precision it settles on, the time it took (all the runs, the failed ones too) and the time of the one run that was needed."""
    import time

    start, chain = load(path)
    count = len(chain)
    truth_precision = 192 + 90 * count + 400
    truth, _ = run(chain, start, truth_precision)
    horizons = [k for k in [1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48, 64, 96, 128, 192, 256, 300] if k <= count]
    print(f"adaptive precision over a chain of {count} (tolerance {float(TOLERANCE)} m on every periapsis up to the horizon K)")
    for horizon in horizons:
        total = 0.0
        precision = 192
        attempts = 0
        while True:
            attempts += 1
            began = time.time()
            try:
                first, _ = run(chain[:horizon], start, precision)
                second, _ = run(chain[:horizon], start, precision * 2)
                agree = all(abs(a - b) < TOLERANCE for a, b in zip(first, second))
            except Broken:
                agree = False
            total += time.time() - began
            if agree:
                break
            precision = precision * 2
        began = time.time()
        final, _ = run(chain[:horizon], start, precision)
        one_run = time.time() - began
        error = max(float(abs(a - b)) for a, b in zip(final, truth[:horizon]))
        print(f"  K = {horizon:>3}: settles on {precision:>5} bits after {attempts} attempt(s), {total:7.2f} s in all; the one run at that precision takes {one_run:6.3f} s; its largest error against the truth: {error:.2e} m")


def interval_run(chain, start, precision):
    """The same chain in interval arithmetic (mpmath.iv): every number is a range that is certain to hold the exact value. Returns the range of the last periapsis, or raises Broken
    where an interval cannot say whether the ship goes through the next sphere."""
    from mpmath import iv

    iv.prec = precision

    def number(value):
        if isinstance(value, Fraction):
            return iv.mpf(value.numerator) / iv.mpf(value.denominator)
        return iv.mpf(value)

    def vec(values):
        return [number(c) for c in values]

    def idot(a, b):
        return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]

    def icross(a, b):
        return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]

    def isub(a, b):
        return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]

    def iadd(a, b):
        return [a[0] + b[0], a[1] + b[1], a[2] + b[2]]

    def iscale(a, k):
        return [a[0] * k, a[1] * k, a[2] * k]

    x, v, t = vec(start["x"]), vec(start["v"]), number(start["t"])
    periapsis = None
    for lens in chain:
        mu, rho = number(lens["mu"]), number(lens["rho"])
        w, p0 = vec(lens["w"]), vec(lens["p0"])
        u = isub(v, w)
        q = isub(x, iadd(p0, iscale(w, t)))
        uu, qu = idot(u, u), idot(q, u)
        disc = qu * qu - uu * (idot(q, q) - rho * rho)
        if not disc.a > 0 or not qu.b < 0:
            raise Broken("an interval cannot say whether it enters the sphere")
        s = (-qu - iv.sqrt(disc)) / uu
        r0 = iadd(q, iscale(u, s))
        energy = uu / 2 - mu / rho
        if not energy.a > 0:
            raise Broken("an interval cannot say whether it is bound")
        h = icross(r0, u)
        k = isub(iscale(icross(u, h), 1 / mu), iscale(r0, 1 / rho))
        e = iv.sqrt(idot(k, k))
        khat = iscale(k, 1 / e)
        r1 = isub(iscale(khat, 2 * idot(r0, khat)), r0)
        v1 = isub(u, iscale(khat, 2 * idot(u, khat)))
        a = mu / (2 * energy)
        n = iv.sqrt(mu / (a * a * a))
        argument = (1 + rho / a) / e
        big_h = iv.log(argument + iv.sqrt(argument * argument - 1))
        mean = e * ((iv.exp(big_h) - iv.exp(-big_h)) / 2) - big_h
        t = t + s + 2 * mean / n
        x = iadd(iadd(p0, iscale(w, t)), r1)
        v = iadd(w, v1)
        periapsis = a * (e - 1)
    return periapsis


def do_interval(path):
    start, chain = load(path)
    count = len(chain)
    truth_precision = 192 + 90 * count + 400
    truth, _ = run(chain, start, truth_precision)
    print(f"interval arithmetic over a chain of {count} (the width of the range of the last periapsis, against the error of the ordinary run at the same bits; the truth is {float(truth[-1]):.6e} m)")
    for precision in ladder_precisions(truth_precision):
        try:
            box = interval_run(chain, start, precision)
            width = float(box.b - box.a)
        except Broken as reason:
            print(f"  {precision:>6} bits: cannot decide ({reason})")
            continue
        except Exception as reason:
            print(f"  {precision:>6} bits: failed ({type(reason).__name__})")
            continue
        try:
            plain, _ = run(chain, start, precision)
            error = float(abs(plain[-1] - truth[-1]))
        except Broken:
            error = float("inf")
        print(f"  {precision:>6} bits: range {width:.3e} m wide; the ordinary run is off by {error:.3e} m{'  <- range under the tolerance' if width < float(TOLERANCE) else ''}")


if __name__ == "__main__":
    mp.prec = 192
    mode = sys.argv[1]
    if mode == "design":
        count, seed, path = int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
        start, chain = design(count, seed, 192 + 90 * count + 200)
        save(path, start, chain)
        print(f"designed {count} flybys to {path}")
    elif mode == "ladder":
        do_ladder(sys.argv[2])
    elif mode == "legs":
        do_legs(sys.argv[2])
    elif mode == "export":
        do_export(sys.argv[2], sys.argv[3])
    elif mode == "compare":
        do_compare(sys.argv[2], sys.argv[3])
    elif mode == "interval":
        do_interval(sys.argv[2])
    elif mode == "adaptive":
        do_adaptive(sys.argv[2])
