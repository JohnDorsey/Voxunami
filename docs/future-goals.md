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

11. **Later pieces of the level-of-detail work (owner, 2026-10-23; `lod-spec.md`).** Not in Phase 38:
    - *Optional upgrade of the dominant block of a coarse cell:* a dominant exposed block **per direction** (six ids for a mixed cell, one for a uniform one), so
      that snow shows from above and stone from the side. Phase 38 starts with one block per cell chosen by the faces that touch air; this is the upgrade if that
      looks wrong. **Optional.**
    - *Weight the exposed faces of a coarse cell by the light in front of them* (owner, 2026-10-23; option (a) of `lod-spec.md` section 7): a face that looks out at dark, enclosed air counts for little in choosing the
      dominant block, so the walls of caves stop outvoting the grass on the top of a hill in the cells made from bricks (edited regions, ships). Needs sky light, which Cave does not have yet.
    - Planets: (now planned as Phase 39 in `plan.md`, `lod-spec.md` 3.8: a ship with terrain generators sent to the client) a ground that is a ship, a sphere generator with analytic coarse sampling, atmosphere and
      fog for planet distances. Planets are rough spheres of cubes in ordinary space (no curved grids), real-planet size (10^8 m and more).
    - Stars as voxels of 2^32 m and more that break into smaller ones on approach (needs emissive blocks).
    - A telescope item that raises the detail where the player looks (the rule already refines with the field of view; the item and the download cost are new).
    - Fading between detail levels (cracks are accepted at first).
    - A server-side coarse mass for the direction of gravity (Phase 38 is drawing only).
    - GPU-driven culling, if Phase 38's measurement says the CPU is the limit.
    - Textures near the player, with distant detail always untextured (Phase 38 builds only the distance setting).

## Levels (owner, 2026-10-20)

