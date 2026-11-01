# Design decisions

What the project has decided about its shape, and the rules that hold everywhere: the **standing form**, short, so that it can be read
before architectural work. `docs/README.md` is the map of all the documents. The story of how each rule came to be (the phase that
built it, the numbers measured, what was found on the way) is kept whole in `history/decision-diary.md`, which is the older text of
this document and is not maintained; the specs in `specs/` hold the designs, and `history/build-log.md` what each phase built.

An entry here says **what is decided, by whom, and where to read more**, in about eighty words. A number that is also in the code
belongs to the code and its tests; a narrative belongs to the diary. A decision that changes is changed here, in the same commit.

Status key (the same in every document): **Decided** = the project owner said so. **Proposed** = Claude's recommendation, not yet
confirmed. **Measured** = verified by a throwaway script on the dev machine. **Open** = unresolved. "Mine, the owner's to overrule" =
a choice Claude made that the owner has not looked at.

## Decisions by area
- The world and the sky (light, levels of detail, planets, orbits, rivers, the sun, coordinates): `decisions/world-and-sky.md`.
- Players and ships (entities, identity, ships, aboard): `decisions/players-and-ships.md`.
- Joints between the cars of a ship (words, the dev ball joint, physics; nothing built yet): `decisions/joints-and-cars.md`.
- Plugins, messages, the client and the user interface: `decisions/plugins-and-clients.md`.
- Data packs and the content package: `decisions/packs-and-content.md`.

## Games (one monorepo, one shared launcher)
The engine is **Voxunami**.
1. **Voxunami Cave**: Minecraft-like. Infinite in 3 dimensions, complex tick-based logic circuits, many LOD levels,
   gravity shifts, free-moving voxel structures. Voxels are 1 m cubes. The current demo's caverns suit it; other
   terrain types should be added later. Textured blocks near the player, untextured blocks beyond a distance. Flood-fill
   lighting with 256 levels (see Voxunami Cave lighting below).
2. **Voxunami 2048**: 3D 2048, pushing tiles with tools. Small and bounded.
3. **Voxunami Eels** (first called Flatworms; renamed by the owner, 2026-10-16): 3D Tron with flat voxel eels that
   barrel-roll. Built: Phase 17.
4. **Voxunami Sculpt**: tiny-voxel sculpting world: 1/32 m cubes, infinite and procedural, simulated water, swaying
   transparent leaves, gears / magnetism / gravity / friction heating / heat transfer / combustion, free-moving structures.

Every game gets: multiplayer, replay recording to video, lossless save round-trip with missing plugins.

## Decided
- Plugins are an internal modularity tool, not an end-user mod system. (Revised 2026-10-16 and 2026-10-17: mods for
  players are now planned, see `plan.md`, "Mods for players" and "Plugin shape revisions". Until they are built,
  plugins are compile-time crates.)
- Voxels are strictly cubic. Real-time rendering is rasterized; a path-traced offline renderer for replay export comes later.
- The grid is always cubic (owner, 2026-10-16), apart from coarser levels of detail in the distance. A block may fill only
  part of its cell, using a block shape chosen by the block's *state*, so that shapes do not multiply the number of block
  types and run into the renderer's palette cap (256 material values, 16 of them used by avatars). So a voxel carries
  a block and a state, as two channels (built; see `build-log.md`, Phase 7). A game without states leaves the second channel out.
- Worlds have several levels (built; see `build-log.md`, Phase 6). Each level has its own grid, terrain assignments and edited
  bricks. All levels of a world share its channels, content registries, terrain generator versions and **seed**: a level
  differs from another by which generator versions it uses and by what players have changed, since a client regenerates
  terrain from the world's seed and has no other way to learn a level's. The first level is the main level, where players
  start. Levels are named by identifier in files and in game messages, and by their position (`LevelIndex`) between a
  server and its clients. A world's levels are fixed while a server runs.
- Which level a player is in is the server's to say (the root grid of their player entity, `player_presence::Presence`); players say only where they are in it. A
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
- Ships (airships, spacecraft; Phase 32): the decisions are in `decisions/players-and-ships.md`.

