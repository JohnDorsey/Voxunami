"""Throwaway (kept to reproduce the numbers in docs/orbits-and-thrust-plan.md, A4). Mutation experiment on engine/world_storage/src/fit_orbit.rs: each mutation must make
`cargo test -p world_storage --test rail_fit` fail. The file is restored in a `finally`. Run once, as one tracked background command; with arguments, only the mutations
whose names contain one of them."""
import os
import subprocess
import sys
import time
from pathlib import Path

REPO = str(Path(__file__).resolve().parents[4])
FIT = f"{REPO}/engine/world_storage/src/fit_orbit.rs"

MUTATIONS = [
    ("angular momentum the other way", "let momentum = cross(place, velocity);", "let momentum = cross(velocity, place);"),
    ("eccentricity vector with the wrong sign", "toward[part] - place[part] / distance", "toward[part] + place[part] / distance"),
    ("everything under a thousandth is a circle", "let is_circle = eccentricity < CIRCLE;", "let is_circle = eccentricity < 1.0e-3;"),
    ("nothing is a circle", "let is_circle = eccentricity < CIRCLE;", "let is_circle = false;"),
    ("the ratio is always one minus e", "            s / (1.0 + eccentricity)\n", "            1.0 - eccentricity\n"),
    ("the half angle without the ratio", "libm::sqrt(2.0 - ratio) * libm::cos(half),", "libm::sqrt(2.0) * libm::cos(half),"),
    ("the mean anomaly loses the sign of E", "(periapsis, value.copysign(eccentric))", "(periapsis, value)"),
    ("the period with the axis squared", "TAU * libm::sqrt(axis * axis * axis / mu)", "TAU * libm::sqrt(axis * axis / mu)"),
    ("the turns gone added and not taken", ".wrapping_sub(gone)", ".wrapping_add(gone)"),
    ("escape allowed", "if inverse_axis <= NEAR_ESCAPE * 2.0 / distance {", "if inverse_axis < 0.0 {"),
    ("a mass of nothing allowed", "if !(mu > 0.0 && mu.is_finite())", "if !(mu >= 0.0 && mu.is_finite())"),
    ("the periapsis from the wrong parts", "dot(eccentricity_vector, second),\n                dot(eccentricity_vector, first),", "dot(eccentricity_vector, first),\n                dot(eccentricity_vector, second),"),
    ("the true anomaly from the wrong directions", "libm::atan2(dot(place, beyond), dot(place, toward_periapsis))", "libm::atan2(dot(place, toward_periapsis), dot(place, beyond))"),
    ("a circle's mean anomaly from the wrong direction", "libm::atan2(dot(place, second), dot(place, first))", "libm::atan2(dot(place, first), dot(place, second))"),
]

survivors = []
only = sys.argv[1:]
for name, old, new in MUTATIONS:
    if only and not any(word in name for word in only):
        continue
    with open(FIT, encoding="utf-8", newline="") as handle:
        original = handle.read()
    if original.count(old) != 1:
        print(f"SKIPPED  {name}: text found {original.count(old)} times", flush=True)
        survivors.append(name + " (not applied)")
        continue
    started = time.time()
    try:
        with open(FIT, "w", encoding="utf-8", newline="") as handle:
            handle.write(original.replace(old, new))
        environment = dict(os.environ, PROPTEST_DISABLE_FAILURE_PERSISTENCE="1")
        process = subprocess.Popen(
            ["cargo", "test", "-p", "world_storage", "--test", "rail_fit"],
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
        with open(FIT, "w", encoding="utf-8", newline="") as handle:
            handle.write(original)
    print(f"{outcome:<12} {name}  [{time.time() - started:.0f} s]", flush=True)
print("survivors:", survivors or "none")
sys.exit(1 if survivors else 0)
