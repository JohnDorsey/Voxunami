# Motion in frames: why ship bugs keep appearing, and the options

Written 2026-11-12 after the owner asked: the walker-speed bug in a burning ship "seems to be part of a larger category of physical
un-realness bugs that we're having with ships sometimes"; is there a root cause, and is there a better way to represent and transform
player motion so such problems "kinda solve themselves"? The owner's reasoning: a walker's velocity aboard a ship is their velocity
through the universe minus the ship's, so move in the direction of relying on that, without getting rid of physics frames entirely.

**Options 1 and 2 are built (below); the rest is a proposal.** Numbers marked *ran* were measured with a throwaway probe (Windows only; Linux not run). Statements
marked *read* come from reading the code and the older notes, not from running or tracing anything.

## A correction of an earlier claim

On 2026-11-11, after the change record and the test-only renumber (`0bc4014`), I wrote that the speed lost in `agree_with` "isn't needed
while ids only change through a split or merge, so I'd leave it until a rename is built." **That was wrong.** A real `split_grid` of the
deck, with a walker jumping on it, changes the jump today. No renumber is involved.

*Ran:* a deck burning at 5 with a pull of 5, a walker aboard, Space pressed, then the deck split into two halves N ticks after the key.
The walker is then scripted on the part it stands on, and the highest eye rise in the ship's axes is taken over 12 ticks.

| Split N ticks after the key | none (control) | 0 | 1 | 2 | 3 | 5 | 8 |
|---|---|---|---|---|---|---|---|
| Peak rise (m) | 0.6183 | 0.7025 | 0.6647 | 0.5933 | 0.4258 | 0.5667 | 0.6183 |

A split at tick 8 comes after the peak and changes nothing. So undocking or docking with someone in the air changes their jump now. The
effect is small in play, but real.

The probe shows two effects together and I did **not** separate them. *Read:* a lower peak is `agree_with` zeroing the speed on a change
"in or out of a ship" (`body.reframe(position, [0; 3], frame_rotation)`); a higher peak is the "first tick aboard" rule in `felt_aboard`,
which uses zero ship acceleration for one tick (half weight). The probe file was deleted; as a regression test it should be rebuilt from
the description above.

## The root cause: three causes that stack

I do not think there is one. *Read*, confidence medium to high.

1. **Speed is stored against a chosen frame, and the frame's id is part of the data.**
   - The body holds six id fields (`frame`, `buckled`, `carrier`, `departure.ship`, `motion.ship`, `motion.carrier`) and the seat holds
     another (`Seated.ship`).
   - Every change of frame is a hand-written conversion. *Counted by grep* in `games/voxunami_cave/src/movement.rs`: 6 `reframe`
     calls, 1 `change_velocity`, 6 uses of `against_the_planet` / `in_the_world`, 8 of `planet_speed`.
   - Any event that changes an id (split, merge, dock, renumber, rail exit) lands in one of those conversions, or in none of them.
     From the older notes, the double planet speed, the walk-off bug, the rail exit and this one all look like this. The older ones were
     not re-verified.
2. **A ship's acceleration is not data: `felt_aboard` works it out by differencing last tick's velocity** (`Motion`).
   - That needs memory and a special rule for the first tick, and it takes any velocity change for a force. Existing code works around
     this: the buckle comment "so that the tick they are let go is not taken for a blow", and the `carried_through` test helper.
3. **The walker lives in its own private physics world, and ships live in others.** Game code interprets between them. Nothing in the
   engine makes the two agree. This is the deepest cause.

## The Galilean idea

A boost (a change of chart by a constant velocity) is physically equivalent for any choice. So a wrong or stale choice of chart could
cost numerical conditioning but never give wrong physics. Rotation and acceleration are not removable by a boost. Hence option 3 below:
free bodies in the inertial chart, standing bodies in the platform's chart, with a Galilean hand-over between them.

The oracle for any new representation is `games/voxunami_cave/tests/aboard_frame_terms.rs`: it measures the jump rise in the ship's frame
under rest, cruise, free fall, half pull and burn, independent of how the motion is stored.

## Options

All opinions; none tested except the probe above.

1. **Tests that state the owner's sentence directly.** Tests first, before any refactor.
   - Boost invariance: add the same velocity to a ship and to a walker aboard it; nothing relative to the ship may change.
   - Id invariance: split, merge or renumber the ship in the middle of a scenario; the walker must not notice.
   - Cost small; confidence it is worth doing high.
