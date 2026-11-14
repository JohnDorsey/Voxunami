# Parallax spike: prisms against smooth parallax (2026-11-13)

The owner asked for parallax in which **the pixels of a picture look like square prisms extruded by varying amounts, with their sides shaded**, and not lumpy with flat shading; and for a spike to see what that costs and looks like against simpler parallax.

**The code of the spike is commit `7d8f955`** (a runtime switch of three modes, for looking and measuring: its tests `parallax_spike_*` are in that commit, and the branch `spike-parallax` is there too). **Prisms were then built as a shader pack's option: see "Afterwards" at the end.** The pictures are in `docs/history/parallax-spike/`. Everything below is marked as **run** (seen or measured here) or **believed** (read from the code or reasoned).

## What was built

A new function `picture_prepare` (`engine/renderer/src/pictures_on.wgsl`) runs first in `fragment_main`, before anything reads a picture, and finds where **the eye's ray** meets the relief of the picture (the alpha of the normal picture, a height: 255 the top, 0 the lowest, `RELIEF_DEPTH` = 0.1 voxel deep by default). The albedo, the specular and normal pictures are then read there (`in.uv + parallax_shift`; the derivatives that choose the mip and solve the frame stay those of the face's own `uv`, since the shift jumps from prism to prism). The mode and the depth come in the one number `frame.pictures.w` (`VoxelRenderer::set_parallax_spike(mode, depth)`); 0 is off.

| Mode | What it does | Fetches |
|---|---|---|
| 1, **offset limiting** | shifts the picture once by the height at the pixel (no division by the cosine, so the shift stays small) | one `textureGather` |
| 2, **parallax occlusion** | 12 equal steps down the ray through the heights as a smooth surface (bilinear, by `textureGather`), the place between the last two steps found by interpolation; the normal picture's tilts light it (flat shading of a lumpy surface) | up to 14 gathers (one more is the check that there is any relief) |
| 3, **prisms** | the ray walked from pixel to pixel of the picture as a line is drawn on a grid (up to 12 cells); each pixel is a column at its own height, **flat on top** (the art's tilts are not used); where the ray enters a column that is already under its top, the hit is on the **side wall** it came in by, whose normal (`parallax_wall`) goes to `Surface::shading_normal` through the bump path, so the sun, the shadow of the relief (`relief_sun`, started from the hit's height) and the default pack light it like any face; walls are darker the deeper they are (`open` from 1 to 0.45) | one fetch for each cell walked |

A wall takes the colour of its own column's pixel; a top takes the picture's pixel. Where the four pixels round a place are all at the top (grass and every picture with no normal companion) nothing is done.

## What it looks like (run)

`docs/history/parallax-spike/`:

- `kinds.png`: three columns, **none, parallax occlusion, prisms**, rows with the eye 65, 40 and 22 degrees over a floor of the real `dirt.png` and `dirt_n.png`, a sun 30 degrees up from the right and the front.
- `offset-and-deeper.png`: **offset limiting**, then parallax occlusion and prisms at **twice the depth** (0.2 voxel).
- `plateaus.png`: a picture whose relief is one square plateau, seen from the four sides: none, parallax occlusion at 0.2, prisms at 0.2.
- `walls.png`: one plateau, the eye between +x and +z, the sun over +x alone (first row) and over +z alone (second): **the wall that faces the sun is the bright one in both, and the plateau's shade falls on the floor on the other side**, which checks the signs of the walls (a probe made for that; the first look at `plateaus.png` alone did not settle it).

What I see (my reading of the pictures, not a measurement):

