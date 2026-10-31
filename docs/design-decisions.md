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
- Structures such as airships (owner, 2026-10-18 to 22; the goals are in `future-goals.md`, goal 2; built as Phase 32; the full text is in the diary):
  - Gravity aboard is the world's gravity, in the grid's local axes (a player on a ship rolled on its side falls toward the world's down). Ships rotate
    fully. There is no hard limit on the size of a ship: limits are budgets (work per tick, as for every rule), not caps.
  - **Docking merges**: two ships that dock become one rigid body and one grid, and anything passes the join (crew, items, fire, light, the rules of any blocks
    that touch). Blocks flush with the ports are welded and the ships cannot undock until the player breaks them. The yellow face of a dev docking port
    docks with another's: snap within 10 degrees and 0.3 metres, any roll in quarter turns; the pull is on the centre of mass with the torque of the lever
    and **does not scale with mass**. The dev port is creative only. Face properties (`engine:marked_face`, `engine:mark_color`, `engine:docking_face`) are
    engine-reserved block properties, so the definition format did not change. Merging is `ServerWorld::merge_grids`, undocking `split_grid`.
  - A ship is saved while in flight, as a grid in a file of its own, portable to another world. A player whose ship file cannot be found is asked: world
    spawn, midair, or the title screen.
  - **Units are metric**: a voxel is a metre; kilograms, newtons, newton metres, seconds. Until materials have densities every block that is not air weighs
    1,000 kg a cubic metre (`engine:density`). Gravity in the overworld is 5 m/s^2 (owner, 2026-10-22).
  - **Every block has an orientation**, saved the same way for all blocks (channel `base_blocks:block_orientation`, 0 to 23, the order of
    `LatticeRotation::all()` pinned as a file format), so turning is composing numbers with no knowledge of the block, and a plugin that is not loaded is
    turned too. `BlockTable` is indexed by block, state and orientation and de-duplicates shapes. A block with a front face is turned to face the player, one
    with an axis lies along the face it was put against; the client says which face it clicked (`SetBlock.against`) and the server works out the turn.
  - A rule with chance keyed by grid does not go on bit for bit across a merge or a split, only in distribution (owner).
  - Added by the owner 2026-10-20 (`future-goals.md`, "Ship goals"): huge ships pull; ships are drawn with levels of detail far away; fluids may flow
    inside a moving grid and cross a ship's edge; a ship may vanish with its pilot's logout (a server setting); the simulation distance for ships is a
    server setting; felt acceleration and fossilization are future options. Unpiloted ships stay simulated; every part of a split and every merge gets a
    new id; a logged-out passenger returns aboard; a blocked arrival asks the pilot.
  - Order of work: moving grids (Phase 32) come before the native and WASM loaders (Phase 33), because the loader ABI encodes the coordinate model.

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
- How light reaches the renderer (decided by the spike in `docs/history/light-spike.md`, built in Phase 7): each quad carries its
  light in 8 bits of its material word, the mesher uses the light of the voxel in front of a face as part of the merge
  key, and the quad stays 8 bytes. Measured on rolling hills with made-up lamp light: meshing time +2 to +8 percent, quad
  count +25 percent for Minecraft-style 16 levels around very dense lamps, +62 percent for 256 smooth levels, none for
  uniform light. Rounding light to fewer bands in the mesher is the lever if a scene gets heavy (not built); a light
  texture is the later route to smooth shading and would not change storage or the protocol.

## Decided (levels of detail; Phase 38, owner 2026-10-23; specified in `lod-spec.md`)
- One level-of-detail system for terrain and every grid, ships included, so that the ground can one day be a ship (a planet: a rough sphere of cubes in ordinary
  space, no curved grids). It must scale to planets of 10^8 m and more; the maximum detail level is **116** (a cell of 2^116 voxels; it was 40 until the coordinates were 128 bits, see "Coarse levels go to the root"). Detail levels are counted in voxels; a game has one voxel size.
- One grid shows several detail levels at once, by the distance of each part; the rule is an octree refinement by distance (cell size against distance and pixel size).
- A coarse cell stores the dominant block (chosen among the blocks that touch air, so a layer of snow shows), a fill fraction and a light level (brightest child up to
  32 voxels, an exposure-weighted average above). Coarse cells are untextured always; a client setting gives the distance within which blocks are textured.
- Coarse data is hybrid: the client derives what the generator gives, the server sends it where there are edits and for ships. It is drawing only (no rule reads it) and is
  never saved. A ship can cap how coarsely it is drawn (a detail level); the cap does not keep a ship from disappearing when it is under a pixel.
- **Planets are ships** (owner, 2026-10-23): a ship can store terrain generator versions (with a declared extent) and send them to the client, which generates the unmodified bricks itself, as for a level; edits are stored bricks over the generated ones. Built in
  Phase 39; Phase 38 keys everything by grid and treats the source of unedited cells as per-grid and optional, so that it fits (`lod-spec.md` 3.8).
- Coarse nodes reach a client as plugin messages (`engine:set_detail`, `engine:coarse_nodes`), so the protocol does not change and recordings carry them. Client and server work out which cubes are drawn with one function from the brick the viewer is in
  (taken at its middle); bricks are drawn inside a ball round it (plus the cubes of two that touch it) and nodes outside, exchanged in groups so that nothing is drawn twice or dropped (`lod-spec.md` 3.5, 3.6a). Cave does not draw coarse terrain until the
  generators' coarse cells are fast enough (`lod-spec.md` section 9).
- **Cubes that hold nothing are not drawn** (2026-10-24, `lod-spec.md` 9b): a cube of air all through, and a cube of ground all through with ground all round it (within one cube edge), is left out of what the client draws, with everything inside it. A generator says so
  (`TerrainGenerator::may_show`, from conservative bounds, never from samples; a generator that cannot say says yes). Nothing is saved from it, but the picture depends on it, so it follows the hard rules for generators: IEEE operations only, and a released generator's answer is not
  changed. A cube with an edit in it or beside it (`EditedCubes`, which the client keeps from the overrides it is sent) and every cube that has a brick of the fine region in it are never left out: a player can build in the air and dig in the ground. The server sends
  overrides and nothing else, so it follows the selection only to the cubes that have one and does not ask the terrain.
- Rendering uses reverse-Z with an infinite far plane. Orientations are `f64`. GPU-driven culling is built only if measurement says the CPU is the limit.

## Decided (planets, orbits, the universe and the sky; Phases 38 to 41, owner and me, 2026-10-23 to 28)
Specs: `lod-spec.md`, `planet-spec.md`, `rails-and-recipes-spec.md`, `great-rivers-spec.md`, `places-spec.md`, `sun-spec.md`, `shadow-spec.md`. The full text of each entry,
with its numbers and what was found, is in `history/decision-diary.md`. Entries marked *mine* are choices Claude made, the owner's to overrule.

**Planets**
- The first planet is a ship with a terrain generator, 1% of the Earth's radius (63,710 voxels), in the level `mini_cosmos`, which a new world has first and the
  overworld second (`/level go overworld`). Its outside is a cube-sphere of heightmaps blended where they meet; caves are in the bricks, not in the levels of
  detail; surface gravity 10 m/s^2 (owner). Layers are read radially from the depth. Its output and its coarse nodes are file format like every generator's.
- **A planet is a kind of ship with slightly different rules, sharing most of the code; all planets will be on rails; planets are never docked to each other or to a
  moon; a ship that docks with a planet is written into the planet's grid and the planet's id does not change** (owner, 2026-10-25). A planet is never merged
  or split, and cannot be held, pushed, piloted or told to vanish. A ship docked to a planet can be undocked (the side not on the ground leaves).
- *Mine:* a planet weighs what its terrain declares (the pull at its surface fixes the mass, M = g R^2 / G) and has no body in `grid_bodies`; **a level's pull is
  the level's own** and **only a ship over `pull_mass_threshold` pulls** (Cave: 10^19 kg). Its body for a walker is fixed in the frame of its level with
  colliders only near a free ship, from the stored brick or else the generator, and not at all farther than 4,096 bricks from the frame's origin (`f32`).
- *Mine:* a planet is known from anywhere in its level, drawn in detail by the same view that draws a level, and its nodes are the generator's, not
  corrected by the server (an edit shows in the bricks near the viewer). A generator may give a solid core and a view leaves out cubes hidden behind it.
- *Mine:* a new world is space first and the overworld second, with the planet made with it (2,100 bricks each way, held); the world the tests of the
  overworld start in is the overworld alone; an empty level is a generator; a new player stands at the pole of the planet; a level has a place players
  arrive at; a planet is always in memory.
- Cave is to become a space-exploration game (KSP, KSA); ships will go on rails far from players or in a time warp (`future-goals.md`, unscheduled). Until
  then simulation of a ship stops beyond a server-set distance of the nearest player (`/ship distance`) (owner, 2026-10-25); the bodies of a level are in a
  frame for each place, not one around the first ship.

**Data and rails**
- **A terrain generator may be data: a recipe is (instance name, family, parameters as bytes).** The family is code and a file format; the world keeps its
  recipes (`engine:generator_recipes`) and its description carries them; a recipe whose family is missing leaves a placeholder and is kept byte for byte.
- **A planet is a body: a file of a pack (`bodies/NAME.json`)** naming a level, a place, a family with its parameters, the extent of the ground and its weight.
  The family has a data side that reads parameters and makes their JSON Schema; the game's own planet is a body of the game's own data; a world takes its
  bodies when it is made, never when loaded; a pack whose body cannot be made is refused at load.
- **A rail is part of a ship (a spin and/or a circular orbit) and a pure function of the world's clock** (`libm` in `f64`, pinned to the bit), so nothing is integrated
  and nothing drifts. The engine puts ships on their rails in order of their numbers, so a moon follows its planet. A body on a rail is in free fall with what it
  goes round. A rail is a function of whole ticks (`RailTime`), and an orbit's offset is worked out in 128-bit fixed point (`ExactOffset`) and kept as bricks and a rest.
  Limits: a radius of at most 2^120 voxels, a period of at most 2^80 seconds, a phase of at most 2^63 radians.
- **A planet on a rail has a sphere of influence** (one and a half times the reach of its ground, at most 120,000 voxels): everything in the level inside it goes with the
  planet, moved by the planet's displacement of the tick before anything looks at the world; ships in it are simulated in a frame that goes with it, and a planet on a
  rail is a kinematic body. The prices (a ship made at rest near a moving planet is going at its speed against it, and so on) are in the diary.
- *Mine:* a star is a body like a planet, a ball of ground of the planet family. Open: bodies are made in the order of their files' names, which makes names such as
  `star_planet_moon` a rule of the pack author; the order should come from the orbits.

**Coordinates and levels of detail**
- A coordinate of more than 64 bits goes on the wire as a length-prefixed integer (a byte for the number of bytes, then the zigzag value); a number has one encoding (owner, 2026-10-27).
- **Coordinates are 128 bits** (owner, 2026-10-27): `VoxelCoordinate` and `BrickCoordinate` are `i128` within +-2^120 voxels, `WorldPosition` an `i128` brick and an `f32` offset,
  the octree keys `u128`, and the octree's root level 116. Saves: a clean break (readers of older versions were deleted; a section of another version is refused, or kept
  as it is if a game's). Terrain stays the same to the bit. **Speed is a requirement: any new per-voxel loop checks once whether the position is within 2^62 and then
  works in 64 bits** (the first version made terrain 2.2 times slower). Squares of distances can overflow `i128` across the range and are compared exactly after a check
  per axis. Still limits: a noise octave's wavelength is at most 2^56, and a turned grid is exact only within 2^53 voxels of its own origin.
- **Coarse levels go to the root** (owner, 2026-10-27): `MAXIMUM_DETAIL_LEVEL`, `LevelOfDetail::MAXIMUM_LEVEL` and the octree's `ROOT_LEVEL` are 116 (a compile-time assertion
  keeps them the same). No generator makes anything above level 40 yet, so the levels above are checked by scale with synthetic fields; seeing everything at every scale
  is about 2.2 million nodes, which is why a view has a maximum distance. Open: the renderer's depth range at these distances (`future-goals.md`).
