# Phase 38: levels of detail

Status: designed with the owner on 2026-10-23 (their answers are in section 2); not started. Phase 38 comes **before** Phase 33 (the mod loaders) by the
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

### 3.5 Protocol (version 8; version 7 is the orientation change in 38a)

New server message `CoarseNodes` (node address, the 32³ cells, run-length coded the way bricks are, `Arc` shared) and a client message `SetDetail`
(`cell_pixels`, the field of view and view size or just the resulting ratio, and the download rate in bytes a second). The server derives the set of nodes
a client needs from the client's reported camera (it already knows each player's pose) and `SetDetail`, with integer arithmetic for the node set so that
client and server agree on the rule's result for the same inputs; the client's own drawing uses its exact camera. Sent nearest and largest-error first,
**coarse before fine**, so something is on screen at once; the rate limit applies to the whole. Ships the client should know about are no longer only
those in the watched box (3.7). Replay: the new messages are in the recording like any other; a replay needs nothing else.

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

### 3.7 Which ships a client knows (a gap that this phase has to close)

Today a ship is known only if it is inside the watched box, about 500 voxels. A planet, or a ship a hundred kilometres away that is still big
in the sky, must be known farther away. Rule: a ship is **known** while any of its nodes would be at least a pixel across; a small ship disappears at a
distance that follows from its size, a planet never within any range this game has. A cap on coarseness cannot change that, because below a pixel there
is nothing to draw (decision 3). The server's *simulation* distance for ships (the frozen ships of Phase 32) is a separate thing and does not change.

### 3.8 What this phase leaves room for (planets)

A grid with a **generator** (like the root grid of a level) instead of "every brick stored" is the missing piece for a planet-sized ship: today nothing
generates a ship, and the spawn message sends every brick. Nothing in Phase 38 may assume that a ship's bricks all exist: the node API
says "this node has contents or is empty, or the generator has it". A sphere generator, its coarse sampling, and a ground that is a ship are the next phase.
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
- **38g Meshing nodes.** The mesher takes a node's cells and draws cubes for cells with fill at least half, merging faces as it does now; light from
  the cell. Tests: a flat world's coarse node is one slab; a snow-topped hill's coarse node has snow on top.
- **38h Rendering.** `Placement` scale, new mesh keys, draw coarse nodes, swap on refinement. Offscreen render tests: terrain at several distances; a
  ship with two detail levels at once; existing render tests unchanged.
- **38i Protocol and streaming for terrain.** Version 8: `CoarseNodes`, `SetDetail`, server dirty tracking for edits, the rate limit, coarse before
  fine. Tests: an edit far away appears in the coarse view of another player; replay of a recording shows the same; the rate limit holds in bytes.
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

## 6. Not here

Planets and a ground that is a ship; the sphere generator; the telescope; fading between detail levels; a block per direction (optional upgrade,
`future-goals.md` goal 11); a server-side coarse mass for gravity; stars as voxels (needs emissive blocks); atmosphere and fog tuned for planet distances;
GPU-driven culling (unless measured to be needed); textures.
