# Voxunami

A voxel game engine plus several games in one monorepo, with a shared launcher.
Games: Voxunami Cave (Minecraft-like), Voxunami 2048, Voxunami Flatworms (Tron-like), Voxunami Sculpt (tiny voxels).
Read `docs/design-decisions.md` before architectural work; update it when a decision changes.
Layout: `engine/*` libraries, `plugins/*` content, `games/*` games, `launcher/` starts them.
Run a game: `cargo run --release -p launcher -- cave` (`--help` lists options; `replay FILE` watches a recording;
`serve cave` runs a dedicated server; `cave --join HOST` joins one).

## Principles
- Built to last and grow: no technical debt; bold refactors are fine if they are better.
- Descriptive names. No uncommon abbreviations (`position` not `pos`, `coordinate` not `coord`);
  very common ones (ID, LOD, GPU, UI) are fine.
- Simplicity first. Content and features are plugins where possible.
- Dependency direction: games -> plugins -> engine. The engine never knows about a plugin or game.
- New code starts in the game that needs it; promote it to shared code when a second game does.
  Exception: foundations (coordinates, grid storage, registry, save format, protocol) are designed up front.

## Hard rules
- Terrain generation and gameplay simulation must be bit-identical on Windows and Linux:
  no std transcendental float functions (`sin`, `exp`, `powf`...; use `libm`; `clippy.toml` enforces this),
  no fast-math,
  no wall-clock time in simulation or client cosmetics (use game tick + seed).
- Saved files contain only namespaced string ids (`plugin:name`). Data from missing plugins is
  preserved byte-for-byte on load/save, never dropped.
- The client's view of the world comes only from the server message stream (singleplayer runs an
  integrated server). This is what makes replay recording work.
- A terrain generator version is a file format: never edit a released one, add a new version.

## Workflow
- Claude commits and pushes when a phase is done (the owner lifted the old no-commit rule). Each phase ends with passing
  tests, clippy and rustfmt before it is committed.
- Phased delivery: tests first, safest piece next, verify, continue.
- When a turn ends waiting for the user to say "continue", suggest an effort level for that next prompt.
- Crates share test strategies and fixtures through a `test-support` cargo feature that dependents enable
  in their dev-dependencies. Do not make a test-only crate that depends on the crate under test.
- Mutation experiments (deliberately breaking code to check tests bite) must set
  `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1`; `proptest-regressions/` files are only for real failures.
- Checks: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`.
