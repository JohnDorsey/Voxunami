"""Makes `functions.txt`, the correctly rounded results the tests of `wide_float` are compared with.

    python generate.py > functions.txt

The results are worked out by other people's code and other algorithms than the library the crate uses: `+ - * /` and the square root in exact arithmetic (Python integers and fractions),
and the functions by mpmath at 256 bits more than the width asked for (and as many more as an input close to 1 loses by cancellation), then rounded to the width (to the nearest, ties to even).
Every such result is worked out twice, at two working precisions 128 bits apart, and the script stops if the two rounded results differ: a reference that depends on its working precision is not
one (an acosh close to 1 at 1,024 bits was wrong at 256 spare bits, found by the first run of the tests, whose library had it right). Needs `mpmath`; the file it makes is committed, so the
tests need only Rust.

Each line: the function, the width in bits, the arguments and the result, each number as `[-]0x<hex digits of the odd mantissa>p<decimal exponent>` (the form `WideFloat` prints).
The inputs are the same on every run (a seeded generator), so a regeneration changes nothing unless this file does.
"""
import random
from fractions import Fraction
from math import isqrt

from mpmath import acosh, atan, atan2, cos, cosh, exp, ln, mp, mpf, pi, sin, sinh, sqrt

WIDTHS = {128: 12, 129: 12, 192: 12, 256: 12, 512: 8, 1024: 3, 4096: 2}
SPARE = 256


def text(mantissa, exponent):
    """The pair as `[-]0x<hex>p<exp>` with an odd mantissa (zero is `0x0p0`)."""
    if mantissa == 0:
        return "0x0p0"
    sign = "-" if mantissa < 0 else ""
    magnitude = abs(mantissa)
    while magnitude % 2 == 0:
        magnitude //= 2
        exponent += 1
    return f"{sign}0x{magnitude:x}p{exponent}"


def round_fraction(value, bits):
    """`value` (a Fraction) rounded to `bits` bits, to the nearest, ties to even, as (mantissa, exponent)."""
    if value == 0:
        return 0, 0
    sign = -1 if value < 0 else 1
    value = abs(value)
    exponent = value.numerator.bit_length() - value.denominator.bit_length() - bits
    while value / Fraction(2) ** exponent >= 2**bits:
        exponent += 1
    while value / Fraction(2) ** exponent < 2 ** (bits - 1):
        exponent -= 1
    scaled = value / Fraction(2) ** exponent
    quotient = scaled.numerator // scaled.denominator
    rest = scaled - quotient
    if rest > Fraction(1, 2) or (rest == Fraction(1, 2) and quotient % 2 == 1):
        quotient += 1
    return sign * quotient, exponent


def round_mpf(value, bits):
    """An mpf (computed with more bits) rounded to `bits` bits, as (mantissa, exponent)."""
    sign, mantissa, exponent, _ = value._mpf_
    return round_fraction(Fraction(mantissa * (-1 if sign else 1)) * Fraction(2) ** exponent, bits)


def fraction(pair):
    mantissa, exponent = pair
    return Fraction(mantissa) * Fraction(2) ** exponent


def exact(pair):
    return mpf(pair)


def evaluate(function, arguments, bits, cancelled=0):
    """`function` of the exact `arguments` rounded to `bits` bits, worked out at two precisions that must give the same result. `cancelled` is how many leading bits the function loses."""
    results = []
    for extra in (0, 128):
        mp.prec = bits + SPARE + cancelled + extra
        results.append(round_mpf(function(*[exact(argument) for argument in arguments]), bits))
    if results[0] != results[1]:
        raise ValueError(f"the reference depends on its working precision: {function.__name__} at {bits} bits")
    return results[0]


def number(rng, bits, low, high, signed):
    """A number of exactly `bits` bits with a leading bit in [2^low, 2^high)."""
    mantissa = rng.getrandbits(bits - 1) | (1 << (bits - 1))
    if rng.random() < 0.15:
        # A number with a short mantissa, as a whole number of seconds or a double is.
        mantissa = (rng.getrandbits(rng.randint(1, 53)) | 1) << 0
        mantissa <<= bits - mantissa.bit_length()
    exponent = rng.randint(low, high - 1) - (bits - 1)
    if signed and rng.random() < 0.5:
        mantissa = -mantissa
    return mantissa, exponent


