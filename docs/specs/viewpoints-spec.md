# Viewpoints: the camera as something that asks for terrain (a draft, 2026-11-21)

Status: **a draft; the owner's first answers are in (section 8, 2026-11-21) and the interview goes on; nothing is built.** It extends `lod-spec.md` (which decides what one viewer draws) to several viewers at once: the ship you fly, a map, a free camera, a preview of an
orbit. Read with `far-nodes-spec.md` (far ships and the server's nodes), `planet-spec.md`, `protocol-and-replay.md` and `design-decisions.md`. Marks as in `lod-spec.md`: **[code]** is what I read in the code,
**[run]** what a program measured (the programs are in `history/spikes/viewpoints/`, written against the code of commit `aa419ee`, and are not kept compiling), **[belief]** what I did not check. The owner's decisions are in section 8; every other choice here is mine and marked as a recommendation until the owner has answered what is open there.

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
- **[code] A picture goes on the screen at most 64 to an update** (`SHOWN_PER_UPDATE`, `lod.rs`), each a new GPU buffer (`VoxelRenderer::gpu_brick`: one `wgpu::Buffer` for the whole-voxel quads and one for the detailed ones). 12,000 nodes need about 190 updates: 3.1 s at 60 frames a second even when every picture is ready. **[run, 2026-11-21] The cap is not what limits a cold build** (section 9a): with caps of 64, 128 and 256 a view from 264 km back to the surface took 3.05, 2.96 and 2.94 s, and counting only the pictures that have something in them instead of every cube changed nothing; it stays at 64. It will matter when a switch shows pictures that are already held (a move, not an upload).
- **[code, run in phase 0] A node is replaced only when everything that replaces it is ready.** `plan_swaps` makes a group of a cube and the cubes of the other set it overlaps, and a group changes only when every cube that comes in is ready. After a big move (a teleport, a switch, a fast flight) the old picture is a few huge nodes, each holding thousands of the new cubes, so
  **the old picture stays until the whole new cut under it is built**: from 264 km up back to the surface the view wanted 15,147 cubes, showed the 24 old nodes for 3.8 s (a few seconds, 2.5 s at least, with nothing changing), and then 12,449 meshes came in a few bursts. This is not coarse first. It is what makes the owner's 0.25 s unreachable by speed alone, and it was a change of the planner (6.2, item 11, **built 2026-11-21**: from 264 km up back to the surface the picture is of nodes of level 7 and finer from 0.26 s on, and the finest level on the screen goes down a level at a time; the build is still 3.6 s and costs 23% more uploads).
- **[code] The renderer draws everything it holds and does not choose** (`upload_coarse_node` says so). Every frame `VoxelRenderer::gather` walks `bricks`, `coarse_nodes`, `ship_bricks` and `ship_coarse_nodes`, makes a `Drawable` of each (an exact offset from the camera
  in 128-bit integers), and the culling and the shadow planning loop over all of them. Every upload and removal also counts up `terrain_version`, which the shadow maps watch. **Changed in phase 1 (2026-11-21):** the four maps are `MeshStore`s with a list that is drawn and a list that is hidden (held on the graphics card and nothing else); `gather` walks the drawn list only, and `terrain_version` counts a change of the draw list only. The text above is what the renderer was.
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
away from the ground. Cuts were worked out with no session (`probe_pure_cuts.rs`); mesh sizes and times are from a real headless client (`probe_two_views.rs`: server, player, `update_view`, release; the client's default of one worker thread fewer than the cores, **11** on this machine, which this section first said was three: corrected in section 9a).

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

**Memory (real pictures, whole-voxel quads of 8 bytes, detailed 16).** The ship's cut at 490 m, settled: 12,449 non-empty meshes of the planet, **23.2 MB** (levels 1 to 7: 3,184 / 2,771 / 2,218 / 2,007 / 1,659 / 575 / 35 meshes, 1.5 to 3.5 KB each; 83% of the 14,915 nodes of the cut had something to see). A map at 20 km up: 2,686 meshes, 8.1 MB; at 66 km up of the surface: 7,319 meshes, 16.3 MB. A map from 200 km: 228 meshes, 0.8 MB; from 2,000 km: 8 meshes, 23 KB. The maps between were **not** run for size: from the per-level sizes of the real cut (1.5 KB at level 1 to 4.7 KB at level 8), a map's own cut is
about 21 MB at 1 km (81% of its nodes are the ship's), 17 MB at 3 km, 13 MB at 10 km, 7 MB at 30 km and under 3 MB at 100 km (an estimate from sizes, about a fifth too high, since 83% of the cut's cubes had a mesh).
Level 0 near ground is about 11 KB a brick (spawn walk: 130 bricks, 1.4 MB). These are meshes only, not wgpu's own overhead or a second copy on the CPU, and the planet's nodes have no cave walls (a node of the overworld's caves has 25 times the quads, `lod-spec.md` 9a): **[belief]** the real figure with cave geometry in far nodes would be many times larger.

**What it costs to build one cold.** From a map back to the ship's cut at 490 m: 12,011 uploads (the other 144 were shared) in 3.2 to 3.5 s (3.4 thousand a second: **the rate the job feed allowed, not the threads' own**, and on 11 threads, not three: section 9a); the first visit to a map at 20 km up from the ground: 2,248 uploads in 0.6 s; at 200 km: 228 in 0.05 s. **From a map at 264 km back to the surface: 12,449 uploads in 3.8 s, and nothing on the screen but the 24 giant nodes of the map until the end** (the next paragraph's cause is in section 2; after the progressive swap and the job feed's fix, 3.1 s and a picture of 383 meshes at 0.26 s, sections 9 and 9a). Uploading a mesh (one buffer) costs 12 to 22 microseconds on the main thread, by adapter (iGPU Vulkan 12, RTX 3090 DX12 14 to 22, iGPU DX12 10 to 23):
12,000 are 0.15 to 0.27 s of the main thread, before the cap of 64 an update.

**What it costs a frame to hold meshes nobody sees** (`probe_held_meshes.rs`: N coarse nodes behind the camera, all culled; the frame is the whole offscreen call at 64 by 64, median of 15): about **0.1 ms for every 1,000 held**, on each of the three adapters, shadows on or off.

| held, unseen | 0 | 1,000 | 5,000 | 20,000 | 25,000 |
|---|---|---|---|---|---|
| frame, iGPU Vulkan | 0.16 ms | 0.17 | 0.53 | 2.28 | 2.09 |
| frame, RTX 3090 DX12 | 0.17 | 0.22 | 0.59 | 2.50 | 2.84 |
| frame, iGPU DX12 | 0.13 | 0.17 | 0.56 | 2.72 | 2.74 |

(The 25,000 row is 50,000 uploads of which 24,700 replaced others: my lattice folded. Treat the last column as 25,000.) So **keeping both cuts in the renderer's maps, as it was built, costs about 2 ms at 20,000 meshes: 12% of a 60 fps frame**, for nothing drawn. That is the number behind "no noticeable frame cost". **After phase 1, the same probe with the meshes hidden** (`probe_hidden_meshes.rs`, all four adapters, shadows on and off, run again in the same session as the first, whose rows were 1.6 to 2.5 ms at 20,000 and 1.9 to 3.4 ms at 50,000): **0.12 to 0.29 ms at 20,000 and 0.13 to 0.30 ms at 50,000 held, against 0.08 to 0.28 ms with none held**: nearly all of the cost of holding is gone (the largest step from 0 to 50,000 held is 0.13 ms, RTX 3090 DX12 without shadows, 0.17 to 0.30, and 0.08 ms at 20,000; the next is 0.06 ms; whether that is the hash tables' growth, the driver's tracking of its buffers or the run to run noise of the adapters was not told apart). Uploading is as it was (10 to 25 us a mesh).

**What it takes to keep the ship's cut current while the ship flies** (`probe_pure_cuts.rs`: a viewer circling the planet at a constant height; the cubes of the cut one second later that the cut now lacks, the mean of 12 seconds; the bricks are the ball's and nearly all air):

| height | speed | nodes entering each second | bricks entering each second |
|---|---|---|---|
| 490 m | 100 m/s | 570 | 1,514 |
| | 1,000 m/s | 5,749 | 8,552 (all of the ball) |
| | 7,800 m/s | 15,468 | 8,552 |
| 20 km | 100 m/s | 6 | 1,508 |
| | 1,000 m/s | 49 | 8,552 |
| | 7,800 m/s | 402 | 8,552 |

The real client built 3.4 thousand nodes a second while the job feed limited it, and **about 4.9 thousand (15,000 nodes in 3.1 s on 11 threads) once it did not** (section 9a; fewer cores build fewer), so a cut can be kept complete up to about 850 m/s near the ground and at any speed from 20 km up; faster, it lags, which is what the active view shows at that speed too (the cost is the same).

## 4. The idea reviewed: several detail levels resident, one drawn

**What it is.** Keep the pictures of every cut that matters (the ship's, the map's) resident, and draw only the cut of the active view. The pictures for a cube do not depend on the viewer (a node's mesh is made from its cells with no neighbours, `mesh_coarse_node(&node, &[None; 6])`; a brick's from its voxels and its six neighbours; **[belief]**
nothing in a picture depends on the camera: textures near the viewer, `lod-spec.md` decision 6, are not built, and if they are chosen when the picture is made they would split the pictures by viewer), so one store of pictures keyed `(grid, cube)` can serve any number of views.

**Verdict: yes, with one correction to the shape.** The data say the idea is cheaper than it sounds and a literal reading of it is not enough:

1. **It is cheap in memory.** Keeping the ship's cut costs at most 23 MB (about 8 MB high up). A map's own cut is up to about 21 MB (at 1 km, mostly the ship's nodes) and under 3 MB beyond 100 km; what a map adds to the ship's cut is the map-only column of section 3: 2,000 to 2,700 nodes, about 4 to 10 MB. The union of the ship's cut and any map is about 32 MB at the most at Cave's planet, shared nodes counted once. Memory is not what decides.
2. **It is not free in the frame as the renderer is built**: 2 ms at 20,000 meshes, because the renderer walks what it holds. Resident must be **separate from drawn**: a store that nothing walks every frame, and a draw list that is only the active cut. That split is the change that makes the idea work, and nothing else in this document is as valuable.
3. **It is not instant unless a switch is O(1) per picture.** A switch that creates buffers is capped at 64 a frame and costs 12 to 22 microseconds each: 3 s for the ship's cut. A switch has to flip the draw list (a key moving between two maps, or one pointer), with the GPU buffers already made.
4. **"Several levels of the same area" is not the shape of the saving.** A pyramid with every level over an area holds more than two cuts; the two cuts share only the far field, and the map's near-ship nodes are ancestors that no cut of the ship's holds (section 3). What is kept is not a pyramid but **the union of the cuts of the viewpoints that are kept warm**.
5. **Keeping a cut warm is not keeping it still.** A ship that flies while you look at a map moves on: the cut you will return to is the cut at the ship's place **then**. So a warm viewpoint has to be tracked (its cut worked out again as the ship moves, built in the background), at the cost the active view pays now in worker time and none in frame time. At orbital speeds in empty space the cut is nearly nothing (air is not asked for, `ship-terrain-cost.md`); near a planet's face it is 570 nodes a second at 100 m/s, 5,749 at 1 km/s and 15,468 at 7.8 km/s (section 3: the build rate is 3,400, so a warm cut keeps up to about 600 m/s near a surface and at any speed from 20 km up). The order of building (coarsest first) makes the worst case a coarse picture on return and never a hole.
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

**What is kept** (choices inside 1): (a) CPU mesh and GPU buffer both, instant and the most memory; (b) CPU mesh only, uploaded into one arena buffer on a switch, a bulk copy that I **believe** is a few milliseconds for 23 MB and did not measure, half the memory; (c) the node's cells, re-meshed on a switch: about 1.3 ms for a mixed node, so no;
(d) nothing: today. Recommendation: (a) for the active and warm cuts, (b) for what an LRU keeps beyond them.

## 6. The design I recommend (a draft)

### 6.1 Terms

- **Viewpoint**: what asks for terrain. A place (a level and a position), an orientation, a **projection**, detail settings (`cell_pixels`, the pixel size, the farthest distance, an optional zoom cone), a **fine radius** (the ball of bricks), a **time offset** (0 now; for an orbit preview, ahead), and a **role**:
  *body* (the player's eyes: a fine ball, bricks and edits, follows the body or ship it is aboard); *free* (no body, so the fine ball is as small as the altitude needs and pruned by what may show, since nobody builds from it), with two ways of being steered: **orbit**, locked on a *focus* (the map, and the
  third-person camera at a short distance), and **fly**, the spectator's camera, which looks where the player points; *predicted* (a free viewpoint at another time).
- **Focus**: the thing an orbit camera is locked on: a ship, a planet, a star (decision 7). The viewpoint's place is the focus's place plus a distance along two angles, and it follows the focus, so a camera locked on a planet keeps its place in the planet's own axes and the planet's cut
  does not change while the planet moves on its rail, and a camera locked on a ship moves over the planet as the ship does.
- **Projection** (owner, 2026-11-21): *perspective* everywhere; in the levels with planets in them (`mini_cosmos` and the levels to come) perspective only; in the overworld *orthographic* is available and is the default.
- **Active** viewpoints are drawn this frame (one per window; a minimap would be a second). **Warm** ones are not drawn and are kept in one of two ways, and there are at most three warm and the first design uses two: *tracked* (the cut is worked out again as the viewpoint moves and building goes on at once, as for the active one) and
  *lazy* (worked out again about once a second, coarsest first, with a share of the workers). The body's view is **tracked whenever it is not the active one** and the body exists ("quite warm", owner). The map's is tracked for a window after it was last active, lazy for a longer one, then released (my defaults: 600 client ticks, 30 s at
  20 ticks a second, then 12,000, 10 minutes; open question e), which is what makes switching back and forth fast and lets the map take a second or two to catch up after the ship has moved a lot.
- **Picture store**: the pictures (a CPU mesh, a GPU buffer, a version) keyed `(grid, cube)`, with the viewpoints that want each; unwanted ones go to an LRU under a byte budget; a picture whose data version changed is made again when it is next wanted, not before. **Draw list**: the cubes of the active viewpoints' cuts, the only thing
  the renderer walks. Building goes by priority: the active viewpoint's, then a tracked one's, then a lazy one's, each coarsest first.

### 6.2 What must hold (each is a test, apart from 9)

1. A picture is the same bytes whichever viewpoint asked for it (made through two viewpoints, compared).
2. **A frame costs what is drawn, not what is held**: counted visits of `gather` equal the draw list (a counter, not a clock), with 0 and with 100,000 pictures in the store. *(Built, phase 1: `tests/gpu_rendering/resident.rs`, `a_frame_visits_what_is_drawn_and_not_what_is_held`, with 8,000 meshes of the four kinds hidden and then shown, on every adapter; 100,000 was not tried.)*
3. **The second switch does no work**: from the ship's view to a map and back, counted generator calls, mesh jobs and buffer creations are all 0, and the draw list changes in one frame. *(Built for the nodes of one view, phase 2a, 9c: on the ground and 400 bricks up and back, 645 nodes were made of the 9,056 the ground took, 92 pictures uploaded and 6,696 shown from hold; bricks and a second view are not held yet, so the counts are not 0.)* The first switch builds only the cubes the other view has not (section 3's "map-only").
4. No hole and no double: at every frame the draw list tiles the active cut (the swap planner's property, `plan_swaps`, now across a switch), and a cut still being built shows the coarser picture it replaces and never nothing.
5. The cubes of a viewpoint are the same on both ends of the connection: client and server run the selection from the same inputs, **per viewpoint** (today from the player's pose).
6. Memory: the store is within its budget plus what is pinned or drawn; an eviction never takes a drawn or pinned picture; the order of eviction does not depend on a clock.
7. **No wall clock** (the hard rule on client cosmetics): the windows of warmth, the LRU and the order of building are counted in client ticks or frames, never in seconds.
8. Replay: a recording plus the viewpoints the client had draws what the live client drew (viewpoints are recorded as client input, section 6.3).
9. **The owner's times (2026-11-21)**, as targets the probes measure in seconds and the tests check as counted work: switching to the **ship's view**: a coarse picture with no hole in the first frame and the draw list equal to the wanted cut **within 0.25 s** (about 15 frames); to the **map**: coarse first, the whole of it **within 4 s at worst**
   (the first time, or after the ship has moved a lot), and **within 0.25 s when switching back and forth** (inside the tracked window). A wall clock may not decide a test, so a test counts jobs, flips and updates and waits for a condition with a deadline (`tick_until`).
10. **A viewpoint changes no state of the server's world**: with a free viewpoint watching anywhere for any number of ticks, the world's saved bytes, the generator assignments, which ships are awake or parked and the contents of the generated-brick cache that simulation uses are the same as with no such viewpoint (section 6.3).
11. **A refinement goes through intermediate cuts** (found in phase 0; **built 2026-11-21**, `coarse_cells::plan_progressive_swaps`, the tests are in `coarse_cells/tests/progressive_swaps.rs` and `world_view/tests/lod.rs`): when the wanted cut is far finer than what is shown, the view builds and shows it **level by level**, each step a cut that tiles the area (the wanted cut with every cube above a level L replaced by its ancestor), L going down, so that what is on the screen is never more than one level coarser than what has been built. The extra cost is the ancestors, about a third more nodes of a surface (a quarter, then a sixteenth...), and many of them are already in the store (the map's). Test: a view jumped from a far cut to a near one has, after every update, a picture that tiles the cut and is finer than the last.

### 6.3 Network, and what "never affects the simulation" asks of the server

`SetInterest` and `SetDetail` become **one message that holds a list of viewpoints** (protocol 13; the owner allows breaks before the first release, `CLAUDE.md`): for each an id, a level, a centre brick, a radius for edited bricks, the projection and detail settings, a priority. The server watches the union of the regions, runs the coarse
plan once per viewpoint from **the viewpoint** and not the player's pose, sends coarsest first within one byte credit shared by all of a client's viewpoints (the active one first), and refuses a list whose regions together pass a cap (`maximum_watched_bricks` today for one). The client keeps the edited bricks of every region in the list and forgets those in
none. Far-ship tiering already works from the region and keeps working from the union.

**A viewpoint with no body is read-only on the server (owner: "never affect simulation").** What I found that it would touch today **[code]**:
- *The saved world.* A watch is answered by `Server::assignments_covering`, which **assigns** the level's current default generator to every cube round the region that has none (`assign_unassigned`, `server.rs`), and the assignments are saved in the world file. A body that watches does that today by design ("assigned at the first need"). A free camera that flies must not: it would write
  the places it looked at into the world. For a free viewpoint the server answers with the default for an unassigned cube and writes nothing. Whether the client then keeps those answers is its own memory (the client's tree grew by about 43,000 nodes for each megametre flown, 400 MB at 444 Mm, measured 2026-11-20; the client can forget default answers outside every region).
- *The cache simulation uses.* `generated_brick` and the cache of 256 generated bricks are shared with the colliders and the walkers; a camera that asks for bricks and hints all over a level would push out what the tick needs. A free viewpoint's server work (coarse plans, overrides) is on its own workers and its own bounded cache.
- *Which ships are awake.* Awake, frozen and parked follow the players' bodies, not their cameras (`grid_lifecycle`): a free camera wakes nothing, brings no ship in from its file, and so sees a ship that is **frozen in memory** as it is (near, with its bricks, if it meets the region) and a ship that is **parked in a file** only as a far ship, from the nodes of its sidecar (`far-nodes-spec.md`),
  at any distance: **not up close** (decision 9).
- *Everything else* a viewpoint causes is replication (messages to that client): edited bricks, nodes, far ships. Those change nothing in the world.

### 6.4 What belongs to the camera and what to the player

*Camera (viewpoint)*: the region and detail asked for; the coarse-node plan; the far-ship tier; the zoom cone; the cut and what is drawn; the projection; the atmosphere, fog distance and sky (set for the active camera: the fog radius is the viewpoint's farthest distance; none in an orthographic map); the sun and the sky's brightness as seen
from it (a time offset moves them); the up direction and frame; the picking ray for *looking* (the ray for *placing* a block is the body's); the listener, when there is sound.
*Player or ship (not the camera)*: collision, the prefetch of bricks for a body, simulation (which ships are awake, section 6.3), edits and reach, which entity the client controls (`set_client_focus`), the saved place.

### 6.5 The uses

- **The map** is an orbit viewpoint locked on a focus. Perspective in a level with planets: the rule of `lod-spec.md` 3.3 as built, with a fine radius of 0 (the ball of 8,552 bricks that every viewer has today is pure waste for a camera nowhere near ground), and the cuts of section 3: nearly free to keep beyond 30 km. A zoom is a dolly: the cuts on the way are built ahead, coarsest first.
  **Orthographic (the overworld's default) needs a rule of its own** **[belief, not built, not run]**: in an orthographic view a pixel is the same size in voxels at every distance, so "fine enough" cannot depend on the distance from the viewer, and the cut is the cubes of **one level**, `2^n` voxels across being about `cell_pixels` pixels, over the box the picture sees and that may hold something. A picture of
  1920 by 1080 pixels at 3 pixels a cell is 20 by 11 nodes across the ground: **a few hundred nodes at any zoom**, against 15,000 for the perspective view near the ground. Two consequences: a zoom changes the level of the whole picture at once (a pop, not the local, gradual change of a perspective cut), so the next level is built ahead while zooming and the swap is one group (or faded); and the cost per
  node of the overworld's generator decides whether the 4 s hold (section 6.6, A).
- **The free (fly) camera.** A free viewpoint steered by the player, with no focus and no body, anywhere in any level (decision 8): the plan from the viewpoint (6.3), the server's read-only rule, and the pruned fine ball. It is the worst case for the assignment tree of the client and, unfixed, of the server (6.3).
- **Ahead along an orbit.** A *predicted* viewpoint: a time offset, and the ghost of the ship (or none, open question d). It needs the client to place every grid at the other time, which needs the rails of the bodies on the client (`wide_kepler::predicted_path` exists on the server; **[belief]** the client has no rails and only the poses the server sends). Out of scope for the first phases; the
  viewpoint carries the offset from the first.

### 6.6 What the answers of 2026-11-21 lead to (I checked each)

- **A. The overworld's map is blocked by what a coarse node costs there.** The overworld has no coarse nodes at all today (`Cave::level_detail` is none for it). **[code, and `lod-spec.md` 9a, measured 2026-10-23]** a node of its hills takes 18 to 43 ms and with its sheet caves 94 to 201 ms; the column sampler that would take 0.3 ms (a far field of "the outer skin of the world", `lod-spec.md` 9a item 2) was
  never built for it (the planet has its own, `planet_node.rs`, 0.5 to 1 ms). **[belief, from those figures]** a few hundred mixed nodes make about 2 to 10 s on three threads and a third of that on 11 (which this section first assumed were three: section 9a), so the 4 s of the map is **missed without the far field on a machine with few cores, and borderline on this one** (a few hundred nodes at 18 to 43 ms, or at 94 to 201 ms with caves, over 11 threads: 0.5 to 3 s, **[belief]** and the first of them wait for the generator's own setup), and the caves that a far field leaves out are invisible from above, though cave mouths and overhangs would not show: decision 10.
- **B. Orthographic is a renderer feature that does not exist.** **[code]** `Camera` has one projection, a vertical field of view with reverse-Z and an infinite far plane; fog, shadow cascades and the culling are by distance from the camera. An orthographic camera needs a projection and a depth range from the box, culling by the box, and a decision for fog, sky and shadows (open question a). The selection (6.5) is a new function beside
  `select_around_brick`.
- **C. The ship's view can be kept warm up to a speed, and no further** (measured, section 3): keeping the cut complete takes 570 new nodes a second at 100 m/s near the ground (the real client builds about 4,900 on 11 threads, first measured as 3,400 for the reason in section 9a), 5,749 at 1 km/s and 15,468 at 7.8 km/s, and 6, 49 and 402 from 20 km up. So "quite warm" holds to about 850 m/s near a surface (less on a machine with fewer cores) and at any speed high up; faster, the warm cut lags, and so does the active one
  at that speed (the same cost). Coarse first makes the lag a coarser picture and not a hole.
- **D. A star is not an object today.** **[code]** the sun is a direction worked out from the world's clock (`sun-spec.md`), "stars as voxels" are listed under *not here* in `lod-spec.md`. A focus needs a place: decision 7.
- **E. "Wherever they want" with "never affect the simulation" means read-only service** (6.3): the world's saved generator assignments change with every watch today.
- **F. In multiplayer a free camera sees the edits of the whole world and every level.** That follows from the answer and is accepted as the answer; the viewpoint list in the protocol is where a game could one day limit it, and none is built now.

## 7. Phases (tests first, safest piece next, verify, continue)

0. **Understand the observation of section 9. Done (2026-11-21): a probe artifact, and one real finding** (the swap is all or nothing, section 2): its fix is item 11 of 6.2, in phase 3 (the planner), and was built first, since it changes no data (see `build-log.md`, "Viewpoints, phase 0 and the progressive swap").
1. **Resident apart from drawn in the renderer** (the safest and most valuable): a store that `gather` does not walk, `show` and `hide` as moves between the maps, `terrain_version` counted for the draw list only. Tests 2 and 6.1 first; every render test passes unchanged. **Built 2026-11-21** (`build-log.md`, "Viewpoints, phase 1"): `renderer::HeldMesh` names a mesh (a brick or a node of the terrain or of a grid), `VoxelRenderer::{hide_mesh, show_mesh, upload_hidden_mesh, is_mesh_hidden, hidden_mesh_count}`; nothing in the game calls them yet (phase 2 does: the picture store decides what is hidden, and what leaves the graphics card).
2. **One picture store and the pins** (`CubeStates` and the brick states become views of it), with versions and the LRU; test 3 on one viewpoint that moves out and back (generator calls counted). **Split in two (2026-11-21), and 2a is built** (9c): the pictures of the *nodes* of one view are held when they leave the cut, hidden in the renderer, under a byte budget, with versions and an oldest-first order; the pin of a view's own cut is "wanted". **2b, not built:** the bricks (level 0: the mesher's, with voxels and neighbours), a picture store shared by several views with a pin for each (it needs the viewpoints of phase 3), the CPU-only tier (b of section 5) and the identical-bytes test (6.2, 1).
3. **`Viewpoint`, several views, a shared `may_show` memo, and the progressive swap (6.2, item 11)**; the session holds a list; roles, the fine radius by role, the tracked and lazy windows; the occupancy and contact windows told on a switch. Test 4 across a switch; the times of 9 measured by a probe.
4. **The protocol** (6.3) with tests 5 and 10 and the recordings made again; the server's read-only answers for a viewpoint with no body.
5. **Focus and the orbit and fly controls** (a game decides the keys); the focus registry (decision 7); the map in levels with planets.
6. **The overworld's map**, in this order: the far field for the overworld's generator (A), an orthographic projection in the renderer (B), the orthographic selection (6.5), the level's rule for the projection (owner: orthographic default in the overworld, perspective only in levels with planets).
7. **Time offset and rails on the client** (its own spec).

## 8. The owner's answers (2026-11-21) and what is still open

**Decided** (the first six of the twelve questions of the first draft; the answers are quoted in part):

1. **A switch is both**: a cut (a key; the view changes in a frame) and a dolly (to zoom).
2. **The map is locked on the currently focused object**, which can be the ship, a planet or a star. **Perspective only** in `mini_cosmos` and the other levels with planets in them; **orthographic available and the default** in the overworld.
3. *(Not asked in words; the answer to 2 settles it.)* **The overworld gets a map** (orthographic): see 6.6 A for what that needs.
4. **The ship's view is kept quite warm**; the map may take a second or two to catch up after the ship has moved a lot. **At most 3 warm viewpoints, 2 used in the first design.**
5. **Coarse first.** The ship's view sharpens **within 0.25 s**; the map **within 4 s at worst**, and **within 0.25 s when switching back and forth** (6.2 item 9).
6. **A player can detach and send the camera wherever they want, and it never affects the simulation** (6.3; 6.6 E and F).

**Decided by taking my picks** (the owner, 2026-11-21, on questions 1 to 4 of the second draft):

7. **A focus is any grid or entity with a pose** (ships, planets, moons: nothing new). A star becomes a grid on a rail, with an emissive block, when stars are built; until then the sun is not focusable (6.6 D).
8. **Both ways of steering the free camera**: *orbit* (the map, locked on a focus) and *fly* (the spectator's camera, looking where the player points); and "anywhere" is **any level**.
9. **A free camera sees a ship that is frozen in memory near (with its bricks) and a ship parked in a file only far** (the nodes of its sidecar, nothing finer than level 1), since nothing may be loaded for a camera. If the blockiness of a parked ship close up is noticed, the way out is a read-only copy loaded for the view without waking the ship (not built, not decided).
10. **Far maps, and the overworld's orthographic map, are made from the outer skin of the ground**, the hills without caves (`lod-spec.md` 9a): cave mouths and overhangs vanish from nodes; the bricks near the ship still have them (6.6 A).

**Still open:**

a. Orthographic: may it tilt (orbit angles) or only look straight down? What does it show of lighting: flat colours, or the sun and its shadows, and any fog or sky? [May tilt; the sun and shadows on; no fog and no sky haze.]
b. The map's zoom range: the nearest distance (the size of the focus?) and the farthest (the whole universe, `great-rivers-spec.md`)? [Nearest: a ship's length; farthest: the largest node.]
c. Replays: a free camera in a replay sees the right unedited ground (the client's own) and only the edits that were recorded in the recording client's viewpoints. Enough, or must a replay be watchable from anywhere? [Enough.]
d. Entities in a map (only terrain, planets and far ships, and players only inside a body viewpoint's region; or markers) and the ghost of the ship on a predicted orbit (a transparent copy, a line, nothing). [Terrain, planets and far ships; a line.]
e. The windows of warmth of 6.1 (tracked 30 s, lazy 10 minutes, counted in ticks) and a byte budget for the store (256 MB of pictures holds about ten surface cuts). [As written.]

## 9. What phase 0 found (2026-11-21)

The first draft of this section reported an unexplained observation: in the headless rig (`probe_two_views.rs`) a planet seen from the spawn drew nothing, the same camera position drew 834 meshes the first time and 12,155 after a trip away and back, and the picture grew with each step walked. **That was my probe, not the game.** Resolved by instrumenting the view (temporary prints, since removed):

- The probe built its picture of what the renderer holds from the `MeshUpdate`s of `update_view`, and **started after `Rig::settle`**, which runs `Player::settle` twice and throws the updates away: the first picture of the world (15,823 cubes, 12,449 meshes of the planet) was made in them. The "834" was only what changed since, and a screen that holds only deltas grows as you walk. With the updates kept (`join_settle`) the planet has 13,128 meshes at the spawn, 12,449 at 490 m the first time and every time after a trip, and 12,328 after a nudge of one brick; a walk of 8 bricks moves the spawn's by 1%. At the spawn the view did draw: 15,823 wanted, 15,823 shown, in about 500 updates (49 a update: the cap of 64).
- **The lead (`MayShowMemo` keeping a stale answer) is refuted**: at each reselect the memoised answers were compared with fresh ones, and none disagreed (0 of 33,030 asked at the surface); the cut is what the rule gives (15,147 cubes from 490 m, of which 14,915 nodes in the pure model).
- **One real finding**, in section 2: the swap is all or nothing per old node. From 264 km up back to the surface the picture was the 24 old nodes for 3.8 s, with nothing changing for the first 2.5 s at least (which is what the first probe took for a stall). The planner needs intermediate cuts (6.2, item 11).
- The numbers of section 3 that came from the incomplete screen (a map at 20 km, 66 km, the surface) are corrected; the pure-model tables did not use it.

## 9a. What limits a cold build: the job feed, not the cap (2026-11-21)

The owner asked for the cap of 64 pictures an update to be raised. It was measured first, **[run]** in a release build with the probe's updates paced at 60 a second (`PROBE_FRAME_MS`; the first probe called `update_view` as fast as it could, which is not what a game does), a view from 264 km up back to the surface, on the client's 11 worker threads:

| the cap (counted in cubes added, as built / in pictures uploaded) | 64 / - | - / 64 | - / 256 | - / 1,024 | none |
|---|---|---|---|---|---|
| seconds until the last change | 7.49 | 7.42 | 7.41 | 7.41 | 7.58 |

The cap was reached in 132 of 944 updates (counted in cubes) and 79 (in pictures), and removing it changed nothing. **What took 7.4 s, where the unpaced probe had taken 3.6, was how many nodes the threads were handed:** `LodView::start_jobs` hands out work only when the view is updated, and held 4 nodes a thread, so 11 threads were given 44 nodes a frame: 2,600 a second at 60 frames a second where they can do about 5,000. At 4 / 8 / 16 / 32 / 128 nodes a thread: 7.54 / 4.16 / 3.20 / 3.25 / 3.21 s.

**Built: `job_feed::jobs_to_hold`**, not a bigger constant. A count of nodes is the wrong unit when a node's cost varies by a factor of 200 (a node of the planet about 1 ms, of hills 18 to 43 ms, of hills with caves 94 to 201 ms): 32 queued for each thread is seconds of work for the slow ones, which cannot be taken back or put in another order when the view moves. The rule holds twice what the threads finished since the last update, and one a thread, between 4 and 64 a thread: it doubles while the threads run dry and stays at the old 4 for slow nodes, by counting alone (no clock). **Result: 3.10 s at 60 frames a second, 3.12 s at 30, 3.06 s unpaced** (7.4, -, 3.6 before), uploads 13,362 where there were 15,271 and removals 1,141 where there were 3,050 (a build that goes faster shows fewer pictures on the way). With the cap at 64 / 128 / 256 on the new rule: 3.05 / 2.96 / 2.94 s: **the cap stays at 64** (a hitch of 1.4 ms in the worst frame, 12 to 22 us a picture) until a switch shows pictures that are held already, which are moves and not uploads.

**Corrections to what this document and the build log said before.** (1) The probe's client has **11 worker threads** (`Session::new`: the cores less one), not three as I wrote in sections 3 and 6.6. (2) The "3.4 thousand nodes a second" was **the job feed's rate** (about 44 nodes an update at the probe's update rate) and not the threads' own: it is about 4.9 thousand with 11 threads, which moves the speed to which a ship's cut can be kept complete from 600 to about 850 m/s (6.6 C), and makes the overworld's map (6.6 A) borderline on this machine and missed on one with few cores. (3) What the 3.1 s is bound by is answered in 9b.

## 9b. What bounds the 3.1 s: the CPU, not the code around it (2026-11-21)

**[run]** with counters in the node threads (`docs/history/spikes/viewpoints/probe_job_bound.rs` and `job_bound_counters.patch`), release, updates paced at 60 a second, the view from 264 km up back to the surface, on a machine with 6 cores and 12 logical processors:

| node threads | seconds | speed-up over one | a node costs (generate + mesh) | threads busy |
|---|---|---|---|---|
| 1 | 19.7 | 1.0 | 1.07 ms (0.69 + 0.38) | 1.0 |
| 3 | 6.9 | 2.9 | 1.12 ms | 2.9 |
| 6 | 4.3 | 4.6 | 1.29 ms | 5.5 |
| 11 (the client's) | 3.25 | 6.1 | 1.81 ms (1.14 + 0.67) | 10.0 |

- **The threads are the limit, and they are not short of work.** With 11 they waited for a job for 0.04 s in the whole build, and were busy 10.0 threads of 11: 32 s of processor time in 3.2 s. The job feed, the cap and the one lock the threads take jobs from are not what bounds it (the lock's wait is that 0.04 s). The main thread is not either: `update_view` took 1.7 s of the 5.8 s of the run, a mean of 5.2 ms a call at 16.7 ms a frame, and the threads never ran dry while it did.
- **Why 11 threads are 6 times one and not 11:** the cost of one node rises with the number of threads, in generating and meshing alike (1.65 times for generating and 1.76 for meshing, from 1 thread to 11), which is what a machine with 6 cores whose second hardware thread adds a part of a core, and a clock that falls when all cores are busy, would do. **This is read from the curve, not shown:** a cost that rises evenly in two different phases is not a lock's or the allocator's signature, which would show in the phase that allocates most, but no test here told processor sharing from memory bandwidth or the allocator.
- **What the nodes are:** 18,112 were made on the way back, of which 12,449 are the cut at the surface (69 percent), 2,841 came out empty (16 percent, asked and found to have nothing), and 2,822 were steps on the way (16 percent: the progressive swap of 3716f27 makes the nodes between a giant and what replaces it, and these were drawn and replaced). By level, made / empty: 1: 4,090 / 906; 2: 3,948 / 664; 3: 3,316 / 531; 4: 2,824 / 358; 5: 2,348 / 219; 6: 1,150 / 131; 7: 336 / 32; 8: 100 / 0. The steps are the price of showing a coarse cut at 0.26 s instead of the old giants for 3.8 s: about a sixth of the 3.1 s (about 0.5 s) bought that.
- **What this means.** The 3.1 s is the cost of making 18,000 nodes again, and the 12,449 of them that are the cut at the surface **were made, drawn and thrown away a moment before, on the way up**. The cost is not reduced by tuning the feed (it is at the machine's rate: about 5,600 nodes a second, empty ones included, on 6 cores) but by making fewer nodes, which is what a warm view (phase 2: nodes that leave the cut are hidden, not dropped) is: coming back to the cut that was left would be swaps of nodes held, the 0.25 s of 8. A cut made cold, the first time or after a far jump, stays a cost of about 1 ms of a core for each node (generating 0.69 of it): half again as much for each node and more with more threads on this machine, and a machine with fewer cores takes longer (one thread: 19.7 s).
- **Not measured:** what a game that renders and runs a server on the same 12 logical processors loses to 11 busy node threads for 3 s (6 threads took 4.3 s against 3.25, so leaving half of them free costs a third more time to the cut; which is better for the frame rate is not known without the renderer in the loop); how much of the 1.07 ms a node could be saved by cheaper generating (0.69 ms of it, 64 percent); whether the empty nodes (16 percent) could be known to be empty without generating them.

## 9c. Phase 2a: the nodes of a view are held, and the 3.1 s is 0.27 s (2026-11-21)

**Built:** `LodView::hold_hidden_pictures(budget)` (off, 0, by default, so every view that does not ask is as it was); `CubeStates` has a fourth state of a node's picture, **held** (not shown, ready, charged to the budget by the bytes of its mesh and at least 64 so that the records are bounded, with an order number that counts up and never repeats: no clock); `MeshUpdate::{HideCoarse, ShowCoarse, HideGridCoarse, ShowGridCoarse}` (the renderer's `hide_mesh` and `show_mesh` of phase 1); `Session::hold_hidden_pictures`, which the windowed and headless runs call with `HELD_PICTURE_BUDGET` = 64 MiB for each view of terrain (the level's, each planet's).

**The rules**, each a test: a node is held only if its picture is of the data now and no job is making another (otherwise it is dropped as before); a held picture of data that has changed since (the server said something of the node while it was held) is not shown and is not touched until the node is wanted, and then it is dropped from the renderer and the node is made again (so the old picture is never put up); past the budget the **oldest** are let go of, never one that is wanted (those are the pin of a single view: what it is about to show); a node with nothing to see is held too, without a mesh (so the generator is not asked to find that out again: 16 percent of the nodes made); a reset takes down held meshes with the rest; showing a held picture does not count toward the cap of 64 pictures put on the screen in an update (it is a move), and a refinement does not make steps for a part of the cut whose pictures are all ready already.

**Measured [run]** (release, updates paced at 60 a second, the probe of 9a, `docs/history/spikes/viewpoints/probe_two_views.rs` with the screen kept as the renderer keeps it): from 264 km up back to the surface, **3.44 s without holding, 0.27 s with it**; 12,449 nodes shown from hold and 100 pictures uploaded where 13,711 were; 3,651 updates in the first call. Going up held 13,878 nodes (24.8 MB of meshes), and 2.7 MB was still held at the end. The view test: the ground (7,564 nodes drawn, 9,056 made for it, in a view that reaches 40,000 voxels) went up 400 bricks and came back with 645 nodes made and 92 pictures uploaded and 6,696 shown from hold; the 645 are probably the steps toward the bricks near the viewer (bricks are not held), which the next piece should look at.

**Not measured / not built:** what the 0.27 s is (the owner's target is 0.25 s; the bricks of the fine ball and the steps toward them are the first suspects, and these were not looked at); the memory of 12,000 hidden GPU buffers beyond their bytes (the probe counted meshes' bytes, not the adapter's); bricks (2b); a second view's pins; what a game's frame costs with the budget on (phase 1 measured the hidden meshes at 0.12 to 0.29 ms for 20,000, and nothing here has run in a window).

## 10. Not here

Sound; the user interface of a map (scale bar, labels, icons); a minimap's second render pass (the draw lists allow it; the renderer's second pass and its shadows are not designed); stars as voxels; the server's simulation distances; a map made of anything but the voxels' own nodes (alternative 6); a limit on how far a free camera may go (6.6 F).
