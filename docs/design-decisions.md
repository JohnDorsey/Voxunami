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
- The grid is always cubic (owner, 2026-10-16), apart from coarser levels of detail in the distance. A block may fill only
  part of its cell, using a block shape chosen by the block's *state*, so that shapes do not multiply the number of block
  types and run into the renderer's palette cap (256 material values, 16 of them used by avatars). So a voxel carries
  a block and a state, as two channels (built; see "Built", Phase 7). A game without states leaves the second channel out.
- Worlds have several levels (built; see "Built", Phase 6). Each level has its own grid, terrain assignments and edited
  bricks. All levels of a world share its channels, content registries, terrain generator versions and **seed**: a level
  differs from another by which generator versions it uses and by what players have changed, since a client regenerates
  terrain from the world's seed and has no other way to learn a level's. The first level is the main level, where players
  start. Levels are named by identifier in files and in game messages, and by their position (`LevelIndex`) between a
  server and its clients. A world's levels are fixed while a server runs.
- Which level a player is in is the server's to say (`player_presence::PoseBook`); players say only where they are in it. A
  player moves with the `/level go NAME` command, or a game moves them (a portal later) through the same message.
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
- Workflow: Claude commits and pushes finished phases (rule lifted by the owner after Phase 1f); each phase ends with
  passing checks.

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
- How light reaches the renderer (decided by the spike in `docs/light-spike.md`, built in Phase 7): each quad carries its
  light in 8 bits of its material word, the mesher uses the light of the voxel in front of a face as part of the merge
  key, and the quad stays 8 bytes. Measured on rolling hills with made-up lamp light: meshing time +2 to +8 percent, quad
  count +25 percent for Minecraft-style 16 levels around very dense lamps, +62 percent for 256 smooth levels, none for
  uniform light. Rounding light to fewer bands in the mesher is the lever if a scene gets heavy (not built); a light
  texture is the later route to smooth shading and would not change storage or the protocol.

## Decided (rules for lattice simulation and physics)
- Tick phases (confirmed by the owner): each tick reads a frozen snapshot of the previous tick and produces the next one.
  A brick update never writes to a neighbour; cross-brick effects are queued and applied together in a fixed order.
  Lattice rules (heat, fluid flow) lose nothing; chain reactions within one tick (instant wire propagation, cascading
  updates) must be rewritten, for example by solving a connected wire network as one graph. Safety net: a
  partition-invariance test that shifts a scene by 1-31 voxels and requires identical results. Entities (such as 2048's
  moving blocks) are not lattice cells and are processed one at a time in a fixed order; see "Decided (2048)".
- Physics (confirmed by the owner): Rapier (rapier3d 0.36.1) behind a Voxunami adapter crate, with
  `enhanced-determinism`, single-threaded stepping on the server, a local floating-origin frame, large merged voxel
  colliders for terrain, and gravity, buoyancy and drag applied as per-body forces. Evidence and caveats are in
  `docs/physics-spike.md`. Not built yet.

## Decided (2048)
- No largest number (owner, 2026-10-16). A voxel is a material's block and, as its state, a number (built: Phase 8), so
  numbers are not a list of blocks and are not limited to powers of two. The one remaining limit is that the state is 32
  bits: the largest number is 4 294 967 295, and a merge whose result would not fit does not happen (the blocks meet as
  if they were different). Reaching it by play is not realistic (2^31 needs a billion merges of 2s), and going further
  is additive: a second channel for the high 32 bits, which old worlds already hold as its default 0.
