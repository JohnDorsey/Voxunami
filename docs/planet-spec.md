# Phase 39: planets are ships (and the level `mini_cosmos`)

Status: started 2026-10-24. This builds `lod-spec.md` 3.8 (planets are ships: grids with terrain generators) and the part of 38j that a planet needs (a ship drawn with levels of detail), and uses them for one
planet in a new level. What I checked in the code is marked **[code]**; what I believe is marked **[belief]**.

## 1. What the owner asked for (2026-10-24)

A new world level `mini_cosmos` with one planet in it; the default level for now. The planet is **1% of the size of the Earth**, **a ship**, with a terrain generator **like the overworld's but producing a sphere**, its outside
shaped by **a 2D heightmap bent into a sphere**. **LODs are on for it.** Questions about big gaps, along the way.

Answers to my questions (the same day):

- **The bend:** a cube-sphere with the heightmaps blended where faces meet (no seams, little pinching).
- **Gravity:** 10 metres a second a second at the surface. (I had written that the overworld falls at 32; it falls at 5, `GravitySettings::default`, the owner's decision of 2026-10-22. **[code]** I had trusted an old doc.)
- **Caves:** in the bricks, as the overworld has them; **not in the levels of detail**, which are made from the ground without them (`lod-spike.md`: a far view with caves is 100 to 1,000 times slower to make).
- **Levels:** a new world has both, `mini_cosmos` first (where players start) and the overworld second. Saved worlds are unchanged.

## 2. My assumptions (say if wrong)

- **1% of the Earth is 1% of its radius**: 63,710 voxels (a voxel is a metre), 127 kilometres across. The generator is a version of its own, so the number is part of the version, not a parameter of the file.
- The planet is **held** (it does not move or spin), centred on the origin of the level, and the level is empty space around it. Moving, spinning and docking planets are later ("let's decide later", owner).
- The planet's **mass is declared**, not summed from blocks (it has none stored): the mass that gives 10 metres a second a second at the surface under the real G (6.07e20 kilograms). The level's own pull (`GravitySettings::base`) is **zero in `mini_cosmos`**
  (space), so gravity there is the planet's pull toward its centre. That needs a level's gravity to be its own setting, which today is one for the server (a small change, below).
- Players spawn **on the surface at the top of the planet** (+y) and are put aboard it when they stand on it, by the rule that puts a player aboard a ship.
- The sky stays as it is for now (a blue sky and fog): a planet in space wants a sky that goes black with height, which is a renderer job of its own.
- **Planet seed:** the world's; later planets get one from their ship's id.

## 3. The design

**The shape (`terrain::PlanetGround`, built in 39-1).** A voxel is ground when `radius + height(direction) - |p| > 0` where `p` is the voxel's middle from the planet's centre (the corner where eight voxels meet, so the planet is exactly
symmetric under the turns and mirrors of the cube). The value is about how many voxels the middle is under the surface. `height` is the blend of six 2D heightmaps, one for each side of a cube; a heightmap is the overworld's fractal noise (planar),
read at `radius` times the angle across the face (`atan` is the 9-term polynomial of Abramowitz and Stegun 4.4.49, 1e-5 off, and beyond 1 a straight line that touches it, so a face can be read a little past its edge), in whole voxels. A face counts in
a direction if its share of the distance is within 0.15 of the greatest; its weight is a smoothstep of how near it is, and the weights are scaled so that their squares add to one (the blend of independent noises keeps its spread; a plain average
of two has half the variance, and the seams would show as flat bands). Only `+ - * /`, `sqrt`, `abs`, `min`, `max` and `floor`, so it is bit for bit the same on every machine (no `sin`, no `atan`, no fused multiply-add). The planet is
`PlanetSettings::one_percent_of_earth`: radius 63,710, hills 160 voxels tall at the largest (wavelength 1,024, nine octaves down to 4), so that a kilometre of walking changes the height by tens of metres and the hills are as steep per octave as the
overworld's are, and the overworld's sheet caves in the planet's own coordinates (3D noise needs no bending).

**Bounds (built in 39-1).** Over a box, `|p|` is bounded by the nearest and the farthest corner; the height by the noise of each face that can count in the box (found from the box: a face counts only where its distance along its axis is above
the greatest of the nearest distances less the width of the seam), over the box of the heightmap that the box maps to. With one face counting the bound is that face's; with several the weights' squares add to one, so by the Cauchy-Schwarz
inequality the height is within the root of the sum of the squares of the faces' largest values. **Measured:** a brick at the surface has a bound 134 to 151 wide where one face counts and 404 where three blend (so the corners of the cube are looser, and
the cubes round them that may show are more; it costs only work). Sound by a property test over boxes from one voxel to the whole planet.

