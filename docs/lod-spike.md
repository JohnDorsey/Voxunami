# Levels-of-detail spike: how fast can far terrain be made, culled and drawn?

Questions (owner, 2026-10-24): after tight bounds and pruning (`lod-spec.md` 9b), what else would make the far view cheaper? Try whatever is promising, and push the best
of it as far as it goes.

**Recommendation, with how sure I am:**

1. **Make far terrain from columns** where the ground is a height field (the far view of the rolling hills, without caves): one batch of the same noise for a node's 32 x 32
   columns (bit for bit what sampling each column gives), a height for each column, and a mesh made straight from the heights. A node of hills takes **25 microseconds**
   on one thread instead of 13 to 37 milliseconds for the cells and 1 more for meshing them (**400 to 1,500 times**), and a whole view out to 100 million voxels is made from nothing in **0.1 second** on
   12 threads. *Measured; confidence high.* It needs a generator that can say its ground is a height field (a new coarse path, so a new generator version) and it leaves the caves out of the far view, which
   9a already recommended.
2. **Keep the pruning as built**, and later tighten its margin (a few cells instead of a cube's edge) once there is a picture comparison to prove it is safe. *Measured; margin: argued.*
3. **Occlusion culling is worth much more than I thought on this terrain, and no cheap sound method gets it.** In the rolling hills, from a valley or an ordinary place, 99 to 100% of the far surface is hidden
   behind nearer ground; from the highest ridge, half. A horizon made only of what the generator can *promise* (its bounds) is sound and takes 13 ms but culls two thirds from a valley and nothing from an
   ordinary place. What would get it all is a horizon from the heights as drawn, worked out off the main thread and used to **build hidden nodes last**, not to skip them. *Potential: measured with rays. The method: argued, not built.*
4. **Draw with one indirect call over one arena when more than about 5,000 nodes are in view**: 8 to 23 times less CPU time on the RTX 3090 (0.5 ms against 11.6 ms for 25,500 nodes); below that the draws
   as they are cost under a millisecond. The integrated GPU is limited by quads, not draws (5 ns a quad), so there the setting that matters is the pixels a cell is drawn at. *Measured; confidence high.*
5. **Not built, and why:** a ray-marched far field (the noise has no stored max pyramid, so the bounds of a big box are the whole amplitude, and baking one is the clipmap design; for the cost of all the far
   nodes above it is not obviously better than rasterising them), a cubemap of the far field (it saves the far draw, 0.5 to 7 ms, at the price of re-baking every few tens of metres and a depth to composite with; worth it
   only at planet distances), and column meshes read by the vertex shader from a height buffer (a quarter of the memory; a different renderer).

Reproduce: `cd docs/spikes/lod && cargo run --release -- help` (a standalone project, like the other spikes; it uses the real terrain, selection, mesher and renderer). Measured on the owner's machine (Ryzen 5 7600X, RTX 3090,
AMD Radeon integrated graphics, Windows 10), release build, the rolling hills of seed 2024 (hills 40 high, the largest 128 voxels across, six octaves). Counts repeat; times are of one run.

## 1. Pruning, and the size of the problem (`pruning`, `detail`)

The numbers are in `lod-spec.md` 9b. One more table, **the pixels a cell is drawn at** (the quality of the far view), for hills, nodes and bricks (the bricks are the same 2,752 at every setting):

| cell of | to 1,500 | to 20,000 | to 1 million | to 100 million |
|---|---|---|---|---|
| 1.5 pixels | 6,837 | 39,100 | 83,676 | 135,072 |
| 2 | 6,837 | 24,038 | 50,070 | 80,846 |
| **3 (default)** | 4,221 | 12,350 | 24,214 | 37,958 |
| 4 | 3,041 | 7,634 | 14,886 | 23,306 |
| 6 | 1,666 | 3,961 | 7,437 | 11,533 |
| 8 | 1,120 | 2,551 | 4,891 | 7,531 |
| 12 | 547 | 1,311 | 2,571 | 4,063 |

The count falls by about 3 times for each doubling of the cell (the spec said 4; the fine structure of the hills and the shell of kept cubes do not scale), and a cell cannot be finer than 2 voxels outside the ball of bricks, which is why 1.5 and 2
pixels give the same nodes to 1,500 voxels. This is the knob for weaker machines.

## 2. Columns (`columns`, `scaling`, `view`)

A node of hills (no caves), made by: the generator as it is (cells from a lattice of samples, tight bounds); one sample of the noise for each column of cells; a **batch** of the noise (a lattice and a blend along x for each octave, then
along z; the same operations on the same numbers in the same order, so the same bits, checked at 2^40 voxels from the origin); the top cell of each column from its height; the greedy mesher on the cells made from the columns; the mesh
made straight from the columns. Microseconds a node, one thread:

