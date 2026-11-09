# The sun's exact short ray (contact trace)

Status: **a design, nothing built** (2026-11-09). The spike that justifies it is `contact-shadow-spike.md` (read its tables first); the owner's wishes it serves are in `future-goals.md` ("Shaders", 2026-11-09): a default shader that is robust, fast and has **no gap at the base of a shadow**, 60 fps on the integrated GPU, and shaders that players can swap. Where this says "measured" it is the spike's; where it says "I believe" nothing was run.

## 1. The rule

A shadow map decides by comparing depths one texel wide, and the bias that keeps a thin floor from shading itself leaves a lit strip of one to two texels at the foot of every wall (the spike: 0.07 to 0.57 voxel by cascade, the same on all four adapters; and the same strip as a leak into closed caves). The strip is a **contact error**: it only matters where the occluder is within a few cells of the place. So:

> For a place on a face, walk the sun's ray through the voxels for up to `K` cells, starting in the cell in front of the face. If a cell that blocks the sun whole is entered, the place is in shadow. Otherwise the shadow map decides. `light = trace_clear ? map : 0`.

The start in the cell in front of the face means nothing is eased toward or away from anything: a wall beside the place is in the way or it is not, which is a fact about the lattice and not about floating point (spike: zero lit pixels at a wall's foot at every sun from 5 to 88 degrees, on all four adapters, with 6 cells). What "no gaps" then means, as a test: **for every place on a face of a whole opaque cube, within `K` cells of the sun-ward occluder, the shader's light equals a CPU reference tracer's.** Beyond `K` cells the map's own softness (a texel, at a silhouette far from the place) remains, and is not a gap.

## 2. What the code already gives (verified by reading)

- The renderer never sees voxels: `MeshUpdate::Upload { coordinate, mesh }` (`engine/world_view/src/mesher.rs`) carries the mesh only. The client's `WorldMesher` meshes a brick on a worker thread **only when the brick and its six neighbours are present**; `GridMesher` and `ShipTerrainMesher` do the same for ships and for planets, under the grid's pose (planets are ships with a generator: `planet-spec.md`; the screenshots of 2026-11-09 are at y of about 63,700 voxels, over a pole, which is where a planet's surface is and not where a flat level's would be; I did not check Cave's world file).
- `MeshedBrick` already holds a `ModelId` for every voxel of the brick (`models`), and `BlockTable` answers `is_solid`, `is_opaque_cube`, `is_sight_through`, `is_light_through`, `transparency`. A shape is a **bitmask of 8 by 8 by 8 sub-cells** (`Shape::occupied`), so partial blocks fit an exact walk too.
- **A trap:** `mesh_brick` returns an empty mesh for a brick that is all air *and* for one that is solid and buried (the fast paths), and the renderer then holds nothing for either. The spike's volume had every voxel; the real one must say **Full** for a buried brick, or the ray walks through the rock of a cave and the leak is back. So occupancy is not a by-product of the mesh: it is sent for every brick that is meshed, empty ones included.
- A brick's mesh and its occupancy can leave the same job together, so they arrive in the same `apply_updates` and neither is ahead of the other.

## 3. The data

### 3.1 Per brick (CPU, `voxel_meshing`)

`BrickOccupancy`: `Empty` (all air or nothing that blocks), `Full` (every voxel blocks), or `Mixed(Box<[u32; 1024]>)` (one bit per voxel, x fastest, in words of 32: the layout the spike uses, 4 KB). The bit is set when the voxel's model **blocks the sun whole**: for phase 1, `is_opaque_cube`. Everything else is clear (phase 3 adds the rest, 5.3). It is computed in the same worker job as the mesh, from `models`, and costs one pass over 32,768 voxels that the mesher makes anyway. `MeshUpdate` gains `UploadOccupancy { grid, coordinate, occupancy }` (and a remove), emitted for **every** meshed brick, so the renderer's table is as complete as the mesher's view.

### 3.2 On the GPU, per grid

One **window** per grid that has bricks in view (the level's root grid; each planet; later each ship): a toroidal table of 32 by 32 by 32 `u32` entries (128 KB), indexed by the brick coordinate in the grid's own frame modulo 32 (the view radius is at most 16, so 33 bricks across would be needed only at the extreme; 32 holds radius 15 and the radius-16 case needs 64: decide with 4.2). An entry says `UNKNOWN` (0: nothing known, which is clear), `EMPTY`, `FULL`, or a slot of the **pool**, a storage buffer of 4 KB slots (a free list on the CPU). When the window moves with the camera the CPU clears the entries that left it before reusing them (a stale entry would be a wrong shadow); a brick's pool slot is freed when its entry goes. Per window, per frame, a small uniform: the window's origin brick relative to the camera in world axes (as `occupancy_origin` in the spike), the grid's world-to-local rotation (a ship's `placement.rotation` transposed; the identity for the root grid), and the brick coordinate of the window's centre.

### 3.3 Memory (an upper bound, not a measurement)

The owner's screenshots read "drawn 4585/16300 bricks" at the default view radius of 12; I take that as drawn over drawn-plus-culled, about 16,000 bricks and coarse nodes holding geometry (I did not check what the title counts). If all were `Mixed` the pool would be 65 MB at one bit a voxel; the buried and empty bricks cost an entry, and many surface bricks are mostly air or mostly rock, so I believe the real figure is a fraction of that, but **phase 0 measures it** (how many bricks are Empty, Full and Mixed at radius 12 in Cave, in a cave, over the planet, and in a ship's yard) before the pool's size or the sub-brick option (7) is decided. A storage buffer binding is limited to 128 MiB by default in `wgpu`, so the pool is one buffer up to that size.

## 4. The shader

### 4.1 Which volume, in which frame

The fragment knows its placement (the vertex stage has it), so the placement carries a **window number** as a flat varying; a face is traced in the window of **its own grid**. The ray is turned into the grid's frame (`to_local * sun_direction`, and the place relative to the window's origin turned too), so a planet that spins under the camera is walked in its own lattice. Grid-local positions near the window are a few hundred voxels from its origin, where `f32` is good to about 3e-5 of a voxel; **the exactness does not depend on that**, because the start cell comes from the face's normal and its own plane (`floor(place + normal / 2)`), not from the position alone.