- **A game says how far each level is drawn** (`GameDefinition::level_detail`): Cave draws the level of space in nodes to 2 x 10^27 voxels and the overworld in bricks only (its nodes cost
  60 to 375 ms each). Not done: the server's correction of far nodes after an edit.

**The great rivers and places**
- **The great rivers** (owner, 2026-10-27): one river across the universe in `mini_cosmos`, of one block (`base_blocks:great_river_water`, passable, never flows), a billion light years
  across at the widest and 80 metres at the narrowest, never within 4.5 billion light years of the first planet. *Mine:* a path of knots joined by courses of a small whole direction
  (at most 5) with a radius that changes as a power of two, so a tube 80 metres across is sharp 4 x 10^26 voxels out with `i128` and `f64` only; it is a `DensityField` with sound
  bounds; the generator is `voxunami_cave:great_rivers_v1`. Known, not decided: a thin river vanishes from the coarse nodes above about level 5.
- **`/tp` is for several planets** (owner, 2026-10-27): names are namespaced; a body is reached on the ground nearest to the player; two numbers after a name are a latitude and a
  longitude and three are x, y and z of the body; everyone may use it for now. *Mine:* a body is a place named as the generator of its ground; latitude and longitude turn with
  the body (north is its +y); a place in another level takes the player there; `/tp X Y Z` stays and is exact for any whole number.
