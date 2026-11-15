"""Compare the four float-search outputs; report per function how many results differ and by how many ULP."""
import math
import sys
from pathlib import Path

folder = Path(sys.argv[1])
runs = {
    "monty-win": folder / "fs_monty_windows.txt",
    "monty-lin": folder / "fs_monty_linux.txt",
    "cpy-win": folder / "fs_cpython_windows.txt",
    "cpy-lin": folder / "fs_cpython_linux.txt",
}


def load(path):
    lines = path.read_text().splitlines()
    names = lines[0].split()[1:]
    rows = [line.split()[1:] for line in lines[1:]]
    return names, rows


def ulp_distance(left_hex, right_hex):
    left, right = float.fromhex(left_hex), float.fromhex(right_hex)
    if math.isnan(left) or math.isnan(right):
        return 0 if (math.isnan(left) and math.isnan(right)) else 10**9
    if math.isinf(left) or math.isinf(right):
        return 0 if left == right else 10**9
    spacing = math.ulp(max(abs(left), abs(right), 5e-324))
    return int(round(abs(left - right) / spacing))


data = {name: load(path) for name, path in runs.items()}
names = data["monty-win"][0]
for key, (other_names, _) in data.items():
    assert other_names == names, key
pairs = [("monty-win", "monty-lin"), ("cpy-win", "cpy-lin"), ("monty-win", "cpy-win"), ("monty-lin", "cpy-lin")]
for left_key, right_key in pairs:
    left_rows, right_rows = data[left_key][1], data[right_key][1]
    print(f"\n== {left_key} vs {right_key}  ({len(left_rows)} rows)")
    total_differing = 0
    for column, name in enumerate(names):
        differing = 0
        worst = 0
        first = None
        for index, (left_row, right_row) in enumerate(zip(left_rows, right_rows)):
            if left_row[column] != right_row[column]:
                differing += 1
                distance = ulp_distance(left_row[column], right_row[column])
                worst = max(worst, distance)
                if first is None:
                    first = (index, left_row[column], right_row[column])
        total_differing += differing
        if differing:
            print(f"  {name:<6} differs in {differing:>5} of {len(left_rows)}  worst {worst} ulp  first row {first[0]}: {first[1]} vs {first[2]}")
    print(f"  total differing values: {total_differing}" + ("   (identical)" if total_differing == 0 else ""))
