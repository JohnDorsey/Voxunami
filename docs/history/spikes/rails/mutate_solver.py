"""Throwaway (kept to reproduce the numbers in docs/orbits-and-thrust-plan.md, A2). Mutation experiment on exact_kepler.rs and wide.rs: each mutation must make `cargo test -p world_storage --lib` (the exact_kepler and wide tests) fail.
Each file is restored from memory after each run, in a `finally`. Run once, as one tracked background command, and let it finish; with arguments, only the mutations whose names contain one of them. Needs nothing but cargo."""
import os
import subprocess
import sys
import time

from pathlib import Path

REPO = str(Path(__file__).resolve().parents[4])
SRC = f"{REPO}/engine/world_storage/src"
KEPLER = f"{SRC}/exact_kepler.rs"
WIDE = f"{SRC}/wide.rs"

MUTATIONS = [
    (KEPLER, "steps 3 -> 1", "const STEPS: usize = 3;", "const STEPS: usize = 1;"),
    (KEPLER, "steps 3 -> 0", "const STEPS: usize = 3;", "const STEPS: usize = 0;"),
    (KEPLER, "no reflection", "let reflected = mean_anomaly > HALF_TURN;", "let reflected = false;"),
    (KEPLER, "slope sign", "let slope = (ONE - multiplied(self.eccentricity, cosine, 126)) as u128;", "let slope = (ONE + multiplied(self.eccentricity, cosine, 126)) as u128;"),
    (KEPLER, "step direction", "eccentric = if remainder > 0 {", "eccentric = if remainder < 0 {"),
    (KEPLER, "x = cos + e", "x: cosine - self.eccentricity,", "x: cosine + self.eccentricity,"),
    (KEPLER, "y uses e not the minor axis", "y: multiplied(self.minor, sine, 126),", "y: multiplied(self.eccentricity, sine, 126),"),
    (KEPLER, "distance sign", "distance: ONE - multiplied(self.eccentricity, cosine, 126),", "distance: ONE + multiplied(self.eccentricity, cosine, 126),"),
    (KEPLER, "always the plain first guess", "let mut guess = if e < 0.3 {", "let mut guess = if true {"),
    (KEPLER, "no Newton steps in f64", "for _ in 0..8 {\n            let (value, slope)", "for _ in 0..0 {\n            let (value, slope)"),
    (KEPLER, "no special case at a half turn", " || mean_anomaly == HALF_TURN {", " {"),
    (KEPLER, "e over 2 pi without the 2 pi", "eccentricity_in_turns: multiplied(eccentricity, ONE_OVER_TAU, 126),", "eccentricity_in_turns: eccentricity,"),
    (KEPLER, "one over 2 pi, last digit", "0x0a2f9836e4e441529fc2757d1f534ddc", "0x0a2f9836e4e441529fc2757d1f534ddd"),
    (KEPLER, "minor axis one too big", "minor: product.square_root() as Fixed,", "minor: (product.square_root() + 1) as Fixed,"),
    (KEPLER, "the original overflow (order of the sums)", "let remainder = (eccentric as i128 - mean as i128).saturating_sub(sine_part);", "let remainder = eccentric as i128 - sine_part - mean as i128;"),
    (KEPLER, "series for E - sin E one term short", "for index in 0..12 {", "for index in 0..2 {"),
    (KEPLER, "stable slope without e", "(value, ratio + 2.0 * e * half * half)", "(value, ratio + 2.0 * half * half)"),
    (WIDE, "division: > for >=", "            if remainder >= divisor {", "            if remainder > divisor {"),
    (WIDE, "division: saturation edge", "if self.high >= divisor {", "if self.high > divisor {"),
    (WIDE, "root: compare", "if remainder.is_at_least(candidate) {", "if remainder.is_at_least(candidate.plus(Self::ONE)) {"),
    (WIDE, "is_at_least: > for >=", "self.high > other.high || (self.high == other.high && self.low >= other.low)", "self.high > other.high || (self.high == other.high && self.low > other.low)"),
    (WIDE, "minus", "self.plus(other.negated())", "self.plus(other)"),
]

survivors = []
only = sys.argv[1:]
for path, name, old, new in MUTATIONS:
    if only and not any(word in name for word in only):
        continue
    with open(path, encoding="utf-8", newline="") as handle:
        original = handle.read()
    if original.count(old) != 1:
        print(f"SKIPPED  {name}: text found {original.count(old)} times")
        survivors.append(name + " (not applied)")
        continue
    started = time.time()
    try:
        with open(path, "w", encoding="utf-8", newline="") as handle:
            handle.write(original.replace(old, new))
        environment = dict(os.environ, PROPTEST_DISABLE_FAILURE_PERSISTENCE="1")
        process = subprocess.Popen(
            ["cargo", "test", "-p", "world_storage", "--lib", "--", "exact_kepler", "wide::"],
            cwd=REPO, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=environment,
        )
        try:
            stdout, stderr = process.communicate(timeout=150)
            result = subprocess.CompletedProcess(process.args, process.returncode, stdout, stderr)
        except subprocess.TimeoutExpired:
            # a mutant that loops forever is caught by a test that never finishes: kill the whole tree (cargo and the test binary under it)
            subprocess.run(["taskkill", "/F", "/T", "/PID", str(process.pid)], capture_output=True)
            process.communicate()
            print(f"CAUGHT (hang) {name}  [{time.time() - started:.0f} s]", flush=True)
            continue
        compiled = "could not compile" not in result.stderr
        if not compiled:
            outcome = "DID NOT COMPILE"
            survivors.append(name + " (did not compile)")
        elif result.returncode != 0:
            failed = [line.split()[1] for line in result.stdout.splitlines() if line.startswith("test ") and line.endswith("FAILED")]
            outcome = "CAUGHT   by " + ", ".join(sorted(failed))[:150]
        else:
            outcome = "SURVIVED"
            survivors.append(name)
    finally:
        with open(path, "w", encoding="utf-8", newline="") as handle:
            handle.write(original)
    print(f"{outcome:<12} {name}  [{time.time() - started:.0f} s]", flush=True)
print("survivors:", survivors or "none")
sys.exit(1 if survivors else 0)