| level | generator today | a sample each | batch | columns | cells + greedy mesh (quads) | direct mesh (quads) | cells drawn like the generator's |
|---|---|---|---|---|---|---|---|
| 1 | 27,474 | 77 | **8.4** | 5.4 | 1,101 (1,084) | **24.6** (1,108) | 99.30% |
| 2 | 37,209 | 77 | 6.8 | 3.8 | 1,125 (1,117) | 25.3 (1,154) | 99.58% |
| 3 | 28,687 | 65 | 6.6 | 3.8 | 1,003 (978) | 23.4 (1,008) | 99.63% |
| 4 | 20,046 | 54 | 6.3 | 4.0 | 941 (833) | 21.9 (839) | 99.81% |
| 5 | 12,837 | 42 | 5.0 | 3.9 | 953 (767) | 20.5 (653) | 99.87% |

- The batch is 9 to 10 times faster than a sample for each column, and the same bits (a test over every level from 1 to 7 and four places, one at 2^40 and one at -2^41 voxels). The mesh made from the columns covers **exactly the same faces** as
  the greedy mesher gives for the cells made from them, apart from the faces that nothing sees (the bottom of the node, and the top of a column that goes on over it; with the neighbours known they are buried), which it does not make. It makes
  about the same number of quads. A second pass (a shift for the division, no call to `ceil`, a bitmask for the merged tops, packed quads) took heights to packed quads from 19.8 to 11.7 microseconds.
- **A node is 25 microseconds end to end** (noise, columns, mesh; 12,005 nodes of levels 1 to 5, with the allocations): 40,100 nodes a second on one thread, **311,850 on 12** (38.5 ms for the 12,005).
- **A whole view from nothing** (the pruned selection, then every node, quads in the packed form the renderer draws):

  | to | nodes chosen | choosing | making, 1 thread | making, 12 threads | quads | memory |
  |---|---|---|---|---|---|---|
  | 1,500 | 4,470 | 4.5 ms | 76.7 ms | **11.8 ms** | 1.4 million | 11 MB |
  | 20,000 | 12,869 | 7.5 ms | 212.6 ms | **31.0 ms** | 3.8 million | 31 MB |
  | 1 million | 24,873 | 10.7 ms | 335.4 ms | **49.7 ms** | 4.7 million | 37 MB |
  | 100 million | 38,619 | 14.0 ms | 443.2 ms | **85.9 ms** | 4.7 million | 38 MB |

  (Beyond 100 kilometres the ground is flat: every octave of these hills is finer than a cell there, so the coarse levels add nodes and almost no quads. A planet with big features would have more.) With the generator it
  is today, one node of this view costs what the whole view costs here.
