"""E4 addendum of `docs/orbits-and-thrust-plan.md` (2026-11-02): the detector with a cheaper check. THROWAWAY, kept to reproduce the numbers there.

    python detector_margin.py CHAIN.json        # a chain made by `e4_chain.py design`

`do_adaptive` of `e4_chain.py` checks a run at `p` bits against a run at `2p`. The error of the run at `p + 64` is 2^-64 of the error of the run at `p`, so a run only 64 bits wider is as good a
stand-in for the truth, and costs a good deal less than one twice as wide. This runs the same search (start at 192 bits, double until the run and the check agree to the tolerance on every
periapsis of the first K flybys) with both checks and says the precision each settles on, the attempts, and the whole search in units of the one run at the precision it settled on.
"""
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import e4_chain as chain
from mpmath import mp

mp.prec = 192


def search(chain_data, start, horizon, check):
    """Doubles the precision until a run and the check run agree on every periapsis of the first `horizon` flybys. Returns (precision, attempts, seconds of the whole search)."""
    total = 0.0
    precision = 192
    attempts = 0
    while True:
        attempts += 1
        began = time.time()
        try:
            first, _ = chain.run(chain_data[:horizon], start, precision)
            second, _ = chain.run(chain_data[:horizon], start, check(precision))
            agree = all(abs(a - b) < chain.TOLERANCE for a, b in zip(first, second))
        except chain.Broken:
            agree = False
        total += time.time() - began
        if agree:
            return precision, attempts, total
        precision *= 2


def main(path):
    start, chain_data = chain.load(path)
    count = len(chain_data)
    truth_precision = 192 + 90 * count + 400
    truth, _ = chain.run(chain_data, start, truth_precision)
    print(f"chain of {count}")
    for horizon in [k for k in (6, 12, 24, 48, 96) if k <= count]:
        row = []
        for name, check in (("twice the bits", lambda p: p * 2), ("64 bits wider", lambda p: p + 64)):
            precision, attempts, total = search(chain_data, start, horizon, check)
            began = time.time()
            final, _ = chain.run(chain_data[:horizon], start, precision)
            one_run = time.time() - began
            error = max(float(abs(a - b)) for a, b in zip(final, truth[:horizon]))
            row.append(f"{name}: settles on {precision} bits, {attempts} attempt(s), {total / one_run:4.1f} times the one run, error {error:.1e} m")
        print(f"  K = {horizon:>3}: " + " | ".join(row))


if __name__ == "__main__":
    main(sys.argv[1])