- Voxunami 2048 is infinite: a landscape of rolling hills in which every solid voxel is a numbered block (a power of
  two, half of them 2s, a quarter 4s, and so on up to 64s; material in cubes of 8 voxels: 70% stone, 20% ice, 10% spring
  ice). Any block can be pushed, and only tools push or pull blocks (left click pushes away from the player, right click
  pulls toward them, within reach of the player's last pose).
- What happens to a pushed block depends on its material. Stone takes one step and does not coast. Ice coasts, one step
  per tick, until it meets another block, and does not push it: it stops. Spring ice is ice that bounces back the way it
  came. Later materials will pass momentum on (deterministic, identical on every platform, integer ratio speeds).
- What a moving block does to a block it meets is decided by the tool that pushed it (built: Phase 9). The double tool
  merges equal numbers (whatever either is made of) into their double; the add tool adds any two numbers; the subtract tool
  leaves the difference, and equal numbers cancel and both blocks vanish. The block that was struck stays where it was,
  keeping its own material, with the new number. A result that does not fit in 32 bits is not a merge. The score is the
  sum of the numbers of the blocks merging has made (a cancellation makes none).
- Assumptions made while building it, easy to change: blocks never push other blocks (the owner's words allow stone to
  push a row; only the momentum materials will transfer motion); a push has a range (48 steps for the ices, since an
  infinite world may have nothing in the way) and every tick of a motion, bounce included, uses up a step; pushing a
  block that is already moving gives it the new direction and the full range; blocks of 2^20 do not merge.
- Planned (owner): blocks will push chains of blocks, eventually. There may be tools beyond the three.
- A player chooses the tool with the keys 1, 2, 3 (double, add, subtract); the push message names it, a moving block keeps
  the tool that pushed it (it is saved with the block), and pushing a block already in motion gives it the new tool.
- Moving blocks are entities, not lattice cells, so they do not need the phased-tick rule: they act one at a time in the
  order they started moving, each seeing what the earlier ones did that tick (`voxunami_2048::motion`). That order
  decides what happens when two blocks meet in the same tick, and is the same on every machine. Which blocks are moving
  is saved with the world (the section `voxunami_2048:progress`), so saving mid-motion loses nothing. The phased-tick rule
  stays proposed for lattice rules (light, fluids, heat), which are the first thing that needs it.

## Future goals (owner, 2026-10-16; none built)
- Blocks that let flood fill (light, and later other fills) through some sides but not others, like a slab or a stair.
  The block shape in the block definition now says which faces are open (`Shape::connections`, built in Phase 7) and
  is the collision shape; lighting and physics do not use it yet. (The grid stays cubic; a block fills part of its cell
  through its state; see "Decided".)
- Shaders written in GLSL or HLSL. (Not checked here: wgpu's shader translator, naga, reads WGSL, SPIR-V and GLSL, and I
  believe not HLSL directly, which would need compiling to SPIR-V first.) Cosmetic and client-side only, so they cannot
  affect simulation; they need a defined interface, validation and limits, and do not exist in the planned offline
  path-traced replay renderer.
- Fire spread in Cave and other randomly ticked events such as coral growth: each tick the server samples positions in the
  active region with a deterministic random number from (seed, tick, position). Needs flammability and similar block
  properties, an active-region definition and a per-tick budget. (Coral needs water, which Cave does not have yet.)
- Motorized hinges and pistons to move constructions that are off the main grid: sub-grids with transforms, driven by
  Rapier revolute and prismatic joints with motors. The physics spike showed a motor-driven hinge identical on Windows and
  Linux. Needs sub-grids in the stream, a model matrix per grid in the renderer, and raycasts in a grid's own space.
- Multiple levels, first reached by commands and later through portals. A level id has to run through the protocol, the
  save format and the server, and there must be a command channel. Portals that show another level need rendering two
  worlds in one picture.

## Proposed
- Order of features, to let real uses shape the engine and the plugin API before either is frozen (proposed 2026-10-16;
  the guide is: do first what is expensive to change later, what is most uncertain, and what gives a second user to a
  shared piece). 1. Levels: a level id through the protocol, save format and server, with one level existing; then a text
  command channel (done: Phase 6). 2. Block shapes in block definitions (open faces, collision shape, translucency, light), with a spike
  on how light reaches the 8-byte quad (done: Phase 7). 3. 2048 tools: chain pushing and tool-chosen merging. 4. Lighting with
  directional flood fill: the first lattice rule, under the phased-tick rule and its partition-invariance test.
  5. Random ticks and fire in Cave: the second lattice rule; only then extract the common lattice-rule shape
  (neighbourhood snapshot in, edits out, deterministic random numbers, active region, budget). 6. Cave gravity and
  collision on Rapier. 7. Flatworms (movable earlier or later; it needs no physics). 8. Mod stages 1 to 5 (see below),
  started once step 5 shows what a block definition and a rule hook carry. 9. Sub-grids with hinges and pistons, and Sculpt.
  10. Mod hooks: lattice rules first, then physics (engine-provided motor and piston parts that mods configure rather than
  implement). 11. Portals, user shaders, assets and an SDK. Level awareness must be in the mod API from its first version.
- Mods for players (proposed 2026-10-16; today plugins are compile-time crates, see "Decided"). One host API, two loaders:
  the contract is a versioned table of C-callable functions the host hands to a mod, plus a few functions the mod exports
  (register content, generate a brick, later: tick and message hooks). A native-library loader (C ABI, for mods the owner
  trusts: first-party add-ons, server-admin installs) and a WASM loader (sandboxed, one file for every platform, for mods
  players download) sit behind that one contract, so a mod written in C can be built either way. Native code cannot be
  sandboxed and cannot be forced to give the same floating-point results on every machine, so only the WASM loader is
  for untrusted mods and for anything clients must reproduce, such as terrain generators. Bulk calls only (whole bricks),
  never per-voxel calls across the boundary. Staged, each stage ending with something that runs and is tested:
  1. Settle the in-tree plugin API with the games we have (more of 2048's materials and Cave's rules into plugins), so the
     contract is shaped by real uses. 2. Data-driven mods (a folder or zip with a manifest: blocks, colours, materials):
     no code runs, and it forces the manifest, mods folder, ids, versions and missing-mod behaviour. 3. Dogfood the API:
     run `base_blocks`' generator through it; the pinned digests must match the native output exactly. 4. Spike both
     loaders as throwaway prototypes and measure boundary cost, generator speed against native, and identical results on
     Windows and Linux; choose from the numbers. 5. One narrow code mod type, terrain generators, with tests that
     hostile mods (endless loop, huge memory, wild writes, crash) are stopped. 6. Multiplayer: the handshake lists the
     mods a world needs by id and hash, and mismatches are refused clearly; downloading comes later. 7. Server-side
     logic hooks with CPU and memory budgets, on top of the phased-tick rule. 8. Assets, an SDK, templates and an example
     mod; only then promise compatibility. The API is labelled experimental until others have written real mods. A mod
     cannot add GPU code, change the engine's internals (save container, network protocol, renderer, physics), touch the OS,
     or depend on wall-clock time or unseeded randomness.
- Rust, wgpu, mesh-based GPU-driven rendering, nested clipmap LOD shells.
- Everything with voxels is a `Grid`: sparse, chunked, with a rigid transform. Terrain is the infinite root grid
  (procedural base plus saved overrides); structures, creatures and tiles are finite grids.
- Voxel coordinates are i64 per axis within +-2^60; rendering is camera-relative.
- A saved brick is stored whole and replaces the generated one, only when its content differs from generated.
- Server sends per-brick content hashes so a client whose generated terrain differs notices and requests the data.
- Generator version assignment: the first time a region is generated it is assigned the world's current default
  version, in whole aligned cubes of a per-game size (larger cubes mean fewer boundary nodes). Released versions are
  frozen; a missing generator plugin loses only unmodified terrain.
- Entity gravity direction is server-authoritative replicated state, computed from coarse LOD mass, smoothed per entity.
- Network stream (`docs/protocol-and-replay.md`): clients tell the server a box of bricks to watch; the server sends
  which generator made each part and every *edited* brick in it, then each tick the changes inside it. Terrain nobody
  has edited is never sent. A brick's first edit is sent as the whole brick, later edits as change lists with a digest of
  the result. Game and plugin messages ride in a generic message that names its owner by identifier.
- Replay recording is the tick-by-tick stream one client received, appended to a checksummed file as it arrives; playing
  it back is a connection that plays the server, so the client code is unchanged.
- Plugins are compile-time crates. The launcher links the games and starts one by command name (revised in 1f from
  "one process per game from a manifest": simpler, and nothing needs it yet).
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
- `tools/terrain_viewer` (since retired, see 1f): `WorldStreamer` generated and meshed bricks on worker threads (a brick is
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

Phase 9 (tools choose how blocks combine, Voxunami 2048):
- `tool::Tool` (`Double`, `Add`, `Subtract`) and `Tool::combine(moving, struck)`, which says whether two numbers combine
  and into what (`Combination::Becomes(number)` or `Vanish`). The merge rule left `NumberedBlock`. A `Mover` has a tool;
  `Movers::push_block` takes one; a push request carries it (one more byte); a saved mover carries it (saved section
  version 4; older versions read as the double tool).
- The client has the chosen tool in its state and shows it in the title. Property tests: with double or add the total of
  all numbers is kept; with subtract it never grows and no block is ever numbered 0.
- Not built: chain pushing (next), more tools.

Phase 8 (numbers as block states, Voxunami 2048):
- A voxel of 2048 is a material's block (`voxunami_2048:stone`, `ice`, `spring_ice`; air is `voxunami_2048:air`) and, in
  the channel `voxunami_2048:number`, its number as the state. `NumberedBlock` is a material and a `u32` number;
  `from_voxel(block, number)` is `None` for air, for blocks from elsewhere and for a number of 0. Emptying a voxel resets
  its number, so air holds the default in both channels (tested).
- The engine learned colours by state: `BlockDefinition::with_state_colors` and `BlockTable::material_of` give each
  (block, state) a palette entry, and the mesher merges by that. Colours repeat (state `s` has colour `s % count`), so
  every state has one. 2048 uses 61 colours per material: 2 is a primitive root modulo 61, so the first 60 powers of
  two each have a colour of their own and any other number shares the colour of the power with its residue. The
  renderer's palette limit (240) is respected: 4 blocks plus 183 colours.
- The landscape generator is now `voxunami_2048:numbered_hills_v2`: the same numbers and materials at the same voxels as
  version 1, written as a material block plus a number. Version 1 (a block for every number) is retired, not edited: a
  world that names it still loads, keeps its edits and its generator name, and has no terrain where that generator made
  it (tested). Worlds made before this phase are therefore not playable; none were kept.
- The progress message and the saved state carry the largest merge as a number; the saved section is version 3, and
  versions 1 and 2 (an exponent) are still read. The score saturates instead of overflowing.
- Merging is still the doubling rule of `NumberedBlock`; moving it into tools is the next phase.

Phase 7 (block states and shapes, and light in the quad):
- `block_models` (new engine crate): a `Shape` is a union of boxes in eighths of a voxel, from which it works out what the
  mesher and later lighting need: which part of each face of the cell it covers (`face_cover`, 64 bits per face), the
  rectangles of it that show (`exposed`), and which faces of the cell are connected through it (`FaceConnections`, a 6 x 6
  matrix: the open faces a flood fill may cross). `Shape::full/empty/half/stair/from_boxes`. A `BlockDefinition` has a
  colour, whether it is see-through, a light emission and the shape of each of its states; `BlockTable` turns the
  definitions of a game's block registry into what every consumer asks of a (block, state): shape, solidity, opacity, what
  hides what, colour. An unknown block (from a plugin that is not loaded) is an opaque magenta cube; a state the block does
  not have is state 0.
- `voxel_meshing`: `mesh_brick` takes `BrickChannels` (blocks, optional states, optional light) and a `BlockTable`. Whole
  opaque cubes are merged greedily with the key (block, light) as before; blocks that do not fill their cell, and
  see-through ones, give `DetailedQuad`s at eighths of a voxel (16 bytes packed), culled where a neighbour hides them. The
  8-byte quad carries light in the top 8 bits of its material word (block ids are 24 bits).
- `renderer`: a second pipeline draws detailed quads; both shade by the light; `RenderStatistics::detailed_quads_drawn`.
  `upload_brick` takes a `PackedBrickMesh`.
- `world_view`: `ViewChannels { blocks, states, light }` and an `Arc<BlockTable>` are what a mesher is built from;
  queries `state_at`, `is_solid_at` (something to stand on and aim at) and `is_opaque_at` (hides what is behind it).
- `game_shell`: `GameContent::view_channels` and `block_table` replace the material channel, palette and opacity table.
- `base_blocks`: the channel `base_blocks:block_state`, and seven shapes for stone, dirt and grass (whole, bottom slab,
  top slab, stairs rising east, west, north, south); the terrain generator is unchanged (its pinned digests still pass).
- Voxunami Cave: the server accepts `SetBlock` with a state and refuses a state the block does not have (air has only 0),
  writing both channels; R cycles the shape of the block you place; the status line names it. A world file written
  before states exists loads with every voxel in state 0 (tested). Voxunami 2048 uses plain cube definitions; its
  unbounded numbers through the state channel come with the 2048 tools step.
- Limits, so they are not forgotten: see-through blocks are drawn in their opaque colour (no blending), and hide only faces
  of the same block; a sealed hollow between shaped blocks (two stacked slabs) still draws the faces inside it; aiming uses
  whole voxels, not the shape; no game has a light channel yet, so light was only exercised with test and spike fields.

Phase 6 (levels and commands; protocol version 2):
- `WorldData` is a list of levels (`LevelData`: terrain assignments and edited bricks), the first being the main level.
  `LevelIndex` (in `voxel_space`) numbers them. The file has an `engine:levels` section; files written before levels
  load as one level named by the game (`LoadContext::legacy_level`) and are saved in the new form. `add_level` refuses a
  level with other channels or generator versions, or a name already used.
- The protocol names a level in `SetInterest`, `InterestChanged`, `BrickContents` and `VoxelEdits`, and adds `Command`
  and `CommandReply`. A client watches one region of one level at a time; moving to another level sends the whole new
  region. The server tracks edits, generated bricks and audiences per level (`Audience::Watching(level, brick)`).
  Recordings of protocol 1 cannot be played.
- `GameLogic::command_received` and `ServerWorld::reply`; `server::CommandLine` takes a command apart. Replies reach
  only the sender, after the tick's game messages. Games use `PoseBook::handle_command` for `/level`.
- The client keeps every level's data and forgets other levels' bricks when the watched level changes; the session
  re-meshes from scratch for the new level (`WorldMesher::reset`) and draws only players in its level. In a window, `/` or
  Enter opens a command line shown in the title; Enter sends it, Esc drops it; answers show in the title and print.
- Cave and 2048 act in the level the player is in. 2048's moving blocks belong to a level and are saved by level name.
  Neither game makes a second level yet; tests do.

Phase 5 (what the earlier phases left):
- Other players are drawn: `renderer::Avatar` (a head over a body in one of 16 colours, taken from the last 16 palette
  values, so games have 240), `Session::other_players` (everyone but this player) and `view::show_other_players`.
  Avatars go through the same pipeline as bricks and are placed exactly like them, so they are as precise far from the
  origin. They do not turn to face anywhere, and they are one voxel wide.
- The server keeps the last 128 generated bricks (`generated_cache`), so game logic reading voxels of terrain nobody has
  edited no longer regenerates a brick per read. A generated brick never changes once its region has a generator.
- `engine:generators` is version 2: one number per leaf, with positions implied by the tiling
  (`region_octree::tile_from_levels`). A world with no edits went from about 40 KB to 3.3 KB at view radius 8. Version 1
  files still load and are saved as version 2.

Phase 4 (infinite 2048, replacing the 4 x 4 x 4 board of Phase 2):
- `games/voxunami_2048` is now: `material` (stone, ice, spring ice and what each does when pushed), `numbered_block` (a
  material and a power of two, 60 block ids), `motion` (the rules of moving and merging blocks, on a `BlockField` so they
  run on plain maps in tests), `landscape` (the frozen generator `voxunami_2048:numbered_hills_v1`, pinned), `content`,
  `messages` (push a block; progress), `logic` (the server: reach check, pushes, one step per tick, score, saved state)
  and `game` (the tools; the title says which block you look at). The 4 x 4 x 4 board, its rules and its push plates
  are gone, and so are the old messages and saved state, so worlds from that stand-in are not loadable.
- `PlayerPose::distance_to_voxel` (in `player_presence`) is Cave's reach check promoted, since a second game needed it.
- Measured on this machine at view radius 12: 9171 bricks held, 183 k quads drawn, 1.3 ms a frame (745 fps) on the
  integrated AMD GPU at 1280x720 and 2.9 s to load on the RTX 3090.

Phase 3 (the network):
- `transport::tcp`: `TcpConnection` and `TcpServerListener` (length-prefixed frames, never blocking, a bounded queue per
  peer, graceful close), and `ListenerSet` so one server accepts the local player and network players together.
- `Server::save` saves a running server's world with its game's state; `game_shell` gained `ServerThread` (the old
  integrated server, now with an optional network listener, autosave and join announcements), `--host`, `--join`,
  `--autosave`, and `voxunami serve GAME` for a dedicated server. Verified with real processes on this machine and with
  tests over real TCP; not yet across two machines.

Phase 2 (second game, and what it showed was shared):
- `game_shell` (engine): everything Cave had that is not Cave: the window and fly camera, the singleplayer server on a
  thread, `Session` (client, mesher, camera, aiming, sending), worlds and recordings on disk, headless screenshots and
  benchmarks, replay viewing, command-line options and help. A game implements `GameDefinition` (its name, content,
  server rules, what clicks and keys do, what its status line says) and `GameContent` (its blocks and worlds).
- `player_presence` (engine): the pose messages and the server's `PoseBook`, used by both games.
- `GameLogic::save_sections`: a game saves state the voxels do not show (a score) as plugin sections of the world file.
- `games/voxunami_2048`: a 4 x 4 x 4 board of tiles made of voxels with push plates; the first version, replaced in
  Phase 4. It showed that a game's state belongs in the voxels, with only what they do not show saved as a section.
- `voxunami_cave` shrank to its blocks, its one message, its rules and its clicks.

Phase 1f (first game):
- `world_view`: `WorldMesher` keeps meshes of the bricks around a viewer up to date on worker threads, reading from a
  client's world (generating unedited bricks from the generator the world assigns, taking edited ones as they are,
  re-meshing a changed brick and its neighbours, discarding stale results); `cast_ray` finds the voxel under the
  crosshair. It replaces the old viewer's streaming code.
- `plugins/base_blocks`: air, stone, dirt, grass, and the frozen generator `base_blocks:rolling_hills_v1` (pinned).
- `games/voxunami_cave`: `CaveContent` (plugins, new and loaded worlds), the server's `CaveLogic` (block changes within
  reach of the player's last pose; everyone told where everyone is), `IntegratedServer` (the server on its own thread at
  20 ticks per second: what singleplayer is), `Session` (a player's side without a window), a window with a fly camera,
  mouse capture, crosshair, break and place, and headless screenshots and benchmarks. `--world FILE` saves and loads the
  world, `--record FILE` records the session; replays play in real time from the recorded player's point of view, or
  render one tick to an image.
- `launcher` (`voxunami`): lists games, starts one by name, and opens a recording with the game its welcome names.
- `renderer` gained an opt-in crosshair. `replay::read_welcome` reads which game a recording belongs to.
- `tools/terrain_viewer` is gone: the game does everything it did (`--screenshot`, `--benchmark`, `--adapter`), through
  the same client path a networked player uses.

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

Light in the quad (Windows 10 machine, release, one thread; `docs/light-spike.md`): meshing a 416 x 416 x 256 voxel patch
of rolling hills with a light channel takes 2 to 8 percent longer than without one; quads grow 0 to 25 percent for
Minecraft-style light and 62 percent for 256 smooth levels around very dense lamps.

Rendering and loading (Ryzen 7600X; 1280x720; default rolling hills, 1 m voxels; offscreen, GPU time included):
- Integrated GPU (AMD Radeon, Vulkan): 6.9 ms per frame (146 fps) at view radius 12 (1.4 M quads drawn of 5.5 M
  resident) and 2.4 ms (416 fps) at radius 8. The target was 30 fps. The windowed viewer ran at 359 fps on it.
- RTX 3090 (Vulkan): 1.7 ms (600 fps) at radius 12; 0.6 ms at radius 8. An earlier run of the same benchmark
  averaged 4.5 ms with a 26 ms 95th percentile and did not reproduce in three repeats (probably the GPU clock ramping).
- Loading radius 12 (9171 bricks generated, 3366 meshed) took 4.2 s on 11 worker threads, so loading, not drawing, is
  the cost to attack next. Quad counts are high: 5.5 M quads (44 MB) at radius 12, from 1-voxel detail and caves.
- Memory per quad is 8 bytes; indexed triangles would be about 56.
- Through the game, which gets its world over the message stream (1f): the same scene gives 1.7 ms per frame on the
  RTX 3090 and 6.8 ms (148 fps) on the integrated GPU at radius 12, and loads in 4.0 s, so the stream added nothing
  measurable. The window ran at 60 fps (vsync) while recording.
- Only the 7600X's integrated GPU was measured; a weaker minimum-spec GPU is untested.

Cross-platform determinism: all tests, including the pinned terrain digests and the pinned recording of a standard
multiplayer session, pass on Linux (Ubuntu in WSL2, rustc 1.99.0) as well as Windows. Same CPU architecture and compiler version, so this does not cover other architectures.

Across machines (Phase 3): a Linux server (Ubuntu in WSL2) and a Windows client in separate processes, over TCP, drew a
picture byte-identical to a Windows-only run of the same seed: the client regenerates terrain from the seed and the
server's generator assignments, and both operating systems produce the same voxels. Only terrain was exercised (no
edits), on one physical machine; a real two-computer network test is still to do.

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
- Keyboard controls in a window (reported by the owner, 2026-10-16): in both games the keys type into the title bar instead
  of moving the player. Not yet investigated.
