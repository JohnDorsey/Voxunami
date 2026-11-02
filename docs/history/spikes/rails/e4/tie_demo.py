"""E4 addendum of `docs/orbits-and-thrust-plan.md` (2026-11-02): what the detector (a run against a run at twice the bits) says about a flyby that grazes the sphere of influence exactly (a tie), and
about one that misses the tie by 2^-300 and by 2^-1000. THROWAWAY, kept to reproduce the numbers there.

    python tie_demo.py

One ship goes along +x at 1e4 m/s and passes a lens at the origin at a distance of exactly 3e14 m; the lens has a sphere of influence of 3e14 m (plus the margin). `e4_chain.py` (beside this file)
is the model. A run that cannot say the ship enters the sphere raises `Broken`, which the loop of `do_adaptive` counts as a disagreement; here the outcomes of the two runs are compared as words.
"""
import os
import sys
from fractions import Fraction

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import e4_chain as chain
from mpmath import mp

mp.prec = 192

START = {"x": [Fraction(-(10**16)), Fraction(3 * 10**14), Fraction(0)], "v": [1.0e4, 0.0, 0.0], "t": Fraction(0)}


def lens(rho):
    return {"mu": 1.0e20, "rho": rho, "w": [0.0, 0.0, 0.0], "p0": [Fraction(0)] * 3}


def attempt(rho, precision):
    try:
        periapsis, _ = chain.run([lens(rho)], START, precision)
        return f"goes in, periapsis {float(periapsis[0]):.6e} m"
    except chain.Broken as reason:
        return f"BROKEN ({reason})"


if __name__ == "__main__":
    for name, rho in [
        ("exact tie: rho = 3e14", Fraction(3 * 10**14)),
        ("a margin of 2^-300: rho = 3e14 + 2^-300", Fraction(3 * 10**14) + Fraction(1, 2**300)),
        ("a margin of 2^-1000: rho = 3e14 + 2^-1000", Fraction(3 * 10**14) + Fraction(1, 2**1000)),
    ]:
        print(name)
        precision = 192
        while precision <= 6144:
            first = attempt(rho, precision)
            second = attempt(rho, precision * 2)
            verdict = "agree" if first == second else "DISAGREE"
            print(f"  {precision:>5} bits: {first:<48} | at twice the bits: {second:<48} -> {verdict}")
            precision *= 2
