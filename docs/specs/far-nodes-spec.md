# Far nodes for edited terrain and for ships (a plan, 2026-11-16)

Status: **a plan, nothing built; the owner answered on 2026-11-17 (section 7), and question 3 is reopened and worked out in section 8** (which changes F4 and F5). It finishes what `lod-spec.md` left open: the server's correction of far
nodes after an edit (`world-and-sky.md`: "Not done"), step 38j (ships), and the planet's nodes (`planet-spec.md`, "Not done" (c)). Marks as in `lod-spec.md`: **[code]** is what I read in the code,
**[run]** what I ran, **[belief]** what I did not check.

## 1. What is true today

- **[code]** The server half exists: `coarse_streaming::CoarseServerPlugin` keeps an *override* for each cube of detail level 1 to 12 that holds an edited brick (the generator's node patched with the exact cells of the edited
  bricks and of the overrides below), remakes dirty ones finest first (8 a tick, **on the thread of the tick**), and sends each client the overrides of the cubes it selects, coarsest first, within a byte credit. The client half
  exists: `LodView::set_override` (keyed by grid) makes the node from the override instead of the generator and replaces it where it is. A test in `game_shell/src/coarse_view_tests.rs` shows an edit far away in a watcher's
  view, a late arrival seeing the same, and a recording showing the same **[run, passes]**.
- **[code]** **No game installs the plugin**, and Cave asks for 0 bytes a second (`Cave::grid_detail`, `level_detail`), so in the game a dug hole shows in the bricks round the player and never in the nodes beyond. The reason
  written down (2026-10-23) is the cost of the generator's node in the overworld, 60 to 375 ms; **I have not re-measured it** and the build log suggests the overworld's nodes were made faster since (`build-log.md`, columns). The overworld is
  drawn in bricks only, so it does not need nodes at all; the level of space and the planet do, and their generators make a node in a millisecond or two.
- **[code]** The plugin handles **root grids only** (`State::mark_brick` returns for any other grid). A **planet** is a grid, so a block built on it, or a hole dug in it, is in no node. `ShipTerrainMesher` has a `LodView` but is
  never given overrides. A **plain ship** has no nodes at all: `GridMesher` draws bricks only.
- **[code]** **A ship is known to a client only if its bricks meet the watched region** (the view radius, 12 bricks by default, plus 2 or 4), or it is a planet, or the client's body is aboard it (`grid_replication::grid_messages`). A ship
  farther off is not in the picture: **a rocket 600 voxels away is not drawn** (read from the code; I have not flown it). The server sends all the bricks of a ship it tells of, up to 16,384 for a client.
- **[run, fixed by F1, 2026-11-17]** The client used to keep the overrides it was sent only in the `LodView` it had. The session makes a **new** `LodView` when the player goes to a level the game draws differently (`follow_level_detail`), and the server's record
  of what it sent (`ClientState::sent`) stays, and `SetDetail` does not clear it, so a game that installed the plugin and had both kinds of level lost its overrides for good. A test showed it (a crater dug in a coarse level was gone from the picture after a visit to a bricks-only level and back).
  The session now keeps every `CoarseNodes` update (`game_shell::coarse_overrides`) and gives them to any view it makes. **Not reachable in any game today** (no game installs the plugin).

## 2. What matters in Cave, in my order (a belief about what the owner will see first, not a measurement)

1. **What players build on or dig out of the planet, seen from orbit or from a few kilometres.** A planet is the main place a player builds. Cost to do: moderate (the machinery exists; the grid is not a root grid).
2. **A ship seen from far away** (a rocket leaving, two ships docking at a distance, a base on a moon that is a ship). Today it is not drawn past about 450 to 500 voxels (the default view radius is 12 bricks, and the region watched is 2 or 4 bricks more). Cost to do: large (the wire changes).
3. **Edits to the level of space's own terrain.** Rare in play; the plugin already does it once it is installed.
4. Edits in the overworld seen far away: **not needed**, the overworld is bricks only.

## 3. The design

