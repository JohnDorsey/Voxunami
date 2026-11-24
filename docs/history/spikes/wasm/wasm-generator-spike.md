# Spike: a terrain generator as a wasm module, native against wasm (2026-11-24)

Question 5 of `../../../specs/generator-identity-spec.md`: before any host is built, what does one real generator cost as wasm, and is the terrain the same?

**What it is.** A standalone workspace (not part of the main one), three crates:
- `spike_generator`: Cave's rolling hills (`games/voxunami_cave/src/generators/hills.rs`, copied because that crate cannot be built for wasm; it only uses the public types of `terrain`, so the generator code, the noise and the layering are the engine's own) behind `materials(x, y, z, out)`.
- `generator_module`: the same code as a `cdylib` for `wasm32-unknown-unknown`: `init(seed_low, seed_high)` and `generate(x, y, z)` into a buffer of 32,768 `u32`, **no imports**. 108,128 bytes.
- `harness`: runs the same 1,152 bricks (12 x 12 columns, bricks y -2 to 5, seed 5) natively, in wasmtime and in wasmi, each with and without fuel, and after each brick compares it with the native brick voxel for voxel (not timed).

Run it with `run.sh` (needs `rustup target add wasm32-unknown-unknown`; the build folder is outside the repository). Results: `results_2026-11-24.txt`.

**Findings** (**[measured]** unless marked; one machine, a Ryzen 5 7600X on Windows 10; the native side is built with the spike's release profile, LTO and `panic = "abort"`, not the game's):

| | mixed brick (700 of them: a surface or a cave) | one-value brick (452: sky or deep ground) | vs native |
|---|---|---|---|
| native | 5.2 ms | 1.29 ms | 1 |
| wasmtime (compiles to machine code) | 5.3 ms | 1.33 ms | **1.01** (three runs) |
| wasmtime with fuel | 6.5 ms | 1.63 ms | **1.24** (three runs) |
| wasmi (interpreter) | 71 ms | 17.8 ms | **13.6** (18.6 in a first run on a busier machine) |
| wasmi with fuel | 72 ms | 17.8 ms | 13.7 (16.5 in that run) |

- **Identical:** all 1,152 bricks equal the native ones voxel for voxel in all four wasm configurations (0 differing). That is one machine and one operating system; Linux was **not run**.
- **A one-value brick is not free:** it costs about a quarter of a mixed one, so the bounds shortcut does not classify all of them. (The harness's name for the class, "uniform", means only that the output is one value.)
- **Cost of the wasm side apart from the brick:** compile 59-64 ms (84-93 ms with fuel), once per module; instantiate 15-34 us; init 28-50 us; 1,472 KiB of linear memory per instance (the default 1 MiB stack among it, **believed**); reading a brick out 2 us.
- **Fuel** is a count of instructions that depends on the runtime: 159 million per mixed brick in wasmtime, 296 million in wasmi, for the same module and brick. Wasmtime's count was the same in all four runs; wasmi's was 295,672,244 in a run of two passes and 295,672,445 in a run of one (a difference of 201, under a millionth; the cause was **not investigated**, the two runs differ in their passes).

**What it does not say.** Not measured: several threads (a view's workers would each need an instance; the module is compiled once), another generator (a planet, or the coarse cells: `coarse_node`, `may_show` and `solid_core` would be more exports, and the host would call them), a generator with imports, the size of wasmtime in a build (the harness builds both runtimes in 3 minutes cold), Linux, a much larger module.
