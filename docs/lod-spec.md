# Phase 38: levels of detail

Status: designed with the owner on 2026-10-23 (their answers are in section 2); steps 38a to 38i are built (section 4). Phase 38 comes **before** Phase 33 (the mod loaders) by the
owner's decision. Read with `moving-grids-spec.md` (grids and ships), `future-goals.md` ("Ship goals" 1) and `design-decisions.md`. What I checked in
the code is marked **[code]**; what I believe and did not check is marked **[belief]**; the choices are mine unless marked *owner*.

A word that means three things here: **level** alone is already a world level (`LevelIndex`, a dimension), and `region_octree` calls the size of a
cube of bricks its "level" (`ROOT_LEVEL` 56). In this document the new idea is always a **detail level** (`LevelOfDetail`, which exists): detail level `n`
means a cell is `2^n` voxels along each edge. Detail level 0 is full detail.

## 1. What is true today

- **[code]** `voxel_space::LevelOfDetail` exists (a `u8`, 0 to 60, `edge_length_in_voxels() = 1 << n`), and `DensityField::sample` and `bounds`
  already take a footprint of that type, so the generators were designed with coarse sampling in mind. `TerrainGenerator` has one method,
  `generate_brick`, at full detail only.
- **[code]** Voxel coordinates are `i64` within ±2^60, bricks are 32 voxels (2^5) per edge, `WorldPosition` is a brick plus an `f32` offset in it, and
  rendering is camera-relative. A cell of detail level 40 (2^40 voxels, about 1.1 × 10^12 m at a metre a voxel) fits with room to spare.