**3.1 One mechanism for every grid.** An override is, as now, the cell summary of a cube made from *exact* data where there is any and from a base where there is not. The generalisation is only the base and the data:

| grid | base node | exact data |
|---|---|---|
| a level's root (as built) | the level's generator | edited bricks of the level |
| a planet (a ship with terrain) | the planet's generator, none outside its extent | the ship's stored bricks |
| a plain ship | empty | the ship's bricks (all of them are stored) |

`remake` takes a `Grid` of bricks and an optional generator and does not care which. `mark_brick` stops refusing non-root grids. The message already names the grid. Whether the *edit event* of a ship reaches the plugin is
**[belief]**: the plugin listens to `on_channels` with the event's grid, and a ship's `set_moving_voxel` records the same kind of change; the first test of F2 settles it.

**3.2 The server's cost.** Today every dirty cube costs a call to the generator for the whole cube (`terrain.coarse_node(cube)`) on the thread of the tick, up to 8 a tick, up to 12 levels per edit. For a planet and for the level of
space this is milliseconds **[belief: from the build log, to be measured in F0]**. Two changes that do not change what is sent: do the work on a worker, as `prefetch.rs` does for bricks, and remember the generator's node of a cube
(it never changes once released, so the memory needs no invalidation, only a limit). Not recommended: sending only the patch and letting the client merge it with its own generator node. It would take the generator off the server
and cut bytes, but a parent's cells depend on the children's *exposure*, which is deliberately not sent, so the client could not make a coarser node from a patch alone. I have not tried it; this is reasoning.

**3.3 Who is sent what.** For a grid that stands still the selection stays as built: the same function on both ends from the viewer's brick and the `SetDetail` values (`lod-spec.md` 3.5).
A **moving** grid (a planet on a rail at 125 m/s moves 6 voxels a tick, a new brick about every 5 ticks; a rocket at 6 km/s moves 300 voxels a tick, 9 bricks) puts the viewer in a different brick of the grid's own axes every few ticks or every tick, so the per-client selection would be remade that often (about 4 ms for a
sphere **[measured in 38f]**, per client, per grid). Proposal: for moving grids send by **bands of distance**, not by brick: the levels from a coarsest down to `k`, where `k` follows the distance from the viewer to the box of the grid in steps of one detail level with a margin, so
a client far from a ship gets only its coarse nodes and one that comes closer gets the next level down before it needs it. It sends a little more than the exact selection and costs nothing per tick. To be decided after F0's numbers.

**3.4 A ship that is known by its nodes only (the wire).** `GridsSpawned` gets a **tier**: *near* (as now: all the bricks follow) and *far* (no bricks; the nodes arrive as `CoarseNodes`). A far ship is told of when it would be a few pixels
across (`lod-spec.md` 3.7 is the rule; it was never built), and promoted to near by the bricks being sent when its bounds meet the region; the other way, demoted, by the bricks being dropped and the ship kept. It costs no brick of the
budget (`maximum_ship_bricks`). Things to settle: whether the **entities aboard** a far ship are told of (the pilot of a far rocket: I would say yes, they are in `entity_messages` through `known_grids` already); the **protocol version** goes up (it is 11 today, so 12; an earlier draft of this plan said 9, which was wrong)
(owner, before the first release: formats may break); a **recording** carries it unchanged (it is a message stream), and a test shows a replay of a far ship draws the same.

**3.5 The client.** (a) The overrides belong to the **session**, not to a view: one store, keyed by grid, that a `LodView` reads (`set_override` now writes into the view). A view that is made again is given what the server has already sent.
That is the whole fix of the mismatch in section 1, and it is small and stands alone (F1). (b) `CoarseNodes` for a grid goes to the view that draws that grid: the level's `LodView`, a planet's `ShipTerrainMesher`, or a far-ship view.
(c) A far-ship view is a `LodView` over a `CoarseTerrainSource` for a ship with no generator: `known(brick)` is the stored brick or an empty one inside the bounds, `coarse_generator` is none (the override is the only source), and
**`version()` must change when the ship's bricks do** (today a planet's is the constant 0 because nothing about it changes, and the may-show memo is keyed by it; a ship's changes, so a stale memo would hide a node). `GridMesher`
stays for ships that are near.

