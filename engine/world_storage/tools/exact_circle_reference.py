"""Reference values for the circles of `docs/orbits-and-thrust-plan.md` (and `tests/rail.rs`, `tests/rail_circle_bits.rs`), worked out with `mpmath` at 120 digits.

    python exact_circle_reference.py vectors     # the offsets that tests/rail.rs pins, and the one of a ship round a ship in server_world

Run it with `mpmath` installed (`pip install mpmath`). Nothing here is read by the build: the values it prints are pasted into the Rust source, and the tests check the program against
them. `basis`, `stored` and `offset` are also what `exact_ellipse_reference.py` builds on. (Until 2026-11-03 this script also made the tables of the fixed-point sine and cosine that
placed the planets, and the planes and angles that the fit from a state used; they are gone, and where a planet is now comes from `engine/wide_kepler`, which has its own reference
rows from `engine/wide_kepler/tests/reference/generate.py`.)
"""
import math
import random
import sys

from mpmath import floor, mp, mpf, pi, sin, cos, sqrt

mp.dps = 120
def basis(normal):
    """The two directions of the plane, as `wide_kepler` makes them, from the normal as the program holds it."""
    n = [mpf(c) for c in normal]
    size = sqrt(sum(c * c for c in n))
    n = [c / size for c in n]
    axis = 0 if abs(normal[0]) < 0.9 else 2
    reference = [mpf(0)] * 3
    reference[axis] = mpf(1)
    along = sum(reference[i] * n[i] for i in range(3))
    inside = [reference[i] - n[i] * along for i in range(3)]
    inside_size = sqrt(sum(c * c for c in inside))
    first = [c / inside_size for c in inside]
    second = [
        n[1] * first[2] - n[2] * first[1],
        n[2] * first[0] - n[0] * first[2],
        n[0] * first[1] - n[1] * first[0],
    ]
    return first, second


def stored(normal):
    """What `Orbit::new` keeps of a normal: each coordinate over the `f64` length (the same `f64` operations as the program)."""
    size = math.sqrt(normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2])
    return [c / size for c in normal]


def offset(normal, radius, period, phase, tick, ticks_per_second, shift=(0, 0, 0)):
    """The offset from the centre of an orbit as whole bricks and a rest, plus `shift` voxels."""
    first, second = basis(stored(normal))
    turns = mpf(tick) / (ticks_per_second * mpf(period))
    turns -= floor(turns)
    total = turns + mpf(phase) / (2 * pi)
    total -= floor(total)
    angle = 2 * pi * total
    voxels = [mpf(radius) * (cos(angle) * first[i] + sin(angle) * second[i]) + shift[i] for i in range(3)]
    bricks = [int(floor(c / 32)) for c in voxels]
    return bricks, [c - 32 * b for c, b in zip(voxels, bricks)]


def vectors():
    random.seed(20261027)
    cases = [
        # normal, radius, period, phase, tick, ticks a second
        ([0.0, 1.0, 0.0], 6.0e26, 3.156e7, 0.7, 12_345, 20),
        ([1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0], 2.5e20, 7.0e15, 2.0, 7_600_000_000_037, 20),
        ([0.6, 0.8, 0.0], 1.0e18, 5.0e9, -1.25, 333, 20),
        ([0.0, 1.0, 0.0], 1.5e11, 3.156e7, 0.0, 0, 20),
        ([0.2672612419124244, -0.5345224838248488, 0.8017837257372732], 8.8e26, -6.3e12, 3.0, 99_999_999_999, 20),
        ([0.0, 0.0, 1.0], 3.84e8, 2.36e6, 5.5, 18_446_744_073_709_551_615, 20),
    ]
    print("const OFFSETS: [PinnedOffset; %d] = [" % len(cases))
    for normal, radius, period, phase, tick, ticks_per_second in cases:
        bricks, rest = offset(normal, radius, period, phase, tick, ticks_per_second)
        print("    (%r, %r, %r, %r, %d, %d, %r, [%s])," % (
            normal, radius, period, phase, tick, ticks_per_second, bricks, ", ".join(repr(float(x)) for x in rest)))
    print("];")

    # server_world.rs: a body 5e17 voxels round a ship at brick (1000, -2000, 3000), offset (3.5, 7.25, 30.0).
    parent = (1000 * 32 + mpf("3.5"), -2000 * 32 + mpf("7.25"), 3000 * 32 + mpf("30.0"))
    bricks, rest = offset([0.6, 0.8, 0.0], 5.0e17, 9.0e8, 1.0, 123_456_789, 20, shift=parent)
    print("// a ship round a ship: bricks", bricks, "offset", [repr(float(x)) for x in rest])


if __name__ == "__main__":
    if sys.argv[1:] == ["vectors"]:
        vectors()
    else:
        sys.exit(__doc__)
