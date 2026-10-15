# Design decisions

Status key: **Decided** = the project owner said so. **Proposed** = Claude's recommendation, not yet confirmed.
**Measured** = verified by a throwaway script on the dev machine. **Open** = unresolved.

## Games (one monorepo, one shared launcher)
The engine is **Voxunami**.
1. **Voxunami Cave**: Minecraft-like. Infinite in 3 dimensions, complex tick-based logic circuits, many LOD levels,
   gravity shifts, free-moving voxel structures. Voxels are 1 m cubes. The current demo's caverns suit it; other
   terrain types should be added later. Textured blocks near the player, untextured blocks beyond a distance. Flood-fill
   lighting with 256 levels (see Voxunami Cave lighting below).
2. **Voxunami 2048**: 3D 2048, pushing tiles with tools. Small and bounded.
3. **Voxunami Flatworms**: 3D Tron with voxel flatworms that barrel-roll.
4. **Voxunami Sculpt**: tiny-voxel sculpting world: 1/32 m cubes, infinite and procedural, simulated water, swaying
   transparent leaves, gears / magnetism / gravity / friction heating / heat transfer / combustion, free-moving structures.

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

## Voxunami Cave lighting (decided; not built yet)
- Flood-fill light with 256 levels (0-255). The top levels are blindingly bright and rarely used.
- Several channels of "sky light" that all look the same colour by default but could optionally be shown in different
  colours. All spread by flood fill, losing 1 per step, but they have different sources: a block exposed to air gets a
  brightness equal to the number of air blocks in a row in the channel's direction. Example: a stone block with 77 air
  blocks to its east has brightness 77 in the east channel, and passes 76 in the east channel to its neighbours even if
  those neighbours have fewer than 76 air blocks to their east.
- Answers so far: the sky-light channels are the six axis directions; the run of air is capped at 255; block light
  and sky light are separate channels that the client combines visually with maths similar to screen-mode blending;
  light is stored on the transparent block, like Minecraft (decided; solid blocks hold no light). So the stone-block
  example above is read as the exposed stone face lighting the transparent block in front of it; working out the exact
  source value for that block is part of building the lighting system.
- Light that reaches 255 blocks crosses many bricks, so updates need the phased-tick rule's brick-boundary handling.

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
- Physics: Rapier (rapier3d 0.36.1) behind a Voxunami adapter crate, with `enhanced-determinism`, single-threaded stepping
  on the server, a local floating-origin frame, large merged voxel colliders for terrain, and gravity, buoyancy and
  drag applied as per-body forces. Evidence and caveats are in `docs/physics-spike.md`. Awaiting the owner's confirmation.
- Network stream (`docs/protocol-and-replay.md`): clients tell the server a box of bricks to watch; the server sends
  which generator made each part and every *edited* brick in it, then each tick the changes inside it. Terrain nobody
  has edited is never sent. A brick's first edit is sent as the whole brick, later edits as change lists with a digest of
  the result. Game and plugin messages ride in a generic message that names its owner by identifier.
- Replay recording is the tick-by-tick stream one client received, appended to a checksummed file as it arrives; playing
  it back is a connection that plays the server, so the client code is unchanged.
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

Phase 1c (seeing it):
- `voxel_space` gained `FaceDirection` and `WorldPosition` (an exact integer brick plus an `f32` offset, so positions and
  camera-relative vectors stay precise at any distance).
- `voxel_meshing`: greedy meshing of a brick and its six neighbours into 8-byte `PackedQuad`s. Verified against a
  slow reference that quads cover exactly the exposed faces, once each, with the right material.
- `renderer`: wgpu renderer (`VoxelRenderer`, `Camera`, `GraphicsContext`). One instanced draw per visible brick; the
  vertex shader expands each packed quad into two triangles, so there are no vertex or index buffers. Frustum culling
  and distance fog; placement is camera-relative from exact integers; material values beyond the palette draw
  magenta (how content from a missing plugin will look). Offscreen rendering to RGBA for tests and screenshots.
- `tools/terrain_viewer`: developer tool. `WorldStreamer` generates and meshes bricks on worker threads (a brick is
  meshed only when it and its six neighbours exist, unloading beyond the view radius); a winit window with a fly camera;
  headless `--screenshot` and `--benchmark`; `--adapter integrated` to test on the integrated GPU.
- GPU tests run on every physical adapter and backend (here: RTX 3090 and the AMD iGPU, each on Vulkan and DX12).
  Without a GPU they skip, unless `VOX_REQUIRE_GPU` is set.

Phase 1d (saving):
- `voxel_grid`: channels declare what their values mean (`ChannelValueKind`: a plain number, or an id from a named
  registry); `remap_values`; a canonical, strictly validated brick encoding.
- `content_registry`: `IdentifierRegistry`, a type-erased view for translating ids.
- `terrain`: `WorldTerrain` exposes its configuration and can restore saved assignments.
- `world_storage`: the section container, the engine sections, `WorldData` (terrain recipe + edited bricks + plugin
  sections), `ParsedWorld` / `LoadContext` / `LoadReport`. The format is specified in `docs/world-file-format.md`.

Phase 1e (the stream; protocol version 1, documented in `docs/protocol-and-replay.md`):
- `binary_codec` (the byte primitives, moved out of `world_storage`), `voxel_space::BrickBox` (inclusive box of bricks,
  with `without` for the newly watched part), `region_octree::BrickCube::covering`, `voxel_grid::Grid::content_digest`.