- **A superflat level.** A level of flat ground in layers (as Minecraft's superflat world type: for example bedrock, stone, dirt, grass), for building,
  for testing, and for ships, where the level's terrain is the same everywhere and so costs nothing to simulate or send.
  *Found:* worlds already have several levels, each with its own generator versions (`design-decisions.md`), and a flat generator exists in the test
  fixtures (`base_terrain:flat_v1` in `world_storage::test_support`), not as game content. *Needs:* a generator in the `engine::terrain` family that takes
  its layers as data (block, thickness; the bottom layer going down for ever, nothing above the top), so that a data pack or a command can define a
  level with it; a way to make the level (`add_level` cannot be called while a server runs, `plan.md` Open, so this wants levels created at world
  creation or through the planned level commands); a released version of the generator is a file format and is never edited (a hard rule), so its layer
  list is part of what the world records for the level. Cave could offer it as a world type at creation (`serve cave --level superflat`).

## Space exploration (owner, 2026-10-25)

- **Voxunami Cave is to become a space-exploration game, like Kerbal Space Program and KSA.** Consequences the owner has named so far:
  - **Ships on rails.** A ship that is too far from every player, or any ship while a time warp is in effect, is put on rails: its motion is the orbit (or the course) it is on, worked out from the time and not stepped by the bodies, as for the planets, which are on rails too (`planet-spec.md`, the owner's stance of 2026-10-25). *Needs:* a kinematic body or no body for a ship on rails; the conversion both ways (a ship that comes near
    a player leaves its rails with the velocity the rails give it, and one that goes far, or is warped, takes a course from its velocity and the pulls of the planets); a game clock that can run faster than the ticks (time warp) and that simulation and cosmetics read (the hard rule against the wall clock already requires the game tick); what a ship on rails does when it meets a planet's ground (it cannot, on a course that is checked, or it lands). Until then ships
    that are far from every player simply stop (`ShipSettings::simulation_distance`, 256 voxels by default, shown and changed by `/ship distance`).
  - *Probably also:* orbits and maneuvers, patched conics or a sum of pulls, a solar system of planets and moons on rails, atmospheres and a sky that goes black with height, and ships that are far larger than a player's. None of these is asked for yet; they are listed so that the work in between (frames for each place, planets as ships) is chosen to leave room for them.

## A universe of 93 billion light years (owner, 2026-10-27)

- **Wishes.** A player flies continuously from anywhere to anywhere, with no loading screens or warp commands. Continuous generated structures can span the whole universe *in the same block
  grid*. The owner leans to nested frames (B) **and** wider coordinates (A: `i128` bricks), and wants scaled levels (C) considered because of a possible technology that makes ships grow or shrink.
- **Sizes (arithmetic).** 93 billion light years is 8.8e26 m, about 2^89.5 voxels at 1 m. A brick is 32 voxels, so the brick needs 85 bits: an `i128` brick reaches about 2^132 voxels, trillions of times
  that, or the same universe with voxels of picometres. The range was +-2^60 voxels (about +-122 light years) until 2026-10-27; it is now +-2^120 voxels, with body files limited to 1e15 voxels.
- **Measured (2026-10-27, scripts and a copy of the repository; `build-log.md`, "Rails in exact arithmetic").** (1) A sum of orbits through absolute `f64` positions breaks long before the integers do: at a star
  2.5e20 m out the moon was a median 7.6 km from where it should be round its planet. Done: rails now add the offset to the parent's `WorldPosition` (exact in bricks), and the turn is worked out from whole ticks.
  (2) The topmost rail of a galaxy had an `f64` radius: the star's place moved in 32 km steps. Done (the owner asked for accurate radii): an orbit is worked out in 128-bit fixed point, exact to 2^-33 voxel
  at radii up to the size of the universe (`exact_circle.rs`; `f128` is unstable on the stable toolchain and `libm` has no `f128` functions). A ship at the size of the universe is now placed exactly (a test puts one 6 x 10^26 voxels out, at brick 10^25, to the last bit of its `f32`). (3) `i64` to `i128` was not only
  mechanical: the types widened with the compiler's help, and the arithmetic that depended on the width and the formats were decisions. Done 2026-10-27 (`design-decisions.md`, "Coordinates of 128 bits"; `build-log.md`).
- **Where C still earns its place with `i128` coordinates.** (a) *Levels of the same grid* (coarse summaries: "this cube of 2^40 bricks is empty or all one material"; built to level 116, 2026-10-27, see `design-decisions.md` "Coarse levels go to the root"): the real prerequisite for structures that span
  the universe, for the view of the far, for raycasts and flights at many times the speed of light; the generator should be able to say it (empty, solid, mixed) for any cube, and `region_octree` is already a hierarchy.
  (b) *The renderer's depth range* (bands of distance, camera-relative): **measured 2026-10-28, not needed at 10^25 voxels** on four adapters (reversed infinite depth and camera-relative positions already do it); the fog's distance is computed from a scaled vector as a precaution. (c) *A scale per grid* (a ship that grows or shrinks; a universe in a bottle): a number on a grid, not a coordinate system. Docking a grid of another
  scale into a planet is lossless only for scales that differ by a power of two and a grid that has levels finer and coarser than its voxel; that is the design question to settle before the technology.
- **Other games (from memory, medium confidence; only the floating origin, the physics range and the sphere of influence were checked against sources).** Kerbal Space Program: a floating origin and Krakensbane keep the
  active vessel near the origin; only vessels within about 2.2 km are physically loaded and the rest are on rails; patched conics with a sphere of influence per body; a solar system at a tenth of real size; no
  interstellar space. The sacrifices: no Lagrange points, jumps at a sphere's edge, nothing simulated off screen, stations larger than the physics range impossible, time warp that is hard for many players. Our rails (a
  body is a function of the clock) and ships that feel every nearby body avoid the first two (to be checked: that gravity sums over all pulling bodies); the others are ours to decide.
