"""Throwaway: runs the self-check of `engine/world_storage/tools/exact_ellipse_reference.py` against deliberately wrong copies of it; every mutation must make it fail.

    python mutate_ellipse_reference.py        # needs mpmath; prints CAUGHT or SURVIVED for each, exits 1 if any survived

The copies are made in a temporary directory; the real file is not touched.
"""
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[4] / "engine" / "world_storage" / "tools"
MUTATIONS = [
    ("minor axis uses sqrt(1 + e^2)", "root = sqrt(1 - e * e)", "root = sqrt(1 + e * e)"),
    ("velocity sign flipped", "vx = -factor * sin(eccentric)", "vx = factor * sin(eccentric)"),
    ("x uses cos E + e", "x = a * (cos(eccentric) - e)", "x = a * (cos(eccentric) + e)"),
    ("velocity forgets 1 / (1 - e cos E)", "factor = (TWO_PI / mpf(period)) * a / (1 - e * cos(eccentric))", "factor = (TWO_PI / mpf(period)) * a"),
    ("Kepler solved for E + e sin E", "if middle - e * sin(middle) > mean_anomaly:", "if middle + e * sin(middle) > mean_anomaly:"),
    ("mean anomaly ignores the phase", "total = turns + mpf(phase) / TWO_PI", "total = turns"),
    ("periapsis rotates the wrong way", "q = [-s * first[i] + c * second[i] for i in range(3)]", "q = [s * first[i] + c * second[i] for i in range(3)]"),
    ("negative period not reversed (n always positive)", "factor = (TWO_PI / mpf(period)) * a", "factor = (TWO_PI / abs(mpf(period))) * a"),
]

source = (TOOLS / "exact_ellipse_reference.py").read_text(encoding="utf-8")
survivors = []
for name, old, new in MUTATIONS:
    if source.count(old) != 1:
        sys.exit(f"mutation {name!r}: the text it changes is found {source.count(old)} times, not once")
    with tempfile.TemporaryDirectory() as directory:
        shutil.copy(TOOLS / "exact_circle_reference.py", directory)
        Path(directory, "exact_ellipse_reference.py").write_text(source.replace(old, new), encoding="utf-8")
        result = subprocess.run([sys.executable, "exact_ellipse_reference.py", "selfcheck"], cwd=directory, capture_output=True, text=True)
    caught = result.returncode != 0
    print(("CAUGHT   " if caught else "SURVIVED ") + name)
    if not caught:
        survivors.append(name)
print("survivors:", survivors or "none")
sys.exit(1 if survivors else 0)
