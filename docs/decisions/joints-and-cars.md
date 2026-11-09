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
- **Built 2026-11-09 (step 2):** the block is a data file, `games/voxunami_cave/data/blocks/dev_ball_joint.json`, the last block of Cave (number 13, so no number moved). As made it is the box
  `[2,0,2]-[6,6,6]` in eighths of a voxel (a voxel is a metre: 0.5 by 0.75 by 0.5), gunmetal gray, with its ball end up and that face a flat blue; `engine:axis_face` is up, so it is placed like a
  log, with the ball end away from the face that was clicked; `engine:joint_face` is up too. It has no pictures yet. The finder is `Docking::joint_ends_of` (the same scan of a brick that finds
  ports and mass, `BrickScan`); it never returns a port, and `ports_of` never returns a joint end. The shared geometry is `BlockFace`, `BlockFaceView` and `approach` (`Port` and `PortView` were
  renamed; the docking code and its tests are as they were). The code does not keep the ends of joints yet: that is the coupling logic (step 3).
- Two such blocks facing across a cell boundary, 0.5 m apart at the tips, are **coupled**; a ball of 0.5 m diameter is drawn in the gap, and only then. The ball is two hemispheres, one in
  each cell: bent, the join shows; that is accepted for the prototype.
- States of a pair: apart, coupled (mobile), coupled and immobilized, locked. A joint is **derived from block states and positions and never saved**.

## Coupling, lock, unlock
- Couple when the pivots are within 0.3 m and the faces within 45 degrees; until then pull with 10 kN each (as for docking, independent of mass) until the pivots are within 0.05 m, then the joint.
  Coupling applies positional forces only, no torque toward 180 degrees. **Agreed 2026-11-09:** as for docking, the two cars must also be close in velocity when the joint is made (a limit on their relative
  speed, **1 m/s of linear speed and 0.2 rad/s of angular speed between the two cars at the pivots, agreed 2026-11-09**), because a joint made with a relative velocity begins with a collision that loses energy and leaves a gap (found building the first tests, `build-log.md`).
- **Lock** is the only thing that turns the cars toward alignment (180 degrees, or the nearest turn that lines the grids up cube for cube). Within the docking snap (0.3 m, 10 degrees) it snaps
  and merges the grids. A lock request lasts until it succeeds or the joint is mobilized; on an immobilized joint it is refused with "mobilize first".
- **Unlock** splits the grids and lands in *coupled (mobile)*. Lock and unlock act on the **pair of cars**: unlock cuts every joint between them at once (the flood fill in `undock` returns
  `Welded` if the halves touch anywhere else).
