# What a fast ship costs the server, and what could be done about it (held back)

Written 2026-10-31 after profiling the tick of one fast cube. The owner held it back (2026-10-31) because the shape of the fix depends on what a ship
will be: ships of several grids joined by a hinge or a tether, grids that travel together, and ways of changing shape that are not known yet, even within a tick. An optimisation
written for one grid with one box of bounds may be wrong for those, so it waited for the hinge prototype (`future-goals.md`).

**Option 2 below, the tube, was done on 2026-11-07, for ships and for walkers, because the owner flew the dev rocket and the game ran at two ticks a second** (see "Done"). The rest is still
not scheduled. What was done takes the bounds of the body's colliders and its velocity, as the cube did, so it is no more tied to one grid than the cube was; a ship of several grids that is
cut into bodies should find its tube from each body's own bounds and velocity. The one number it adds is `physics::TERRAIN_EVERY_WAY_VOXELS`.

## What was found (measured)

A free 3-voxel cube of stone, 2 km over Cave's planet, costs the server this much a tick (`engine:grid_bodies_move`; the tick is 50 ms):

| Speed (m/s) | Near the planet, before phase A | After | Far from it, before | After |
|---|---|---|---|---|
| 0 | 0.06 ms | 0.05 | 0.05 | 0.03 |
| 100 | 0.30 | 0.17 | 0.13 | 0.06 |
| 400 | 2.0 | 1.4 | 0.59 | 0.38 |
| 786 (a circular orbit there) | 14.7 | 8.5 | 4.5 | 1.9 |
| 1,500 | 168 | 55 | 70 | 11 |

- **It is not Rapier.** One fast cube in `physics::BodyWorld::step` costs 0.04 ms a tick at 24 solver steps; the server's whole step of Rapier is 0.08 ms. (An earlier note blamed the 24
  steps; it was wrong, and `build-log.md` says so.)
- **It is `grid_bodies::keep_terrain`**, 99 percent of the system. A ship needs colliders for the terrain bricks near it. The margin is `8 voxels + 0.25 s x speed` ahead (wanted) and
  `2 + 0.1 s x speed` (needed), **in every direction, as a cube**, and the planet's extent holds the ship, so the planet is asked as a second ground.
- Before phase A the ring loops went over every brick of the whole cube once per 8-voxel ring: about 47,000 visits a tick at 786 m/s, growing about as the speed to the third or fourth power.
  About 110 bricks a tick came into the cube and 110 left, each a collider made or removed (air, at that height).
- Method, for doing it again: time each system in `plugin_host::PluginHost::tick`, the stages of `grid_bodies::Bodies::step`, and the parts of `keep_terrain`, with throwaway counters (none are
  committed); run a cube at several speeds in a `Scene` of `games/voxunami_cave/tests/common`. `tests/orbit.rs` has the orbit that makes a fast ship.

## Done

- **Phase A** (`ca88c4a`): `wanted_rings` works out each brick's ring once, from the ring of each coordinate along each axis, instead of once per ring. The result is the same (a property test
  against the old way, and a mutation that fails it). It is the safe part, whatever ships turn out to be.

