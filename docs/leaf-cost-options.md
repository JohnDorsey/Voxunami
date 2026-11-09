# What leaves and glass cost, and the options to cut it

**A proposal; nothing here is built.** Written 2026-11-09 after the benchmark of cut-out shadows (`block-textures-spec.md`, "Leaves and glass"; `build-log.md`, "Leaves and glass (2026-11-08)"). Not scheduled: it is on the owner's unscheduled list (`future-goals.md`).

## What was measured

A throwaway GPU test (removed again), 1280 by 720, all four adapters, camera and sun moving, 30 batches of 10 frames: 36 solid canopies of leaves of 9 by 9 by 5 voxels (99,846 quads), against the same volume as one opaque block (270 quads, merged).

| Mean frame, ms | leaves, shadows off | opaque canopy, shadows off |
|---|---|---|
| AMD integrated GPU (Vulkan / DX12) | 3.2 / 3.0 to 3.1 | 0.47 / 0.43 |
| RTX 3090 | 0.11 to 0.14 | 0.04 to 0.11 |

So on the integrated GPU the leaves cost about 2.6 ms more than the same volume as an opaque block; on the RTX they cost almost nothing extra. The cut-out shadow pipelines are not the problem (0.01 to 0.09 ms over the plain shadows). The scene is one dense blob, not a tree: a real tree is a thin shell of leaves and costs a fraction of this. **That the cost is worth cutting is therefore not established**; it is a worst case.

## Why there are so many quads (verified by reading the code)

A block with `sight_through` is not an opaque cube, so the mesher does not send it through the greedy path (`mesh_whole_faces`, which merges runs of identical faces into 8-byte `Quad`s). It goes through `mesh_partial_blocks` and becomes 16-byte `DetailedQuad`s, one per face, never merged, and a block that shows back faces (leaves and glass) gets each one twice (the second looks the other way). Check by arithmetic: a 9 by 9 by 5 canopy has 10 x-planes of 45 faces, 6 y-planes of 81 and 10 z-planes of 45, so 1,386 faces and twice that with the back sheets, 2,772; times 36 canopies is 99,792, within a few quads of the 99,846 measured (the rest is the floor). A solid 32-voxel cube of leaves is 196,608 quads, 3 MB.

## Correction

When I offered this optimization I wrote that merging "would attack the 2.6 ms part directly". **That was not established and may be wrong.** Merging cuts the number of quads (here from about 99,800 to about 2,300 by the same arithmetic: each canopy would be 64 merged rectangles) and so the instance memory, the vertex work and the triangle setup. It does **not** cut overdraw: a view ray through a canopy still crosses the same number of sheets, each still reads the picture and may `discard` (and `discard` defeats early depth rejection). If the 2.6 ms is fragment work, merging saves little of it. I have not measured which it is.

## The experiment to run first (cheap, not run)

Render the same scene at 320 by 180 as well as 1280 by 720 on the integrated GPU. If the extra cost falls roughly with the pixel count, it is fragment work (overdraw); if it stays, it is per-quad work. That decides which option below is worth building. The benchmark test was not kept (it was a temporary `#[ignore]`d test in `engine/renderer/tests/gpu_rendering.rs`), so it has to be written again: the scene is described above (nine bricks of floor, four solid 9 by 9 by 5 canopies on each).

## Options

1. **Merge leaves and glass into the greedy path (cuts per-quad cost; not overdraw).** A `Quad` has four free bits (`geometry` bits 28 to 31 of `PackedQuad`; bits 0 to 27 are used: checked in `quad.rs`). One of them can mean "this sheet is seen from behind", so a merged rectangle also gets its mirror. Needs: the mesher's `mesh_whole_faces` to take sight-through whole cubes (today it takes only `is_opaque_cube`), the suppression rule `front_draws_the_sheet_from_behind` carried over to rectangles (it is per-quad today), the shader to read the bit, and the golden pins re-pinned (every leaf brick's mesh changes). Picture tiling already works on merged quads (the picture's coordinates come from the corner's place in voxels). Risk: the suppression rule is the subtle part (two sheets on one plane look the same from one place; a proptest guards it today).
2. **Draw fewer sheets inside a mass (cuts overdraw; changes the look).** The owner chose "truly all, from any angle", so this is a change of decision, not an optimization: for example draw only the sheets within a few voxels of a surface, or only every other plane deep inside a mass. Not recommended without the owner.
3. **Dither far leaves and glass (cuts cost far away; already wished).** Past the picture distance a face is already one flat colour (cheap, solid). The owner's dither idea (`future-goals.md`) would keep far foliage see-through, which costs *more*, not less, unless the far sheets are also thinned. Only if options 1 and 2 are not enough.
4. **Do nothing.** The RTX does not notice, and a real tree is a shell. Reasonable until a forest is measured on the integrated GPU.

## Recommendation (confidence: medium)

Run the resolution experiment, then measure a realistic scene (a forest of hollow canopies, 2 voxels thick) before building anything. If the experiment says fragment-bound, option 1 will disappoint and the real choice is option 2, which is the owner's. If it says per-quad-bound, option 1 is worth doing and is mostly mechanical.
