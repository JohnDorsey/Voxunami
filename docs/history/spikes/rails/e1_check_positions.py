"""Checks the positions that part D of `e1_fine_control_test.rs` prints against the 120-digit reference of `engine/world_storage/tools/exact_ellipse_reference.py`.

    python e1_check_positions.py LOG   # LOG is the output of the test (--nocapture); run from anywhere, it finds the reference by its path

THROWAWAY (E1 of `docs/orbits-and-thrust-plan.md`).
"""
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TOOLS = os.path.join(HERE, "..", "..", "..", "..", "engine", "world_storage", "tools")
sys.path.insert(0, TOOLS)

from mpmath import mpf  # noqa: E402

import exact_ellipse_reference as reference  # noqa: E402


def double(bits):
    return struct.unpack(">d", struct.pack(">Q", int(bits, 16)))[0]


def main(path):
    # (The first line a test prints comes after its name on the same line.)
    lines = [line[line.index("D: "):].strip() for line in open(path, encoding="utf-8", errors="replace") if "D: " in line]
    orbit = next(line for line in lines if line.startswith("D: orbit")).split()[2:]
    radius, ratio, periapsis, period, phase = [double(bits) for bits in orbit]
    normal = [double(bits) for bits in next(line for line in lines if line.startswith("D: normal")).split()[2:]]
    worst = 0.0
    for line in lines:
        if not line.startswith("D: tick"):
            continue
        parts = line.split()
        tick = int(parts[2])
        bricks = [int(parts[4]), int(parts[5]), int(parts[6])]
        rest = [double(parts[8]), double(parts[9]), double(parts[10])]
        expected_bricks, expected_rest, _ = reference.offset(normal, radius, ratio, periapsis, period, phase, tick, 20)
        ours = [32 * b + mpf(r) for b, r in zip(bricks, rest)]
        theirs = [32 * b + r for b, r in zip(expected_bricks, expected_rest)]
        off = max(abs(a - b) for a, b in zip(ours, theirs))
        worst = max(worst, float(off))
        print(f"tick {tick:>20}: largest difference from the reference {float(off):.3e} m")
    print(f"worst {worst:.3e} m")


if __name__ == "__main__":
    main(sys.argv[1])