### 4.2 The walk

A 3D DDA as in the spike, with a **second level**: when the brick entered is `EMPTY` the walk jumps to where the ray leaves it (one step instead of up to 32); `FULL` is shadow; `UNKNOWN` ends the walk clear. A budget of steps and a longest distance bound the work, so the cost depends on how far the walk is allowed and not on the world. The ties of a ray along a lattice plane break in a fixed order (x, then y, then z), which the CPU reference repeats; tests keep probes off ties.

### 4.3 How far (`K`)

Enough to cover the map's strip at the place's cascade. The map's strip is `T * (cos(el) + 1.5 sin(el))` across (`T` the texel of the cascade that is looked up), and a steep ray climbs about `tan(el)` layers for each unit of it. Measured: with `T` from 1/16 to about 1/4 voxel, 2 cells settle every sun up to 60 degrees and 6 settle every sun tried (to 88 degrees, a wall 4 high). The rule to build is **`K` from the cascade index and the sun's elevation, bounded** (my first guess: the strip's length times `tan(el)` plus 3, between 3 and 24 cells); the test matrix of 6 fits it, the build does not trust the guess. A place whose cascade is beyond what level 0 covers uses phase 5.

### 4.4 Cost

Measured on the spike's flat 36 KB volume, 1280 by 720, about **0.08 ms per cell searched on the integrated GPU** and under 0.1 ms in all on the RTX: 6 cells are about 0.5 ms of a 16.7 ms frame. A real pool will not fit in cache, so this is a best case (I believe); the empty-brick jump should reduce the steps for long walks (I believe, unmeasured). Budget for the build: **at most 1 ms on the integrated GPU at 720p** in a scene with many contact edges, measured, or the step budget comes down.

## 5. What is traced, and when

1. **Phase 1 (whole opaque cubes).** What the spike does, for the root grid and for planets.
2. **Ships and boxes as occluders (later).** Each ship near the camera has its own window; a face is traced against its own grid's window and then against the few nearest other grids (the ray turned into each pose; at most 4 to 8, by distance); an entity's box is an analytic ray-box test against a short list in the frame uniform. A ship's foot on a planet is a contact like any other.
3. **Partial blocks and cut-outs (phase 3).** A second bit plane marks voxels that are not whole opaque cubes (`complex`); a hit in one asks the block's **shape** (the 512-bit mask, a table indexed by `ModelId`, about 64 bytes a model) with a walk through the eighths of the voxel, and for leaves and glass the **picture's alpha at the entry point**, the rule the cut-out shadows already have (`shadow_cutout_fragment`). Until then these keep today's strip. (This is the part with most room for being wrong: the voxel's model id must reach the GPU for complex voxels, and a model's state or orientation changes its shape.)
4. **Coarse levels (phase 5).** The map's strip is about as wide as a cell of the level that is drawn there (texels follow pixels and cells follow pixels), so the same short walk in the **cells of the node's own level** (`CoarseCell::fill` thresholded the way the coarse mesh is drawn) removes it, with a constant `K`. Not designed further here.

## 6. Tests, in the order they are written (tests first, as the project works)

1. **`BrickOccupancy` (unit and property):** a bit is set iff the voxel's model is an opaque cube, for random bricks and tables; `Empty` and `Full` equal "every voxel is" (and the uniform fast paths agree); a buried brick has occupancy and no mesh; the occupancy of a brick is the same however the neighbours are.
2. **The reference tracer (CPU, `world_view` beside `cast_ray`):** `sun_reaches(window, place, face, direction, cells)` in exact integer cells with the same tie order as the shader; checked against sampling along the ray at 1/1000 voxel on random soups of voxels, and against `cast_ray`.
3. **Window and pool (CPU):** moving the window clears what left; a slot is freed once; unknown is clear; full is shadow; the toroidal index never aliases two bricks in the window.
4. **The shader against the reference, on every adapter (`VOX_REQUIRE_GPU=1`):** the spike's block scene and closed room become permanent tests with the whole matrix (suns 5 to 88 degrees, cascades by camera height, strip and holes, room leaks); then a **random scene test**: random voxel soups in a few bricks, probes at the centres and offsets of faces, aimed narrow cameras as `pixel_straight_through` does, every probe's light against the reference tracer, with a debug output that shows the trace's result alone. A planet scene: the same under a turned pose.
5. **No false shadows:** the existing "floor nothing stands on is as bright" test, and the 15 shadow tests, pass unchanged with the volume on.
6. **Mutation experiments** (`PROPTEST_DISABLE_FAILURE_PERSISTENCE=1`, one harness at a time): start in the voxel behind the face; the wrong axis at a tie; `K` one fewer; unknown solid; `Full` taken as clear; the empty-brick jump one brick short; the window not cleared on a move; the grid's turn not applied; the occupancy not sent for a buried brick; the mesh and occupancy not in the same update. All must be killed.

## 7. Phases (each ends with tests, clippy and fmt, as `CLAUDE.md` says)

- **0. Measure** (small): counters in `WorldMesher` for Empty, Full and Mixed bricks, printed by a test or a flag; decide the pool and the radius. Exit: the numbers in `contact-shadow-spike.md`.
- **1. The CPU side:** 6.1, 6.2, `MeshUpdate::UploadOccupancy` from the three meshers; nothing on the GPU. Exit: tests pass, no picture changes.
- **2. The renderer:** windows, pool, per-grid uniform, the shader walk with the empty-brick jump, `K` by cascade, 6.3 to 6.6; promote the spike's tests; **remove** the spike's `set_occupancy` and its hand-filled box. Exit: the matrix, the room and the planet scene at zero holes and zero leaks on four adapters; cost within the budget.
- **3. Partial blocks and cut-outs;** 4. **ships and boxes;** 5. **coarse levels;** each with its own tests and a measurement.
- **Optional: sub-bricks** of 8 by 8 by 8 (64 bytes) with an empty/full flag in a table, if phase 0 says the pool is too big (a mixed surface brick may need a third of its 4 KB; I believe, unmeasured). It also speeds the walk.

## 8. For swappable shaders

The engine owns the volume and the answer; a pack gets **`sun_visibility(place, normal) -> f32`** (map and trace together, as `light` above) and, later, a bounded **ray query** `trace(origin, direction, longest) -> hit`, and writes only how a surface is lit with them. A pack therefore cannot bring the strip back by choosing its own bias, which is what packs of other games do each for themselves. The volume would also give exact sky cover (replacing the heightfield of `shadow-spec.md` 8, whose approximations the spec lists) and cheap ambient occlusion to a pack, at no new data cost; neither is designed here. A pack's loops can still hang a GPU: the engine's queries are bounded, a pack's own are its author's (to be thought about with the pack format).

## 9. Open decisions (my recommendation first)

1. **Composition: `min(map, trace)`**, not the trace alone within range. The map has no false shadows by design (the bias is toward lit), so the minimum is exact where the trace reaches and the map's own where it does not.
2. **Radius of the volume: the whole view radius,** if phase 0 says the pool is affordable; else a smaller radius with the strip returning beyond it (one to two pixels, and the owner has said none).
3. **Unknown bricks are clear** (the map decides): the cost is a leak at the edge of the loaded view, where streaming is still arriving; the alternative, unknown as solid, would darken the edge of the world.
4. **Phase 1 includes planets,** since Cave's ground is one; a version for the root grid only would not show in Cave.
5. **Hard shadows only.** The owner has no preference; a few rays across the sun's disc would give a penumbra that is zero at contact, at several times the cost.
