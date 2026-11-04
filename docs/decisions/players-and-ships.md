# Players and ships: what was decided

Entities on the wire, who a player is, players as entities, ships a player can make and fly, and players aboard them. Specs: `entities-spec.md`, `moving-grids-spec.md`. Moved from `design-decisions.md`, which keeps what holds for every game; the stories behind these entries are in `history/decision-diary.md`. Status key as in `design-decisions.md`.

## Decided (structures such as airships)
- Structures such as airships (owner, 2026-10-18 to 22; the goals are in `future-goals.md`, goal 2; built as Phase 32; the full text is in the diary):
  - Gravity aboard is the world's gravity, in the grid's local axes (a player on a ship rolled on its side falls toward the world's down). Ships rotate
    fully. There is no hard limit on the size of a ship: limits are budgets (work per tick, as for every rule), not caps.
  - **Docking merges**: two ships that dock become one rigid body and one grid, and anything passes the join (crew, items, fire, light, the rules of any blocks
    that touch). Blocks flush with the ports are welded and the ships cannot undock until the player breaks them. The yellow face of a dev docking port
    docks with another's: snap within 10 degrees and 0.3 metres, any roll in quarter turns; the pull is on the centre of mass with the torque of the lever
    and **does not scale with mass**. The dev port is creative only. Face properties (`engine:marked_face`, `engine:mark_color`, `engine:docking_face`) are
    engine-reserved block properties, so the definition format did not change. Merging is `ServerWorld::merge_grids`, undocking `split_grid`.
  - **The dev command seat and the dev rocket engine** (owner, 2026-11-04; `voxunami_cave:dev_command_seat` and `dev_rocket_engine`, creative only, like the dev docking port): the first way to fly a ship with the keys of a
    space game. `/ship sit` or `U` sits a player in the seat they look at (within 6 voxels, aboard the ship); W S pitch, A D yaw, Q E roll, shift raises the throttle and control lowers it (half the whole a second). The seat gives
    **a large torque, about a radian a second a second about any axis whatever the ship weighs** (so it works from an estimate of the ship's inertia), up to a turn of a radian a second; an axis with no key down is braked to a stop,
    exactly, so that a ship can go on a rail. *Mine:* the braking and the rate cap are mine (the owner asked for torque on the keys); without them the ship tumbles on. Every engine of the ship pushes it with 200 kN at full
    throttle, opposite its exhaust and from its own voxel (so an engine off the middle also turns the ship), with no fuel and no gimbal. The engines work only while someone sits; getting up cuts the throttle. A pilot is in the
    record of pilots (the hover controls of `/ship pilot` and the seat keep out of each other's way). The readout of the flight (throttle, turn, distance and speed against the planet that pulls most, the orbit) is a message
    to the seated player, drawn as lines of the HUD. Not done: a seat that moves the player into it, a dedicated key for the throttle to full and to nothing, an altitude over the ground (the server does not know a planet's
    surface radius), any gimbal.
  - A ship is saved while in flight, as a grid in a file of its own, portable to another world. A player whose ship file cannot be found is asked: world
    spawn, midair, or the title screen.
  - **Units are metric**: a voxel is a metre; kilograms, newtons, newton metres, seconds. Until materials have densities every block that is not air weighs
    1,000 kg a cubic metre (`engine:density`). Gravity in the overworld is 5 m/s^2 (owner, 2026-10-22).
  - **Every block has an orientation**, saved the same way for all blocks (channel `base_blocks:block_orientation`, 0 to 23, the order of
    `LatticeRotation::all()` pinned as a file format), so turning is composing numbers with no knowledge of the block, and a plugin that is not loaded is
    turned too. `BlockTable` is indexed by block, state and orientation and de-duplicates shapes. A block with a front face is turned to face the player, one
    with an axis lies along the face it was put against; the client says which face it clicked (`SetBlock.against`) and the server works out the turn.
  - A rule with chance keyed by grid does not go on bit for bit across a merge or a split, only in distribution (owner).
  - Added by the owner 2026-10-20 (`future-goals.md`, "Ship goals"): huge ships pull; ships are drawn with levels of detail far away; fluids may flow
    inside a moving grid and cross a ship's edge; a ship may vanish with its pilot's logout (a server setting); the simulation distance for ships is a
    server setting; felt acceleration and fossilization are future options. Unpiloted ships stay simulated; every part of a split and every merge gets a
    new id; a logged-out passenger returns aboard; a blocked arrival asks the pilot.
  - Order of work: moving grids (Phase 32) come before the native and WASM loaders (Phase 33), because the loader ABI encodes the coordinate model.

## Decided (entities on the wire; built in Phase 27c)
- **Protocol versions.** (Revised in Phase 28.) A version is bumped whenever a message changes, and a program reads only its own version: no
  recording has been made and nothing is released, so nothing older is kept readable (the `OLDEST_READABLE_PROTOCOL_VERSION` constant, the
  old `Players` message and the session's fallback for it are gone). Once there is a release, keeping recordings watchable becomes a decision
  to take again. The version is in the `Welcome`, so changing it changes the stream hashes of the golden sessions and the pinned recording.
- Entities are replicated by interest: those whose grid is the root grid of the watched level and whose brick is in the watched region.
  The server sends the difference from what each client was last told (despawns, spawns, changes, in that order, in order of id), only the
  components a kind marks replicated, and a client store (`ClientEntities`) is built from the stream alone, so a replay shows the same
  entities.

## Decided (who a player is; built in Phase 27e, the owner's decision after review of 27d)
- **A keypair per install, no accounts.** The player is an Ed25519 public key, made once and kept in a file in the game's data folder (`--identity
  FILE` chooses another; the file is 32 secret bytes, never overwritten if it is not). There are no names, passwords or servers that know players.
  A client proves it holds the private key: `Hello` carries the key, the server answers `Challenge` (32 bytes), the client sends `Prove` (a
  signature of a text that names the game, the challenge and the key, so a proof is good for one login to one game). A key can play once at a
  time. Nothing on the wire is encrypted: this is proof of who, not secrecy. Moving to another machine is copying the file.
- **Challenges are random in real servers and counted where the only client is in the same process** (tests, the single-player host used by
  tests and headless runs), so recorded sessions are byte-identical. A server anyone can reach over a network must never count them.
- **A join takes two server ticks** (challenge, then welcome). A recording starts with the challenge, then the welcome; players of a replay
  prove nothing to anyone and use a throwaway identity. The golden sessions were repinned once for it; the cause was checked, not assumed (the
  old code with an idle tick before each join, against the new stream with the challenge removed and the version set back to 3, gives the same
  hashes in all three games).

## Decided (a body put inside terrain is moved out of it; found in Phase 27f by a flaky test)
- **`CharacterBody::new` and `teleport` clear the body at the next `update_terrain`**: if its box (a hair smaller than the body, 0.02 each
  way) reaches into terrain, as the collision source says voxel by voxel, it is moved the *shortest* way out: up, down, or sideways along an
  axis, in steps of an eighth of a voxel up to 64 voxels, up first at equal distance. Not "straight up": that launches a player through a roof
  or over a wall. If there is no room within 64 it stays where it was put. A voxel the world does not have counts as solid.
- **Why it is explicit**: the physics alone did not resolve it. Rapier's depenetration pushed a body some way sideways and left it inside
  blocks, and the brick colliders omit boxes buried in solid terrain, so a body deep in the ground touched nothing and fell through it.
- A body that is not inside anything is never moved (teleporting to where it already stands changes nothing, to the bit).
- Cost: only on placement, and only when inside terrain; a failed search is about a millisecond plus the generation of the bricks it looks at,
  which is why the limit is two bricks and not more.

## Decided (a walker that a wall stops climbs the step the controller declined; found 2026-11-02)
- **A character that walked exactly along an axis into the foot of a stair (yaw 0.0, the default look direction) stopped at it.** The cause is in Rapier's own
  stepping, not in the f64 interface or the nudge: it casts the body up along the face of the step, a graze at exactly the distance a hit counts at, and
  whether that hits is the sign of a 1e-14 rounding in the contact's normal (measured, and the mechanism is in `docs/history/joint-probe.md`).
- **Rapier's stepping is still tried first, and `CharacterBody::step_up` is the second try**, for a grounded walker that a wall has stopped (less than half of
  its push into the wall got through): raise the body by the step height, go on by the move, let it down, each by the controller's ordinary move with its
  stepping off (those mind a graze no more than a slide along a wall does). It takes the step only if the body could be raised by all of it, has the room on the
  step that Rapier's stepping asks for (half the radius and the skin), and comes down onto a floor not steeper than the controller climbs (50 degrees).
- **Why a second try and not a replacement**: where Rapier's stepping climbs, nothing changed, to the bit (the pinned hashes of the scripted run and of
  both ship runs are the ones from before); a replacement would have moved all of them with nothing to say which of the moves was a fix.
- **Kept as the controller has it**: a step with less room above than a step and a skin (0.61) is not climbed, even if the body would fit after the step.
- Tests: `a_character_walking_exactly_along_any_axis_into_a_stair_climbs_it_from_wherever_it_starts` (four directions, four places each),
  `..._is_stopped_by_what_it_may_not_climb` (a block, a wall, a ledge with no room on it, a stair under a low ceiling) and, in `foreign.rs`, a steep slope.

## Decided (players are entities; built in Phase 27f)
- **A player is an entity** of kind `player_presence:player`, with the key (server only), the connection number and the look angles (told to
  clients) and `engine:hidden`. Its pose is the player's pose; its level is the root grid. A player who leaves is hidden, not removed: it is
  in the save, and its key finds it again (the player resumes where they were, and is told their level). Another key never takes it over.
- **`engine:hidden`** is an engine component: replication skips a hidden entity (and despawns it for those who knew it); the world keeps it.
- **Clients know the players they can see**, by the entity replication, not everyone: players in another level, or beyond the watched region,
  are not known. `PoseBook` and the `Players` broadcast are gone (the message itself was deleted in Phase 28).
- The look angles are kept as reported (a component) beside the orientation quaternion, so that nothing recovers angles from a quaternion.
  The calls that read or move players take the world (`Presence`), a game holds it as it held the book.

## Decided (ships a player can make and fly; built in Phase 32h; the choices are mine unless marked, the owner's to overrule)

- **Ships come from `/ship`, Cave's command, on an engine that only knows how to detach blocks into a grid** (owner, 2026-10-20: approved). The engine
  gained two small questions and no rule: `ServerWorld::generated_voxel` (what the terrain generator made at a voxel, whatever was built there) and
  `Presence::view_in_the_world` (a player's eyes and the turn of their view, in the world).
- **A ship is made of what was built, not of the ground it stands on:** `/ship create` takes the blocks that differ from the generator's, joined
  through faces to the one the player looks at (up to 4096); `all` takes every solid block, which is the first text and is nearly always too many,
  since a boat built on a beach is joined to the planet.
- **A command is about the ship you are aboard, else the nearest within 48 voxels** of the box of its bricks.
- **A ship has colliders for the terrain close to it, not for the bricks around it:** within 8 voxels of the bounds of its own colliders and five ticks of its speed, and
  they are let go 16 voxels farther out. A brick of natural terrain costs 10 to 25 ms to generate and make colliders of on the thread of the tick, and a ship of a few
  blocks occupies a whole brick, so the bricks around its bricks were 16 to 64 of them at a time and stopped the game for a quarter of a second as it came to the
  ground (found by the owner playing, 2026-10-21). A walking player is the same: terrain within 8 voxels of the character and a quarter of a second of its speed, let go
  16 voxels farther out, where it had the 27 bricks around its brick (a hitch of 100 to 125 ms at each new brick edge).
- **A ship's collision with thin ground is kept by solver steps, not by Rapier's continuous collision detection (planned as 32j; measured 2026-10-21):** a ship's whole cubes are
  one `Voxels` collider, which Rapier's CCD leaves out, so the flags do nothing. `BodyWorld::step` will use more solver steps for the tick when a body is fast (3 below about
  48 voxels a second, 24 at most), which leaves every slower scene bit for bit as it was. A floor of one voxel is gone through from about 96 voxels a second today.
- **Work a tick may do is limited by a count of work, never by time (planned as 32k):** a budget in milliseconds would make the simulation depend on the machine, against the hard
  rule on wall-clock time. What a ship needs this tick (the terrain it can reach) is never limited; what it will need soon is made nearest first, a few a tick, and the same budget
  serves the connectivity check of splitting (32l).
- **A pilot flies by force, in the physics the ship already has:** the keys never move the ship's pose. Each tick before the ships move, a force holds the
  ship up against gravity, speeds it up toward the keys, and takes a part of its speed away, so the ship's mass, its collisions and the
  other ships' pull all still apply, and a recording of the stream is a recording of a flight. The way the keys point is taken from the pilot's
  request of this tick, not from the record of their view, which is a tick behind (found: the first W of a pilot who had just looked level went up).
  The numbers (24 a second a second, doubled by control; drag 1.0 a second) are a first guess for the owner's feel.
- **Turning is the physics's:** no key turns a ship yet; a force off its middle does. The controls are for the command chair (Phase 35).

## Decided (players aboard ships; built in Phase 32g; the choices are mine unless marked, the owner's to overrule)

- **A player's record is the truth about which frame they are in and where; the body is made to agree with it.** A player is an entity whose
  `grid` is a level's root grid or a ship (owner, 2026-10-18: "a character's pose is `(frame, local)`"). Everything that asks where a player is
  (`Presence::pose_of`, `players`, `level_of`) answers in the world, so a rule never has to know about frames; what moves players uses
  `local_pose_of` and `set_pose_in`. Taking a ship away (`remove_grid`) puts what was aboard in the level, in the same call.
- **Aboard is simulated in the ship's own coordinates, with the world's gravity turned into them** (owner, 2026-10-18/20): exact carry, no
  fictitious forces. The body's box and controller stand in whatever up the field gives; the way a character faces is carried along as up turns
  (parallel transport by the shortest turn), so a ship that yaws turns the player's view with it and a ship that pitches or rolls does not tip it.
- **Boarding and leaving are geometric rules looked at once a tick, not counted** (changed from the 3-tick hysteresis of the first text; the owner's
  to overrule): aboard when on the ground with a block of a ship a tenth below the feet; left when in the air with nothing of the ship under
  along gravity within the ship's box, or the body's middle a voxel outside it. The two conditions cannot hold together, so nothing flaps.
  Boarding takes away the part of the speed relative to the ship that points away from it: a body that stands on something is not moving off it
  (a walker that a falling ship has lifted onto itself was thrown ten voxels up at the speed the ship had, before this was added).
- **A character is never left inside what it can touch**: every tick one that something has moved into is moved out the shortest way (`CharacterBody::keep_clear`: up first, steps
  of an eighth, at most 8 voxels, else it stays), for terrain by its voxels and for ships by their colliders, with a box 0.02 smaller on every side. Found by measuring in 32g: a
  ship that fell on a walker pressed them through the floor of the world.
- **The view is the player's own angles in the room the body's frame says** (`PlayerPose::view_orientation`): the entity's orientation is the turn
  of that room and then the angles, so the angles are never converted when a player boards or leaves, and the stream of a player who never
  boards is byte for byte what it was. The camera has a `frame_rotation` and looks at its angles in it.
- **A ship that sleeps has no speed on record, and one that is held has none until whoever moves it says how fast** (`set_grid_held(true)` stops it):
  what a player who steps onto it inherits is its recorded speed at the place (`MovingGrid::velocity_at`).
- **A rule asks about the world through the engine's any-grid calls and is tested on a ship** (2026-10-22): `WorldData::grid_frame` / `voxel_in_world`, `PlayerPose::distance_to_voxel_of`,
  `PlayerReach`; `ship_parity.rs` runs a rule's scenario in terrain and in a ship, so "a mod's rules work aboard" is tested, not trusted. The level-keyed voxel calls are gone (owner,
  2026-10-22: pre-alpha, be bold): the one way to read and write voxels is `ServerWorld::grid(GridId)` and `GridView`, for the terrain of a level and a ship alike.
- **A ship that goes away with its pilot comes back where it was, or the player decides (32i):** it is stopped and put in a file; when a player who was aboard joins it is looked at
  against the terrain and the other ships, and if it fits it is there. If not the player is asked (move it, break the terrain, break its blocks, put it there anyway, or go to the spawn);
  the engine does not guess. A ship with another player aboard does not go. The questions are not saved.
- **Ships are kept by the distance of the nearest player in their level (32i, `engine/grid_lifecycle`):** moving within the simulation distance (256 voxels by default), frozen beyond
  it, in a file beyond the unload distance (1024), with a tenth more to leave a state than to enter it. A server with no player in a level keeps no ship of it awake. The host chooses
  the distances and what a pilot's logout does (`ShipSettings`).
- **A ship is saved as a file of its own, named by the ship and a write counter saved with the world, and a world is saved as a set (32i):** the ship files first, the world file that names
  them last, then the ship files nobody names are removed. Whatever stops a save, the world file that is there is whole and every ship file it names is there. A passenger is saved in the
  frame of its ship with its place in the world.
- **A player who comes back is told where they are** (`player_presence:placed`, in the world), because a client watches around its camera and a
  returning player may be anywhere. (Not yet done for `/tp`; a follow-up.)