## Decided (rules for lattice simulation and physics)
- Tick phases (confirmed by the owner): each tick reads a frozen snapshot of the previous tick and produces the next one.
  A brick update never writes to a neighbour; cross-brick effects are queued and applied together in a fixed order.
  Lattice rules (heat, fluid flow) lose nothing; chain reactions within one tick (instant wire propagation, cascading
  updates) must be rewritten, for example by solving a connected wire network as one graph. Safety net: a
  partition-invariance test that shifts a scene by 1-31 voxels and requires identical results. Entities (such as 2048's
  moving blocks) are not lattice cells and are processed one at a time in a fixed order; see "Decided (2048)".
- The shape two rules have in common (light and fire; the `lattice` crate): a rule keeps its voxels still to be looked at in `lattice::ActiveVoxels`; a tick takes them, works out what each does from
  the world as it stands, and only then writes, so the order and the bricks never matter, and what was written wakes the voxels around it. Chance is `lattice::voxel_random(seed, clock, level,
  voxel, purpose)`, with the rule's own clock saved with the world (a server's tick starts again at 0). Limits on work are per rule (light: a budget of voxels a tick; fire: only fires within range
  of a player). Not extracted: a trait for a rule, since two rules do not show what is common; sampling random voxels in an active region (coral, growing grass) has no home yet.
- Physics (confirmed by the owner): Rapier (rapier3d 0.36.1) behind a Voxunami adapter crate, with
  `enhanced-determinism`, single-threaded stepping on the server, a local floating-origin frame, large merged voxel
  colliders for terrain, and gravity, buoyancy and drag applied as per-body forces. Evidence and caveats are in
  `docs/history/physics-spike.md`. Built so far for players only (Phases 15 and 16, below); rigid structures, buoyancy and
  joints are not.
- Players' bodies (decided with the physics, Phases 15 and 16): the server moves them. A client says which keys are
  held and which way it faces; it never says where it is, and a game that moves players ignores pose messages. This is what
  keeps recordings and every other player's view honest, and what makes the movement the same on every machine.
  Prediction on the client is not built (see `plan.md`, "Open").

## Decided (2048)
- The decisions of the game itself (numbers, materials, tools, chain pushing, moving blocks as entities) are in `specs/2048-spec.md`.

## Foundations (proposed 2026-10-16 and built on since; not separately confirmed by the owner)
- Rust, wgpu, mesh-based rendering. Levels of detail are chosen by one octree refinement rule by distance for terrain and every grid alike, not by nested shells
  (owner, 2026-10-23, `lod-spec.md`; this replaces the earlier "nested clipmap LOD shells", and "GPU-driven" rendering is built only if measurement shows the CPU is the limit).
- Everything with voxels is a `Grid`: sparse, chunked, with a rigid transform. Terrain is the infinite root grid
  (procedural base plus saved overrides); structures, creatures and tiles are finite grids.
- Voxel coordinates are `i128` per axis within +-2^120 (bricks within +-2^115); rendering is camera-relative. (They were `i64` within +-2^60 until 2026-10-27; see "Coordinates are 128 bits" below.)
- A saved brick is stored whole and replaces the generated one, only when its content differs from generated.
- Server sends per-brick content hashes so a client whose generated terrain differs notices and requests the data.
- Generator version assignment: the first time a region is generated it is assigned the world's current default
  version, in whole aligned cubes of a per-game size (larger cubes mean fewer boundary nodes). Released versions are
  frozen; a missing generator plugin loses only unmodified terrain.
- Entity gravity direction is server-authoritative replicated state, computed from coarse LOD mass, smoothed per entity.
- Network stream (`docs/reference/protocol-and-replay.md`): clients tell the server a box of bricks to watch; the server sends
  which generator made each part and every *edited* brick in it, then each tick the changes inside it. Terrain nobody
  has edited is never sent. A brick's first edit is sent as the whole brick, later edits as change lists with a digest of
  the result. Game and plugin messages ride in a generic message that names its owner by identifier.
- Replay recording is the tick-by-tick stream one client received, appended to a checksummed file as it arrives; playing
  it back is a connection that plays the server, so the client code is unchanged.
- Plugins are compile-time crates. The launcher links the games and starts one by command name (revised in 1f from
  "one process per game from a manifest": simpler, and nothing needs it yet).
- Brick size 32^3 voxels (rows fit a u32 bitmask; 1 m at Game 4 scale). Revisit when the mesher exists.
