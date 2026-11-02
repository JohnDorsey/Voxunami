"""Throwaway (kept to reproduce the numbers in docs/orbits-and-thrust-plan.md, A3). Mutation experiment on the ellipse code of rail.rs (step A3): each mutation must make `cargo test -p world_storage` (the rail tests) fail. Each file is restored in a `finally`.
Run once, as one tracked background command; with arguments, only the mutations whose names contain one of them."""
import os
import subprocess
import sys
import time
from pathlib import Path

REPO = str(Path(__file__).resolve().parents[4])
RAIL = f"{REPO}/engine/world_storage/src/rail.rs"

MUTATIONS = [
    ("never the ellipse path", "if self.ratio != 1.0 {\n            return self.on_an_ellipse", "if self.ratio < 0.0 {\n            return self.on_an_ellipse"),
    ("circle ignores the periapsis", "gone.wrapping_add(exact_circle::turns_of_radians(self.periapsis))", "gone"),
    ("rotation: x sign", "exact_circle::multiplied(x, cosine, 126) - exact_circle::multiplied(y, sine, 126)", "exact_circle::multiplied(x, cosine, 126) + exact_circle::multiplied(y, sine, 126)"),
    ("rotation: y sign", "exact_circle::multiplied(x, sine, 126) + exact_circle::multiplied(y, cosine, 126)", "exact_circle::multiplied(x, sine, 126) - exact_circle::multiplied(y, cosine, 126)"),
    ("x and y swapped into the plane", "exact_circle::circle_point(self.radius, (y, x), first, second)", "exact_circle::circle_point(self.radius, (x, y), first, second)"),
    ("speed without the distance", "self.radius * (TAU / self.period) / exact_circle::float_of(anomaly.distance)", "self.radius * (TAU / self.period)"),
    ("speed: direction of the first part", "-exact_circle::float_of(anomaly.sine) * scale,", "exact_circle::float_of(anomaly.sine) * scale,"),
    ("speed: the second part is the cosine", "exact_circle::float_of(anomaly.velocity_y) * scale,", "exact_circle::float_of(anomaly.cosine) * scale,"),
    ("speed: rotation sign", """toward * cosine - along * sine,
                    toward * sine + along * cosine,""", """toward * cosine + along * sine,
                    toward * sine + along * cosine,"""),
    ("speed: rotation, second part", """toward * cosine - along * sine,
                    toward * sine + along * cosine,""", """toward * cosine - along * sine,
                    toward * sine - along * cosine,"""),
    ("speed: the period's sign lost", "self.radius * (TAU / self.period) / exact_circle::float_of(anomaly.distance)", "self.radius * (TAU / self.period.abs()) / exact_circle::float_of(anomaly.distance)"),
    ("mean anomaly a little out", "let anomaly = Ellipse::new(self.ratio).anomaly(mean_anomaly);", "let anomaly = Ellipse::new(self.ratio).anomaly(mean_anomaly ^ (1 << 100));"),
    ("no check of the shape", "(SMALLEST_RATIO..=1.0).contains(&ratio) && periapsis.abs() <= LARGEST_PHASE", "true"),
    ("check of the ratio without the smallest", "(SMALLEST_RATIO..=1.0).contains(&ratio) && periapsis.abs() <= LARGEST_PHASE", "(0.0..=1.0).contains(&ratio) && periapsis.abs() <= LARGEST_PHASE"),
    ("the file forgets a periapsis on a circle", "let is_ellipse = |orbit: &Orbit| orbit.ratio != 1.0 || orbit.periapsis != 0.0;", "let is_ellipse = |orbit: &Orbit| orbit.ratio != 1.0;"),
    ("a circle may be written as an ellipse", "if shape == (1.0, 0.0) {", "if false {"),
    ("an ellipse flag with no orbit", "if flags & HAS_ELLIPSE != 0 && flags & HAS_ORBIT == 0 {", "if false {"),
    ("ratio and periapsis written the other way", "            writer.write_u64(orbit.ratio.to_bits());\n            writer.write_u64(orbit.periapsis.to_bits());", "            writer.write_u64(orbit.periapsis.to_bits());\n            writer.write_u64(orbit.ratio.to_bits());"),
    ("new makes an ellipse", "Self::elliptical(around, normal, radius, 1.0, 0.0, period, phase)", "Self::elliptical(around, normal, radius, 0.5, 0.0, period, phase)"),
    ("the flag byte for an ellipse", "const HAS_ELLIPSE: u8 = 4;", "const HAS_ELLIPSE: u8 = 8;"),
]

survivors = []
only = sys.argv[1:]
for name, old, new in MUTATIONS:
    if only and not any(word in name for word in only):
        continue
    with open(RAIL, encoding="utf-8", newline="") as handle:
        original = handle.read()
    if original.count(old) != 1:
        print(f"SKIPPED  {name}: text found {original.count(old)} times", flush=True)
        survivors.append(name + " (not applied)")
        continue
    started = time.time()
    try:
        with open(RAIL, "w", encoding="utf-8", newline="") as handle:
            handle.write(original.replace(old, new))
        environment = dict(os.environ, PROPTEST_DISABLE_FAILURE_PERSISTENCE="1")
        process = subprocess.Popen(
            ["cargo", "test", "-p", "world_storage", "--test", "rail", "--test", "rail_ellipse", "--test", "rail_circle_bits", "--test", "ship_rail"],
            cwd=REPO, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=environment,
        )
        try:
            stdout, stderr = process.communicate(timeout=240)
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
        with open(RAIL, "w", encoding="utf-8", newline="") as handle:
            handle.write(original)
    print(f"{outcome:<12} {name}  [{time.time() - started:.0f} s]", flush=True)
print("survivors:", survivors or "none")
sys.exit(1 if survivors else 0)
