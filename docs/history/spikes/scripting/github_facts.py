"""Read-only GitHub API facts for the scripting candidates (raw JSON through `gh api`, not a summariser)."""
import json
import subprocess
import sys

REPOS = [
    "rune-rs/rune", "rhaiscript/rhai", "koto-lang/koto", "facebook/starlark-rust", "pydantic/monty",
    "quickjs-ng/quickjs", "bellard/quickjs", "DelSkayn/rquickjs", "boa-dev/boa", "luau-lang/luau", "mlua-rs/mlua",
    "janet-lang/janet", "albertodemichelis/squirrel", "wren-lang/wren", "mruby/mruby",
    "cel-rust/cel-rust", "01mf02/jaq", "CertainLach/jrsonnet", "eduardosm/rsjsonnet", "tweag/nickel", "kcl-lang/kcl",
    "godotengine/godot", "libriscv/godot-sandbox", "IppClub/YueScript", "RustPython/RustPython", "pocketpy/pocketpy",
    "wasmerio/wasmer", "bytecodealliance/wasmtime", "EngineHub/WorldEdit", "NLnetLabs/roto", "mattwparas/steel",
]


def gh(path):
    result = subprocess.run(["gh", "api", path], capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60)
    if result.returncode != 0:
        return None
    return json.loads(result.stdout)


def main():
    print(f"{'repo':<28} {'licence':<14} {'stars':>6} {'open issues':>11} {'pushed':<11} {'archived':<8} latest release")
    for repo in REPOS:
        info = gh(f"repos/{repo}")
        if info is None:
            print(f"{repo:<28} (not readable)")
            continue
        licence = (info.get("license") or {}).get("spdx_id") or "none"
        release = gh(f"repos/{repo}/releases/latest")
        latest = f"{release['tag_name']} {release['published_at'][:10]}" if release else "-"
        print(f"{repo:<28} {licence:<14} {info['stargazers_count']:>6} {info['open_issues_count']:>11} {info['pushed_at'][:10]:<11} {str(info['archived']):<8} {latest}")
    print()
    for label, path in [
        ("godot-proposals #2972 (list comprehension)", "repos/godotengine/godot-proposals/issues/2972"),
        ("monty PR #784 (lazy generator expressions?)", "repos/pydantic/monty/pulls/784"),
        ("monty PR #1020 (generator functions?)", "repos/pydantic/monty/pulls/1020"),
        ("monty issue #364 (next() on genexpr)", "repos/pydantic/monty/issues/364"),
        ("pocketpy issue #449 (genexpr)", "repos/pocketpy/pocketpy/issues/449"),
        ("luau rfcs PR #185 (table comprehension)", "repos/luau-lang/rfcs/pulls/185"),
    ]:
        item = gh(path)
        if item is None:
            print(f"{label}: (not readable)")
            continue
        merged = item.get("merged_at") or ""
        print(f"{label}: title={item['title']!r} state={item['state']} reason={item.get('state_reason')} created={item['created_at'][:10]} closed={(item.get('closed_at') or '')[:10]} merged={merged[:10]}")


main()
