# Places: `/tp` for more than one planet

Status: built 2026-10-27 (owner's request to redesign the teleport command for several planets). What I checked by running code is marked **[run]**; what I believe from reading is **[belief]**.

## 1. What the owner decided (2026-10-27)

1. **Names are namespaced** (`voxunami_cave:planet`, `moons:moon`).
2. **A body is reached on the ground nearest to the player.** Coordinates are optional: **two numbers are a latitude and a longitude, three are x, y and z** (of the body).
3. **Everyone may use it for now.** It should someday be operator only (`future-goals.md`, unscheduled).

## 2. The grammar

| You type | What happens |
|---|---|
| `/tp` | lists the places and the ways to say where |
| `/tp X Y Z` | the old form: the eyes at these coordinates of the level you are in (exact for whole numbers of any size, as `i128`; fractions are kept) |
| `/tp PLACE` | a body: the ground nearest to you. A landmark: in view of it |
| `/tp PLACE LATITUDE LONGITUDE` | a body: the ground at that latitude and longitude, in degrees |
| `/tp PLACE X Y Z` | a body: that place in the body's own axes, in voxels |

A first word with a colon is a place; any other first word is read as the `X` of the old form. A place in another level takes the player there (`/level go`'s move, then the arrival).

**Latitude and longitude** are of the body's own axes, so they turn with it: the north pole is its +y; longitude 0 is along +x; longitude 90 is along -z (east is counterclockwise seen from the north pole); latitude is -90 to 90 and a longitude is any number. The pole of the first planet (`/tp voxunami_cave:planet 90 0`) is the column where a new player stands.

## 3. What a place is

- **A body**: any ship with terrain. **Its name is the name of the generator that makes its ground**, which a body's file already gave it (`voxunami_cave:planet`, `moons:moon` for `moons/bodies/moon.json`). Nothing was added to the world file or to a ship: **[checked]** `add_bodies` makes the recipe's instance name the body's identifier, and a test reads it back. A planet that has no body file has the name of its generator (`base_blocks:small_planet_v1` in an old world).
- **A landmark**: a knot of a great river. `base_blocks:great_river_narrowest` and `base_blocks:great_river_widest`, in the first level whose terrain is the rivers. A landmark takes no coordinates (`/tp base_blocks:great_river_widest 1 2` says so). It replaces `/river` of the first prototype, which also had `/river N` for any knot of the river; **that is not kept** (a place per knot would make the list forty names long, and nobody asked).
- `places::places` makes the list from the world each time (a planet that is put in a world later is a place at once). New kinds of place are a variant of `PlaceKind` and a branch of `go_to_a_place`; they are in the Cave crate until a second game wants them (the principle: new code starts in the game that needs it).

## 4. How the ground nearest to the player is found

1. **The direction.** The player's eyes in the world are taken into the body's own axes (`GridPose::world_to_local`), and the direction is from the body's centre to them. A player who is not in the body's level, or is exactly at its centre, has the pole.
2. **The ground.** The line is followed from where it leaves the box of the body's extent in toward the centre, and the first voxel that is not air is the top of the ground there (**not** a column along an axis, as `landing::on_a_planet` has, so it works at any direction). A brick that is air all through is crossed in one step to the face the line leaves it by; inside a brick the line is walked in half voxels. It reads the voxels the way a walker does (the stored brick, else the generator's), so what has been dug or built counts.
3. **The eyes.** At the middle of that voxel, then along the direction by half the sum of the parts of the direction (the extent of a voxel along it) and 1.64 voxels (`landing::EYES_OVER_THE_GROUND`). For the pole this is the same place `on_a_planet` gives **[run: the pole test]**.
4. **A check of the answer.** The voxels at the eyes and a metre under them are read again and must be air; if not, the command says "the place found to stand on is not in the air" and moves nobody.
5. **To the world.** Through the body's pose, so a body that is turned or moved, or is a moon, works **[run: a planet turned a quarter, and a moon 400 km out]**.

A cave under the surface is not found (the line stops at the first ground from outside). A body with ground on no part of the line (a ring) gives "there is nothing to stand on there".

## 5. What was checked

- **Tests (`voxunami_cave/tests/places.rs`, 10; `places`, 3 unit tests; `great_river.rs`, 7).** The list names the planet, the landmarks and the ways to say where; a second planet (a pack's moon) is a place by its own name; **nearest** from 36 km over the +x side and from over -x and up (latitude 16.7, longitude 180.0 reported); **latitude and longitude** at the pole, the equator at 0, 90, 180 and -90 of longitude, the south pole and 45 degrees, each on the line to within 1.4 x 10^-4 radians (9 m at the radius of the planet; it is about a voxel) and **never told they were moved out of the ground, and at rest within 3 m of where they arrived after ten seconds** (a player put high would fall); **three numbers** in the body's axes, with fractions; **a planet turned a quarter** has its pole at the world's +z and its axes its own; **a moon** at 400 km, on the side that faces the player, and the planet again from it; **from the overworld** the planet is reached at its pole in its level, and the player's own client is told of the level (a mutant that left that out was alive until this was asserted); every refusal says why (no such place; one or four numbers; a latitude outside -90 to 90; not a number; coordinates for a landmark) and moves nobody; a world with no planet has no such place.
- **Mutation experiments** (10 mutants of the arrival and the command). Eight were caught by the tests as first written (the eyes 30 voxels up, the longitude's sign, the pole always, the pose ignored, a skip over air that reaches into the ground, a latitude limit of 95, three numbers ignored, a floor that truncates; the pole one needed its mutant written again, it did not compile). **Two lived and were closed:** (1) a missing `move_to_level` (the server ends in the right level, the client is not told): the tests now check the client's level. (2) Eyes put 3 voxels *inside* the ground: the ticks after the arrival push the body out of the planet's ground, and `say` runs two of them, so no test can see where the player was put. The arrival now **checks its own answer** against the ground (the voxels at the eyes and a metre under them must be air, or it refuses with "the place found to stand on is not in the air"), which is the kind of check that turns a silent fault into a loud one; the mutant fails the tests through it.
- **The golden session** moved by one byte (the list of commands in the reply to `/nonsense`: 7,204,133 to 7,204,132), worked out before it was run.

## 6. Known, and not done

- **A player arrives in the level's frame, not aboard.** The rule that puts a walker aboard a ship probes one point under the feet along gravity, so on a slope a player who stands may not be aboard (seen: latitude 45 and latitude 16.7 at longitude 180, standing for ten seconds, not aboard **[run]**). That is the rule of the ships, not of the arrival; the walker is held up either way.
- **The body of a player turns upright to the pull as it falls**, which moves the eyes about a metre in the first ticks when the planet's up is not the level's (the same for `/tp X Y Z` **[run]**).
- **A body that moves** (on a rail) is where it is when the command is run; the player is not carried by it until they stand on it (the carry of a planet's sphere of influence does that for anything near).
- **The facing is not set for a body** (a player looks the way they looked; only the landmarks put the place straight ahead of where they look, as `/river` did): the server cannot set the way a player faces, since `PlayerPlaced` has a position only. A `facing` option wants that message to carry the angles.
- **A body far from the origin in precision**: the direction is worked out in `f64`, so the ground is found to a voxel only for a body whose own coordinates are inside 2^53 voxels (as every pose is).
- **Not operator only.** See `future-goals.md` (unscheduled).
