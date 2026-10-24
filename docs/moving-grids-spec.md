# Phase 32: moving grids (ships)

Status: the owner answered section 6 on 2026-10-20 (answers at its end, and applied in the text); 32a to 32h are built (`build-log.md`), and the pause for the owner after 32h is now. Read from the plan (requirements 1 to 16, the owner's
decisions of 2026-10-18 and 2026-10-20), `future-goals.md` (goal 2 and "Ship goals"), and from the code after Phase 31. The choices are
mine and marked; what I checked in the code is marked **[code]**, what I believe from memory is marked **[memory]**.

## 1. What is true today

- **[code]** `GridId` exists and is the address of every rule's voxels (`ActiveVoxels`, `GridView`, `BlocksChanged`, pending-work sections).
  Only root grids exist: `ServerWorld::grid` answers `NoSuchGrid` for any id that is not a level's root (`engine/server/src/grid_view.rs`).
  Ids from `1 << 32` up are reserved for allocated grids; the counter is not saved because nothing allocates.
- **[code]** `EntityPose` is already a frame and a local pose (`grid`, `position`, `orientation`), and the entity store is keyed by grid.
  Orientation is a full quaternion, so a turned frame needs no new type. `WorldPosition` is an exact integer brick plus an `f32` offset.
- **[code]** The renderer places each brick by a camera-relative translation computed exactly from integers, and draws it axis-aligned.
  There is no rotation and no per-grid matrix (`placements` in `voxel_renderer.rs`). Picking (`world_view::ray`) is a voxel walk in world
  space. A client's break or place message names a voxel of the level, not of a grid.
- **[code]** Physics is `rapier3d` 0.36.1 with `enhanced-determinism`, used for the character only (a kinematic controller against
  colliders built from the voxels of nearby bricks through `CollisionSource`). There are no dynamic bodies in the engine; the spike
  (`physics-spike.md`) showed bodies, joints and buoyancy identical on Windows and Linux.
- **[code]** Gravity is one constant, `CharacterSettings::gravity` (32.0, down). Nothing computes a direction from mass, although
  `design-decisions.md` says that is the plan.
