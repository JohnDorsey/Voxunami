# Pixelated shadows (history, 2026-11-11): what it costs

The owner's wish: the effect of the Complementary shader's pixelated shadows, in which each pixel of a block's picture is wholly in the shadow or wholly out of it, so that the edge of a shadow steps along the picture's pixels. The question: what it costs to do this for everything, ships and planets included. Labels: **[run]** I ran it; **[looked]** I looked at pictures; **[read]** read in source or code; **[believed]** reasoned, not tested. Everything temporary was restored from copies; nothing in the game changed.

## 1. Short version

- **The effect is cheap to make, and the shadow map's resolution is what costs.** **[run]** A prototype that moves the place a face's shadow is looked up at to the middle of the picture texel it is in (about 25 lines of shader, no new texture reads) costs **0.1 to 0.2 ms on the integrated GPU (about 1%) and nothing measurable on the RTX 3090** (15.43 and 15.58 ms against 15.42 and 15.54 over two runs; 4.04, 4.09, 4.22, 4.03 on the RTX). It works on terrain, on a ship turned 30 degrees, and in real Cave **[looked]**.
- **How far it can look right is set by the map's texel.** A cascade's texel is `2 * half width / 1024`: a sixteenth of a voxel (exactly a texel of Cave's 16 by 16 pictures) in the finest cascade, which reaches about 28 voxels. The next is an eighth (2 pixels of a picture), then a quarter. Where the map's texel is wider than the picture's, the shadow's edge steps in the map's texels, not the picture's: still blocky, not aligned. To have a sixteenth to 56 voxels the maps must be twice as wide, to about 112 four times.
- **That costs [run]** (all 15 layers, camera walking and the sun turning, pole of Cave, 1280 by 720):

| map size | texel of the first four cascades (voxels) | reaches a sixteenth up to | memory of 15 layers | RTX | integrated |
|---|---|---|---|---|---|
| 1024 (today) | 1/16, 1/8, 1/4, 1/2 | about 28 | 60 MB | 4.01 ms | 15.35 ms |
| 2048 | 1/32, 1/16, 1/8, 1/4 | about 56 | 240 MB | 4.05 | 17.80 (+2.45, +16%) |
| 4096 | 1/64, 1/32, 1/16, 1/8 | about 112 | 960 MB | 4.38 | **out of memory** (wgpu error) |

- **What range matters:** a picture's texel is bigger than a screen pixel only within about 37 voxels of a face seen straight on at 1280 by 720 with a 70 degree view (55 at 1080p, 74 at 1440p, 110 at 4K) **[calculated: texel 1/16 voxel over the angle of a pixel]**; pictures are drawn to 64 voxels and fade over 16 more (`DEFAULT_PICTURE_DISTANCE`). So today's maps are exact where a 720p player can see the texels, 2048 serves 1080p, 4096 serves 4K.
- **My view (medium confidence):** do the snap (it is nearly free), keep 1024 for the first release, and decide the map size by what the integrated GPU can afford; the thing worth building if the owner wants more reach is a **separate, larger array for the first two or three cascades only** (cheaper than widening all 15 layers; **not measured**), and a rule that the snap grid is the wider of the picture's texel and the cascade's, so that a place beyond the exact reach is pixelated in coarser pixels than the picture's instead of blurred (**not built, not tried**).

## 2. How Complementary does it **[read]**

The source read is the current public `ComplementaryReimagined` repository, which is the r5 line: the repository has no v4 tag, so I cannot say what v4 did. The licence is all rights reserved, so nothing was copied; the prototype is written from the idea (the technique is the common one of snapping with screen-space derivatives, which their source credits to a Unity forum post).
- It is an option (`PIXELATED_SHADOWS`, off by default) with a scale (`PIXEL_SCALE`, from 4 times the picture's texel to one sixteenth).
- Per pixel it works out, from the derivatives of the picture's coordinates, how far on the screen the middle of the picture's texel is, and moves the **position used for the shadow lookup** by that much, using the derivatives of the position (so along the face, in the camera's frame; no world grid). It does nothing where the derivatives are degenerate (a grazing face).
- **The shadow map itself is not touched**: one map, 2048 or 4096 wide, no alignment of its texels to the picture's. Entities use the same code with their own picture; there is no distance rule (the effect simply vanishes when a texel is smaller than a pixel), no cascades, and the source has no performance claim. My inference, not theirs: their edges are not exactly aligned either.

## 3. What I built and saw

The prototype (temporary, in `voxel.wgsl`): `snapped_place` solves for the screen offset that takes the picture's coordinates to the middle of their texel, applies it to the position, and the map lookup and the walk of the sun's ray start from that place (the picture still reads the true place). A frame uniform's spare slot held the number of texels (16; 0 is off); off was the same code with an early return.
- **Terrain, a floor and a wall of checker-picture blocks, a sun 35 degrees up [looked]:** off, the shadow edge is soft and ragged at less than a texel; on, it is a clean staircase of the picture's pixels. A low sun (15 degrees) is also clean, but the steps are coarser along the sun's direction: a map texel projects onto a slanting floor stretched by one over the sine of the sun's height, so the shadow's edge toward the sun is only as good as that **[believed, and the picture agrees]**; the walk of the sun's ray (on by default in Cave, exact for the first cells) makes it exact where it reaches.
- **A ship turned 30 degrees [looked]:** the steps follow the ship's own turned picture grid, because the derivatives know the face. The ship's wall shades its own floor, in the picture; I did not make a scene of a ship shaded by terrain or the reverse. Four adapters (RTX and AMD, Vulkan and DX12) ran the scene.
- **Real Cave [looked]:** 09:00 at the pole, a zoomed crop of a dirt bank: off, soft edges; on, edges that step with the picture's pixels. Where a whole picture is in light or in shadow nothing changes (90 to 6,400 pixels of 921,600 differ in the pictures I made).
- **At 40 voxels, where the cascade is the second [looked]:** at 1024 the edge is ragged at two picture pixels; at 2048 it is clean; at 4096 crisp.

## 4. What it costs, by part

| part | cost | status |
|---|---|---|
| the snap in the shader | 0.1 to 0.2 ms iGPU, 0 RTX; about 25 lines | **[run]** |
| the picture's size per face (the prototype used 16) | `textureDimensions` of the face's size class, a few instructions | **[believed]** |
| faces with no picture (flat colour beyond the picture distance, coarse nodes, avatars, boxes) | no texel grid: not pixelated; a world grid of a sixteenth would give them one | not built |
| ships and planets near the camera (bricks with pictures) | the same snap; a turned grid works by derivatives | **[run]** for a ship, **[believed]** for a planet's bricks |
| ships as casters | the maps that hold things that move are those whose texel is a voxel or less (5 cascades at 1024, 6 at 2048, 7 at 4096), redrawn when a ship moves | **[believed]**: more cascades and wider maps raise the cost of a moving ship |
| a planet that moves | kept maps are redrawn for the ground (build log, 2026-11-11, +2.5/+3.3 ms); wider maps multiply it | **[believed]** |
| the contact walk | starts from the snapped place, so it is exact per picture pixel in its reach; its cost is unchanged | **[believed]**: it is on by default in Cave's headless pictures (not in the test scene, which had no walk) |
| the sky's maps (13 taps, 8 voxels across) | not snapped; the taps blur it anyway | by design |
| the sun turning | a map is redrawn when the sun has turned a little, and the staircase then moves by a texel: it steps and does not slide | **[believed]**; the maps' texels are fixed to the world (`Cascade::new`), so a moving camera does not make it swim |

## 5. What I did not do

One scene in the real game (the pole), one test scene, a dev-profile build, one run per cell except the snap cost. Not measured: the cost of widening only the first cascades; the cost of ships and moving planets with wider maps; 1080p or 4K frames; a sun at 7 degrees (Cave's start) with the snap; how often the grazing fallback leaves a face unsnapped; shadows through leaves and glass (their maps read pictures at the map's pixel, which I did not change). The snap should be an engine option (the player's or the pack's choice), not a pack's own, because it is done where the map is looked up, which a pack cannot do; that is a decision for the owner.

## 6. If the owner says go (my plan, with the tests first)

1. A GPU test on four adapters that a picture texel is wholly lit or shaded (the probe scene of this study, with the within-texel spread counted), for terrain and a turned ship, written to fail first.
2. The snap behind a setting, with the picture size from the texture; the pack interface unchanged.
3. The grazing fallback and the low sun looked at, and Cave's start sun (7 degrees) in the pictures.
4. Then, only if the owner wants more reach: the separate array for the fine cascades, measured on the integrated GPU, with the memory budget stated.
