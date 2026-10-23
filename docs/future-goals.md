# Future goals

Owner wishes that are not built and not scheduled (the scheduled work is in `plan.md`). Each says what it would need; where a
goal reaches back into the plan, the requirement is listed in `plan.md` under "Requirements pulled forward" because it is cheap
now and costly later.

## Earlier goals (owner, 2026-10-16)
- Blocks that let flood fill (light, and later other fills) through some sides but not others, like a slab or a stair.
  The block shape in the block definition now says which faces are open (`Shape::connections`, built in Phase 7) and
  is the collision shape; lighting and physics do not use it yet. (The grid stays cubic; a block fills part of its cell
  through its state; see `design-decisions.md`, "Decided".)
- Shaders written in GLSL or HLSL. (Not checked here: wgpu's shader translator, naga, reads WGSL, SPIR-V and GLSL, and I
  believe not HLSL directly, which would need compiling to SPIR-V first.) Cosmetic and client-side only, so they cannot
  affect simulation; they need a defined interface, validation and limits, and do not exist in the planned offline
  path-traced replay renderer.
- Fire spread in Cave (built differently: Phase 13, fires tick themselves) and other randomly ticked events such as coral
  growth, which need what fire did not: each tick the server samples positions in the
  active region with a deterministic random number from (seed, tick, position). Needs flammability and similar block
  properties, an active-region definition and a per-tick budget. (Coral needs water, which Cave does not have yet.)
- Motorized hinges and pistons to move constructions that are off the main grid: sub-grids with transforms, driven by
  Rapier revolute and prismatic joints with motors. The physics spike showed a motor-driven hinge identical on Windows and
  Linux. Needs sub-grids in the stream, a model matrix per grid in the renderer, and raycasts in a grid's own space.
- Multiple levels, first reached by commands and later through portals. A level id has to run through the protocol, the
  save format and the server, and there must be a command channel. Portals that show another level need rendering two
  worlds in one picture.

## Far-future goals (owner, 2026-10-17)

Three goals. Difficulty is the assistant's judgment, not a measurement: 1 = days; 2 = a week or two; 3 = a few weeks with known
techniques; 4 = a month or more, with hard parts we have not solved; 5 = a multi-month project, or one whose result is uncertain.
What each needs from the plugin API is listed in `plan.md` ("Requirements pulled forward"); the reasoning is here. Labels:
**[measured]** by a throwaway script on the dev machine, **[web]** from a source listed at the end, **[memory]** my recollection,
not checked.

### 1. An octopus that moves by reaching out and touching nearby walls with its tentacles (NPC or player character)

Overall difficulty: 4; 3 for a first, in-tree version once moving grids exist.

| Part | Difficulty | Note |
|---|---|---|
| Body as an articulated assembly: voxel segments (sub-grids) joined by joints | 4 | The "sub-grids with hinges" goal above with many joints to a creature; needs moving grids in the stream and the renderer (goal 2) |
| Reach-and-grab locomotion: find surface points in reach, anchor the tips there, pull the body, release and reach again; any gravity direction | 3 | A custom controller, not Rapier's character controller. The code is small; making it feel good and not jitter is the work |
| Player control: aim a reach, choose a tentacle | 2 | Needs an input mapping per control target (Phase 27) |
| NPC control: choose where to reach | 2 to 3 | A hand-written rule first; a brain emitting intents later (goal 3) |
| Determinism | 2 | The physics spike showed a motor-driven hinge identical on Windows and Linux; stepping is single-threaded |

What it needs from the API, and nothing more:
- a player is the controller of an entity (a *control target*) whose kind supplies a controller system and an input mapping, so
  walking, flying, a ship and an octopus are one mechanism, not four special cases;
- physics access for a plugin's controller in fixed tick phases (apply a force, create or remove an anchor or joint, read contacts):
  small and engine-owned, in the spirit of the earlier decision that mods configure engine-provided parts. The in-tree octopus should
  shape that API before anyone designs it for mods;
- a bulk query for the nearest solid surface within reach.

### 2. Airships: a moving grid of player-placed blocks with drills, turrets and headlights; players walk inside; a command chair gives a third-person view from which parts are clicked to interact; modded interactive blocks work aboard with no extra work

Overall difficulty: 5. It is the largest feature in the design, and the one that most shapes the plugin API.

