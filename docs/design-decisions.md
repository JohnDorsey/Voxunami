# Design decisions

What the project has decided about its shape, and the rules that hold everywhere. The other documents:
`plan.md` (the build plan, what is proposed, what is open), `future-goals.md` (owner wishes that are not scheduled, with their
difficulty), `build-log.md` (what each phase built, and measurements; history, read the part you need),
`plugin-architecture-review.md` (the reasoning behind the plugin shape revisions), and the specifications
`world-file-format.md` and `protocol-and-replay.md`.

Status key (the same in every document): **Decided** = the project owner said so. **Proposed** = Claude's recommendation, not yet
confirmed. **Measured** = verified by a throwaway script on the dev machine. **Open** = unresolved.

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
- Structures such as airships (owner, 2026-10-18; the feature is in `future-goals.md`, goal 2, and scheduled as Phase 32 in `plan.md`):
  - Gravity aboard is the world's gravity, not a pull toward the ship's floor. A rule asking for gravity gets the world's, in the
    grid's local axes, so it changes as the ship turns; a player aboard a ship rolled on its side falls toward the world's down.
  - Ships may rotate fully (not only about the vertical axis).
  - Ships have docking connectors with a configurable attraction force, as in Kerbal Space Program: an engine-provided part that a mod
    configures.
  - There is no hard limit on the size of a ship. Someday ships may be as large as small moons, with levels of detail used when the
    whole ship is in view and for collisions. Limits are budgets (work per tick, as for every rule), not caps.
  - A ship can be saved while in flight, as a grid, in a file of its own, not in the file that holds the terrain near it. A ship file
    is portable: it can be loaded in a different world.
  - Docking merges: when two ships dock they become one rigid body and one ship (one grid). Anything passes the join: crew, items,
    fire, light, and the interaction of any blocks that interact with their neighbours, as if the two had been one from the start.
    It is not only the connector blocks that join: any blocks that become adjacent are adjacent. If solid blocks flush with the ports
    touch when the ships dock, they are welded, and the ships cannot undock until the player breaks the blocks that join them.
  - A player who connects to a server and whose ship file cannot be found is asked what to do: spawn at the world spawn, spawn in
    midair, or quit to the title screen.
  - Undecided: whether a ship's own mass is a source of gravity.
  - Order of work: moving grids (Phase 32) come before the native and WASM loaders (Phase 33), because the loader ABI encodes the
    coordinate model.

## Voxunami Cave lighting (decided; block light built in Phases 11 and 12, sky light not)
- Built so far: one channel of block light, from lamps (see `build-log.md`, Phases 11 and 12). The rest of this section is the
  plan for sky light, and for how the two combine.
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

## Decided (the plugin host; built in Phase 23)
- A game's server rules are plugins run by `plugin_host::PluginHost` (the specification and its record are in `plugin-api-spec.md`). What a plugin
  registers, and when: systems in the phases `Decide`, `Controllers`, `Physics`, `Settle` and `Rules`; handlers of events, delivered first in first
  out right after the system or handler that caused them returns; handlers of client messages and commands; saved state.
- **The order of the plugin list never changes the result.** Plugins are registered by dependency then identifier, systems and handlers are ordered
  by the `after`/`before` they declare (ties by plugin identifier, then their own), saved sections are written sorted by identifier, and nothing
  runs in parallel. Each game's golden session (its recorded stream and saved world, hashed and pinned) is run with the plugin list as listed, reversed and
  rotated.
- A system's reads and writes of channels are declared and checked (in debug builds); two systems that use one channel, one writing it, with no order between
  them are refused when the host starts. A `Mind` system cannot write the world, only emit intents. A rule reads and writes one grid at a time, by `GridId`
  and the grid's own coordinates.

## Decided (rules for lattice simulation and physics)
- Tick phases (confirmed by the owner): each tick reads a frozen snapshot of the previous tick and produces the next one.
  A brick update never writes to a neighbour; cross-brick effects are queued and applied together in a fixed order.
  Lattice rules (heat, fluid flow) lose nothing; chain reactions within one tick (instant wire propagation, cascading
  updates) must be rewritten, for example by solving a connected wire network as one graph. Safety net: a
  partition-invariance test that shifts a scene by 1-31 voxels and requires identical results. Entities (such as 2048's
  moving blocks) are not lattice cells and are processed one at a time in a fixed order; see "Decided (2048)".
- The shape two rules have in common (built after the second, as planned: light and fire; the `lattice` crate and
  Phases 13 and 14). A rule keeps its voxels still to be looked at in `lattice::ActiveVoxels` (ordered by level then
  position, savable by level name). A tick takes them, works out what each does from the world as it stands, and only then
  writes: so the order and the bricks never matter, and what was written wakes the voxels around it for the next tick. Chance
  is `lattice::voxel_random(seed, clock, level, voxel, purpose)`, pinned against an independent computation, with the
  rule's own clock saved with the world (a server's tick starts again at 0 on every start, so it cannot make a saved
  world carry on exactly). Limits on work are per rule: light takes a budget of voxels per tick (the rest wait in order),
  fire looks only at fires within range of a player in their level (the rest are frozen). Not extracted, because two rules do
  not show what is common: a trait for a rule, since light writes one channel and fire writes two and wakes differently.
  What still has no home is the sampling of random voxels in an active region (for coral and growing grass, which are not
  made of blocks that tick themselves); fire did not need it.
- Physics (confirmed by the owner): Rapier (rapier3d 0.36.1) behind a Voxunami adapter crate, with
  `enhanced-determinism`, single-threaded stepping on the server, a local floating-origin frame, large merged voxel
  colliders for terrain, and gravity, buoyancy and drag applied as per-body forces. Evidence and caveats are in
  `docs/physics-spike.md`. Built so far for players only (Phases 15 and 16, below); rigid structures, buoyancy and
  joints are not.
- Players' bodies (decided with the physics, Phases 15 and 16): the server moves them. A client says which keys are
  held and which way it faces; it never says where it is, and a game that moves players ignores pose messages. This is what
  keeps recordings and every other player's view honest, and what makes the movement the same on every machine.
  Prediction on the client is not built (see `plan.md`, "Open").

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
- Chain pushing (built: Phase 10; the owner asked for it "eventually", and the details are mine, easy to change): a moving
  block that meets a block its tool does not combine it with pushes it if its material pushes (stone does; ice stops and
  spring ice bounces, as the owner said they do). The block and the row of blocks behind it move one step ahead, if the
  row, with the pushing block, is at most 12 blocks (`PUSH_LIMIT`) and ends in empty space; otherwise nothing moves and
  the pushing block stops. Pushed blocks move that one step and do not coast; one that was already moving keeps moving
  from its new place. The add and subtract tools combine with any block they meet, so they never push; the double tool
  pushes past blocks of different numbers.
- Assumptions made while building it, easy to change: only the momentum materials (not built) will transfer motion; a push has a range (48 steps for the ices, since an
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

## Foundations (proposed 2026-10-16 and built on since; not separately confirmed by the owner)
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
