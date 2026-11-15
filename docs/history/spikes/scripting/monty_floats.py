"""Float, hash and clock probes for Monty 1.6.0, run in separate processes to compare."""
import sys
from pydantic_monty import Monty

CASES = [
    "import math\nrepr(math.sin(1.0))",
    "import math\nrepr(math.sin(1e22))",
    "import math\nrepr(math.exp(1.0))",
    "import math\nrepr(math.pow(2.5, 3.7))",
    "repr(2.5 ** 3.7)",
    "import math\nrepr(math.cos(0.5))",
    "import math\nrepr(math.atan2(1.0, 2.0))",
    "import math\nrepr(math.log(10.0))",
    "import math\nrepr(math.sqrt(2.0))",
    "import math\nrepr(math.sin(1e300))",
    "import math\nrepr(math.exp(700.5))",
    "import struct\nstruct.pack('<d', 1.1).hex()",
    "hash('abc')",
    "hash((1, 2))",
    "[hash(s) for s in ['a', 'b']]",
]
with Monty(request_timeout=20.0) as pool:
    for code in CASES:
        with pool.checkout(limits={"max_feed_duration_secs": 2.0, "max_memory": 100_000_000}) as session:
            try:
                value = session.feed_run(code)
            except Exception as error:
                value = f"{type(error).__name__}: {str(error).strip().splitlines()[-1][:100]}"
        print(f"{code.splitlines()[-1]:<45} {value}")
    # a frozen clock and a seeded random through the OS policy
    for code in ("import time\ntime.time()", "import random\n[random.random(), random.random()]"):
        with pool.checkout(
            limits={"max_feed_duration_secs": 2.0},
            os_policy={"datetime": __import__("datetime").datetime(2026, 1, 1), "random_start": {"seed": 7}},
        ) as session:
            try:
                value = session.feed_run(code)
            except Exception as error:
                value = f"{type(error).__name__}: {str(error).strip().splitlines()[-1][:100]}"
        print(f"{code.splitlines()[-1]:<45} {value}   [frozen clock, seed 7]")