| Part | Difficulty | Note |
|---|---|---|
| Rules and events addressed to a grid in its own coordinates, not to the world | 2 now, 4 if done late | A type change while only root grids exist (`GridId`, `GridView`; plan.md); a rewrite of every rule and the save format afterwards |
| Moving grids in the stream and the renderer: a transform per grid per tick, a matrix per grid, rays in a grid's own space | 3 to 4 | Already named in the goals above; replay stays valid because the stream carries the transforms |
| The ship as a Rapier body: colliders rebuilt from blocks as they change, mass from block density, buoyancy and thrust | 3 to 4 | Determinism of Rapier is pinned for the cases the spike tried; large merged colliders that change often are the cost risk |
| Connectivity: a ship that is cut in two becomes two grids; a built ship breaks off the terrain and can merge back | 3 | A flood fill on edits; entities aboard must be reassigned to the grid they end up in |
| Players and entities moving inside a moving grid: simulated in the grid's frame, carried exactly by it; boarding and leaving; velocity inherited on leaving | 5 | The sharpest edge: two coordinate frames, a hand-over between them, and every mod's entity (a minecart) needing the same. I expect most of the bugs to be here; not verified against other games |
| Command chair, third-person view, click a part for a menu or an action | 3 | Needs a control target, a view mode, picking that returns a grid and a local position, and the UI of Phase 30 |
| Mining drill and automated turrets | 3 each | Need queries in world space across all grids, damage and projectile entities, and power or ammunition, which no plugin has yet |
| Directional headlights | 4 | Flood-fill light cannot follow a thing that moves every tick (a lamp's light takes about 64 ticks to spread); a moving beam is a light the renderer draws (dynamic lights), and so is the light a ship casts on the ground it passes over |
| Modded blocks work aboard automatically | 3 | Mostly a property of the API (below), kept true by a test |

**Why "works aboard automatically" can be promised, and what it takes.** In Minecraft a block's code and every mod address the world
in world coordinates, so Valkyrien Skies, the mod that adds moving ships, is a library that needs an add-on for each large mod (Create
works through the VS: Clockwork add-on), and its pages list conflicts with mods that change collisions, chunk loading or physics
[web]. Compatibility there is added afterwards, mod by mod. Here it can be a property of the API the mods are written against, in
three parts:
1. A rule or handler is given a grid and local coordinates and has *no way* to read a neighbour in world coordinates. A wire, piston,
   door or furnace written against that works in any grid, static or moving, because a grid is all it ever sees.
2. Facts that are about the world are given in the grid's own terms: gravity as a local direction, the world position of a local
   voxel, sky exposure of a local voxel.
3. A frame-invariance test (the same rule, the same scene, in a static grid and in a translated, turned and moving one, identical
   local results) is required of every plugin rule, so the property is tested rather than hoped for.
Space Engineers is built on grids natively [memory], the closest precedent for "a block belongs to a grid"; I did not find how its
mods or gravity behave aboard rotating ships.

**What cannot be automatic, and the default:**
- *World effects*: weather, wind, the day, sky light, an explosion's radius. They come from the services above or are the plugin's job.
- *Two grids side by side* (a ship docked at a pier): a rule never sees across grids, so a wire or piston does not cross. Default:
  nothing crosses; the docking connector (decided 2026-10-18, below) is the way, and what crosses it is an open question.
- *A minecart or any entity leaving a grid* (a rail that ends at the ship's edge): the entity is handed to the frame it ends up in; a
  rail that connects across the gap is a connector case.
- *Placement that only makes sense on the ground* ("needs soil", "needs sunlight"): the single flag, `engine:placeable_on_moving_grids`,
  default true, as asked.
- *Cost*: a ship with many active blocks adds work every tick; budgets are per grid, as for every rule.

**Decided by the owner, 2026-10-18** (recorded in `design-decisions.md`): gravity aboard is the world's, not a pull toward the floor;
ships rotate fully (the yaw-only simplification I suggested is not taken); docking connectors have a configurable attraction force, as in
Kerbal Space Program; there is no hard size limit, and ships may someday be as large as small moons, with levels of detail for viewing
the whole ship and for collisions; a ship is saved in flight as a grid in a file of its own.

**What those decisions cost, in my judgment:**
- *World gravity aboard* makes the interior of a rolled ship a place where things fall toward a wall. It is consistent and needs no special
  case, but it means a rule's `gravity()` is a local direction that changes every tick, rules that cache "down" are wrong, and a player
  in the ship's frame feels a rotating gravity vector. Ships with interiors you can live in will usually be kept level by their pilots.
- *Full rotation* removes the cheap simplification; the frame-invariance test therefore turns the grid (by multiples of 90 degrees) and
  gives the static grid the matching local gravity.
- *Docking connectors* are an engine-provided part (a pair of ports, an attraction force and a snap distance), configured by mods.
  Docking is a *merge* of two grids into one (decided 2026-10-18), so nothing needs to cross a boundary: after it there is one grid and
  every rule sees one grid, as if it had been built whole. Three things follow. (1) Two grids can only merge on one lattice: their
  relative turn must be a multiple of 90 degrees and their offset a whole number of voxels, so the connector's attraction must pull the
  ships into that alignment (a snap) before the merge happens. (2) Merging and splitting are operations on grids that every plugin's
  state must survive: persistent per-grid state is keyed by voxels or is a cache that can be rebuilt (the multiscale light's cells are
  laid from a grid's origin, so a merge at an odd offset recomputes the cells it touches). (3) Undocking is a split: the connector pair
  no longer joins the two halves, and if any other blocks still do (a weld) the undock is refused until they are broken.
- *No size limit* is a promise about the API (nothing assumes a small grid; plan.md, requirement 11) and about work (budgets, not caps),
  not about the first version. The parts that grow with size: collision (coarse colliders and aggregated mass for the whole ship, fine
  colliders only near a contact), drawing (levels of detail per grid), streaming (interest per grid in its own coordinates), connectivity
  after an edit (incremental or hierarchical, not a flood fill of the whole ship), and precision (each grid works relative to its own
  origin). A moon-sized grid is a long way off; the first ships will be small.
- *A ship in its own file* means a `GridId` that is stable within a world (requirement 10) and a file format for a grid (requirement 12),
  and that a save writes several files that must agree.
- *Portable ship files* (decided): a ship file has two kinds of content. The ship itself (blocks, block data and the registry and channel
  tables that make it readable anywhere) and, only when saved in flight, its flight state (pose, velocities, pending rule work). Loaded
  into another world, or a second time into the same one, the flight state is dropped, the ship gets a fresh `GridId`, and a rule's
  pending work is rebuilt by waking the voxels. Entities aboard are not in the ship file: they belong to the world. Items inside a
  chest travel with the ship, as part of its block data, and ids from plugins the new world lacks are preserved unread.
- *The missing-ship prompt* (decided): a player's saved pose aboard a ship is a frame and a local pose; if the ship's file is missing
  when they connect, the server holds the player and asks them (spawn at the world spawn, spawn in midair, or quit to title). For
  midair the world file keeps, for every entity whose frame is a grid, the world-space pose it had at the last save. This needs a
  small prompt facility (a choice sent to one client, its answer returned, the entity held meanwhile).

**Still open:** how forgiving docking is (how far from alignment the attraction can pull, and whether it turns the ship as well as moves it).
(Whether a ship's own mass is a source of gravity was answered on 2026-10-20: yes, for sufficiently huge ones; see "Ship goals" below.)

### 3. NPCs powered by local neural networks that can learn

Overall difficulty: engineering 3; design and research 5. The code to run a network is small; making learning that players notice,
that stays stable, that can be tested, and that fits in a 50 ms tick is the risk.

Cost of running a network **[measured]** (a plain f32 multilayer perceptron in scalar Rust, ReLU, one thread, `-O` for baseline x86-64,
this machine; the answer was bit-identical on every run):

| Network | Multiply-adds | Per decision |
|---|---|---|
| 64-128-128-16 | 26 624 | 9.6 microseconds |
| 729-256-128-16 (a 9 by 9 by 9 window of the world as input) | 221 440 | 113.5 microseconds |
| 4096-512-256-32 | 2 236 416 | 1 227 microseconds |

One hundred NPCs deciding five times a second would cost about 0.5 ms a second with the first network, 57 ms a second (about 6% of one
core) with the second, and 614 ms a second (about 61% of a core) with the third. Learning costs several times a forward pass for each
training sample, so it must run off the tick, in the background.

| Part | Difficulty | Note |
|---|---|---|
| Running a network deterministically | 2 | Scalar f32 in a fixed order, no fast-math and no transcendental functions (ReLU, or `libm`), is the same class of determinism as the rest of the simulation. I believe a library that picks SIMD paths at run time (a BLAS, an ONNX runtime, some Rust ML crates) can give different bits on different CPUs [memory]; so, own code, or a library with its kernels pinned, checked by test |
| Sensors and actions | 2 to 3 | A bulk read of a window of the world through the block properties; nearby entities; the NPC's own state. Actions are *intents* (move toward, use item) validated by the same rules a player's input passes |
| Learning | 4 to 5 | Which algorithm (imitating the player, reinforcement with designed rewards, evolution across generations), stability, how long until a player sees it, compute, and the size of what is saved |
| Saving, replay and testing | 3 | Weights are saved as opaque plugin data (unknown data is preserved by rule). Replay is unaffected, because a client replays the stream, not the server's thinking. Testing needs seeded evaluations and an intent log, because a learner is hard to test by its output |

**Precedent.** Shipped games have used small networks with designer-chosen inputs and rewards: Creatures (1996) used networks and
evolution per creature, and Black & White's creature learned from player feedback [web: the sources disagree on whether that was
perceptrons or reinforcement learning]; a rally game trained its drivers offline [web]. A forum post notes that in-game learning is risky
because it is hard to test and tune [web]. Free-form learning of the kind people imagine from the phrase rarely ships.

**Design stance.**
- *The brain emits intents; the body executes them.* A brain, a hand-written state machine and a human player all go through one input
  path. The `Mind` determinism class (plan.md) says a brain may differ between machines; the deterministic rules that execute its intents
  do not.
- *Brains are data where possible:* a network in a small fixed format (a fixed set of operations) plus a manifest of sensors and
  actions, shipped in the content package, so a mod can ship a brain with no code and no sandbox, and the learning rules are chosen from
  an engine-provided list: the same "configure engine-provided parts" principle as for physics.
- *Learning runs in the background, and its result is applied at a tick boundary and logged*, so a saved world carries on and a server
  can be reproduced from its intent log. No API for this until learning is built.

### What the three goals change in the plan

Needed now, because it breaks mods later and costs little while there are no worlds, mods or recordings (all in `plan.md`,
"Requirements pulled forward"): rules that see a grid and never the world; named tick phases; a determinism class on systems; a
`BlockUsed` event; engine-reserved properties; a frame-invariance test; view modes and picking in grid terms; entity poses that carry a
frame; control targets; block data; a brain slot; inventories; and counting the API's size in verbs, not kinds of content.
Not changed: the content package, the schema, patches, the palette. Ordering: the loader ABI encodes the coordinate model, so it
should not be frozen before grids, frames and entities exist. Phases 32 and 34 to 37 in the build plan are these goals as work (32 before the loaders, which are 33).

Sources: [Valkyrien Skies FAQs](https://blog.curseforge.com/valkyrien-skies-mod-faqs/), [Valkyrien Skies on ModDex](https://moddex.gg/mod/valkyrien-skies),
[The AI of Creatures](https://www.alanzucconi.com/2020/07/27/the-ai-of-creatures/), [Agents that learn (forum)](https://www.gamedev.net/forums/topic/344721-agents-that-learn/).

## Ship goals (owner, 2026-10-20)

Five wishes that extend goal 2. None is scheduled as work beyond what `moving-grids-spec.md` (Phase 32) says it reserves for them; the
cheap-now requirements are listed in `plan.md` under "Requirements pulled forward" (items 17 to 21). Each is stated as the owner put it,
then what it needs and what I found in the code.

1. **Ships are drawn with levels of detail when far away, just like terrain.**
   *Found:* terrain has none yet. `plan.md` (Open, "Rendering at scale") says the view at radius 12 "needs LOD (coarser bricks far away)";
   the renderer draws full bricks, one draw call each. So this goal is really "terrain LOD, built to serve grids too". *Needs:* a mesh is
   addressed by (grid, brick, level) from the first moving grid, so that nothing assumes level 0; coarse levels come from the grid's own
   octree (`region_octree`) in its own coordinates; the renderer draws any grid's bricks under that grid's matrix. Phase 32 reserves the
   address and draws level 0; the coarse levels are built once, for terrain and grids together, in a later rendering phase.
2. **Sufficiently huge ships have their own gravitational pull.**
   *Found:* gravity today is one constant, `CharacterSettings::gravity` (32.0), pointing down; `design-decisions.md` already says "entity
   gravity direction is server-authoritative replicated state, computed from coarse LOD mass", but nothing computes it. Plan item 1 of the
   pulled-forward requirements says a rule asks `gravity()` with no position; a field that varies in space needs a position, so it should be
   `gravity_at(local position)` before any plugin uses the version without. *Needs:* gravity as a field with sources (the world's own,
   and any grid whose aggregate mass is above a configurable threshold, using the mass the physics already needs for its body), summed at a
   point; every consumer (character, bodies, rules, fluids) asks the field. *Cost:* a handful of big sources, so it is cheap; the
   interesting part is what a player aboard a huge ship feels (the sum of the world's pull and the ship's) and that two big ships pull
   each other (a physics decision for later).
3. **Fluids flow inside a moving grid; at its edge they may re-enter the world; the same for current on a channel.**
   *Found:* there are no fluids yet (Cave has no water; Sculpt is where they are planned). Fluid rules are lattice rules per grid, so inside
   a ship they already work in the ship's own axes, with the field's gravity expressed in those axes (that is requirement 1 and the
   frame-invariance test). The new thing is crossing a boundary, which a rule must never see directly (a rule has a `GridView`, one grid).
   *Needs, as I would plan it:* a rule can hand the engine an *outflow* (grid, local voxel, direction, channel, amount); the engine
   converts the voxel with the grids' current transforms (`local_to_other`, nearest voxel, rounding that is deterministic) and delivers an
   *inflow* to the receiving grid's rules as a message at the next tick boundary, in a fixed order, with a conservation rule (what is
   delivered is subtracted at the source, and a refused inflow is returned, never created or lost). Water that falls out of the bottom of a
   ship has two policies, per fluid and per server: stop at the edge (the owner's present lean) or continue into the nearest world voxel.
   Electric current and a redstone-like signal use the same mailbox with a different channel and no amount. The mailbox is the one
   cross-grid door; docking (a merge) remains the way grids become one lattice.
4. **A ship may disappear when its pilot logs out and reappear when they log back in, like a Minecraft horse with its rider.**
   *Depends on:* server setting and a setting on the command chair (block data). *Needs:* grids that can be unloaded and loaded while the
   server runs (a ship file, already required), a pilot relation (the chair's control target), and rules for what is aboard when the pilot
   leaves. My reading of the horse: it is saved with the player and put back at the player's position. See `moving-grids-spec.md`
   section 2.8 for the policy and the open questions (passengers, a blocked arrival).
5. **Simulation distance for ships is a server setting.**
   *Found:* "frozen beyond simulation distance" is a stated principle (`design-decisions.md`) but I did not find a simulation distance in the
   code: the server's limits are `maximum_watched_bricks` (what one client may watch) and the view radius; rules run on their active sets.
   *Needs:* one server setting for how far from any player a ship is simulated (physics and rules); a ship farther away is frozen where it
   is, or parked (unloaded and saved) after a delay. Defined in `moving-grids-spec.md` section 2.7.
6. **Feeling the force of acceleration aboard a ship is a server option (owner, 2026-10-20).**
   *Found:* the Phase 32 design has no fictitious forces: a ship that accelerates, turns or spins does not push a person aboard, only the
   direction of gravity changes relative to them (`moving-grids-spec.md` 2.6). *Needs:* a server setting (off or on, possibly a strength) that adds
   the inertial terms (minus the ship's linear acceleration, the centrifugal and Euler terms of its rotation) to the force a character
   aboard feels, in the ship's frame. Characters aboard are already simulated in that frame, so it is one added vector per tick; the work is
   in making it feel right (a hard turn throwing people against a wall, comfort settings) and in rules aboard: fluids would then slosh
   if they were given the same vector, which is a separate choice.
7. **Fossilization: another way to protect a ship whose pilot logs out (owner, 2026-10-20).**
   Beside `stay` and `vanish`, a ship may be *fossilized*: depending on server settings it becomes indestructible and immobile by any
   means except commands. `/ship fossilize` and `/ship defossilize` are subcommands of `/ship`, available only to operators, even to players in creative. *Needs:*
   an operator permission on commands (none exists yet; I found no operator or permission concept in the code), a ship flag the physics treats as fixed and the
   edit path refuses to change (including explosions and fire), a server setting that decides whether logout may fossilize and who may
   undo it, and a visible marker. Fits `on_pilot_logout` as a third value.

8. **A `GridSplit` message in the stream (owner, 2026-10-22), and, optionally, the parts' bricks derived by the client.**
   *Found:* splitting (32l) sends no message of its own: a client sees the ship leave (`GridsDespawned`) and the parts arrive (`GridsSpawned` and the `BrickContents` of every brick of
   every part, since nothing generates a ship's bricks); the lineage is in the tombstones, which are saved but not sent. *Needs:* a message `GridSplit { from, parts }` (the spec's wording
   in `moving-grids-spec.md` 2.7), so that a client can keep what refers to the old ship (a camera, a picked block, a drawn selection) on the part it went to, and so that a
   recording says it; a protocol version, and the golden sessions that record the stream are regenerated.
   *Optional part 2:* the client works out the bricks of the parts from the bricks of the whole instead of being sent them again. A part keeps the pose and the coordinates of the
   ship, so a part's brick is the whole's brick with the voxels of the other parts set to their defaults: the message would carry, for each part, the voxels it keeps (the
   server already has them as `SplitPart`; run-length coded they are small beside the bricks), and the client would cut its copy of the ship's bricks accordingly and keep them under the
   new ids (re-meshing only the bricks that changed). *Needs, beyond the message:* the client must hold the whole's bricks at that moment (a ship it did not watch gets the
   parts in full, as now); the voxels in no part (air with light in it) are dropped on both sides the same way, as `split_grid` drops them; and a test that the client's derived
   bricks equal the bricks the server would have sent, bit for bit, for random cuts.


9. **Ship mass calculation from materials, and metric units throughout (owner, 2026-10-22): a conversation to have soon.**
   *Today:* every block that is not air weighs `engine:density` = 1000 kilograms a voxel (the default of the property), scaled by the share of the cell its shape fills; a voxel is one
   metre, so this is one metric ton per cubic metre for every material, as a placeholder (owner, 2026-10-22). Nothing but that number distinguishes stone, wood and a lamp.
   *To talk about:* densities of the real materials (stone, dirt, wood, metal, glass, water), whether a block's mass belongs to the block definition or to a material that definitions name
   (so a data pack changes one number, not every block), hollow and partly filled blocks, the mass of what is aboard (players, items, cargo), and how mass should show to the player (a
   display, a scale, a command). *Decided with it:* all the game's measurements are metric wherever they exist in our world (metres, kilograms, seconds, newtons, and so on; a voxel is a
   metre; gravity is 5 metres a second a second, the planet not being the Earth); the code and the docs say "voxels a second" for what is a metre a second, and a rule that needs a unit says which.


10. **More ways a placed block is turned (follows the owner's answer of 2026-10-22).** Pistons (one front face) and logs (a line of symmetry) are placed as in Minecraft. Not built: slabs
    and stairs placed by the half of the face the player clicks and the way they look (they are still chosen by the shape key), a tool that rotates a block already placed, and the
    rest of Minecraft's families (a block that faces the way the player faces, not toward them, like a furnace; a block that turns by the half of a face clicked, like a hopper).
    The orientation channel and the placement properties are in place for any of these; each is a rule in `games/voxunami_cave/src/placement.rs` and a property of the block.

## Levels (owner, 2026-10-20)

- **A superflat level.** A level of flat ground in layers (as Minecraft's superflat world type: for example bedrock, stone, dirt, grass), for building,
  for testing, and for ships, where the level's terrain is the same everywhere and so costs nothing to simulate or send.
  *Found:* worlds already have several levels, each with its own generator versions (`design-decisions.md`), and a flat generator exists in the test
  fixtures (`base_terrain:flat_v1` in `world_storage::test_support`), not as game content. *Needs:* a generator in the `engine::terrain` family that takes
  its layers as data (block, thickness; the bottom layer going down for ever, nothing above the top), so that a data pack or a command can define a
  level with it; a way to make the level (`add_level` cannot be called while a server runs, `plan.md` Open, so this wants levels created at world
  creation or through the planned level commands); a released version of the generator is a file format and is never edited (a hard rule), so its layer
  list is part of what the world records for the level. Cave could offer it as a world type at creation (`serve cave --level superflat`).
