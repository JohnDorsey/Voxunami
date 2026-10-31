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
  - Docking, as decided on 2026-10-22 (built in 32m part 2): the yellow face of one dev docking port docks with the yellow face of another; ships snap to exact alignment on all axes within 10 degrees
    and 0.3 metres, and any roll in quarter turns is allowed because the grids need only be aligned cube to cube; the attraction also twists the ships into line; **forces do not scale with the
    masses of the ships**: each port gets the same force and each ship the same torque, so lighter ships accelerate more. The dev docking port is creative only. A block's face properties
    (`engine:marked_face`, `engine:mark_color`, `engine:docking_face`) are engine-reserved block properties, not a new field of a block definition, so the bytes of a definition and the
    data-pack format do not change and a pack sets them like any property. Merging is `ServerWorld::merge_grids` and undocking is `split_grid` with the ports not counted as a join, so everything that
    works across a ship works across a docked pair with no work from a mod; a hinge or an axle joining ships is a different union and is not planned to carry natural processes across.
  - **Units are metric** wherever the thing exists in our world (owner, 2026-10-22): a voxel is a metre, mass is in kilograms, force in newtons, torque in newton metres, time in seconds. The code and the
    older text say "voxels a second" for a metre a second. Until materials have real densities (a conversation in `future-goals.md`, goal 9), every block that is not air weighs 1,000 kilograms
    a cubic metre (a metric ton), the default of `engine:density`. **Gravity is 5 metres a second a second** (owner, 2026-10-22: the planet is not the Earth, so it need not be 9.8; it was 32, a number from block games):
    `GravitySettings`, `BodySettings` and `CharacterSettings` default to it, and the jump is 3.6 metres a second (was 9), which is the same jump of 1.3 metres.
  - **Every block has an orientation, saved the same way for all blocks** (owner, 2026-10-22; replaces the first answer that closed a block's states under the turns): a voxel has a channel `base_blocks:block_orientation`,
    the number (0 to 23, the index in `LatticeRotation::all()`, whose order is pinned as a file format) of the turn that carries the block as it was made to the way it stands. It is the same for a stone, a log and a
    block of a plugin that is not loaded, however many visibly different orientations the block has, so turning is composing numbers (a merge turns every voxel's orientation by the transform, with no knowledge of the
    block, and data of missing plugins is turned too). A block's definition does not list turns; `BlockTable` is indexed by block, state and orientation (`ModelId = base * 24 + orientation`) and de-duplicates the
    shapes, so a cube in 24 orientations is one shape and a slab six. Face properties (`engine:marked_face`, `engine:docking_face`, `engine:front_face`, `engine:axis_face`) are given for the block as it is made and turned at read
    time. A fire's age, and 2048's numbers, are states and are not orientations, so they turn like everything else. **Placing** (`engine:front_face` and `engine:axis_face`, set per state): a block with one front face (a piston)
    is turned to face the player as in Minecraft (up if put within two voxels beside the player and more than two under their eyes, down if above them, else opposite the way they look along the ground); a block with a line of
    radial symmetry (a log; wood is one, with both ends marked) lies along the face it was put against; any other block is not turned. The client says which face it clicked (`SetBlock.against`), the server works out the turn
    from the player's eyes in the grid's own axes (a ship's, aboard one) and writes it. Breaking resets it.
  - **The docking force is at the port** (owner, 2026-10-22), as a force on the centre of mass and the torque of the lever; a ship whose centre is far from its port is swung by it, and the twist does not have to overcome that for
    ships that are built sensibly (the port near the middle of the face).
  - **Chance keyed by grid is fine** (owner, 2026-10-22): a rule with chance does not go on bit for bit across a merge or a split, only in distribution.
  - A player who connects to a server and whose ship file cannot be found is asked what to do: spawn at the world spawn, spawn in
    midair, or quit to the title screen.
  - Added by the owner, 2026-10-20 (`future-goals.md`, "Ship goals"; planned in `moving-grids-spec.md`): sufficiently huge ships have their own
    gravitational pull; ships are drawn with levels of detail when far away, like terrain; fluids may flow inside a moving grid, and fluid or
    current may cross a ship's edge to the nearest voxel of another grid (the shape is left open, and must be possible); a ship may disappear
    when its pilot logs out and reappear when they log back in, depending on a server setting and the command chair's setting; simulation distance
    for ships is a server setting. Also (same day): feeling the force of acceleration aboard is a future server option; fossilization (an
    indestructible, immobile ship, operator commands only) is a future way to protect a ship at logout. Answers to the Phase 32 spec: unpiloted ships
    stay simulated (they fall); every part of a split gets a new id and a merge gets a new id; a logged-out passenger returns aboard the ship wherever
    it has gone; a blocked arrival asks the pilot (move ship, accept damage to terrain, accept damage to ship, spawn in terrain, warp to world spawn).
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
- How light reaches the renderer (decided by the spike in `docs/history/light-spike.md`, built in Phase 7): each quad carries its
  light in 8 bits of its material word, the mesher uses the light of the voxel in front of a face as part of the merge
  key, and the quad stays 8 bytes. Measured on rolling hills with made-up lamp light: meshing time +2 to +8 percent, quad
  count +25 percent for Minecraft-style 16 levels around very dense lamps, +62 percent for 256 smooth levels, none for
  uniform light. Rounding light to fewer bands in the mesher is the lever if a scene gets heavy (not built); a light
  texture is the later route to smooth shading and would not change storage or the protocol.

## Decided (levels of detail; Phase 38, owner 2026-10-23; specified in `lod-spec.md`)
- One level-of-detail system for terrain and every grid, ships included, so that the ground can one day be a ship (a planet: a rough sphere of cubes in ordinary
  space, no curved grids). It must scale to planets of 10^8 m and more; the maximum detail level is **116** (a cell of 2^116 voxels; it was 40 until the coordinates were 128 bits, see "Coarse levels to the root"). Detail levels are counted in voxels; a game has one voxel size.
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

## Decided (planets as ships, and the level `mini_cosmos`; Phase 39, owner 2026-10-24; specified in `planet-spec.md`)
- The first planet is a ship with a terrain generator, 1% of the Earth's radius (63,710 voxels), in a new level `mini_cosmos` that is the default level for now; a new world has it first and the overworld second, and `/level go overworld` reaches the overworld (owner). Its
  outside is a 2D heightmap bent into a sphere: a cube-sphere with the faces' heightmaps blended where they meet (owner). Gravity at its surface is 10 m/s^2 (owner; the overworld's is 5). Caves are in the bricks and not in the levels of detail (owner).
- Mine, the owner's to overrule: the planet's heights are the overworld's noise on six faces (9 octaves from a kilometre, 160 voxels at the largest), read at the angle across the face (a polynomial, not `atan`: IEEE operations only, as every generator) and blended with weights whose squares add
  to one (so the blend keeps the spread of the noise); the layers (grass, 3 of dirt, stone) are read from the depth under the surface, radially, so a cave under the ground does not turn the ground beside it to grass; bounds combine the faces by Cauchy-Schwarz, which is looser at
  the corners of the cube (404 wide at a brick against 134 to 151 elsewhere) and costs only work; the planet's coarse nodes are made by walking columns along the axis the node is nearest to and asking the field at the middle of each cell, with the layers looking outward along the axis
  (`coarse.rs` took the step "up" from a cell as a parameter for this). A planet's generator is a new version with its own ids; its output and its coarse nodes are file format like every generator's.
- Mine, built in 39d: **a planet has no body.** `grid_bodies` leaves a ship with terrain out of its bodies, its weighing and its splitting: it weighs what its terrain declares (the pull at the surface fixes the mass: M = g R^2 / G), it is held, and the edits stored over its generator are not a ship that can come apart or be gone. A walker meets it through
  the world's reads (`GeneratedHull`) with the same rules as any ship (`ShipSource`, `SolidAt`). **A level's pull is the level's own** (`set_level_gravity`, `ServerConfig.level_gravity`, `GameDefinition::level_gravity`), and **only a ship over `GravitySettings::pull_mass_threshold` pulls** (Cave: 10^19 kilograms), so a raft pulls nothing and a planet pulls the walker toward its centre.
  (Open then, done in 39g: a rigid-body ship had no collider against a planet.)
- Mine, built in 39e: **a planet is known from anywhere in its level** (every client that watches the level is told of it, and the lifecycle never puts it in a file), **drawn in detail by the same view that draws a level** (`LodView` over a `CoarseTerrainSource`), and **its nodes are the generator's and are not corrected by the server** (an edit shows in the bricks near the viewer only; a decision for later,
  with the messages already keyed by grid). **A generator may give a solid core** (`TerrainGenerator::solid_core`), and a view leaves out the cubes behind it from a viewer outside it. Cave's planet is drawn out to 10^7 voxels in cells of 3 pixels; the terrain of a level is not drawn beyond its bricks yet (`coarse_detail` is still none).

