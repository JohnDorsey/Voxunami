# Spike: the lit strip at the foot of a wall, and an exact trace of the sun's ray (2026-11-09)

**A spike, not a feature.** Branch `worktree-shaders`; the code is in `engine/renderer` (the `occupancy` binding, `contact_light` in `voxel.wgsl`, `VoxelRenderer::set_occupancy`) and in the `#[ignore]`d tests named `spike_*` at the end of `gpu_rendering.rs`. It is not meant to be merged as it is: the volume is a hand-filled box, and only whole opaque cubes are in it. Owner's wish that started it: shaders the player can swap, with a default that is robust, fast and has **no** gaps at the base of shadows, whatever the redesign costs; 60 fps on the integrated GPU.

## What the owner saw

A thin lit strip along the inner edge where any block meets the ground, at almost any time of day (screenshots of 2026-11-09), and the same strip in caves, where no sun can reach.

## What was measured (all four adapters gave the same numbers: RTX 3090 and the AMD integrated GPU, each on Vulkan and DX12)

**Today's shader.** A block of height 4 on a floor, seen from straight above, the floor's light with shadows over its light without, along a line from the wall's foot; the strip is the distance to the first pixel under half. 256 by 256 pixels, a pixel a hundredth of a voxel:

| Camera over the floor | sun 5 to 85 degrees up | cascade texel (`T`) |
|---|---|---|
| 20 voxels (finest cascade) | 0.07 to 0.12 voxel | 1/16 |
| 60 voxels (a coarser one) | 0.28 to 0.57 voxel; at 85 degrees none of the 0.35 voxel of true shadow is dark | about 1/4 (my reading of the numbers, not checked) |

It fits `T * (cos(elevation) + 1.5 * sin(elevation))` within 20 percent everywhere but 75 degrees at 60 voxels (0.57 measured, 0.43 by the formula; unexplained). **The cause (my derivation, which the numbers support):** the map keeps a wall's back face eased away from the sun by `CASTER_SLOPE_BIAS` (1.5) times the face's depth slope, which for a wall is `tan(elevation)`, and the place looked up is eased toward it by one texel (`DEPTH_BIAS_TEXELS`). A floor point beside the wall is deeper than the wall's biased depth only once it is about 1 to 1.8 texels from the foot. That is the price of the bias that keeps the underside of a thin floor from shading the floor (the 2026-10-28 fix traded a five-texel gap for this one). It is a **texel**, not a number of pixels: texels follow pixels, so the strip is one or two pixels at every distance, which is what the screenshots show.

**A closed room in solid rock** (no sun can reach inside; every lit pixel is a leak): 6 to 9 of 85 floor pixels at the foot of the wall on the sun's side are lit under suns from 10 to 85 degrees. The same mechanism as the strip: the nearest back face to the sun for such a texel is the wall at the place's foot. This is the leak of the cave screenshots, reproduced; it has not been checked against the owner's own world.

## The prototype

A one-bit-per-voxel volume (a storage buffer, 36 KB for the benchmark, 4 KB for a brick) and, in the fragment shader, a walk of the sun's ray through it (a 3D DDA) for up to `K` cells, starting **in the cell in front of the face**, so nothing is eased toward or away from anything. The result multiplies the shadow map's answer: `light = trace_clear ? map : 0`. Off by default (no volume, `K` 0); the existing 15 shadow tests pass unchanged, with it off.

| Test | Today | `K` = 2 | `K` = 6 |
|---|---|---|---|
| strip at the foot, first lit pixel (all suns, both cascades) | 0.07 to 0.57 voxel | 0.004 (none) | none |
| lit pixels that should be dark, along the whole strip to 0.05 short of the true end | up to 44 of 77 | 0 up to 60 degrees; at 75 to 88 degrees up to 24 (coarse) | **0 everywhere tested** (to 88 degrees) |
| closed room, lit pixels at the wall's foot | 6 to 9 of 85 | 0 (3 at 85 degrees) | 0 |

The cells needed grow with how steep the sun is and how coarse the cascade (a steep ray climbs layers before it reaches the wall); they are also bounded by the wall's height. In these scenes 6 were always enough. The truth used is analytic (the wall is 4 high, so a point at distance `d` from the foot is dark if `d * tan(elevation) < 4`), not a CPU tracer: the real build needs the CPU reference described below.

**Cost** (1280 by 720, a floor with about one column in eight standing 1 to 5 voxels high, camera and sun moving, 30 batches of 10 frames, `wait_for_gpu`): mean frame, ms:

| Cells searched | RTX 3090 (Vulkan) | AMD integrated (Vulkan) | AMD integrated (DX12) |
|---|---|---|---|
| 0 | 0.06 | 0.8 to 0.95 | 0.77 |
| 4 | 0.07 | 1.27 | 1.27 |
| 8 | 0.09 | 1.60 | 1.60 |
| 16 | 0.13 | 2.17 | 2.22 |

About **0.08 ms per cell** on the integrated GPU: six cells are about 0.5 ms of a 16.7 ms frame. Caveats: the base scene is light (0.8 ms), so only the *added* cost carries over; the volume (36 KB) fits in cache and a real one will not (so this is a best case, which I believe and did not measure); the benchmark is at 720p.

## What the spike does not show

- Only whole opaque cubes are in the volume. **Slabs and stairs** (eighths of a voxel), **leaves and glass** (a hole decision needs the picture), and **moving grids** (ships, avatars; each has its own frame) are not traced: they keep today's strip.
- A ray of `K` cells settles occluders within `K` cells only; beyond, the map decides, with its own softness (the opposite error: at the far edge of a shadow the map is dark or lit up to a texel off, and at a coarse cascade that is several voxels). At a cascade with `T` of a voxel or more and a steep sun the cells needed are many; the way out is a hierarchical walk (the coarse levels already say "empty" or "all one material" per cell), not a longer flat one.
- The data: the client holds the bricks, but nothing uploads their occupancy, keeps a clipmap round the camera, or updates it when a block changes.
- Cave screenshots of the owner's world, not reproduced.

## Answers to the owner's two ideas (2026-11-09)

1. **"Search the nearest few blocks, and if they are air, use the shadow map."** This is what the prototype is, and it works: measured above. Fast (bounded work), and it fixes exactly the strip, because the strip is a contact error that a few cells settle.
2. **"Fill in the shadow at the feet of blocks after the map is made."** Possible in principle (write unbiased depths into the map's texels at wall feet), but it needs the same list of wall feet as the trace needs the volume, it is sun-dependent (rewritten whenever the map is), and it leaves the filter's one-texel blur: the strip shrinks, it does not go. Not recommended over the trace.

## What a real build needs (not decided)

A clipmap volume round the camera kept from the bricks; a CPU reference tracer and a test that compares the shader's lit/shadow with it on random scenes and suns on every adapter (that is what makes "no gaps" a checked rule); the slab and leaf cases; a decision for the far cascades; and, for swappable shaders, the engine owning this visibility and handing a pack `sun_visibility(place, face)` so that a pack cannot bring the strip back.