def main():
    rng = random.Random(20261102)
    lines = []
    for bits, count in WIDTHS.items():
        lines.append(f"pi {bits} {text(*evaluate(lambda: +pi, [], bits))}")

        for name in ("add", "sub", "mul", "div"):
            for _ in range(count):
                a = number(rng, bits, -60, 60, True)
                b = number(rng, bits, -60, 60, True)
                if name in ("add", "sub") and rng.random() < 0.4:
                    # Nearly the same number: the sum or the difference loses many leading bits.
                    b = (a[0] + rng.randint(-3, 3), a[1])
                if name == "div" and b[0] == 0:
                    continue
                x, y = fraction(a), fraction(b)
                value = {"add": x + y, "sub": x - y, "mul": x * y, "div": x / y}[name]
                lines.append(f"{name} {bits} {text(*a)} {text(*b)} {text(*round_fraction(value, bits))}")

        for _ in range(count):
            a = number(rng, bits, -40, 80, False)
            lines.append(f"sqrt {bits} {text(*a)} {text(*round_sqrt(fraction(a), bits))}")

        domains = {
            "sin": (-100, 100, True),
            "cos": (-100, 100, True),
            "atan": (-30, 30, True),
            "sinh": (-60, 6, True),
            "cosh": (-60, 6, True),
            "exp": (-30, 8, True),
        }
        functions = {"sin": sin, "cos": cos, "atan": atan, "sinh": sinh, "cosh": cosh, "exp": exp}
        for name, (low, high, signed) in domains.items():
            for _ in range(count):
                a = number(rng, bits, low, high, signed)
                lines.append(f"{name} {bits} {text(*a)} {text(*evaluate(functions[name], [a], bits))}")

        for _ in range(count):
            a = number(rng, bits, -60, 100, False)
            lines.append(f"ln {bits} {text(*a)} {text(*evaluate(ln, [a], bits))}")
        for _ in range(count // 2):
            # Near one, where the logarithm loses its leading bits.
            a = (2 ** (bits - 1) + rng.randint(1, 1000), -(bits - 1))
            lines.append(f"ln {bits} {text(*a)} {text(*evaluate(ln, [a], bits, cancelled=bits))}")
        for _ in range(count):
            a = number(rng, bits, 0, 100, False)
            lines.append(f"acosh {bits} {text(*a)} {text(*evaluate(acosh, [a], bits))}")
        for _ in range(count // 2):
            a = (2 ** (bits - 1) + rng.randint(1, 1000), -(bits - 1))
            lines.append(f"acosh {bits} {text(*a)} {text(*evaluate(acosh, [a], bits, cancelled=bits))}")
        for _ in range(count):
            y = number(rng, bits, -30, 30, True)
            x = number(rng, bits, -30, 30, True)
            lines.append(f"atan2 {bits} {text(*y)} {text(*x)} {text(*evaluate(atan2, [y, x], bits))}")
    print("\n".join(lines))


def round_sqrt(value, bits):
    """The square root of a positive Fraction rounded to `bits` bits, to the nearest, ties to even (exact in integers)."""
    # sqrt(value) = sqrt(n / d); take a power of 4, k, so that the root of value * 4^k is at least bits + 2 bits wide.
    k = bits + 4 - (value.numerator.bit_length() - value.denominator.bit_length()) // 2
    scaled = value * Fraction(4) ** k
    integer = scaled.numerator // scaled.denominator
    root = isqrt(integer)
    exactly = root * root == scaled
    # root is the floor of the root of the scaled value, so the sticky bit is "not exact"; the root has more than bits + 2 bits.
    extra = root.bit_length() - bits
    if extra < 2:
        raise ValueError("not enough bits in the root")
    quotient, remainder = divmod(root, 1 << extra)
    half = 1 << (extra - 1)
    above = remainder > half or (remainder == half and not exactly)
    tie = remainder == half and exactly
    if above or (tie and quotient % 2 == 1):
        quotient += 1
    return quotient, extra - k


if __name__ == "__main__":
    main()