**3.6 Hard rules.** Nothing here is simulation: nodes are cosmetic, so no cross-platform bit identity is needed beyond replay (which a message stream gives). The rate limit is by ticks (a credit refilled each tick), not by clock.
Saved files do not change (coarse data is never saved, owner's decision 16). A ship's cap on coarseness (38j) is a separate small field, not part of this plan.

## 4. Phases (each ends with tests, clippy, rustfmt, rustdoc, mutation experiments, docs, a commit, a push)

- **F0 Measure** (throwaway, no code kept): the time of `remake` for a planet cube at each level, for a plain ship of 100 and of 16,000 bricks, for the level of space; the bytes of an override at each level; the cost of reselecting a sphere's
  leaves as often as a planet on a rail or a fast ship makes it needed; the generator's node cost in the overworld now. Written in the build log. **What F3, F5 and the answer to question 5 depend on.**
- **F1 The session keeps the overrides** (the client half of 3.5a). **Built 2026-11-17** (`game_shell::coarse_overrides`; the test is `coarse_view_tests::an_edit_the_server_told_of_is_still_drawn_far_away_after_a_visit_to_a_level_that_is_drawn_in_bricks_only`, which failed first and passes now; three mutants killed). Test first: a game with a coarse level and a bricks-only level (Cave's `great_river`/overworld pair needs a server correction; the `game_shell` example rig with the plugin
  does it: `coarse_view_tests.rs` gets a second level), go and come back, the far nodes are as the server meant. Fails on today's code. Small, safe, first.
- **F2 Planets** (**built 2026-11-17**, build log; not built: the test of a planet on a rail, the player's own rate setting, the scaled light in an edited node). Items 1 and 3 of section 2, in part: the plugin handles a ship with terrain (3.1), the session routes `CoarseNodes` to the planet's mesher, Cave installs the plugin and asks for a rate for planets. Tests: dig on the planet, the node
  over it changes from far away; a player who arrives later sees the same; a recording shows the same; the planet moving on its rail does not resend. Mutants: an override not routed, an override of the wrong grid, a version that does not change.
- **F3 The server does the work off the tick** (3.2), after F0 says it is needed: a worker and a cache of generator nodes; tests that the tick does not wait (counted, not timed: a clock may not decide a test) and that what is sent is
  byte for byte what the synchronous version sent.
- **F4 Ships, the nodes** (3.1 and 3.5c): overrides of plain ships from their bricks, the far-ship view, a ship drawn in nodes where it is known; tested with a ship the client already knows (so no wire change yet): its nodes beyond the
  bricks, and an edit to it.
- **F5 Ships, the wire** (3.4, and 3.3 for moving grids): the far tier, the bands of distance, the promotion to bricks and the demotion. Protocol version. **The largest and riskiest piece, and the one that makes the rocket at 5 km visible.**

F1 and F2 can be had in days of work; F4 and F5 are the bulk. F3 may turn out not to be needed.

## 5. Risks

- **The wire change (F5)** touches the replication of ships, the budget, entities aboard, the client's idea of which ships it knows and every golden recording. It is the step to stop at and say so if it grows, as 38a was.
- **A moving grid's selection** (3.3) is a guess until F0.
- **A far ship that is also a planet's neighbour** (a base on a moon): its nodes are in its own axes and its pose is the world's, so it is drawn right; the check that nodes of a ship that turns are placed exactly is `lod-spec.md` 3.3's property test, which holds for any grid.
- **Exposure and parents**: an override that is made from other overrides depends on their exposure, which is not sent; this only matters on the server and is unchanged.

## 6. What I would not do

- Send a ship's nodes to a client that already holds its bricks: it can make them itself from the bricks (`coarse_node_of_brick`); only the far tier needs the server's.
- Turn the plugin on in the overworld: its nodes are not drawn there and the generator's cost is what stopped it.

## 7. The owner's answers (2026-11-17)

1. **Order** F1, F2, then F4, F5: **yes.**
2. **The rate**: a Cave default of 1 megabyte a second that a player can lower: **yes.**
3. **Which ships are known far away**: **reopened**, section 8.
4. **Entities aboard a far ship**: whether one is shown from far away depends on **data of the entity** (not a blanket rule). Entities may be used for multi-voxel blocks, which are then part of how a far ship looks. Open: which component carries it (8.6).
5. **The largest detail level with an override** (12: cells of 4 km, a node of 128 km): the owner asked what it means; explained in the reply. I keep 12 unless told otherwise. A cell of that level is as small as three pixels only from about 1,000 km, so a bigger planet's edits are not visible at the levels above it anyway.
6. **The protocol may break** (11 to 12) for the far tier: the owner asked what it means; explained in the reply (an old client and an old server cannot talk, and the golden recordings are made again). Before the first release the rule in `CLAUDE.md` allows it. Waiting for a yes.

## 8. Ships far away (the owner's questions of 2026-11-17, answered from the code; reading, not runs, unless marked)

**8.1 What "known" means.** A client *knows* a ship when the server has told it the ship exists (`GridsSpawned`) and sent its bricks; a ship the client does not know is not drawn at all. Today **[code]** a ship is known if its bricks
meet the region watched (the view radius plus 2 or 4 bricks, about 450 to 500 voxels), or it is a planet, or the client's body is aboard it. The pixel rule of `lod-spec.md` 3.7 (known while a node of it would be a pixel across) was written and never built.
At 1080 lines and a 70 degree view a pixel is 0.0013 radians, so under that rule **a ship 10 voxels long is known out to about 7.7 km and a 1 km ship to about 770 km** (arithmetic, not a run).

**8.2 Is a ship ever simulated beyond the simulation distance?** The rule **[code: `grid_lifecycle::update`, `ShipSettings`]**: moving and edited within 1,024 voxels of the nearest brick of the ship (so already by the size of the ship: a ship's *near edge* counts, not its middle),
frozen beyond (1,126 on the way out, a tenth more), **parked in a file and out of memory beyond the unload distance** (1,024 too by default, so a ship is parked as soon as it freezes). Only players in the same level count; a level with no player has nothing awake.
The exceptions I found:
- **A planet and a ship on a rail are never parked** and are brought in at once whatever the distance. They are *frozen* when no one is near, but **their rail goes on** (its place is a function of the clock, `apply_rails` for all of them every tick, with the hand-overs between bodies at their exact instants). A coasting ship becomes a rail ship, so a ship thrown into orbit moves at any distance. That is movement, not physics: nothing of its rules, collisions or edits runs.
- **Ships coupled by joints are one group** kept by the nearest member: the far end of a long train is awake 1,000 or more voxels from every player; a ship in a file coupled to one on a rail comes in with it.
- **The host's setting** (`everywhere`, tests use it too) turns distances off.
- The other way: a ship that **waits for its pilot** is parked and is *not* brought in by a near player.
- A saved world has all its ships in files at the start; they come in when a player is within 1,024 voxels (or at once, for rails and planets).
- **Unpiloted ships** are simulated like any other within the distance (`players-and-ships.md`); the pilot does not matter.
Not checked: whether any *plugin* ticks a frozen ship by itself (I read the lifecycle plugin and the docs, not every rule). What this means for far display: **a ship that is farther than 1,126 voxels from every player is, for most ships, not in memory at all**, so the server has nothing to make nodes from without reading its file. The 8.5 proposal answers that.

**8.3 Are coarse nodes saved?** No, and nothing is planned yet that saves them **[code]**. Decision 16 ("never saved, rebuilt as needed") was made for terrain, where a generator can remake any node. For a ship **nothing can remake them but all of its bricks**. The ship file
(`world_storage::ship_file`) is a container of sections (`engine:ship`, `engine:grid_flight`, `engine:grid_terrain`, `engine:rail`, and any other, which is **kept byte for byte** by a reader that does not know it), so a new section is possible without breaking anything. But a **separate file** next to the ship's is better, in my opinion (medium confidence):
the ship's file is written whole when the ship is parked, and a sidecar can be written a cube at a time as the nodes are made; an external program that writes a huge ship can write the sidecar too, since it knows the shape; and a damaged or missing sidecar costs only the far view. It would be a **reversal of decision 16 for ships** only (the owner's word is the reversal; this is what the owner asked for).

**8.4 A very large ship ("a Death Star").** What would happen today **[code]**: bringing the ship in reads the **whole** file into memory (`read_ship` gives a whole `MovingGrid`), the first tick after it comes in walks **every voxel of every brick** for the rules (`take_changes`), and a client is sent its bricks only up to 16,384 (`maximum_ship_bricks`). So a ship of millions of bricks cannot be brought in *at all*, never mind drawn; that is a separate limit from LOD and needs bricks paged in as a level's are (not planned anywhere that I found; I would call it a prerequisite and say so to the owner).
What already exists: the **dirty-ancestor** machinery and the rate-limited remake of nodes after an edit (`coarse_streaming`, on the tick thread today); the bricks' own nodes are made by `voxel_meshing::coarse_node_of_brick` from a brick and its six neighbours. What is **not** built or planned: a worker that makes nodes (that was F3, "if needed"), nodes that are kept between runs, nodes made from a file without bringing the ship in.

**8.5 The proposal (a far record).** (a) For every ship, parked or not, the server keeps a small **far record** in memory: its box, its pose or rail, its level, and its *coarsest nodes* (say the levels from 5 or 6 up: a few hundred cells for a ship of any size). The world already keeps the first three for a parked ship (`ParkedGrid`: bounds, pose, level, rail flag), so this is those plus the nodes.
(b) The nodes come from the **sidecar file** (8.3), read when the world starts or the ship is first seen. (c) A **worker thread** keeps the sidecar up to date: it builds missing nodes from the ship's bricks at low priority, so a ship made by another program gets them as time allows, and remakes the ancestors of a changed brick **while players edit** (the dirty marks exist; the worker is F3). It does not have to be quick, as the owner said; what it must do is *never block the tick* and *keep up on average*. A far client gets what exists and the rest as it arrives; the ship is drawn from what is there. (d) A tool command (`launcher prepare-ship FILE`) can build a sidecar offline for a ship made outside.
(e) **Parking distance by size.** I would keep the *simulation* distance at 1,024 voxels from the nearest brick (what moves and is edited is what a player can touch) and make the **unload distance grow with the size of the ship**, e.g. the larger of the setting and a multiple of the ship's longest side, because bringing a large ship back costs in proportion to its bricks and its far view should not flicker. This is an opinion (medium) and one number to choose with the owner; it needs the far record to be worth it, since a parked ship is drawn from the record.
(f) Far display is then decided by the **pixel rule with a minimum size** (8.1), and it needs no loaded ship.

**8.6 Entities (the owner's answer 4).** An entity kind is a set of components, each marked replicated or not (`EntityKindSpec::component`). A component such as *seen from afar* (the largest distance, or the smallest pixel size, at which it is told of and drawn) would let a kind decide, and a multi-voxel block made as an entity would set it from its size. For a far ship the server tells a client of the entities aboard that have the component and are near enough by their own rule. To decide: the component's name and unit, and whether a ship's far record keeps the list of such entities (it must, for a parked ship).

**8.7 What changes in the phases.** F4 (ship nodes) and F5 (the wire) gain, in front of them: **F4a, the far record and the sidecar** (the format, the reading, the writing; tests that a parked ship's nodes come from the sidecar and a damaged one costs nothing but the far view), and **F3 (the worker)** is no longer "if needed": it is where nodes of ships are made. The paging in of the bricks of a huge ship is a separate piece of work, outside this plan.