- **Prisms are what was asked for**: crisp square columns, the pixel art kept as pixel art, flat tops in the picture's own colours, the near walls lit by the sun and the far ones dark, the plateau's shade on the floor. At 0.1 voxel (the dirt art's own depth, 1.6 pixels) the steps are small and read as a rough surface; at 0.2 they read plainly as columns.
- **Parallax occlusion is the lumpy one**: it bends the picture into a smooth warped surface (the heights are bilinear), the pixel grid curves, and there are horizontal comb-like streaks where the steps show at a low eye (12 steps are too few for the deep one). It is the right look for a mud or a stone surface and the wrong one for blocky pixel art.
- **Offset limiting** moves the picture a little with the view: a hint of depth, no edges, and the picture swims. Cheapest, and it does not look three-dimensional.

The adapters agree (run): the picture on the integrated AMD GPU is the same through Vulkan and through DX12 to the pixel, and the RTX 3090 differs from it by more than 8 levels in under 1 percent of the pixels (at pixel edges, where the float maths differs), on every sheet.

## What it costs (run)

Release build, integrated AMD GPU and RTX 3090, 1280x720, **a floor of 32 by 32 blocks of dirt filling the frame** (the worst case: every pixel pays), the sun 33 degrees up, no shadow maps, 20 frames timed after one more, 7 rounds with the modes in turn, the first round dropped, readback included. The frame is 3 to 3.6 ms, so the differences are what to read. The throwaway timing is `parallax_spike_cost` (ignored; `VOX_SPIKE_ELEVATION` sets the view).

| Eye over the floor | Adapter (none) | offset | parallax occlusion | prisms | prisms, twice as deep |
|---|---|---|---|---|---|
| 55 degrees | iGPU Vulkan (3.36 ms) | +0.31 | +0.93 | +1.07 | +1.16 |
| 55 degrees | iGPU DX12 (3.56 ms) | +0.26 | +1.27 | +1.19 | +1.57 |
| 25 degrees | iGPU Vulkan (3.05 ms) | +0.31 | +0.85 | +1.12 | +1.40 |
| 25 degrees | iGPU DX12 (3.20 ms) | +0.29 | +1.09 | +1.53 | +1.95 |

- **The RTX 3090: under 0.1 ms for every mode** at 55 degrees (the 25 degree rows for it were not read).
- **Prisms cost about what parallax occlusion does** on this GPU (+1.1 to +1.5 ms against +0.85 to +1.3), and a deeper relief costs more of it, since a deeper ray crosses more pixels; at a low eye the cost of prisms grows (more cells walked). Offset limiting costs a quarter of that. The shade of the relief already costs +0.3 ms on such a floor (`block-textures-spec.md`, "Bump self-shadowing"), which is inside the "none" column.
- **A picture with no relief (every height at the top, as grass is):** offset +0.05 to +0.22 ms, parallax occlusion +0.11 to +0.32, prisms **negative** (-0.17 to -0.38), because the prisms ignore the art's tilts and so skip the bump's frame maths that "none" still does for a tilted pixel.
- **With the mode off, the grass view of `cave --benchmark` costs +0.19 ms (1.2 percent) over the last commit** (15.60 ms against 15.80, four rounds alternating, each agreeing): the price of the code being in the shader and branching on the mode, for pixels that never run it. **A runtime switch is therefore not what to ship**: a mode that is off should not be compiled in (as the pictures' shader is a variant chosen when there are pictures).
- In the game the share of the frame that is dirt (or any picture with a relief) is small, so these are ceilings. **Not measured:** a real game view with relief on, a mix, another machine, the cost of writing depth (see below).

## What would be needed to make prisms real (believed)

1. **Depth.** Nothing moves the face in depth. **(Corrected afterwards: this item overstated the problem. The relief is below the face: a height of 255 is the face's own plane and the rest is deeper, so no column stands out of the quad. The quad is the silhouette and there is nothing to hide behind, and the shadow maps' flat face is right to a tenth of a voxel. Writing `frag_depth` would buy next to nothing and would cost the early depth test on every face of the shader. It was not built.)** What is left of it is the rim: see 2.
2. **The edge of a block.** A ray that leaves the block's picture wraps to the other side of the same picture (it repeats) and does not meet the neighbour's picture; a wall at the edge is the wall of the picture's own repeat.
3. **Distance.** The depth is already faded out with the picture over the picture distance; the number of cells walked should also fall with the distance, and a minified picture (the mips) turns the prisms to noise before that. Both were left alone.
4. **Specialise it** (no runtime switch), cap the cells by the view angle, and decide whether the tilts of the art should light the tops (the spike's tops are flat, as asked).
5. **Settings.** Depth is a number of the spike (`frame.pictures.w` fraction); the owner would want it a setting of a shader pack, as the relief's depth is for the shade (`RELIEF_DEPTH`, a first guess).
6. **Art.** A wall takes its column's colour, so the stripes on a wall of a plateau of dirt noise are the plateau's own pixels; art for prisms might want its own wall colour.

## Opinion (confidence: medium; the first part is what was believed before the build, see "Afterwards" for what held)

For this engine's pixel-art look **prisms are the better parallax**: it is the look asked for, it keeps the pixel grid, and its cost is about that of smooth parallax occlusion, which looks lumpy. As a **default** it is too dear for the integrated GPU everywhere there is a relief (a ceiling of +1 to +1.5 ms on a full-screen floor of dirt, on top of a frame already at the edge of 60 fps), so I would ship it only as an **option of a shader pack**, compiled out when off, with depth writing and the edge cases above done first. Offset limiting is the cheap fallback if a hint of depth is enough. What decides it is how much of a real view has a relief: with Cave's grass and stone (no relief) it costs nothing, and the dirt is where it shows.

## To run it again

On the branch `spike-parallax` (the tests are ignored; the pictures go to `VOX_SPIKE_OUT`, `E:/s/vx/parallax` if unset):

```
cargo test -p renderer --test gpu_rendering --release parallax_spike_ -- --ignored --nocapture
VOX_SPIKE_ELEVATION=25 cargo test -p renderer --test gpu_rendering --release parallax_spike_cost -- --ignored --nocapture
```

The mode is `VoxelRenderer::set_parallax_spike(mode, depth)`; the shader reads it from `frame.pictures.w` (mode its whole part, depth in voxels its fraction).

## Afterwards: prisms as a shader pack's option (2026-11-14)

The owner asked for prisms to be taken further as a shader pack's option, **on while they are tested**. What was built (the design is in `block-textures-spec.md`, "As built", "Prisms"; the setting is in `shader-pack-format.md`):

- **A setting, compiled out when off.** `Relief::Flat` or `Relief::Prisms` (`"relief"` in a pack's `pack.json`; `ShaderPack::with_relief`); a pack that does not say gets `DEFAULT_RELIEF`, which is `Prisms` for now. The shader is made of `pictures_on.wgsl` and either `relief_flat.wgsl` (constants: nothing to run) or `relief_prisms.wgsl`, so a pack with `"flat"` has no prisms in its shader. The spike's modes 1 and 2 and the `frame.pictures.w` number were taken out; they are in `7d8f955`. The example pack `flat` shows the setting.
- **The grass costs next to nothing** (a picture word's bit 18, set by the renderer on a picture whose normal companion has a pixel under 255, `HAS_RELIEF`): a face without it returns before any fetch. Without the bit the grass floor paid +0.33 (Vulkan) and +0.42 ms (DX12) on the integrated GPU for the early-out's gather alone; with it +0.13 and +0.32.
- **Prisms fade out with the screen, not only the picture distance**: where a picture pixel is smaller than half a screen pixel they fade, and past one they are not walked at all (so the cost is the near faces'), and the picture's own tilts fade back in as the prisms go (**a bug the test found: with the tilts dropped under prisms, the far dirt lost its bumps at once**: 29,192 of 64,000 pixels differed from the flat setting where prisms were skipped; found by the test written first, fixed, 0 differ).
- **Depth writing and silhouettes: not built, and not needed** (the correction above). **The block's edge is not fixed** and cannot be from the shader's side: a ray that leaves the block's face reads the picture's other side, which is right where the neighbour has the same picture and a sliver (a tenth of a voxel over the tangent of the eye's elevation: 0.2 voxel at 25 degrees) of wrong colour where it has not, or has air.

**Cost** (release, `prisms_cost`, the floor of dirt filling a 1280x720 frame, the eye 55 degrees over it; the "flat" shader is the one with the prisms compiled out): integrated GPU **+1.23 ms (Vulkan, 3.17 ms) and +1.54 ms (DX12, 3.36 ms)** on dirt, **+0.13 and +0.32 ms** on a picture with no relief; the RTX 3090 +0.01 to +0.06 ms either way. The same worst-case floor as the table above (+1.1 to +1.5 for the spike's prisms): nothing was won on dirt, the grass was.

**Seen in the game** (`cave --screenshot`, the RTX 3090, dev profile; not the integrated GPU): the default view is nearly all grass and 0.3 percent of its pixels differ from the flat setting (2,995 of 921,600, all on a patch of dirt at a cliff's foot); there the dirt reads as fine speckle with lit edges against the flat setting's blobs. A view of dirt filling the frame was not taken (Cave has little exposed dirt).
