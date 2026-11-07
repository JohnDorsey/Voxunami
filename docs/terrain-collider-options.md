# Terrain colliders for fast ships: the bigger options

Written 2026-11-07 after the owner asked "why are empty bricks so expensive, and what are the biggest and riskiest ideas?". It sits beside `ship-terrain-cost.md` (which lists the small options, five of them) and does not replace it. **Nothing here is scheduled except item 1, which was done the same day.**

## What was measured (2026-11-07, release build, throwaway timers, removed again)

One dev rocket (a seat and four engines, a pilot aboard) going straight up over Cave's planet; 60 ticks after the first 10; the "whole tick" is the server tick plus the test client's.

| | 1 km/s | 3 km/s | 6 km/s |
|---|---|---|---|
| Whole tick before the early-out | 7.0 ms | 25.8 ms | 50.6 ms |
| Whole tick after it | 2.6 ms | 7.7 ms | 15.1 ms |

- At 6 km/s about 935 bricks a tick are made and 935 dropped on each of two grounds (the space level and the planet). Every call of `BodyWorld::set_brick` for the planet was a no-op (the brick was empty and had nothing before) and cost about 19 us, because it scanned all 32,768 voxel positions. That was 35 of the 50 ms.
- Asking the generator whether a planet brick is empty (`brick_hint`) costs about 0.1 us. Rapier is not the cost.
- What is left at 6 km/s after the early-out: working out which bricks to change (the three sets of `bricks_to_change`) is about 9.8 ms, making the space level's bricks 3.4 ms.
- **Not reconciled:** the first version of `ship-terrain-cost.md` said 8.7 ms at 6 km/s; this measurement said 50 to 57 ms with the same brick counts. See the correction in that document.

## Why the planet looks cheap, and when it will not

A planet is a sphere over empty space, so its bricks far from the surface are uniform and a generator can say so almost for free. `plan.md` records natural terrain at 5 to 10 ms a brick to generate. The strategy "list every brick the ship may reach and ask each one" works only while most of them are cheap to answer; with caves or noise in the generator it stops working at once. The tube along the velocity is a *volume*; ground is a *surface*. The options below are ordered by how far they get from the first to the second.

## The options, smallest to largest

Risk is to determinism (the hard rules in `CLAUDE.md`), to the rule that a ship never meets a brick without a collider, and to the ships of several grids that are not designed yet. Opinions are marked as such.

1. **An empty brick that had nothing is not scanned.** `set_brick` returns at once when the hint is `Empty` and the body has no filled voxels and no boxes there. Small, safe, bit-identical. **Done 2026-11-07.**
2. **Keep the three sets of bricks between ticks and move their edges** (option 5 of `ship-terrain-cost.md`). The sets are rebuilt in full every tick (about 10 ms at 6 km/s). Medium. Waits for what a ship of several grids is.
3. **Ask the generator for the largest empty cube round the ship.** `TerrainGenerator::may_show(cube)` exists. If the ship's whole reach is inside a cube that is certainly air, the planet needs no colliders at all; the cost is about log(distance) calls a tick, not the volume of the tube. Bricks that players built (stored bricks) must still count, and the rule that a needed brick always has a collider must hold. Medium; a design question more than an edit (option 3 of `ship-terrain-cost.md`). *My opinion:* the right design, and it matters more with every complication of a generator. Do it when the first generator with noise or caves is a planet.
4. **Ships on rails above some altitude.** Already the owner's goal (`future-goals.md`, "Space exploration"). A ship that is not stepped needs no terrain. It removes the whole problem for the common case of flying far from the ground. Large.
5. **Make the `Collider` values on the worker thread and only insert them on the tick thread.** The results are identical (a collider is a pure function of the voxels it is made from), so determinism is safe. Moderate; it helps the first contact with ground more than flight (`plan.md`, "To do, not scheduled: the building of colliders").
6. **Coarse colliders for fast flight.** At 6 km/s a ship does not need voxel-exact terrain, only to know it will hit something. The coarse cells that exist for drawing could stand in, with exact bricks taken only when the ship slows or comes near. A change in how physics behaves, so the owner's decision. Large.
7. **Terrain collision from the generator's field, with no bricks** (a signed distance or density query in the contact code). Biggest and riskiest: custom contact dispatch in Rapier, and the same answer to the bit on Windows and Linux becomes much harder. *My opinion:* do not, unless 3 to 6 are shown not to be enough.

## Not measured

Client meshing and drawing at rocket speed; network traffic at that speed; several ships, a hinge, or several players on one server. Any of them may be the next ceiling.
