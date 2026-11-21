# Viewpoints: the camera as something that asks for terrain (a draft, 2026-11-21)

Status: **a draft for the owner's interview; nothing is built.** It extends `lod-spec.md` (which decides what one viewer draws) to several viewers at once: the ship you fly, a map, a free camera, a preview of an
orbit. Read with `far-nodes-spec.md` (far ships and the server's nodes), `planet-spec.md`, `protocol-and-replay.md` and `design-decisions.md`. Marks as in `lod-spec.md`: **[code]** is what I read in the code,
**[run]** what a program measured (the programs are in `history/spikes/viewpoints/`, written against the code of commit `aa419ee`, and are not kept compiling), **[belief]** what I did not check. The owner's words
are quoted; every other choice here is mine and marked as a recommendation until the owner has answered section 8.

## 1. What the owner asked for (2026-11-21)

1. Switch back and forth between the up-close ship view and a zoomed-out map view **without regenerating, re-downloading or re-meshing any node that stays the same size between the two**.
2. Switching is **instantaneous**: no moment where the up-close terrain is too low in detail, at least not after the first time.
3. **No noticeable frame-rate cost** for keeping more high-detail nodes in memory.
4. Review the idea of keeping several detail levels of the same terrain in memory and drawing only the ones the active view needs; investigate alternatives (sections 4 and 5).
5. Leave room for what comes next: maps from extremely far away, **a free camera with no ship** (creative, spectator), and **a camera that zooms ahead along an orbit** to show a trajectory in first person without the ship.
6. Then interview the owner to close the gaps (section 8).

## 2. What is true today

- **[code] A view keeps nothing but its current cut.** `LodView` works out the cubes to draw from one viewer (`coarse_cells::select_around_brick`), and a node's record exists only while the node is wanted or shown: `CubeStates::want` drops every
  record that is neither (`cube_states.rs`), `hide_node` drops the picture of a node that goes, and `WorldMesher::hide` drops a brick's picture ("the brick has none until it is made again", `lod-spec.md` 3.6a). Only the yes/no answers of
  `may_show` outlast a cut, for two rounds (`MayShowMemo`). So leaving a view throws all it built away, and coming back makes it again.
- **[code] A picture goes on the screen at most 64 to an update** (`SHOWN_PER_UPDATE`, `lod.rs`), each a new GPU buffer (`VoxelRenderer::gpu_brick`: one `wgpu::Buffer` for the whole-voxel quads and one for the detailed ones). 12,000 nodes need about 190 updates: 3.1 s at 60 frames a second even when every picture is ready.
- **[code] The renderer draws everything it holds and does not choose** (`upload_coarse_node` says so). Every frame `VoxelRenderer::gather` walks `bricks`, `coarse_nodes`, `ship_bricks` and `ship_coarse_nodes`, makes a `Drawable` of each (an exact offset from the camera
  in 128-bit integers), and the culling and the shadow planning loop over all of them. Every upload and removal also counts up `terrain_version`, which the shadow maps watch.
- **[code] One `LodView` for each grid, all driven by one camera.** The level's root terrain is a view; each planet is a `ShipTerrainMesher` with a `LodView` and its own worker threads (`Session::planets`); each far ship has one with no fine ball. Each takes the
  camera's position in its grid's axes (`Session::update_view`, `ship_updates`).
- **[code] One region of interest, one level.** The client sends `SetInterest { level, region }`, a box round the camera's brick, again only when the camera's brick changes (`Session::update_view`). The server sends the edited bricks inside it. On a new region **the client forgets
  every edited brick outside it, and of every other level** (`client_world.rs`, `InterestChanged`), and a brick outside the region is an error (`BrickOutsideWatchedRegion`): a second region is not possible today. So going to a map and back **does re-download** the edited bricks of the ship's region and its
  generator assignments.
- **[code] What the server is asked about coarse nodes is asked once and answered for one viewer.** `SetDetail` carries the pixel size and the rate; the server picks the cubes with the same function as the client, from the **player's** pose (`coarse_streaming::server::plan`, `presence.pose_of`), not from the client's camera. Edits the server sent are kept by the session (`coarse_overrides`) and given to any view made later, so those are never
  downloaded twice; the server's own record of what it sent stays too.
- **[code] Per-brick GPU tables are local to the camera.** The occupancy volume (`set_occupancy`) holds a window of 64 bricks round the camera, "bricks told of must be near each other: a brick 64 bricks from another one in every direction shares its place in the tables, and the one told of last is kept".
- **[code] The fine ball is never pruned.** `select_around_brick` keeps every brick of the fine region whatever `may_hold` says (a player builds in the air), so any viewer, in empty space too, has 8,552 brick cubes at the default radius of 12 bricks.

## 3. Measurements [run]

Cave's planet (radius 63,710, hills about 320 high), the rule of `lod-spec.md` 3.3 at 3 pixels a cell, 1080 lines, 70 degrees, the planet's horizon cull, a ball of 12 bricks. "Nodes" excludes the 8,552 bricks of the ball, which are nearly all air
away from the ground. Cuts were worked out with no session (`probe_pure_cuts.rs`); mesh sizes and times are from a real headless client (`probe_two_views.rs`: server, player, `update_view`, three worker threads, release).

**The two cuts, and how much of the map the ship's cut already holds.** *Shared* is the same cube in both. A map is the same camera moved straight up.

| ship | its cut: nodes | map | map: nodes | shared with the ship's cut | map-only (to build) | ship-only (kept for the way back) |
|---|---|---|---|---|---|---|
| 490 m up | 14,915 (levels 1 to 7) | +1 km | 10,615 | 8,616 (81%) | 1,999 | 6,299 |
| | | +3 km | 7,333 | 4,595 (63%) | 2,738 | 10,320 |
| | | +10 km | 4,292 | 1,586 (37%) | 2,706 | 13,329 |
| | | +30 km | 2,112 | 68 (3%) | 2,044 | 14,847 |
| | | +100 km | 588 | 0 | 588 | 14,915 |
| | | +1,000 km | 8 | 0 | 8 | 14,915 |
| 20 km up | 2,906 (levels 6 to 8) | +1 km | 2,828 | 2,764 (98%) | 64 | 142 |
| | | +10 km | 2,144 | 1,688 (79%) | 456 | 1,218 |
| | | +30 km | 1,360 | 588 (43%) | 772 | 2,318 |
| | | +100 km | 368 | 0 | 368 | 2,906 |

In no row does the map hold a cube finer than the ship's cut inside one of the ship's cubes ("map finer inside ship's": 0): a map is always coarser where they overlap, so **the map's near-ship nodes are ancestors of the ship's, and none of them is in the ship's cut** (a cut's leaves lie 8 to 16 of their own edges from the viewer, and their ancestors are the nodes that were split to reach them). They are new data, made from the generator (or the server's override), not summed from what the ship view holds.

**Memory (real pictures, whole-voxel quads of 8 bytes, detailed 16).** The ship's cut at 490 m, settled: 12,155 non-empty meshes, **22.0 MB** (levels 1 to 6: 3,184 / 2,771 / 2,218 / 2,007 / 1,659 / 316 meshes, 1.5 to 3.5 KB each). The cut seen from 20 km up: 2,392 meshes, 6.9 MB. A map from 200 km: 244 meshes, 0.8 MB; from 2,000 km: 8 meshes, 23 KB. The maps between were **not** run for size: from the per-level sizes of the real cut (1.5 KB at level 1 to 4.7 KB at level 8), a map's own cut is
about 21 MB at 1 km (81% of its nodes are the ship's), 17 MB at 3 km, 13 MB at 10 km, 7 MB at 30 km and under 3 MB at 100 km (an estimate from sizes, and about a fifth too high, since 81% of the cut's cubes had a mesh).
Level 0 near ground is about 11 KB a brick (spawn walk: 130 bricks, 1.4 MB). These are meshes only, not wgpu's own overhead or a second copy on the CPU, and the planet's nodes have no cave walls (a node of the overworld's caves has 25 times the quads, `lod-spec.md` 9a): **[belief]** the real figure with cave geometry in far nodes would be many times larger.

**What it costs to build one cold.** From a map back to the ship's cut at 490 m: 12,011 uploads (the other 144 were shared) in 3.2 to 3.5 s (3.4 thousand a second on three threads); the first visit to a map at 20 km up from the ground: 2,248 uploads in 0.45 s; at 200 km: 228 in 0.05 s. Uploading a mesh (one buffer) costs 12 to 22 microseconds on the main thread, by adapter (iGPU Vulkan 12, RTX 3090 DX12 14 to 22, iGPU DX12 10 to 23):
12,000 are 0.15 to 0.27 s of the main thread, before the cap of 64 an update.

**What it costs a frame to hold meshes nobody sees** (`probe_held_meshes.rs`: N coarse nodes behind the camera, all culled; the frame is the whole offscreen call at 64 by 64, median of 15): about **0.1 ms for every 1,000 held**, on each of the three adapters, shadows on or off.

| held, unseen | 0 | 1,000 | 5,000 | 20,000 | 25,000 |
|---|---|---|---|---|---|
| frame, iGPU Vulkan | 0.16 ms | 0.17 | 0.53 | 2.28 | 2.09 |
| frame, RTX 3090 DX12 | 0.17 | 0.22 | 0.59 | 2.50 | 2.84 |
| frame, iGPU DX12 | 0.13 | 0.17 | 0.56 | 2.72 | 2.74 |

(The 25,000 row is 50,000 uploads of which 24,700 replaced others: my lattice folded. Treat the last column as 25,000.) So **keeping both cuts in the renderer's maps, as it is built, costs about 2 ms at 20,000 meshes: 12% of a 60 fps frame**, for nothing drawn. That is the number behind "no noticeable frame cost".

## 4. The idea reviewed: several detail levels resident, one drawn

**What it is.** Keep the pictures of every cut that matters (the ship's, the map's) resident, and draw only the cut of the active view. The pictures for a cube do not depend on the viewer (a node's mesh is made from its cells with no neighbours, `mesh_coarse_node(&node, &[None; 6])`; a brick's from its voxels and its six neighbours; **[belief]**
nothing in a picture depends on the camera: textures near the viewer, `lod-spec.md` decision 6, are not built, and if they are chosen when the picture is made they would split the pictures by viewer), so one store of pictures keyed `(grid, cube)` can serve any number of views.

**Verdict: yes, with one correction to the shape.** The data say the idea is cheaper than it sounds and a literal reading of it is not enough:

1. **It is cheap in memory.** Keeping the ship's cut costs at most 22 MB (about 7 MB high up). A map's own cut is up to about 21 MB (at 1 km, mostly the ship's nodes) and under 3 MB beyond 100 km; what a map adds to the ship's cut is the map-only column of section 3: 2,000 to 2,700 nodes, about 4 to 10 MB. The union of the ship's cut and any map is about 32 MB at the most at Cave's planet, shared nodes counted once. Memory is not what decides.
2. **It is not free in the frame as the renderer is built**: 2 ms at 20,000 meshes, because the renderer walks what it holds. Resident must be **separate from drawn**: a store that nothing walks every frame, and a draw list that is only the active cut. That split is the change that makes the idea work, and nothing else in this document is as valuable.
3. **It is not instant unless a switch is O(1) per picture.** A switch that creates buffers is capped at 64 a frame and costs 12 to 22 microseconds each: 3 s for the ship's cut. A switch has to flip the draw list (a key moving between two maps, or one pointer), with the GPU buffers already made.
4. **"Several levels of the same area" is not the shape of the saving.** A pyramid with every level over an area holds more than two cuts; the two cuts share only the far field, and the map's near-ship nodes are ancestors that no cut of the ship's holds (section 3). What is kept is not a pyramid but **the union of the cuts of the viewpoints that are kept warm**.
5. **Keeping a cut warm is not keeping it still.** A ship that flies while you look at a map moves on: the cut you will return to is the cut at the ship's place **then**. So a warm viewpoint has to be tracked (its cut worked out again as the ship moves, built in the background), at the cost the active view pays now in worker time and none in frame time. At orbital speeds in empty space the cut is nearly nothing (air is not asked for, `ship-terrain-cost.md`); over a planet's face at 7 km/s it is a few thousand nodes
   a second. **[belief]** not measured; the order of building (coarsest first) makes the worst case a coarse picture on return and never a hole.
6. **The server has to keep a warm viewpoint current too**: the edits near the ship while you look at the map, and the coarse nodes the server sends. Today the server knows one region and one viewer for a client (section 2), so this is a protocol change, not only a client change.
7. **Things that are camera-local must be told when a switch makes a cut the drawn one**: the occupancy window of 64 bricks, the contact-trace windows, the shadow maps (they are drawn for the active camera anyway, and `terrain_version` must not count up for a picture that goes into the store without being drawn). The cost of filling the occupancy of the bricks a switch shows was not measured.

## 5. Alternatives

| | what it is | instant after the first time? | frame cost | memory | work | verdict |
|---|---|---|---|---|---|---|
| 0 | today: a view keeps its cut only | no: 3 s to 0.05 s by size | of what is drawn | the cut | none | the baseline |
| **1** | **warm viewpoints: one store of pictures shared by all views, pinned by the views that want them, resident apart from drawn** | **yes** | **of what is drawn** | **the union of the cuts** | **large (renderer, view, protocol)** | **recommended** |
| 2 | a resident pyramid: every level over an area round the ship, any cut chosen from it | yes, inside the area | of what is drawn | more than the union | medium on top of 1 | no as the base; as a **prefetch rule** for a zoom (section 6.5) yes |
| 3 | an LRU of pictures behind today's view (dropped nodes go to a cache under a byte budget) | usually; not guaranteed | of what is drawn if the cache is outside the draw maps | the budget | small | a stepping stone: it is 1 without pins, tracking, or the server |
| 4 | make a cold build fast (more threads, meshing on the GPU, one arena buffer for uploads) | no: 12,000 nodes in 100 ms needs 35 times today's rate | none | none | large | not an alternative; **the arena is part of 1** (12,000 buffers is a bad shape) |
| 5 | one continuous dolly with the cut built ahead along the path | smooth, not a cut | of what is drawn | small | small | the right way to **zoom** a map; not a switch |
| 6 | a baked map layer (a cube map or tiles of the planet, drawn instead of the voxels) | yes | tiny | tiny | large: a second picture of everything | no: it breaks "LOD works on ships as on terrain" (`lod-spec.md` decision 1) and edits need baking; **[belief]** the one possible use is beyond the largest node (stars, galaxies) |
| 7 | GPU-driven selection over a resident arena (a compute pass picks the cut of each view each frame) | yes, for any number of views | about none | the resident set | very large | the end state; 1 with the arena is its first half; decision 15 of `lod-spec.md` asks to wait for a measured limit, and **0.1 ms a thousand is that measurement** once resident sets pass 20,000 |

**What is kept** (choices inside 1): (a) CPU mesh and GPU buffer both, instant and the most memory; (b) CPU mesh only, uploaded into one arena buffer on a switch, a bulk copy that I **believe** is a few milliseconds for 22 MB and did not measure, half the memory; (c) the node's cells, re-meshed on a switch: about 1.3 ms for a mixed node, so no;
(d) nothing: today. Recommendation: (a) for the active and warm cuts, (b) for what an LRU keeps beyond them.

## 6. The design I recommend (a draft)

### 6.1 Terms

- **Viewpoint**: what asks for terrain. A place (a level and a position), an orientation, a projection, detail settings (`cell_pixels`, the pixel size, the farthest distance, an optional zoom cone), a **fine radius** (the ball of bricks), a **time offset** (0 now; for an orbit preview, ahead), and a **role**: *body* (the player's eyes: a fine ball, bricks and edits, follows the body or ship it is
  aboard), *free* (a map or a spectator camera: no body; the fine ball is as small as the altitude needs and pruned by what may show, since nobody builds from it), *predicted* (a free viewpoint at another time).
- **Active** viewpoints are drawn this frame (one per window; a minimap would be a second). **Warm** ones are tracked and not drawn. **Pinned** is a warm one that stays until released (a bookmark of the ship view while you look at the map).
- **Picture store**: the pictures (a CPU mesh, a GPU buffer, a version) keyed `(grid, cube)`, with the viewpoints that want each; unwanted ones go to an LRU under a byte budget; a picture whose data version changed is made again when it is next wanted, not before. **Draw list**: the cubes of the active viewpoints' cuts, the only thing the renderer walks.

### 6.2 What must hold (each is a test)

1. A picture is the same bytes whichever viewpoint asked for it (made through two viewpoints, compared).
2. **A frame costs what is drawn, not what is held**: counted visits of `gather` equal the draw list (a counter, not a clock), with 0 and with 100,000 pictures in the store.
3. **The second switch does no work**: from the ship's view to a map and back, counted generator calls, mesh jobs and buffer creations are all 0, and the draw list changes in one frame. The first switch builds only the cubes the other view has not (section 3's "map-only").
4. No hole and no double: at every frame the draw list tiles the active cut (the swap planner's property, `plan_swaps`, now across a switch), and a cut still being built shows the coarser picture it replaces and never nothing.
5. The cubes of a viewpoint are the same on both ends of the connection: client and server run `select_around_brick` from the same brick and the same settings, **per viewpoint** (today from the player's pose).
6. Memory: the store is within its budget plus what is pinned or drawn; an eviction never takes a drawn or pinned picture; the order of eviction does not depend on a clock.
7. **No wall clock** (the hard rule on client cosmetics): the warm timeout, the LRU and the order of building are counted in client ticks or frames, never in seconds.
8. Replay: a recording plus the viewpoints the client had draws what the live client drew (viewpoints are recorded as client input, section 6.3).

### 6.3 Network

`SetInterest` and `SetDetail` become **one message that holds a list of viewpoints** (protocol 13; the owner allows breaks before the first release, `CLAUDE.md`): for each an id, a level, a centre brick, a radius for edited bricks, the detail settings, a priority. The server watches the union of the regions, runs the coarse plan once per viewpoint from **the viewpoint** and not the player's pose, sends coarsest first within one byte
credit shared by all of a client's viewpoints (the active one first), and refuses a list whose regions together pass a cap (`maximum_watched_bricks` today for one). The client keeps the edited bricks of every region in the list, and forgets those in none. This also fixes the finding that a detached camera would be sent nodes selected for the player's place. Far-ship tiering already works from the region, and keeps working from the union.
**[belief]** Whether a client may hold a region in a level other than the one it plays in is not decided (section 8, question 9).

### 6.4 What belongs to the camera and what to the player

*Camera (viewpoint)*: the region and detail asked for; the coarse-node plan; the far-ship tier; the view of a zoom; the cut and what is drawn; the atmosphere, fog distance and sky (set for the active camera: the fog radius is the viewpoint's farthest distance); the sun and the sky's brightness as seen from it (a time offset moves them); the up direction and frame; the picking ray for *looking* (the ray for *placing* a block is the body's); the listener, when there is sound.
*Player or ship (not the camera)*: collision, the prefetch of bricks for a body, simulation (which ships are awake: a spectator does not wake them, owner to confirm), edits and reach, which entity the client controls (`set_client_focus`), the saved place.

### 6.5 The three uses

- **Map.** A free viewpoint moved out. Its cut is small beyond 30 km (2,112 nodes at 30 km, 588 at 100 km, 8 beyond 1,000 km) and the far map is nearly free to keep; the cost is the middle (1 to 10 km), where the ship's cut and the map's share most. A zoom is a dolly: the cuts on the way are built ahead (a pyramid's worth for a short distance, rule 2 of section 5), coarsest first. Perspective or orthographic: question 2.
- **A free camera.** A free viewpoint with no body. It needs the plan from the viewpoint (6.3), a rule for who may detach and how far (a game rule: in multiplayer a free camera sees through walls and across the world; `GameLogic` says), and the choice of whether it wakes ships (no). It is the worst case for the server's assignment tree (every cube it looks at is assigned): see the next paragraph.
  `WorldTerrain` assigns a generator to each cube the first time the cube is asked for, and the client's tree grew by about 43,000 nodes for each megametre flown (measured 2026-11-20: 400 MB at 444 Mm). A free camera that flies must not grow the tree it looks at: a viewpoint's region must not assign what is air (the rule that ships' colliders follow since 2026-11-20, `ship-terrain-cost.md`; it changes what "assigned at the first need" means, an owner decision).
- **Ahead along an orbit.** A *predicted* viewpoint: a time offset, and the ghost of the ship (or none). It needs the client to place every grid at the other time, which needs the rails of the bodies on the client (`wide_kepler::predicted_path` exists on the server; **[belief]** the client has no rails and only the poses the server sends). Out of scope for the first phases; the viewpoint carries the offset from the first.

## 7. Phases (tests first, safest piece next, verify, continue)

0. **Understand the observation of section 9** before building on the view: a planet seen from where a player spawns draws nothing in the headless rig, and the same camera position draws 834 meshes the first time and 12,155 after a trip. Either a bug of the view or an artifact of the rig; it decides whether "the up-close terrain is too low" has a cause that is not this spec's.
1. **Resident apart from drawn in the renderer** (the safest and most valuable): a store that `gather` does not walk, `show` and `hide` as moves between the maps, `terrain_version` counted for the draw list only. Tests 2 and 6.1 first; every render test passes unchanged.
2. **One picture store and the pins** (`CubeStates` and the brick states become views of it), with versions and the LRU; test 3 on one viewpoint that moves out and back (generator calls counted).
3. **`Viewpoint`, several views, a shared `may_show` memo**; the session holds a list; roles and the fine radius by role; the occupancy and contact windows told on a switch. Test 4 across a switch.
4. **The protocol** (6.3), with test 5 and the recordings made again.
5. **Detach and the map controls** (a game decides: key, rule), then **tracking cost** for a fast ship measured; then the arena (b) if the buffers' count is the limit.
6. **Time offset and rails on the client** (its own spec).

## 8. Open questions for the owner

1. **What a switch is.** A cut (a key; the view changes in a frame) or a dolly (an animation of about a second), or both? A cut is what asked for instant; a dolly is the cheaper way to zoom.
2. **What a map is.** Perspective only, or also orthographic (a top-down map has a constant scale; the rule of `lod-spec.md` 3.3 would take a pixel size in voxels instead of an angle)? Is it locked to look at the planet, or a free orbit?
3. **Which levels get maps.** Space and the planet (nodes are cheap, 0.5 to 1 ms) yes. The overworld has no nodes today (60 to 375 ms a node with caves, `lod-spec.md` 9): a map of the overworld needs that solved first. Is a map of the overworld wanted?
4. **How many warm viewpoints, and how long.** My default: at most 3, a warm one released after 12,000 client ticks (10 minutes at 20 a second) unless pinned. And does the ship's view keep being **built while you look at the map**, which costs worker time (not frame time) while you fly, or is it only rebuilt on return, coarsest first, in the time that takes (3 s for the surface cut today)?
5. **What "too low in detail" allows** on the first switch: a coarse picture that sharpens in a second, or nothing but the finished picture (a wait)? And after it: any pop-in at all?
6. **Budgets.** A byte budget for the store (my guess: 256 MB of pictures, which holds 10 surface cuts) and for the GPU; the most a frame may spend on a switch (my guess: none beyond flipping the list).
7. **A free camera in multiplayer.** May a player detach, how far, with what shown (far ships, entities)? Who decides: the game's rules, a server setting, or the world? (A free camera sees across the level; the server sends edited bricks to whoever watches them.)
8. **Replays.** The unedited terrain is the client's own, so a free camera in a replay sees the right ground and **only the edits that were recorded in the viewpoints the recording client had**. Is that enough, or must a replay be watchable from anywhere?
9. **Levels.** May a client hold a viewpoint in another level than its body's (a map of space while the body is on the overworld)? Today a client holds one level; others' edited bricks are dropped.
10. **A spectator and ships.** Does a free camera wake frozen and parked ships (my recommendation: no; far ships are drawn from the far tier and the sidecars, `far-nodes-spec.md`)?
11. **Entities in a map.** Only terrain, planets and far ships; players and entities only inside a body viewpoint's region (my default), or markers for ships and players?
12. **The ghost** (6.5): a transparent copy of the ship at the previewed time, a line only, or nothing?

## 9. Observed and not explained

Reported as measured, not as a bug in the game: in the headless rig (`probe_two_views.rs`, `Session::enable_grid_detail` as `Cave::grid_detail()` sets it, the player a walker who `/fly`s and `/tp`s):

- At the spawn (the server puts the player on the planet's surface, brick (0, 1990, 0)) **nothing is drawn for 3 s**: 0 meshes, 1,212 updates with none changed, with one grid known (the planet, with terrain) and the level watched. After the player is moved by `/tp` along the surface 2 bricks at a time, meshes appear and **grow with each step**: 427, 799, 1,118, 1,529.
- The same camera position (16, 64,200, 16, 490 m up) drawn **834 meshes (1.06 MB)** the first time, steady for 25 s (it did not grow); moved one brick, 906. After a trip to a map at 20 km and back, **12,155 meshes (22 MB)**, which is what the rule gives (14,915 nodes, section 3, of which the non-empty ones are those).
  The cut of the first time looks as if its thresholds were about twice the rule's (its level 2 stands where the rule puts level 1), with no overlap and no stall report (`take_stall_report` printed nothing).
- **A lead, not verified**: `MayShowMemo` keeps an answer asked in two rounds running for good, and a planet's `version()` is a constant (`far-nodes-spec.md` 3.5c says so), so an answer given before something was known (a generator, a box) would stay until the camera moves so far that the cube is not asked for. It would fit all of the above. It would be found by asking, at the first update, what `may_show` says for the cubes under the camera.

## 10. Not here

Sound; the user interface of a map (scale bar, labels, icons); a minimap's second render pass (the draw lists allow it; the renderer's second pass and its shadows are not designed); stars as voxels; the server's simulation distances; a map made of anything but the voxels' own nodes (alternative 6).
