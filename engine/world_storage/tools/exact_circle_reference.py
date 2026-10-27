"""Reference values for `src/exact_circle.rs`, worked out with `mpmath` at 120 digits.

    python exact_circle_reference.py constants   # the tables of exact_circle.rs: ANGLES, GAIN and INVERSE_TAU
    python exact_circle_reference.py vectors     # the expected values that exact_circle/tests.rs and tests/rail.rs pin

Run it with `mpmath` installed (`pip install mpmath`). Nothing here is read by the build: the values it prints are pasted into the Rust source, and the tests
check the program against them. A change to a table is a change to where every planet on a rail is, so it goes with new expected values and a note in
`docs/design-decisions.md`.
"""
import math
import random
import sys

from mpmath import floor, mp, mpf, pi, sin, cos, sqrt, atan

mp.dps = 120
ONE = 1 << 126
STEPS = 127  # CORDIC steps 0..126


def fixed(value):
    """`value` with 126 bits after the point, rounded to the nearest."""
    return int(floor(value * ONE + mpf(1) / 2))


def constants():
    # The angle whose tangent is 2^-step, in turns with 128 bits after the point.
    angles = [int(floor(atan(mpf(2) ** -step) / (2 * pi) * (mpf(2) ** 128) + mpf(1) / 2)) for step in range(STEPS)]
    # What the steps make too long: the product of 1 / sqrt(1 + 4^-step), 126 bits after the point.
    gain = mpf(1)
    for step in range(STEPS):
        gain /= sqrt(1 + mpf(2) ** (-2 * step))
    # One over two pi, times 2^192, rounded down: three 64-bit parts, the largest first.
    inverse_tau = int(floor((mpf(2) ** 192) / (2 * pi)))
    limbs = [(inverse_tau >> (64 * part)) & ((1 << 64) - 1) for part in (2, 1, 0)]
    print("const ANGLES: [u128; %d] = [" % STEPS)
    for angle in angles:
        print("    0x%032x," % angle)
    print("];")
    print("const GAIN: i128 = 0x%032x;" % fixed(gain))
    print("const INVERSE_TAU: [u64; 3] = [" + ", ".join("0x%016x" % limb for limb in limbs) + "];")


def basis(normal):
    """The two directions of the plane, as exact_circle::plane makes them, from the normal as the program holds it."""
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
    turns = [0, 1, 1 << 126, 1 << 127, 3 << 126, (1 << 128) - 1, (1 << 125) + 12345, (1 << 124) - 1]
    turns += [random.getrandbits(128) for _ in range(40)]
    print("const SINE_AND_COSINE: [(u128, i128, i128); %d] = [" % len(turns))
    for t in turns:
        angle = 2 * pi * mpf(t) / (mpf(2) ** 128)
        print("    (0x%032x, %d, %d)," % (t, fixed(sin(angle)), fixed(cos(angle))))
    print("];")

    normals = [
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, -1.0],
        [1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0],
        [0.6, 0.8, 0.0],
        [0.2672612419124244, -0.5345224838248488, 0.8017837257372732],
        [-0.9, 0.1, 0.42],  # not a unit
        [1e-3, -1.0, 3e-3],
    ]
    print("const PLANES: [([f64; 3], [[i128; 3]; 2]); %d] = [" % len(normals))
    for normal in normals:
        first, second = basis(normal)
        print("    (%r, [[%s], [%s]])," % (normal, ", ".join(str(fixed(c)) for c in first), ", ".join(str(fixed(c)) for c in second)))
    print("];")

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

    radians = [0.0, 0.7, -1.25, 3.0, 5.5, 1e-5, -1e-5, 12345.678, -98765.4321, 2.0 ** 60, -(2.0 ** 63), 1e-300, 6.283185307179586, 0.5]
    print("const TURNS_OF_RADIANS: [(f64, u128); %d] = [" % len(radians))
    for r in radians:
        t = mpf(r) / (2 * pi)
        t -= floor(t)
        name = "TAU" if r == math.tau else repr(r)  # (a Rust constant: clippy refuses the approximation)
        print("    (%s, 0x%032x)," % (name, int(floor(t * (mpf(2) ** 128)))))
    print("];")

    # server_world.rs: a body 5e17 voxels round a ship at brick (1000, -2000, 3000), offset (3.5, 7.25, 30.0).
    parent = (1000 * 32 + mpf("3.5"), -2000 * 32 + mpf("7.25"), 3000 * 32 + mpf("30.0"))
    bricks, rest = offset([0.6, 0.8, 0.0], 5.0e17, 9.0e8, 1.0, 123_456_789, 20, shift=parent)
    print("// a ship round a ship: bricks", bricks, "offset", [repr(float(x)) for x in rest])


if __name__ == "__main__":
    if sys.argv[1:] == ["constants"]:
        constants()
    elif sys.argv[1:] == ["vectors"]:
        vectors()
    else:
        sys.exit(__doc__)