- **The tube, 2026-11-07** (`physics::SweptBricks`, `BodyWorld::{bricks_ahead, bricks_ahead_in}`, `CharacterBody::load_terrain_around_character`, `grid_bodies::bricks_to_change`). Reported by the owner:
  flying the dev rocket up from a planet, the server ran at two ticks a second after getting off it, and rotation looked like 5 to 10 a second. **Measured** (release, one rocket of 4 engines and a seat
  and a pilot, a new world, from 70 km; the tick is 50 ms):

  | | before | after |
  |---|---|---|
  | powered, 2.3 km/s: the ship's tick | 40 ms (`keep_terrain` 34) | 4 ms |
  | coasting, 4 km/s | 210 to 230 ms | 5.8 ms |
  | coasting, 6 km/s | 700 to 850 ms (the first ticks 2.7 s) | 8.7 ms (the first ticks 60 ms) |
  | bricks in one region of a ship at 6 km/s (needed, wanted, kept: a cube 94 across) | 912,673 | 11,583 along an axis, 16,701 slanted (boxes overlap; the distinct bricks are fewer) |
  | bricks that got colliders in a tick, a ship at 6 km/s | 30,422 | 2,000 |
  | a walker who stepped off a ship at 6 km/s: the tick | 3,050 ms (`CharacterBody::update_terrain`) | about 70 ms, 11 of it in `update_terrain` |
  | bricks the walker's source was asked about in one tick at 6 km/s | 857,375 | 4,212 (the frame is rebuilt each tick at that speed, so it is the whole tube) |

  The margins are the cube up to `TERRAIN_EVERY_WAY_VOXELS` (128 voxels, 4 bricks) and past it that cube carried along the way the body goes for the rest of the margin, in boxes of 128 voxels of the way. It is
  **exactly the cube for a body slower than about 416 voxels a second** (`8 + 0.25 s x speed + 16 <= 128`; the needed margin, to 1,260), so the golden traces, the pins and every scene of the suite did not
  move; past that it is always some of the cube (never a brick the cube had not: a property test), always the bricks ahead (every place the body's box is on its way, to the end of the margin: a property
  test), and nested as the margin grows (the rings of the wanted bricks need it). Tests: `physics::sweep` (5, with 4 mutations caught), `bodies::tests::the_bricks_a_body_will_reach...` (hand-worked
  extents, in a turned frame too), `grid_bodies` (`wanted_rings` against the old way, for boxes and now for regions of several boxes; 2 mutations caught), `tests/fast_ships.rs` (the bricks made
  in a tick, counted: red with the cube), `engine/physics/tests/fast_character.rs`.
  **What it gives up:** the bricks behind and beside a body that goes faster than that are those within 128 voxels and no more (they were a quarter of a second of its speed), so a body that is thrown back at
  more than 2,500 voxels a second by a blow in one tick may reach terrain that was let go; the bricks it has to reach going on are all made. **What is left of the cost** is a count of bricks a tick that
  grows as the speed (about 2,000 made and 2,000 let go a tick at 6 km/s, 25 of the 50 ms with a pilot aboard): option 4 below, and the character's frame being rebuilt each tick above 3,800 voxels a second.

## Not done (options, with what each needs)

1. **The planet is not a second ground unless the ship is near its surface** (biggest of what is left: about 6.6 ms of 8.5 near the planet at 786 m/s). The planet's extent box holds a ship 2 km up, so its
   wanted bricks (thousands, mostly air) are asked for every tick, and the worker that makes them has a cache of 256 (`plan.md`, "Still waiting"). *Belief, not measured:* the prefetch of the wanted
   bricks and their six neighbours is much of it. *Needs:* a measurement of that first; a rule for "near" that is true for a ship of several grids (the nearest grid, not the first).
2. ~~**Look ahead in a tube along the velocity, not a cube.**~~ **Done 2026-11-07** (above), for a tube whose cross-section is the cube of 128 voxels round the body's bounds. (It was thought it would move
   the tests that count made bricks and the golden sessions; it moved none, as it is the cube below 416 voxels a second.) Still open: what a ship of several grids, or one that changes shape within a
   tick, should take its bounds and velocity from.
3. **Skip the lookahead when the generator's bounds say the region is air.** A ship at altitude would need no colliders. *Needs:* a question of the world, "is there ground in this box of bricks",
   answered from the generator's bounds (`TerrainGenerator::may_show` is the near relation, for aligned cubes) and from what players have built (stored bricks, which are ground wherever they are).
   A design question, not an edit.
4. **Do not make and drop colliders for air.** About 110 bricks a tick are set and removed with nothing in them. A hint of "empty" per brick exists (`BrickHint::Empty`); the cost is in making the
   entry at all. *Needs:* a measurement of how much of the remaining time it is.
5. **Fewer, steadier changes of the set.** The set of wanted bricks is rebuilt every tick from scratch; a ship that moves 39 voxels a tick changes only a slab of it. Keep the set and move its edges.
   *Needs:* the same questions as 2, since what "the set" is depends on the ship's shape.

## What to keep true whatever is done

- What is simulated must not depend on the machine: work is limited by counts, never by time (`design-decisions.md`, ships). A budget in bricks a tick is fine; one in milliseconds is not.
- A ship must never reach a brick that has no collider (the *needed* bricks are made whatever it costs). (Ahead of it this holds at any speed. Behind and beside a body faster than 416 voxels a second, only
  as far as 128 voxels does, since 2026-11-07.)
- Golden sessions change only with a reason written down beside the new pin, and a check that only that reason changed them (the F5 move did it by writing the old names back).
- A ship of several grids, a hinge, a tether, grids that travel together: the cost should be of what moves, not of how many bodies there are, and a body that is a part of a ship should not ask for the
  terrain round the whole ship again.
- The prototype ball joint (`decisions/joints-and-cars.md`) starts with a bounds per car, the sum of its longest dimensions as a diameter; whether two cars cost more than one is to be measured before any of the five options above is chosen.
