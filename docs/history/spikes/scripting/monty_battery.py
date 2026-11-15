"""Throwaway test battery for Pydantic Monty 1.6.0 (Windows). Run: venv_monty\\Scripts\\python.exe -I monty_battery.py

Question set: which generator-style expressions work, is anything lazy, what does the sandbox refuse,
which limits fire, and what do floats and hash order look like. Each case gets a fresh session.
"""
import sys
import time

from pydantic_monty import Monty, MontyError


DEFAULT_LIMITS = {"max_feed_duration_secs": 3.0, "max_memory": 200_000_000}


def run_case(pool, label, code, limits="default"):
    # First run had no limits and `range(10**12)` inside a generator expression hung a worker for 95 s,
    # so every case now runs under limits unless a case states its own (None = deliberately none).
    if limits == "default":
        limits = DEFAULT_LIMITS
    started = time.perf_counter()
    try:
        with pool.checkout(limits=limits) as session:
            result = session.feed_run(code)
        outcome = f"OK    {result!r}"
    except MontyError as error:
        outcome = f"{type(error).__name__}: {str(error).strip().splitlines()[-1][:150] if str(error).strip() else ''}"
    except Exception as error:  # noqa: BLE001 - this is a probe, record everything
        outcome = f"!!{type(error).__name__}: {str(error).strip()[:150]}"
    elapsed_ms = (time.perf_counter() - started) * 1000
    print(f"[{elapsed_ms:8.1f} ms] {label:<44} {outcome}")


GENERATOR_CASES = [
    ("sum over genexpr", "sum(x * x for x in range(10))"),
    ("list comprehension with filter", "[x for x in range(10) if x % 3 == 0]"),
    ("dict comprehension", "{x: x * x for x in range(4)}"),
    ("set comprehension", "sorted({x % 3 for x in range(10)})"),
    ("nested three-axis cube, even sum, count", "len([(x, y, z) for x in range(16) for y in range(16) for z in range(16) if (x + y + z) % 2 == 0])"),
    ("genexpr is lazy (range of 10**12, next)", "g = (x for x in range(10**12)); next(g)"),
    ("genexpr type name", "type(x for x in range(3)).__name__"),
    ("generator function with yield", "def count():\n    yield 1\n    yield 2\nlist(count())"),
    ("yield from", "def a():\n    yield from range(3)\nlist(a())"),
    ("any over genexpr short-circuit", "any(x > 2 for x in range(10**12))"),
    ("min / max / sorted over genexpr", "[min(x for x in range(5)), max(x for x in range(5)), sorted((x for x in [3, 1, 2]), reverse=True)]"),
    ("map / filter / zip / enumerate", "[list(map(abs, [-1, 2])), list(filter(None, [0, 1, 2])), list(zip('ab', [1, 2])), list(enumerate('ab'))]"),
    ("itertools.islice over infinite count", "import itertools\nlist(itertools.islice(itertools.count(5), 3))"),
    ("itertools.product", "import itertools\nlen(list(itertools.product(range(4), repeat=3)))"),
    ("generator send()", "def g():\n    x = yield 1\n    yield x\nit = g(); next(it); it.send(7)"),
    ("star-unpack genexpr into tuple", "tuple((x, -x) for x in range(3))"),
    ("lambda + comprehension closure", "f = lambda n: [i * n for i in range(3)]\nf(2)"),
    ("walrus inside comprehension", "[y for x in range(5) if (y := x * 2) > 3]"),
    ("match statement", "match (1, 2):\n    case (a, b):\n        a + b"),
    ("class definition (instance method)", "class P:\n    def __init__(self, v):\n        self.v = v\n    def double(self):\n        return self.v * 2\nP(4).double()"),
    ("class inheritance", "class A:\n    def f(self):\n        return 1\nclass B(A):\n    pass\nB().f()"),
]