- **Text wraps, and coordinates are hexadecimal** (owner, 2026-10-28): wrapping is in `ui::layout`, the only place that knows the width.

**The sky** (owner's "Lighting please" and "Continue", 2026-10-28; all opt-in, so a game that says nothing is drawn as it was)
- **The sun**: a sun per level (`GameDefinition::sun`), a function of the world's clock, which the server sends in the welcome (protocol 10) and the client counts, so everyone has the
  same sun and a recording has the day it was made in; nothing that depends on time reads any other clock. The renderer lights each pixel by `sun_light * cos * visible`
  and a sky term. The rule is written twice (WGSL and Rust) and held together by tests on every adapter; the integrated AMD GPU in DX12 returns no number for `asin(1.0)`, so the shader never asks.
- **Shadows** are cascaded shadow maps with nothing in them that assumes a size of world; only faces that look away from the sun are drawn into a map; a map is kept while the
  camera and sun have barely moved; `.shadows` and `--shadows` turn them off.
- **The sky's cover** (four more cascades seen from above) tells how open the sky is over a place, and the sky's light is multiplied by it. Cave says 0: a cave is black by day, lit by
  lamps, with **no flooded sky light** (owner).
- **The sun is a star**: the sky is drawn from each pixel's direction after the terrain; the stars are a hash field that turns with the day; the sun is one of them, a disc with a glow;
  the night has a light that goes with the sky's cover; a sunset is warm.
- *Mine:* the sizes of the cascades, the share of the sky in a shadow, the pace of the refreshes, the day's length, the disc's size, the stars' number, the night light, the colours
  of a sunset. The graphics are paused (owner, 2026-10-29).

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

## Decided (property bags on block definitions; built in Phase 24)
- A block definition carries namespaced, typed properties; the table keeps one column per declared key, so a read in a hot rule is an index by
  model. Properties are **per state**: a value can be set for every state of a block or overridden for one state (state, else block, else the
  key's default). Kinds are bool and the integers (u8, u16, u32, i32), each range-checked when the table is built; a bad definition is an error
  there, never a surprise in play.
- The owner of a key declares it (dependency direction games to plugins to engine): `base_blocks:emission`, `base_blocks:flammability` and
  `base_blocks:passable` are the base blocks plugin's; the engine reserves `engine:density` and `engine:placeable_on_moving_grids`. A game adds its
  plugins' declarations through `PackageSpec.properties`. Rules take a `PropertyHandle` once and read it by model.
- Saved files are unchanged: properties come from definitions, not from the world.

## Decided (declared messages and commands; built in Phase 25)
- A game or plugin message is a type that implements `protocol::Message` (kind, encode, decode, samples), used by both the sending and the receiving
  end. `Registrar::message::<M>` decodes for the handler (a payload that does not decode is ignored and counted, never disconnects the sender) and
  `declares_message::<M>` lists a kind the plugin only sends. The host tests every declared message's codec on its samples (`check_messages`,
  run when a game's server is built): it round-trips, encodes deterministically, refuses trailing bytes, and a cut-short payload is refused or is
  itself a payload. The engine's own hot-path messages keep their compiled codecs. No code generation and no schema: the review judged it not
  worth it at this size.
- A command is a `CommandSpec` with a name, a summary and typed arguments (word, number, integer, identifier, rest). The host checks a call,
  answers a malformed one (`usage: /tp X Y Z; Z must be a number`), generates the usage line, and provides `/help` itself. No permission level
  until a command needs one.

## Decided (the client interface; built in Phase 26)
- The client half of a plugin is a separate `ClientPlugin` (same identifier as its server plugin) registering `on_message::<M>` (a server
  message, typed), `view` (adjusts what is drawn), `hud_line`, `key`/`keys` and `mouse` (actions) and `command` (a `.`-command, with the same typed
  arguments as server commands). State belongs to the plugin. `GameDefinition` no longer has `ClientState` or the six client hooks.
- Hooks are pure functions of the server's stream and the plugin's state, so a replay is unaffected: `on_message`, `view` and `hud_line` run in a
  replay, and actions and commands only while playing. A hook asks the server for something through its context (`send_command`, `send_message`),
  never by touching the connection.
- **Any number of actions may be bound to one key or mouse button; all run, in order of plugin identifier and then registration** (the owner's
  decision). An action has its own unique identifier; a key or button is its default binding, so that rebinding by client commands (a later
  phase) names actions. `.keys` lists them; `.ambient`, `.help` and `.keys` are the shell's own client commands, registered the same way.
- The engine does not know Cave's light: `world_view` has a neutral `BrickLight` hook (compared by identity) that turns a brick's channels into
  per-voxel brightness; the multiscale light (`ScaledLight`, `LevelFormat`, blending) is in Cave. Measured: the hook costs nothing visible
  (the per-brick time of Cave's 4-scale light, 2.3 to 2.8 ms before and after).
- HUD lines are joined into the window title until the UI toolkit of Phase 30.

## Decided (entities on the wire; built in Phase 27c)
- **Protocol versions.** (Revised in Phase 28.) A version is bumped whenever a message changes, and a program reads only its own version: no
  recording has been made and nothing is released, so nothing older is kept readable (the `OLDEST_READABLE_PROTOCOL_VERSION` constant, the
  old `Players` message and the session's fallback for it are gone). Once there is a release, keeping recordings watchable becomes a decision
  to take again. The version is in the `Welcome`, so changing it changes the stream hashes of the golden sessions and the pinned recording.
- Entities are replicated by interest: those whose grid is the root grid of the watched level and whose brick is in the watched region.
  The server sends the difference from what each client was last told (despawns, spawns, changes, in that order, in order of id), only the
  components a kind marks replicated, and a client store (`ClientEntities`) is built from the stream alone, so a replay shows the same
  entities.

## Decided (who a player is; built in Phase 27e, the owner's decision after review of 27d)
- **A keypair per install, no accounts.** The player is an Ed25519 public key, made once and kept in a file in the game's data folder (`--identity
  FILE` chooses another; the file is 32 secret bytes, never overwritten if it is not). There are no names, passwords or servers that know players.
  A client proves it holds the private key: `Hello` carries the key, the server answers `Challenge` (32 bytes), the client sends `Prove` (a
  signature of a text that names the game, the challenge and the key, so a proof is good for one login to one game). A key can play once at a
  time. Nothing on the wire is encrypted: this is proof of who, not secrecy. Moving to another machine is copying the file.
- **Challenges are random in real servers and counted where the only client is in the same process** (tests, the single-player host used by
  tests and headless runs), so recorded sessions are byte-identical. A server anyone can reach over a network must never count them.
- **A join takes two server ticks** (challenge, then welcome). A recording starts with the challenge, then the welcome; players of a replay
  prove nothing to anyone and use a throwaway identity. The golden sessions were repinned once for it; the cause was checked, not assumed (the
  old code with an idle tick before each join, against the new stream with the challenge removed and the version set back to 3, gives the same
  hashes in all three games).

## Decided (a body put inside terrain is moved out of it; found in Phase 27f by a flaky test)
- **`CharacterBody::new` and `teleport` clear the body at the next `update_terrain`**: if its box (a hair smaller than the body, 0.02 each
  way) reaches into terrain, as the collision source says voxel by voxel, it is moved the *shortest* way out: up, down, or sideways along an
  axis, in steps of an eighth of a voxel up to 64 voxels, up first at equal distance. Not "straight up": that launches a player through a roof
  or over a wall. If there is no room within 64 it stays where it was put. A voxel the world does not have counts as solid.
- **Why it is explicit**: the physics alone did not resolve it. Rapier's depenetration pushed a body some way sideways and left it inside
  blocks, and the brick colliders omit boxes buried in solid terrain, so a body deep in the ground touched nothing and fell through it.
- A body that is not inside anything is never moved (teleporting to where it already stands changes nothing, to the bit).
- Cost: only on placement, and only when inside terrain; a failed search is about a millisecond plus the generation of the bricks it looks at,
  which is why the limit is two bricks and not more.

## Decided (players are entities; built in Phase 27f)
- **A player is an entity** of kind `player_presence:player`, with the key (server only), the connection number and the look angles (told to
  clients) and `engine:hidden`. Its pose is the player's pose; its level is the root grid. A player who leaves is hidden, not removed: it is
  in the save, and its key finds it again (the player resumes where they were, and is told their level). Another key never takes it over.
- **`engine:hidden`** is an engine component: replication skips a hidden entity (and despawns it for those who knew it); the world keeps it.
- **Clients know the players they can see**, by the entity replication, not everyone: players in another level, or beyond the watched region,
  are not known. `PoseBook` and the `Players` broadcast are gone (the message itself was deleted in Phase 28).
- The look angles are kept as reported (a component) beside the orientation quaternion, so that nothing recovers angles from a quaternion.
  The calls that read or move players take the world (`Presence`), a game holds it as it held the book.

## Decided (ships a player can make and fly; built in Phase 32h; the choices are mine unless marked, the owner's to overrule)

- **Ships come from `/ship`, Cave's command, on an engine that only knows how to detach blocks into a grid** (owner, 2026-10-20: approved). The engine
  gained two small questions and no rule: `ServerWorld::generated_voxel` (what the terrain generator made at a voxel, whatever was built there) and
  `Presence::view_in_the_world` (a player's eyes and the turn of their view, in the world).
- **A ship is made of what was built, not of the ground it stands on:** `/ship create` takes the blocks that differ from the generator's, joined
  through faces to the one the player looks at (up to 4096); `all` takes every solid block, which is the first text and is nearly always too many,
  since a boat built on a beach is joined to the planet.
- **A command is about the ship you are aboard, else the nearest within 48 voxels** of the box of its bricks.
- **A ship has colliders for the terrain close to it, not for the bricks around it:** within 8 voxels of the bounds of its own colliders and five ticks of its speed, and
  they are let go 16 voxels farther out. A brick of natural terrain costs 10 to 25 ms to generate and make colliders of on the thread of the tick, and a ship of a few
  blocks occupies a whole brick, so the bricks around its bricks were 16 to 64 of them at a time and stopped the game for a quarter of a second as it came to the
  ground (found by the owner playing, 2026-10-21). A walking player is the same: terrain within 8 voxels of the character and a quarter of a second of its speed, let go
  16 voxels farther out, where it had the 27 bricks around its brick (a hitch of 100 to 125 ms at each new brick edge).
- **A ship's collision with thin ground is kept by solver steps, not by Rapier's continuous collision detection (planned as 32j; measured 2026-10-21):** a ship's whole cubes are
  one `Voxels` collider, which Rapier's CCD leaves out, so the flags do nothing. `BodyWorld::step` will use more solver steps for the tick when a body is fast (3 below about
  48 voxels a second, 24 at most), which leaves every slower scene bit for bit as it was. A floor of one voxel is gone through from about 96 voxels a second today.
- **Work a tick may do is limited by a count of work, never by time (planned as 32k):** a budget in milliseconds would make the simulation depend on the machine, against the hard
  rule on wall-clock time. What a ship needs this tick (the terrain it can reach) is never limited; what it will need soon is made nearest first, a few a tick, and the same budget
  serves the connectivity check of splitting (32l).
- **A pilot flies by force, in the physics the ship already has:** the keys never move the ship's pose. Each tick before the ships move, a force holds the
  ship up against gravity, speeds it up toward the keys, and takes a part of its speed away, so the ship's mass, its collisions and the
  other ships' pull all still apply, and a recording of the stream is a recording of a flight. The way the keys point is taken from the pilot's
  request of this tick, not from the record of their view, which is a tick behind (found: the first W of a pilot who had just looked level went up).
  The numbers (24 a second a second, doubled by control; drag 1.0 a second) are a first guess for the owner's feel.
- **Turning is the physics's:** no key turns a ship yet; a force off its middle does. The controls are for the command chair (Phase 35).

## Decided (players aboard ships; built in Phase 32g; the choices are mine unless marked, the owner's to overrule)

- **A player's record is the truth about which frame they are in and where; the body is made to agree with it.** A player is an entity whose
  `grid` is a level's root grid or a ship (owner, 2026-10-18: "a character's pose is `(frame, local)`"). Everything that asks where a player is
  (`Presence::pose_of`, `players`, `level_of`) answers in the world, so a rule never has to know about frames; what moves players uses
  `local_pose_of` and `set_pose_in`. Taking a ship away (`remove_grid`) puts what was aboard in the level, in the same call.
- **Aboard is simulated in the ship's own coordinates, with the world's gravity turned into them** (owner, 2026-10-18/20): exact carry, no
  fictitious forces. The body's box and controller stand in whatever up the field gives; the way a character faces is carried along as up turns
  (parallel transport by the shortest turn), so a ship that yaws turns the player's view with it and a ship that pitches or rolls does not tip it.
- **Boarding and leaving are geometric rules looked at once a tick, not counted** (changed from the 3-tick hysteresis of the first text; the owner's
  to overrule): aboard when on the ground with a block of a ship a tenth below the feet; left when in the air with nothing of the ship under
  along gravity within the ship's box, or the body's middle a voxel outside it. The two conditions cannot hold together, so nothing flaps.
  Boarding takes away the part of the speed relative to the ship that points away from it: a body that stands on something is not moving off it
  (a walker that a falling ship has lifted onto itself was thrown ten voxels up at the speed the ship had, before this was added).
- **A character is never left inside what it can touch**: every tick one that something has moved into is moved out the shortest way (`CharacterBody::keep_clear`: up first, steps
  of an eighth, at most 8 voxels, else it stays), for terrain by its voxels and for ships by their colliders, with a box 0.02 smaller on every side. Found by measuring in 32g: a
  ship that fell on a walker pressed them through the floor of the world.
- **The view is the player's own angles in the room the body's frame says** (`PlayerPose::view_orientation`): the entity's orientation is the turn
  of that room and then the angles, so the angles are never converted when a player boards or leaves, and the stream of a player who never
  boards is byte for byte what it was. The camera has a `frame_rotation` and looks at its angles in it.
- **A ship that sleeps has no speed on record, and one that is held has none until whoever moves it says how fast** (`set_grid_held(true)` stops it):
  what a player who steps onto it inherits is its recorded speed at the place (`MovingGrid::velocity_at`).
- **A rule asks about the world through the engine's any-grid calls and is tested on a ship** (2026-10-22): `WorldData::grid_frame` / `voxel_in_world`, `PlayerPose::distance_to_voxel_of`,
  `PlayerReach`; `ship_parity.rs` runs a rule's scenario in terrain and in a ship, so "a mod's rules work aboard" is tested, not trusted. The level-keyed voxel calls are gone (owner,
  2026-10-22: pre-alpha, be bold): the one way to read and write voxels is `ServerWorld::grid(GridId)` and `GridView`, for the terrain of a level and a ship alike.
- **A ship that goes away with its pilot comes back where it was, or the player decides (32i):** it is stopped and put in a file; when a player who was aboard joins it is looked at
  against the terrain and the other ships, and if it fits it is there. If not the player is asked (move it, break the terrain, break its blocks, put it there anyway, or go to the spawn);
  the engine does not guess. A ship with another player aboard does not go. The questions are not saved.
- **Ships are kept by the distance of the nearest player in their level (32i, `engine/grid_lifecycle`):** moving within the simulation distance (256 voxels by default), frozen beyond
  it, in a file beyond the unload distance (1024), with a tenth more to leave a state than to enter it. A server with no player in a level keeps no ship of it awake. The host chooses
  the distances and what a pilot's logout does (`ShipSettings`).
- **A ship is saved as a file of its own, named by the ship and a write counter saved with the world, and a world is saved as a set (32i):** the ship files first, the world file that names
  them last, then the ship files nobody names are removed. Whatever stops a save, the world file that is there is whole and every ship file it names is there. A passenger is saved in the
  frame of its ship with its place in the world.
- **A player who comes back is told where they are** (`player_presence:placed`, in the world), because a client watches around its camera and a
  returning player may be anywhere. (Not yet done for `/tp`; a follow-up.)

## Decided (the material palette; built in Phase 31)

- **The game's palette is a storage buffer, indexed by the quad's 24-bit material value,** so a game has 1,048,576 materials (a choice of the renderer, to bound the buffer at
  16 MiB; the format has room for 16 million). Avatar colours are the last 16 values (`0xFFFFF0`..), kept in the frame uniform, so no game value is reserved.
  A value past the palette draws magenta, as a block of a missing plugin does. Nothing outside the renderer changed.

## Decided (the user interface; built in Phase 30)

- **One small interface model, three sources, one drawing path.** `ui` is a crate with no graphics: a tree (`Panel`, `Label`, `Button`, `Bar`, `Swatch`, `Space`), its
  limits, a canonical byte form, and a layout in whole pixels at a whole-number scale (so a picture of it is the same on every machine). The renderer draws
  flat rectangles (`set_overlay`); text is rectangles too. The shell's own screens, a client plugin's `overlay`, and the server's forms all end up in one list.
- **The server describes forms in engine messages,** so every game's clients can draw them with no plugin and a recording shows them. A form belongs to
  one player, has a number never reused, is not saved, and is dropped when the player leaves; only a button it has can be pressed, only by the player it was
  shown to, and only a modal form can be put away. The plugin hears `UiResponded` and answers by updating, closing or doing something else.
- **A modal interface has the player's input and the world has none:** the mouse is freed, held keys are forgotten, a half typed command is dropped,
  clicks and keys (Tab, arrows, Enter, numbers, Escape) go to its buttons. A question the server asks (Phase 27h) is shown this way, and `.choose N` still works.
- **Client plugins cannot take the input:** an overlay is always shown as a panel. A choice is the server's form.
- **The title is for the game and the frame statistics;** what the player is doing (position, HUD, the command being typed, the reply) is on the screen.
- **Pictures compared to the pixel are taken once two frames agree** because of a quirk of the development machine's integrated GPU (see `ui-spec.md`).

## Decided (data packs; built in Phase 29)

- **A pack is data, loaded by the server only.** A folder of JSON adds blocks and patches blocks; no code runs. A client needs no pack: it is sent
  the result in the package (Phase 28), which is why packs needed nothing on the wire. A server that finds any problem in its packs does not start.
- **Every problem is reported with its file and the path inside it, and all of them at once.** The reader walks a `serde_json::Value` itself (serde
  derive would stop at the first error and loses the path); unknown keys are errors with a "did you mean" when one is near; a failed patch changes nothing.
- **The loader is deterministic in what it is given, not in how it is listed:** packs in dependency order with ties by id, the files of a pack by
  name, so numbering and results are the same on every machine and a property test shuffles the listing. A pack may only add blocks in its own namespace
  (its id), and patch only namespaces it names as needs (directly or through its needs), so order cannot decide who wins.
- **Optional patches are the only quiet failure,** and they are not quiet: a patch that names a block or property that is not there is an error unless it
  says `"optional": true`, and then the skip is returned and printed as a warning. A `test` operation that fails is always an error.
- **The schema is generated, never written:** `pack_schemas(&PropertySchema)` makes the three JSON Schemas from the declared keys, kinds, ranges, defaults
  and docs and the shape names. A differential test keeps the schema and the loader agreeing on a corpus of files and on files damaged in one place;
  what a schema cannot say (a box with no size, a state the block lacks, a name taken, a cycle, a failed test) is listed in the test with its reason.
- **What a player can place is a property of the block** (`voxunami_cave:placeable`), set by Cave on its six blocks and by a pack on its own. Cave's items,
  its drops and its number keys come from it; the client reads the list from the server's package, so a client with no pack has the same keys.
- **A client refuses a package it cannot draw** (more colours than the renderer has) with an error instead of a panic, and the same check runs when packs load.
- Mine, M0 housekeeping (2026-10-30; `architecture-review.md`): **a folder in a pack that nothing reads is an error** (`check-pack` names it and suggests the nearest of `blocks`, `patches`,
  `bodies`), **`pack.json` accepts `"format": 1` and a free-text `"version"`**, and the examples in `pack-format.md` are checked by a test.
- Mine, M0 (2026-10-30): **a server that panics is noticed and nothing is saved from it**: a panic leaves a tick half done, and a file written from it could replace a good world. `ServerThread::stop`
  returns the failure, `has_failed` lets `serve` end by itself, and both say which save is the world. Open: a game played with no `--autosave` saves only on exit, so a failure there loses the session.
- Owner and mine, F5 (2026-10-30): **Cave owns Cave's ground.** No world is worth keeping before the first release (owner), so generator versions may change until then. The planet, the great
  rivers, the hills and the planet family are in `games/voxunami_cave/src/generators` under Cave's own ids (`voxunami_cave:rolling_hills_v1`, `small_planet_v1`, `empty_v1`, `great_rivers_v1`, the
  family `planet_v1`); `base_blocks` is only blocks and properties. The engine keeps the machinery and **`SurfaceLayers { air, surface, subsurface, deep }`**, so it names no block; its own tests use
  fixtures under `terrain`'s `test-support` feature. Checked: with the old ids written back Cave's golden session gave its old pins to the digit, so only the names changed.

## Decided (the content package at the handshake; built in Phase 28)

- **The server says what the world looks like.** `Welcome` carries the game's *package data* (`game_shell::PackageData`): the property schema, every
  block's definition (colour, see-through, the boxes of each state's shape, state colours, the properties it sets), the channels the view reads
  and the ambient light. A client builds its block table from it, matching blocks to its own numbering by identifier, so a client and a server
  that disagree about a block draw it the server's way (the server wins, silently), a block the server has no definition for is drawn as unknown,
  and a recording carries the package, so a replay does not depend on what is installed. Terrain generators and client plugins are still compiled
  in (they cannot travel until generators can be sandboxed code).
- **Inline, not cached.** The package is in every welcome (a few KB for Cave: 1,345 bytes). Caching by hash needs a request message, a cache and a
  way for recordings to embed what was fetched, and saves nothing until packages hold assets; it is built in the phase that adds assets. No hash
  is sent now either: nothing would read it.
- **One spelling.** The byte form is canonical (maps in key order, counts and numbers as varints, colours as `f32` bits), and decoding refuses
  anything the encoder would not write (out-of-order or repeated keys, empty boxes, non-finite colours, a default outside its range), so a
  damaged or hostile package is a connection error naming the cause and never a panic. The codec for definitions lives in `block_models`, the
  package around them in `game_shell`.
- **`GameContent` has one source for this**: `package_data()`. `block_table()` and `ambient_light()` are derived from it for the server side;
  clients no longer call them (they take the table, the channels and the ambient light from the welcome). Server code that reads the table keeps
  using `GameContent::block_table`.
- **A package that does not fit what the client loaded** is a loud error (`NoBlockRegistry`, `NoChannel`, a property the schema lacks), not a
  silent fallback to the client's own definitions.

## Decided (asking a player a question; built in Phase 27h)

- Asking is an engine facility, not a game's: any plugin may ask any player a question with a title and 1 to 9 options (`Context::prompt`) and
  hears the answer as the event `PromptAnswered`. The engine owns the numbering and the validity (a question is answered once, by its player, with an
  option it has); the plugin owns what the question means and what to do while it waits (holding an entity, for instance).
- Questions have no time limit and are not saved. A player leaving drops theirs; the plugin asks again if it still needs to. This keeps the
  facility stateless across saves; a saved world never holds a question for a player who may never return.
- Ids are never reused, and the client is told when a question is over (`engine:prompt_closed`), so neither a stale answer nor a stale screen can
  happen. The shell shows the first question on the HUD line and answers with `.choose N`; the toolkit of Phase 30 restyles it without changing
  the messages.

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
