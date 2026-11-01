# What a fast ship costs the server, and what could be done about it (held back)

Written 2026-10-31 after profiling the tick of one fast cube. **Nothing here is scheduled.** The owner held it back (2026-10-31) because the shape of the fix depends on what a ship
will be: ships of several grids joined by a hinge or a tether, grids that travel together, and ways of changing shape that are not known yet, even within a tick. An optimisation
written for one grid with one box of bounds may be wrong for those, so it waits for the hinge prototype (`future-goals.md`).

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

## Not done (options, with what each needs)

1. **The planet is not a second ground unless the ship is near its surface** (biggest of what is left: about 6.6 ms of 8.5 near the planet at 786 m/s). The planet's extent box holds a ship 2 km up, so its
   wanted bricks (thousands, mostly air) are asked for every tick, and the worker that makes them has a cache of 256 (`plan.md`, "Still waiting"). *Belief, not measured:* the prefetch of the wanted
   bricks and their six neighbours is much of it. *Needs:* a measurement of that first; a rule for "near" that is true for a ship of several grids (the nearest grid, not the first).
2. **Look ahead in a tube along the velocity, not a cube.** Terrain behind and beside a fast ship is rarely needed; a tube of the ship's cross-section is ten or more times fewer bricks.
   *Changes* which bricks have colliders, so tests that count made bricks, and perhaps golden sessions, move. *Needs:* what "the ship's cross-section" and "its velocity" are for a ship of
   several grids, or one that changes shape within a tick (the bounds are taken from the body's colliders now: `BodyWorld::bounds_of`).
3. **Skip the lookahead when the generator's bounds say the region is air.** A ship at altitude would need no colliders. *Needs:* a question of the world, "is there ground in this box of bricks",
   answered from the generator's bounds (`TerrainGenerator::may_show` is the near relation, for aligned cubes) and from what players have built (stored bricks, which are ground wherever they are).
   A design question, not an edit.
4. **Do not make and drop colliders for air.** About 110 bricks a tick are set and removed with nothing in them. A hint of "empty" per brick exists (`BrickHint::Empty`); the cost is in making the
   entry at all. *Needs:* a measurement of how much of the remaining time it is.
5. **Fewer, steadier changes of the set.** The set of wanted bricks is rebuilt every tick from scratch; a ship that moves 39 voxels a tick changes only a slab of it. Keep the set and move its edges.
   *Needs:* the same questions as 2, since what "the set" is depends on the ship's shape.

## What to keep true whatever is done

- What is simulated must not depend on the machine: work is limited by counts, never by time (`design-decisions.md`, ships). A budget in bricks a tick is fine; one in milliseconds is not.
- A ship must never reach a brick that has no collider (the *needed* bricks are made whatever it costs).
- Golden sessions change only with a reason written down beside the new pin, and a check that only that reason changed them (the F5 move did it by writing the old names back).
- A ship of several grids, a hinge, a tether, grids that travel together: the cost should be of what moves, not of how many bodies there are, and a body that is a part of a ship should not ask for the
  terrain round the whole ship again.
- The prototype ball joint (`decisions/joints-and-cars.md`) starts with a bounds per car, the sum of its longest dimensions as a diameter; whether two cars cost more than one is to be measured before any of the five options above is chosen.
