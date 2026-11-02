# Joint probe: do Rapier joints hold when over-constrained? (2026-11-01)

Throwaway programs in `spikes/joints/` (not kept compiling; Rapier 0.36.1 with `enhanced-determinism`, f32, no gravity unless said). They back the choices in `decisions/joints-and-cars.md`.
All numbers are from one run on the owner's machine. A "step" is one Rapier step of 12.5 ms.

**A correction first.** In the first runs this was called "iterations". `IntegrationParameters::num_solver_iterations` is the number of **substeps** the step is divided into
(`dt /= num_solver_iterations`, `staged_island_solver/init.rs`); the solver passes inside a substep are `num_internal_pgs_iterations`. Every table below varies substeps unless it says "pgs".

## Results (impulse joints, ball joints, 2,400 steps)
| Test | Result |
|---|---|
| Door on two consistent joints, 4 m apart; A fixed, B spun | worst pivot gap 0.004 mm (4 substeps) and 0.002 mm (16, 64). B's final spin 0.457 rad/s, as angular momentum says. |
| Three joints not on a line (rigid) | 0.024 mm (4), under 0.001 mm (16, 64) |
| Four-bar loop of four joints | 0.007 mm (4), 0.001 mm (16, 64) |
| Mass ratio 1,600:1 and 12,800,000:1 (small slab on a free huge cube, two joints) | 0.002 mm at both |
| Light bar between two huge blocks | under 0.001 mm |
| Door swung into a fixed wall (solid colliders) | 1.2 mm (4), 0.013 mm (16), 0.001 mm (64); no jitter; the door recoils at about 2 cm/s (not investigated) |
| Second joint's pivot off by 0.1, 1, 10 mm | gap 0.14, 1.4, 14 mm: permanent, 1.44 times the mismatch, no growth, no visible jitter |
| Whole door moved x metres from the origin | worst gap 0.002 mm at 0 m, 0.008 at 100, 0.06 at 1 km, 0.98 mm at 10 km; **the same at 16 and 64 substeps** |
| Multibody set | refuses a second joint on a pair and a joint that closes a loop (`insert` returns `None`; the source says so at `multibody_joint_set.rs:162`) |

- **The error floor is f32 rounding**, not the solver: it grows with the distance from the frame origin like the f32 step at that distance. `FRAME_RADIUS_BRICKS` is 500 bricks (16 km), so a joint at the
  edge of a frame is about 1 mm out. Ways down: a local origin per coupled group, multibody joints (exact in the tree, though the world pose is still f32), or `rapier3d-f64`.
