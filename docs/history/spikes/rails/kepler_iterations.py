"""THROWAWAY. How many Newton / Halley steps does Kepler's equation need to reach 2^-126 (and 2^-254) as e goes to 1?

Counts steps of a guarded Newton method (bracket [0, pi] kept, a step outside the bracket becomes a bisection) from three starting values, for mean anomalies from
tiny to pi. Arithmetic is mpmath at 800 bits, so what is counted is the method, not a rounding.
"""
from mpmath import mp, mpf, sin, cos, pi, cbrt

mp.prec = 800


def reference(mean_anomaly, e):
    low, high = mpf(0), pi
    for _ in range(700):
        middle = (low + high) / 2
        if middle - e * sin(middle) - mean_anomaly > 0:
            high = middle
        else:
            low = middle
    return (low + high) / 2


def iterations(mean_anomaly, e, start, tolerance, method):
    low, high = mpf(0), pi
    eccentric = start
    for count in range(1, 400):
        f = eccentric - e * sin(eccentric) - mean_anomaly
        if f > 0:
            high = min(high, eccentric)
        else:
            low = max(low, eccentric)
        d1 = 1 - e * cos(eccentric)
        d2 = e * sin(eccentric)
        if method == "newton":
            step = f / d1
        else:  # Halley
            step = f / (d1 - f * d2 / (2 * d1))
        candidate = eccentric - step
        if not (low < candidate < high):
            candidate = (low + high) / 2
            bisected = True
        else:
            bisected = False
        eccentric = candidate
        if abs(step) < tolerance and not bisected:
            return count
    return None


def start_plain(mean_anomaly, e):
    return mean_anomaly + e * sin(mean_anomaly) if mean_anomaly > 0 else mpf(0)


def start_danby(mean_anomaly, e):
    return mean_anomaly + mpf("0.85") * e


def start_cube_root(mean_anomaly, e):
    # E^3/6 + (1-e) E = M, solved as a cubic (Cardano) -- the right shape near e = 1, small M
    delta = 1 - e
    p = 6 * delta
    q = -6 * mean_anomaly
    discriminant = (q / 2) ** 2 + (p / 3) ** 3
    square_root = discriminant ** mpf(0.5)

    def real_cbrt(value):
        return cbrt(value) if value >= 0 else -cbrt(-value)

    root = real_cbrt(-q / 2 + square_root) + real_cbrt(-q / 2 - square_root)
    return min(max(root, mpf(0)), pi)


starts = {"plain": start_plain, "danby": start_danby, "cubic": start_cube_root}
print("worst number of steps over M in {1e-30 ... 3.1}, 'x' = did not reach the tolerance in 400")
for bits in (126, 254):
    tolerance = mpf(2) ** -bits
    print(f"--- tolerance 2^-{bits}")
    print(f"{'1-e':>9} " + " ".join(f"{name + '/' + method:>14}" for name in starts for method in ("newton", "halley")))
    for delta in ("0.5", "0.1", "1e-3", "1e-6", "1e-9", "1e-12", "1e-20", "1e-30"):
        e = 1 - mpf(delta)
        row = []
        for name, start_function in starts.items():
            for method in ("newton", "halley"):
                worst = 0
                for exponent in (-30, -20, -12, -8, -5, -3, -2, -1, 0, 1):
                    mean_anomaly = min(mpf(10) ** exponent, pi) if exponent < 1 else mpf("3.1")
                    count = iterations(mean_anomaly, e, start_function(mean_anomaly, e), tolerance, method)
                    worst = max(worst, 999 if count is None else count)
                row.append("x" if worst == 999 else str(worst))
        print(f"{delta:>9} " + " ".join(f"{cell:>14}" for cell in row))