**The generator (`terrain::PlanetGenerator`).** Bricks come from the ground and the caves: air, and stone below the layers, are decided by bounds where they can be; the layers are read from the value of the ground (grass up to 1 under the surface, dirt
to 4, stone deeper), not from the voxels above as the overworld does, so a cave under the surface does not turn the ground beside it to grass. The coarse nodes and `may_show` come from the ground without the caves, so a cube is "nothing to see" by the
ground alone (a cube wholly under or over the shell of the surface). **A node of a planet** (`planet_node.rs`) is made from **columns along the axis the node is nearest to**: each of the 34 x 34 columns (the node and a ring round it) is walked from an
estimate of where the ground ends in it, asking the field at the middle of each cell until the cells are clearly air outward and clearly ground inward. A cell's fill is the value of the field at its middle over the width of the cell across the surface
(`edge * (|nx| + |ny| + |nz|)`, the extent of a cube along the normal), clamped; blocks and surfaces are those of `coarse.rs`, whose layering now takes the step to the cell over a cell (`up`) and where the fills come from, so that the layers look outward
along the axis, not up. A node that reaches the plane through the centre of the planet is made by asking about every cell. **Measured (release, one thread, the real planet, nodes at the surface):** 0.5 to 1.0 ms a node at levels 1 to 10 and 6 ms a brick.
The first version walked a fixed point along the column and failed where the surface folds over the column (steep hills, a column at 40 degrees or more to the surface): a search from outside found a root 6 voxels away. The walk asks about each cell, so
it does not depend on the ground being a graph over the axis, and it agrees with asking about every cell of every column in 0.0% to 0.3% of the cells with a surface in them on the real planet (1.6% on a small planet whose hills are twice as steep per
octave). **Accuracy against the voxels** (the node's cell drawn when most of the voxels under it, on a lattice of 4 x 4 x 4, are ground): wrong in 17% of the cells with a surface in them at level 1 (a 2-voxel cell is one question about eight voxels) and 3% to 6%
at levels 2 to 6; the lattice of 4 x 4 x 4 points of `coarse.rs` takes 20 to 29 ms for the fills of one node of this planet against 0.4 to 0.5 ms for the columns (40 to 55 times as much), and was not measured against the voxels here.

**A ship with terrain (`world_storage`, built in 39a).** `MovingGrid.terrain: Option<GridTerrain>`: **one generator version** (a `GeneratorId` of the world's table, which all levels share), the **extent** (a `BrickBox` in the ship's own coordinates) and the
**mass** it declares (`MovingGrid::mass` is set to it; the centre of mass is the ship's own field, the origin for a planet). I had planned a `WorldTerrain` with assignments over the ship's bricks; a planet is one generator version over one box, and a ship
that wants another version is another generator id, so the section is version 1 and carries a name, a box and a weight, and assignments can be a version 2 if a moon ever needs them. A brick that is not stored is generated; an edited brick is stored in `voxels`
and wins. `brick_extent` / `world_brick_bounds` are the hull of the extent and the stored bricks, so the bounds a parked ship is listed with, the leave rule and the box that replication meets are the planet's. The ship file gets the optional section
`engine:grid_terrain` (version 1: the generator's identifier, the box as six zigzag varints, the mass as `f32` bits; a damaged one, a mass that is not a weight, corners the wrong way round or a brick out of range, is refused; a ship that nothing generates
has no such section, so its file is byte for byte what it was). The generator is written by name and read by `ShipFormat` through the reading world's table (`engine:terrain_generator`, like the content of a channel): a world without the generator refuses the
file saying which (`UnknownContent`), and a generator of a plugin that is not loaded is a placeholder in the table, so the planet is kept and saved as it was.

**The server (built in 39b).** `ServerWorld` reads through the generator for a ship with terrain: `moving_voxel`, `moving_generated_voxel` and `moving_uniform_value` (what `GridView::get`, `generated` and `uniform_value` call for a ship) take the stored brick if there is
one, else the brick the generator makes if it is in the extent, else the channel's default. The bricks a planet's generator makes are remembered with a level's (`GeneratedBrickCache`, now keyed by `GridId`, so a level's root grid and each ship have their own), counted by
`bricks_generated`, and a generator whose plugin is missing is `AccessError::TerrainUnavailable` naming it. An edit of a brick that is not stored stores the generated brick with the voxel changed (as a level's terrain does), noted for the clients as a created brick; setting a
voxel to the value it has stores nothing. The bound of the ship is its extent (39a). **Until 39c a planet is left out of `ship_views`** (the wire cannot say what makes its bricks, and sending only its stored bricks would show a client the edits floating in nothing); 39c
sends the terrain once, with `GridsSpawned`, and after that the **stored** bricks only (a recording holds the generator ids and replays by regenerating).

**The wire and the client (built in 39c).** Protocol 8: `GridState` carries `terrain: Option<GridTerrainState { generator: u32, extent: BrickBox }>` (the generator as a number of the world description's table, as in an assignment; the mass is not sent, nothing on a client reads it). The server tells of a planet
as of any ship, with the terrain, and then sends the **stored** bricks only; `ship_views` no longer leaves it out. The client (`ClientGrid.terrain: Option<ClientGridTerrain { generator, extent }>`, translated through the same translator as an assignment) makes the rest:
`world_view::TerrainSource` (a trait that the mesher of the bricks of a level reads from, which `LevelData` and a ship's `ShipTerrainSource` now both are; `known` is a stored brick, or the empty brick beyond the extent, and `generator_for` is the generator) lets a `WorldMesher` do for a ship what it did for
a level, and `ShipTerrainMesher` is that mesher saying what it draws as bricks of the ship (`UploadGridBrick`), under the ship's pose. The `Session` keeps one for each planet, feeds it the camera in the planet's coordinates (`GridPose::world_to_local`), sends the changes of its stored bricks to it
and not to the mesher of ships (which no longer meshes the stored bricks of a planet alone, without the neighbours the generator makes), aims the crosshair through it, and waits for it when it settles. The view summary a replay is compared with names the generator and the extent. A brick beyond the extent is empty and the face of the brick against it is drawn,
so a planet cut off by its extent shows its side. **Golden sessions:** the four that pin a stream or a recording (2048, Eels, Cave, and the end-to-end recording) moved once, for the version in the welcome; each was checked by running the code of the commit before with only the version raised to 8, which gave the new hash and the same length, so none of them tells of a ship.
**Not yet:** the levels of detail of a planet (39e).

**Bodies (built in 39d).** A walker meets a planet through the generator. `grid_bodies::collision::GeneratedHull` is a `CollisionSource` that reads a ship's voxels through the world (`ServerWorld::moving_voxel` and `moving_uniform_value`, now `&self` and public: the bricks a generator makes are
remembered behind a cell, so a body can meet several ships at once while the world is borrowed shared). `ShipSource` is that or the `ShipHull` that reads a ship's stored voxels directly (kept as the fast path of an ordinary ship: the two agree for a ship without terrain, and a test cannot tell them apart,
which is why one mutant of the choice stays). `SolidAt` is a blanket trait on `CollisionSource` that replaces `ShipHull::solid_at`, so the rules of being aboard (`stands_on`, `has_left`, in Cave's `aboard.rs`) take either. Cave's `Blocks::source` chooses by `ship.terrain`, and `step_in_a_level` (the ships near
a walker, as foreign grids; standing on one; the leave rule) and `step_aboard` use it. What a block is to a body and what a whole brick is to it are two functions shared by `WorldTerrain` and `GeneratedHull` (`collision_of_voxel`, `hint_of_brick`), with unit tests; the hint no longer asks about orientations
(whether a shape is empty, a whole cell or part of one does not depend on how it is turned), which changes nothing but that a brick of one block in one state and several orientations is known at once. A planet has no body of its own: `grid_bodies` leaves a ship with terrain out of its bodies, its weighing (the
mass is what the terrain declares, `ServerWorld::set_grid_terrain`) and its splitting and vanishing (its stored bricks are edits of a ground that is not stored, so "no blocks left" and "in pieces" mean nothing for it). A walker dropped over a planet lands on it, is aboard it (in its coordinates), walks on the ground the
generator makes, and falls into a hole that is dug (the edits are stored bricks); a slab put on it upside down is a ceiling slab to a walker.

**Gravity (built in 39d).** A level's own pull: `ServerWorld::set_level_gravity` (where it is not `GravitySettings::base`: none in space), `ServerConfig.level_gravity` by the level's identifier (a level the world lacks is ignored), and the game's `GameDefinition::{gravity, level_gravity}`. Cave: the base is 5 as it was,
`PULLING_MASS = 10^19` kilograms is the least mass of a ship that pulls (a moon, not a raft), and `voxunami_cave:mini_cosmos` has none. The planet's declared mass is M = 10 R^2 / G = 6.08 x 10^20 kilograms for R = 63,710 and the real G, which pulls 10.004 m/s^2 at 15 voxels over the ground
(measured in a test; the test asks for within a tenth). The field aboard is a vector fixed in the planet's axes, as 2.9 of `moving-grids-spec.md` says; the leave rule of a body aboard (a box of bricks) is the extent of the planet.

**What it costs (measured, release, this machine).** A walker meeting the planet for the first time makes the server generate about 50 bricks at 9 ms: joining and settling took 480 ms, against 20 ms with no planet there (five runs, 479 to 486; three runs, 19 to 20). Walking on, a tick is 0.05 ms at the median and 0.1 ms at
p99, and crossing into a new row of bricks costs one tick of 37 to 44 ms (about the size of the bricks of ground and stone below at 6 to 9 ms each; I did not count them). That is the whole of the hitch in 400 ticks of walking, and it is not spread over ticks (as 32h did for the terrain of a level). **Not done:**
(a) spreading the first contact and the crossings over ticks; (b) a rigid-body ship (`grid_bodies`) has no collider against a planet, **verified:** a ship of 4 x 4 x 4 stone let go 40 voxels over the planet accelerated at 10 m/s^2, went through the surface and came to rest on the hills of the overworld 320 metres lower (the colliders that
follow a ship are the level's terrain, and a planet is not that); (c) every brick below the surface that was probed (6 columns of 50 bricks each, from the surface to 1,600 voxels down) has air in it from a cave, so a brick of the planet is hardly ever one block and the hints seldom save a scan.

**Levels of detail for a ship (38j, 39e).** The client makes the nodes of a ship with terrain from its generators, in the ship's own cubes, as it does for the terrain of a level; the **viewer is taken into the ship's axes** by the inverse of the pose, the
selection's `may_hold` is the shell of the surface, the fine region is a ball of bricks round the viewer in the ship's bricks, and the nodes are drawn by `upload_grid_coarse_node` under the ship's pose (the renderer has that). A ship is **known**
while it would be a pixel (3.7 of the LOD spec); a planet is known from anywhere in its level.

**The level and the game (Cave).** `voxunami_cave:mini_cosmos`, the generator of an empty level (all air), a planet made when the world is made; a new world has it first. An overworld-first constructor is kept for the tests that start in the overworld.

## 4. Steps (each ends with tests, clippy, rustfmt, mutation experiments, docs, then a commit)

- **39-1 The planet field and generator** (terrain): the shape, bounds, the generator, the planet's node from columns; tests (sphere, seams, determinism pinned, bounds sound, nodes against the voxels). **Built 2026-10-24** (section 3 says what it is and how it measured).
- **39a `GridTerrain` on `MovingGrid`**, the file section, save and load. **Built 2026-10-24.**
- **39b The server reads through** and the replication of a ship with terrain. **Read-through built 2026-10-24; the replication is 39c's.**
- **39c The wire (protocol 8) and the client generates.** A replay test. **Built 2026-10-24.**
- **39d Colliders from generated bricks; gravity of a level; the planet's mass; boarding and leaving on a planet.** **Built 2026-10-24.**
- **39e Levels of detail for a ship with terrain.**
- **39f The level `mini_cosmos` in Cave**: the empty generator, the planet, the spawn, the default; the tests that start in the overworld; documents.

## 5. Questions

Asked and answered, section 1. Others will be added here as they come up.