2. **One resolver for every stale id.** Tombstones and the change record can already say "this id became that ship, with this lattice
   transform". Make `Seated`, `Motion`, `Departure`, `buckled` and `agree_with` all ask it. The transform includes quarter-turn
   rotations, so a merged ship's velocity vector must be rotated, not copied. This fixes the verified bug and is the only option that does.
3. **Airborne means inertial.** While a walker is in the air or floating, store velocity in world axes and treat ships as moving
   colliders. A hand-over to the ship's frame happens only on landing, as world velocity minus the ship's velocity at that spot.
   - Gravity and the ship moving under the walker do all the work. Coriolis, centrifugal, Euler, `Motion`, `Departure` and the
     first-tick rule go away for airborne bodies. A standing walker stays in the ship's frame, which keeps the exact carry; the ship's
     acceleration only sets which way "up" points, so an error there is harmless.
   - Open risks, none measured: ships need their collider pose set at each physics sub-step, and a fast spinning hull could catch a
     floating walker.
   - Confidence medium that it works; wants a prototype before a commitment.
4. **Air speed as a function of place, not a latch.** `Departure` exists because the air in a ship moves with the ship. Make the air's
   velocity a pure function of position (the ship you are inside, the planet's atmosphere, or none in vacuum), so it cannot go stale.
   First to settle: how to tell "inside a hull", since the engine has no idea of a sealed room.
5. **Ship acceleration stored as data.** Rails know it exactly, engines know thrust over mass, the solver knows its own velocity change.
   This removes the memory and the blow misreads. It touches `world_storage`, the coasting code and the bodies.
6. **The walker as a dynamic body in the ships' physics frames.** The only option that removes cause 3, and it would give the reaction
   force on a ship when a player pushes. It is the biggest and riskiest: a dynamic capsule feels worse than the kinematic one on stairs
   and slopes, and Windows/Linux determinism was not checked. Advised against for now; confidence low to medium.

## Option 1 built (2026-11-12): what the tests found

Four tests at the end of `games/voxunami_cave/tests/aboard_frame_terms.rs`, which read only the jump and the walk, so they do not depend on how the motion is stored. The fourteen oracle tests above them are unchanged.

- **Boost invariance (pass; ran, Windows).** A ship brought to a velocity (30 m/s along x, 60 along -z, 60 up, 250 along x, and (40, 25, -30)) with the walker buckled, who is then let go and jumps in a rest, a free fall and a burn: the rise agrees with the same run at no boost to 0.02 m. Walking: the walker's distance in the ship agrees to 0.05 m, and their distance in the world is the ship's plus theirs in it. **They bite:** with `felt_aboard` mutated to take the ship's acceleration from its speed and not from the change of it, both fail (so do eight of the oracle's). So today's stored form is Galilean-invariant for a walker who stays aboard; the bugs are not there.
- **Id invariance (failed before option 2; ran).** A ship given a new id (`renumber_ship`) or cut in two (`split_grid`) between 0 and 5 ticks after the jump key. In free fall the jump is lost: the highest rise is 0.18 to 0.9 m against 2.16 m with no change (the speed is zeroed, so one tick of the jump is all there is). In a burn it is 0.703, 0.665, 0.593, 0.426 and 0.567 m against 0.618 m. A renumbering gives the same numbers as a split to every digit, so it is the change of id and not the cut that does it. These two were `#[ignore]`d with the reason until option 2, below.
- **Not covered:** a merge or dock (its lattice transform can turn the walker's speed), a walker who is buckled or in the seat when the id changes, a ship on a rail, a walker in a level carried by a planet. All are the next tests.

## Option 2 built (2026-11-12): the resolver

`games/voxunami_cave/src/renaming.rs`: `successor(grids, old, prefer)` says what a retired id became (from `GridStore::resolve`, the tombstones) and the transform from the old ship's coordinates to the new. Every holder of a ship id asks it:

- **The walker's body** (`follow_renamed_ships`, `movement.rs`, called before `agree_with`): the frame, `buckled`, `Motion.ship` and `Departure` (ship and place). The body is put where the record says (the world moved the record with the transform) and its speed in the ship is **turned by the transform's rotation, not zeroed** (`LatticeTransform::apply_direction` and `apply_position`, new in `voxel_space` with property tests). A ship that is merely gone still goes to `agree_with`, as before.
- **The pilot and the seat** (`follow_ships_that_were_replaced`, which had its own copy of the `resolve` logic, now uses `successor`; `Seated::follow` carries the seat voxel). The seat was dropped on any change of id, by design of the doc comment; nothing tested that, and now a pilot keeps the seat and the throttle.

Results (ran, Windows): the two id tests pass (within 0.02 m of the control) and are no longer ignored; a merge that turns the lattice a quarter, mid-jump in free fall, gives the same flight through the world as no merge (the walker goes up, not sideways); a buckled walker is still buckled after a renumbering or a split; a seated pilot keeps the seat and the throttle through a renumbering. **They bite:** with the speed not turned the merge test fails (the walker went (-1.62, 0.54, 0) and not (0, 2.16, 0)); with the `buckled` id not followed both buckle tests fail; before the fix both id tests and the seat test failed.

**Added after (2026-11-12), the three that were left:** a walker on a ship **on a rail** (`renumbering.rs`: floor in orbit, a jump, the ship renamed two ticks into it; they go 1.62 voxels in eight ticks, and 0.18 with the resolver off); a walker in a **planet's carry** (`walkers_near_a_moving_planet.rs`, both the moving planet and the one that stands still: one jump on a held deck, the deck renamed 0 to 4 ticks in; 1.13 voxels with the resolver, 0.33 off); and a **`Departure`** (`aboard_frame_terms.rs`: a walker who walked off a deck that cruises at 50 m/s, the deck renamed or cut two ticks later; they go 30 voxels in twelve ticks as with no change, and 28.2 with the resolver off, the air having braked them from the level's speed). All three fail with the call to `follow_renamed_ships` taken out and pass with it. (The planet test first held the jump key all twelve ticks, and a walker whose speed was zeroed jumped again on landing: with the resolver off it differed from the control by 0.034, nearly under its tolerance; one jump made it 0.8.)

