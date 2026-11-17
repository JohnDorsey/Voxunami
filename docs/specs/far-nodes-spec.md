# Far nodes for edited terrain and for ships (a plan, 2026-11-16)

Status: **a plan, nothing built, no owner answers yet** (the questions are in section 7, each with what I would choose). It finishes what `lod-spec.md` left open: the server's correction of far
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
- **[code]** The client keeps the overrides it was sent in the `LodView` it has. The session makes a **new** `LodView` when the player goes to a level the game draws differently (`follow_level_detail`), and the server's record
  of what it sent (`ClientState::sent`) stays. A game that installed the plugin and had both kinds of level would lose its overrides for good. **Not reachable in any game today; found by reading.**

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
budget (`maximum_ship_bricks`). Things to settle: whether the **entities aboard** a far ship are told of (the pilot of a far rocket: I would say yes, they are in `entity_messages` through `known_grids` already); the **protocol version** goes up
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
- **F1 The session keeps the overrides** (the client half of 3.5a). Test first: a game with a coarse level and a bricks-only level (Cave's `great_river`/overworld pair needs a server correction; the `game_shell` example rig with the plugin
  does it: `coarse_view_tests.rs` gets a second level), go and come back, the far nodes are as the server meant. Fails on today's code. Small, safe, first.
- **F2 Planets** (items 1 and 3 of section 2 in part): the plugin handles a ship with terrain (3.1), the session routes `CoarseNodes` to the planet's mesher, Cave installs the plugin and asks for a rate for planets. Tests: dig on the planet, the node
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

## 7. Questions for the owner (my choice first)

1. **Order**: F1, F2, then F4, F5? (Planets' edits first because they are the player's main building site; ships' far view is the biggest and I would not hide it behind the others.)
2. **The rate** (`bytes_per_second`): a Cave default and a player setting (`.detail`-style command), or a fixed one? I would take a default of 1 megabyte a second that a player can lower.
3. **Which ships are known far away**: by the pixel rule of `lod-spec.md` 3.7 (a ship is known while a node of it would be a few pixels across), or by a fixed distance? I would take the pixel rule, with a minimum size so that a single dropped block is not told to everyone.
4. **Entities aboard a far ship** are told of: yes, as they already are for any ship the client knows. (They are a few boxes; the cost is small.)
5. **The largest detail level with an override** is 12 (cells of 4 km). A planet of 1% of the Earth's radius is 128 km across, so a base is still one cell at that level; a bigger planet would want a higher level. Keep 12 for now?
6. **The protocol may break** for the far tier (version 9), as before the first release?