- **[code]** The renderer draws a brick as instanced packed quads (`voxel.wgsl`), placed by a per-brick `Placement` (an offset relative to the camera and a
  3 × 3 matrix for the grid's turn). Meshes are keyed `(grid, brick)` (`upload_grid_brick`, `remove_grid_brick`). There is no scale in `Placement`. The
  depth buffer is `Depth32Float`. `Camera::far_plane` is 4096 by default and `game_shell/src/view.rs` sets it to twice the view radius.
- **[code]** A client watches one `BrickBox` of full-detail bricks per world level (`ClientMessage::SetInterest`; the server caps it with
  `maximum_watched_bricks`, 33³). The server sends only *edited* bricks; the client regenerates the rest from the seed. That is what makes replay work.
- **[code]** Ships are different: nothing generates a ship's voxels, so `GridsSpawned` is followed by `BrickContents` for **every** brick of the ship
  (protocol version 6). A ship is known to a client only when it is in the watched region.
- **[code]** `Orientation` is `[f32; 4]` and compares bit for bit; 62 files use it (356 mentions). `GridPose` and `EntityPose` hold it.
- **[belief]** Rapier (the physics engine) is built with `f32`. I have not checked. If so, only dynamic ships go through it; a planet would be a held
  ship, which physics does not move.
- **[code]** `design-decisions.md` listed "nested clipmap LOD shells" among the foundations. This phase replaces that (section 3.3).

## 2. The owner's decisions (2026-10-23)

1. The LOD system must work on ships exactly as on terrain. In the future the ground itself may be a ship, so that it can be a planet: a rough sphere of
   cubes in ordinary 3D space, no curved grids; jagged areas where corners point at the sky are accepted. Planets are as large as real ones, up to
   about 10^8 m across, possibly larger. A voxel of 2^32 m an edge (or more) would let stars be voxels that break into smaller ones on approach.
2. Maximum detail level: **40**.
3. A ship can set a **cap on how coarsely it is drawn** (a maximum voxel size), stored as a detail level. That is all it does: it does not stop a ship
   disappearing when it is too small to see (section 3.7).
4. One ship shows **several detail levels at once**, by the distance of each part: falling toward a planet, the nearest part is at full detail first
   and the far parts stay coarse.
5. A configurable download rate is accepted for the data that zooming (a telescope, later) needs.
6. Coarse cells are untextured always. Blocks near the player will be textured one day; a fixed distance, set by a client command, says how near.
7. A coarse cell stores its **dominant block id**, a **fill fraction** and a **light level**. A cell that is mostly stone with some air and lamps is a
   grey cube lit brightly on all sides by the light inside it.
8. The dominant block is chosen **among the blocks in contact with air**, so that a one-block layer of snow is not lost: first version, counting faces
   that touch air (3.2); a block per direction is an optional upgrade (`future-goals.md`, goal 11).
9. At coarse levels light is the brightest child up to one brick (cells up to 32 voxels), and above that an average, so one lamp does not light a
   cube a thousand kilometres wide.
10. Coarse data is **hybrid**: the client derives what the generator gives; the server sends coarse data only where there are edits (or, for a ship,
    always, since nothing generates it).
11. Phase 38 is **drawing only**: nothing in the simulation reads coarse data (so it needs no cross-platform bit identity beyond what replay needs). A
    server-side coarse mass for gravity is a later, separate structure (`future-goals.md`).
12. Planets and the planet generator are **not** in this phase. This phase must not make them impossible.
13. Cracks between detail levels are accepted at first; fading is a later wish.
14. **Reverse-Z with an infinite far plane.** Orientations become **`f64`**.
15. GPU-driven culling only if measurement shows the CPU is the limit.
16. Coarse levels are not saved in the world file; they are derived and rebuilt.
17. Voxel size: Cave needs one size for all grids of a game, so there is no per-grid size. Detail levels are counted in voxels, and the cap is a level.

## 3. The design

### 3.1 Nodes and cells

A **node** is a cube of a grid at a detail level `n`: `32 × 32 × 32` cells, each `2^n` voxels along an edge, so the node is `32 · 2^n` voxels wide and
sits on a multiple of that width. Detail level 0 nodes are the ordinary bricks. A node is addressed `(grid, origin brick, n)`, where the origin is the
brick coordinate of its lowest corner (the existing `region_octree::BrickCube` already names such a cube: its `level` is `n`, and the same words mean
the same cube; this document and the code say `detail_level` for `n` to avoid the clash).

A **cell** summary (`CoarseCell`) has: `block` (the dominant block, `u32` as the game numbers its blocks), `fill` (`u8`, 0 to 255: how much of the
cell is not air; a cell with any block in it has fill of at least 1), `light` (`u8`, 0 to 255, the same scale as the mesher's brightness) and
`exposure` (`u32` fixed point with 8 fractional bits, see below; it is working data, sent with the cell because a parent is computed from it).
Air is block `air` with fill 0. Coarse cells hold block numbers, not names, in memory only; they are never saved, so the "saved files hold only
namespaced ids" rule is untouched.

### 3.2 How a parent cell is made from its eight children (owner's snow problem)

- **fill**: the sum of the eight children's fill divided by 8, rounded up whenever any child has fill (a speck is not lost).
- **exposure**: the area of the cell's surface that touches air, in units of the cell's own face area. At detail level 0, a voxel's exposure is the
  number of its faces that the mesher would draw (a face next to a see-through block) (`BlockTable::is_see_through`); a parent's exposure is the sum of
  its children's divided by 4 (a child's face is a quarter of the parent's), in integer fixed point, saturating. A flat ground one voxel deep has
  exposure about 1 at every level.
- **block**: the block whose children together have the most exposure (the exposure of the children that have that block is added up, so four children of grass with a little surface each can outweigh one of stone with more). Ties go to the block of the lowest octant index, which does not depend on how a game numbers its blocks. If no child has any surface, the block with the most fill.
  A layer of snow one voxel thick on stone is almost all of what touches the sky, so the cell is snow from above. This is a close approximation, not
  an exact answer: a parent sees only its children's dominant blocks, so a thin layer that is not dominant in any child is lost one level up.
- **light**: for parents whose cells are up to 32 voxels (detail levels 0 to 5) the brightest child; above that the average of the children's light
  weighted by their exposure (by fill if none has exposure), because light that matters is on surfaces.
- **drawn**: a cell is drawn as a solid cube when `fill ≥ 128` (half), so that a cell the ground cuts through takes the side the ground mostly is on.
  The cube is lit by the cell's `light` on all sides.

These are the rules I chose to meet the owner's description; the thresholds (half, 32 voxels) are first guesses to be judged by looking at it.

### 3.3 Which nodes are drawn: one rule for terrain and ships (replaces the clipmap shells)

For a viewer at a point `p` (expressed in the grid's own axes by the inverse of the grid's pose, so a ship turned any way works), a node of edge `E`
is **fine enough** when its cell size is small against its distance: `2^n ≤ r · distance(p, node box)`, where `r` is `cell_pixels` times the size of a
pixel as seen from the camera (`2 · tan(field_of_view / 2) / viewport_height`). Otherwise it is replaced by its eight children, down to detail level 0.
With a cell of 2 pixels, a 1080-line view and a 70° field of view, a voxel is split to full detail within about 400 voxels (the owner's target is 256, so the default `cell_pixels` will be about 3); it is a setting
(`.detail`, a client command), not a constant. The rule is the same for the world's terrain and for every ship, and it gives the owner's falling-toward-a-planet
view: near nodes are fine and far nodes coarse, within one grid. **How many nodes** is not "a few per level" (my estimate in the first draft, which was wrong): each
doubling of the distance holds as many cubes as the one before, about `(2 / (32 r))^2` for a surface (and the cube of that for a solid volume), `r` being cells per
distance, so the same number at every level; measured in 38f in section 8.

- **Cap**: a ship's cap `c` forbids nodes coarser than `c`; a grid wider than one node of level `c` is covered by several. The default is no cap. (A planet
with a small cap has very many nodes; the cap is the owner's to set, and a planet should have a large one.)
- **Not drawn**: a node whose projection is under one pixel, whatever the cap (3.7).
- **Zoom**: the data (not only the drawing) must refine with the zoom, but only toward where the player looks, or a tenfold zoom would ask for a
  thousand times the bricks in all directions. So the set of nodes a client needs is: the nodes the rule gives with the *unzoomed* field of view in all
  directions, plus, inside the (widened) view cone, the nodes the rule gives with the actual field of view. A cone-shaped extra, limited by the
  download rate (3.6). The telescope itself is a later item; the rule does not need to change for it.

### 3.4 Where coarse data comes from (hybrid)

- **Unedited terrain**: the client makes it. `TerrainGenerator` gets a second method, `coarse_node(cube, unlit)`, whose default is `None` (as built: the
  default cannot summarize the bricks, because a generator has no block table to say what a face is; an exact summary is what `voxel_meshing` makes of bricks);
  the field generator (`FieldTerrainGenerator`) answers by
  sampling its density field at footprint `n` (`DensityField::sample` and `bounds` already take it), with the surface layering of the generator
  (grass on top, then dirt, then stone) deciding the dominant block. Coarse output of a released generator is as frozen as its `generate_brick`:
  replays must draw the same picture (a hard rule extended, not a new one).
  **This is approximate**: the coarse cell is not guaranteed equal to the summary of the fine cells under it, so a node can change a little when it
  is refined. I think that is acceptable for terrain, and the tests state the measured agreement instead of assuming it (step 38e).
- **Edited terrain**: the server keeps, for each edited brick, its ancestors marked dirty, recomputes them lazily and rate-limited (an edit touches up
  to 40 levels), computing exact summaries from the edited fine bricks and the generated bricks beside them, and sends `CoarseNodes` for the nodes the
  client needs that contain edits. A client uses a node's override when it has one and its own generated cell otherwise, which is right except
  for the moment between the edit and the message.
- **Ships**: nothing generates them, so the server sends the nodes (computed exactly from the ship's bricks). Small ships, wholly within full-detail
  range, send nothing new. A ship with a generator is a later case (planets, 3.8).
- **Never saved**, rebuilt as needed (owner's decision 16).

### 3.5 Protocol (as built in 38i: no version bump; version 7 is the orientation change in 38a)

The two messages are **plugin game messages**, not parts of the protocol: `engine:set_detail` (client to server) and `engine:coarse_nodes` (server to client), the second declared by
the plugin `engine:coarse_streaming` (`coarse_streaming::CoarseServerPlugin`, which depends on `player_presence:players` for where each player is). Recordings carry them like any other
game message, so a replay needs nothing else, and a server without the plugin simply ignores the first.

- `SetDetail { cell_pixels, pixel_size, bytes_per_second, fine_radius, zoom }`: how fine the picture is (3.3), how fast the client can be told, the radius of its ball of bricks (3.6a) and
  an optional telescope cone. Sent when it joins and whenever it changes; 0 bytes a second asks for nothing. Values that make no sense are refused at decode.
- `CoarseNodes(Vec<CoarseNodeUpdate { grid, cube, node: Option<CoarseNode> }>)`, at most 256 to a message: a node is one cell, or runs of cells (equal neighbours cost one run);
  the *exposure* of a cell is working data for making parents and is not sent. `None` takes back what was said.
- **The server** keeps an **override** for every cube of detail level 1 to 12 (cells of 4 kilometres) that holds an edited brick: the generator's node for the cube, patched with the exact cells
  of each half that is known exactly (an edited brick, summarised with the six bricks beside it as the game sees them, or a half that has an override). Edits mark the cube of two bricks
  that holds the brick, and those beside it, dirty; a few dirty cubes are remade each tick, finest first, so that a node is made once from the nodes it is made from; the node only counts as
  new (a new *version*) when what is sent of it changed. Per client it works out the cubes to draw for the brick the client is in (below), and sends the overrides of those it has not sent, **coarsest first**, no
  faster than the client asked (a credit in bytes, topped up each tick; a message's own size is taken off it; a client can save up a second of its rate, or enough for the biggest node).
  Overrides are kept per grid, and only the root grids of levels are handled until 38j.
- **The same cubes on both ends.** Client and server work the cubes out with the same function (`coarse_cells::select_around_brick`) from the same inputs: the brick the viewer is in (taken to be at the
  middle of it, so that they agree whatever the offset), the `SetDetail` values as sent, and the distance, so that a cube is a leaf of the same size on both. The client draws what it works out, leaving out the cubes that hold nothing to see (9b); the server
  follows the selection only to where there is an override (a cube without one is sent nothing, so which cubes the client leaves out does not matter to it, and it does not ask the terrain) and sends the overrides of those leaves.

### 3.6 Meshing and rendering

- `Placement` gets a scale (`2^n`); a node's quads are in cell units, so the same packed quads draw a coarse node. Detailed (sub-voxel) quads are not
  used at coarse levels. Palette colours are the block's colour, with the cell's light; no textures.
- Mesh keys become `(grid, origin, detail level)` (plan requirement 21 reserved the room). A node replaced by its children, or the reverse, swaps
  meshes in one frame; no fading, cracks accepted (decision 13).
- Reverse-Z: the projection maps far to 0 and near to 1 with no far plane, `Depth32Float` is cleared to 0, the depth test is "greater". Fog and
  atmosphere work from distance, not depth, and are unchanged. `Camera::far_plane` is removed.
  Measured in 38b: walls 2 m apart are in order at 10^5 m and 10^7 m, and at 10^8 m the limit is between 4 and 8 m, on `Depth32Float`.
- Drawing cost is one draw per node, as now. **GPU-driven culling is not built** unless step 38l's measurement shows the CPU is the limit.
- Texture distance: a client setting, changed by `.texture_distance`, in voxels. It exists and is tested from this phase; nothing is textured yet, so it
  changes nothing visible until textures exist.

#### 3.6a Bricks and nodes together (built in 38i)

- **The fine region** (`coarse_cells::FineRegion`): the bricks drawn as bricks are a ball of the radius in `SetDetail` round the viewer's brick, plus the whole of every cube of two bricks (a node of
  detail level 1) that touches it, so a fringe of less than two bricks. The selection splits every cube that touches the ball down to bricks, and never splits a cube outside it below detail level 1: the rule
  alone would ask for bricks out to 500 voxels at the default settings, and a client does not hold them. `WorldMesher` in its *held back* mode meshes exactly the bricks the region wants (with the
  neighbours they need loaded, `R + 3` bricks out) and keeps the finished pictures instead of reporting them. The client watches `R + 4` bricks out (`COARSE_WATCH_MARGIN_BRICKS`).
- **Swapping.** What is drawn is a set of cubes that do not overlap (bricks and nodes) and so is what is wanted; they differ as the viewer moves. `coarse_cells::plan_swaps` takes them in **groups**: a cube
  and the cubes of the other set it overlaps (its halves, its quarters, or the cube that holds it) change together, and only when every cube that comes in is ready (a brick meshed, a node made); a cube that
  goes with nothing to take its place goes at once. A property test checks that whatever is ready, nothing is ever drawn twice or dropped, and that with everything ready the result is exactly what is wanted.
  `world_view::LodView` runs it: it asks the mesher for the bricks, makes nodes on threads of its own (from the override if there is one, else from the terrain generator of the middle brick of the cube),
  nearest first, and puts at most 64 cubes on the screen in an update. A node whose override changes is made again and replaced where it is.
- The renderer is told by `MeshUpdate::UploadCoarse` and `RemoveCoarse`. A game turns it on with `Session::enable_coarse_detail(CoarseDetail { maximum_distance, unlit, cell_pixels, bytes_per_second })`
  (the windows and screenshots do when `GameDefinition::coarse_detail()` says so; the default is no), and the fog is then set for the distance drawn (`Session::fog_radius_bricks`).

### 3.7 Which ships a client knows (a gap that this phase has to close)

Today a ship is known only if it is inside the watched box, about 500 voxels. A planet, or a ship a hundred kilometres away that is still big
in the sky, must be known farther away. Rule: a ship is **known** while any of its nodes would be at least a pixel across; a small ship disappears at a
distance that follows from its size, a planet never within any range this game has. A cap on coarseness cannot change that, because below a pixel there
is nothing to draw (decision 3). The server's *simulation* distance for ships (the frozen ships of Phase 32) is a separate thing and does not change.

### 3.8 Planets are ships: grids with terrain generators (owner, 2026-10-23)

A planet is a ship. A ship must therefore be able to **store terrain generator versions** and **send them to the client**, so that the client generates the planet's unmodified bricks itself instead of
downloading them, exactly as it does for the terrain of a level. Phase 38 does not build this (it is Phase 39 in `plan.md`), but it is designed so that it fits, and these are the rules it follows:

- **What a ship gets.** `MovingGrid` gains an optional `terrain`: the generator assignments of the grid (the seed and a `WorldTerrain`-style octree of generator versions over the grid's own brick coordinates) and a
  declared **extent** (a `BrickBox`: the planet's box, since its bricks are not stored). Edits are stored bricks that replace generated ones, as for a level; a brick never stored is generated.
- **The file.** A ship file gets a section `engine:grid_terrain` (version 1): generator ids as namespaced strings (a generator of a plugin that is not loaded is preserved as it was, as for a level), the assignment octree in the
  encoding of `engine:generators` version 2, and the extent. Nothing numeric from a registry is stored (the hard rule), and a released generator version is never edited.
- **The wire.** `GridsSpawned` carries a ship's terrain (generator ids and assignments, and the extent) when it has one; after that the server sends only the stored (edited) bricks, as `BrickContents`, and not every brick. A recording holds the
  ids and replays by regenerating.
- **The server.** `ServerWorld::grid` reads through the generator for a ship with terrain, as for a level's root; saving writes only the bricks that differ from what the generator makes. Bodies get colliders from generated bricks near a body by the same path.
  The mass of a planet is analytic or from coarse cells (a later job: "coarse mass", `future-goals.md` goal 11).
- **Coarse cells.** `TerrainGenerator::coarse_node(cube, unlit)` (38e) takes a cube in the grid's own brick coordinates and knows nothing of levels, so it serves a ship with terrain unchanged. The hybrid rule of 3.4 is the same: the client derives the cells of
  unedited terrain from the generators it was told of, and the server sends cells only where there are edits; for a ship **without** terrain the server sends all of its nodes, since nothing generates them.
- **What this asks of Phase 38 now.** (1) Everything is keyed by `GridId` (the `CoarseNodes` message, the server's store of cells, the client's store), never by "the terrain of the level". (2) The client's source of unedited cells is a **per-grid, optional**
  thing (`CoarseSource`): for the root grid of a level it is the level's terrain; for a ship it is none today and the ship's own terrain in Phase 39. (3) The extent and the "may this cube hold anything" predicate of the selection come from the grid
  (a declared extent and the generator's surface), not from stored bricks. (4) A cube is never assumed to have all its bricks.
- **Phase 39 steps (planned):** 39a `GridTerrain` on `MovingGrid`, the file section, save and load (an unknown generator kept); 39b the server reads through and prunes; 39c the wire and the client generate (and a replay test); 39d colliders from generated bricks;
  39e a sphere generator with analytic coarse cells and the surface predicate; 39f the ground of a level as a ship, and gravity from mass.

Orientation as `f64` (38a) is part of this room: a turned planet's far side has to be placed exactly.

## 4. Steps

Each step ends with passing tests, clippy and rustfmt on Windows and Linux, and is committed. Tests first. The order puts the riskiest piece (38a, a type
used in 62 files) first and alone, so that nothing else is mixed into it.

- **38a Orientation in `f64`** (**built 2026-10-23**, `build-log.md`; protocol 7, ships' and entities' files). Lock current behaviour first: tests of composition, inversion, matrix, yaw/pitch
  and the wire/file round trip with the `f32` values. Then widen the type; saved ships and entities written as `f32` are read as exact `f64`, new ones
  written as `f64` (a format version, per `world-file-format.md`). Physics converts at its edge. Golden sessions that contain ships or entities move:
  each pin gets a justification from measured bytes. This is the step most likely to take longer than planned; I will stop and say so if it does.
- **38b Reverse-Z** (**built 2026-10-23**, `build-log.md`). Test first: two surfaces 1 m apart at 100 m, 100 m apart at 10^7 m and 1000 m apart at 10^8 m draw without z-fighting in an
  offscreen render; and every existing renderer test passes unchanged. Measure and record the real resolution.
- **38c Cell summaries** (**built 2026-10-23**, `build-log.md`). A pure library: `CoarseCell`, combining eight children into a parent (3.2). Tests: the snow layer is snow; a cave wall
  does not outvote the ground; fill never rounds a speck to nothing; light rules at the 32-voxel boundary; ties by octant; saturation; the combination
  of eight is the same whatever order the children are computed in; and a property test that the parent of a uniform region is that region.
- **38d Summaries of stored data** (**built 2026-10-23**). Summarize a `Brick` (or several) to a node: exact. Used for ships and edited terrain. Tests against a direct
  count, on random bricks.
- **38e Generator coarse sampling** (**built 2026-10-23**; the measurements are in section 7). `TerrainGenerator::coarse_node`, the field generator's version. Tests: the agreement of coarse and exact summaries on
  the Cave and 2048 generators, **measured** (the threshold is set from the measurement, and the measurement is written in the build log); the coarse
  output is pinned like `generate_brick` is.
- **38f The selection rule** (**built 2026-10-23**, `coarse_cells::select_nodes`; measurements in section 8). Pure: nodes for a viewer in a grid's axes. Tests: they tile the grid with no gap and no overlap; counts per detail level
  are bounded; a viewer at a point of a 10^8-voxel grid gets fine nodes there and coarse ones far; a turned ship gives the same nodes as an unturned
  one seen from the equivalent point; the cap; the zoom cone; integer arithmetic gives the same answer on both platforms. Measure the node count at
  40 levels.
- **38g Meshing nodes** (**built 2026-10-23**, `voxel_meshing::mesh_coarse_node`). The mesher takes a node's cells and draws cubes for cells with fill at least half, merging faces as it does now; light from
  the cell. Tests: a flat world's coarse node is one slab; a snow-topped hill's coarse node has snow on top.
- **38h Rendering** (**built 2026-10-23**, `renderer::VoxelRenderer::upload_coarse_node` and `upload_grid_coarse_node`). `Placement` scale, new mesh keys, draw coarse nodes, swap on refinement. Offscreen render tests: terrain at several distances; a
  ship with two detail levels at once; existing render tests unchanged.
- **38i Protocol and streaming for terrain** (**built 2026-10-23**, `build-log.md`; section 9). The messages and the server plugin (3.5), the fine region, the swap planner and `LodView` (3.6a), the shell's
  glue, and tests on both ends: an edit far away appears in the coarse view of another player and a player who arrives later sees the same, a recording shows what the live player saw, the rate limit holds in bytes.
  **Cave does not turn it on yet:** the generators' coarse cells are far too slow on its terrain (section 9), which is the next job before it can.
- **Tight bounds and pruning** (**built 2026-10-24**, section 9b; the first two items of 9a's order of work). Cubes of air and cubes of ground with ground all round them are not drawn.
- **38j Ships.** The server sends ship nodes; ships known by size (3.7); the cap stored in the ship (saved: a ship header field, per
  `world-file-format.md`) and set by a `/ship` command. Tests: a big ship at a distance; a small one vanishing at the distance its size gives; the cap.
- **38k Client commands.** `.detail` (cell pixels), `.texture_distance`, and the download rate; each reports its value with no argument.
- **38l Measurement and budget.** Draw calls and frame time at the node counts the rule gives, on the owner's machine, with the numbers in the build log;
  decide on GPU-driven culling from them. The target is in section 5.

## 5. Answers to the open questions (owner, 2026-10-23)

1. The target is 60 frames a second at 1920 x 1080 on the owner's machine, with full detail out to **256 voxels** (not 500).
2. A ship is known while some part of it would be a pixel wide, as in 3.7; a cap does not change that.
3. The coarse light rule of 3.2 stands for now.
4. The texture distance setting and its command are built in 38k and change nothing visible until there are textures.

## 7. What 38c to 38e found (2026-10-23)

- **The agreement of the generator's cells with the bricks' (measured, `world_view/tests/coarse_agreement.rs`).** Rolling hills of seed 2024 (hills 40 high), a few nodes around the surface at each level; "exact" is every brick
  generated, summarised, and made up level by level. For hills **without caves**: the cells drawn alike are 100% at levels 1 and 2 and 99% at 3 and 4; the mean difference of fill is 0.01% to 0.9% of full; the block of the
  surface cells (cells with any surface in either) is the same for 93% (level 1), 88%, 83% and 83% (level 4). **With the caves** of the settings, fill and drawn agree as closely (98% to 100%, differences up to 2.3%), but the block
  of the surface cells agrees for only 78%, 70%, 62% and 41%.
- **The cause is the exposed faces rule, and it is a question for the owner.** The sheets of the caves have more surface than the top of the hill in a cell of 8 voxels or more, so the cells made of the bricks are stone where the
  generator says grass, which is what is seen from outside. The generator's cells are the better picture here; the rule is what is wrong. It matters in the cells made from bricks (the regions with edits, and ships) and not in the
  unedited terrain, which the client makes from the generator; but near an edit in cave country the coarse cells would turn to stone. **Options:** (a) weight each exposed face by the light in front of it, so that dark enclosed air counts for
  little (the cell has a light level already; Cave has no sky light yet, so this waits for it); (b) the optional upgrade of one block per direction (`future-goals.md`, goal 11), which the caves would also disturb, less; (c) count the faces that
  look up and sideways and not those that look down; (d) leave it, and accept stone near edits in caves. I recommend (a) once there is sky light, and (c) until then, but have built none of them: the owner decides.
- **A bug the coarse cells found in the terrain fields.** `ShellCaves` has 0 for its noise when every octave is finer than the footprint, and 0 is the middle of a sheet of air, so from detail level 6 up the whole rolling-hills world
  was air (and a `min` of ground and caves was air). It is fixed (caves that are all finer than the footprint are no caves); full detail, and so every released brick, is unchanged (the pinned brick digests pass).
- **Cost.** A node of terrain from the field takes a few milliseconds to a few tens; the measurement of 4 + 4 + 1 + 1 cubes takes 8 to 14 seconds together with generating all their bricks, which dominates (taken in the test profile, not a
  benchmark). Memory: a node that is not all one cell is 32,768 cells of 12 bytes, 393 kilobytes; nearly every node is one cell. Step 38f measures the node counts that matter.

## 8. What 38f found (2026-10-23)

- **The rule is built as `coarse_cells::select_nodes(extent, &Selection)`.** The viewer is a `WorldPosition` in the grid's own axes (a ship: `GridPose::world_to_local` of the eyes, tested to give the nodes of a ship that is turned
  and moved anywhere). Settings are `cell_pixels` and `pixel_size`; optional parts are the zoom cone, the cap, a distance limit and a `may_hold` predicate (what the grid can hold; for a sphere, the cubes that touch its surface). Only
  IEEE `+ - * / sqrt` and comparisons are used, so every machine agrees (a digest of the cubes of the planet case is pinned). Not integer arithmetic as the first draft said: the answer is the same, and the distances are not whole.
- **Node counts (measured, before any frustum culling), at 3 pixels a cell and a 70 degree view of 1080 lines (full detail to 256 voxels):**
  - a *solid* grid 10^8 voxels across, the viewer at its corner: **180,412** cubes, about 19,000 for each of the levels 0 to 6 (most of it the inside of the planet, which nothing can see);
  - a *sphere* of that size (the cubes that touch its surface), the viewer on top: **25,957** cubes, 3,776 bricks and 1,200 to 1,460 of each larger level, in 4 milliseconds;
  - a *world with a surface* (a slab of 8 bricks thick and a million voxels across): **23,806** cubes, and **57,427** with a 10 times zoom in a cone of 6 degrees (2.4 times as many).
  So `may_hold` must be the surface and not the volume, and **tens of thousands of nodes** (not a few thousand) is the size of the problem before the frustum takes about four in five of them away. That is about 5,000 draws a frame: workable, but
  it is the number that step 38l has to weigh against GPU-driven culling, and `cell_pixels` (a coarser drawing) is the setting that reduces it (the count falls with the square of it).
- **A cone of zoom is exact:** a cube that was chosen without the zoom and that the cone does not touch is chosen with it (tested), and the cone only ever refines.
- **A cap is free:** the walk starts at the cap or the level that covers the grid, whichever is finer, and only goes down, so no check was needed (the one I wrote first was dead, found by a mutation experiment).
- **Mutation experiments:** ten mutants (cap, sub-pixel cull, distance to the middle, zoom touching everything, zoom coarsening, distance limit the wrong way, what the grid cannot hold, an offset in the high half, a strict
  comparison, the zoom pixel used for the cull only) were all caught after two tests were added (the exact boundary of "fine enough", and a zoom of a coarser pixel).

## 9. What 38i found (2026-10-23)

- **The cost of a coarse node is the limit, and on Cave's terrain it is far too high.** A level 1 node of Cave's hills with its caves takes the generator about **375 ms** when the bounds cannot decide its cells (60 ms with a lattice of
  2 x 2 x 2 points in a cell instead of 4 x 4 x 4, which changes what 38e measured); flat ground takes 1 to 2 ms. At the default settings a view out to 700 voxels is about 6,900 cubes, so a Cave view would never finish,
  and the server pays the same for every edit (it starts each override from the generator's node). So `Cave::coarse_detail()` is still none and Cave does not register the plugin. What has to be found before it is turned on: a sampler that
  asks the field about a cell once (or a column once) rather than 64 times, bounds that decide more, a cache of the generator's nodes on the server, and skipping cubes that need no picture (see below). This is the first job of 38l.
- **What was fast and was not:** the nodes of flat ground were 26,630 of 27,805 *uniform*, and each cost 1.6 ms in the generator and 1.5 ms in the mesher: a node of one cell was built and meshed cell by cell. Both have a
  shortcut now (`terrain`: ground that the bounds say has ground all round it and over it is one stone cell; `voxel_meshing`: a node of one cube has the faces of its sides only), each checked against the slow way (on three kinds of field, and by property)
  and by mutation. Sky nodes already took 9 microseconds.
- **Node counts are larger than the "few per level" I wrote in 38f.** At the default settings, out to 1,500 voxels, flat ground has 28,037 cubes (232 bricks, 22,491 nodes of detail level 1, 5,314 of level 2); the sky half of them is empty.
  Most of any real world's cubes are air above or ground below, and only the surface and the caves need a picture: **pruning** cubes that hold nothing (and cubes buried in ground with ground all round, when nothing near them is edited), with a
  predicate in the selection (`may_hold`, which exists for ships), cuts them to the surface shell (built: 9b). It needs the generators to say what is in a cube cheaply.
- **Known limitations, in order of how much they will be noticed:** (1) A cube the server has not yet sent an override for is drawn from the generators, so an edit far away can be missing for a moment when the viewer moves and the
  leaves change, and then appear. The fix is designed: the server sends, with an overridden cube, whether each of its eight halves is overridden too, and the client waits (a few ticks at most) for a half whose parent
  is overridden. (2) Nodes are meshed with no neighbours, so every node has faces on all its sides, including buried ones that are never seen; meshing with the neighbours that the generators give (or pruning, above) removes them.
  (3) The order nodes are made in (nearest first) and the accounting of jobs after a reset are not tested. (4) The `.detail` and similar commands are 38k; for now the pixel size comes from the camera and the height of the picture.
- **Decisions made on the way:** the viewer is taken to be at the middle of its brick, on both ends; the messages are plugin messages and the protocol stays at 7; an override's version only counts up when what is *sent* of it changes (exposure
  changes, which nobody sees, do not make it sent again: measured, a one-voxel dig below the surface changed the exposure of six levels and the cells of two); the server lets a client watch `R + 4` bricks beyond the radius.

### 9a. Where the time of a coarse node goes, and variants (profiled 2026-10-23, not adopted)

Measured on the rolling hills of Cave (seed 2024, hills 40 high, sheet caves), in the test profile, one thread, nodes around the surface. The throwaway code (a counting wrapper round the field, the variants, the
comparison with nodes made from the bricks) is kept as a patch and three files outside the repository; it can be put back for a rerun.

- **One call.** A sample of the field costs 80 to 150 ns and the bounds of one cell 180 to 300 ns (the caves cost about as much as the hills again).
- **The baseline.** A level 1 node with caves takes 262 ms: 1.77 million samples (95% of the time) and 31,800 bounds calls (4%). 84% of its 32,768 cells are *undecided* by the bounds and go to the lattice of 64 samples,
  because the bounds of a noise are the range of the lattice values of the corners of the cells the region touches, which for a region of 2 voxels is still the octave's whole amplitude: the hills' bound is +-78 voxels, wider than the node.
- **Tight bounds change nothing but the time.** The value of an octave cannot move from its value at the middle of the region by more than the steepest slope of the quintic blend (1.875 times the largest difference of two neighbouring lattice values,
  over the cell's size) times how far the region reaches: take the narrower of that and the corner range. The output is identical (the bound only has to be sound), the property tests of soundness pass, and one test that pinned the corner range as the exact bound is the one that changes.
  Hills alone, level 1: 166 ms to 18 ms; with caves 265 ms to 94 ms (the caves keep wide bounds: |noise| - 0.08 near a sheet, and a worst-case slope on three axes).
- **Variants** (ms per node at levels 1 / 2 / 3 / 4, tight bounds on; the surface error is the mean difference in the height of the top of the ground in a column of cells against the nodes made from the bricks of the world *with* caves, in voxels):

  | variant | caves | ms | surface error | cells drawn alike (own field) |
  |---|---|---|---|---|
  | baseline (4x4x4 lattice, layered blocks) | yes | 94 / 201 / 122 / 70 | 0 / 0 / 0.6 / 0.8 | 99.9% |
  | lattice 2x2x2 | yes | 26 / 47 / 27 / 15 | 0 / 0.9 / 1.0 / 1.8 | 98.0% |
  | baseline | no | 18 / 43 / 35 / 21 | 1.3 / 1.5 / 1.7 / 1.4 | 99.9% |
  | lattice 2x2x2 | no | 7 / 14 / 10 / 6 | 1.3 / 1.7 / 1.9 / 2.4 | 99.7% |
  | one sample in the middle of the cell | no | 5.9 / 10 / 7.4 / 4.2 | 1.7 / 1.8 / 1.8 / 1.6 | 98.8% |
  | one sample per cell as a distance, bounds down to blocks of 2 | no | 2.2 / 3.4 / 2.4 / 1.4 | 1.7 / 1.8 / 1.8 / 1.6 | 98.8% |
  | the same, blocks by priority (grass on top, else stone), surface only on top | no | 1.5 / 2.6 / 1.9 / 1.1 | 1.7 / 1.8 / 1.8 / 1.6 | 98.8% |
  | the same, with caves | yes | 3.4 / 4.9 / 3.0 / 1.6 | 1.1 / 1.2 / 1.7 / 2.8 | 94.1% |
  | **one sample per column of cells** (the field is the height of a surface), blocks by priority | no | **0.30 / 0.30 / 0.29 / 0.28** | 1.6 / 1.8 / 1.8 / 1.8 | 99.3% |
  | the same, layered blocks and exact surface | no | 1.0 / 1.2 / 0.8 / 0.6 | same | 99.3% |

- **The caves are the cost twice.** They are the slow bounds, and they are geometry: a view out to 700 voxels is 6,648 cubes of which 3,185 are not uniform with the caves and 734 without, and the meshes have **12.2 million quads against 0.5 million**
  (cave walls inside ground that nobody can see). Leaving them out costs 1.3 to 1.8 voxels of surface height (a level 1 cell is 2 voxels, three pixels at the distance it is chosen at) and the cave mouths in hillsides.
  What a coarse view should be made from is the *outer skin* of the world, which a generator can say (a second, simpler field for far away: the hills without the caves).
- **Material by priority.** For the cells the generator makes, "grass if the cell is the top of the ground, else stone" costs 30% less time than the layered rule and is no different to look at beyond 4 voxels. For cells made from bricks (edits, ships),
  taking the block of highest priority among the children that have a surface (stone < dirt < grass, a number each block would carry) instead of the block with the most exposure raises the grass on top of hills with caves from 80.8% to 89.7% of top cells
  (the cave *floors* are grass too, so "buried grass" is real: 4.8% to 9.7%, 90% at level 4). The caves no longer outvote the grass; a cell of cliff with one grass voxel in it shows grass, which is the accepted loss.
- **What a view is made of.** To 700 voxels: 6,648 cubes, 3,463 air, 2,451 solid, 734 mixed (11%). To 2,000: 38,031 cubes, 19,437 air, 16,349 solid, 2,245 mixed (6%). A mixed node costs about 1 ms to mesh as well. With the column variant and the
  tight bounds the whole 700-voxel view would be about 734 x (0.3 + 1.3) ms, a second on one thread; pruning the air and the buried solid leaves a tenth of the jobs, draw calls and uploads.
- **Order of the work, by what it buys** ((1) and (3) are built: 9b): (1) tight bounds (identical output, 9x for hills, helps brick generation too); (2) a far field for generators and the column fill for fields that are heights (measured in `lod-spike.md`: 25 microseconds a node; a trait method `surface_height`, with the
  per-cell distance as the fallback for other fields, which must then be roughly distances in voxels; a planet's analytic cells in 39e are the same idea); (3) pruning cubes that hold nothing visible (`may_hold`); (4) priority for blocks, with a
  `coarse_priority` property on blocks; (5) a cache of generator nodes on the server, and a faster mesher for nodes that are surfaces of columns.

### 9b. Tight bounds and pruning (built 2026-10-24)

The first two items of the order of work in 9a, built together because pruning needs the bounds to be tight.

- **Tight bounds** (`terrain::noise`, `octave_bounds`). A sample of one octave is a weighted average of the lattice values at the corners of its cell, so it lies between the smallest and largest of the lattice values of the cells a region touches; that is all
  the bound was, and for a region of 2 voxels in an octave of 128 it is still the octave's whole amplitude. Now it is the narrower of that and **the value at the middle of the region, plus and minus the most it can move**: the quintic blend has a slope of at most
  1.875 per cell, so along an axis the value moves by at most 1.875 times the largest step between two neighbouring lattice values (of the cells the region touches), over the cell's size, times how far the region reaches from its middle (summed over the axes, with
  a hair of 1e-9 for rounding). The middle is worked out in doubled coordinates in `i128`, so it is exact at any distance from the origin. The output of every generator is **identical** (a bound only has to be sound); the time falls (9a: 166 ms to 18 ms for a node
  of hills, 265 ms to 94 ms with caves). Tested by property (a bound holds every sample, at every finer level, of regions from one voxel up, up to 2^50 voxels from the origin), by a dense scan of every small region across the borders of the cells of a lattice
  with small cells (a bound made from only some of the cells a region touches fails there, on every axis), by pinning the slope constant to the maximum of the blend's derivative, and by a single voxel being bounded by its own value.
- **`bounds_through_finer(region, footprint)`** (on `FractalNoise` and `DensityField`; a field that does not know says "anything", the default). A cube is drawn at its own level of detail and at every finer one as the viewer comes near, so "is there anything in
  it" has to hold for all of them: the octaves the footprint keeps are bounded as they are, and each it drops may or may not be there at a finer level, so it adds its range or nothing (the hull with 0). `ShellCaves` with every octave dropped has no caves at the
  footprint and may have some at a finer one (the hull with the value of no caves).
- **`TerrainGenerator::may_show(cube)`** (the field generator: `terrain::coarse::cube_may_show`; the default is yes). A cube **may show nothing** when the field is air all through it (`upper <= 0` over the cube) or is ground all through the cube *and the cubes
  round it*, one cube's edge in each direction (`lower > 0` over the cube grown by its edge on every side): nothing of it can be seen from outside, and no air is beside it. `WorldTerrain::may_show` asks the one generator that makes the whole cube (the default where
  nothing is assigned yet) and says yes for a cube with two in it or one whose plugin is missing. Tested on flat ground at the edges (a surface exactly at the middle of a voxel), on plates and cliffs facing every way, on a field whose ground depends on the level,
  and against the real thing: for flat ground, hills and hills with caves, every cube it says no to is a node of one cell, air or stone (0 of 5,598 / 25,229 / 61,812 dropped cubes of flat ground and 4,944 / 23,269 / 58,293 of hills, to 700 / 1,500 / 6,000
  voxels, had anything in them; for the caves 0 of a seventh of the 431 dropped to 700).
- **Where it applies** (`coarse_cells::select_around_brick(brick, view, may_hold)`): a cube `may_hold` says no to is left out with everything inside it. The client (`LodView`) passes "the terrain says it may show something, or an edit is in it or beside it". **Never left out:**
  every cube that has a brick of the fine region in it (a player builds in the air and digs in the ground; the first version left them out, and a test of a block placed three bricks up, which was not drawn, found it).
  The server does not ask the terrain at all: it sends overrides, so it follows the selection only to the cubes that have one (`EditedCubes::holds`), which is cheap whatever the view distance. (Mutation experiments showed that the terrain's say changed nothing the server sent.)
- **Edits** (`coarse_cells::EditedCubes`, kept by the client from the overrides the server sends and by the server from its own): a cube that holds an override, or has one in any of the 26 cubes of its size beside it, is kept. When an override arrives for a place that had
  none, or the last one in a place is taken back, the cubes are worked out again, at most every 10 updates (a flood of edits is one working out).
- **What it buys** (`select_around_brick` at the default settings, 3 pixels a cell, 70 degrees, 1080 lines, a ball of 8 bricks; cubes drawn):

  | to (voxels) | no pruning | flat ground | hills | hills with caves |
  |---|---|---|---|---|
  | 700 | 9,085 | 3,487 | 4,141 | 6,068 |
  | 1,500 | 30,242 | 5,013 | 6,973 | 17,042 |
  | 20,000 | 105,404 | 10,315 | 15,102 | 56,428 |
  | 2 million | 233,248 | 19,595 | 29,062 | 124,992 |
  | 100 million | 340,248 | 27,323 | 40,710 | 182,356 |

  Without caves it is a sixth (flat) to a quarter (hills) of the cubes at a kilometre and a twelfth to an eighth at 100 million voxels, and the number grows by about 1,400 (flat) or 2,100 (hills) cubes for each doubling of the distance. **With caves it is half**: sheet caves 32 voxels apart run through all the
  ground, so every cube underground is mixed and cannot be left out; this is the reason, again, for a far view without caves (9a). Of the cubes kept for hills to 1,500 voxels, 1,831 are mixed, 2,020 are ground (the shell one cube thick under the surface) and 370 are air
  (the shell over it): **half of what is kept is the margin**, and a margin of a few cells instead of a cube's edge would halve the nodes again (it needs care where two levels meet, and a picture compared with the unpruned one to prove it; not done).
- **What it costs** (one thread, release profile). The pruned walk asks about 6,595 (flat) to 18,811 (caves) cubes to 1,500 voxels and 66,000 to 224,000 to 100 million. A question is 40 to 75 ns for flat ground, 130 to 440 ns for hills and 180 to 650 ns with caves (the
  tight bounds); the whole working out is 0.4 ms (flat), 4 ms (hills) or 10 ms (caves) to 1,500 voxels and 2.7 / 13 / 41 ms to 100 million, against 1 ms and 12 ms for the walk with nothing left out. Asking through `WorldTerrain` cost another 1.1 to 1.3 microseconds a
  question, from an octave walk that looked at all eight children at every level (`RegionOctree::all_in_cube`); it now goes down the one that holds the cube, and costs 0.25 microseconds. And the view keeps the answers of the last two workings out
  (`MayShowMemo`, forgotten when the terrain's `version` changes), so moving a brick asks only about the cubes that are new.
- **Mutation experiments:** about 110 mutants over the new code. The first tests missed: the slope constant a little too small, a bound made from the first cell only on x or z, the cell of a position on a lattice line, cave bounds that forgot the octave a footprint
  drops, the edges of the room round a cube (the low sides, one voxel short), a cube judged at level 0, and the bookkeeping of edits; each got a test. Left alone: a hair of rounding that no sample can show (1), and the server working out its leaves when nothing has
  changed (it only saves time).
- **Limits.** (1) The margin, above. (2) A viewer who is inside the ground sees nothing beyond the bricks, as before (nothing beyond is visible from there). (3) A node is meshed with no neighbours, so the faces of ground against ground are made and never seen; the
  ground one cube thick that is kept has them on all sides (9, limitation 2).

## 6. Not here

Planets and a ground that is a ship; the sphere generator; the telescope; fading between detail levels; a block per direction (optional upgrade,
`future-goals.md` goal 11); a server-side coarse mass for gravity; stars as voxels (needs emissive blocks); atmosphere and fog tuned for planet distances;
GPU-driven culling (unless measured to be needed); textures.