**A split of a spinning ship, measured (2026-11-12), and fixed.** I had read, not measured, that a cut of a spinning ship would give the walker a one-tick error of spin times the shift of the centre of mass. Measured: a deck turning about the vertical through its centre of mass, a walker standing on it, the deck cut in two (`a_walker_on_a_deck_that_turns_steadily_is_not_moved_by_the_deck_being_cut_in_two`, `aboard_frame_terms.rs`; the deck is posed as a rigid body that really turns, each part's speed that of its own centre of mass). **With no turn the cut changes nothing (1e-5 voxels), so the set-up is sound. With the turn it was a good deal worse than I read:** at 0.02 radians a second (a turn in five minutes) the walker's eyes moved 0.27 to 0.44 voxels, at 0.1 a second 0.63 to 0.67, and from 0.15 a second (a cut that moves the centre of mass 10 voxels) **the walker was thrown out of the ship in the tick of the cut**; at 0.4 all three cuts threw them out. The cause, found by taking it out: `Motion` held the old centre of mass's speed, and the part's centre of mass is elsewhere, so the difference of speeds (spin times shift, over a 50 ms tick: 2 to 80 metres a second a second sideways in these cases, against a pull of 5) was taken for the ship accelerating, and the walker's up leaned over. The fix: `Motion` also holds the acceleration the last step measured and the ship's centre of mass, and at a rename the old speed is restated as the new ship's speed less one tick of *that acceleration plus what the shift gives* (`spin x (spin x shift) + spin change x shift`, with the shift carried by the transform). Residual after: at most 0.009 voxels at 0.4 radians a second (0.0007 at 0.1; 3e-7 at 0.02). The version with the measured acceleration carried over and not the shift's term left 0.11 to 0.22 voxels at 0.4, which is why the term is there. This also covers a merge or a dock whose plan gives the new ship another speed: the ship is taken to have been going at it for the tick, so the walker is not struck by it (they keep their speed in the ship, as before).

## The id-change coverage step (2026-11-12): what was added and what it found

The owner approved the suit-thruster choices (75 kg and 350 N, C for the block shapes, air control in a pull, the 0.05 dead zone, sitting buckles, the carrier's free-fall field) on 2026-11-12, and asked for the coverage that the first two options left. Added, all in `games/voxunami_cave/tests/` (run on Windows; all pass; the full gate is at the end of this section):

| Case | Where | Worst difference from the same run with no change | With the whole resolver switched off (ran) |
|---|---|---|---|
| A deck turning steadily (0.02, 0.1, 0.4 rad/s) cut in two | `aboard_frame_terms.rs` | 0.009 voxels | passes at 0.02 and 0.1; 0.11 to 0.23 at 0.4 |
| The same, **burning** at the pull along the axis (0.1, 0.4) | same | 0.007 | passes at 0.1; 0.06 to 0.11 at 0.4 |
| The same, the **turn getting faster** by 0.2 rad/s/s (from 0 and from 0.1) | same | 0.013 | 0.14 to 0.28 |
| The deck **merged with a larger ship turned a quarter** (0, 0.1, 0.4) | same | 0.007 | passes at 0 and 0.1; 0.23 at 0.4 |
| A cabin on a **rail** that turns at 0.8 rad/s, a jump, **renamed** in the air | `renumbering.rs` | below 0.02 | fails (the speed in the ship is lost) |
| The same cabin **cut in two**, the tick of the cut | same | below 0.005 | fails |
| A walker who **stepped off a floor on a rail**, which is renamed (`Departure`) | same | below 0.3 of 495 voxels | fails (braked from the level's speed) |

Reading the last column: a walker who **stands** is hardly hurt by the resolver being off at low spins (their speed in the ship is zero, and the "first tick aboard" rule happens to dodge the spurious acceleration); it is the jumping, the fast turn, the burn and the merge that show it. The larger figures at the top of this page (0.27 to 0.67 voxels at 0.02 to 0.1, a walker thrown out from 0.15) are from the in-between state of the first resolver commit (`6167ebd`): the rename kept the walker's speed but did not yet correct the acceleration, which is a worse state than none. That commit's own tests did not reach it because they cut a deck that did not turn.

**Each piece of the correction has a test that only it makes fail** (mutations, ran): the Euler term (`spin change x shift`) only the spin-up test; the centripetal term (`spin x (spin x shift)`) all of the turning ones and the rail cut; the measured acceleration carried over only the burn-and-turn cut and the older free-fall merge; the transform of the old centre of mass only the merge; the departure's new id only the departure test. With the whole call to `follow_renamed_ships` taken out, every new test fails (ran, in the last column above).

**No error was found in the correction.** What looked like some was not, and is worth knowing:
- *A cut of a free spinning cabin changes what the walker feels, rightly.* A part cut off a ship on a rail has no rail: its centre of mass goes straight on, and it turns about **its own** centre of mass, so a walker who was 3 voxels from the old axis is on the other side of the new one, is pulled toward the open end (the pull along the cabin went from +1.9 to -0.6) and falls out. That is what a cut does to a spinning habitat. The scripted decks do not do this because the script keeps the parts turning as one assembly; so the deck tests say "the walker is carried across the change of id correctly", not "a cut changes nothing". The rail cut test compares only the tick of the cut.
- *Two test scripts were wrong, not the game.* The speed a merge's plan was given was a tick old (the script computed it before the ships turned), and the script overwrote a merged ship's speed with one worked out for its not-yet-weighed centre of mass `[0,0,0]`. Both made a merge look like a jolt of 0.2 to 0.8 voxels until the script was fixed (the merge is now done last in the tick, as the docking of the game does it, and an unweighed ship's planned speed is left alone).
- *The merged ship is weighed before the walker steps.* The dock runs first in `engine:grid_bodies_move`'s step and the ship is weighed later in the same step; Cave's movement runs after it. **Read from the code, not run** with the game's own docking and a walker aboard.

Still not covered or not done: a **docking** (a merge by ports) with a walker aboard a spinning ship, which would run the game's own path; a ship on a rail that is **merged** while it turns; the **change of the centre of mass by an edit** (a block placed or broken on a spinning free ship), which may jolt a walker the same way and is not a rename, so none of this reaches it (not measured); the planet ids (kept); and Linux for all of it.

## Recommendation

Options 1 and 2 first: they fix the verified bug and put the owner's sentence into a test. Then prototype 3 and 4 against the tests that
already exist, and decide on 5 and 6 from what that shows. Not checked: Linux, the planet-carrier case, and the older bugs placed under
cause 1.

Related: `decisions/players-and-ships.md`, `decisions/joints-and-cars.md`, `history/ship-rail-frames-review.md`.
