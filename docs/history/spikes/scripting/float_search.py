"""Search for inputs where math functions differ between platforms.

Usage:  python float_search.py monty|cpython OUTPUT_FILE [ROWS]
The same program text runs in Monty (sandboxed, through its worker pool) or in plain CPython (exec).
Inputs come from an integer LCG, so every platform sees identical inputs; results are written as float.hex().
"""
import sys

ROWS = int(sys.argv[3]) if len(sys.argv) > 3 else 5000

PROGRAM = f"""
import math
names = ['sin', 'cos', 'tan', 'exp', 'log', 'pow', 'atan2', 'sinh', 'cosh', 'tanh', 'asin', 'acos',
         'atan', 'log10', 'log2', 'expm1', 'log1p', 'sqrt', 'cbrt', 'hypot', 'fmod']
present = [n for n in names if hasattr(math, n)]
state = 12345
modulus = 2 ** 64
out = []
for i in range({ROWS}):
    state = (state * 6364136223846793005 + 1442695040888963407) % modulus
    u1 = (state >> 11) / 9007199254740992.0
    state = (state * 6364136223846793005 + 1442695040888963407) % modulus
    u2 = (state >> 11) / 9007199254740992.0
    big = (u1 - 0.5) * 2e6
    small = (u1 - 0.5) * 1400.0
    positive = u1 * 100.0 + 1e-9
    other = (u2 - 0.5) * 40.0
    unit = u1 * 2.0 - 1.0
    row = []
    for name in present:
        if name == 'sin' or name == 'cos' or name == 'tan' or name == 'atan':
            row.append(getattr(math, name)(big))
        elif name == 'exp':
            row.append(math.exp(small))
        elif name == 'log' or name == 'log10' or name == 'log2' or name == 'log1p' or name == 'sqrt' or name == 'cbrt':
            row.append(getattr(math, name)(positive))
        elif name == 'pow':
            row.append(math.pow(positive, other))
        elif name == 'atan2' or name == 'hypot' or name == 'fmod':
            row.append(getattr(math, name)(big, other))
        elif name == 'asin' or name == 'acos':
            row.append(getattr(math, name)(unit))
        else:
            row.append(getattr(math, name)(other))
    out.append(row)
[present, out]
"""


def run_cpython():
    namespace = {}
    exec(compile(PROGRAM.rsplit("\n[present, out]", 1)[0], "program", "exec"), namespace)
    return namespace["present"], namespace["out"]


def run_monty():
    from pydantic_monty import Monty

    with Monty(request_timeout=120.0) as pool:
        with pool.checkout(limits={"max_feed_duration_secs": 60.0, "max_memory": 400_000_000}) as session:
            present, rows = session.feed_run(PROGRAM)
    return present, rows


def main():
    mode, output_path = sys.argv[1], sys.argv[2]
    present, rows = run_monty() if mode == "monty" else run_cpython()
    with open(output_path, "w", newline="\n") as handle:
        handle.write("functions " + " ".join(present) + "\n")
        for index, row in enumerate(rows):
            handle.write(f"{index} " + " ".join(float(value).hex() for value in row) + "\n")
    print(mode, sys.platform, "functions:", len(present), "rows:", len(rows), "->", output_path)


main()