- **Not valid:** the "tree under gravity" energy figures (my metric broke: the cars fall 4 km in the run). Its pivot gaps are valid: multibody 0.000000 m at 1:1 and 8,000:1, impulse 0.000488 m at 8,000:1.
- **Multibody velocity rows of the first run are not valid** (a body's velocity set directly is ignored by a multibody link); only the refusals are results.

## Speed noise, and why 64 substeps looks worse than 16
"Speed" is the swinging car's linear plus angular speed; the true value is 1.14286. `std` over steps 2,000 to 2,400 was mostly a **slow drift**, not noise.

| substeps | speed error | step-to-step jitter (rms) |
|---|---|---|
| 4 | -0.61 % | 0.0000033 |
| 16 | -0.06 % | 0.0000040 |
| 64 | +0.37 % | 0.0000162 |
| 256 | +1.60 % | 0.0000618 |

- Passes per substep (`num_internal_pgs_iterations` 1, 4, 16, 64) change nothing (-0.61 % at 4 substeps for all of them).
- Two effects meet: a **loss** that falls about tenfold from 4 to 16 substeps (the integration step is too big), and a **gain** that grows in proportion to the number of substeps (64 to 256 is
  four times the substeps and 4.3 times the gain). 16 is where they nearly cancel. The cause of the gain is *not found*; I believe it is f32 rounding added at every substep, but have not checked
  (a run with `rapier3d-f64` would say). Consistent and 1 mm-mismatched pivots behave alike.
- So: more substeps is not better. 16 is the value to use; do not raise it for accuracy.

## The move to Rapier in f64 (2026-11-01, owner's decision)
`engine/physics` now uses the `rapier3d-f64` package (0.36.1, `enhanced-determinism`), renamed in `Cargo.toml` so the source still says `rapier3d`. Its interface keeps `f32` for speeds, forces and the offset in a
`WorldPosition`; `engine/physics/src/real.rs` is where numbers cross. Orientations already were `f64` and now reach the solver unrounded. The position of a body is carried to whole bricks in `f64`, so only the offset in a
brick is rounded to `f32` (about 2 micrometres), not a place that is 16 km from the frame's origin (1 millimetre). `CharacterSettings` is `f64`.

What was checked, and how:
- **Windows and Linux agree to the bit.** The pinned hashes of the physics crate (ships, characters, ships against characters, terrain boxes) were taken on Windows and pass under Linux (WSL).
- **Every test of behaviour passed unchanged** in `physics` (39, 19, 14, 43); only hash pins moved, and a note by each says why. Two tests of `voxunami_cave` were changed in what they claim, not only in a number:
  a held ship's turn is exact and a moved ship's is the solver's `f64` one (the test used to claim that the solver rounded to `f32`), and a walker on a planet that goes round a rail is within a tenth of a millimetre of
  where it stood (it was the same to the bit). Measured every ten ticks, that walker moves 4e-6 of a voxel in the first 80 ticks and then rests; with the solver in `f32` it did not move. **The cause is not found.**
- **The box counts of the 24 terrain bricks are the old ones**; only their hash moved.
- **A hard impact resolves differently, and the new answer is the nearer to the converged one.** Busy scene of `bodies.rs`: a ship of 4, turned, dropped from 14 m, hits the ground at 25.6 m/s, which is 0.43 m a substep.
  The two builds are the same to four places until that impact. After it, with the usual 3 substeps, the old build gave a velocity of about (2.5, 5.0, 5.6) and the new about (6.2, -3.9, 7.1). With 12 substeps both give
  about (6.2, -3.9, 7.1), within a fifth of a percent of each other. (With 48 the two agree at the impact to a percent and then tumble differently.) One scene, so it is evidence, not proof. A start moved by one `f32` step
  (5e-7) changed nothing in either build, so this is not rounding noise amplified.
- Not found out: why the old build's coarse answer differs. A guess is that some constant in the contact code is `f32`-sized, which I have not looked for.

## The walker that creeps and hops (investigated 2026-11-01)
**What it is.** A character standing still on a flat floor, when the direction of gravity (so `up`) is not exactly the floor's vertical and keeps changing a little, is moved by the controller.
Reproduction: `spikes/joints/walker_creep_test.rs` (a test for `engine/physics/tests/`, run in the build before the f64 move at `ad636db` and after it).

**Verified, by tracing every writer of the character's centre and the controller's move at each step:**
- On a planet the pull points at the planet's centre, so on a flat voxel floor 0.5 voxel from the pole's axis, 10 km up, `up` is tilted 5e-5 rad from the floor's vertical.
- With the tilt *constant* neither build moves at all (600 and 3,600 steps). With it *changing* both move; the change can be as small as 1e-12 a step in f64.
- Every downward move of the controller (a gravity step, 2.2 mm) then has a sideways part of its length times the tilt (2.2175e-3 x 4.98e-5 = 1.1044e-7, seen to four places). That is the creep, toward where
  the pull points: about 1.7e-8 a tick in the planet scene (a millimetre an hour), 3.6e-6 a second in the reproduction with a changing tilt (f32: 4e-9).
- The controller keeps a skin of 0.01 and pushes a body 1e-5 off a surface it touches (`normal_nudge_factor`). At rest it cycles: 1e-5 up, back down; in the reproduction, every second or so, three
  gravity steps (2 to 7 mm) take it into the skin and the next pass, which has almost nothing to move, pops it out by up to 1.1 to 1.6 cm in one go (traced: the height before and after each stage of a tick).
  The height of the eyes therefore **hops by up to 1.6 cm, about once a second**, in f64 at any rate of change of the tilt tried (1e-12 to 1e-6 a step; 87 to 103 hops in 120 s), **when the tilt is already there
  (5e-5) from the first step**. The f32 build does the same only when the tilt changes by 1e-6 a step or more (0.0034 degrees a second; 86 hops in 120 s), and not at all below 1e-8 a step.
  **Corrected in the turning-ship study below:** when the character stands level first and the tilt then grows from nothing, f64 does not hop at slow rates (largest jerk 1e-5 at 1e-4 rad/s). What decides it is not found.
- So the f64 move did not make the hop up; in one setup it makes a behaviour that needs a fast change in f32 happen at any change.

**Not verified:** why f32 does not enter the cycle at small changes. The guess is that a distance in the contact code is compared with a constant sized for f32 and the move is dropped, which I have not looked for.
**Not known:** how often a player meets a gravity that changes. Standing on a planet that goes round a rail the change is about 5e-12 a step (from the creep's own feedback), which the reproduction says is enough to hop;
but the planet scene (`bodies.rs`, 200 ticks) showed no hop, only the 1e-5 nudges, so the reproduction may be harsher than the game. A ship that turns under a standing player changes it far faster.

**Can a player notice it?** 1.5 cm of eye height is about 13 pixels at a metre's distance (1080 lines, 70 degrees) and 2 mm about 2; a hop about once a second would be a visible shimmer of the floor and things on it,
if it happens in play. Not tested in play. The creep (a millimetre an hour) cannot be seen.

**On turning ships** (`spikes/joints/walker_turning_test.rs`; the character stands level for 4 s, then the pull turns in its frame at a steady rate, about one horizontal axis or a diagonal one, until the tilt is 0.6 rad,
which is what a player aboard a ship that rolls is given; physics crate only, not played). In both builds:
- Rates of 1e-4 and 1e-3 rad/s: at most one jerk of over half a millimetre in 60 s (f64: none at 1e-4).
- Rates of 1e-2 to 0.2 rad/s: **a jerk of about 1 cm (the skin) every few degrees of roll**: f64 4 to 9 in the 34 degrees, f32 7 to 23. f64 is not worse here.
- The character also **slides 0.2 to 0.4 m sideways during a 34 degree roll**, at any rate (the box turns about its middle, so the feet swing and the controller moves the middle); the eyes sink 4 to 11 cm. Same in f32.
So on a ship that turns, a standing player's eye height jerks by about a centimetre several times during a roll, before and after the f64 move. Whether the camera shows it is not tested.

**A fix is not made.** Options: do the gravity step along the floor's normal (or skip it) while the character is grounded and has no velocity along `up`; drop the sideways part of a downward move that is shorter than the
skin; keep the shape's turn unchanged until `up` has changed by more than a threshold. Each changes how a character stands and needs the regression test the reproduction can become.

## The walker that does not climb the stair it faces exactly (investigated 2026-11-02)
**What it is.** A character walking exactly along an axis into the foot of a stair (half a block per step, under the 0.6 it climbs) stops at it: `yaw` 0.0 forwards (the default look direction), and 0.0 with
`forward` -1 or `strafe` +-1 for the other three directions. One 1e-9 radian off the axis climbs it. Present before the f64 interface (commit 0b17130, `f32` yaw 0.0), so not caused by it. It stayed
unseen because the stair test walked at `90.0f32.to_radians()`, 4e-8 off the exact quarter turn.

**Verified** (by tracing every move of the controller, each hit with its normal, and by repeating the casts of its stepping from outside on the same poses; the scene is the test's, eyes stop at 11.63, z 14.31):
- The stepping (`handle_stairs`) is entered and returns `false` at its **first** test, the cast of the body straight up by the step height (0.61). That cast **hits**, with the stair's own face as the normal:
  `(1.1e-7, -2.2e-7, 1.0)` at time 0.0047 in the forward case, `(1.1e-14, -1.1e-14, -1.0)` at time 0 against a face at z = 19 in a backwards one. The casts forward at the raised height and down onto the step are clear.
- The body is exactly the skin (0.01) from the face. The controller stops a body that runs into something at the `target_distance` of its cast, which is the skin, and the stepping's up cast uses the
  same `target_distance`, so it starts at the distance at which a cast counts a hit, and goes along the face. In parry (`shape_cast_support_map_support_map.rs`) a hit that starts there is dropped
  only if the cast is moving away from the contact (`normal_vel >= 0.0`); along a face it is neither, and the sign of the rounding of the normal decides.
- So it is a toss that the walker cannot win by anything it does. Over four directions from four places each, 3 of 16 stop with the nudge at 1e-5. With the nudge at 1e-4 (Rapier's default) the original
  case climbs and **two other places stop** instead: the nudge only changes which places lose. (The nudge of 1e-5 is also exactly the controller's cutoff for a move worth making, `len > 1e-5`, so a
  move that is only into the face leaves a nudge that is dropped: at yaw 0 the body ended at z = 14.31, not 14.31001, while at 1e-9 the nudge was made. That is consistent with the trace, not shown by a
  count of it. It is not the cause: with the nudge always made, places still stop.)
- At a yaw of 1e-9 the first hit of the move is the floor, the nudge lifts the body 1e-5 and the second hit is the face at time 1e-5; the stepping then goes through, which can only happen if the up cast returned
  nothing. That cast is the same graze and came out the other way.

**The fix** is a second try (`CharacterBody::step_up`) when the controller's stepping has declined: see `decisions/players-and-ships.md`. The controller's ordinary moves are not troubled by the graze (a hit
there is a slide along the face, which they are made of), so the try is built from three of them. Rapier's stepping is tried first and what it climbs is untouched: the pinned hashes of the scripted run and of
both ship runs are unchanged. The stricter test (four directions, four places) fails 3 of 16 before and none after; the original test passes.
**Mutations** (each applied alone to `step_up`'s code): taking the second try away, the raise check, the room check, the stopped-by condition, the grounded condition and the slope check are each caught by a
test (the room check also by a ship run's hash). Two checks that an earlier version had, that the body is grounded after the drop and that it is not lower than it began, were not caught by anything, and
cannot be: a floor was hit if the body is grounded, and a body that is walking moves less than its width a step. They were removed.

**Not verified.** That other casts of the controller's stepping are free of the same toss (the one forward at the raised height and the one down onto the step are clear in everything traced, which is not a
proof). The cost of the second try: it runs only on a step where a grounded walker is stopped by a wall, with two to four extra moves; not measured. Linux: see the commit message for what was run.
**Not done.** `normal_nudge_factor` stays 1e-5; whether the nudge being dropped when it equals the cutoff matters anywhere else is not looked at.

## What reading a ship back as f32 and giving it to the solver again costs (2026-11-01)
Rapier keeps each body's pose and speed in `f64` in the `BodyWorld` of its frame between ticks. After each step the plugin reads the state and writes it into the grid (`write_back`: the offset in a brick and the speeds
become `f32`); it gives a ship to the solver again (`set_state`) only when something else changed the grid (`moved` in `sync_ship`, comparing with what it wrote), when it is first added, when it moves to another frame
(after 2 x 500 bricks, 32 km), when it is frozen (the grid's speed is zeroed, so the comparison differs), and when it is loaded. Measured (`spikes/joints/refeed_test.rs`), a body at 840 m/s for an hour (72,000 ticks),
against the same run left alone: read back and given again every tick, 2.7 cm out; every 20 ticks 0.8 mm; every 400 ticks (about what frame moves are) 0.03 mm; ten times 0.003 mm. A slow turning body, every 400 ticks: 0.05 mm.
Re-rounding a value that is already an `f32` changes nothing, so a ship that sits still through any number of freezes and loads loses nothing after the first. A frozen ship loses its speed *by design*
(`set_grid_frozen`), which is not a rounding.

## Determinism
`rapier3d-f64` 0.36.1 has the `enhanced-determinism` feature (crates.io metadata: `simba/libm_force`, `parry3d-f64/enhanced-determinism`). In the source it replaces the hash map of pairs with an ordered
`IndexMap` and fixes the transcendental functions to libm; it cannot be combined with `simd8`. Without it the solve order can differ between runs on one machine (Rapier's own regression test for
issue 868) and `sin`/`cos` differ between platforms: either breaks the rule that the simulation is bit-identical on Windows and Linux. Not built or run with f64 here.
