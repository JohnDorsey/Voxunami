# Design decisions

Status key: **Decided** = the project owner said so. **Proposed** = Claude's recommendation, not yet confirmed.
**Measured** = verified by a throwaway script on the dev machine. **Open** = unresolved.

## Games (one monorepo, one shared launcher)
1. Minecraft-like: infinite in 3 dimensions, complex tick-based logic circuits, many LOD levels,
   gravity shifts, free-moving voxel structures. Voxels are 1 m cubes.
2. 3D 2048: push tiles with tools. Small and bounded.
3. 3D Tron with voxel flatworms that barrel-roll.
4. Tiny-voxel sculpting world: 1/32 m cubes, infinite and procedural, simulated water, swaying transparent leaves,
   gears / magnetism / gravity / friction heating / heat transfer / combustion, free-moving structures.

Every game gets: multiplayer, replay recording to video, lossless save round-trip with missing plugins.

## Decided
- Plugins are an internal modularity tool, not an end-user mod system.
- Voxels are strictly cubic. Real-time rendering is rasterized; a path-traced offline renderer for replay export comes later.
- Platforms: Windows and Linux. Integrated-GPU target: 30 fps at 720p with reduced view distance on the dev
  machine's own AMD iGPU.
- Naming: no uncommon abbreviations; very common ones (ID, LOD, GPU, UI) are fine.
- Gravity: local zones plus per-entity gravity, any direction (not axis-aligned). Gravity for a player may point
  toward large nearby solid terrain. "Sky light" comes from large open spaces, since there may be no sky.
- Multiplayer: about 12 players per game; Game 1 needs a path to thousands. Later, the game server can request
  terrain from other processes.
- Tick rate is configurable per game: 20 Hz default, Tron about 30 Hz. Logic and simulation are tick-based and
  frozen beyond simulation distance.
- Terrain: both vertically layered and uniform 3D-density world types. Clients generate terrain from the seed.
  The server saves and sends only terrain modified since generation (by players or natural processes).
- A world may use several terrain generators / generator versions. The server records which version produced each
  region in a coarse sparse octree: uniform subtrees collapse to one leaf, so data is spent only near boundaries between
  versions, and the tree is small enough to send to clients at any render distance.
- Free structures are articulated assemblies: rigid bodies with buoyancy, collisions, arbitrary-angle rotation and
  splitting when disconnected. They include aircraft, spacecraft and walking machines. A structure can carry smaller
  structures, merge with them, or connect to them by axle, hinge and similar joints. A structure is assembled from
  blocks placed on the terrain, then broken off the terrain, and it can merge back into the terrain.
- Game 4 simulation: water and heat run continuously; combustion fronts and falling blocks are event-driven. There are
  several fluid kinds; some are magnetic and some turn into gases (phase change).
- Build order: walking skeleton (Phase 1), then 2048, then Minecraft-core, then Tron, then the sculpting game.
- Workflow: Claude never commits; each phase ends with passing checks and a suggested commit message.

## Proposed
- Rust, wgpu, mesh-based GPU-driven rendering, nested clipmap LOD shells.
- Everything with voxels is a `Grid`: sparse, chunked, with a rigid transform. Terrain is the infinite root grid
  (procedural base plus saved overrides); structures, creatures and tiles are finite grids.
- Voxel coordinates are i64 per axis within +-2^60; rendering is camera-relative.
- A saved brick is stored whole and replaces the generated one, only when its content differs from generated.
- Server sends per-brick content hashes so a client whose generated terrain differs notices and requests the data.
- Generator version assignment: the first time a region is generated it is assigned the world's current default
  version, in whole aligned cubes of a per-game size (larger cubes mean fewer boundary nodes). Released versions are
  frozen; a missing generator plugin loses only unmodified terrain.
- Tick phases: each tick reads a frozen snapshot of the previous tick and produces the next one. A brick update never
  writes to a neighbour; cross-brick effects are queued and applied together in a fixed order. Lattice rules (heat,
  fluid flow) lose nothing; chain reactions within one tick (instant wire propagation, cascading updates) must be
  rewritten, for example by solving a connected wire network as one graph. Safety net: a partition-invariance test
  that shifts a scene by 1-31 voxels and requires identical results. Awaiting the owner's decision.
- Entity gravity direction is server-authoritative replicated state, computed from coarse LOD mass, smoothed per entity.
- Plugins are compile-time crates; one process per game, started by the launcher from a manifest.
- Brick size 32^3 voxels (rows fit a u32 bitmask; 1 m at Game 4 scale). Revisit when the mesher exists.

## Built
Phase 1a (headless foundations, all in `engine/`):
- `namespaced_id`: validated `namespace:name` identifiers; ordering is by namespace, then name.
- `voxel_space`: `VoxelCoordinate`, `BrickCoordinate`, `LocalVoxelIndex`; floor semantics for negative coordinates.
- `voxel_grid`: sparse `Grid` of `Arc<Brick>`; a brick has one palette-compressed `ChannelStorage` per channel of a
  `GridLayout`; copy-on-write makes `clone()` a cheap snapshot. An absent brick means all channels at default.
- `content_registry`: `Registry<T>` assigns dense `RuntimeId`s and keeps content-less placeholders for missing plugins.

