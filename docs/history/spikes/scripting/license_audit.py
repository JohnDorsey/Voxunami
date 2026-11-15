"""Walk the dependency graph of each engine in the scratch crate and list licence expressions (from cargo metadata)."""
import json
import subprocess
import sys
from collections import defaultdict

ROOTS = ["rune", "rhai", "koto", "starlark", "rquickjs"]
metadata = json.loads(
    subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--manifest-path", "E:/s/vx/scripting-spike/Cargo.toml"],
        capture_output=True, text=True, encoding="utf-8", errors="replace", check=True,
    ).stdout
)
packages = {package["id"]: package for package in metadata["packages"]}
nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}


def closure(root_name):
    start = next(pid for pid, package in packages.items() if package["name"] == root_name)
    seen, stack = set(), [start]
    while stack:
        current = stack.pop()
        if current in seen:
            continue
        seen.add(current)
        for dependency in nodes[current]["deps"]:
            # normal dependencies only: skip dev- and build-only edges
            if any(kind["kind"] in (None, "build") for kind in dependency["dep_kinds"]):
                stack.append(dependency["pkg"])
    return seen


def classify(expression):
    text = expression or "(none declared)"
    flags = []
    for marker in ("GPL", "LGPL", "MPL", "EPL", "CDDL", "BUSL", "SSPL", "Unicode", "OpenSSL", "CC-BY", "(none"):
        if marker in text:
            flags.append(marker)
    return flags


for root in ROOTS:
    members = closure(root)
    by_licence = defaultdict(list)
    for package_id in members:
        package = packages[package_id]
        by_licence[package.get("license") or "(none declared)"].append(f"{package['name']} {package['version']}")
    print(f"\n=== {root}: {len(members)} crates in its dependency tree")
    for expression, names in sorted(by_licence.items(), key=lambda item: -len(item[1])):
        flagged = classify(expression)
        marker = f"   <-- check: {','.join(flagged)}" if flagged else ""
        shown = ", ".join(sorted(names)[:4]) + (f", ... ({len(names)})" if len(names) > 4 else "")
        print(f"  {expression:<55} {len(names):>4}  {shown}{marker}")
