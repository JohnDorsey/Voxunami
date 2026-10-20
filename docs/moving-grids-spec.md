# Phase 32: moving grids (ships)

Status: proposed, waiting for the owner's answers to section 6. Nothing is built. Read from the plan (requirements 1 to 16, the owner's
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

A grid has a header: `on_pilot_logout` (`stay` or `vanish`, section 2.8), `held` (whether it moves when nobody pilots it, section 2.7), and
the origin id of requirement 10 (so a ship file loaded twice is two grids).

### 2.2 Frames: one small pure module

`voxel_space::frames` (no I/O, no physics): the pose of a grid, and these functions, all deterministic and pinned by tests and properties
(round trip, composition, exactness for turns by multiples of 90 degrees and whole-voxel offsets):

- `local_to_world(pose, local position) -> WorldPosition`, `world_to_local`, the same for directions (gravity, velocity);
- `voxel_in_other(from pose, voxel, to pose) -> Option<VoxelCoordinate>`: the voxel of the other grid containing the centre of this voxel,
  rounding half toward negative infinity, `None` when the matrix is not invertible. **This is the one cross-grid conversion** the owner's
  wishes for fluids and current need (section 2.10); building it now costs a function and its tests, and merges use it too;
- `lattice_alignment(from, to) -> Option<LatticeTransform>`: the turn (one of the 24 axis rotations) and whole-voxel offset that carries one
  grid onto the other's lattice, or `None`; docking's snap is "pull `lattice_alignment` toward `Some`" (section 2.7).

Arithmetic is `+`, `-`, `*`, and `sqrt` (correctly rounded), never fused or transcendental; building a quaternion from angles goes through
`libm`. A quaternion is re-normalised each step by a fixed formula.

### 2.3 Physics

A `BodyWorld` in the `physics` crate (one per level), stepped in the engine's physics phase, single-threaded, `enhanced-determinism` as in
the spike:

- A ship body is a dynamic Rapier body. Its collider is a compound of boxes made from the blocks' collision shapes
  (`Shape`), merged greedily per brick (a brick of solid blocks is a few boxes, not 32768). Colliders are rebuilt for a brick when it
  changes, never for the whole ship. **Mass** is the sum of `engine:density` of its blocks; the centre of mass and inertia come from the
  same sum (Rapier computes them from per-collider density, so each box carries the blocks' mean density).
- Terrain is collidable by ships through the terrain colliders the character already uses, built for the bricks near a ship's bounds only.
- Two ships collide with each other. Ships and characters collide: a character in the world frame is a kinematic capsule against a ship's
  colliders.
- Forces on a ship come from: the gravity field (section 2.9), plugin forces (`apply_force(grid, local point, vector)` in the pre-physics
  phase, requirement 2), and contacts. Buoyancy (the spike's model) is not in this phase: Cave has no fluids.
- *Held* ships (section 2.7) are kinematic: they do not move, and everything collides with them.
- A ship's pose is read back at the end of the physics phase and is the stream's truth for that tick.

Size and cost: the colliders are the cost risk (merged compound colliders that change often). The step is to measure it (32f): a ship of
10,000 blocks edited every tick. The no-hard-limit decision means the first version must degrade by budget, not by cap: when the rebuild
queue is long, brick colliders are rebuilt over several ticks (the ship is briefly slightly wrong), logged.

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

### 2.6 Aboard: the hard part

A character's pose is `(frame, local)`. A character *aboard* is simulated in its ship's frame, as decided on 2026-10-18: its position is
unchanged when the ship moves (exact carry), its collision world is the ship's blocks through a `CollisionSource` in local voxels (the same
trait the terrain uses, so the controller does not change), and gravity is the field's value in the ship's axes (2.9), a vector that changes
as the ship turns.

- **Boarding:** a world-frame character that is grounded for 3 ticks on a voxel of a ship (found by a short downward probe against nearby
  ships, in each ship's frame) becomes aboard: its pose is re-expressed in the ship's frame; its velocity becomes velocity relative to the
  ship's point.
- **Leaving:** an aboard character that is not grounded on the ship and is outside the ship's bounds expanded by 1 m for 3 ticks is moved to
  the world frame, inheriting the ship's velocity at that point (linear plus angular times the offset). The three-tick hysteresis on both
  ends stops flapping at an edge. Jumping off the deck is leaving; the character keeps its momentum, as a person leaving a train would.
- **What aboard cannot see:** terrain and other ships' hulls are not in its collision world, so a character aboard near the edge could not
  be stopped by a cliff next to the ship. The leave rule makes that edge the same place the world takes over, and the world frame then has
  the ship as a body to collide with. I expect bugs here, and the steps (32h) put a scenario test for every transition: boarding at rest,
  boarding while the ship moves, jumping off, being on a ship that turns, a ship that is destroyed under a player, and the same under
  replay.
- **No fictitious forces:** a ship that accelerates or spins does not push a person aboard sideways; only the direction of gravity changes
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
- **Held:** a ship with no pilot is *held* (kinematic: it stays where it is) by default, unless held is turned off. An unpiloted free ship
  falls under gravity, which for an airship with no thrust is a crash. Held is the safe default; free is a choice.
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
  component, and that cost is spread over ticks with a budget (the ship is whole until the check finishes). The part with the most mass keeps
  the id; the others get new ids from the counter. Each part continues with the velocity of its own centre of mass (linear plus angular
  times offset); entities aboard go to the part whose block they stand on, else the nearest by distance, then the event `GridSplit
  { from, parts }` is sent and recorded.
- **Merging and docking:** two grids merge only if `lattice_alignment` says they are on one lattice. A docking connector (engine part: a
  pair of ports, an attraction force, a snap distance) pulls two ships together; within the snap distance, the engine moves them to exact
  alignment and merges them: the smaller grid's bricks are re-keyed into the larger by the lattice transform (no re-mesh needed beyond the
  seams), the larger id lives on, the smaller becomes a tombstone, entities aboard are re-framed, `GridMerged { from, into, transform }` is sent.
  Welding (solid blocks flush with the ports that touch when merged) follows the 2026-10-18 decision. Docking is the biggest single piece
  after 32h; section 4 puts it after a pause for review.

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
  - *Blocked arrival:* if the saved pose now overlaps terrain or another ship (something was built there meanwhile), the engine searches
    deterministically for the nearest free pose within 32 m (upward first); if none is free the ship stays parked and the player is told
    why, and may try again later. This is a guess; it is question 6.
  - *Offline passengers:* a passenger who logged out aboard stays in the ship's frame as a hidden entity (players are hidden, not removed),
    so a parked ship keeps them, and they return into it.

### 2.9 Gravity as a field (built now, because the API is what is costly to change)

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
- Fictitious forces aboard (2.6).

## 4. Steps (each ends with fmt, clippy, tests on Windows and Linux, mutations, docs, then commit and push)

The phase is large; I propose a pause for the owner after 32h, before docking.

- **32a Frames** (2.2): the pure module, with property tests first (round trips, composition, exact 90-degree turns, rounding half toward
  negative infinity, alignment). No other crate changes. Safest piece, so first.
- **32b Grids on the server** (2.1): `GridStore`, the id counter, tombstones, `ServerWorld::grid` for allocated grids, `create_grid` /
  `remove_blocks_from_grid`, edit replication with a `GridId`; the frame-invariance harness of requirement 6 (the same rule in a static grid
  with local gravity and a turned, moving one, by turns of 90 degrees) and its first users: the existing Cave rules (light, fire) under it.
- **32c Protocol and replay** (2.4): version 6, the grid messages, interest, `GridId` in edits and in the client's actions, replay golden
  sessions regenerated.
- **32d Renderer and picking** (2.5): the rotation, GPU tests that a turned grid draws the same picture as the same blocks laid out in world
  space (pixel-compared with `settled`), picking tests in turned frames.
- **32e Bodies** (2.3): the `BodyWorld`, compound colliders per brick, mass, terrain contacts; deterministic digests on Windows and Linux;
  the cost measurement for 10,000 and 100,000 blocks edited each tick (reported, then the budget chosen).
- **32f Gravity field** (2.9): the service and its consumers (character, bodies, rules), `gravity_at(local)`, the setting for huge ships.
- **32g Aboard** (2.6): the character in a ship's frame, boarding and leaving, the transition scenario tests, replay equality.
- **32h Cave's ship commands and the first flight** (2.7 creation and held): `/ship create | push | hold | pilot`; an end-to-end test that a
  ship is made, flown, boarded, turned and left, recorded and replayed to the same picture. **Pause for the owner here.**
- **32i Saving and loading** (2.8): ship files, `engine:grids`, loading on demand, the missing-ship prompt, the consistent save, the
  vanish policy and the blocked arrival search, the simulation-distance setting with freeze and park.
- **32j Splitting** (2.7): the budgeted connectivity check, parts, velocities, entities re-framed, events and the stream.
- **32k Merging and docking** (2.7): connectors, attraction, snap, merge, welds, undock refusal; merge invariance and split invariance tests
  (requirement 14).

## 5. Risks

- **Aboard physics (32g)** is the one I would bet on being the source of most bugs. It is the reason for the scenario tests, and for
  stopping at 32h to check the feel before investing in saving and docking.
- **Collider cost (32e).** If compound colliders rebuilt per edit are too slow for large ships, ships become slowly-updating bodies; the
  measurement decides, and the spike's numbers are for a static ship.
- **Determinism of bodies in many orders.** Rapier's `enhanced-determinism` held in the spike for the cases it tried. Bodies are created,
  removed, split and merged here in many orders, so the golden digests must run bodies in a shuffled-creation test.
- **Protocol churn.** `GridId` in edits and actions touches client plugins and Cave; it is the right place to pay for it (nothing is released).
- **Scope.** This is larger than any earlier phase; the pause after 32h and the order (safe, pure pieces first) are the control.

## 6. Questions for the owner (my recommendation first)

1. **One phase with a pause after 32h, or two phases?** One phase, stopping after 32h to look at ships in the game before saving, splitting
   and docking are built (recommended). Or make 32i to 32k a Phase 32b; the numbering of later phases would not move (they are 33 and up).
2. **How do ships exist before the command chair?** Cave commands `/ship create`, `/ship push`, `/ship hold`, `/ship pilot`, built on an
   engine API (recommended, because the chair arrives in Phase 35 and the phase needs something to test with). The alternative is to build a
   thruster block now, which pulls in properties and rules for thrust and is more than this phase should hold.
3. **Unpiloted ships: held (frozen in place) or free (they fall)?** Held by default (recommended); free as a choice per ship. Without
   thrust, a free airship crashes, so held is the safe default; the vanish policy exists partly to avoid leaving ships floating.
4. **Gravity pull of huge ships:** build the field and a point-mass source now, off by default, with the threshold as a server setting
   (recommended, medium confidence), or only the `gravity_at(local)` interface now and no ship sources until coarse levels of detail exist.
   The interface change is the part that is costly later.
5. **Rename `gravity()` to `gravity_at(local position)` in the plugin context now** (recommended; no plugin outside the repository exists).
6. **A blocked arrival of a vanished ship:** search for the nearest free pose within 32 m, upward first; else stay parked and tell the
   player (recommended). Or: always place the ship and push whatever overlaps out of the way (simpler, can hurt), or ask the player.
7. **Ship simulation distance defaults:** 256 m to simulate and 1024 m to park, both server settings (recommended; I will adjust after the 32e
   measurements). Say if you want a different shape, such as one setting in bricks or a time delay before parking.
8. **Terrain level of detail first, or after ships?** After: ships are built drawing every visible brick, with a per-client budget, and a
   rendering phase later builds levels of detail for terrain and grids together (recommended). Or build terrain LOD first, which delays
   ships by a whole phase and gives ships distant drawing from the start.
9. **No sky light on ships in this phase** (section 3): accepted as a stated limit (recommended), or make sky exposure a requirement
   of this phase (it needs a world query across grids and so touches the rule boundary).