- `world_storage`: `WorldData::describe` (the world's shape as a small world file), `SourceTranslator` (translates a
  source's bricks, values and generator ids into the loaded world's numbering; loading uses it, and so does a client),
  and a `test-support` module holding the pretend game with switchable plugins that the tests of every crate below share.
- `protocol`, `transport` (connection interface and in-process loopback), `server` (the tick loop, sessions, watched
  regions, edit replication, `GameLogic` for games to implement), `client` (handshake, a world view built only from
  server messages, applied one whole tick at a time), `replay` (file format, recording connection, playback connection).
- `end_to_end_tests`: a real server, clients and recorders together. Includes replay-equals-live and the check that
  every player sees what the server has in the region it watches.

## Measured
Storage (Ryzen 7600X, release, one thread, best of 7, counting allocator; the single-voxel `get` figures include
loop and bounds overhead, so bulk decode will be faster):
- `get` 1.2-2.6 ns per voxel for every representation; `Grid::get` at random coordinates 9 ns; `Grid::set` 27 ns.
- Filling a brick with `set`: 4-8 ns per voxel for up to 10 distinct values, 19 ns at 100 (linear palette search).
  `from_values` (3 distinct) takes 1.9 ns per voxel, so generators should build whole bricks.
- Memory: a 67 M voxel terrain-like region (2048 bricks, solid layers plus a surface layer) takes 2.5 MiB.
  Uniform bricks cost almost nothing; a 3-value surface brick channel is 8 KB; a dense channel is 4 bytes per voxel.
- Cloning a 2048-brick grid takes 14.5 us; the first write to a snapshotted surface brick takes 2.3 us.

Rendering and loading (Ryzen 7600X; 1280x720; default rolling hills, 1 m voxels; offscreen, GPU time included):
- Integrated GPU (AMD Radeon, Vulkan): 6.9 ms per frame (146 fps) at view radius 12 (1.4 M quads drawn of 5.5 M
  resident) and 2.4 ms (416 fps) at radius 8. The target was 30 fps. The windowed viewer ran at 359 fps on it.
- RTX 3090 (Vulkan): 1.7 ms (600 fps) at radius 12; 0.6 ms at radius 8. An earlier run of the same benchmark
  averaged 4.5 ms with a 26 ms 95th percentile and did not reproduce in three repeats (probably the GPU clock ramping).
- Loading radius 12 (9171 bricks generated, 3366 meshed) took 4.2 s on 11 worker threads, so loading, not drawing, is
  the cost to attack next. Quad counts are high: 5.5 M quads (44 MB) at radius 12, from 1-voxel detail and caves.
- Memory per quad is 8 bytes; indexed triangles would be about 56.
- Only the 7600X's integrated GPU was measured; a weaker minimum-spec GPU is untested.

Cross-platform determinism: all tests, including the pinned terrain digests and the pinned recording of a standard
multiplayer session, pass on Linux (Ubuntu in WSL2, rustc 1.99.0) as well as Windows. Same CPU architecture and compiler version, so this does not cover other architectures.

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
- Network scale: a socket transport with bounded queues; a spatial index over watched regions (an edit currently scans all
  clients); resending single bricks to repair a client that fell out of sync; replay keyframes for seeking; interest with a
  level of detail. See "Known limits" in `docs/protocol-and-replay.md`.
- Generated terrain on the server is regenerated on every read of an unedited brick; a cache is needed before real games.
- Which replicated entity state (players, structures) is common to all games; it travels as game messages until a second
  game shows what to promote.
- Active-voxel budget per tick for Game 4; set empirically once simulation exists.
- Generator boundaries: hard cuts, or may a new generator blend toward its neighbour? Who can trigger regeneration?
- How joints are authored, how merging aligns rotations (snap to 90 degrees?), and what happens on overlap.
- How "sky light" is computed from open space.
- Terrain performance: skip sampling known-uniform columns, reuse planar noise across a column, and design caves so
  bounds can classify more bricks.
- Compact wire format for the assignment tree (the in-memory tree lists 7 siblings per level around an isolated cube).
- Determinism is verified only for x86-64 and rustc 1.99.0 (Windows and Linux); other architectures and compiler
  versions are untested.

- Rendering at scale: the 5.5 M-quad view at radius 12 needs LOD (coarser bricks far away) and GPU-driven culling to
  scale further; draw calls are currently one per visible brick.
- GPU tests that are *failing* while running in parallel have twice crashed or hung the test process (an access violation,
  and a ten-minute hang); neither reproduced in later parallel runs and passing runs have been stable. When debugging a
  GPU test failure, run `--test-threads=1` under a timeout.
- Interactive controls (keyboard, mouse capture, resizing) were written but not exercised by the assistant, who cannot
  see or touch the window; the window opens, renders and closes cleanly on both GPUs.

## Build plan
Phase 1 is the walking skeleton. Each step ends with passing tests, clippy, rustfmt and a suggested commit.
- 1a Foundations: identifiers, coordinates, grid storage, registry. **Done.**
- 1b Terrain: density fields with bounds and footprint, deterministic noise, generator-version octree, brick
  generation with pinned golden digests. **Done.**
- 1c Seeing it: CPU meshing, wgpu renderer, window with fly camera, offscreen render tests, integrated-GPU check. **Done.**
- 1d Saving: world container with sections, edits over generated terrain, unknown-data preservation tests. **Done.**
- 1e Server and stream: message protocol, loopback transport, recorder, replay-equals-live test. **Done.**
- 1f Launcher and first game crate.
The physics spike is done (`docs/physics-spike.md`); building the physics adapter belongs with the structures work.