- **Open.** Who carries a free ship in a star's frame (only planets carry things today); *(done 2026-10-27: the great rivers, a generator that makes something at every level to 116; see `great-rivers-spec.md` for what it does not do: a thin river vanishes from the nodes, and Cave does not draw a level beyond its bricks)*; the renderer's depth range at these distances; a delta form of the coordinates on the wire if traffic needs it (option (e) below); noise wavelengths above 2^56; time warp and ships on rails. (`i128` coordinates: done.)
- **The wire format for `i128` coordinates (decided and built 2026-10-27: option (b), a length-prefixed integer; `binary_codec` `write_wide_signed` / `read_wide_signed`, used by every message and section that holds a coordinate).** The owner's reason: a later 256- or
  512-bit coordinate (if shrinking ships becomes a large part of play) then needs no new format, only a reader that accepts a longer length (the length byte allows up to 255 bytes; the `i128` reader refuses over 16).
  The cost, measured: a coordinate with a magnitude under 64 takes 2 bytes, not the 1 of a varint (and the largest `i128` takes 17). Options as they were weighed: Today a coordinate is a signed varint of an `i64` (zigzag, 7 bits a byte: 1 byte near the origin, 9 or 10 at the ends). (a) *The same varint, widened*:
  LEB128 of a zigzag `i128`, up to 19 bytes; small near the origin, so nothing changes for a game that stays in a solar system; a decoder must refuse an encoding that is not the shortest, or that has more than
  19 bytes or bits past the 128th, so that a value has one encoding (the saves and the replays compare bytes). (b) *A length-prefixed integer* (a byte for the number of bytes, then those bytes): the same size
  as (a) for small values, simpler to decode without a loop, a canonical form to check. (c) *A prefix varint* (the length in the first byte's leading ones, as in the Matroska and QUIC integers): fewer branches and
  the length known from the first byte, but 128-bit values need a nine-byte-plus escape of our own. (d) *Fixed 16 bytes*: simple, 8 to 16 times larger near the origin, which is where almost all the traffic is.
  (e) *Delta from the last*: a coordinate sent as a difference from the previous (or from the viewer's brick), small whatever the distance from the origin, which also fits a stream of bricks near one another; the largest
  saving, but the decoder carries state and a lost message would matter on an unreliable transport. My lean is (a) with the canonical-form rules, and (e) later where traffic needs it.

## The great rivers, shadows and teleport (owner, 2026-10-27)

- **The great rivers** (built as a first prototype, `great-rivers-spec.md`): *wanted next:* rivers that **branch and merge** (a path may start at a knot of another; `GreatRivers` holds a list of paths for this), and a way for thin structures to stay drawn in the coarse levels (a cell that holds a line).
- **The sun is built (2026-10-28, `sun-spec.md`):** the sun as a light of the world's clock, the terminator of each planet as a sphere, a soft twilight, the sky going dark, `.time`. **Shadow maps are built (the same day, `shadow-spec.md`):** cascaded, camera-relative, kept while right, right at every scale. **The sky's cover is built (the same day, `shadow-spec.md` section 8):** a cave under a hill is black by day and a hill's lee keeps a quarter of the sky. **The sun is a star (the same day, `sun-spec.md` section 8):** a disc and a glow, the stars, a night light and a sunset colour. *Next, in my order:* the sky's gradient (a zenith to a horizon colour, scattering), a moon with phases and its light, a sun and a moon's phases from the planets' rails (an eclipse already works for the balls), `sun` in a pack's level data, and, for the cost of the maps, a pool of bricks in one buffer drawn by indirect calls (`shadow-spec.md`, section 4).
- **Huge mountains in the overworld** (the owner, 2026-10-28): "thousands or millions of blocks tall", someday. *A requirement on what is built, not a task:* the shadow maps must not break at that scale, and the test of it is in the suite (`shadow-spec.md`, section 3: a block four million voxels high, and a mountain half a million voxels high shading voxels at its foot, to within a voxel). *What it needs of the rest, not done:* the terrain generator (a version of it, released versions being file formats), and the game to say how far the overworld is drawn (`GameDefinition::level_detail`) so that the mountains are seen from far; the shadows cover what is drawn, to 59 million voxels.
- **Shadows from shaders in place of the flooding of sky light** (the owner, "for most sky light"; to be discussed before it is built; the first step is built, above). Today the renderer has a direction to the sun and a fixed shading of faces by it (`voxel.wgsl`), Cave has no sky light (the ambient light is 0), and block light (lamps) is flooded and stays. The order I recommended: the renderer's depth range first (reverse-Z and camera-relative positions), then the sun as a light of the game's clock with the terminator of each planet worked out as a sphere, then cascaded shadow maps for what is near, then a cheap ambient term for caves.
- **The teleport command for several planets** (the owner, 2026-10-27): *built the same day*, `places-spec.md`: `/tp PLACE [LATITUDE LONGITUDE | X Y Z]`, namespaced names, the ground nearest to the player. *Not done:* a `facing` option (the server cannot yet set the way a player looks: `PlayerPlaced` carries a position only), going to any knot of a river by number, and places registered by plugins and packs (the places are Cave's, from its bodies and its rivers).

## Unscheduled (owner, 2026-10-27)

- **Shaders** (owner, 2026-11-09). (1) Players can **swap the shader**, as with Iris for Minecraft; post-processing passes are left out for now. (2) The **default shader is robust, fast and free of bugs**: in particular **no gap at the base of a shadow** (the lit strip where any block meets the ground, and the same leak into caves), whatever redesign it takes, at **60 fps on integrated graphics**; no preference about penumbras yet. *Done:* the strip measured and its cause found, and a prototype that removes it for whole cubes at about 0.08 ms a cell on the integrated GPU (`contact-shadow-spike.md`, code in commits `3f6ffb1` and `e166faf` of the branch `worktree-shaders`, out of the tree because its binding made a shadow test flake on the integrated GPU); a design for the real thing, in phases (`contact-trace-spec.md`); **phase 0** (the volume would be about 20 MB at the default radius, 42 MB at the largest) and **phase 1** (the meshers say which voxels of each brick block the sun, and a CPU reference of the walk) and **phase 2** (the renderer: windows, pool, the shader walk as a variant of its own, tested against the reference on every adapter, 34 of 36 mutants killed; **the lit strip is gone in real Cave**). *Needs:* (the cost was over the budget, the walk adding 3.8 ms to 19.1 in real Cave on the integrated GPU, and is now paid for) Measured 2026-11-09 (section 11 of the spec): a depth pre-pass is a loss (+9 to +11 ms) and painting the contact into the map is free and partial; **drawing only the faces of a brick that can look at the camera takes 3.6 to 5.9 ms off the frame in three views with the pictures unchanged, more than the walk costs: built 2026-11-09 (section 12 of the spec), and the frame with the walk is now 17.0 ms, 59 fps**; the owner said to hold off on a setting. **Partial blocks (slabs, stairs, posts) and ships (a ship's window, and a face walking the windows of the other grids) are done** (sections 13 and 14 of the spec; the four windows go to the nearest grids, section 15); the boxes of entities (section 16); (leaves and glass were built and set aside: the map has no strip at their feet, section 17); then the far levels; and a pack interface where the engine owns visibility and a pack owns how a surface is lit (**built 2026-11-10: `shader-pack-spec.md`, `shader-pack-format.md`; hooks for the sky, the fog, vertices and time are next if packs want them**).
- **Cheaper entity boxes in the sun's ray** (2026-11-10). The test of avatars' and items' boxes costs +0.6 ms on the integrated GPU in real Cave with three other players (10.51 ms against 9.91), where the scene's 0.04 ms a box predicted 0.13: most of it is the shader variant itself, not the loop (`contact-trace-spec.md`, section 16). Options, none tried: (1) fewer registers in `entity_boxes_block` (keep the box test in `f32` vectors, test the nearest box first and stop, cull boxes by distance from the place before the slab test); (2) one variant for entities and other grids together, so that a scene with both pays for one; (3) test the boxes only for faces whose place is within the walk's reach of a box (a per-frame list of the boxes' reach in the cascade). Measure first: the same real-Cave benchmark with `VOX_WITHOUT_ENTITY_RAY=1`, and with one avatar and with sixteen, to see how much is the variant and how much the boxes. *Not an owner wish: my own todo.*
- **Cheaper leaves and glass** (2026-11-09). Leaves cost 2.6 ms over an opaque block on the integrated GPU in a dense test stand (nothing on a discrete GPU), because they are unmerged 16-byte quads. Options (merging them into the greedy path, drawing fewer inner sheets, doing nothing), a cheap experiment to run first, and a correction of my earlier claim that merging would fix the cost: `leaf-cost-options.md`.
- **Dither far leaves and glass** (owner, 2026-11-08). Past the texture distance (and in the coarse levels) a face is one flat colour, so far leaves and glass are solid cubes of the alpha-weighted average of their picture
  (accepted for now). The idea: beyond the fade band drop each pixel at random with the chance of one minus the picture's average alpha, so far foliage stays see-through. *Needs:* a screen-door or hash pattern in the shader that is
  steady from frame to frame (no wall-clock; the game tick and the face's place), a per-block opacity next to the colour, and a decision for the coarse nodes, which carry no picture and would still be solid.

- **Operator only.** `/tp` (and, for the same reason, the commands that change the world: `/ship`, `/gamemode`, `/light`) should someday be for operators only; for now everyone may use them. *Needs:* a notion of who is an operator (a list in the world or the server's configuration, by the key of a player), and the command registry asking it (`CommandSpec` has no permission yet). No rule of the game changes until then.
- **Teleport facing.** `/tp` should be able to say which way the player faces on arrival (to face a place, or given angles; and to face the planet's horizon on arrival at a body). *Needs:* the server to be able to set the way a player faces, which it cannot: `player_presence::PlayerPlaced` carries a position only and the client keeps its own angles, so the message wants a yaw and a pitch (a game message of Cave's or of the presence plugin), the client to take them, and a rule for what the angles are measured in (the level's axes, or the body's local frame, which a player's frame follows). Today only the landmarks of the great rivers put the place straight ahead, by backing off along the way the player is already looking (`places-spec.md`, section 6).
- **Forces over solver steps, and how many steps a tick has** (owner, 2026-10-31). A force put on a body is kept by Rapier for every solver step of the tick, so a ship at orbital speed (24 steps, 39 metres a step) feels the planet's pull of the place it started in, and gains energy and angular momentum at every tick (`tests/orbit.rs`, `build-log.md`, M1 step 1). The number of steps is the settings' 3, or more when a body is fast (`ceil(speed x tick / 0.8)`, at most 24: `physics::BodyWorld::steps_for_this_tick`). To decide: work the pull out again at every step, or kick the velocity at the middle of the tick, or another integrator; whether a tick should have a fixed number of steps for every body, a number by speed as now, or one by body; and what a step is worth. *Known:* the steps themselves are cheap (one cube costs 0.04 ms a tick at 24 steps, profiled 2026-10-31), so the choice is about accuracy and the golden sessions, not time. Changing it re-pins every golden session that has a moving ship.
- **What a fast ship costs the server** (profiled 2026-10-31; held back by the owner until ships of several grids are understood). One cube at orbital speed costs 8.5 ms of a 50 ms tick (14.7 before a first safe fix), nearly all of it in the terrain colliders asked for round it, not in Rapier. Five options to cut it, what each needs, and why they wait (a hinge, a tether, grids that travel together, shapes that change within a tick) are in `ship-terrain-cost.md`. The bigger options (a largest empty cube from the generator's bounds, ships on rails above an altitude, colliders made on a worker, coarse colliders for fast flight, terrain with no bricks), ranked, with what was measured on 2026-11-07, are in `terrain-collider-options.md`.
- **Tractor beam** (owner, 2026-11-01). Not now: a station, or a ship, with an area of effect in which other ships move with its frame on the same rail is not wanted yet. Someday a *tractor beam block* may give that effect to ships in the area of a station. *Needs:* the frame that follows a rail generalised from planets to any grid on a rail (`Follow` carries a planet today; `ServerWorld::influence_radius` is for planets), a rule for what a beam holds and lets go, and the per-ship frames of `orbits-and-thrust-plan.md`, which have no area of effect.
- **Fine control of an orbit over huge distances** (owner, 2026-11-02). With a periapsis of kilometres and an apoapsis of light years, a player at the periapsis should be able to throttle a thruster far below 1 percent and change the apoapsis by amounts small enough to tune a rendezvous with another ship. Measured in E1 (`orbits-and-thrust-plan.md`, "E1 done"): the rail and the Kepler solver are accurate enough (1.2e-10 m from the 120-digit reference at a light year); **a throttle, a speed that is an `f64`, making an orbit again from a state at the nearest point (a part in 10^8 at this length) and the solver's integration are not**; a push of 1e-12 m/s at 10.7 km/s moves the apoapsis by 2.4e9 m and an `f64` speed cannot keep it. *Needs:* an analog throttle (an `f64` on the wire); burns that change the elements by Gauss's equations from the rail's exact state, summed in more digits than an `f64`, on a rail and not in the solver (step C); a decision about the sphere of influence for such an orbit; the finest tuning to promise.
- **THE FINE-TUNING PROMISE (owner, 2026-11-02; weakened only if the owner decides it is utterly impossible without unacceptable compromises).** In a universe of real galaxies, stars and planets built from real astronomical data, it is possible to plan an orbit from any planet of any star to any other planet of any other star in the same galaxy, with up to ten gravity assists, to arrive at exactly the periapsis the orbit lines predicted, and to control the periapsis at the destination planet to within 1 millimetre. **With it, for burns:** a pilot gives up no freedom (a ship spins at random, manoeuvres with RCS and off-centre engines at any time, as in Kerbal Space Program) and need know no implementation detail. The owner is open to more precision in the rails. What the analysis finds is in `orbits-and-thrust-plan.md` ("The fine-tuning promise"); `history/spikes/rails/flyby_amplification.py` is the estimate of the digits, and **E4 measured it** ("E4 done" there): a chain of assists with no correction between needs about 22 to 28 bits an assist (336 bits for 12, 4,288 for 192, 8,432 for 300), a leg alone 64 to 88 bits; adaptive precision (run at `p` and `2p`, double until they agree) found the right precision every time at 4 to 5 times the cost of the final run; four pure-Rust libraries give the same correctly rounded bits (one with a faulty subtraction); the choice of library waits on the licence. *Adaptive precision with a horizon set by a button* (owner, 2026-11-02): each press of "continue prediction" adds a segment to the horizon, and only the segments inside it are checked for enough precision.
- **Nothing is lost by storing a planned path** (owner, 2026-11-02). A path planned to the millimetre must lose nothing by saving, loading, freezing, thawing, parking or unparking, and the owner would keep numbers of 4,096 bits in the world on long missions rather than lose a bit. *Measured (E5, `orbits-and-thrust-plan.md`):* for a ship on a rail today the elements, the place and the speed are the same to the bit after each of those and after all of them in turn (`tests/orbit.rs`); what changes a rail is a making again from a state (a push, a collision, something near, a change of the planet's mass), never storing. *Needs:* the same test kept for every future form of the rail (wide elements, burn programs), and wide numbers in the file when the elements are widened.
- **The map may show an almost-circular orbit as a circle** (owner, 2026-11-03): an eccentricity below 1e-14 is shown as a circle in the map interface; the rail and the file keep the ellipse (nothing is snapped internally; the owner may reintroduce it if there is a good enough reason).
- **Server settings for predictions** (owner, 2026-11-03): the most consecutive sphere-of-influence changes a player may ask a prediction for at once (default 100: the "continue prediction" button stops there), the fewest rails shown ahead before more are added by themselves (default 3), and the widest number a rail may have (default 65,536 bits). They limit predictions and never a ship's travel. Built with the hand-over between rails (`orbits-and-thrust-plan.md`, "The hand-over between rails: the owner's answers").
- **A keyboard shortcut that moves the map to the end of the prediction** (owner, 2026-11-03), which is generally the player's destination; and **a prediction that is remembered**: a ship stores the integer of how many sphere changes after its last manoeuvre the player predicted, and the prediction is regenerated after a logout however long, so that a player who comes back to a mission after a long time knows where they were going.
- **A client may work out previews of its own ship's orbit** (owner, 2026-11-02). Although the client is kept thin in most ways, when a player predicts their own orbit far ahead (dozens of assists) the client may do the heavy calculation. It is no cheating risk because the server never accepts a result computed by a client; it accepts only the player's choice of actions with their ship. *Follows (E6, `orbits-and-thrust-plan.md`):* the number layer gives the same bits on the client and the server; the previews run in a thread and check for being cancelled between operations; and the exact (wide) state of a ship's path has to reach the client in the message stream, not rounded to `f64`.
- **Licence of the repository** (owner, 2026-11-02). Thinking of GPL or AGPL; LGPL libraries are acceptable (so `malachite`, which is LGPL-3.0-only, may be used). Not decided, and there is no `LICENSE` file yet (decision 10 of `architecture-review.md`). AGPL's network clause is the one that reaches a dedicated server (`serve cave`).
- **Elliptical orbits, ships on exact rails, long burns** (owner, 2026-11-01): planned and investigated in `orbits-and-thrust-plan.md`; ellipses and the period from the mass are built (steps A0 to A3b); the conversions between a state and an orbit, the rails for free ships and the burns are not.
- **A planet whose mass changes** (owner, 2026-11-02; **decided: (b), 2026-11-02**, the mu follows the mass; what is built is the conversion, `Orbit::from_state`, tested for exactly this: the place and the speed of the moment are kept and the orbit changes. Not built: when it is done, and for whom, below). A small planet can be mined until its mass changes a lot, and a ship in orbit round it would then go in a different orbit. Today a body's mass is what its file says at the time the world is made, and the `period` of a rail is a number in the rail (given, or worked out once from that mass), so mining changes the *pull* that ships feel but not where a planet on a rail goes, and a rail that was worked out from the mass
  does not follow it. For a ship on an exact rail (step A4 of the plan) there are three honest choices. (a) **The rail's mu is fixed when the orbit is made** and mining changes only the real forces: simple, and a ship that is parked round a mined planet drifts from where it would have gone. (b) **The mu follows the mass:** at each change of mass (an event, not every tick) every ship on a rail round that body is turned into a state (position and speed, which the change does not touch) and back into elements with the new mu, so that it
  is continuous in space and speed and its orbit changes, as it physically would; the conversions are the work of A4, so this costs little more than a rule for when and for whom. (c) **A mass for orbits that is a property of the body** and not its blocks: a planet whose gravity is declared. My opinion: (b), at the moments the mass is settled (a change counted in a tick, ships round it re-fitted, an unobserved one once when it is next looked at), with the mu a number kept next to the mass; confidence medium, because it needs a decision about
  what a parked ship does while the planet is being mined and nobody is looking.
  **The owner's rule for when (2026-11-02, to be looked at again if it ever causes trouble):** a ship answers a change of the mass **at the apsis where the answer is safe**. A re-fit keeps the place and the speed, and at an apsis the speed is across the radius, so that apsis keeps its distance and only the other end moves. So an **increase** is answered at the **periapsis** (the periapsis stays, the apoapsis comes in: it cannot fall into the planet) and a **decrease** at the **apoapsis** (the apoapsis stays, the periapsis goes up: it cannot be thrown out). At each apsis the ship asks the planet for its mass, compares it with the last mass it knew, and re-fits only if the change is of the sort that apsis is for; at the wrong end it waits for the other.
  **Built (2026-11-02, `orbits-and-thrust-plan.md` B2), with one thing it found:** an apsis keeps its distance **as an apsis, not necessarily the same one**: at the periapsis the speed is that of a circle for (1 + e) times the mass, so an increase by more than that factor makes the point the far end and sends the near end down (e = 1/3: a mass half as much again takes 5,000 to 4,000); a decrease below (1 - e) makes the far end the near end and throws the far end out. The rule is safe for a change smaller than the eccentricity; beyond, the check that the new orbit clears the ground is what keeps a ship off the planet.
  **The owner accepted this (2026-11-02):** an answer that moves the apses is fine, a change big enough to threaten a ship would be very large, and it can be looked at again on the feedback of players, once there are players.
  *Edges I see, not decided:* a circle has no apsides (an orbit with a ratio of 1.0 would answer at once, and the change in it is small); an unobserved ship needs the history of the planet's mass to replay the passes it missed (the passes are known from the rail, the masses are not); a long wait to the right apsis lets the real pull and the rail's mu disagree meanwhile. *Ideas of the owner, later:* for very small planets the mass of the inventories of landed players could count in the weight of the planet, but a player who logs out must not make the planet lighter.
- **The prototype ball joint** (owner, 2026-10-31 and 2026-11-01): talked through, decisions in `decisions/joints-and-cars.md`, measurements in `joint-probe.md`; **not built yet**, the owner will say when. Open items that follow from it:
  - *Better bounds for a ship of several cars.* The prototype uses the sum of each car's longest dimension as a diameter, per car. *Needs:* a measurement of one cube against two (the cache of 256 bricks does not absorb the churn at orbital speed; belief, not measured), then a tube along the velocity or a sphere about the pivots.
  - *Loops of cars, several couplings per pair, pistons, lifting frames, walking machines.* *Needs:* the integer consistency rule, the reduction of a pair's joints to one constraint, and for loops a direct solver beside Rapier (not a fork: Rapier's solver is about 3,500 lines of parallel, coloured, SIMD code that moves every few weeks); a test with contacts and big mass ratios first.
  - *A joint between a planet and a car.* *Needs:* a check of f32 anchors a planet's radius from its origin, the planet's own motion (rail, following frame), the car and planet in one frame, and rules for parking a car whose planet partner is never parked.
  - *Joint error at the edge of a frame* was about 1 mm when positions were f32; Rapier and the positions are f64 now (2026-11-01 and 11-02), and it is far below that (not measured again). *Needs:* a local origin for each coupled group, or a smaller `FRAME_RADIUS_BRICKS`.
  - *A standing character hops and creeps when gravity's direction changes a little* (found 2026-11-01 by investigating a test that moved with the f64 switch; analysis in `joint-probe.md`): up to 1.6 cm of eye height about once a second, in f64 at any rate of change, in f32 only at 1e-6 rad a step or more. *Needs:* a decision among the three fixes listed there, and a regression test from `spikes/joints/walker_creep_test.rs`; first, whether it shows in play.
  - *Per-ship solver choice and dials* (owner, 2026-11-06): the loop closed by Rapier's impulse joint or by a direct solve, on a multibody or on impulse joints, with substeps and iterations as ship data. *Needs:* the decision in `joints-and-cars.md` (under review), the one box "a coupling = two cars and pivot pairs", and for the direct solve hinge loops (singular rows) and loops under contacts tried first; measurements in `loop-probe.md`.
  - *Rename the code's `Ship` to say it is one grid*, once "ship" means the whole vehicle everywhere else.
  - *Blocks larger than a metre* (benches, signs, paintings): talked through, no decision. Several ordinary blocks that draw one slice each (a "part" voxel that points to its anchor) work with the renderer as it is; pictures and text need a textured pass; models need a model pass. The renderer has no textures, no blending pass, and a shape cannot leave its voxel.
- **Planetary mass change** (owner, 2026-11-04): **not in the first release.** What is built, the report on what it does to players online and offline, a flaw found in it, and the ARGOS device for Cave are in `planetary-mass-change.md`.
- **A contact that moves only one of the two bodies: a player inside a ship never moves it** (owner, 2026-11-07). A player walking, bumping or jumping inside their own ship must change the ship's trajectory by nothing at all, not even by a rounding-sized bit of the rail's width; but a player outside, pushing on the ship with the EVA thrusters of a suit, holding a handle or just bonking against the hull, must move it. *Measured (`loop-probe.md` section 15, program `dominance`):* Rapier's **dominance groups** make the ship's velocity exactly unchanged (0.0) but solve the contact against the ship as a wall *at rest*, not a moving one (a 80 kg player and a ship at 2 m/s: the player ends at 0.64 m/s and sinks 201 mm into it), so they are wrong for a ship that moves. A **velocity-based kinematic** body is a moving wall that nothing moves: the ship's change of velocity was exactly 0.0, the player rode along at 2.014 m/s with no overlap. With equal dominance (the EVA case) the ship takes -2.4e-3 m/s from the 80 kg player against 100 t, as it should. *Needs:* the ship to stand in the player's contacts as a kinematic body (its velocity from the rail) while the player is inside, and as a dynamic one while outside (so that the push reaches the rail); a rule for the switch that is the same as "inside" for the air below; a test with a spinning ship (my two spinning cases were badly built, the face turned 1 rad away, so they say nothing); whether to toggle the ship's body type or give the player's contacts a kinematic twin; what the player's contact with another ship, a cargo crate or another player aboard does (ordinary contacts: they may move what they touch); a regression test that the rail and the ship's momentum are unchanged, to the bit, by a player walking about inside.
- **Life support air** (owner, 2026-11-07): air to breathe is held only in **enclosed spaces** of a ship, as its own data layer (a part of a grid, like the light). It does not exist on planets: on a planet a player gets as much oxygen as the altitude and the planet's atmosphere say (a rule of the body, no layer); inside an enclosed ship the player breathes the ship's air instead of the planet's. *Needs (my reading, nothing decided):* a test of "enclosed" from the block shapes' open faces (Phase 7 gave blocks open faces and collision shapes), a layer saved with namespaced ids like the rest, what a hole does (leak out, at what rate), and whether the air has an amount, a pressure or a mixture or is just present or not; the same "is this player inside" answer as the one-way contact above; what a player breathes in a ship that is on a planet with a hole in it.
