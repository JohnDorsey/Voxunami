# The shade study (history, 2026-11-11)

The owner's aim: *shadows at a distance are useful for judging the shape of terrain, which pilots need.* After the planet's ground was made to cast at a distance (`shader-systems-survey.md`, section 7.1), the question left was whether the shade is **legible** at a distance. This is what I measured and saw. Labels: **[run]** I ran it; **[looked]** I looked at the pictures; **[believed]** reasoned from the code, not tested.

## 1. Short version

- **The shade in Cave is crushed.** **[run]** Averaged over ten fixed views, **38% of the terrain pixels in the 60 rows under the horizon, and 37% in the next 100, are nearly black** (brightest channel under 30 of 255). In the views that face away from the sun the figure is **54% to 76%** at every time of day I tried, noon included. Cause **[run, by changing the one number]**: a place the sun does not reach gets `sky_light_in_shadow` 70 of 255 of the sky's light, and the sky's light is 110 of 255, so an upward face in shade is about 12% of full brightness and a wall that faces away from the sun, which `side` darkens further, is a few percent.
- **Raising that one number removes the crush and calms the terrace stripes** **[run]**: at 130 the far crush is 0.4% and the middle 3.4%; at 190, 0.1% and 1.3%. The dark-to-bright ratio of the far terrain goes from 0.29 to 0.42 (130) and 0.51 (190). The stripes (the alternation of bright tops and black risers of terraces seen from far, which look like moiré) fall by a quarter at 130 and over a third at 190.
- **What I recommend, medium confidence, and a matter of taste that is the owner's:** about **130** for Cave (a shaded face keeps about a fifth of a lit one's brightness, a shade of 4.5 to 1), not changed yet. See the pictures, which I sent with this report; the numbers cannot say which is nicer.
- **What the numbers do not say.** I have no measure of "can a pilot read the relief", only proxies. The one that looked like a measure of relief (the variation of the brightness of 8 by 8 blocks) **fell** as the shade was lifted (0.52 to 0.38 to 0.30), because it counts the black-and-bright stripes as relief; I do not rely on it.

## 2. Method

Ten pictures of Cave from the pole, 1280 by 720, dev profile, `--camera 16,64300,16 --pitch -3`, at 06:45, 09:00, 12:00, 15:00 and 17:30 (`--time`), each facing yaw 0 and yaw 180, with the planet's ground casting (commit `27bb30a`). One run each; a picture is deterministic. The horizon of each column is the first row from the top where the picture jumps by more than 12 of 255 between rows (a sky is smooth, terrain is not). Bands under it: far (the 60 rows below), middle (the next 100), near (the rest, down to row 650, above the hotbar). Per band: **crushed** (share of pixels whose brightest channel is under 30), **mean luminance** (0.2126 R + 0.7152 G + 0.0722 B, of the picture's sRGB values), **stripes** (the mean absolute luminance change from a row to the next, over the mean luminance), **dark/bright** (the 15th over the 85th percentile of luminance), and **relief** (the coefficient of variation of 8 by 8 block luminance). The scripts were scratch files and are not kept; the definitions above are exact.

The variants differ in one number, `sky_light_in_shadow` of Cave's `Sun` (`games/voxunami_cave/src/game.rs`): 70 (today), 130 and 190 (of 255). Each was a temporary build; the file was restored from a copy and is unchanged.

## 3. Numbers (average of the ten views)

| band | variant | crushed | mean luminance | stripes | dark/bright | relief |
|---|---|---|---|---|---|---|
| far | 70 (today) | 38.2% | 49.4 | 0.106 | 0.29 | 0.515 |
| far | 130 | 0.4% | 55.4 | 0.080 | 0.42 | 0.379 |
| far | 190 | 0.1% | 59.8 | 0.066 | 0.51 | 0.303 |
| middle | 70 (today) | 36.7% | 45.2 | 0.172 | 0.38 | 0.481 |
| middle | 130 | 3.4% | 52.1 | 0.135 | 0.47 | 0.350 |
| middle | 190 | 1.3% | 57.3 | 0.115 | 0.53 | 0.280 |
| near | 70 (today) | 19.0% | 54.3 | 0.044 | 0.39 | 0.389 |
| near | 130 | 9.2% | 60.4 | 0.040 | 0.45 | 0.326 |
| near | 190 | 4.1% | 65.0 | 0.038 | 0.49 | 0.289 |

What is still crushed at 130 and 190 (the near band, 4% to 14%) is, **[believed]**, mostly what the sky's cover makes black on purpose (`sky_light_under_cover` 0: under a ledge it is a cave), which does not depend on this number: the same share appears at noon and at 17:30.

## 4. What the pictures show **[looked]**

Three pairs of far bands (noon facing away from the sun; 17:30; 06:45 facing away), today's shade above, 130 in the middle, 190 below. Today's: the far hills facing away from the sun are an even near-black mass, and the terraces between them and the camera are bright tops with black risers, which at a distance read as thin stripes and not as slopes. At 130 the hills have a visible green with their ridges and gullies; the stripes are fainter; the lit side of a hill still stands out. At 190 the far view is smoother again and flatter; I did not judge it nicer than 130.

## 5. What this does not settle, and what I did not test

1. **Taste.** A lit-to-shaded ratio is a look. Cave's is one number per level and a game's own; a pack can also choose (`pack_shade` receives `sky_visible`, and the proxy is applied before it).
2. **The player already has `.ambient N`** (0 to 255, "20 to 40 is dim like a cave"), which lifts faces that no light reaches. **[believed, not tested here]** it would lift the shade too, with the same effect on the pictures, and it is a lamp term that also lifts the insides of caves.
3. **The constant is blind to orientation and to the size of the sky that is hidden.** A wall facing away from the sun in the open sees half the sky, a valley floor shaded by a hill sees most of it, the lee of a high ridge sees little: today they all get the same share. A share that depends on the face's direction against the way up (a hemisphere) and a real occlusion that reaches farther than 205 voxels (section 8 of `shadow-spec.md`) are what the packs in Minecraft spend their passes on; I did **not** build or measure either. *Believed:* the hemisphere is cheap (a dot product and the way up, which the sky's maps already work out), the far occlusion is not.
4. **The terraces themselves.** The stripes come from voxel steps seen at a grazing angle from far away; a lower contrast between a top and its riser (less of `side`, a better share) lessens them but cannot remove them; a coarser level of detail at distance, or a fog that lifts the far shade toward the sky colour (aerial perspective), would. Not tried.
5. **Other places.** Ten views at the pole of one planet. Mountains, a canyon, an orbit approach and the night were not looked at.
6. **A planet moving on a rail** is still not measured (`shader-systems-survey.md`, 7.1).
