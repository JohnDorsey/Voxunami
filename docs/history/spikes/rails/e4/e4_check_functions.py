"""Checks the results that `e4_wide dump` prints (dashu-float, which the other two libraries equalled bit for bit) against mpmath, an implementation by other people and other algorithms.

    python e4_check_functions.py dump.txt

For each line: the function, the width in bits, the exact input and the result as hex mantissa * 2^exponent. mpmath computes the function of the exact input at the width plus 200 bits and
it is rounded to the width (to the nearest, ties to even); the result must be that number. THROWAWAY (E4 of `docs/orbits-and-thrust-plan.md`).
"""
import sys

from mpmath import acosh, atan, cos, exp, ln, mp, mpf, sin, sinh, sqrt

FUNCTIONS = {"sqrt": sqrt, "sin": sin, "cos": cos, "atan": atan, "sinh": sinh, "acosh": acosh, "exp": exp, "ln": ln}


def parse(text):
    mantissa, exponent = text.split("*2^")
    negative = mantissa.startswith("-")
    value = int(mantissa.lstrip("-"), 16)
    return -value if negative else value, int(exponent)


def to_mpf(pair):
    mantissa, exponent = pair
    return mpf(mantissa) * mpf(2) ** exponent


def round_to(value, bits):
    """`value` (an mpf with more bits) rounded to the nearest number of `bits` bits, ties to even, as the pair (mantissa, exponent)."""
    if value == 0:
        return 0, 0
    sign, mantissa, exponent, size = value._mpf_
    mantissa = mantissa * (-1 if sign else 1)
    # value = mantissa * 2^exponent exactly (mpf keeps what it was computed at)
    drop = max(0, size - bits)
    if drop == 0:
        return mantissa, exponent
    magnitude = abs(mantissa)
    quotient, remainder = divmod(magnitude, 1 << drop)
    half = 1 << (drop - 1)
    if remainder > half or (remainder == half and quotient & 1):
        quotient += 1
    return (-quotient if mantissa < 0 else quotient), exponent + drop


def normal(pair):
    mantissa, exponent = pair
    if mantissa == 0:
        return 0, 0
    while mantissa % 2 == 0:
        mantissa //= 2
        exponent += 1
    return mantissa, exponent


def main(path):
    total = 0
    wrong = 0
    for line in open(path):
        if not line.startswith("DUMP"):
            continue
        _, name, bits, source, result = line.split()
        bits = int(bits)
        mp.prec = bits + 200
        reference = FUNCTIONS[name](to_mpf(parse(source)))
        expected = normal(round_to(reference, bits))
        got = normal(parse(result))
        total += 1
        if expected != got:
            wrong += 1
            print(f"DIFFERENT {name} at {bits} bits")
    print(f"{total} results checked against mpmath, {wrong} not the correctly rounded value")


if __name__ == "__main__":
    main(sys.argv[1])