- Mine, built in 39f: **a new world is space first and the overworld second**, with the planet made with it; **the world the tests of the overworld start in is the overworld alone** (`new_overworld_world`), and a saved world loads as it was. **The planet is 2,100 bricks each way, held, of the mass `10 R^2 / G`**; **an empty level is a generator** (`base_blocks:empty_v1`), not a level with no terrain.
  **A new player stands on the ground at the pole of the planet** if their client began where the game says players start, and is where they asked to be otherwise (`--camera`); **a level has a place players arrive at** (a rule the game gives the presence: the pole of a planet, or where players start when they come from a level with a planet), instead of the same coordinates in another world.
  **A planet is always in memory** (a flag in the world file's ship list; the lifecycle brings it back whatever the distance). Still open: see the list at the end of 39f in `planet-spec.md`.

- Owner, 2026-10-25: **a planet is a kind of ship with slightly different rules, sharing most of the code of ships; all planets will be on rails; planets are never docked to each other or to a moon** (alignment of nodes and bricks is not worth it, and on rails the question of what happens when two come near does not arise); **a ship that docks with a planet is written into the planet's grid and the planet's identifier does not change**. The code is to stay simple, and to be solid before it grows.
- Mine, built in 39g: **a planet has a fixed body in the frame of its level, with colliders only for the bricks near a free ship**, from the stored brick or else the generator (`GeneratedHull`), by the rules and the code the terrain of a level has (`Ground::{Level, Planet}`); not limited to the generator's extent. **A planet farther than 4,096 bricks from the frame's origin has no body** (`f32`). **Where a ship may arrive** takes the ground of a planet as terrain, **the world's spawn is a rule of the game given the world and the level** (the pole of a planet, else where players start), and **`merge_grids` refuses a ship with terrain**. Still open, for the owner: see (i) to (n) at the end of 39f in `planet-spec.md`.
- Mine, built after 39g, following the owner's stance: **a ship that docks with a planet is written into it** (`ServerWorld::dock_into`: its blocks are stored as edits of the planet, checked against the planet's generated blocks too; the ship's id leaves a tombstone naming the planet, which keeps its own), a planet's ports are those in its stored bricks, and **a planet is never merged or split** (`MergeError::HasTerrain`, `SplitError::HasTerrain`). `/ship create` works on any grid. **A ship docked to a planet can be undocked** (the side of the pair that does not stand on the ground leaves, as a new ship; both on the ground is refused), **a planet cannot be held, pushed, piloted or told to vanish**, and **items fall along gravity in the axes of their grid and are picked up in every grid of the level** (survival works on planets and ships).
- Owner, 2026-10-25: **Cave is to become a space-exploration game (KSP, KSA)**: ships will go on rails when far from the players or in a time warp (`future-goals.md`, unscheduled). **Until then simulation of ships just stops beyond a distance of the nearest player: a server setting** (`ShipSettings::simulation_distance`) **with a command to change it** (`/ship distance`). Mine, built after 39g: **the bodies of a level are in a frame for each place**, not one frame around the first ship, so that ships near different players, far apart, are all simulated.

- Mine, built in 40b (planets from data, `rails-and-recipes-spec.md`): **a terrain generator may be data: a recipe is (instance name, family, parameters as bytes)**. The family is code and a file format (`base_blocks:planet_v1` is the algorithm of `terrain::planet`, and never changes once a world has used it); the instance name is the name in the world's table of generators, so a ship file, the world file and the wire name it as they name any generator and **nothing in them changed**; the parameters are the family's own bytes (`PlanetSettings::to_bytes`, 35 bytes), opaque to the engine. **The world keeps its recipes** (`engine:generator_recipes`) so that a pack edited later does not change a planet that exists, **and the description carries them**, so a client makes the same generators without a new message. A recipe whose family is missing, or that refuses its parameters, leaves a placeholder in the table (as a missing plugin's generator does) and is kept byte for byte. `small_planet_v1` stays in code for the worlds that have it.

- Mine, built in 40c (planets from data, `rails-and-recipes-spec.md`, `pack-format.md`): **a planet is a body: a file of a pack (`bodies/NAME.json`) that names a level, a place, the family of generators that makes its ground with the family's parameters, the extent of the ground and its weight** (a mass or the pull at its surface). **The family has a data side** (`data_packs::FamilyData`, in the plugin beside the code) that reads the parameters with every problem pointed at and generates the JSON Schema of them, so `check-pack` and the editors know a family without the engine knowing any. **The game's own planet is a body of the game's own data**, in the same format (`include_str!`, read when the package is made, a mistake is a loud panic), and a world takes its bodies **when it is made**, never when it is loaded. `"seed": "world"` is exactly the seed of the world. A package that has bodies tries to make them in a thrown-away world whenever packs are loaded, so a pack whose body cannot be made is refused at load.

- Mine, built in 40d (planets on rails, `rails-and-recipes-spec.md`): **a rail is part of a ship** (`MovingGrid::rail`, the ship file's `engine:rail`): a spin and/or a circular orbit, **a pure function of the world's clock** (`libm` in `f64`, the whole turns taken out first; pinned to the bit), so nothing is integrated, nothing drifts, and a world that is saved and loaded has its planets where the clock says. **The engine puts ships on their rails** (`Server::step`, `WorldData::put_ships_on_their_rails`), not a plugin, in order of the ships' numbers so that a moon follows the planet it goes round (the centre must have a lower number). **A body on a rail is in free fall with what it goes round: what is on it feels its own pull and not that of its ancestors** (`gravity_in`), which is what makes a walker aboard stand still on a planet at 12 metres a second. (A planet's body in the physics was still a teleport in 40d; 40e made it kinematic, below.)
- Mine, built in 40e (the bodies of a planet on rails, `rails-and-recipes-spec.md` 3.4): **a planet on a rail has a sphere of influence** (one and a half times the reach of its ground, at most 120,000 voxels, the smallest sphere when in several, kept a quarter farther out than entered), and **everything in a level inside it goes with the planet: the server moves it by the planet's displacement of the tick**, before anything in the tick looks at the world, so that players, items and ships are never a tick of the planet's motion behind it (6 voxels at 125 metres a second; 1,500 at an orbital speed). This includes ships that are **held** (held against the planet, at its speed in the world, so that they go on with it when let go) and **frozen**; it does not include a ship on a rail of its own, nor a planet. **What keeps a state of its own for an entity is told how far the world carried it** (`ServerWorld::carried_by_rails`, forgotten when the entity is put somewhere), because the carry of the record alone is undone by the next write of that state: the body of a player (`CharacterBody::displace`, in `Movement::step`) is moved by it. It is not found by comparing the record with the body: a record that differs is not always the newer. **The ships a planet carries are simulated in a frame that goes with it** (`Frame::follow`): places are the world's less the planet's displacement since the frame was made (a `Displacement`, exact in bricks), velocities the world's less the planet's, no terrain of the level (it stands still in the world), gravity without the pull of what the planet goes round. A ship leaving or entering a sphere keeps its place and its speed in the world (the plugin writes a ship's velocity as what it has against the frame plus the planet's). **A planet on a rail, and every planet in a following frame, is a kinematic body**, set each tick to where it was at the start of the step so that the ground carries what rests on it in step with the planet the world shows. The prices: a ship made at rest in the world near a planet that moves is going at its speed against it; an entity leaving a sphere loses its speed against the planet; ships either side of a sphere's edge are in different frames and do not touch; blocks built in space are not solid to what a planet carries.
- Mine, built in 40f (a solar system in data, `rails-and-recipes-spec.md`): **a star is a body like a planet: a ball of ground of the planet family**, not a body with no terrain (the default the spec proposed). Nothing in the engine knows a star; every place that treats a grid with no terrain as a ship (merge, split, hold, carry, lifecycle) leaves it alone. A terrainless body comes when a star is drawn as a light: it needs a marker then anyway. Open: bodies are made in the order of their files' names (a body goes round one made before it), which makes names such as `star_planet_moon` a rule of the pack author; the order should come from the orbits.
- Mine, built after 40f (2026-10-27; the owner asked to continue after the discussion of a universe of 93 billion light years): **a rail is a function of whole ticks, and a body that goes round
  a body is placed by adding its offset to the other body's position, not by summing plain numbers.** `RailTime` (a count of units and how many make a second; the world's is the tick) and
  `WorldPosition::translated_by_voxels`. Not changed: the unit of a position (an `i64` brick and an `f32` offset), the format of a rail (the `f64`s of a period and a radius), the hard rule
  (`libm`, no wall clock). Why: the arithmetic was exact only while the world was small and young (measured: a moon 7 km from where it should be round a star 2.5x10^20 m out; a planet 1.3 m out at
  twelve thousand years), and the fix costs nothing at the scale of today's worlds.
- Owner's, 2026-10-27: **a coordinate of more than 64 bits goes on the wire as a length-prefixed integer** (a byte for the number of bytes, then the zigzag value in that many little-endian bytes, the fewest that hold it;
  zero is one byte), not as a wider LEB128 varint. A number has one encoding: a length over the width, or a top byte of zero, is a decode error. The reason is growth: 256- or 512-bit coordinates (if shrinking ships
  become a large part of play) need only a reader that accepts a longer length. `ByteWriter::write_wide_signed` / `ByteReader::read_wide_signed` (`i128` today). The existing varints are unchanged.
