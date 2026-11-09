# Joints and cars: what was decided (owner, 2026-11-01; the solver changed 2026-11-09; being built on the branch `worktree-dev-ball-joint`)

A joint lets one ship have parts that move against each other. The first one is a prototype, the **dev ball joint**, a block that couples two cars like a ball hitch.
This file records the talk that came before any code; the measurements behind the physics choices are in `joint-probe.md`. Status key as in `design-decisions.md`.

## Words (owner)
- **Ship** is the whole vehicle; a **car** is the blocks of one grid of it. (The code's `Ship` is one grid; it is to be renamed to say so, a mechanical change, not yet made.)
- **Dock / undock** are used only for docking ports, and **never** for anything else. **Lock / unlock** are the same act for joints. **Dock and lock merge grids; nothing else does.**
  Undock and unlock split them.
- **Couple / decouple**: two joint blocks come together and hold, or are parted. (Not "connect": that word is the network's, `ServerMessage::Disconnected`.)
  Coupling is automatic when the blocks meet; decoupling is a command. **Mobilize / immobilize**: let the joint move, or make it rigid at its present angle, without merging.
- "Hinge" is reserved for a joint with one axis. "Port" is not shortened from "docking port", because it may name fluid attachments later.
- The command is `/joint lock|unlock|mobilize|immobilize|decouple`. **Decouple is always available**: on an immobilized joint it mobilizes and decouples in one step, on a locked one it
  unlocks and decouples in one step.

## The block
- Id `dev_ball_joint` (creative only, like the dev docking port, which stays as it is). 0.75 x 0.5 x 0.5 m. It is put on the face that was clicked, facing away from it, one small
  end flush with the face and centred, the other end 0.25 m short of the next cell. It has **its own face property, `engine:joint_face`**, and its own finder, so that `ports_of`,
  `docked_pairs` and `dock` never see a joint as a docking port. The geometry the two share (`Port`, `PortView`, `approach`) gets a neutral name. A block may later have both properties
  (a joint that is also a docking port).
- Two such blocks facing across a cell boundary, 0.5 m apart at the tips, are **coupled**; a ball of 0.5 m diameter is drawn in the gap, and only then. The ball is two hemispheres, one in
  each cell: bent, the join shows; that is accepted for the prototype.
- States of a pair: apart, coupled (mobile), coupled and immobilized, locked. A joint is **derived from block states and positions and never saved**.

## Coupling, lock, unlock
- Couple when the pivots are within 0.3 m and the faces within 45 degrees; until then pull with 10 kN each (as for docking, independent of mass) until the pivots are within 0.05 m, then the joint.
  Coupling applies positional forces only, no torque toward 180 degrees. **Agreed 2026-11-09:** as for docking, the two cars must also be close in velocity when the joint is made (a limit on their relative
  speed; the number is not chosen), because a joint made with a relative velocity begins with a collision that loses energy and leaves a gap (found building the first tests, `build-log.md`).
- **Lock** is the only thing that turns the cars toward alignment (180 degrees, or the nearest turn that lines the grids up cube for cube). Within the docking snap (0.3 m, 10 degrees) it snaps
  and merges the grids. A lock request lasts until it succeeds or the joint is mobilized; on an immobilized joint it is refused with "mobilize first".
- **Unlock** splits the grids and lands in *coupled (mobile)*. Lock and unlock act on the **pair of cars**: unlock cuts every joint between them at once (the flood fill in `undock` returns
  `Welded` if the halves touch anywhere else).
- Cars collide with each other, and the two cars of a coupling too: **contacts between them stay on (owner, 2026-11-09).** Coupling across physics frames is refused for now.
- **The dev ball joint lets one car pitch, yaw or roll against the other (owner, 2026-11-09):** the ball turns freely about all three axes (what a spherical joint is). No angle limit has been chosen; the
  hulls stop a bend where they meet (the cars are 2 m apart at the pivot's ends at rest).
- **A joint never breaks, for starters (owner, 2026-11-09).**

## Physics (decided for the prototype; the reasoning is in `joint-probe.md`)
- **Rapier works in f64** (done 2026-11-01, before any joint was built): the joint error floor was f32 rounding of the distance from the frame's origin (about 1 mm at the frame's edge), and it is now rounding of an `f64`.
  The measurements above were taken in `f32` and are to be repeated in `f64` when the joint is built.
- **Decided by the owner, 2026-11-09: the first solver is Rapier's impulse joints, one spherical joint for each pivot pair of a coupling.** This replaces the multibody tree this file first chose (2026-11-01):
  `loop-probe.md` found impulse joints conserve momentum to rounding and cost about linearly in the cars, and a multibody neither. So a second coupling on the same pair and a coupling that closes a loop
  are not refused by the solver (the integer consistency rule below still applies to a second coupling of one pair), the root car and the heaviest-car rule are gone, and an immobilized joint is not a
  fixed multibody joint (how to immobilize is decided when it is built). The substeps are no longer fixed at 16 (that was measured in f32, `joint-probe.md`): the joint gap at the game's settings is 0.85 mm
  and falls about four times for each doubling of the substeps (`loop-probe.md`, section 14), so the substeps and passes are dials (Rapier keeps them per body: `additional_solver_iterations`,
  `additional_pgs_iterations`). **A coupled car gets extra substeps by default (owner, 2026-11-09: yes; the number proposed is 4 extra):** a car a thousand times lighter than its partner, swinging
  fast, left a gap of 6.0 mm at the game's settings and 0.16 mm with 12 extra (`build-log.md`, "Joints, step 1").
- **Planned, not built: the owner will set the kind of joint per ship with commands.** The options named (2026-11-09): impulse joints for everything (the first, above); a multibody with impulse joints for
  the loops; a multibody with an MLCP solver for the loops. This is why the physics layer is given a **coupling = two cars and a list of pivot pairs**, never one joint per block: the choice of solver is
  inside that one box, and nothing outside it (the blocks, the coupling logic, the commands) may know which kind it is.
- The **integer consistency rule** applies when a second joint couples the same two cars: the squared distance between the pivots must be equal in both cars' grids, in half-voxel integers
  (and, from four joints, the handedness). Without it the cars are permanently strained by the mismatch (measured: 1.44 times it). It does not apply to the closing link of a loop of
  three or more cars; that depends on the pose.
- Future, not scheduled: several couplings per pair reduced to one equivalent constraint (two on a line are a hinge, three not on a line are rigid), then a direct solver for loops
  that sits beside Rapier. Not a fork of Rapier.

## Bounds and lifecycle
- For choosing which bricks need colliders, a ship's bounds are the sum of its cars' longest dimensions used as a diameter, per car (a to-do is to improve this, `future-goals.md`).
- A coupled group **freezes and is parked together**, by the nearest of its cars, so that a joint never anchors an active car to a frozen one. (Under question 2026-11-09: whether a mechanism may stop
  because no player is near; see "Promises".)
- **A coupled group is at most 8 km across (owner, 2026-11-09, "for now")**, so that it fits in one physics frame (a frame is 16 km from its origin to its edge).
- Later, a joint between a planet and a car: lock and unlock to a planet reuse `dock_into_planet` and `undock_from_planet`; the open problems are in `future-goals.md`.

## Under review (2026-11-06; **still open except the choice of solver, decided 2026-11-09 above**): what measuring the plan above found
`loop-probe.md` ran the plan in Rapier 0.36.1 (f64) and `BodyWorld`. The owner wants to keep both ways of closing a loop open and to **choose per ship**, with the solver's dials (Rapier has them per body:
`additional_solver_iterations`, `additional_pgs_iterations`) as ship data. Numbers are in that document; in short:
- Rapier's multibody costs about the cube of the cars: 24 cars 8.4 ms of a 50 ms tick, 56 cars 71 ms; impulse joints grow about linearly (86 and 175 us).
- A multibody does not conserve linear momentum, with or without a loop (first order in the step: a spinning rig of 16 cars strays 2.7 m in 40 s at a 1 ms step); impulse joints conserve it to rounding. The loop method
  (Rapier's own impulse closing joint between two links, which it supports; or a direct 3 x 3 solve beside Rapier) changes the loop's gap (9e-6 m or 1e-14) and little else. So the choice that matters is multibody against
  impulse joints, not the loop method; a multibody for small articulated assemblies and impulse joints with the dials for everything else is what the numbers favour (opinion).
- Overlapping colliders joined by a joint make energy (1.2 MJ from a 0.5 m overlap) and, with a multibody joint, momentum, unless the joint turns contacts off between its bodies (then exactly nothing).
  **A rule to add whichever solver is chosen:** a joint disables contacts between its two cars, and so does every pair of cars of one rigid group.
- A multibody puts a child where its joint coordinates say, so its joint frames must be chosen from the poses the cars have when it is built (a ring built without them was straightened and exploded).


## Promises (owner, 2026-11-09; **being refined: they say what must hold, not yet how**)
The owner's words, condensed:
- **(a) A mechanism works the same everywhere.** Joint physics work the same at all speeds, in an atmosphere or out of it. A player never meets a mechanism that works in one situation and fails in
  another for a reason that is not obvious, or for a bug. It is fine for a custom landing gear or a Star Trek turbolift to stop working because air rushes through the interior (a hole), because of extreme-G
  manoeuvres, or because the ship landed on a planet of high gravity. It is **never** fine for a mechanism to stop working or work differently because of what the coordinates look like, which ships are
  near, how many players are on the server, or how fast the ship goes in a vacuum. The places where the game's intended behaviour breaks down are few, simple to memorize and not arcane.
- **(b) Ships with joints go on rails.** The ship is kept at the centre of a physics frame and the frame is moved along the rail. The motion of joints, motorized or not, cannot affect the rail. Engines
  affect the rail. If two ships cross paths while one is on a rail they may not both be at the centre of a frame, but whatever error that brings must not affect the rail. The only thing that takes a ship
  off its rail is a collision with another ship or with a player on the outside of the ship. A player on the inside (in the pressurized area) changes the ship's course by nothing, bumping into it or using
  suit thrusters.
- **The goal:** players can make and keep extremely precise trajectories to extremely distant places and still have fun inside the ship and use motorized parts.

**What the code does today that these would forbid (read in the source, 2026-11-09, not run):**
1. `BodyWorld::steps_for_this_tick` cuts a tick into 3 to 24 steps by the speed of the **fastest dynamic body in the frame**, so a mechanism's solver steps depend on its own speed and on every other ship in its frame.
2. A frame is one `BodyWorld` for all the ships of a place; its origin is the brick of the first ship that made it, and a ship moves to another frame (its joints would die) at 32 km from the origin.
3. A rail is per grid, holds a place and not a turn (`coasting_candidate`: a ship that turns is not put on one), and any force or torque takes the ship off it; a joint motor turns cars.
4. Bodies can sleep: a body alone in space slower than about 0.09 m/s slept after 0.5 s and stopped dead in the probe (`loop-probe.md`, not measured again through `BodyWorld`), which would stop a slow mechanism.
5. A ship far from every player is frozen (`ServerWorld::set_grid_frozen`, `ShipSettings::simulation_distance`, 256 voxels), so a mechanism stops when no player is near.
6. Positions in a frame are `f64` from the frame's origin, so a mechanism's numbers differ in the last bits with where in the frame it is (the rounding floor of an `f64` is about 5e-13 m at 4 km, as worked out in `loop-probe.md`; a joint's actual gap is millions of times that).