Phase 1b (terrain, in `engine/`):
- `region_octree`: canonical sparse octree over brick coordinates (`RegionOctree`, `BrickCube`). Uniform subtrees collapse
  to one leaf; `leaves()` is a stable Morton-order serialization. Isolating one cube costs 7 leaves per tree level above
  it (337 for a level-8 cube in empty space) however many bricks inside it are generated.
- `terrain`: `DensityField` (positive = solid) with `sample` and conservative `bounds` at a `LevelOfDetail`; value noise on
  a power-of-two integer lattice (exact cell/fraction arithmetic, splitmix64 hashing, no transcendental functions);
  an octave is kept at a footprint only if its wavelength is at least the footprint, so LOD output differs from full
  detail by at most the dropped octaves' amplitude (tested). `FieldTerrainGenerator` turns a field into bricks (grass,
  3 dirt, then stone), classifying bricks as all air or all stone from bounds alone when it can. `WorldTerrain` assigns
  generator versions to regions lazily and reports unassigned or missing-generator regions instead of failing.
- `voxel_space` gained `VoxelBox` and `LevelOfDetail`; `voxel_grid` gained stable FNV-1a content digests.
- `clippy.toml` forbids the platform-dependent float functions workspace-wide.

## Measured
Storage (Ryzen 7600X, release, one thread, best of 7, counting allocator; the single-voxel `get` figures include
loop and bounds overhead, so bulk decode will be faster):
- `get` 1.2-2.6 ns per voxel for every representation; `Grid::get` at random coordinates 9 ns; `Grid::set` 27 ns.
- Filling a brick with `set`: 4-8 ns per voxel for up to 10 distinct values, 19 ns at 100 (linear palette search).
  `from_values` (3 distinct) takes 1.9 ns per voxel, so generators should build whole bricks.
- Memory: a 67 M voxel terrain-like region (2048 bricks, solid layers plus a surface layer) takes 2.5 MiB.
  Uniform bricks cost almost nothing; a 3-value surface brick channel is 8 KB; a dense channel is 4 bytes per voxel.
- Cloning a 2048-brick grid takes 14.5 us; the first write to a snapshotted surface brick takes 2.3 us.

Terrain generation (default rolling hills, release, one thread): a surface brick takes 5.4 ms and a brick the bounds
classify as sky 0.26 ms. Sheet caves run through almost everything, so only 1 of about 1300 sampled underground bricks was
uniform stone: the bounds shortcut helps in the sky but not underground with this cave design. A column-constant
heightfield is re-sampled for each of 36 voxels of height, a known inefficiency left for when it matters. Extrapolation,
not measurement: at Game 4's voxel size, 50 m around the player is about 24 000 surface bricks, tens of CPU-seconds.

Simulation throughput (plain f32 7-point heat stencil, one channel, no branching, 6 cores / 12 threads):
- DRAM-bound 256^3 grid: about 1.9 G voxel-updates/s on 1 thread, about 4 G/s on 6 or 12 threads.
- Cache-resident 32^3 bricks: about 3.2 G/s on 1 thread, about 16 G/s on 12 threads (baseline SSE2 build).
- `target-cpu=native` adds only about 10-20%.

Caveat: that is the cheapest possible kernel on a strong desktop CPU. Real kernels (material lookups, state machines,
fluids) will be several times slower, and the minimum-spec CPU slower still.
Implication: 300 M voxels updated at 20 Hz needs 6 G updates/s for one trivial kernel, so most voxels must sleep.
Implication: a continuous-valued channel stored densely costs 4 bytes per voxel (1.2 GB per 300 M voxels), so
continuous channels need quantization or sparse activity.

## Open
- Phased-tick rule: accept or modify (see Proposed).
- Active-voxel budget per tick for Game 4; set empirically once simulation exists.
- Generator boundaries: hard cuts, or may a new generator blend toward its neighbour? Who can trigger regeneration?
- How joints are authored, how merging aligns rotations (snap to 90 degrees?), and what happens on overlap.
- Physics: evaluate an existing engine (e.g. Rapier) versus custom, in a dedicated spike.
- How "sky light" is computed from open space.
- Terrain performance: skip sampling known-uniform columns, reuse planar noise across a column, and design caves so
  bounds can classify more bricks.
- Compact wire format for the assignment tree (the in-memory tree lists 7 siblings per level around an isolated cube).
- Cross-OS determinism is untested: only Windows is available on the dev machine so far.

## Build plan
Phase 1 is the walking skeleton. Each step ends with passing tests, clippy, rustfmt and a suggested commit.
- 1a Foundations: identifiers, coordinates, grid storage, registry. **Done.**
- 1b Terrain: density fields with bounds and footprint, deterministic noise, generator-version octree, brick
  generation with pinned golden digests. **Done.**
- 1c Seeing it: CPU meshing, wgpu window, fly camera, offscreen render test, integrated-GPU check.
- 1d Saving: world container with sections, edits over generated terrain, unknown-data preservation tests.
- 1e Server and stream: message protocol, loopback transport, recorder, replay-equals-live test.
- 1f Launcher and first game crate.
A physics spike can slot in after 1c.
