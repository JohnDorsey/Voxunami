"""Throwaway (kept to reproduce the numbers in docs/orbits-and-thrust-plan.md, A3). Mutation experiment on the ellipse keys of the packs (step A3): each mutation must make `cargo test -p data_packs -p game_shell` fail. Files are restored in a `finally`."""
import os
import subprocess
import sys
import time
from pathlib import Path

REPO = str(Path(__file__).resolve().parents[4])
BODY = f"{REPO}/engine/data_packs/src/body.rs"
SCHEMA = f"{REPO}/engine/data_packs/src/schema.rs"
PACKAGE = f"{REPO}/engine/game_shell/src/package.rs"

MUTATIONS = [
    (BODY, "eccentricity of 1 allowed", "if (0.0..1.0).contains(&number) {", "if (0.0..=1.0).contains(&number) {"),
    (BODY, "negative eccentricity allowed", "if (0.0..1.0).contains(&number) {", "if (-1.0..1.0).contains(&number) {"),
    (BODY, "the ratio is the eccentricity", "return Some(1.0 - number);", "return Some(number);"),
    (BODY, "ratio below the smallest allowed", "if (SMALLEST_ORBIT_RATIO..=1.0).contains(&number) {", "if (0.0..=1.0).contains(&number) {"),
    (BODY, "ratio above one allowed", "if (SMALLEST_ORBIT_RATIO..=1.0).contains(&number) {", "if (SMALLEST_ORBIT_RATIO..=2.0).contains(&number) {"),
    (BODY, "both keys allowed", """            walk.fail(
                path,
                "an orbit has an \\"eccentricity\\" or a \\"ratio\\" (1 - eccentricity), not both",
            );
            None""", "            Some(1.0)"),
    (BODY, "the periapsis is ignored", "Some(periapsis) => walk.number(periapsis, &path.key(\"periapsis\")),", "Some(_) => Some(0.0),"),
    (BODY, "the smallest ratio is 2^-63", "pub const SMALLEST_ORBIT_RATIO: f64 = 1.0 / ((1u128 << 64) as f64);", "pub const SMALLEST_ORBIT_RATIO: f64 = 1.0 / ((1u128 << 63) as f64);"),
    (SCHEMA, "the schema allows both keys", '"not": { "required": ["eccentricity", "ratio"] },', ""),
    (SCHEMA, "the schema allows an eccentricity of 1", '"exclusiveMaximum": 1, "default": 0', '"maximum": 1, "default": 0'),
    (SCHEMA, "the schema has no smallest ratio", '"minimum": SMALLEST_ORBIT_RATIO, "maximum": 1, "default": 1', '"minimum": 0, "maximum": 1, "default": 1'),
    (PACKAGE, "the shell drops the ratio", "                orbit.ratio,\n                radians(orbit.periapsis_degrees),", "                1.0,\n                radians(orbit.periapsis_degrees),"),
    (PACKAGE, "the shell drops the periapsis", "                radians(orbit.periapsis_degrees),\n                orbit.period,", "                0.0,\n                orbit.period,"),
    (PACKAGE, "the shell forgets to turn the periapsis into radians", "                radians(orbit.periapsis_degrees),\n                orbit.period,", "                orbit.periapsis_degrees,\n                orbit.period,"),
]

survivors = []
only = sys.argv[1:]
for path, name, old, new in MUTATIONS:
    if only and not any(word in name for word in only):
        continue
    with open(path, encoding="utf-8", newline="") as handle:
        original = handle.read()
    if original.count(old) != 1:
        print(f"SKIPPED  {name}: text found {original.count(old)} times", flush=True)
        survivors.append(name + " (not applied)")
        continue
    started = time.time()
    try:
        with open(path, "w", encoding="utf-8", newline="") as handle:
            handle.write(original.replace(old, new))
        environment = dict(os.environ, PROPTEST_DISABLE_FAILURE_PERSISTENCE="1")
        process = subprocess.Popen(
            ["cargo", "test", "-p", "data_packs", "-p", "game_shell"],
            cwd=REPO, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=environment,
        )
        try:
            stdout, stderr = process.communicate(timeout=300)
        except subprocess.TimeoutExpired:
            subprocess.run(["taskkill", "/F", "/T", "/PID", str(process.pid)], capture_output=True)
            process.communicate()
            print(f"CAUGHT (hang) {name}  [{time.time() - started:.0f} s]", flush=True)
            continue
        if "could not compile" in stderr:
            outcome = "DID NOT COMPILE"
            survivors.append(name + " (did not compile)")
        elif process.returncode != 0:
            failed = sorted({line.split()[1] for line in stdout.splitlines() if line.startswith("test ") and line.endswith("FAILED")})
            outcome = "CAUGHT   by " + ", ".join(failed)[:130]
        else:
            outcome = "SURVIVED"
            survivors.append(name)
    finally:
        with open(path, "w", encoding="utf-8", newline="") as handle:
            handle.write(original)
    print(f"{outcome:<12} {name}  [{time.time() - started:.0f} s]", flush=True)
print("survivors:", survivors or "none")
sys.exit(1 if survivors else 0)