SANDBOX_CASES = [
    ("open a file", "open('C:/Windows/win.ini').read()[:20]"),
    ("import os", "import os\nos.listdir('.')"),
    ("import sys", "import sys\nsys.platform"),
    ("import subprocess", "import subprocess"),
    ("import socket", "import socket"),
    ("__import__('os')", "__import__('os')"),
    ("eval", "eval('1+1')"),
    ("exec", "exec('x=1')"),
    ("compile", "compile('1', 'f', 'eval')"),
    ("getattr dunder escape", "().__class__.__bases__[0].__subclasses__()"),
    ("globals()", "list(globals().keys())[:5]"),
    ("pathlib read", "from pathlib import Path\nPath('C:/Windows/win.ini').read_text()[:20]"),
    ("time.time (clock)", "import time\ntype(time.time()).__name__"),
    ("import random, unseeded", "import random\nrandom.random()"),
    ("environ", "import os\nos.environ.get('PATH')"),
]

LIMIT_CASES = [
    ("infinite loop, 0.5 s feed limit", "while True:\n    pass", {"max_feed_duration_secs": 0.5}),
    ("genexpr burn, 0.5 s feed limit", "sum(1 for _ in range(10**12))", {"max_feed_duration_secs": 0.5}),
    ("memory bomb list, 50 MB limit", "x = [0] * (10**8)", {"max_memory": 50_000_000}),
    ("memory bomb string, 50 MB limit", "x = 'a' * (10**8)", {"max_memory": 50_000_000}),
    ("memory bomb comprehension, 50 MB limit", "x = [i for i in range(10**7)]", {"max_memory": 50_000_000}),
    ("unbounded recursion", "def f(n):\n    return f(n + 1)\nf(0)", {"max_recursion_depth": 200}),
    ("deep recursion default 1000", "def f(n):\n    return f(n + 1)\nf(0)", None),
    ("exponent blow up", "2 ** (10**9)", {"max_memory": 50_000_000}),
    ("sum of a big range, no limit, time it", "sum(range(10**7))", None),
]

FLOAT_CASES = [
    ("math.sin(1.0) repr", "import math\nmath.sin(1.0).hex()"),
    ("math.sin(1e22) repr", "import math\nmath.sin(1e22).hex()"),
    ("math.exp(1.0) hex", "import math\nmath.exp(1.0).hex()"),
    ("math.pow(2.5, 3.7) hex", "import math\nmath.pow(2.5, 3.7).hex()"),
    ("2.5 ** 3.7 hex", "(2.5 ** 3.7).hex()"),
    ("math.cos(0.5) hex", "import math\nmath.cos(0.5).hex()"),
    ("math.atan2(1, 2) hex", "import math\nmath.atan2(1, 2).hex()"),
    ("math.log(10.0) hex", "import math\nmath.log(10.0).hex()"),
    ("math.sqrt(2.0) hex", "import math\nmath.sqrt(2.0).hex()"),
    ("set of strings order", "list({'alpha', 'beta', 'gamma', 'delta'})"),
    ("dict order after delete/insert", "d = {'a': 1, 'b': 2}\nd.pop('a')\nd['a'] = 3\nlist(d)"),
    ("hash('abc')", "hash('abc')"),
]


def main():
    print("python", sys.version.split()[0], "platform", sys.platform)
    started = time.perf_counter()
    with Monty(request_timeout=20.0) as pool:
        print(f"pool start: {(time.perf_counter() - started) * 1000:.1f} ms")
        for title, cases in (("GENERATOR AND SYNTAX", GENERATOR_CASES), ("SANDBOX", SANDBOX_CASES), ("FLOATS AND ORDER", FLOAT_CASES)):
            print(f"\n== {title}")
            for label, code in cases:
                run_case(pool, label, code)
        print("\n== LIMITS")
        for label, code, limits in LIMIT_CASES:
            run_case(pool, label, code, limits)


if __name__ == "__main__":
    main()