- 2048 shows numbers by colour only (the renderer draws no text; the title names the block you look at), and only
  stone, ice and spring ice exist. Materials that pass momentum on, other tools (a line push, an undo), and players
  being in the way of blocks are not built. The play controls were never tried with a real mouse and keyboard.
- Levels: a world cannot gain a level while a server runs, no game ships a second level yet, and `add_level` cannot check
  that a level's generators were built from the world's seed (a mismatch shows up as clients going out of sync). Typing
  commands in the window was written but never tried with a real keyboard.
- The game is a fly camera: no gravity, collision or physics yet (the Rapier adapter comes with structures), other
  players are drawn as a plain pillar that does not face where they look, and there is no inventory.
- Network scale: a spatial index over watched regions (an edit currently scans all clients); passwords or at least a
  shared secret, and encryption, before play over the internet; resending single bricks to repair a client that fell out of sync; replay keyframes for seeking; interest with a
  level of detail. See "Known limits" in `docs/protocol-and-replay.md`.
- The server's cache of generated bricks is 128 bricks, chosen without measuring; it matters when logic reads many
  different bricks every tick (large moving structures, simulation across a wide area).
- Which replicated entity state (players, structures) is common to all games; it travels as game messages until a second
  game shows what to promote.
- Active-voxel budget per tick for Game 4; set empirically once simulation exists.
- Generator boundaries: hard cuts, or may a new generator blend toward its neighbour? Who can trigger regeneration?
- How joints are authored, how merging aligns rotations (snap to 90 degrees?), and what happens on overlap.
- How "sky light" is computed from open space.
- Terrain performance: skip sampling known-uniform columns, reuse planar noise across a column, and design caves so
  bounds can classify more bricks.
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
- 2 Second game (Voxunami 2048) and the shared shell. **Done.**
- 3 The network: TCP, hosting, joining, dedicated servers. **Done.**
- 4 Infinite 2048: numbered landscape, pushing, coasting, merging. **Done.**
- 5 Other players drawn, server brick cache, compact generator tree. **Done.**
- 6 Levels (protocol 2, file format, server, client, session) and a text command channel with `/level`. **Done.**
- 7 Block states and shapes in block definitions, detailed quads, light in the quad (with its spike). **Done.**
- 8 Numbers as block states in 2048 (colours by state in the engine, generator v2). **Done.**
- 9 Tools in 2048: the push names a tool, and the tool decides how blocks combine. **Done.**
- 1f Launcher and first game crate: `world_view`, the `base_blocks` plugin, Voxunami Cave, the launcher; the old viewer
  retired. **Done.**
The physics spike is done (`docs/physics-spike.md`); building the physics adapter belongs with the structures work.
