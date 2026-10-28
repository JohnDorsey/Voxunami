# The sun (Phase 41a)

Status: built 2026-10-28, the owner's "Lighting please" after the great rivers. What I ran or looked at is marked **[run]**; what I believe from reading the code **[belief]**.

## 1. What the owner wanted and what this is

The owner's direction (2026-10-27): shadows from shaders in place of the flooding of sky light "for most sky light". I recommended an order: the renderer's depth range (measured, not needed), then **the sun as a light of the game's clock with the terminator of each planet worked out as a sphere**, then cascaded shadow maps for what is near, then a cheap ambient term for caves. **This phase is the second step.** It makes the planet, the rivers and the overworld visible without `.ambient`, with a day and a night. It is *not* shadows: a hill does not shade the valley behind it and a cave under a sunlit hill is lit as the hill is (section 7).

## 2. The rule

- **A sun is a game's, per level** (`GameDefinition::sun(level) -> Option<Sun>`, default none: every other game is as it was). `Sun` (`engine/game_shell/src/sun.rs`) is a day length in seconds of the world's clock, the hour at clock 0, the direction at noon, the axis it turns about (a great circle if the axis is across noon), how bright it is (`light`), how bright the sky is where the sun reaches (`sky_light`), whether the level's ground is flat, and the colour and thickness of the sky.
- **The clock is the world's**, not the count of ticks of a run: the sun is a function of `ServerWorld::world_tick()`, which is saved with the world (`rails-and-recipes-spec.md`, 40a). The server now sends it in the welcome (**protocol 10**: `Welcome::world_clock`, the clock at the welcome's tick) and the client counts one for each tick (`ClientWorld::world_clock`). So everyone has the same sun, a world left at noon is at noon when started again, and a recording has the day it was recorded in. **[run]** (`server/tests/behaviour.rs`, `client/tests/stream_handling.rs`, `game_shell/src/sun_view_tests.rs`, which includes a recording).
- **Hours** are worked out from whole ticks of the day, so a world of any age is as exact as a new one (`Sun::hours_at`; a test at 2.4 x 10^15 ticks). The direction is the noon direction turned about the axis by a twenty-fourth of a turn for each hour from noon, with `libm`.
- **What the sun does to a face**, for each pixel, in `voxel.wgsl` (`sunlight`) and, as the same arithmetic, in `engine/renderer/src/sun.rs` (`sunlight`, `sun_visibility`): `sun = sun_light * max(cos(angle to the face), 0) * visible`; the sky lights every face the sun reaches by `sky_light * visible` (shaded by the face's side as lamps are); and what there was, the sky's and the sun's light combine like screen blends with the light of lamps and the ambient light (`1 - (1 - a)(1 - b)`; the code adds `b (1 - a)`, which is the same and leaves a picture with no sun exactly as it was). **[run]** (`a_sun_with_no_light_leaves_every_picture_as_it_was...`, and all the older picture tests unchanged).
- **`visible`** is how much of the sun gets past what is in the way, 0 to 1 with a soft edge, from:
  - **balls**: each planet's *solid core* (`TerrainGenerator::solid_core`, the ball that is ground all through, which the horizon cull already used), where the planet is now. The sun is hidden when its disc is within the disc the ball takes up in the sky of the place; the edge is a smoothstep in angle, as wide as the sun's disc (0.005 rad) plus, for a place at the ground of the ball, a twilight (0.1 rad, about six degrees, falling off as `(radius / distance)^8`). A ball smaller in the sky than its edge is wide hides only part of the sun (an annular eclipse; a speck hides none). The balls multiply, so a moon in front of a planet's sun is an eclipse. At most eight balls (the nearest) are used.
  - **a flat ground** (the overworld): the sun is hidden when it is below the horizon (`y` up), with the same soft edge.
- **A face at the ground of a ball is lit by a turned normal.** The ground of a planet of voxels is faces that look along the axes of its cube whatever way the ground goes, so from far the planet was three flat patches of light and the sun went off a whole cube face at once (**[run]**: a first picture at 18:00 from above the pole was black). A face within a hundredth of the ball's radius over its ground, or inside it, is lit by the normal half-way between its own and the way out of the ball; the turn is gone by three hundredths of the radius, so a ship above a planet is not turned. **[run]** pictures: a lit globe with a soft terminator (section 6).
- **A face inside the core is lit as if it were on the ball's surface**, in its direction. (A far node's faces may be under the ground by a cell, hundreds of voxels; my first rule put them in the dark and the planet was black from 250 km: **[run]**.)
- **The sky** (`sun_view`): its colour is the game's colour times the daylight at the camera (the same rule at the camera's place) times the air there: 1 up to half of `thickness` over the nearest planet's core, fading to 0 at `thickness`, so the sky is black in space. The sky's light fades with the air too. The fog is the sky's colour as ever.
- **The sides of faces are shaded from one fixed direction** when a sun is on (`Atmosphere::side_direction`), so a lamp lights a wall as it did at every time of the day.