- **Coordinates of 128 bits (owner's wish 2026-10-27, built the same day; "A universe of 93 billion light years" in `future-goals.md`).** `VoxelCoordinate` and `BrickCoordinate` are `i128` per axis,
  within +-2^120 voxels (+-2^115 bricks, which is 2^116 bricks across, so `region_octree::ROOT_LEVEL` is 116, not 56); `WorldPosition` is an `i128` brick and an `f32` offset; the octree keys are `u128`.
  The orbit limit (a radius of at most 2^120 voxels) is the edge of the world, so no orbit is refused for its size any more, only a place past the edge. Chosen over keeping `i64` with nested frames
  only because the owner wants one block grid that spans the universe. What was decided on the way, and why:
  - *Wire.* A coordinate is three length-prefixed integers (`ByteWriter::write_wide_signed`; owner's choice (b): 256- and 512-bit coordinates need only a reader that accepts a longer length). `protocol::{read,write}_{brick,voxel}_coordinate` are
    the one place a coordinate is written, and game messages use them too. Protocol 9.
  - *Saves: a clean break (owner's choice).* The plain sections are version 2, `engine:entities`, `engine:grids` and a ship's flight version 3, `engine:generators` version 3 (a leaf's level is seven bits), the pending work
    of `lattice` version 2, Cave's fire version 2, 2048's saved state version 5. A section of any other version is refused (an engine section) or counts as none (a game's, which is kept in the file as it is), never misread. The
    readers of older versions, which these had (`PoseFormat::Single`, generators 1 and 2, 2048's versions 1 to 3), were deleted, not left dead.
  - *Digests.* `Grid::content_digest` hashes a coordinate as 16 bytes; it is not a stored value. A brick's digest has no coordinate in it and did not move.
  - *Terrain stays the same to the bit.* The noise hash of a lattice point is the old one for any coordinate that fits an `i64` (the upper 64 bits are mixed in only for one that does not), checked against an independent
    computation and by the checksum of 2,400 generated bricks being the same before and after. Generator versions are file formats: nothing in a released one changed for any place that could be reached before.
  - *Speed is a requirement, found by measuring.* The first working version made terrain 2.2 times slower (29.0 s against 13.1 s for the same 2,400 bricks): a sample takes dozens of lattice hashes, and a 128-bit value in each of
    them costs. The fix keeps the hot path in 64-bit arithmetic: `FractalNoise::sample` checks once whether a position is within 2^62 and then every octave runs on `i64`, exactly as before (`NarrowPosition`), with a test that
    the two paths agree to the bit at the boundary. Now 12.7 s, and Cave's whole golden session 48.2 s against 49.0 s. *Any new per-voxel loop must do the same: check once, then work in 64 bits.*
  - *Arithmetic that depended on the width* (found by reading, each with a test at the ends of the range): squares of distances (`FineRegion`, the mesher's range of bricks, the order cubes are made in) overflow `i128` across the range
    and are compared exactly after a check per axis, or saturate where only an order is wanted; the level at which one cube covers a grid; a `1u64 << level`; the octree's cost (isolating one assigned cube costs 7 leaves a level, so
    420 more leaves, one byte each, in the generators of a world).
  - *Not widened, still limits.* (Coarse nodes stopped at detail level 40 and `LevelOfDetail` at 60 when this was written; both went to 116 in the entry below.) A noise octave's wavelength is at most 2^56; a grid that is turned is converted through `f64`, so is exact only within 2^53 voxels of its own origin (as it always was).
  - *Tests whose premise was a limit.* `/tp` far out, a ship carried past the end of the world, and a coarse-assignment test that compared costs relative to a root of 56 were changed in their inputs (not their assertions) to
    stay beyond the new edge or relative to the new root.
- Mine, built after that (2026-10-27; the owner asked for accurate orbit radii "even if it requires fixed point 128 bit"): **the offset of an orbit is worked out in 128-bit fixed point and kept as
  whole bricks and a rest (`ExactOffset`), not as `f64`s.** `exact_circle.rs`: CORDIC sine and cosine in 126 bits, the plane in 126 bits, the radius multiplied in 256 bits (`u128` pairs; no
  dependency). Chosen over `f128` (unstable on the stable toolchain, no `libm` functions for it) and over double-double (two `f64`s: about 106 bits, whose bit-identity across machines would
  need arguing): the destination is an integer lattice of bricks, the precision wanted is absolute rather than relative, and integers are the same bits everywhere by construction. The format of a
  rail is the same (the `f64`s of a radius, a period and a phase); what changed is the meaning of those numbers at large radii (they are now exact). New limits, refused at make and read: a radius of at
  most 2^120 voxels, a period of at most 2^80 seconds, a phase of at most 2^63 radians. `WorldPosition::translated_by_bricks` carries the sum of the two offsets into the bricks before rounding to an
  `f32`, as `translated_by_voxels` did not (it lost up to 2 x 2^-19 voxel when the sum passed a brick's edge). Reference values and tables: `engine/world_storage/tools/exact_circle_reference.py`.

- **The great rivers (owner's wish 2026-10-27: "the generated object that uses all LOD levels"; built the same day; `great-rivers-spec.md`).** One river, a path across the universe, in the level `mini_cosmos` of a new world, of one block (`base_blocks:great_river_water`, passable, never flows), a billion light years across at the widest and 80 metres at the narrowest, far from the first planet (never within 4.5 billion light years). **The owner's** are the object, the block, the extremes, one path first with branching and merging later, and the distance from the planet. **Mine, the owner's to overrule:**
  - *A path of knots joined by courses of a small whole direction and a number of steps*, the water the union of a ball at each knot and a tube along each course whose radius changes as a power of two (the log of it in a straight line). Chosen over a polyline of arbitrary integer vectors because the offset of a voxel crossed with such a vector overflows an `i128` at the size of the universe, and over exact big-integer arithmetic because a direction of at most 5 keeps the cross and dot products inside an `i128` for any voxel of the world, so a tube 80 metres across is sharp 4 x 10^26 voxels out with nothing but `i128` and `f64`. The radius law is the one that makes thin parts long (a horn); a radius that was linear along a course would be thin only at a point.
  - *The river is a `DensityField`*, so bricks, `may_show` and coarse nodes are the code every field has, and **the bounds over a box are sound by a ball round its middle** with a margin of 10^-12 of the sizes (and a voxel) for the rounding of a sample; only the pieces near a box are looked at (`near`; a test shows it changes no sample). Water stands in for stone, dirt and grass, so a river is one block.
  - *`f64` places the surface of a tube only to about 2^-52 of its radius*: nobody sees that (the wall of the widest part is a staircase of steps of a billion voxels), and bricks within that of a wall are sampled voxel by voxel.
  - *The generator is `base_blocks:great_rivers_v1`, the fourth of the table; the block is the eighth, so `dev_docking_port` is 8.* It is the terrain of `mini_cosmos` in a new world; a saved world is loaded as it was. The golden session moved by 152 bytes (control runs in the comment on `PINNED`).
  - *`/river [narrowest|widest|N]`* puts the player three radii and 200 metres from a knot with it straight ahead of where they look (from the angles their program last sent), or at right angles to the river if that place is in the water. A stopgap: the owner has asked for the teleport command to be designed again (`future-goals.md`).
  - *Known, not decided:* a thin river vanishes from the coarse nodes above about level 5 (a cell is drawn at half full). In `great-rivers-spec.md`, section 7. (Cave drew no level beyond its bricks until 2026-10-28: next entry.)

- **Places: `/tp` for several planets (owner's wish 2026-10-27; built the same day; `places-spec.md`).** **The owner's:** names are namespaced; a body is reached on the ground **nearest to the player**; **two numbers after it are a latitude and a longitude and three are x, y and z** (of the body); everyone may use it for now (operator only is an unscheduled wish). **Mine, the owner's to overrule:**
  - *A body is a place named as the generator of its ground is*, which a body's file already named (`voxunami_cave:planet`, `moons:moon`): **no file format, protocol or ship changed**, a planet put in a world later is a place at once, and the list is made from the world each time. A landmark is a knot of a great river (`base_blocks:great_river_narrowest` and `..._widest`); `/river` is gone, and with it going to any knot by number. New kinds of place are a variant of `PlaceKind` in the Cave crate until a second game wants them.
  - *The ground nearest to the player* is where the line from the body's centre to their eyes (in the body's own axes) meets the ground from outside: walked in from the box of the body's extent, a brick of air crossed in one step, the voxels read as a walker reads them (stored, else generated), so a turned or moving body works and what was dug counts. Chosen over the column along an axis that the pole's landing uses, which works at the pole only.
  - *Latitude and longitude turn with the body*: north is its +y, longitude 0 along +x, 90 along -z. The body of a player is not made to face anything (the server cannot set the way a player looks: `PlayerPlaced` has no angles).
  - *The old `/tp X Y Z` stays*, in the level the player is in, and is exact for whole numbers of any size (`i128`), where it was `f64`.
  - *A place in another level takes the player there* (`/level go`'s move, so the client is told), and a player who is moved out of the ground is told so.
  - The golden session moved by one byte (the list of commands), worked out before it was run.

- **Text wraps, and coordinates are hexadecimal (owner's request 2026-10-28; `ui-spec.md`).** Text wider than the screen is broken into lines in `ui::layout` (the only place that knows the width): at spaces, a word longer than a line cut, a no-break space to keep a letter with its number; the window's own lines may be 2,000 letters (a server's descriptions are still 80 a label); a column of nothing but text that is taller than the screen is cut with dots, and nothing with a button is ever cut. The coordinates of the status line are `x 0x1f y 0x6e z -0x5`, from binary. *Mine, the owner's to overrule:* lowercase digits; the status line stays a line of text that wraps (a panel of its own for coordinates was not asked for).

- **The sun (the owner's "Lighting please", 2026-10-28; `sun-spec.md`).** A game has a **sun per level** (`GameDefinition::sun`): a function of the **world's clock**, which the server now sends in the welcome (**protocol 10**) and the client counts, so everyone has the same sun and a recording has the day it was made in; nothing that depends on time reads any other clock. The renderer lights each pixel by `sun_light * cos * visible` and a sky term for every face the sun reaches (`Atmosphere::sun_light`, `sky_light`; both 0 by default, and a world with neither is drawn exactly as before), where `visible` is a soft-edged function of what is in the way: **the solid core of each planet** (balls; the terminator is a sphere and the planet's day follows its core) and a **flat ground** (the overworld). A face at the ground of a planet is lit by a normal turned half-way to the way out of the planet (a planet of voxels has the light of a cube otherwise). The sky colour is the game's, darker with the daylight at the camera and black above the air of a planet. **Shadows from terrain are built the same day** (see the next entry). Chosen by me, the owner's to overrule: a 20-minute day beginning at 7, sun 255 and sky 110, a sky blue to 30 km over a planet, nights black (no moon), soft edges of 0.005 rad plus 0.1 rad of twilight, `.time` and `--time` to pin the sun for one player. The rule is written twice (WGSL and Rust) and held to each other by tests on every adapter; **the integrated AMD GPU in DX12 returns no number for `asin(1.0)`**, so the shader never asks it for that.
- **Shadows (the owner's "Continue" and the note on mountains, 2026-10-28; `shadow-spec.md`).** **Cascaded shadow maps**, up to eleven of 1024 x 1024 texels, each a picture of the terrain from the sun of a square across the light round the camera, in half widths that double from 32 to 512 voxels and then grow fourfold and eightfold, as far as the picture is drawn: **nothing in them assumes a size of world** (camera-relative, biases in texels, a depth fitted to the casters and measured from near the receivers, a place looks up the finest cascade that holds it by its own distance), and the suite draws the same scene at every scale from a brick to cells of a million voxels, and a mountain half a million voxels high shading voxels at its foot. **Only the faces that look away from the sun are drawn into a map**, so a lit place cannot shade itself at any angle. **A map is kept** while the camera is within an eighth of its half width of where it was and the sun within 0.005 of where it was, and drawn again when it is wrong, and, when it is only out of date, after waiting for the frames its cost asks (finest first, one always allowed): the first version, which drew them all in every frame, cost 32 ms in a frame of 4. The terrain's shadow takes the sun's light as it is and a share of the sky's (`Sun::sky_light_in_shadow`, Cave 70 of 255): a proxy for the sky being hidden, which the sky's cover (next entry) refines for what is over a place. `.shadows` and `--shadows` turn them off for a player (a slow GPU). Chosen by me, the owner's to overrule: the sizes of the cascades, the share of the sky in a shadow, the pace of the refreshes.
- **The sky's cover, the ambient term for caves (the owner's "Continue", 2026-10-28; `shadow-spec.md` section 8).** **Four more cascades of the same texture, of the terrain seen from straight above** (the front faces, the highest thing over each column; the camera's own way up: +y on flat ground, the way out of the nearest planet), kept while the camera and the terrain are, whatever the sun does. A place looks at **13 columns** round it (out to 8 voxels) and the share that nothing is over is how open the sky is; the sky's light of a pixel is multiplied by `mix(sky_light_under_cover, 1, open)`; the sun's is not touched. A place on a wall is moved 9 voxels out before it is looked up, so that a wall is judged by the air in front of it. Only near the camera (to 11, 39, 94 and 205 voxels): farther the sky is open. **Opt in**: `Atmosphere::sky_light_under_cover` is 255 by default (no cover, no maps, pictures as they were) and Cave says 0 (a cave is black by day, lit by lamps): **no flooded sky light**, as the owner asked. Cost: nothing on the RTX 3090 (+0.1 ms), about 1.3 ms (8%) on the integrated GPU at 1280 x 720. Chosen by me, the owner's to overrule: four maps, thirteen columns, 3 and 8 voxels, the push of 9.
- **The sun is a star; the night has a light; a sunset is warm (the owner's "the sun should be an actual star, like any other, visible from the surface of the planet", and "continue with the plan", 2026-10-28; `sun-spec.md` section 8).** **The sky is drawn, from the direction of each pixel, after the terrain and only over the pixels it leaves** (a triangle over the picture at depth 0 with the test `Equal` to the clear value: no cost I can measure; a picture of nothing but stars costs the integrated GPU a millisecond). **The stars are a field of a hash**, six cube faces of 128 x 128 cells equal in angle (so the same on every machine, in every window size, and testable on the CPU by `star_in_cell`), **that turns with the day** by the sun's own turn undone, **so the sun is at one place in the field and is a star like the others, near**: a disc (0.02 radians, four times the real one) with the air's glow, visible from the ground over the hills and from space among the stars. **The night light is the sky's light at night** (`night_light x (1 - visible) x uncovered`): it goes with the sky's cover, so a cave is black at night as by day. **A sunset is a bump in the sun's height**, measured from the horizon of the solid core on a planet (the light's, not the disc's), that takes the sky, the fog and the disc to orange. **Opt in**: an empty `Heavens` and no night light (the defaults) are every picture as it was; Cave says 40 and the shell fills `Heavens`. Chosen by me, the owner's to overrule: the disc's size, the stars' number, the night light's 40, the colours of a sunset.
- **A game says how far each level is drawn (the owner's "continue", 2026-10-28, after the rivers; `great-rivers-spec.md`, `lod-spec.md`).** `GameDefinition::level_detail(level) -> Option<CoarseDetail>`, whose default is `coarse_detail()` for every level, so a game that said only that is drawn as it was; `Session::enable_level_detail` (what the windows and screenshots call now, instead of `enable_coarse_detail`) makes the view follow it: nodes beyond the bricks in a level the game draws so, bricks only in another, changing over (and taking down what the old view drew) when the player goes from one to the other. **Cave draws the level of space in nodes to 2 x 10^27 voxels, past the size of the universe, and the overworld in bricks only**: the overworld's nodes cost 60 to 375 ms each (`lod-spec.md` section 9), a level of rivers' cost is a few milliseconds. The window refreshes the fog when the radius changes. *Measured:* the great river is drawn from 320 m and from 1.4 x 10^25 voxels, the renderer needing nothing for that range on four adapters (a test keeps it); the cost at the planet's pole is 0.39 ms worst per `update_view`. *Not done:* the server's correction of nodes (not registered in Cave): an edit shows in the bricks and not in the far nodes of the level of space. *Mine, the owner's to overrule:* the distance (2 x 10^27 is past anything, and the fog's start at 60% of it means no fog), and that the far nodes are dark like everything (ambient light 0).

## Decided (the plugin host; built in Phase 23)
- **Coarse levels to the root (owner's wish 2026-10-27, "the generator's coarse levels above detail 40"; built 2026-10-27).** `MAXIMUM_DETAIL_LEVEL` and `LevelOfDetail::MAXIMUM_LEVEL` are 116, the octree's `ROOT_LEVEL` (a cell of 2^116 voxels, and a node of the whole
  world; a compile-time assertion keeps the three the same number). What was decided and found, tests first:
  - *The machinery was nearly ready; the bugs were in what the old limit hid.* (1) The depth of ground over a cell, which gives dirt or stone, was counted in a `u64` of voxels times 255: from level 64 the width of a cell truncated to nothing, so every cell of a deep
    node looked like the top of the ground (grass), and in a build that checks overflow it panicked. Now a `u128`. (2) A planet's bounds over a box that reaches far across a face (a cube beside the plane through its centre) read the heightmap at a ratio of millions, which overflowed the coordinate; the face
    is never read past a ratio of 1.36 (`LARGEST_RATIO_READ` is 2, with a test of the 1.36), so the ratio is clamped, and a heightmap that does not fit the coordinates leaves the bound at the noise's whole range. (This was in the `i64` code too, out of reach
    only because nodes stopped at level 40. The planet's pinned outputs did not move: a tighter valid bound changes no decision that was not already exact.) (3) The renderer's scale of a node was `1u64 << level`: now an exact `f32` power of two.
    (4) The distance a view reaches was clamped to a `u32` of bricks (1.4 x 10^11 voxels): now a `u128` (`BrickBox::around_by`; `around` is its narrow form), so a lump 10^20 bricks away is drawn.
  - *Checked by scale.* A flat field, a flat field with the surface through the middle of a row, and a planet's ball give the same node (to the cell) at every level from 2 or 3 to 114 or 115 when everything is scaled together: the radius times 2^k, the cube
    moved and `k` levels coarser; and the explicit layers (grass on top, stone below, one row half full) at every level up to 115. At level 115 the node reaches the end of the world on two axes and the cells past it are air, so the surface shows at its edge: not equal, by design.
  - *The cost of seeing everything.* Every size of cube has a ring of about 19,000 round the viewer with the usual settings, so a view of all the universe at every scale is about 2.2 million nodes (the whole-world walk in `selection.rs`); that is why a view has a
    maximum distance, and nothing sends that many.
  - *Not done, still limits.* No generator in the repository makes anything that needs a level above 40 (a planet may be up to a billion voxels across, a node of level 40 has cells of 10^12), so the levels above are checked with synthetic fields and a planet scaled up in the
    tests, not seen in play; the first generator of a star cluster or a galaxy will be the first to use them. The renderer's depth range at these distances (squares of distance in an `f32` overflow past 10^19) is the open item (b) of `future-goals.md`.
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
- **A character is never left inside what it can touch: every tick, one that something has moved into is moved out the shortest way**
  (`CharacterBody::keep_clear`, after the terrain and the ships' colliders are in place and before the steps; up before the others at one
  distance, in steps of an eighth, at most 8 voxels, else it stays). Found by measuring in 32g: the controller slides a character out of an
  overlap along one collider's surface, with no regard to what is behind it, and the colliders of terrain leave out boxes buried in solid
  terrain, so a ship that fell on a walker pressed them into the floor and they fell down through the world (eyes at 202.6 to 94.6); a ship
  that went faster than about a voxel a tick, or a wall thinner than the walker, left them half in the ground; and a block built over a
  walker's head made the controller shake them 0.45 into the floor and out again every step. The same rule moves a character out of a block
  built where it stands, for terrain and for a ship's blocks. It looks at the voxels for terrain (exact, and sees what is buried) and at the ships' colliders
  for ships, with a box 0.02 smaller on every side so that standing on something, or against it, is not inside it. A wall thin enough to jump
  over a walker in a tick (more than a voxel a tick for one a voxel thick) passes them without touching, which harms no one.
- **The view is the player's own angles in the room the body's frame says** (`PlayerPose::view_orientation`): the entity's orientation is the turn
  of that room and then the angles, so the angles are never converted when a player boards or leaves, and the stream of a player who never
  boards is byte for byte what it was. The camera has a `frame_rotation` and looks at its angles in it.
- **A ship that sleeps has no speed on record, and one that is held has none until whoever moves it says how fast** (`set_grid_held(true)` stops it):
  what a player who steps onto it inherits is its recorded speed at the place (`MovingGrid::velocity_at`).
- **A rule asks about the world through the engine's any-grid calls, and is checked on a ship (2026-10-22):** a fire set on a ship did not burn, because Cave's fire rule found out whether a
  player was near it by asking whether its grid was the root grid of the player's level, which a ship never is, so every fire on a ship waited for ever. The rule's logic was grid-generic and right;
  the question about the world was not, and the engine gave no way to ask it that did not have to be written by hand (block editing had its own copy, correct, 20 lines). Now there is one:
  `WorldData::grid_frame` / `voxel_in_world`, `PlayerPose::distance_to_voxel_of`, `PlayerReach`; fire and block editing both use it. The promise that a mod's rules work aboard is kept by the
  grid-keyed design for what a rule does to voxels, and by these calls for what it asks of the world, and is *tested*, not trusted: `ship_parity.rs` runs a rule's scenario in the terrain and in a
  ship. The level-keyed voxel calls are gone (owner, 2026-10-22: pre-alpha, no modders yet, so be bold): `ServerWorld::{voxel, set_voxel, uniform_value, generated_voxel}` are crate-private and the one way to
  read and write voxels is `ServerWorld::grid(GridId)` and `GridView`, for the terrain of a level (`GridId::root(level)`) and a ship alike. `detach_blocks` takes a grid, so it lifts blocks out of a
  ship as well as out of terrain.
- **A ship that goes away with its pilot comes back where it was, or the player decides (32i):** it is stopped and put in a file that waits for those aboard; when a player who was aboard
  joins (the pilot, or a passenger who was away) it is looked at against the terrain and the other ships at the place it was left, and if it fits it is there. If not, the player is asked
  (move it, break the terrain, break its blocks, put it there anyway, or leave it and go to the spawn); the engine does not guess. A ship that cannot be found asks too, with three other
  answers (the spawn, the air, the title). The look is of the centres of the cells that something can bump into: deterministic and quick, and it finds every overlap deeper than half a
  voxel; it is not a collision test, and the bodies resolve the rest. A ship with another player aboard does not go (they would lose their floor). The questions are not saved.
- **Ships are kept by the distance of the nearest player in their level (32i, `engine/grid_lifecycle`):** moving within the simulation distance, frozen beyond it (held,
  their rules waiting), in a file beyond the unload distance, with a tenth more to leave a state than to enter it so that a ship at an edge does not change every tick.
  The distance is to the nearest brick, compared as squares. The host chooses the two distances and what a pilot's logout does (`ShipSettings`); the defaults are 256
  and 1024 voxels. A server with no player in a level keeps no ship of it awake: each is frozen and then put away, which is what the plan says (a simulation distance with freeze and
  park), and what a test of what ships do must say it does not want (`ShipSettings::everywhere`).
- **A ship is saved as a file of its own, named by the ship and the number of the write, and a world is saved as a set (32i):** the ship files first, the world
  file that names them last, and then the ship files nobody names are removed. The number is a counter saved with the world, not the tick (the first plan): a
  tick begins again with the server, and a new run could write the name that the world file on the disk still names. Whatever a save is stopped by, the world file
  that is there is whole and every ship file it names is there. After a load every ship is in its file; the server brings in the ones that are wanted, and the
  rules hear of them as changes of what is in them. A passenger is saved in the frame of its ship with the place it has in the world, and put there if the ship
  is lost.
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
- Mine, M0 housekeeping (2026-10-30; `architecture-review.md`): **a folder in a pack that nothing reads is an error** (`recipes/` was reported as fine and did nothing; `check-pack` now names the folder and suggests the nearest of `blocks`, `patches`, `bodies`), and **`pack.json` accepts `"format": 1` and a free-text `"version"`**, so a later engine that reads another format can tell which one a pack was written for. The examples in `pack-format.md` are checked by a launcher test, so the guide cannot drift from the loader.
- Mine, M0 (2026-10-30): **a server that panics is noticed and nothing is saved from it.** A panic leaves a tick half done, and a file written from half a tick (the ships' files are shared with the last good save) could replace a good world with a bad one, so the world on disk stays the last autosave. `ServerThread::stop` returns the failure instead of panicking its owner, `has_failed` lets `serve` end by itself, and both say which save is the world. Open: a game played with no `--autosave` saves only on exit, so a failure there loses the session.
- Owner and mine, F5 of `architecture-review.md` (2026-10-30): **Cave owns Cave's ground.** The owner said no world is worth keeping before the first release, so generator versions may change until then (the rule in `CLAUDE.md` says so). What was in `engine/terrain` for a game's sake moved to `games/voxunami_cave/src/generators`: the planet (cube-sphere heightmaps, its coarse nodes by columns, its family and the pack-file side of it), the great rivers, and the overworld's hills. The generators' identifiers are Cave's now (`voxunami_cave:rolling_hills_v1`, `small_planet_v1`, `empty_v1`, `great_rivers_v1`, and the family `planet_v1`), and `base_blocks` is only blocks and their properties (it no longer depends on `terrain` or `data_packs`). **The engine keeps the machinery:** density fields with bounds, noise, the generator and family traits, the registry, the coarse-cell code, and **`SurfaceLayers { air, surface, subsurface, deep }`**, which replaced `TerrainMaterials { air, stone, dirt, grass }` so that the engine no longer names a block (Cave's are grass, dirt and stone; a list of layers with thicknesses is the next step if Cave wants another layer, and not before). The engine's own tests use fixtures under the `test-support` feature of `terrain` (`rolling_hills` as a fixture and a `ball`), not Cave's content. Checked, not assumed: with the five identifiers written back as they were, Cave's golden session gave its old stream hash, length and saved-world hash to the digit, so the move changed nothing but those names.

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
  `docs/history/physics-spike.md`. Built so far for players only (Phases 15 and 16, below); rigid structures, buoyancy and
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
- Rust, wgpu, mesh-based rendering. Levels of detail are chosen by one octree refinement rule by distance for terrain and every grid alike, not by nested shells
  (owner, 2026-10-23, `lod-spec.md`; this replaces the earlier "nested clipmap LOD shells", and "GPU-driven" rendering is built only if measurement shows the CPU is the limit).
- Everything with voxels is a `Grid`: sparse, chunked, with a rigid transform. Terrain is the infinite root grid
  (procedural base plus saved overrides); structures, creatures and tiles are finite grids.
- Voxel coordinates are `i128` per axis within +-2^120 (bricks within +-2^115); rendering is camera-relative. (They were `i64` within +-2^60 until 2026-10-27; see "Coordinates of 128 bits" below.)
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
