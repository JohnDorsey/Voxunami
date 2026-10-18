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
- *Docking connectors* are an engine-provided part (a pair of ports, an attraction force and a snap distance, a joint when they meet),
  configured by mods, which also settles how two grids can ever touch. What crosses a docked connector (structure only, or crew,
  power, items) is open.
- *No size limit* is a promise about the API (nothing assumes a small grid; plan.md, requirement 11) and about work (budgets, not caps),
  not about the first version. The parts that grow with size: collision (coarse colliders and aggregated mass for the whole ship, fine
  colliders only near a contact), drawing (levels of detail per grid), streaming (interest per grid in its own coordinates), connectivity
  after an edit (incremental or hierarchical, not a flood fill of the whole ship), and precision (each grid works relative to its own
  origin). A moon-sized grid is a long way off; the first ships will be small.
- *A ship in its own file* means a stable `GridId` (requirement 10) and a file format for a grid (requirement 12), and that a save writes
  several files that must agree.

**Still open:** what crosses a docked connector; whether a ship file is portable between worlds (a blueprint, as KSP craft files are);
whether entities aboard are saved with the ship or with the world; whether a ship's own mass is a source of gravity (a moon-sized ship
would be).

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