## 3. Cave's sun

A day of twenty minutes of the world's clock (1,200 s: 50 s an hour), beginning at 07:00, in the level of space and in the overworld (no other level has a sun). Noon is at the direction (0, 0.94, 0.342) turned about (0, -0.342, 0.94): the sun is never straight overhead at the pole where players start (70 degrees at noon), rises towards +x and sets towards -x, and is 12 hours above the horizon and 12 below there. `light` 255, `sky_light` 110, sky `[0.55, 0.72, 0.92]` (the blue it always had) to 30 km over a planet. **All of these numbers are mine; the owner overrules** (a longer day, a different start, a dimmer sky).

## 4. For the player

- `.time` says the time of day by the world's clock; `.time 15:30`, `noon`, `midnight` pins the sun for this player (the client's own setting: nobody else's, nothing in a recording); `.time auto` lets the clock move it.
- `--time TIME` on the command line does the same for a window, a screenshot (`cave --screenshot x.png --time 18:30`) or a recording being watched (`replay FILE --time noon`).

## 5. Tests

Renderer: the rule on the CPU (14 unit tests: edges, sunsets that only darken, eclipses, a billion light years, the turn of normals); the shader on every adapter against the rule (sunsets over a planet, a face inside a ball, a face on the flank, 8 more); a sun with no light leaves every picture as it was. Protocol and client: the welcome's layout is pinned, the clock counts. Shell: the sun follows the clock, two players and a recording agree, `.time`, `--time`. Cave: the planet's core is the one ball in the sun's way, the sky at noon, midnight and in space, the overworld's day, and **a picture on every adapter** (the ground and the sky at noon, at 18:30 and at midnight, through `light_the_scene`, as a window runs it).

**Found by the tests on four adapters, and not by one:** on the integrated AMD GPU in DX12, `asin(1.0)` is not a number, so every place on or inside a ball was black there while the other three adapters were right. The shader asks `asin` of at most 0.999999 now (`MOST_OF_A_BALL`; a thousandth of a radian), and the test of a face inside a ball is what keeps it.

## 6. Looked at **[run]**

Headless pictures (`cave --screenshot`, release): the ground at 07:00, 17:30, 18:00, 18:30, 19:00 and midnight (lit hills, a dimmer evening, a sky going dusky blue at 18:30, black by 19:00); the planet from 150 km above its pole at noon, 15:00, 18:00 and 21:00 (a lit disc, a terminator creeping in, a lit half with a soft edge down the middle, black). The first pictures of the planet from space were wrong (black; flat patches): the two bugs above.

## 7. Limits, known and not hidden

- **No shadows.** A hill does not shade what is behind it; a cave under a sunlit hill has its sun-facing walls lit by the sun through the rock. That is the next phase (cascaded shadow maps for what is near), and the reason the owner wanted shader shadows. Until then the planet's own terminator is the only shadow, and the sun-facing walls of caves are bright by day.
- **Nights are black**, like the caves: no moon, no stars, no night light. `.ambient` is how a player sees at night. A small night light (moon) is a number away if the owner wants it.
- **The terminator is that of the core**, a ball 280 voxels smaller than the planet (about 4 degrees of arc, in the twilight's width), and the hills' own horizon is not in it.
- **A sunset is not red**: the light is white and the sky only dims.
- **Far nodes are lit by the same rule** (they have no light of their own), so rivers and planets are lit from anywhere (the river is a flat blue: water has no shading but the sun's).
- **Eight balls**: a level with more than eight planets near the camera has the farther ones not shade.
- **The cost is none that I can see:** `cave --benchmark --size 1280x720` at the pole, release, RTX 3090, alternated with the commit before (built in a worktree): 3.90 ms against 3.90 ms and 4.41 against 4.36 ms average, over 200 frames each, twice. **[run]** (The planet's ball is in play there; the loop is at most eight balls per pixel.) Not measured on the integrated GPU.
- **Only Cave has a sun.** Packs cannot say one yet (a `sun` in a level's data is an easy thing to add).
- **The sky does not look at the sun**: no disc, no glow, no stars.
