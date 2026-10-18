# Light spike: how does light reach the 8-byte quad?

Question: Voxunami Cave will have flood-fill light with 256 levels (see "Voxunami Cave lighting" in
`design-decisions.md`). The renderer draws quads of 8 bytes, which the mesher builds by merging neighbouring faces of the
same block into big rectangles. Where does a face's brightness go, and what does it cost?

**Recommendation: keep the light in the quad, as built, with the light as part of the merge key, and let the mesher round
light to a chosen number of brightness bands (a setting, default 256, i.e. none, until a scene needs more).** The quad
stays 8 bytes: the 8 bits for light were free in the material word (block id 24 bits, light 8 bits). The cost is only
that big quads are cut where light changes, which the table below measures. Confidence: high that the quad count and
meshing time figures are right for this terrain and these made-up fields; moderate on how they carry over to real lamp
and cave lighting, which does not exist yet.

Reproduce: `cd docs/spikes/light && cargo run --release`. The project is standalone and not part of the cargo workspace;
it uses the real mesher (`voxel_meshing::mesh_brick`) and the real `base_blocks` terrain. Measured on the owner's
Windows 10 machine, one thread, release build, best of 5 runs of meshing the whole patch; the numbers that are not times
are exact and repeat.

## What was measured

A patch of the default rolling-hills terrain (seed 2024, which has sheet caves): 13 x 13 columns of bricks, 8 bricks tall,
so 416 x 416 x 256 voxels. 509 of its 1352 bricks have any surface; the rest are sky or solid rock. The mesher takes a
light channel, and a quad is as bright as the voxel in front of the face, so faces merge only when block *and* light
match. Each row of the table is one made-up light field, rounded to a number of evenly spaced brightness bands, meshed
with the light channel in place and compared with meshing without any light channel:

- `uniform`: 255 everywhere. Checks the setup: the quad count must equal the baseline, and does (791 287).
- `flood16`: Minecraft style. One lamp for every 14 x 14 columns (900 lamps), two voxels above the ground, level 15,
  flooding through air and losing one level per voxel. Stored as level x 17. A very lamp-dense night.
- `smooth256`: the same lamps, but each fades linearly over 24 voxels in 256 levels, ignoring walls.
- `gradient`: one level more every 4 voxels along x + z. Like a slow tint across the whole world.
- `noise`: unrelated light in nearly every voxel. The worst case, to show the ceiling.

## Results

Baseline, no light channel: 791 287 quads in 509 surface bricks (1555 per brick), 927 ms for the patch.

| field | bands | quads | x baseline | ms | x baseline time |
|---|---|---|---|---|---|
| uniform | 256 | 791 287 | 1.00 | 949 | 1.02 |
| flood16 | 256 (= its 16 levels) | 987 718 | 1.25 | 1004 | 1.08 |
| flood16 | 8 | 882 880 | 1.12 | 966 | 1.04 |
| flood16 | 4 | 833 438 | 1.05 | 964 | 1.04 |
| smooth256 | 256 | 1 282 954 | 1.62 | 974 | 1.05 |
| smooth256 | 64 | 1 182 473 | 1.49 | 971 | 1.05 |
| smooth256 | 32 | 1 071 475 | 1.35 | 967 | 1.04 |
| smooth256 | 16 | 940 433 | 1.19 | 971 | 1.05 |
| smooth256 | 8 | 865 054 | 1.09 | 965 | 1.04 |
| gradient | 256 | 964 773 | 1.22 | 973 | 1.05 |
| gradient | 64 | 834 377 | 1.05 | 970 | 1.05 |
| gradient | 16 | 801 845 | 1.01 | 960 | 1.04 |
| noise | 256 | 1 844 285 | 2.33 | 979 | 1.06 |
| noise | 16 | 1 761 075 | 2.23 | 977 | 1.05 |
| noise | 4 | 1 535 443 | 1.94 | 984 | 1.06 |

(All rows, with quads per brick, are printed by the program. The flood16 rows for 256, 64, 32 and 16 bands are identical
because its 16 levels survive rounding to any of those.)

## What it says

- **Meshing time barely moves** (+2 to +8 percent) whatever the light, even when the quad count more than doubles. Looking
  at the light costs little next to scanning the voxels. Verified.
- **Quad count is what light costs**, and only where light changes: +25 percent for a lamp-dense Minecraft-style night,
  +62 percent for the same lamps in 256 smooth levels, +133 percent in the pathological noise case. Memory is 8 bytes per
  quad, so the smooth case is +3.8 MiB on this patch (6.0 MiB to 9.8 MiB) and the drawing cost grows with the quad count.
- **Rounding to bands pays most for smooth light.** 32 bands bring smooth256 from 1.62x to 1.35x, 16 bands to 1.19x.
  Minecraft shows 16 levels, so that much banding is at least familiar, but how much is acceptable is a matter of
  looking at it, which this spike did not do.
- In ordinary daylight or a fully lit world, the field is uniform and the cost is zero (the `uniform` row).
- Light is not added to the *shape*: detailed quads (slabs, stairs) carry the same 8 bits in their own 16-byte form.

## Not measured (so treat as argument, not result)

- Light inside caves, light from the sky, and changing light (edits that move light and re-mesh neighbours). The fields
  here are fixed and made up; real flood fill will make the interior of caves look more like `flood16` than like `uniform`.
- GPU drawing time with more quads. Earlier numbers in `build-log.md` ("Measured") put 1.4 M quads at radius 12 at
  6.9 ms on the integrated GPU; the growth here is proportional, so a 25 to 60 percent increase is a visible but not
  dangerous cost. Not re-measured with light.
- The alternatives. I believe, from reading the vertex shader and the mesher, but did not measure:
  - Light per vertex (smooth shading) needs 4 more values per quad and must still cut quads wherever the light is not a
    plane across the rectangle, so it saves nothing on quad count and costs 4 or more bytes per quad.
  - A light texture (one byte per voxel, 32 KiB per surface brick, 16 MiB for the 509 bricks here) takes light out of the
    quads entirely, so quads would merge as in the baseline, and gives smooth interpolated light for free. It costs more
    memory than the extra quads do in the smooth case (16 MiB against 3.8 MiB), and a texture upload per changed brick.
    It is the way to get smooth light later, without changing storage, the protocol or the saved light channel; the
    channel stays 256 levels either way.

## Decision

Light goes in the quad, in the merge key, 8 bits, unrounded by default. A mesher option to round to a number of bands
(not built; trivial: `quantise` in the spike is the whole of it) is the lever if a scene gets heavy, and a light texture
is the later route to smooth light. Revisit when the lighting system exists and real scenes can be measured.