- **[code]** Control targets, block data and the `Brain` slot are **not** built (`plan.md`: "stay planned for the phases that first need
  them"). The prompt (27h) and modal forms (30) are built, and are what the missing-ship choice uses.
- **[code]** The frame-invariance test of plan requirement 6 does not exist yet (I found no such test); Phase 32 builds the harness.
- **[code]** There is no terrain level of detail yet. `plan.md` (Open, "Rendering at scale") says one is needed; the renderer draws every
  visible brick at full detail. There is also no simulation-distance setting: `ServerConfig` has `maximum_watched_bricks`, the host has
  a view radius, and rules run on their active sets. The "frozen beyond simulation distance" principle is not implemented.
- **[code]** There are no fluids in any game yet.

Consequence for the owner's new wishes (`future-goals.md`, "Ship goals"): level-of-detail drawing "just like terrain" has nothing to be like
yet; the gravity of huge ships needs a gravity field that does not exist; fluids and current crossing a ship's edge need a cross-grid door
that rules must not otherwise have; and simulation distance needs defining for ships (and, I recommend, for terrain, later). Section 2.9 and
2.10 say how each is planned for here, and which are built now.

## 2. Design (my proposal)

### 2.1 A grid, and what makes a grid a ship

A grid is the thing `GridId` names: sparse bricks of channels in its own integer coordinates. A *ship* is an allocated grid that has a
*body*: a pose (`WorldPosition` of the grid's origin voxel, `Orientation`), a linear and an angular velocity, and the physics derived from
its blocks (mass, centre of mass, inertia). The pose is in the *level's* root frame; a ship in a ship (a grid docked but not merged, a
sub-grid on a hinge) is the later "sub-grid with joints" goal, so a grid's body pose is relative to its level, and the type leaves room for a
parent later without using it.

State lives in a `GridStore` beside the levels in `WorldData` (the data the world saves), with: the counter that allocates ids, the grids,
and *tombstones* (a merged-away id naming its successor and the transform, requirement 13). The rule API does not change: `ServerWorld::grid`
answers for allocated grids too, and a rule sees exactly what it saw before: one grid, local coordinates, no world.

A grid has a header: `on_pilot_logout` (`stay` or `vanish`, section 2.8), `held` (a ship that does not move, section 2.7; off by default), and
the origin id of requirement 10 (so a ship file loaded twice is two grids).

### 2.2 Frames: one small pure module

`voxel_space::frames` (no I/O, no physics): the pose of a grid, and these functions, all deterministic and pinned by tests and properties
(round trip, composition, exactness for turns by multiples of 90 degrees and whole-voxel offsets):

- `local_to_world(pose, local position) -> WorldPosition`, `world_to_local`, the same for directions (gravity, velocity);
- `voxel_in_other(from pose, voxel, to pose) -> Option<VoxelCoordinate>`: the voxel of the other grid containing the centre of this voxel,
  rounding a centre on a boundary toward negative infinity (the floor), `None` outside the coordinate range. **This is the one cross-grid conversion** the owner's
  wishes for fluids and current need (section 2.10); building it now costs a function and its tests, and merges use it too;
- `lattice_alignment(from, to) -> Option<LatticeTransform>`: the turn (one of the 24 axis rotations) and whole-voxel offset that carries one
  grid onto the other's lattice, or `None`; docking's snap is "pull `lattice_alignment` toward `Some`" (section 2.7).

Positions are computed in `f64` from a brick and an offset. Exactness is for the lattice (`LatticeRotation`, `LatticeTransform`: integers, 24 turns); a pose's quaternion is `f32`, so a quarter turn is exact only to about `1e-7`, which is why `lattice_alignment` takes a tolerance and docking will snap a ship to the exact lattice turn. Arithmetic is `+`, `-`, `*`, `/` and `sqrt` (correctly rounded), never fused or transcendental; building a quaternion from angles goes through
`libm`. A quaternion is re-normalised each step by a fixed formula.

### 2.3 Physics

A `BodyWorld` in the `physics` crate (one per level), stepped in the engine's physics phase, single-threaded, `enhanced-determinism` as in
the spike:

- A ship body is a dynamic Rapier body. **Built (32e): its whole cubes are the voxels of one Rapier `Voxels` collider** (as `physics-spike.md` recommended: a sparse
  chunked shape with its own BVH that handles the seams between voxels), edited in place a brick at a time (only the voxels that differ), and the blocks that fill part
  of their cell are boxes, one compound per brick that has any. (The first draft of this section had a compound of merged boxes per brick, as terrain has; the spike's
  collider is simpler and cheaper to edit.) **Mass** is the sum of `engine:density` of its blocks, kept as moments per brick (`MassMoments`, summed in brick order, in
  `f64`), from which come the centre of mass and the tensor of inertia that Rapier is given as they are.
- Terrain is collidable by ships through the terrain colliders the character already uses, built for the bricks near a ship's bounds only.
- Two ships collide with each other. Ships and characters collide: a character in the world frame is a kinematic capsule against a ship's
  colliders.
- Forces on a ship come from: the gravity field (section 2.9), plugin forces (`apply_force(grid, local point, vector)` in the pre-physics
  phase, requirement 2), and contacts. Buoyancy (the spike's model) is not in this phase: Cave has no fluids.
- *Held* ships (section 2.7) are kinematic: they do not move, and everything collides with them.
- A ship's pose is read back at the end of the physics phase and is the stream's truth for that tick.

Size and cost, **measured in 32e** (`cargo run --release -p physics --example ship_cost`, this machine, one thread, a hull held in a hash set, so the scan of a brick is
dearer than it will be from a brick): a ship of 10,164 blocks (one brick) is made in 1.5 ms and a tick of three steps awake costs 0.011 ms (0.24 ms at worst); one block
edited every tick makes its brick again in 0.49 ms (0.58 ms at worst). A ship of 101,614 blocks (eight bricks): 15.5 ms to make, 0.024 ms a tick awake (0.45 at worst), 0.69 ms (0.91 at worst) to remake the brick
of an edit. A hundred ships of 2,304 blocks dropped on the ground: 0.13 ms a tick on average, 4.6 ms at worst while they land, 0.008 ms once they sleep. So the cost
is the scan of a brick when an edit lands (about half a millisecond), not the stepping; a ship edited in many bricks in a tick (an explosion) is that times the bricks, and
the plan is a budget of bricks made again per tick (the rest are made over the next ticks), which is not built because nothing measured needs it yet.

### 2.4 The stream (protocol version 6)

- **Grids are replicated like entities:** `GridsSpawned { id, level, header, pose }`, `GridsChanged { id, pose, velocity }` (every tick in
  which the pose changed; the pose is the exact `WorldPosition` and the quaternion), `GridsDespawned`, and the tombstones for merges.
- **Interest:** a client is sent a grid if its bounds (the bounding box of its bricks, turned by its pose) meet the box of bricks the client
  watches. It is sent the grid's bricks as `BrickContents` for that grid, subject to a per-client budget. A grid beyond the budget is sent
  nothing and the client draws nothing of it (the level-of-detail answer, section 2.10, is what fills that gap later).
- **Edits and bricks gain a `GridId`** (today they name a level): `BrickContents`, `VoxelEdits`, the client's break and place and use
  messages. A root grid's id is its level, so the root case is unchanged in meaning.
- A replay needs nothing else: poses are in the stream, so a recording shows the ship exactly as it moved (the hard rule that the client's
  view comes only from the stream).

### 2.5 The renderer and picking

- Each brick's placement gains the grid's 3-by-3 rotation (the translation is still computed camera-relative and exactly: the brick's
  position in the grid, turned, plus the grid's `WorldPosition`, differenced against the camera's). Normals are rotated by the same matrix
  before lighting; the sun and the sky stay in world space. Culling uses a bounding sphere, so a turned brick needs no new test.
  Placements are 64 bytes today; a rotation adds 36 (or 48 aligned). Bricks of one grid share the rotation, so it could be a per-grid
  uniform instead; I would start with the placement because it keeps one draw per brick and measure.
- The mesh key becomes `(grid, brick)` (and later `(grid, brick, level)`, section 2.10).
- Picking: the camera ray is transformed into each nearby grid's frame (an inverse of the pose, in the same arithmetic as 2.2), walked
  there with the existing voxel walk, and the nearest hit wins. The hit is `{ grid, voxel, face }`; `ClientContext::target()` returns it, and
  the client's actions send it. Terrain and ships look the same to a client plugin.
- Entities aboard are drawn under the grid's matrix (their pose is local to it).

### 2.6 Aboard: the hard part (built in 32g)

A character's pose is `(frame, local)`. A character *aboard* is simulated in its ship's frame, as decided on 2026-10-18: its position is
unchanged when the ship moves (exact carry), its collision world is the ship's blocks through a `CollisionSource` in local voxels (the same
trait the terrain uses, so the controller does not change), and gravity is the field's value in the ship's axes (2.9), a vector that changes
as the ship turns. As built:

**Entities in a frame.** `EntityPose.grid` may be a ship. The store keeps any grid; `ServerWorld::spawn_entity` and `set_entity_pose` refuse
a ship that is not there (`EntityError::NoSuchGrid`, and nothing changes). `ServerWorld::remove_grid` puts everything aboard (players away
included) in the ship's level where the ship had it, turned as it was, in the same call, so nothing is left in a frame that is gone. A client
is told of every entity aboard each ship it is told of, after the ship and its bricks, wherever in the ship they are (a ship is in the picture
whole); a ship that leaves the picture takes them with it. A world is saved with those aboard in the frame of their ship, with the place they
have in the world beside it (32i); a file that lists their ship has them aboard it, and one that does not has them at that place. The client's
`ClientWorld::entity_in_the_world` says where an entity is in the world.

**The record is the truth about where a player is.** `Presence::pose_of`, `players` and `level_of` answer in the world (a passenger is where
the ship puts them, in the ship's level); `frame_of`, `local_pose_of` and `set_pose_in` are for what moves players. Each tick
`voxunami_cave:move_the_bodies` (after `engine:grid_bodies_move`, so the players meet the ships where they are this tick) reads each player's
record, makes the body agree with it if it does not (the player was sent to another level, a ship was taken from under them, they came back
aboard one), steps the body, and writes the new place, in the frame the body is in, back. `/tp` writes the record at once.

**Boarding and leaving (aboard.rs).** A walker in a level that ends a tick on the ground, not flying, with a block of a ship within a tenth of a
voxel under their feet (along the world's down, in the ship's coordinates, looking at the eighths a block's shape comes in) is put aboard it:
the same place, and the speed they have *relative to the ship* at that place (the ship's recorded speed at a point is its centre of mass's plus its turning
crossed with the arm: `MovingGrid::velocity_at`), in the ship's axes. A passenger has left when they are in the air and nothing of the ship is under
them along gravity anywhere in the box of the ship's bricks (a jump from a deck is followed from where the line enters the box), or the middle of
their body is more than a voxel outside that box; they are put in the level at the same place with the speed they had in the ship, turned, plus the
ship's at that place. A player who walks off the edge of a deck leaves the moment they are off it, not three ticks later; one who jumps from the
deck stays aboard. **These two rules differ from the 3-tick hysteresis above and I changed them (and it is the owner's to overrule):** the
condition for boarding needs a block under the feet and the one for leaving needs none, so no one is passed back and forth at an edge, and a
tick is all either needs; and a body left standing in the level on a moving ship is carried by nothing. (A body aboard cannot see the terrain, so
three ticks beyond the edge of a deck that stands on the ground would have it walking through the ground.) A flying player is not aboard; one
who flies out of the ship's box leaves it.

**Ships are colliders for a body in a level.** The blocks of the ships within two bricks of a player are colliders in the player's own Rapier
world (`CharacterBody::update_foreign`), put each tick where the ship is and turned as it is, made again where the ship is edited
(`foreign_brick_changed`). Standing on a moving ship's deck for the one tick before boarding, the body is not carried; what carries it is the boarding.
The controller loses the whole of a slide along a wall where the wall meets the floor (found in 32g by a test that walks along a turned ship's
wall; the cause was found later, in 32h): the shape cast gives a wall a normal with a hair of slope (about 1e-5), so the slide along it has a hair
of downward speed, the controller then meets the floor at once, takes the walker to be sliding down a slope it must not slide down, and a flat floor has
no direction along it to keep, so nothing is moved. A slide that comes out as nothing is made again, level and along the wall, leaning a little away
from it (the wall is within the controller's gap, and a slide that touches it again meets the same hair): for walls of ships and of terrain alike.
(Regression tests: six turns of a ship's wall, both ways, steep and shallow; and a sweep of 144 walks at a terrain wall, of which 31 stood
still before, and one of which stood still for a hundred steps in the pinned Cave session.)

**Keeping clear (found by measuring; I had written that a ship that moves into a player "overlaps them and nothing yet pushes them out", which
was a belief from reading the code and was wrong in both directions).** For a ship that moved slowly (4 voxels a second) the controller did
push a walker ahead of the wall; but a ship that fell on a walker pressed them into the floor and they fell down through the terrain and the
world (the eyes went from 202.6 to 94.6), because the controller slides a body out of an overlap along one collider's surface with no regard to
what is behind it, and the colliders of terrain leave out boxes buried in solid terrain; a ship at 60 voxels a second left a walker half in the
floor; and a block built over a walker's head made the controller shake them 0.45 into the floor and out again at every step. So each tick, after the terrain and the ships'
colliders are in place and before the steps, `CharacterBody::keep_clear` moves a character that is inside terrain or a ship's block out the
shortest way (up before the others at one distance, by eighths of a voxel, at most 8 voxels; if there is no room it stays). Measured with the
box 8 thick: a ship at 4, 24 and 60 voxels a second pushes the walker ahead of its face all the way; one that moves past the middle of the
walker in a tick (120 a second) puts them out on the far side, and a wall a voxel thick that goes over them in a tick leaves them where they were:
none of these is inside anything afterwards or in the ground. A ship that falls on a walker lifts them onto itself (the shortest way out), and
they are aboard it; boarding takes away the speed relative to the ship that points away from it, or the walker would be thrown up at the speed
the ship had (they were thrown ten voxels). The same rule moves a body out of a block built where it stands. Cost (median of 1000 server ticks each, Windows): the server's whole tick is 21
microseconds with a ship and no player, 50 to 60 with one walker near the ship, under it or aboard it, 75 with one on hills far from it, and 250
with eight aboard (about 30 for each); adding the check changed none of these by more than the run to run difference (about 5 microseconds).

**The body's frame (physics, `CharacterBody`).** Gravity is a vector: `step_with_gravity`. Up is against it; the box of the character stands
in it (a turn by the shortest arc from `+y`, a half turn about x if opposite); the way the character faces at yaw 0 (`heading`) is carried along
whenever up turns, by the shortest turn between the two ups each time (a half turn about the character's right if they are opposite), so a character in a
room that tips keeps facing as a person would, and a room that turns only about the vertical axis turns the character's view with it. When
up is `+y` and heading `-z` (the plain frame) every operation is the one a character has always done, to the bit: the pinned scripted run and the
golden sessions did not change. `reframe` moves a body to another frame keeping its speed, its flying and whether it stands. A walker in a ship
rolled a quarter turn stands on what is the floor now (the walker slides down the old floor into the corner and stands on the wall), and in
one turned over in a moment falls to the ceiling and stands on it; in both the walker is aboard throughout.

**What a player sees.** The entity's orientation is the turn of the player's view in its frame: the body's frame turn, then the angles the
player gave (`PlayerLook`, still the player's own and never converted at a boundary). For a player who has never been aboard this is
`from_yaw_pitch(yaw, pitch)` to the bit, so those streams did not change. The client finds the turn of the room from the entity
(`PlayerPose::frame_rotation_of`) and gives the camera a `frame_rotation`; the camera looks at its angles in that room, in the world. So the
view is carried with the ship about the vertical axis and stays level against gravity for any pitch or roll of the ship. The picture of a
replay of a walker carried by a turning ship was compared to the pixel with the live one (`pictures.rs`). A player who comes back to the entity
they left is told where they are in the world (`player_presence:placed`): a client watches the bricks around its camera, and a player may have left far from where the
program starts it (found: this was already so for a player who logged out far from the start; it is fixed for returning players here, not yet for `/tp` far away).

**Known limits (none new in kind).** A body aboard does not see the terrain or other ships (the leave rule is what keeps it from walking through a
cliff that touches the hull); players do not push ships or each other, and nothing is hurt by being pushed or pressed (there is no damage
yet); items, light and fire do not ride ships; ships' speeds are not sent to clients; a ship beyond the per-client
budget of bricks is not in the picture, nor are those aboard it; one frame per level for bodies (32e).

- **Momentum on leaving** (as decided: "as a person leaving a train would"): the character keeps the ship's speed, and its air control (14 voxels a second a second,
  `CharacterSettings::air_acceleration`) then pulls its horizontal speed toward what the keys ask: a ship at 6 voxels a second gives about a metre of carry. If the owner wants
  the momentum kept in the air, the air control is what changes.
- **No fictitious forces by default** (a server option to feel them is a goal, `future-goals.md`, "Ship goals" 6; the design leaves room: it would add the
  ship's acceleration, and the centrifugal and Euler terms, to the gravity a character feels): a ship that accelerates or spins does not push a person aboard sideways; only the direction of gravity changes
  relative to them. A person standing in a ship that accelerates hard feels nothing, and a person in a ship that rolls slides toward the
  world's down. This is simple and deterministic; I think it is the right first version, and the thing most likely to feel wrong in flight
  (confidence: medium). It is also what the 2026-10-18 decision ("gravity aboard is the world's") implies.
- Other entities in a frame (items, later a minecart) use the same pose type; only players are simulated aboard in this phase, because no
  other entity moves yet.

### 2.7 Lifecycle: creating, simulating, freezing, parking, splitting, merging

- **Creating:** without a command chair (Phase 35) ships need another way to exist. Proposal: `/ship create` takes the connected solid
  blocks (6-connected, up to a limit) containing the block the player looks at out of the terrain, replicated as edits, into a new grid
  with the same pose (the grid's origin voxel at the terrain voxel, no turn), so it is where it was. `/ship push x y z` gives it an impulse,
  `/ship hold on|off` and `/ship pilot` (the sender flies it with the fly keys; a very small control target, the full mechanism comes with the
  chair) make it testable. These are Cave commands and tests, not engine features, so the engine API (`create_grid`,
  `remove_blocks_from_grid`, `apply_force`, `set_hold`) is what the phase delivers.
  **As built in 32h** (`games/voxunami_cave/src/ships.rs`, `plugins/ships.rs`; the engine side is `detach_blocks`, from 32b, and
  `ServerWorld::generated_voxel`, `Presence::view_in_the_world`):
  - `/ship create` looks along the player's view (24 voxels) at the first block that bodies cannot pass through, and takes **the blocks
    that someone built** (a block that is not what the terrain generator made there) joined to it through faces, up to 4096; `/ship create all`
    takes every solid block joined to it, ground and all, which is what the text above says and is refused as too many wherever the ground
    is joined (nearly always). *This differs from the text above, on purpose:* anything built on the ground is joined to the ground, so
    the text above could not make a ship of a boat built on a beach.
  - The other commands are about **the ship the player is aboard, else the nearest within 48 voxels** (distance to the box of its bricks).
    `/ship hold on|off`; `/ship push X Y Z` changes the ship's speed by that much (voxels a second, in the world's axes) with a force for
    one tick, from its mass, and a held ship refuses it.
  - `/ship pilot` (aboard a ship; again to let go): the pilot's keys do not walk them but fly the ship. Each tick, before the ships move,
    a force is put on it that holds it up against gravity, speeds it up by 24 voxels a second a second toward where the keys point
    (W and S along the way the pilot looks, pitch included; A and D to the sides; space and shift up and down in the world; control doubles
    it), and takes a part of its speed away every second (drag 1.0), so with no keys it hangs where it is and with W it settles at about 24
    voxels a second. One pilot to a ship. A pilot who is no longer aboard it, or who leaves, stops piloting; the ship then falls (unpiloted
    ships stay simulated) unless it is held. *Turning is not a control yet:* a ship turns when a force is put on it off its middle (the
    engine's `apply_grid_force` with a place), as the physics gives; the pilot's view turns with it (32g). Whether the pilot should steer
    with the mouse, and what the real controls are, is for the command chair (Phase 35) and for your feel of this.
- **Unpiloted ships stay simulated** (owner, 2026-10-20): movement physics and gravity continue with no pilot, so an unpiloted ship with no
  lift falls. *Held* (kinematic: it stays where it is) is an option a command sets per ship (`/ship hold on|off`), off by default.
- **Simulation distance (the owner's server setting):** `ShipSettings { simulation_distance, unload_distance }` in `ServerConfig`
  (`--ship-simulation-distance`, `--ship-unload-distance` for `serve`; the integrated server uses the defaults). A ship is *simulated* (its
  body steps, its rules run, its edits are accepted) while any player is within `simulation_distance` metres of its bounds; otherwise it is
  *frozen*: its body is asleep and kinematic, its rules do not run, and its pending rule work is kept. Replication is a separate matter (the
  view radius). A frozen ship that is beyond `unload_distance` of every player is *parked*: saved to its file and dropped from memory. Both
  distances are measured to the ship's nearest brick, with a 10 percent hysteresis so a ship at the boundary does not flap. Defaults: 256 m and
  1024 m (a guess; the right values need the cost measurements of 32f). `unload_distance` is never less than `simulation_distance`; the
  server refuses a setting that is.
  Terrain has no such setting today. I recommend giving terrain rules the same shape later; it is not in this phase.
- **Splitting:** after an edit that removes a block, a ship is checked for connectivity (6-connected non-air blocks). The check floods from
  the removed block's neighbours at once and stops as soon as they have met; only a cut that really separates parts costs the whole
  component, and that cost is spread over ticks with a budget (the ship is whole until the check finishes). Every part gets a new id from the counter, and
  none keeps the original's (owner, 2026-10-20); the original id becomes a tombstone naming all the parts, so a saved reference to it can be
  re-resolved (an entity or a pilot reference picks the part it ends up in). Each part continues with the velocity of its own centre of mass (linear plus angular
  times offset); entities aboard go to the part whose block they stand on, else the nearest by distance, then the event `GridSplit
  { from, parts }` is sent and recorded.
  **As built in 32l** (`grid_bodies::connectivity`, `grid_bodies` plugin, `ServerWorld::split_grid`, Cave's pilots):
  - *The check.* A block is part of a ship if it is not air. When blocks are taken out of a ship (a change that leaves air), the blocks beside those places are the *seeds*, and a
    region is grown from each, a voxel at a time in turn (`Connectivity`). Regions that touch are one. If the seeds end in one region, the ship is whole, found as soon as they meet
    (a dent, a hole, an end block: a few voxels, or none for a block that hung by one face). A region that runs out of voxels is a piece cut off; when one region is left that has not run
    out, everything else is joined to it and is not flooded, so the cost of a cut is the size of the smaller parts. The work is a count of voxels looked at, `SPLIT_VISITS_PER_TICK` = 4096
    for all the ships together, in order of ship; the ship is whole until its check says otherwise (a cut through a ship of 15,000 blocks took four ticks). What is found does not depend on
    how the work is divided (tested for every budget from 1 to 40). A ship edited again while it is being checked is checked again from all the seeds so far: a ship that is cut every few
    ticks is not split until the cutting stops.
  - *The split.* `ServerWorld::split_grid(ship, parts)`, with the voxels and the centre of mass of each part (the plugin works the centre out from the densities): each part is a new ship
    in the same place and turned the same, with only its voxels (every channel of them), held or frozen if the ship was, with the same pilot-logout wish and the sections of plugins; none
    has the ship's id, which leaves a tombstone naming the parts (they are saved with the world file and resolved by `GridStore::resolve`). The parts are the pieces that were cut off,
    in the order they were found, and then all the rest of the blocks as one more. A part goes on at the speed its centre of mass had in the ship: the ship's speed there plus its
    turn crossed with the arm. The rules hear of the parts as of a ship that came out of a file. Blocks' other channels in air voxels (light, say) are not carried: the rules make them again.
  - *Aboard.* Whatever was aboard goes to the part of the block it stands on (the first block along gravity, within four voxels), else to the part with the voxel nearest to it, and
    is where it was in the ship's coordinates, which are the parts' too, so nothing jumps; players' bodies are reframed by the record as for any change of frame. Cave: a pilot of a ship
    that is cut apart is at the controls of the part they are aboard.
  - *Not done, on purpose:* a message `GridSplit` in the stream. Clients see the ship leave and the parts arrive (the stream already says so for any grid), and the lineage is in the tombstones;
    nothing on the client needs more yet, and a new message would be a protocol version. A block placed so that it touches the ship only at an edge or corner is not a piece the check
    looks for (nothing taken out, so no seed); in Cave a block is placed against a face, so it is joined.
- **Merging and docking:** two grids merge only if `lattice_alignment` says they are on one lattice. A docking connector (engine part: a
  pair of ports, an attraction force, a snap distance) pulls two ships together; within the snap distance, the engine moves them to exact
  alignment and merges them: the smaller grid's bricks are re-keyed into the larger by the lattice transform (no re-mesh needed beyond the
  seams), the merged ship gets a new id (owner, 2026-10-20) and both old ids become tombstones naming it, entities aboard (including logged-out
  passengers) are re-framed, `GridMerged { from, into, transform }` is sent.
  Welding (solid blocks flush with the ports that touch when merged) follows the 2026-10-18 decision. Docking is the biggest single piece
  after 32h; section 4 puts it after a pause for review.
  **As built in 32m part 1: the merge** (`engine/server/src/merge.rs`; the owner, 2026-10-22: docked ships become one ship, so that every natural process crosses what was their boundary
  with no work from a mod; a hinge or an axle is a different union, and not planned to carry processes across):
  - `ServerWorld::merge_grids(a, b, &MergePlan) -> Result<GridId, MergeError>`. The ship with more bricks (of two as large, the lower id) is the *base*: it keeps its coordinates, its pose and
    its sections. `lattice_alignment(other, base, plan.tolerance)` gives the transform that carries the other onto the base's lattice, and every block of the other is carried by it (a block is a
    voxel whose `plan.block_channel` is not the channel's default; every non-default channel of it comes along). A block that would land on a block of the base is `MergeError::Overlap`; ships
    that are not on one lattice (a half-voxel off, a tilt that is not a quarter turn) are `NotOnOneLattice`; also `SameShip`, `DifferentLevels`, `OutOfRange`, and `Access` for a ship that is not there.
    Everything is worked out before anything changes, so an error leaves the world as it was.
  - The merged ship is a *new* ship (a new id from the counter; neither old id is kept), and each old id becomes a tombstone naming it, with the transform to it (`GridStore::merge`; `resolve`
    follows it). It is held if either was, frozen if both were, and goes at the velocity in the plan: the server does not know masses, so whoever calls (the docking plugin) works out the
    momentum-conserving velocity. The rules hear of it as of a ship that came out of a file.
  - *Aboard:* whatever was aboard either ship is aboard the new one, at the same block and turned the same: the base's keep their coordinates; the other's are carried by the same transform
    (`LatticeTransform::apply_point`) and their turn composed with the quarter turn.
  - *Turning values:* a block of the other that is turned is, in general, a different block (a stair that faced north faces east). The server cannot know that, so the plan carries `turn`, called with
    the non-default values of a voxel and the `LatticeRotation`, and only when the rotation is not the identity. The game's block table supplies it.
  - *Not here:* the message `GridMerged` (clients see two ships leave and one arrive, as for a split).
  **As built in 32m part 2: docking** (`grid_bodies::docking` and `plugin.rs`; Cave's `voxunami_cave:dev_docking_port`; the owner's decisions of 2026-10-22 are marked *owner*):
  - *Ports.* A block says it is a docking port, and which face docks in each state, with the property `engine:docking_face` (0 none, else 1 plus the face index: west 1, east 2, down 3, up
    4, north 5, south 6; per state). The engine also reserves `engine:marked_face` and `engine:mark_color` (0xRRGGBB): the marked face of a state is drawn in that colour and the other faces in
    the block's own (`BlockTable::material_of_face`; one palette entry for each colour a block marks; the mesher gives each face its own material, so faces merge only with faces of the same
    one). These are properties, so a data pack can set them: a pack can make a docking port without code. A property of a face is given for the block as it is made and turned at read time by the orientation of the voxel (`BlockTable::docking_face(block, state, orientation)`), so a port faces the way its ship does, and a stair, a slab and a port are turned by the same number.
  - *The dev docking port* (*owner*): a creative-only block, `voxunami_cave:dev_docking_port`, gunmetal gray with one yellow face, the one that docks with the yellow face of another. It has
    one state; the yellow face is the east as it is made, and it faces the way the block is turned (its front is placed like a piston's, see below). Creative only is a flag (`voxunami_cave:creative_only`):
    placeable in a creative world, it leaves no item when broken in survival, so playing never gives one and there is none to place.
  - *What pulls.* Each tick, for every two ships of a level that are not frozen and are not both held, the closest pair of ports that pull on each other: within the range (6 m), each looking
    toward the other (a little behind it by the snap distance is allowed), and against each other. A pair that was undocked and has not come apart, and a pair that could not be docked where
    it is, are left out. Each port gets the same force toward the other, **whatever its ship weighs** (*owner*): 10 kN for the dev port's settings (`DockingSettings::default()`; a game sets
    its own), so a light ship is moved more than a heavy one (a checked test: 500 newton seconds in a tick to each of a 4 and a 3 metric ton ship). The force is at the port (*owner*, 2026-10-22: a ship whose centre is far from its port is swung by it): it is put on the centre of mass, and the turn it gives by being at the port is added as a torque worked out from the centre of mass the plugin keeps (`torque_of_force`), because Rapier works out a lever from a centre of mass it updates when it steps, which is not the one just set for a ship made this tick. A twist pulls them into line: the same torque on
    each ship, the second about the axis that brings it into line by the shortest way and the first the other way, 10 kN m at 30 degrees or more out and in proportion below.
  - *Into line* (*owner*): lined up means the grids' voxels agree cube to cube, so of the 24 turns of the lattice the four that put the second's docking face against the first's are the ones
    that count, **any roll about the docking axis in quarter turns docks**, and the nearest of the four is the one aimed at.
  - *The snap* (*owner*): ports within 0.3 m of each other and within 10 degrees of the nearest of the four turns dock: the lighter ship (never a held one) is put exactly where it docks (the
    pose a lattice turn and a whole number of voxels give, checked to be the one `lattice_alignment` finds from the poses), and `merge_grids` makes one ship of the two, going at the velocity
    both together had (`physics::BodyMotion::join`: perfectly inelastic, linear momentum, and angular momentum about the new centre, kept exactly, proved by a property test). If either was held
    the whole is held and still. If blocks of the two would meet, nothing is done and the pose is put back; the pair is not tried again until one of them has moved.
  - *Undocking.* `/ship undock` cuts the docked pair of ports nearest the player, on the ship they are aboard or the nearest: the two ports are not a join, and if nothing else joins the sides the ship
    becomes two (`split_grid`), which go apart at half a metre a second in all (shared out so that their momentum is as it was), and do not pull on each other until they are out of range. If something
    else joins them (a weld: solid blocks beside the ports that are beside each other now) it is refused, and says so, until those are broken. Pilots keep the controls of the whole, or the part they are on.
  - *Orientation, one channel for every block* (*owner*, 2026-10-22; built after the first answer, which gave every block extra states). The owner asked that a block's orientation depend on the player who places it
    (pistons like Minecraft's, logs like Minecraft's), and that orientation be saved the same way for all blocks, whatever number of visibly different orientations they have, so that they can be rotated by simple
    algorithms, and blocks of unloaded plugins too. So: the channel `base_blocks:block_orientation` (a number, default 0) holds the index (0 to 23) of a `LatticeRotation::all()` turn, identity first (the order is pinned by a
    test as a file format). `BlockTable::model_of(block, state, orientation)`: the model is the state's shape turned by the orientation (a number past 23 is read as 0, a world is read as found); the shape pool is de-duplicated
    by the cells a shape fills, so tables do not grow with orientations (a cube 1 shape, a slab 6, a stair 12: its L profile is left as it is by a half turn about its diagonal). The mesher, the picking, the hull and terrain
    colliders, the weigher, the arrival check and the docking read the channel. A merge turns the orientation of each voxel natively (`MergePlan.orientation_channel`, `own.then(transform.rotation)`), for any block; the rest of
    the values still go through the `turn` hook, which docking leaves empty. The standard solid blocks have their seven states again (whole, two slabs and four stairs, as before the closure): stone, dirt, grass and wood have seven, and the other
    ways a slab or stair stands are orientations. The docked-roll test says, for every roll including the eighths, that the slab in the whole has the orientation of the transform and the shape of the second's slab turned by it.
  - *Placing* (`games/voxunami_cave/src/placement.rs`, pure and tested; `SetBlock.against`, a face or none, was added to the message). A block says how it is placed with two properties of the state (`engine:front_face`, `engine:axis_face`;
    `Placement::{Fixed, FrontToPlayer, AlongPlacedFace}` in the table). *Front*: faces the player: up if the block is put within 2 voxels of the player's eyes along both ground axes and more than 2 voxels under the eyes, down if it
    is at or above the eyes, else the opposite of the way the player looks along the ground (Minecraft's piston rule, approximately: the one change is that it uses the eyes of the player and not their feet). *Axis*: runs along the face
    clicked, whichever way the player looks. The server computes it in the grid's own axes (`PlacerView::in_frame`), so aboard a ship the block is turned to the player as seen from the ship. Wood is a log: both ends in the
    wood-end colour (`engine:mark_both_ends`). The dev docking port has a front, so it is placed facing the player; two ports dock when they face each other, as before. *Not built:* slabs and stairs placed by the face or the
    half of the face clicked (they are still chosen by the shape key and placed unturned), and a rotate tool for blocks already placed.
  - *Merge invariance and split invariance* (requirement 14, built 2026-10-22; `tests/ship_invariance.rs`): the light of a scene built as two ships and merged is, voxel for voxel, the light of one ship built whole (and the two do differ
    before the merge, which the test checks), and the light of a ship cut in two is that of two ships built apart. Both are exact because the light has no chance in it. Chance is keyed by grid (`voxel_random(seed, clock, grid, at, purpose)`),
    so a rule with chance, the fire, cannot go on bit for bit across a merge, and the owner has said that is fine (2026-10-22).
  - *Not here, on purpose:* ports of different kinds that refuse each other (every port docks with every port; a second property could say which), docking two held ships (nothing would move them),
    ports of ships that are frozen, and a limit to the work: every pair of ports of two ships in a level is looked at each tick, which is a few dozen for any ship we have.
  - Tests: 14 docking tests in `games/voxunami_cave/tests/docking.rs` (the pull, the momentum, the roll, the held ship, the player and the pilot aboard, undocking, the weld, fire across the former
    boundary and not across ships that only touch), the geometry in `grid_bodies/src/docking.rs`, the join in `physics/src/motion.rs`, the turn of states in `block_models`.

**Writing a rule that works aboard (as built after the fire on a ship, 2026-10-22).** The promise of ships is that a block behaviour, interaction or natural process that a game or a mod
adds works on board any ship with no effort from its author. It holds by construction for the part of a rule that is about one grid, and by discipline for the part that asks about the world:
- *A rule that is about one grid* (what burns next to a fire, how light spreads, what a block does to its neighbours) works aboard if it names voxels as `(GridId, VoxelCoordinate)` and reads and
  writes them through `context.grid(id)` / `world.grid(id)`. The pending work (`ActiveVoxels`), the change events (`ChannelsChanged`) and the random numbers (`voxel_random`) are all keyed by grid, so
  the rule never has to know it is aboard. **There is no call keyed by level** (the owner removed them, 2026-10-22): `ServerWorld::grid(GridId)` is the only way to read or write voxels, and the terrain
  of a level is `GridId::root(level)` (`GridId::MAIN` for the main level), a grid like any other. `GridView` has `get`, `set`, `uniform_value` (is a brick one value?) and `generated` (what the
  terrain generator made there; the channel's default for a ship). A rule takes the `GridId` it is given by an event or a pending voxel and passes it on; a rule that is about the terrain of a
  particular level (Eels' arena, 2048's board) says `GridId::MAIN` or `GridId::root(level)` where it starts, and keys everything after by grid, as 2048's blocks and movers now are.
- *A question about the world* (is a player near this voxel? how high is it? is it under the sky?) needs the voxel's place in its level, which for a ship is the voxel turned and placed by its pose.
  The engine answers it in one place for every grid: `WorldData::grid_frame(grid)` (a `GridFrame`: the terrain of a level, or a ship with its level and pose),
  `WorldData::voxel_in_world(grid, voxel)`, `PlayerPose::distance_to_voxel_of`, and for a rule that looks at many voxels a tick `PlayerReach::new(&presence, world)` and `reach.near(grid, voxel, distance)`.
  A rule must not do this arithmetic itself.
- *Directions.* A ship turns, so "down" in its coordinates is not always -y: `ServerWorld::gravity_in(grid, place)` gives gravity in the ship's axes. A rule that moves things down must use it.
  (No rule of Cave does yet; fire spreads to all six neighbours alike.)
- *Grids are islands.* Two grids that touch (a ship on the ground, two docked ships) do not interact: fire does not spread from a ship to the ground it rests on, light does not cross, fluids would not
  flow. Most rules are right to be so; a rule that wants the contact must ask for it, and there is no engine support for it yet (`plan.md`, Open).
- *The check.* `games/voxunami_cave/tests/ship_parity.rs` runs each rule's scenario once over a `Site` (`tests/common/parity.rs`) in the terrain and in a ship, with the same blocks at the same offsets
  and the same place in the world, and fails if they differ. A game adds the scenarios of its own rules there; a rule with no scenario is a rule nobody has shown works aboard.

### 2.8 Saving, loading, and ships that vanish with their pilot

- **Files:** each grid is saved as `<world name>.ships/<grid id>-<tick>.voxship`, in the container format, with its own `engine:registries`
  and channel tables (so it is readable in any world) and its bricks (the same brick encoding); the optional section `engine:grid_flight`
  holds its pose, velocities, held flag and pending rule work; a load into another world, or a second load into the same one, drops
  `engine:grid_flight` and allocates a fresh `GridId` (the decision of 2026-10-18). Pending rule work is rebuilt by waking the voxels.
  Unknown sections and plugin ids are preserved byte for byte (hard rule).
- **One consistent save:** ship files are written first (each by the existing atomic temp-then-rename writer), the world file last, and
  the world file names exactly the ship files of this save (grid id, file, tick, checksum); older ship files are removed only after the
  world file is in place. A crash leaves the previous consistent set. The world file's new section `engine:grids` holds the counter,
  the tombstones, and the list of ships with their level, parked or not, and file.

**As built in 32i, part 1 (the files and the consistent save; `engine/world_storage`, `engine/server`, `engine/game_shell`):**
- *The name of a file is the ship and the number of the write, not the tick of the first text:* `<id>-<n>.voxship`, in `<the whole name of the world file>.ships/` (`cave.vxw.ships`: the extension is not replaced, so that two world files of one name do not share a folder, one cleaning the other's files away),
  with *n* from a counter that is saved in `engine:grids`. Ticks begin again every time a server starts, so a new run would write a file with the name that the
  world file on the disk still names, and a crash between the two writes would leave a world whose ship does not match its checksum; a counter that only goes
  up cannot. `ShipFileName` can be made only from a ship and a number, or read in exactly that spelling (no sign, no leading zeros, no root grid), so a world
  file cannot send a program to another path.
- *The file is a container* of `engine:header`, `engine:registries` (the tables of the registries its channels use), `engine:channels`, `engine:ship` (the name of
  its level, and its bricks written as a level's edited bricks are), `engine:grid_flight` (pose, both speeds, held, what its pilot's logout does) and, for a planet, `engine:grid_terrain` (Phase 39a: the generator by name, the box of bricks it makes, the mass). Sections of
  plugins that nobody here reads are carried in `MovingGrid::sections` and written back as they were. A file that names content the reading world lacks is
  refused, saying what (`ParsedShip::referenced_content` lists it, so that an importer registers it first); a channel the world has no place for is refused too.
  Every cut of a file and every changed byte of it is found (tested for each byte and four changes of it).
- *The world file's `engine:grids`* lists, in order of id, each ship's level, pose, the bricks it touches, the number and the checksum of its file, and its flags (one byte: bit 0, it
  waits for those aboard; bit 1, it is a planet, which has terrain; Phase 39f: a world from before reads the byte as 0 or 1, as it was); then the counter of ids, the counter of file numbers, and the retired ids with what took their places and the lattice turn between
  (one of the 24). A world that never had a ship writes none, and saves to the bytes it always had.
- *After a load every ship is in its file* (`GridStore::parked`), and the world knows where it is, in which level and which bricks it touches, without reading
  it. `ServerWorld::load_ship` brings one into memory (the checksum is checked first); `park_ship` puts one away (the file is written first); each changes
  nothing if it cannot. The rules hear of a ship that came in as a change of every voxel that is not at its default in each channel they watch (`take_changes`,
  after the changes of the tick), as for a ship that was just made, which is how "pending rule work is rebuilt by waking the voxels" is done. Until the
  lifecycle of part 2, a server brings in every ship when it is made (`Server::load_parked_ships`).
- *A save is a `WorldSave`:* the files of the ships that are in memory, then the world file, then the removal of the ship files that the world file does not name.
  Tested with a disk that fails at each of its calls in turn: the world file that is there is the old one or the new one, whole, with every file it names; the
  next save, when the disk works, leaves nothing else. `Server::save` returns it and the shell writes it into the folder beside the world file, for the autosave
  and for the save on exit. A world with a ship in memory cannot be saved as a world file alone (`SaveError::ShipsInMemory`), so the mistake is loud.
- *`engine:entities`:* an entity in the frame of a ship is written in it, with the place it has in the world, in the byte the format had reserved for that. A file
  that lists the ship has them aboard; one that does not has them at that place (else in the main level at the coordinates they have: nothing is lost).

**As built in 32i, part 2 (the lifecycle; `engine/grid_lifecycle`, `server::ShipSettings`):**
- *The settings* are `ShipSettings { simulation_distance, unload_distance, logout_policy }` in `ServerConfig` (and `Server::with_ship_settings`). The defaults are 256
  and 1024 voxels (the spec's metres: a voxel is a metre at the scale Cave is played at). `ShipSettings::new` refuses a negative or non-number distance and an
  unload distance below the simulation distance; infinity is a distance (`ShipSettings::everywhere`, which the tests of what ships do use, since they have no
  player near them). `serve` has `--ship-simulation-distance`, `--ship-unload-distance` and `--ship-logout-policy`; the game that one plays alone uses the defaults.
- *`GridLifecyclePlugin`* (the system `engine:grid_lifecycle_update`, in the controllers phase, before anything moves) looks at every ship once a tick, in order of id.
  A ship in memory and awake is frozen when no player of its level is within a tenth more than the simulation distance of its nearest brick; a frozen ship is let go
  when one is within the simulation distance, and put in a file when none is within a tenth more than the unload distance; a ship in a file is brought in when one
  is within the simulation distance, unless it went away with whoever was aboard it. Distances are compared as squares, so nothing takes a square root. A
  ship that cannot be written or read is left as it is and tried again after 200 ticks, not every tick; the plugin's report says why.
- *Frozen* is `MovingGrid::frozen` (not saved), set by `ServerWorld::set_grid_frozen`, which also takes its speed away. The bodies hold a frozen ship as they hold a
  held one, and the rules wait: `ActiveVoxels::take_simulated` leaves the voxels of a grid that waits pending, both light rules use it (`LightWorld::is_simulated`
  and `ScaledLightWorld::is_simulated`, which Cave answers with `ServerWorld::grid_is_simulated`), and the light of a ship is made when the ship is let go. Fire
  never ran on a ship (a ship is never "near a player" to it), so it has nothing to keep.
- *A player who joins aboard a ship that is in a file* has it brought in by a handler of `PlayerJoined` (after the players' own), before anything places them. The
  record of where a player is is read through `GridStore::frame_of`, which knows ships in files, so a player in the frame of a ship that is in a file is in the world
  where the ship was put away, in its level, and is told so; `spawn_entity` and `set_entity_pose` accept a frame that is in a file.
- *A server no longer brings every ship in when it starts* (part 1's temporary rule): a ship in a file waits for a player to come near, or aboard.
  `Server::load_parked_ships` stays for a program that wants them all.

- **Loading on demand:** a parked ship is loaded when a player comes within `simulation_distance` of its saved pose, or when an entity
  whose frame it is (a player aboard) must be placed. A file that cannot be found or read is the missing-ship case: the player's entity is
  held and the prompt (27h) with a modal form (30) asks: spawn at the world spawn, spawn in midair (at the world-space fallback pose saved
  with every entity whose frame is a grid, requirement 16), or quit to the title.
- **A ship that vanishes with its pilot** (the owner's wish, 2026-10-20): the server setting `ship_logout_policy` is `chair` (each ship's
  own `on_pilot_logout` decides; the default), `always_stay` or `always_vanish`. When the pilot leaves: if the policy gives *vanish*, the ship
  is saved and parked at once, the way the horse goes with its rider; when the pilot returns, it is loaded and placed at its saved pose,
  with them aboard. In this phase the pilot is whoever `/ship pilot` made it; the command chair's setting arrives with the chair (Phase 35),
  and writes the same header field.
  - *Others aboard:* a ship with any other player aboard does not vanish (they would lose their floor); it stays, held, and the pilot's
    return finds it where it was.
  - *Blocked arrival (owner, 2026-10-20: ask the player):* if the saved pose now overlaps terrain or another ship, the returning pilot is
    asked with the prompt facility, with these options: **move ship** (the engine's deterministic search for the nearest free pose within
    32 m, upward first), **accept damage to terrain** (the ship is placed; the terrain blocks it overlaps are destroyed), **accept damage to
    ship** (placed; the ship's blocks that overlap are destroyed), **spawn in terrain (glitchy)** (placed overlapping, nothing destroyed;
    bodies resolve it however they do), **warp to world spawn (leave ship behind)** (the pilot goes to the level's spawn and the ship stays parked). The last two are
    my reading of the owner's wording. The ship is held parked until the answer. The options are a list in the code, so more can be added.
  - *Offline passengers (owner, 2026-10-20: they return aboard, even if the ship has moved):* a passenger who logged out aboard stays in the ship's frame as a hidden entity (players are hidden, not removed),
    so a parked ship keeps them, they return into the ship wherever it now is, and a split or a merge in the meantime re-frames them like everyone
    else aboard. A test covers: log out aboard, the ship moves, log in, still aboard at the same local place.

**As built in 32i, part 3 (the pilot's logout and the questions; `engine/grid_lifecycle`, Cave's `ShipCommandsPlugin`):**
- *The pilot's logout* is a handler of `PlayerLeft` in Cave (`voxunami_cave:a_player_who_leaves_stops_piloting`). The player `/ship pilot` made the pilot of a ship stops piloting when
  they leave, and the ship goes with them if the policy says so: `chair` (the default) leaves it to the ship's own `on_pilot_logout`, which `/ship vanish on|off` sets until the command
  chair exists, `always_stay` and `always_vanish` say it for every ship. A ship that goes: if another player who is connected is aboard it, it stays and is held instead (they would lose
  their floor, and the pilot finds it where it was; the handler is ordered after the players' own, which has taken the leaving pilot out of the record of who is where, so the check finds
  only the others, and the host refuses to start if that handler is renamed); otherwise it is stopped (both
  speeds zero, so it is at rest when they come back) and put in a file that waits for those aboard (`park_ship(ship, true)`). If it cannot be put in a file, because the world has nowhere
  to keep ships or the disk fails, it stays and goes on as it was, its speeds given back. Players who are away do not count as aboard: their entities stay in the ship's frame, hidden,
  and whichever of them joins first has the ship brought in, as the pilot would. A server that is stopped (a singleplayer window closing, Ctrl-C on a dedicated server) does not run the
  logout of the players who are connected, so nothing vanishes then: the ships are saved where they are, and the players are aboard them when they join (an open point, in `plan.md`).
- *Coming back.* The game gives the lifecycle the world's spawn (`GridLifecyclePlugin::new`; Cave: the start of its default camera) and, to have it look at where a ship that went away
  lands, what blocks are to a body and which channels are blocks and states (`with_arrivals(ArrivalConfig { .. })`; without it such a ship is brought in where it was). When a player joins
  aboard a ship that waits for those aboard, the ship is read from its file, not yet admitted, and its **hull** is made: the cells whose block is not empty to a body, each as the place of its
  centre (the empty cells of its bricks are not in it). The hull is *free* at the ship's saved pose if no centre is inside the terrain of the ship's level or inside a block of another ship
  there; a place the world cannot hold counts as inside. If it is free the ship is admitted as it was, in the tick the player joined. If not, it stays in its file and the player is asked,
  with the prompt facility, in this order: **1** move the ship to the nearest free place (moves of whole voxels along the world's axes, within a ball of 32 voxels, nearest first; of two as
  near the higher; of two as near and as high the lower x and then the lower z; the ship is not turned; if there is none the player is told, and asked again with that said), **2** break the
  terrain that is in the way (the voxels of terrain that hold the centre of a blocked cell, every channel back to its default), **3** break the blocks of the ship that are in the way,
  **4** put the ship there anyway, **5** go to the world spawn and leave the ship behind (it stays in its file and no longer waits for those aboard, so whoever comes near finds it).
  The look is of centres: it finds every overlap deeper than half a voxel, deterministically and quickly, and the bodies push the rest apart. A ship that did *not* go away with its player
  (it was put away because nobody was near) is brought in without the look, as it would be had they never gone.
- *A ship that cannot be found* (its file is missing, or damaged, or not the one the world file names) when a player joins aboard it: the player is asked to **1** go to the world spawn,
  **2** appear in the air where they were (the place in the world that their place in the ship had when it was put away) or **3** quit to the title (they are disconnected, "you chose to quit").
  The ship stays on the list of the world file with the reason in `LifecycleReport::problems`, so a file that is found again can be brought in.
- *The questions* are asked of one player each and are not saved: a server that stops while a player is asked asks again when they join. A player who leaves is no longer waited for
  (`LifecycleReport::waiting` counts those who are). When one player's answer brings a ship in, the questions of the others about that ship are withdrawn, and only those. An answer to
  another plugin's question is not taken for an answer about a ship (every plugin hears every answer). The player numbers the options from 1 (`.choose 2`); the code numbers them from 0.
- Tests: `grid_lifecycle/tests/arrivals.rs` (24 tests, the engine alone on the pretend game: each answer, the radius of the search to the voxel, the other level, two players, two ships,
  another plugin's question, a file that is lost) and Cave's `ship_vanishing.rs` (16 tests: the commands, the policy, others aboard, a ship at speed, a restart, a disk that fails, the
  screen of each question and an answer to it).

### 2.9 Gravity as a field (built in 32f, because the API is what is costly to change)

As built: `voxel_space::{GravitySettings, MassPull, gravity_at}` (the pure field, in `f64` with only products, quotients and square roots; bits pinned); `ServerConfig::gravity` (a server setting, also
changeable at run time with `ServerWorld::set_gravity_settings`) with the level's constant `base` (default 32 down, as characters fall), a `pull_strength` (default the real G, 6.674e-11, so that only a moon
pulls), an optional `pull_mass_threshold` (default none: nothing pulls) and a `softening`; ships carry the `mass` and `centre_of_mass` the bodies work out from their blocks;
`ServerWorld::gravity_at(level, position)` (the level's axes) and `gravity_in(grid, local position)` (the grid's own axes: for a ship the world's gravity there, turned into the ship's axes), and for
plugins `Context::gravity_at(grid, local)`. The bodies are pulled by it: each tick a ship that moves gets the force `mass * gravity` at its centre of mass (a ship's own pull is nothing there, so nothing is left out); a ship
at rest sleeps while that is the same and is woken when it changes by more than a thousandth. Characters do not feel it yet (they fall in their own constant, 32g puts a player aboard a ship in the ship's frame
with the field in its axes).

`GravityField` in the physics crate: `gravity_at(frame, local position) -> local vector`, summing *sources*: the level's own source (today's
constant, pointing down) and, for each ship whose mass is at least `ship_gravity_mass_threshold` (a server setting; default: none, i.e.
off), a point-mass pull toward its centre of mass of strength `ship_gravity_strength * mass / (distance² + softening²)`. Ships fall in the field
as well. The service on the plugin context is `gravity_at(local position)`, not `gravity()`: the plan's requirement 1 said `gravity()`, and a
field that varies in space needs a position, so I propose changing the name and signature now, while no plugin outside the repository exists.

Three things I can say about it, with their confidence:
- A point mass at the centre of mass is exactly right far from a ship, and wrong near the surface of a long or flat one (the pull would
  point at the centre, not at the nearest hull). The existing decision to compute the direction "from coarse LOD mass" is the better answer
  and needs the coarse levels of section 2.10; the point mass is the placeholder with the right interface. (Confidence: high on the
  limitation, medium that a placeholder is worth building before the coarse levels.)
- For someone standing on a huge ship, the dominant pull is toward the ship's own centre of mass, which is a *fixed* vector in the ship's
  frame. So aboard such a ship gravity feels steady, not rotating, which is what a moon would feel like. This falls out of the design with no
  special case. (Confidence: high.)
- Which ships pull whom: all the huge ones pull everything, including each other; ship to ship pull is the only O(n²) piece, and there are
  few huge ships. I would not special-case it. (Confidence: medium.)

### 2.10 Reserved for the owner's other wishes (nothing here is built in this phase unless it says so)

- **Levels of detail for ships.** Planned so that nothing blocks it: the mesh key is `(grid, brick)` and becomes `(grid, brick, level)`;
  coarse levels would come from the grid's own octree in its own coordinates and be drawn under the same per-grid matrix; interest is
  per grid and already has a budget. Not built, because terrain's levels of detail do not exist: building them for ships alone would be a
  second system; they should be built once, for both, in a rendering phase of their own (it is the largest part of the "no hard limit on
  size" promise, and I would schedule it before ships get big, not before ships exist).
- **Fluids, and current on a channel, crossing a ship's edge.** A rule can never read another grid. The planned door is a *mailbox*:
  a rule emits an `Outflow { grid, local voxel, direction, channel, amount }` to the engine; at the tick boundary the engine converts
  the voxel with `voxel_in_other` (built now, section 2.2) to the other grid (the nearest grid whose body contains that point, found
  through the body world), and delivers an `Inflow` to that grid's rules in a fixed order (by target grid, then voxel, then source grid),
  with conservation (an inflow nobody accepts goes back to the source, so nothing is created or lost) and a rate limit. Whether water that
  leaves the bottom of a ship stops at the edge or re-enters the nearest world voxel is then a per-fluid and per-server policy, which is the
  shape the owner described. One caveat: a turned ship's voxel grid does not line up with the world's, so several of its voxels can map to
  one world voxel and some world voxels get none; the mailbox must therefore accept amounts, not voxels, which is why fluid and current
  should be modelled as quantities on a channel (they already are, in the plan). Built with the first fluid, not now. Also: rules aboard
  see the gravity field only, not the ship's acceleration, so water in a ship that accelerates does not slosh (the same choice as in 2.6).
- **Simulation distance** is the setting in 2.7. **Pilot logout** is 2.8. **A huge ship's gravity** is 2.9.

## 3. What it does not do (and why later)

- Command chairs, third-person view, clicking parts, drills, turrets, headlights (Phase 35, 34): the chair needs control targets and block
  data, which no earlier phase has needed.
- A sub-grid on a hinge (octopus, pistons): bodies and frames are here, joints are not.
- Buoyancy and thrust from blocks (needs fluids and a thruster block).
- Sky exposure of a local voxel (requirement 1 named it): it needs a world query across grids. A ship's light in this phase comes only from
  its own lamps; it receives no sky light and the sun does not light its interior through a window. Recorded as a limit, not a bug.
- Merging a ship into the terrain (landing). `/ship create` is a one-way trip in this phase.
- Terrain levels of detail, fluids, the mailbox, a simulation distance for terrain.
- Fictitious forces aboard (2.6), and fossilization (a protected state for a ship whose pilot logs out, `future-goals.md`, "Ship goals" 7).

## 4. Steps (each ends with fmt, clippy, tests on Windows and Linux, mutations, docs, then commit and push)

The phase is large; I propose a pause for the owner after 32h, before docking.

- **32a Frames** (2.2): the pure module, with property tests first (round trips, composition, exact 90-degree turns, rounding half toward
  negative infinity, alignment). No other crate changes. Safest piece, so first.
- **32b Grids on the server** (2.1): `GridStore`, the id counter, tombstones, `ServerWorld::grid` for allocated grids, `create_grid` /
  `remove_blocks_from_grid`, edit replication with a `GridId`; the frame-invariance harness of requirement 6 (the same rule in a static grid
  with local gravity and a turned, moving one, by turns of 90 degrees) and its first users: the existing Cave rules (light, fire) under it.
- **32c Protocol and replay** (2.4): version 6, the grid messages, interest, `GridId` in the edits and bricks of the stream, replay golden
  sessions regenerated. (`GridId` in the client's break, place and use messages is in 32d, with the picking that can name a ship.)
- **32d Renderer and picking** (2.5): the rotation, GPU tests that a turned grid draws the same picture as the same blocks laid out in world
  space (pixel-compared with `settled`), picking tests in turned frames.
- **32e Bodies** (2.3): the `BodyWorld`, compound colliders per brick, mass, terrain contacts; deterministic digests on Windows and Linux;
  the cost measurement for 10,000 and 100,000 blocks edited each tick (reported, then the budget chosen).
- **32f Gravity field** (2.9): the service and its consumers (character, bodies, rules), `gravity_at(local)`, the setting for huge ships.
- **32g Aboard** (2.6, built): the character in a ship's frame, boarding and leaving, the transition scenario tests, replay equality.
- **32h Cave's ship commands and the first flight** (2.7 creation and held): `/ship create | push | hold | pilot`; an end-to-end test that a
  ship is made, flown, boarded, turned and left, recorded and replayed to the same picture (built, `flight.rs` and `pictures.rs`). **Pause for the owner here.**
- **32i Saving and loading** (2.8, built in three parts): ship files, `engine:grids`, loading on demand, the missing-ship prompt, the consistent save, the
  vanish policy and the blocked arrival search, the simulation-distance setting with freeze and park.
- **32j Fast ships stop at thin ground** (2.3; built 2026-10-21, see the build log; found by the owner's first play of 32h, measured 2026-10-21; done *before* splitting and merging, see "Why here" below).
  *What happens:* a ship goes through ground that is thinner than it moves in one solver step. `BodyWorld` runs 3 steps of a twentieth of a second, so a ship
  moves its speed divided by 60 in a step, and (measured in the `physics` crate alone, cubes of 1 and 4 and rafts of 10 by 10 dropped on a slab; the ship's size makes no
  difference) it goes through a floor **1 voxel** thick from about **96 voxels a second**, and through floors of 2 and of 3 voxels from about 200 (with 6 steps: 200, and
  400). A 1-voxel wall is what a player builds; 96 is a fall of about 150 voxels, not a flight (a ship flown with control held does about 48). Rapier caps speed at 400 a step.
  *Why a flag does not fix it:* Rapier does continuous collision detection by itself, for every fast dynamic body against fixed colliders, but it leaves voxel-shaped
  colliders out of it (`shape_never_ccd_swept`: a body made only of them has an infinite CCD thickness and is never counted as fast), and the whole cubes of a ship are one
  `Voxels` collider (2.3). `ccd_enabled` and `soft_ccd_prediction` on the ship bodies were tried and change nothing, bit for bit.
  *The fix is ours and small:* `BodyWorld::step` chooses the number of solver steps for the tick from the fastest point of any dynamic body (its speed plus its turn
  rate times the radius of its colliders) so that no point moves more than about 0.8 voxel in a step: 3 for anything under about 48 voxels a second, 24 at most (the cap
  of 400). Measured, 28 steps stop every hull at 400 even on a floor of 1. Slower scenes are bit-identical to before (their pins hold); only scenes that go faster than
  48 change, and the fall of 300 in `ship_physics.rs` is one. It helps ships hitting ships too. *Tests first:* the table (hull by floor thickness by speed) that fails
  today, kept as a test; a thin raft on a thin raft; the Cave case, a ship from 1000 above the flat floor stops on it (today it lands on the hills below, and at 357 a
  second a cube of 4 ends 4 voxels *above* the floor, which is not understood: the step must explain it or remove it); the cost of a ship of 100 and of 1000 blocks at the
  top speed. *Confidence:* high that it works (measured); medium on the cost for big ships (only fast ones pay, and only while they are fast), which the measurement decides.
  *As built:* `BodyWorld::steps_for_this_tick` (constants `MOST_A_STEP_MOVES` 0.8 and `MOST_STEPS` 24). The "4 above" was not a bug: a cube that tumbled rests on another face, with its origin corner a cube's width up.
  Not chosen: our own shape cast to limit the velocity (more code and more cases: rotation, compounds) and a lower speed cap (it would show in play).
- **32k Terrain colliders on a budget** (2.3, 2.7; built 2026-10-22, see the build log; found by the owner's first play of 32h): a big ship (a 30 by 30 and a 40 by 40 raft measured, 900 blocks and more) still
  has single ticks of 70 to 80 ms as it reaches new natural ground, because the terrain within 8 voxels of such a ship is 8 to 27 bricks and each costs 5 to 25 ms to
  generate and make colliders of on the thread of the tick (the generating is a third to a half of it). 32i makes the same cost appear when a ship is loaded or unfrozen
  next to natural ground, and 32l's connectivity check needs the same thing, "at most so much work a tick", so this step builds that once: **a per-tick budget counted
  in units of work, never in time** (a wall-clock budget would make the simulation depend on the machine, which the hard rule forbids; a count is the same on every machine).
  A ship's terrain has two zones: the *needed* zone, the bricks that the ship's bounds sweep in this tick (and a voxel or two around), is always made, so a ship can never
  reach a brick that has no colliders; the *wanted* zone, the rest of the lookahead (8 voxels and five ticks of speed), is made nearest first, at most as many bricks a tick as
  the budget says (chosen by measurement, about 2), ties broken by brick coordinate. A ship that has just been made, loaded or unfrozen, and so has a large needed zone
  all at once, is the case to decide by measurement: wait the few ticks it takes (kinematic and asleep, as a frozen ship is) or pay the spike once. The same budget is the
  "bricks made again per tick" of 2.3, for a ship edited in many bricks in a tick. *Tests first:* a big raft falling to natural ground (the worst tick of the 30 by 30 raft is
  82 ms and of the 40 by 40 73 ms today) has no tick over 30 ms; a ship's path with the budget is the path without it whenever the lookahead is long enough (a property
  test over speeds); a ship cannot reach a brick without colliders however small the budget is. Related and *not* part of this step: natural terrain costs 5 to 10 ms a brick to
  generate, and every reader pays it when the world's cache of 128 bricks has forgotten the brick (plan.md, Open). Generating is a pure function of the seed and the
  coordinate, so a worker thread that fills that cache ahead of the tick cannot change any result (the tick would find the brick, or generate the same one itself): it is the
  next idea if the budget leaves ticks too long, and it needs no change to the simulation.
- **32l Splitting** (2.7, built 2026-10-22, see the build log): the budgeted connectivity check (on the budget of 32k), parts, velocities, entities re-framed, events and the stream.
- **32m Merging and docking** (2.7): connectors, attraction, snap, merge, welds, undock refusal; merge invariance and split invariance tests
  (requirement 14). Built in parts: (1) the merge operation `ServerWorld::merge_grids` (built 2026-10-22); (2) docking: ports, attraction, snap, merged velocity, undock, Cave's dev docking port and
  `/ship undock`, fire across a former boundary (built 2026-10-22); (3) the invariance tests of requirement 14 (built 2026-10-22, below), every block in every orientation and metric gravity.

*Why 32j and 32k are here (decided 2026-10-21):* both change the body layer that splitting and merging build on, and both would change the pins those steps make
(32l and 32m make new bodies, with velocities, in new orders), so doing them after would mean pinning twice; the budget is a facility that 32l uses; and neither
needs anything from 32i, which is why they are not before it (the owner asked for saving next). Splitting was 32j and merging and docking 32k in the text before
2026-10-21; they were renumbered so that the letters stay in the order the steps are done.
## 5. Risks

- **Aboard physics (32g)** is the one I would bet on being the source of most bugs. It is the reason for the scenario tests, and for
  stopping at 32h to check the feel before investing in saving and docking.
- **Collider cost (32e).** If compound colliders rebuilt per edit are too slow for large ships, ships become slowly-updating bodies; the
  measurement decides, and the spike's numbers are for a static ship.
- **Determinism of bodies in many orders.** Rapier's `enhanced-determinism` held in the spike for the cases it tried. Bodies are created,
  removed, split and merged here in many orders, so the golden digests must run bodies in a shuffled-creation test.
- **Protocol churn.** `GridId` in edits and actions touches client plugins and Cave; it is the right place to pay for it (nothing is released).
- **Rapier's continuous collision detection does not cover voxel colliders (found 2026-10-21).** A ship goes through ground thinner than it moves in a step; the
  fix (32j) is more solver steps for fast bodies. Anything that relies on Rapier's CCD for a ship (a flag, a "bullet" body) will not get it.
- **Scope.** This is larger than any earlier phase; the pause after 32h and the order (safe, pure pieces first) are the control.

## 6. Questions for the owner, answered (2026-10-20)

1. One phase with a pause after 32h: **default.**
2. Ships exist through Cave's `/ship` commands on an engine API: **approved.**
3. Unpiloted ships: **not held. Their movement physics and gravity continue to be simulated** (held remains a per-ship option). This replaces my recommendation.
4. Gravity field and point-mass source for huge ships, off by default: **default.**
5. `gravity_at(local position)` replaces `gravity()`: **default.**
6. A blocked arrival: **ask the player**, with the options in 2.8; fossilization (a ship made indestructible and immobile except by commands, with
   `/ship fossilize` and `/ship defossilize` for operators only) is a later way to protect a ship (`future-goals.md`).
7. Simulation distance defaults (256 m, 1024 m, server settings): **default.**
8. Terrain levels of detail after ships: **default.**
9. No sky light on ships in this phase: **default.**

Also decided: a split gives every part a new id; a merge gives the result a new id; both leave tombstones. A passenger who logs out aboard and
returns after the ship has moved is still aboard.
