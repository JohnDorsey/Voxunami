# The world and the sky: what was decided

Light, levels of detail, planets, orbits, the great rivers, `/tp`, the sun and its shadows, and the size of the universe (128-bit coordinates). Specs: `lod-spec.md`, `planet-spec.md`, `rails-and-recipes-spec.md`, `great-rivers-spec.md`, `places-spec.md`, `sun-spec.md`, `shadow-spec.md`. Moved from `design-decisions.md`, which keeps what holds for every game; the stories behind these entries are in `history/decision-diary.md`. Status key as in `design-decisions.md`.

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
  colliders only near a free ship, from the stored brick or else the generator, and not at all farther than 4,096 bricks from the frame's origin (a limit set when positions were `f32`, kept now that they are `f64`).
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
- **A rail is part of a ship (a spin and/or an orbit, a circle or an ellipse) and a pure function of the world's clock** (`libm` in `f64`, pinned to the bit), so nothing is integrated
  and nothing drifts. The engine puts ships on their rails in order of their numbers, so a moon follows its planet. A body on a rail is in free fall with what it
  goes round. A rail is a function of whole ticks (`RailTime`), and an orbit's offset is worked out in 128-bit fixed point (`ExactOffset`) and kept as bricks and a rest.
  Limits: a radius of at most 2^120 voxels, a period of at most 2^80 seconds, a phase of at most 2^63 radians.
- **A free ship that nothing disturbs goes on a rail, round the planet that pulls it most (2026-11-02, step B1 of `orbits-and-thrust-plan.md`).** After a second without a push, a move, a hold or
  anything near it, a ship that does not turn and is clear of the planet's extent is given the orbit its centre of mass and its speed are on (`Orbit::from_place`); the bodies hold it there (a fixed
  body), and anything that disturbs it takes the rail off where it is. Observed or not, saved and brought back or not, a ship on a rail is where the clock says, with the same elements. Its gravity
  is the planet's alone (patched conics). The rules and what they leave out are in `orbits-and-thrust-plan.md` ("B1 as built"). A tumbling ship, one inside the extent of a planet and one near
  something stay bodies of the solver's, with the solver's error.
- **A planet on a rail has a sphere of influence** (one and a half times the reach of its ground, at most 120,000 voxels): everything in the level inside it goes with the
  planet, moved by the planet's displacement of the tick before anything looks at the world; ships in it are simulated in a frame that goes with it, and a planet on a
  rail is a kinematic body. The prices (a ship made at rest near a moving planet is going at its speed against it, and so on) are in the diary.
- *Mine:* a star is a body like a planet, a ball of ground of the planet family. Open: bodies are made in the order of their files' names, which makes names such as
  `star_planet_moon` a rule of the pack author; the order should come from the orbits.

**Coordinates and levels of detail**
- A coordinate of more than 64 bits goes on the wire as a length-prefixed integer (a byte for the number of bytes, then the zigzag value); a number has one encoding (owner, 2026-10-27).
- **Coordinates are 128 bits** (owner, 2026-10-27): `VoxelCoordinate` and `BrickCoordinate` are `i128` within +-2^120 voxels, `WorldPosition` an `i128` brick and an `f64` offset (an `f32` one until 2026-11-02),
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