- `picture` draws these nodes with the real renderer (three views, 1600 x 900): about 9,400 nodes with something in them and 2.8 million quads are made, 2,900 nodes and 0.87 million quads are in the frustum.
- **What it does not do:** caves, overhangs, and any ground that is not a function of x and z. A cube with such ground in it would go the generic way (and a cave world's far view would leave the caves out, as 9a said);
  the same idea would extend to a list of spans for each column (two height fields give a floating island or an arch), at the cost of the mesher walking the lists; I did not build that.

## 3. How much is hidden (`occlusion`, `horizon`)

*The oracle:* for a cube of the pruned view to 6,000 voxels that has surface in it, 64 places on that surface, and a ray from the eye to each through the ground as it is drawn there (the heights at the level of detail of the
distance, with half a cell of room). A first version of this, with the heights at full detail and less room, called almost everything hidden at every distance, and was wrong: a bump of a voxel hides 300 voxels of ground behind it at the angle a far
place is seen at, but far ground is drawn in cells that have no such bump. The numbers below are the corrected ones.

| eye | height | cubes with surface | cubes with all 64 places hidden | places hidden |
|---|---|---|---|---|
| valley (the lowest of 1,600 places) | 11 | 3,177 | 3,176 (**100%**) | 100% |
| an ordinary place | 67 | 3,185 | 3,151 (**99%**) | 100% |
| the highest ridge | 121 | 3,154 | 1,588 (**50%**) | 93% |

This terrain is steep (a hill 40 high on 128 voxels, and finer ones on it): the steepest ground seen in a direction from an ordinary place is a slope of 0.16 in the middle of directions (9 degrees), up to 0.35. A world with gentler
ground would hide less; one with mountains, more.

*The sound horizon* (`horizon`): the cubes of the pruned view to 20,000 voxels are taken near to far; a cube is hidden if its highest possible point (the generator's upper bound over the piece of its footprint) is under the
horizon of every direction it spans, and each cube that is kept adds to the horizon the steepest ray it is certain to stop (the lower bound). A cube is tested and the horizon is made in pieces of the footprint, a quadtree to a depth.
Of the cubes **with surface**, culled (and ms for about 12,600 cubes); every culled cube was checked against the rays and **none was wrong**:

| eye | whole footprint | quarters (2 splits) | 16ths (4 splits) |
|---|---|---|---|
| valley | 38% (5 ms) | **65%** (13 ms) | 66% (15 ms) |
| 40 high | 3% (7 ms) | 30% (30 ms) | 32% (100 ms) |
| ordinary, 67 | 0% (6 ms) | 1% (39 ms) | 1% (182 ms) |
| ridge | 0% (6 ms) | 0% (35 ms) | 0% (237 ms) |

It is sound and cheap and it is nearly useless away from a valley. Why: the horizon it builds has a middle slope of 0.004 where the rays say 0.164. A bound over a piece of 32 voxels or more is the whole range of the finer octaves (the
bound gives up beyond 4 lattice points), so the lowest the ground can be in a piece is far under the ridge it is part of, and the ridge is what hides things. Bounds are good for small regions (that is what tightening them did) and
this question is asked about the ground at the resolution of a cell near the eye. **What it takes is a horizon made from the heights themselves**, a ray for each of a few thousand directions through the ground as drawn (I
estimate 100 to 200 ms; not built), which is not a bound and so can be wrong by a cell: I would use it to **order** the building (hidden nodes last, and never before the rest), with the GPU culling by depth what is drawn,
rather than to leave nodes out.

## 4. Drawing very many nodes (`gpu`)

A node is 157 quads (hills that step every 6 cells), 1920 x 1080, reverse-Z, the shader of the renderer. Three ways: **instanced** (what the renderer does: a vertex buffer for each node, a dynamic offset for its place, one draw each); **arena + indirect** (every
node's quads in one storage buffer, a place for each in another, the CPU culls and writes the arguments, one `multi_draw_indirect`); **GPU culled** (the same, with a compute shader that culls and writes the arguments). All three draw the
same picture (a hash of the pixels agrees on every adapter and both back ends). CPU time to record and submit a frame; GPU time of the passes by timestamp queries (milliseconds); Vulkan.

| nodes in view | instanced CPU / GPU | arena + indirect CPU / GPU | GPU culled CPU / GPU |
|---|---|---|---|
| **RTX 3090** | | | |
| 840 | 0.21 / 0.11 | 0.07 / 0.04 | 0.07 / 0.05 |
| 3,246 | 0.73 / 0.43 | 0.13 / 0.15 | 0.11 / 0.16 |
| 12,800 | 3.74 / 1.73 | 0.46 / 0.59 | 0.28 / 0.61 |
| 25,501 | 11.63 / 3.42 | 1.00 / 1.19 | 0.52 / 1.23 |
| **AMD Radeon (integrated)** | | | |
| 840 | 0.35 / 0.85 | 0.11 / 0.88 | 0.10 / 0.88 |
| 3,246 | 1.29 / 2.70 | 0.18 / 2.65 | 0.15 / 2.66 |
| 12,800 | 6.26 / 9.86 | 0.55 / 9.79 | 0.32 / 9.84 |
| 25,501 | 16.56 / 19.00 | 1.19 / 19.45 | 0.67 / 19.45 |

(DirectX 12 gives the same picture: 16.5 ms against 1.2 ms of CPU for 25,501 nodes on the 3090.) What follows: the draws are **not** the limit for any view the selection gives (9,000 to 12,000 nodes with something in them to 24 kilometres, 2,500 to 2,900 of them in the frustum), which
costs about a millisecond as the renderer draws now; at 25,000 in view the instanced path is 16 ms of CPU and the arena is 1. On the integrated GPU the time is the quads (5 nanoseconds each: 2 million quads, 10 ms) and does not
change with how they are submitted, so there the pixels a cell is drawn at (section 1) and what is hidden (section 3) are the levers. GPU culling on top of the arena saves another half millisecond of CPU and costs nothing on the GPU; it is
not needed before 50,000 nodes. (The 1.3 microseconds a node that I gave you earlier included the readback of the picture and the renderer's own sorting and writing; the draw itself is 0.2 to 0.5 microseconds of CPU.)

## What I got wrong on the way

- The first count of what is hidden said 100% at every distance: the oracle was too strict (full-detail heights, less than a half cell of room). Corrected, section 3.
- The first sketch of the sound horizon used one range of heights for a whole cube, and found 38% from a valley and nothing from an ordinary place; refining the pieces helped only from a valley. I had expected the quadtree to reach the oracle.
- A mutation experiment on `cube_may_show` makes a test run for ever (a case of an exact zero); it is equivalent, and the test it slows is the one that would catch a real mistake.

## What would settle what is left

- Occlusion: a horizon scanned from the heights as drawn, off the main thread, ordering the nodes to build (the potential is measured, the method is not).
- A picture comparison of the view with and without pruning at many places, to prove the margin of a few cells (9b, limits).
- Columns in the engine: a field says it is a height field and gives the heights of a batch of columns; the generator makes nodes from them as a new version (`lod-spec.md`, order of work). The mesher from heights belongs with `mesh_coarse_node`.
