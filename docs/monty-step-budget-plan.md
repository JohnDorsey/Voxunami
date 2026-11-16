# Monty step budget: a plan

**Status: a proposal. Nothing is built.** Written 2026-11-16 for the owner's approval; it extends the Monty fork described in the section "Decision (2026-11-15): Monty" of `history/interoperability-survey.md`.

## Why

Ship computers run on the server, and their results can reach the simulation, which must be bit-identical on every machine. Monty's limits are wall-clock or CPU time, and memory. A time limit cuts a script off at a different instruction on a slow machine than on a fast one, so the same ship computer could do different things on two servers, or on the same server on a busy day. A **step budget** counts work instead of time, so the cut-off is the same everywhere. Time limits stay as a second, independent safety net.

Evidence for the gap (read from the fork's source at `v1.6.0`, plus the in-process probe): `ResourceLimits` has `max_feed_duration`, `max_turn_duration`, `max_memory`, `max_recursion_depth`, `max_suspensions` and `max_total_sleep`; nothing counts instructions. A measured wrinkle: `max_memory` without `monty-alloc` as the global allocator stops only big single allocations, not a loop that appends many small objects (see the survey). That is a second determinism and safety gap, discussed under "Not part of this plan".

## What a step is

- **One bytecode instruction is one step.** The dispatch loop in `crates/monty/src/bytecode/vm/mod.rs` (`VM::run`) already counts instructions with a `u8` countdown (`CHECK_INTERVAL = 255`) and runs a checkpoint every 256th. That countdown is per `run()` call, so a nested call (a sort key callback, a `map` function) restarts it and a short callback never reaches a checkpoint. For a count that cannot be dodged, `run()` must add the instructions it actually executed when it returns, on every exit path, not only at checkpoints.
- **Native loops charge by items.** Rust code that loops over items calls `check_time_every(i)`, which fires once per 64 items (`LOOP_CHECK_INTERVAL`). I counted 133 such call sites (`check_time_every` and `check_memory_time_every`) across 62 files, plus 40 plain `check_time` and `check_memory_time` calls. When one fires it would also charge 64 steps. Work in a partial block of fewer than 64 items goes uncharged, so one step can stand for at most about 64 items of native work: a bounded constant factor, which is acceptable for a safety budget.
- **Large single operations charge by size.** Operations such as `2 ** 10_000_000`, `'a' * 10**8`, sorting a million elements, regular expressions and `Decimal` do their work in one native call. They need an audit (phase 4) so each charges steps in proportion to its input or output size, or is refused above a size.
- **The count depends on the bytecode compiler.** If an upstream release changes how Python compiles to bytecode, every script's step count changes. Pin the fork to one release line, keep a reference script whose count is a golden number, and treat a budget as tuned to a Monty version.

## Decisions I recommend (the owner may overrule any)

1. **Kill, do not pause.** When a budget runs out, the script ends with an uncatchable error, like the other resource errors. A script that wants to keep running calls a host function such as `wait(ticks)`, which is the only place the host can pause it. ComputerCraft does the same ("too long without yielding") [recalled, not checked]. Pausing in the middle of an instruction would need a snapshot of native Rust frames, which Monty cannot take.
2. **Two budgets, matching the time limits:** `max_feed_steps` (summed across turns, reset when a feed starts) and `max_turn_steps` (reset at each resume). A ship computer gets a fresh turn budget each tick.
3. **Counts are `u64` and survive a snapshot,** like the feed time does today.
4. **Time limits stay.** A step budget does not replace them; it bounds the deterministic part.

## Phases (tests first, safest piece next, verify, continue)

**Phase 0: tests that fail now, and a baseline.** In `crates/monty/tests/step_budget.rs`:
- the same script and budget give the same outcome and the same final step count on repeated runs, in debug and release builds, and (by the golden-number method used for `portable_math`) on Windows and on Linux;
- a golden count for three small scripts (a counting loop, a function-call loop, a comprehension) pins the counting rule;
- exceeding `max_turn_steps` raises an uncatchable error that `try/except` and `finally` cannot intercept, with a message naming the scope, limit and used count;
- a fresh turn after a host call resets the turn count; the feed count carries on;
- a snapshot round trip through serde keeps the counts;
- `sum(range(10**9))`, `sorted(range(10**6), key=lambda x: x)` and `map(lambda x: x, range(10**9))` all stop (the last two are the callback hole described above);
- a debug build and a release build stop at the same step count even with a competing busy thread, which is the property a time limit cannot give;
- with no step limit set, the interpreter's speed is unchanged (the repository already has `_limits` benchmarks; run them before and after).

**Phase 1: the tracker.** Add the two limits to `ResourceLimits` with builder methods, the counters to `ResourceTracker`, a `ResourceError::Steps` variant with its scope, `tighten_limits` support, and serde defaults. First check how a dump with a changed `ResourceLimits` shape loads: the crates appear to use a positional wire format (`postcard` is among their dependencies), and the dump code has a version and an upgrade path (`dump_format.rs`, `dump_upgrade.rs`), so this may need a version bump. We never keep dumps as game data, so I would bump and not keep compatibility; I have not read that code yet.

**Phase 2: the VM.** Count instructions exactly (flush on every exit of `run()`), check against the limits at the existing checkpoint, hoist the "is a step limit set" test as the loop already does for time and memory.

**Phase 3: native loops.** Make the `check_time_every` family charge 64 steps when it fires. All 133 sites then charge without being edited one by one.

**Phase 4: the audit.** List every native operation whose cost is not proportional to what the loop polling sees: big-integer power and multiplication, string and bytes repetition, sorting, regular expressions (the `fancy-regex` crate can backtrack), `json`, `hashlib`, `Decimal`, `math.factorial`, `math.comb`. Give each a size-proportional charge or a size refusal, with one test each. This is the largest and least certain phase; I would report its findings before building it.

**Phase 5: hosts.** Expose the two limits in the worker pool's configuration (ship computers use `monty-pool`) and the wire messages, and check the Python and JavaScript bindings still build.

**Phase 6: verify and record.** Full tests on Windows and Linux, clippy, formatting, the `FORK.md` section, the survey, and the probe re-run with step limits.

## Cost and risks

- **Cost:** phases 0 to 3 are about M (the counting is small; the tests and the format question are most of it), phase 4 about M on its own, phases 5 and 6 about S. I would stop after phase 3 to report before starting phase 4.
- **Risk, count drift:** an upstream compiler change shifts every count (above).
- **Risk, a hole nobody listed:** the audit finds native work with no size to charge against. The honest answer then is a size refusal, which changes what scripts may do.
- **Risk, rebase cost:** the VM loop and 133 call sites are where upstream also changes; the fork gets harder to rebase. Phase 3 deliberately touches one function, not every site.
- **Not part of this plan:** making the memory limit deterministic. Without `monty-alloc` as the global allocator, `max_memory` does not stop many small objects; with it, the count is of requested bytes, which I believe are the same on every 64-bit platform but have not verified. Ship computers run in a worker process where `monty-alloc` can be installed. A cross-platform test that the memory error fires at the same point is a separate, smaller task.

## What I need from the owner

- Approval of the five decisions above (kill not pause; two budgets; counts survive snapshots; time limits stay; a pinned Monty line).
- Whether to build phases 0 to 3 now and report before phase 4.