- Cars collide with each other, and the two cars of a coupling too: **contacts between them stay on (owner, 2026-11-09).** Coupling across physics frames is refused for now.
- **The dev ball joint lets one car pitch, yaw or roll against the other (owner, 2026-11-09):** the ball turns freely about all three axes (what a spherical joint is). No angle limit has been chosen; the
  hulls stop a bend where they meet (the cars are 2 m apart at the pivot's ends at rest).
- **A joint never breaks, for starters (owner, 2026-11-09).**
- **Built 2026-11-09 (step 3, `engine/grid_bodies/src/joints.rs` and `plugin/couplings.rs`), and what the tests found, which is where it differs from the words above:**
  - *The pull is a controller, mine, and not a constant force.* A constant 10 kN toward the other end arrives at its greatest speed exactly where the coupling is made, so no pair could meet the speed limit (worked out, not
    measured: a pair of 500 kg cars from 6 m arrives at 15 m/s). The pull is **at most 10 kN on each of the two cars** (the owner's number, equal and opposite, so the lighter car moves more), and it asks the pair for the greatest speed it can be
    stopped from in the distance left (a deceleration of 2 m/s/s, or half of what 10 kN does to the pair if that is less), taking up the difference over 0.1 s. A longer response (0.5 s) let light ends overshoot each other and
    jam with their tips in contact; the lag of the speed behind the one asked is the deceleration times the response, 0.2 m/s. From 3 to 6 metres, at rest, cars of 1.2 to 30 tonnes were coupled after 1.5 to 4.6 seconds.
  - *The pull is at the end, as docking's is at the port (owner, 2026-11-09: "don't differ from docking").* I had put it on the centre of mass first, because a pull at an end away from the centre swings the car, and two ends side by side
    swung the two cars in opposite ways, 0.6 rad/s each (1.2 rad/s of one against the other, measured), which nothing took away and which is six times the 0.2 rad/s a coupling allows, so such a pair never coupled. With the pull at the
    end, **a torque slows the cars' spin against each other** (equal and opposite, at most 10 kN m, docking's twist torque; the inertia is a third of the trace of each car's tensor, an approximation): the two-ends pair then couples, and
    without the torque it does not (checked by taking it out). It only slows their turning against each other: nothing turns the ends toward each other, so the 45 degrees is for the players to aim. **This is a torque, which the first words of this
    section said coupling would not apply; it is not toward alignment.**
  - *What is pulled is a group.* Cars joined by couplings are one mass under a pull, and **a group is pulled by one other group at a time, the nearest**. A chain of three cars (the middle one pulled from both sides) overshot by
    half a metre and jammed with the tips of the bars in contact; with this rule it couples. Two cars of one group are not pulled or coupled to each other (a loop is for later).
  - *When the joint is made:* in the first tick the ends are within 0.3 m, the faces within 45 degrees of exactly opposite, the pivots going past each other at 1 m/s or less and the cars turning against each other at 0.2 rad/s or less.
    That is how I read "until the pivots are within 0.05 m, then the joint": the pull does not go on to 0.05 m, the solver closes what is left. Measured: the pivots were 0.12 to 0.28 m apart and closing at 0.5 to 1.0 m/s, and the gap
    was closed in the tick (by the solver's position correction, which adds no speed): the two cars stop against each other at the pivot, an inelastic joining that loses at most half the reduced mass times the speed squared
    (worked out, not measured: 750 J for two 3.2 tonne cars at 0.97 m/s; both cars' speeds were 0.000 in the tick after, and the gap 0), and nothing is gained. **The owner agreed (2026-11-09) to keep the snap of up to 0.3 m.**
  - *The pivot is the middle of the end's face* (a quarter of a metre beyond the bar's tip), so two coupled ends have their tips half a metre apart, as this file says the ball is drawn.
  - *Coupled cars keep 4 extra substeps and may not sleep* (set when coupled, undone when the car is in no coupling): a coupled pair given 5 cm/s kept going for ten seconds; without the switch it went to sleep after half a second
    and stopped dead, 2 cm in ten seconds.
  - *One ball for two cars, in one frame:* the closest pair of ends of two cars makes the coupling and the other ends of the same two cars are left (the integer rule is for later); a coupling is let go when either car or end is gone, and
    otherwise not (a joint never breaks).
  - *A group is always in one frame (step 4a, built the same day).* A car leaves its frame at 32 km from the frame's origin and its joint died with its body; one car of a pair crosses first (by my arithmetic, not measured: about one crossing in a hundred for a pair four metres
    long at 6 km/s, a tick being 300 m of flight; more for a long train). Now the cars coupled to one that leaves leave with it and are put in a frame together (a car goes to its coupled partner's frame if it is near enough), and the joint is made again in the
    new frame as it was (`couplings_moved`, not `couplings_made`). Tested with the edge of a frame run between two coupled cars, and with one car just inside another frame's radius and its partner just outside (each test fails when its
    half of the fix is taken out). **The 8 km size of a group is what lets a whole group be near one frame's origin, and nothing enforces it yet**; a train longer than a frame would lose its joints again.
  - *A group is kept as one by the lifecycle (step 4b, the owner chose the server holding the groups).* The server world holds the pairs of ships that are coupled (`ServerWorld::coupled_ships`; the bodies plugin writes it each tick after it couples and lets go); the lifecycle
    (`engine/grid_lifecycle`, `groups.rs`) takes the squared distance of the nearest ship of a group, in memory or in a file, as the distance of every ship in it. So a group is frozen when every ship of it is beyond the stop distance, let go when its nearest ship is within the
    simulation distance, put in files when every ship is frozen and the nearest is beyond the unload distance, and brought in when the nearest is within the simulation distance (a ship that waits for its pilot stays in its file, but its place counts). A group with a ship awake and a
    ship frozen (a ship brought in alone from its file) is awake: the frozen ships are let go as long as the nearest is within the stop distance. **The pair is remembered while one of its ships is in a file** (the joint went with the body); when both are in memory the joint is made
    again by the coupling rule at once, the ends being together and a frozen ship having no speed (so the 0.2 rad/s spin limit is not in the way; the centimetre waiver was made for this). The pair is forgotten when a ship is gone or an end is. Not saved: a world loaded from a file has
    the pairs forgotten, and its ships come back by their own distances until the bodies couple them again (step 4d).
  - *Not done, and a problem seen:* **A coupling is derived and never saved**, as decided; but the rule asks for relative spin of
    0.2 rad/s or less, so a swinging group that is saved and loaded would not be coupled again until it turned slowly. The joint only cares about the speed of the pivots (a ball turns however it is), so **the spin limit is waived for ends that are
    already within a centimetre (owner, 2026-11-09: yes, a centimetre; `spin_waived_within`)**; the speed limit is not.

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
- A coupled group **freezes and is parked together**, by the nearest of its cars, so that a joint never anchors an active car to a frozen one. **A mechanism moves only while a player is within 1,024 voxels
  of the group (owner, 2026-11-09: "let's start with 1 km", then "1024 voxels for both of them for now")**; a group with no player that near is frozen, and parked when none is near enough to unload it, and its rail goes on all the same
  (see "Promises"). **The 1,024 is the simulation distance of every ship, jointed or not** (`ShipSettings::DEFAULT_SIMULATION_DISTANCE`, was 256, changed 2026-11-09); the unload distance was 1,024 already, so a ship is put away as soon as it stops moving.
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


## Promises (owner, 2026-11-09; the rules below are the owner's answers to the first round of questions, the way to build them is still to be designed)
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

**The owner's answers (2026-11-09, second round):**
1. **"The same" is bit-identical**, wherever the ship is and however fast it goes. (Means, **[mine]**: the group's frame is centred on the group's centre of mass, so the numbers a mechanism works
   with are the same whatever the place; tests that the same ship gives the same bits at the origin, a long way off and at speed, alone and among a thousand ships.)
2. A ship's frame is **in free fall**: no tidal pull on a ship inside its frame (accepted).
3. **Every turning ship, jointed or not, rides a rail**, and its turning is worked out in the frame. (This reverses what `coasting.rs` does today: a rail holds a ship's place and not its turn.)
4. **A ship is on a rail whenever it is not in an atmosphere.** For a ship landed on a body that has no atmosphere the owner left the choice to me: "whatever keeps complexity low and is most likely to be
   bug-free". **[mine, not built]** It is off its rail while anything is near it, as today (`NEAR_MARGIN`), an ordinary body of the contact pass; it goes on a rail again when it has lifted off and been quiet for
   `QUIET_SECONDS`. No new landing rail. (No atmosphere exists in the game yet; today a ship is off its rail near anything, in a level with a pull of its own, and when it turns.)
5. **A collision with the outside of a ship gives its rail a kick and also changes its spin**, and the rest of the blow goes into the cars' motion against each other: the owner expects this to need
   careful design. **[mine, to design before the lifecycle phase]** The kick on the rail must be the exact impulse, and so must the change of angular momentum; how the impulse is shared between the
   ship's motion, its spin and its joints is for the joint solver to say (an impulse put on the struck car in the frame, at the point of contact), but the *size* of the impulse comes from a contact pass
   that sees one body (`loop-probe.md`, section 14, which found the whole ship as one contact body works and a car by car one does not), and a ship hit through a loose joint has a smaller effective mass
   at the point of impact than the whole ship. Which of the two sizes to use is the open part.
6. **Two railed ships that pass close both stay on their rails**; their collision is worked out in a temporary shared frame in which the rails are the authority, and only a real hit changes either.
7. **Inside** is the ship's *mostly enclosed* space, and any space that has life support air. Players, cargo and loose items there never move the rail by moving or by colliding with the ship. A player
   on EVA **does** move it (nudging an out-of-fuel ship with suit thrusters is wanted). A **deflector shield block**, a **tractor beam block** or some similar advanced block can give a ship the
   property that EVA and collisions with other ships do not change its rail: a forcefield round the ship inside which they have no effect on the rail and which nothing outside can enter (a player or
   ship is deflected), paid for with a stored resource (energy, mana) of the ship that projects it, and with **no reaction force on that ship**. Both blocks are in `future-goals.md`. **"Mostly enclosed" (owner, 2026-11-09, for now): more than 95 percent enclosed, measured by casting rays from the place to find the sky.** Later, perhaps: a flood of
   starlight through the cells, measured by how bright it is where the player stands.
8. **A mechanism moves only while a player is within 1 km** (see "Bounds and lifecycle").
9. **Time warp:** for starters mechanisms pause while the rail goes on. **Later** (`future-goals.md`): what is aboard goes on at normal speed while the outside universe and the ship's place on its
   rail go on at a high speed, so that an hour aboard is an hour of ordinary play (crops grow, fire spreads) in a year of travel.
10. **No sleeping state between active and parked.** A group nobody is near is parked and its rail goes on, even with nobody within light years. **Read in the source, 2026-11-09, not run now:** a
    rail is a function of the world's clock (`coasting.rs`, header), so "observed or not, a ship on a rail is where the clock says, and one saved on a rail and brought back is where the clock says
    too". A parked ship that is *not* on a rail is where it was left, not moving; that is what today's rule that a turning ship is not put on a rail would have meant for a group with a motor, and what
    answer 3 removes.
11. **A thrusting group flies free until burns on rails exist (owner, 2026-11-09: "yes").** Engines on a rail are step C of `orbits-and-thrust-plan.md`, not built: today any thrust takes a ship off
    its rail. Until step C, a jointed group that is thrusting is an ordinary body of the contact pass, its frame following its own body, with no promise about the precision of its path; only a
    coasting group rides a rail. Its mechanisms behave the same either way (promise (a)): the frame is centred on the group in both. "Engines affect the rail" (promise (b)) waits for step C.

**What the code does today that these would forbid (read in the source, 2026-11-09, not run):**
1. `BodyWorld::steps_for_this_tick` cuts a tick into 3 to 24 steps by the speed of the **fastest dynamic body in the frame**, so a mechanism's solver steps depend on its own speed and on every other ship in its frame.
2. A frame is one `BodyWorld` for all the ships of a place; its origin is the brick of the first ship that made it, and a ship moves to another frame (its joints would die) at 32 km from the origin.
3. A rail is per grid, holds a place and not a turn (`coasting_candidate`: a ship that turns is not put on one), and any force or torque takes the ship off it; a joint motor turns cars.
4. Bodies can sleep: a body alone in space slower than about 0.09 m/s slept after 0.5 s and stopped dead in the probe (`loop-probe.md`, not measured again through `BodyWorld`), which would stop a slow mechanism.
5. A ship far from every player is frozen (`ServerWorld::set_grid_frozen`, `ShipSettings::simulation_distance`, 256 voxels then, 1,024 since 2026-11-09), so a mechanism stops when no player is near.
6. Positions in a frame are `f64` from the frame's origin, so a mechanism's numbers differ in the last bits with where in the frame it is (the rounding floor of an `f64` is about 5e-13 m at 4 km, as worked out in `loop-probe.md`; a joint's actual gap is millions of times that).
