# Build log

What each phase built, and what was measured. History: read the phase you need (`grep -n "^Phase" docs/build-log.md` lists them).
The decisions that came out of them are in `design-decisions.md`; the plan is in `plan.md`.

## Built
Phase 1a (headless foundations, all in `engine/`):
- `namespaced_id`: validated `namespace:name` identifiers; ordering is by namespace, then name.
- `voxel_space`: `VoxelCoordinate`, `BrickCoordinate`, `LocalVoxelIndex`; floor semantics for negative coordinates.
- `voxel_grid`: sparse `Grid` of `Arc<Brick>`; a brick has one palette-compressed `ChannelStorage` per channel of a
  `GridLayout`; copy-on-write makes `clone()` a cheap snapshot. An absent brick means all channels at default.
- `content_registry`: `Registry<T>` assigns dense `RuntimeId`s and keeps content-less placeholders for missing plugins.

Phase 1b (terrain, in `engine/`):
- `region_octree`: canonical sparse octree over brick coordinates (`RegionOctree`, `BrickCube`). Uniform subtrees collapse
  to one leaf; `leaves()` is a stable Morton-order serialization. Isolating one cube costs 7 leaves per tree level above
  it (337 for a level-8 cube in empty space) however many bricks inside it are generated.
- `terrain`: `DensityField` (positive = solid) with `sample` and conservative `bounds` at a `LevelOfDetail`; value noise on
  a power-of-two integer lattice (exact cell/fraction arithmetic, splitmix64 hashing, no transcendental functions);
  an octave is kept at a footprint only if its wavelength is at least the footprint, so LOD output differs from full
  detail by at most the dropped octaves' amplitude (tested). `FieldTerrainGenerator` turns a field into bricks (grass,
  3 dirt, then stone), classifying bricks as all air or all stone from bounds alone when it can. `WorldTerrain` assigns
  generator versions to regions lazily and reports unassigned or missing-generator regions instead of failing.
- `voxel_space` gained `VoxelBox` and `LevelOfDetail`; `voxel_grid` gained stable FNV-1a content digests.
- `clippy.toml` forbids the platform-dependent float functions workspace-wide.

Phase 1c (seeing it):
- `voxel_space` gained `FaceDirection` and `WorldPosition` (an exact integer brick plus an `f32` offset, so positions and
  camera-relative vectors stay precise at any distance).
- `voxel_meshing`: greedy meshing of a brick and its six neighbours into 8-byte `PackedQuad`s. Verified against a
  slow reference that quads cover exactly the exposed faces, once each, with the right material.
- `renderer`: wgpu renderer (`VoxelRenderer`, `Camera`, `GraphicsContext`). One instanced draw per visible brick; the
  vertex shader expands each packed quad into two triangles, so there are no vertex or index buffers. Frustum culling
  and distance fog; placement is camera-relative from exact integers; material values beyond the palette draw
  magenta (how content from a missing plugin will look). Offscreen rendering to RGBA for tests and screenshots.
- `tools/terrain_viewer` (since retired, see 1f): `WorldStreamer` generated and meshed bricks on worker threads (a brick is
  meshed only when it and its six neighbours exist, unloading beyond the view radius); a winit window with a fly camera;
  headless `--screenshot` and `--benchmark`; `--adapter integrated` to test on the integrated GPU.
- GPU tests run on every physical adapter and backend (here: RTX 3090 and the AMD iGPU, each on Vulkan and DX12).
  Without a GPU they skip, unless `VOX_REQUIRE_GPU` is set.

Phase 1d (saving):
- `voxel_grid`: channels declare what their values mean (`ChannelValueKind`: a plain number, or an id from a named
  registry); `remap_values`; a canonical, strictly validated brick encoding.
- `content_registry`: `IdentifierRegistry`, a type-erased view for translating ids.
- `terrain`: `WorldTerrain` exposes its configuration and can restore saved assignments.
- `world_storage`: the section container, the engine sections, `WorldData` (terrain recipe + edited bricks + plugin
  sections), `ParsedWorld` / `LoadContext` / `LoadReport`. The format is specified in `docs/world-file-format.md`.

Phase 23e (closing Phase 23; owner's request, 2026-10-18):
- `GameDefinition::new_logic` is replaced by `GameDefinition::plugins(content)`; the shell builds the `PluginHost`
  (`game_shell::host::build_server_with_plugins` takes a different list or order). The three golden sessions also run with their plugin
  lists reversed and rotated, and must give the pinned numbers.
- Checked that the shuffle test bites: removing the sorting of saved sections alone leaves it passing (plugins are also registered in
  identifier order, so two mechanisms guard the same thing); removing both makes it fail on Cave.
- Phase 23 as a whole: grid addressing (`GridId`, `GridView`), the plugin host with phases, ordered systems, events, asks, minds and
  checked declarations, and every game converted, with no pinned number changed except the two deliberate ones of 23d. 84 test
  binaries pass on Windows and on Linux.

Phase 23d (Cave as plugins; owner's request, 2026-10-18):
- `CaveLogic` is split into `voxunami_cave:block_editing`, `:movement`, `:fire` and `:light` plus the shared players plugin, in
  `games/voxunami_cave/src/plugins/`. They hear of one another's writes through `ChannelsChanged`; a block edit is asked about first
  (`BlockEditRequested`).
- The golden session's stream is bit-identical to the one before (checked by restoring the old unknown-command text for one run); two
  deliberate changes re-pinned it: the unknown-command reply is generated and alphabetical, and the host sorts saved sections by identifier
  (each section's bytes compared, identical; only their order in the file differed).
- New tests: a plugin the game does not know refuses an edit and the player is told; a fire lit by another plugin is noticed by fire and light.
  Six mutations of the wiring (fire no longer notices new fires, light no longer notices changed blocks, bodies no longer remake terrain, an edit
  no longer writes its state, a refusal ignored, the light's order edge removed) are all caught.

Phase 23c (2048 and Eels as plugins; owner's request, 2026-10-18):
- `player_presence::PlayersPlugin` (the shared record of where players are: pose messages if the game believes them, `/level`, forgetting those who
  leave, announcing poses in `Settle`), `voxunami_2048:rules` and `voxunami_eels:rules`. Each game's `new_logic` builds a `PluginHost`.
  `PoseBook::level_command` was split from `handle_command` for it.
- Held fixed: the golden sessions and all existing tests pass unmodified on both platforms; moving Eels' `swim` system to a later phase fails its golden
  session. Four new tests for the players plugin (belief in reported poses, one announcement per change, announcement in the tick that moved a
  player, `/level`, forgetting, the unknown-command reply). Eels now answers `/level`.

Phase 23b (the registrar and the plugin host; owner's request, 2026-10-18):
- New engine crate `plugin_host`: `Plugin`, `Registrar`, `PluginHost` (the server's `GameLogic`), phases (`Decide`, `Controllers`, `Physics`,
  `Settle`, `Rules`), systems and handlers ordered by declared `after`/`before` with ties by plugin and name, events delivered first in first out,
  `ask` events with a first-refusal-wins verdict, `Mind` systems that can only emit intents, declared reads and writes checked in debug builds,
  and a start-up check that two systems using one channel (one writing) are ordered. `server` gained `ServerWorld::watch_channels` and
  `take_changes`.
- Tests: 25 (the order of a tick; independence of the plugin list's order; every registration error; fire-then-light in one tick; only watched
  channels reported; FIFO delivery; the cascade limit; asks; declared channels; messages; commands; players; minds; saving), a `compile_fail`
  doctest that a mind cannot write, and seven mutations of the host (ties, delivery, the ambiguity and declared checks, the limit, intents, ask
  order) all caught.
- Found while writing them: the spec said both that a handler's writes are delivered "before the next handler of the same event" and that
  they join a FIFO queue; the code does the second (simpler: what a handler causes cannot run in the middle of the handlers of its own event), and the spec
  now says so.

Phase 23a (grid addressing; owner's request, 2026-10-18; the first code step of `plugin-api-spec.md`):
- `voxel_space::GridId` (the address a rule uses; a root grid's id is its level's index), `server::GridView` (`id`, `get`, `set`) and
  `ServerWorld::grid`. `lattice::ActiveVoxels` and `voxel_random` are keyed by grid; Cave's light, multiscale light and fire rules and
  their adapters take a grid where they took a level. Names in those files changed from `level` to `grid` where they meant the world's
  level (a light level stays a level).
- Held fixed, and checked: the three golden sessions pass unmodified (stream and saved world, Windows and Linux), the pinned `voxel_random`
  numbers pass unmodified, and a new test writes a root grid's pending voxels by hand in the old by-level-name layout and compares the
  bytes. A voxel of a non-root grid cannot be saved yet and panics, which a test pins.

Phase 23, step 1 (locking behaviour before the plugin refactor; owner's request, 2026-10-18):
- `games/*/tests/golden_session.rs`, one for each game: a scripted session (fixed tick counts, fixed coordinates; never `Rig::settle`, which
  steps the server a variable number of times while the client's workers finish, and never what the mesher has drawn) records the
  stream of frames one player receives (a replay file) and saves the world; both are hashed (FNV-1a) and pinned. Cave drives block
  placing and breaking, the classic light, fire (far from the lamp, whose light would hide the fire's), every multiscale scheme and
  an odd-width one, commands, walking, flying and teleporting. Each also runs the session twice in one process and requires
  equal results. The numbers are the same on Windows (release) and Linux (debug).
- Found while writing them: a client only changes what it watches when `update_view` is called, so a first version whose player
  teleported away recorded none of what happened there, and a mutation (the light no longer told of fire's writes) went unnoticed.
  The tests call `update_view` each tick, and now fail on that mutation, on a light budget changed by one voxel, on a changed
  coasting range in 2048 and on a changed round timing in Eels.
- Sanity assertions in the scripts (the lamp's light spreads; a flame gives light where the lamp's does not reach; blocks moved in
  2048) so that the numbers lock something.

Phase 22 (cells of any width; owner's request, 2026-10-17; still a spike):
- A `LightScheme` is now a number of levels (32, 16, 8 or 4) and the widths of its scales: one to seven, each 1 to 32, each wider than the one
  before. The four presets are the old schemes (powers of two); `/light multiscale LEVELS WIDTH...` asks for any other, such as
  `/light multiscale 8 1 3 7 17`, and `/light multiscale 16` is still the preset. A scheme's name lists its widths (`8 levels, cells 1, 3, 7, 17`).
  Saved with the world as `[2, levels code, widths...]`; the earlier `[1]` and `[1, code]` are still read. Seven scale channels now
  (`light_scale_0` to `_6`).
- Cells are laid from the world's origin, so a width that is not a divisor of 32 has cells that are part in one brick and part in the next
  (and a cell at 17 to 33 is part in two bricks even though 17 is less than 32). The rule is unchanged: it reads and writes a cell's word at
  its anchor, the first voxel (`anchor_of(width, voxel)`, floor division, so negative coordinates are right). A brick is drawn from
  what it holds alone, so the server also keeps a copy of the word in every other brick the cell is in, at the first voxel of the cell there
  (`word_homes`; up to 8 copies, 2 for each axis, for a cell of at most 32); `world_view::combined_light` reads it there. Copies
  cost a write each and are changes the clients are sent like any other. If a brick cannot be written (terrain the server cannot generate)
  its copy is not made and it draws that cell dark (believed from the code; not tested).
- The client learns the scheme from the server, because the widths are not in the word: `voxunami_cave:light_scheme` goes to a player
  when they join and to everyone when `/light` changes it (so it is in recordings, and a replay draws the same). The shell has a hook for
  it, `GameDefinition::view_channels(content, client_state)` (default: the content's own), called after every tick's messages, and
  `WorldMesher::set_channels` meshes every brick again when it changes. `PackageSpec.scaled_lights` and `ScaledLightSpec` are gone: what the
  scales are belongs to Cave's client state, not to a package. The view radius, the mesher's jobs and the channels a job draws from are
  one thing: each job carries the channels it was made with.
- Weights for widths between the powers of two are on a straight line between those of the neighbouring powers (`full_brightness`; width 3
  is 217, 5 is 166, 17 is 88, 23 is 79), so the weights of the presets are what they were.
- Tested: `word_homes` (every voxel of a cell is in a brick that has the word at the first voxel of the cell there, for negative and positive
  bricks and widths to 32), the rule against the reference for four schemes with odd widths (the reference now cuts cells to the box), a lamp
  being the greatest level at every scale in them and going out, the scheme message (every malformed payload refused), the command and
  saving; in the full server and a client, a lamp in a cell across the edge of a brick has its word in both bricks on the server and on the
  client and is drawn blended. A mutation experiment (the server writing only the anchor's copy) breaks that test. `world_view` tests
  for cells whose first voxel is in the brick before, bricks before the origin, and cells wider than a brick.
- Four configurations on the Phase 21 scene (release, this machine; the rule alone, with the game's budgets):

  | scheme | budgets | fill: ticks | fill: ms | cells written | voxels read | cells lit by scale | empty: ticks | no budget: ticks |
  |---|---|---|---|---|---|---|---|---|
  | 8 levels, cells 1, 3, 7, 17 | 9375, 2205, 214, 15 | 174 | 310 | 1 494 | 4.3 M | 328, 278, 227, 375 | 256 | 7 |
  | 8 levels, cells 1, 5, 17 | 9375, 568, 15 | 174 | 299 | 1 243 | 4.2 M | 328, 276, 375 | 256 | 7 |
  | 8 levels, cells 2, 5, 11, 17 | 5000, 568, 56, 15 | 174 | 396 | 1 489 | 5.1 M | 282, 276, 229, 375 | 256 | 7 |
  | 4 levels, cells 1, 3, 5, 7, 11, 17, 23 | 9375, 2205, 568, 214, 56, 15, 6 | 20 | 165 | 156 | 2.4 M | 25, 24, 24, 19, 19, 24, 19 | 21 | 3 |

  | light (the real server) | the switching tick | mean, next 400 ticks | 95th | worst | over 10 ms | over 50 ms |
  |---|---|---|---|---|---|---|
  | classic | 17.2 | 2.75 | 21.8 | 40.2 | 40 | 0 |
  | 8 levels, cells 1, 3, 7, 17 | 26.1 | 3.95 | 17.3 | 53.4 | 71 | 1 |
  | 8 levels, cells 1, 5, 17 | 23.4 | 3.92 | 17.7 | 46.9 | 71 | 0 |
  | 8 levels, cells 2, 5, 11, 17 | 26.4 | 4.45 | 20.4 | 53.4 | 78 | 1 |
  | 4 levels, cells 1, 3, 5, 7, 11, 17, 23 | 60.5 | 0.67 | 0.0 | 54.7 | 9 | 1 |

  What it says (measured: the numbers; believed from the code and pictures: the reasons). The three 8-level schemes cost about the same
  as each other, a little more than classic per tick (one tick over 50 ms in 400 for two of them), and fill in 174 ticks, slower than the
  preset 8-level scheme (103), which also had cells of 16 and 32; why was not investigated. The 4-level scheme with seven scales is cheapest
  per tick (3 levels is only 3 cells of reach, 156 cells in all) but its switching tick is the slowest (60 ms: it reads whole cells
  of 23, 12 167 voxels each, once). Cost follows the finest scale and the number of levels, as in Phase 21, not how many scales there are.
- Pictures (`docs/multiscale-modes/montage_custom.png`: classic, then the four; same view and no ambient light or fog as Phase 21).
  Mean ground brightness measured from the pictures the same way as before (classic 26.5, which matches the 27 of Phase 21):
  8 levels with 1, 3, 7, 17: 61.5; with 1, 5, 17: 56.2; with 2, 5, 11, 17: 70.3; 4 levels with seven scales: 42.3. Cells of 17 show as flat dark
  squares far from the lamp, on a grid of 17 from the origin (it does not line up with the lamp or the platform, which is the point of
  the experiment); with 1, 5 and 17 the middle step is wide and the glow is plainer; the scheme that starts at cells of 2 has the
  brightest ground (why was not investigated: its finest weight is the lower, 255); 4 levels leaves the far parts black, as only three cells
  of each scale are reached.

Phase 21 (multiscale light schemes and weights; owner's request, 2026-10-17; still a spike):
- Weights: each scale now has its own weight in drawing (`multiscale_light::full_brightness`; 382, 255, 178, 128, 89, 64 for cells of
  1 to 32; 255 is full brightness and a channel's brightness is its level over its greatest level times its weight, at most 255). Fine
  cells count for more and coarse for less, instead of 17 a level for all, to give the dark end of the scale smaller steps (the coarse
  channels' steps are what a lamp's far glow is made of) and the bright end larger ones (the finest channel reaches full
  brightness at two thirds of its levels). Not judged by eye beyond the pictures below.
- Schemes (`LightScheme`, chosen with `/light multiscale 32|16|8|4`, 16 if not said, saved with the world): 32 levels with cells of 1, 2, 4;
  16 with 1 to 8 (the earlier spike); 8 with 1 to 16; 4 with 1 to 32. A lamp is the greatest level at every scale (an emission of 60 or
  more; a fire of 40 is two thirds). A cell's word is now six five-bit fields (what it gives across each face; its level is the greatest) and,
  in the top two bits, which scheme (so the client knows how many levels there are with no message: `world_view::LevelFormat`,
  `ScaledLight::brightness`); a cell that gives no light is 0. The level is no longer stored (the rule never read it), so a lamp's own
  cell, a whole block, now draws as lit (it gives light to every side), where before it held none. The layout has six scale channels
  (`light_scale_0` to `_5`; a scheme uses its first 3 to 6). Budgets are by voxels looked at, not cells (`default_budgets`: about 150 000
  voxels a scale a tick, so 9 375 cells at scale 0 and 2 at 32), which fixes the worst case of cells of 32 reading 32 768 voxels.
- Tested: the rule against the independent reference in every scheme (a second property test over boxes of 32 for the schemes with cells
  to 16 and 32), scheme codes, packing and decoding agree between the game and `world_view`, emission levels, budgets, saving a scheme,
  the command, and each scheme on the server. Mutation experiments: 8 of 9 broke a test (one only after adding a test, for a lamp shut in a cell); the 9th, the check that skips
  a face with nothing touching is also made later in the same function and is the one that changes nothing.
- Scene for comparing them (`games/voxunami_cave/tests/multiscale_modes.rs`, ignored tests, run with
  `cargo test --release -p voxunami_cave --test multiscale_modes -- --ignored --nocapture`): a platform 96 voxels square in the sky,
  flat on one side of a lamp (x below 48) and hills up to 14 voxels on the other, lamp 2 voxels above the flat ground. The terrain is two
  triangle waves, the same for every scheme.
- Rule benchmark (release; the rule alone on that scene with the game's budgets; reads are of a plain function, so milliseconds understate
  the server, where a voxel read costs more):

  | scheme | fill: ticks | fill: ms | cells written | voxels read | empty: ticks | empty: ms | no budget: ticks |
  |---|---|---|---|---|---|---|---|
  | 32 levels, cells to 4 | 156 | 774 | 92 552 | 2.77 M | 506 | 1 494 | 31 |
  | 16 levels, cells to 8 | 133 | 327 | 12 923 | 3.0 M | 264 | 100 | 15 |
  | 8 levels, cells to 16 | 103 | 254 | 1 576 | 3.4 M | 147 | 10 | 7 |
  | 4 levels, cells to 32 | 96 | 296 | 139 | 4.8 M | 96 | 3 | 3 |

- Server benchmark (release; the real server on the scene with a player; `ms` per tick; the tick is 50 ms):

  | light | the switching tick | mean, next 400 ticks | 95th | worst | over 10 ms | over 50 ms |
  |---|---|---|---|---|---|---|
  | classic | 12.7 | 2.44 | 19.7 | 34.3 | 33 | 0 |
  | 32 levels, cells to 4 | 7.1 | 9.08 | 34.4 | 66.5 | 131 | 7 |
  | 16 levels, cells to 8 | 8.7 | 4.08 | 17.0 | 49.3 | 83 | 0 |
  | 8 levels, cells to 16 | 22.7 | 1.63 | 8.8 | 37.0 | 15 | 0 |
  | 4 levels, cells to 32 | 59.6 | 1.25 | 9.8 | 47.7 | 20 | 0 |

  (The median is 0 for all: most ticks have nothing to do. The switching tick reads every edited brick, here a platform of about 20 bricks,
  and so grows with the world.) What it says: the 32-level scheme costs about twice 16 per tick, overruns the tick seven times in 400, and
  fills and empties slowest in ticks (a sphere of 31 cells at the finest scale),; 16 is about classic's cost; 8 and 4 are cheaper than classic in ticks, with a switching tick that is
  slow because cells of 16 and 32 are costly to look into the first time (the 4-level switch reads whole cells of 32). The mean cost
  of fill is dominated by the finest scale in every scheme, so the number of levels, not the number of scales, drives the cost.
- Pictures (`docs/multiscale-modes/montage.png`: classic, then 32, 16, 8, 4 levels; from straight above, no ambient light, no fog; the lamp is
  the pale square, flat ground on the left): the schemes agree on the far shape of the glow; the fewer the levels the bigger and plainer
  the steps, and in 8 and 4 the cells of 16 and 32 show as flat squares; the 4-level scheme shows a dark band at the back of the hills (cells
  that are mostly hill are not connected to the lamp's air by a face that touches). Classic is much dimmer at the same distance
  (255 levels over 64 voxels, so the lamp is 255 only at the lamp, where multiscale is full over a whole cell). Mean brightness of the
  ground, 0 to 255: classic 27, 32 levels 91, 16 levels 80, 8 levels 66, 4 levels 53; measured from the pictures, not a luminance unit.

Phase 20 (a dark Cave and the window's own commands; owner's request, 2026-10-17):
- There are no sky light channels yet (the plan, with six directions, is in "Voxunami Cave lighting"), so "sky light zero"
  is the one stand-in there was: `AMBIENT_LIGHT`, the brightness of a face no light reaches, was 160 (a lit day) and is
  now 0, until sky light is calculated. Cave is black where no lamp or fire reaches, and the sky and fog colours are unchanged.
  2048 and Eels have no light channel, so every face is fully lit and ambient light does nothing for them.
- Commands the window runs itself start with a period; those for the server start with a slash, and the server never sees
  a line that starts with a period (`client_commands.rs`; the command line opens on either key, and in a replay only on the
  period). `.ambient` shows how bright a face no light reaches is drawn, `.ambient 0-255` sets it (0 is black; about 20 to 40 is
  dark like Minecraft's caves) and `.ambient default` returns to the game's own; `.help` lists them. A player's setting is their
  own: not sent, not recorded, not saved, gone when the window closes. A game cannot add client commands yet.
- The report that lamps gave no light with multiscale light on: not reproduced as a failure. In the real window (release build,
  commands typed, a lamp placed with the mouse) lamp light appears with multiscale light on, and tests cover lamps placed
  before and after switching and in a world file saved before the scales existed. Under the old ambient 160 it is faint: a face
  1 voxel from a lamp is 55 percent brighter, 7 voxels away 28 percent, 12 away 12 percent, and the 16-level light reaches only
  15 voxels (classic reaches 64). Now that unlit faces are black it is clear. If it is still not seen, the steps to reproduce are
  what is missing.

Phase 19 (a spike: block light at four scales at once in Voxunami Cave; owner's request, 2026-10-16; not a decision about how Cave is lit):
- What it is: `games/voxunami_cave/src/multiscale_light.rs`, the light rule with the cell size as a parameter. At scale k the world
  is a grid of cells 2^k voxels along each side (1, 2, 4, 8), each holding at its first voxel a word in a channel per
  scale (`voxunami_cave:light_scale_0` to `_3`): its level, 0 to 15, in the low four bits, and above them, for each of its six
  faces, the level it gives the cell across that face (28 bits; drawing uses the low four, `LEVEL_MASK`). Light goes through the
  *air* (changed 2026-10-17, replacing "a cell is blocked when more than half of it is solid", which left the air of a mostly solid
  cell dark under a lit cell): a voxel is open if light can get into it; light passes between two cells across a face where an
  open voxel of one touches an open voxel (or a lamp) of the other at the same place in the face, and inside a cell it goes from
  the face it came in by to another only if one connected component of open voxels reaches both, at the place it came in. A cell
  with no open voxel holds none and still gives its source, as a lamp does. A cell's *source* is the
  brightest light any voxel in it gives off, the same at every scale: a block's emission divided by 4, rounded up, at most 15
  (so a lamp of 64 is 15 at all four scales, a fire of 40 is 10). (Phase 19 first added the emission up and divided it by
  a unit that grew with volume, so a lamp was 15, 2, 1, 1; changed on 2026-10-17 at the owner's request.) Otherwise a cell holds
  its source or one less than the most that comes in, and gives across a face one less than what came in at another face that one
  component joins to it (never back the way it came in). One cell a tick at every scale, so scale 3 spreads 8 voxels a tick.
  Each scale has a pending set of cells (`lattice::ActiveVoxels`, unchanged) and its own budget (15 000, 3 000, 600, 100
  cells a tick: a cell of 8 reads 512 voxels the first time it is looked into). What is in a cell is kept from tick to tick and
  forgotten when a block in it changes.
- Drawing: the client draws each voxel with the lights that reach it blended, `level * 17` for each scale and the plain light
  channel: what is missing from full brightness is the product of what each leaves missing (screen blending, as the renderer
  does ambient light), so lights add up and never pass full (`world_view::blend_brightness`; it was the brightest of them until
  2026-10-17). Two lights of half brightness are three quarters, and a very dim light beside a bright one rounds away. `world_view` got `ScaledLight` and `combined_light` (made in the mesh worker from the brick's channels, with
  a shortcut for bricks whose light is the same everywhere, which is nearly all of them); the mesher, the quad format and the
  shader are unchanged. `PackageSpec::scaled_lights` declares them (at most 4).
- Switching: `/light` says which, `/light classic` and `/light multiscale` change it. A switch clears every light (the classic channel and
  the four scales, in edited bricks, which is the only place light is) and has the new light look at every emitting block in
  the edited bricks. The choice and each scale's pending cells are saved (`voxunami_cave:light_model`, `light_scale_k`). Classic is
  the default, and its rule and numbers are untouched. Every Cave world's layout now has the four channels; a world from before
  loads with them empty and the classic light (tested).
- Tested: the rule against an independent search from the sources, at every scale, over random edits in two rounds (proptest);
  wall thickness, glass, lamps taken away, the same levels under a small budget, one-cell-a-tick; through server and sessions
  (switching both ways, levels seen by every player, faces drawn in steps of 17, saving and loading); the drawing in `world_view`.
  Mutation experiments: 14 of 14 caught after one more test.
- What it showed (measured in a release build unless said; all of it from one scene, so take the numbers as orders of magnitude):
  - Cost: with a lamp worth 1 at the coarse scales (the first rule) the multiscale light took 0.12 ms a tick on average and the
    classic 7.3 ms; that was mostly reach, not cleverness (15 cells against 64). With a lamp worth 15 at every scale (the
    rule now) coarse light fills a sphere of about 15 cells at each scale, and cost became comparable to classic (release, one
    lamp in the air, 300 ticks: classic 3.7 ms a tick on average and 34 ms worst; multiscale 4.3 ms and 78 ms worst, over the 50 ms
    tick, because a cell of 8 reads 512 voxels each time). Keeping what is in a cell between ticks and cutting the coarse
    budgets brought it to about 3.8 ms and 41 ms worst for one lamp, and 4.8 ms and 46 ms for twenty. The coarse scales now
    fill slowly (100 cells a tick at scale 3 is some seconds for its whole sphere); a lamp taken away takes hundreds of ticks to
    be fully out at scale 3. With the connectivity rule (each cell keeps what it gives each neighbour, so more words change)
    the numbers are, measured on an open box: with no budget every scale fills and goes dark in 15 to 16 ticks; with the
    game's budgets (15 000, 3 000, 600, 100) scale 3 filled in 259 ticks and went dark in 558 (scales 1 and 2: 15 and 36,
    16 and 68). Cost of those budgets with a lamp worth 15 everywhere: worst tick 40 ms with one lamp and 50 ms with twenty (the
    tick is 50), mean 6.2 and 8.4 ms against classic's 3.7 and 4.9. Halving the budgets did not lower the worst tick much and
    made scale 3 slower, so they stayed. Tuning budgets by cells is the wrong unit, and
    nearest-the-change-first ordering of the pending cells would spend them better (neither done).
  - The look: with the first rule (and the brightest light drawn) faces were lit in steps of 17 and the coarse scales showed as
    blocks of light 2, 4 and 8 voxels across; blending and lamps at 15 everywhere should make the coarse light much brighter and
    broader, and I have not looked at it in the window since. In the one scene
    drawn (a cluster of eight lamps and two single lamps over a hill, in daylight ambient) the lit slope is brighter and
    stepped; I did not try it at night, and nothing has been tuned.
  - Ground: with the first rule a coarse cell that was half ground was blocked, so the air in it stayed dark under a lit
    cell (reported by the owner, 2026-10-17) and thin walls leaked at coarse scales. With the connectivity rule above a mostly solid
    cell is lit from the air that touches the lit cell's air, a wall across the whole box stops light at every scale however thin,
    and a hole in it lets light through at every scale. What is left of the idea's weaknesses: a cell gives one level across a face
    for every place on it where cells touch, so light in one pocket of air passes sideways into a second pocket of the next
    cell that touches the same face (a level for each place would need more than a word); a cell has one level for all its air,
    so a pocket shut in a lit cell is drawn lit; and what comes in is judged on the air of a whole face, not the air beyond it.
  - Speed: coarse light spreads fast in metres (scale 3: 8 voxels a tick, 160 m/s at 20 Hz). Probably wants a slower rule for
    large scales, or to be treated as always settled.
- What the contract needed, and what it did not (the point of the spike): the channel list in the spec already allowed any number of
  channels; the view contract needed a list of scaled lights (added); the rule shape held (per-scale pending sets on
  `ActiveVoxels`, evaluate from the snapshot, write, wake neighbours) with no change to `lattice`; the budget had to become per
  scale. What was missing: a way to find every emitting block or every lit voxel without scanning every edited brick (switching
  does this scan, which is fine at this size and not at a large world's); a way for a game to ask the engine for coarse-cell
  aggregates (the rule reads E^3 voxels by hand); and a place for the choice of rule that is not a command (a world setting).
- Costs not measured: the four channels make every edited brick bigger on the wire and in the file (a lit scale channel is
  paletted, a few bits a voxel; unlit ones are free), and the Cave integration tests took about 20 percent longer with the
  channels declared in the debug profile (about 28 s to 34 s), and a further 20 percent with the drawing code (to 41 s), although
  in a release build loading and meshing a view of radius 12 took the same time with and without it (4.6 s). Why the debug
  profile feels it, I did not find out.
- Not done: smoothing between cells (trilinear, in the shader, would remove the blocks), colours, sky light at coarse scales
  (the natural user: the sun is a source of a huge cell count), light across levels, tuning of units and thresholds, and any
  choice about which of the two lights the game keeps.

Phase 18 (mod stage 1, first step: the plugin contract, from what three games repeat):
- Finding: the three games hand-wrote nearly the same `GameContent` (a block registry, channels, one terrain generator
  made from the seed, a level name, how the view reads the channels); Cave got part of it from the `base_blocks` plugin,
  2048 and Eels wrote all of it. That repeated shape is the contract a data-driven mod will have to fill in.
- `game_shell::package`: `PackageSpec` (level, block registry kind, channels, which are block/state/light, a generator
  factory `seed + layout -> generators`, default generator, assignment cube level, ambient light) and `ContentPackage`, which
  is the `GameContent` for it. Eels, 2048 and Cave now declare a spec and keep thin wrappers for their own channel accessors.
  `base_blocks` split its ids from its registry (`BaseBlockIds` is `Copy`, with the channels and generators; `BaseBlocks`
  derefs to it and hands its registry to the game), so a plugin is: a registry, ids, channel definitions, generators.
- Checked, not assumed: the saved bytes of a new world are identical before and after, for all three games (hash of
  `new_world(5).save`, computed in a worktree of the old commit), and every pinned generator digest and save test passes
  unchanged on Windows and Linux.
- Not done, and what it shows about the rest of stage 1: only one terrain generator and one level per package (a mod
  would add more), no block registration from several plugins into one registry (the registry still comes from one
  place), Cave's rules (light, fire, movement) and 2048's motion rules are still in the games (they are lattice rules and
  entity rules, which stage 7 is about), and the spec is a Rust struct, not yet data on disk (stage 2).

Phase 17 (Voxunami Eels, 3D Tron; `games/voxunami_eels`; the owner's design is only "3D Tron with flat worms that barrel-roll", so the rules below are mine and easy to change):
- Rules (`round.rs`, on any arena, no servers): up to 8 eels in a hollow 64-cell cube. Each swims one cell every 3 ticks (30 ticks
  a second, so 10 cells a second) and leaves its body behind as a permanent wall of its colour. An eel dies on the wall, on any body,
  or when two want the same cell. The last one swimming wins the round and a point; a lone player plays until they die; a new
  round starts 3 s after each one ends, the arena cleared. Players joining during the countdown are in the round, later ones wait for
  the next. A step works from the arena as it stood, so the order of eels never matters.
- The eel is a ribbon 3 cells wide and 1 thick. Rolling turns it a quarter about its heading (flat or on its edge), which changes
  which gaps it fits and the wall it leaves. Simplifications to know: only quarter rolls (two states, so Q, E and Space all do the
  same), steering is relative to the camera whose up is always the world's, and the wings fold in on a step that turns (the eel is
  its head cell then), because a 3-wide ribbon swung round sweeps back over the body it just laid and could never turn. An eel
  is not hurt by the cells of its own previous step, so it can turn into its own wing's cell.
- Controls: A/D (or the arrows) left and right, W/S up and down, Space/Q/E roll. Chase camera 6 cells behind and 2.5 above the
  head; the server computes every camera (as the players' poses) and moves it a third of a cell each tick so it glides.
  Clients never say where they are.
- Terrain: `voxunami_eels:arena_v1` makes only the wall (dark panels, bright grid lines every 8 cells and along the edges), pinned
  like the other generators; bodies are edits (block `trail` or `head`, the eel's colour slot as the state, 8 colours).
- State: the match (who swims, scores) is in memory only, so a saved world holds only bodies, and the first round after loading
  clears them. Scores last while their player stays.
- Shell hooks added for it (all default to the old behaviour): `GameDefinition::ticks_per_second` (the 20 Hz constant moved
  into the trait; Eels uses 30, which the doc had planned for Tron), `server_sets_view` (the camera takes the server's angles,
  turned toward them smoothly, and the mouse does not turn it), `shows_other_players`, `shows_crosshair`. The generic controls
  line no longer claims W A S D fly: each game's own text says how it moves (Cave and 2048 text fixed).
- Tests: 36 unit tests (steering, ribbon geometry, rules on a plain map with a property test that live eels never overlap and
  everything painted is inside, messages, camera), 9 through server and sessions (colours, countdown, glide, steering and rolls,
  death and clearing, two players, leaving, ignored poses, save/load, recording and replay). Mutation experiments: 14 of 17
  caught; the survivors are the skip for all-air bricks when clearing (only speed), sending standings only when they change
  (only traffic), and the glide fraction, which then got its own test.
- Not done: nothing is shown but the title for scores (the renderer has no text); no gaps or power-ups; players always start in the
  same places; a client that never presses a key swims into the wall in about 6 s; real play with two people on a network was not
  tried, and how steering feels (ten cells a second, one steer remembered per step) was judged only from screenshots and key taps.

Phase 15 (the `physics` crate: a walking body on Rapier; the only crate that imports Rapier):
- `engine/physics`: `CharacterBody` is a player's body in a Rapier world of its own (one per player, around a floating
  origin that is moved when the body gets far from it, so `f32` keeps its precision). Terrain is read through a
  `CollisionSource` (what is in a voxel, and a cheap hint per brick: empty, solid or unknown), made into colliders a brick at
  a time, and made again for a brick when `brick_changed` says it was edited. Terrain that cannot be read is a wall, not a
  void, so a player never falls out of the world while it loads. `BlockDefinition::passable` (flames) takes a block out of
  the way of bodies without changing how it is drawn or aimed at.
- What worked and what did not, so it is not tried again: a Rapier capsule against `Voxels` stuck on seams and crept by
  about 1e-4 a step. What works: a cuboid body (0.6 x 1.8 x 0.6), terrain as boxes merged greedily within a brick, a small
  overlap (0.05) across solid brick boundaries, the character controller's sideways and vertical moves done as two moves,
  `normal_nudge_factor` 1e-5, no snapping to the ground, and automatic stepping up 0.6. Partial shapes (slabs and the
  like) are boxes of their own. The speeds are in `CharacterSettings` (gravity 32 voxels/s^2, jump 9, which is about 1.27
  voxels, walk 4.3, run 5.6, fly 10.9 and 21.6, terminal speed 78), 60 steps a second; yaw uses `libm`.
- Tested: 15 tests in `engine/physics/tests/walking.rs`, including a property test that a body is never inside solid
  terrain, a scripted run whose hash is pinned and is the same on Windows (debug and release) and Linux, and that editing
  terrain under a body is noticed. Mutation experiments on the crate: every change caught except one that changes nothing.

Phase 16 (Voxunami Cave: gravity and collision; the server moves the players):
- Messages (`games/voxunami_cave/src/messages.rs`): `voxunami_cave:spawn_at` (a pose, taken once, when the player joins) and
  `voxunami_cave:move_input` (keys as a `MoveRequest`: forward/side/up, jump, sprint, yaw, pitch; repeated about every
  100 ms and when it changes). `GameDefinition::{server_moves_players, join_message, movement_message}` let a game opt in; a
  game that does not (2048) works as before.
- Server (`movement.rs`, `logic.rs`): `Movement` holds a body per player, takes 3 physics steps per 20 Hz tick, and puts the
  result in the `PoseBook` so everyone, and any recording, sees it. A jump asked for between two ticks still happens.
  Edits, and the changes fire makes, tell the bodies to make the terrain again; a player moved to another level keeps
  their place and gets that level's ground. `/fly` toggles flying (key F), `/tp X Y Z` puts the eyes at a position.
- Client (`game_shell::following`): the camera follows what the server says; `PoseFollower` carries the view on at the
  speed of the last two positions between ticks so that steady walking is smooth, and shows the last position told when
  none comes (standing still). The way the player faces stays their own.
- Tests (Cave `tests/play.rs`): falling and standing on the ground to a few hundredths and then bit-still; keys walk the way
  the player faces; a tap of Space jumps 1.1 to 1.5 voxels and lands; a built wall stops them against its face; breaking
  the ground under them, and fire burning it away, make them fall; flames do not stop them; `/fly` and `/tp` and their
  error replies; a player's own pose, a second spawn and nonsense moves change nothing; the cellar level has the same
  ground; a scripted walk over the hills with pinned hashes, identical on Windows (debug and release) and Linux. Mutation
  experiments on the movement code: the step count, tap-jump, edit and fire notifications and flame passability are caught;
  two survive because they cannot be seen (ignoring a pose message is also what falls through, and a second spawn is
  overwritten by the next tick), and one is not tested (terrain that cannot be read being a wall; `physics` tests it).

Phase 13 (fire, Voxunami Cave; the second lattice rule):
- Blocks: `wood` (burns slowly), `fire`, and `grass` now burns fast. `BlockDefinition::flammability` (0 to 255, carried by
  `BlockTable::flammability`) says how readily a block catches fire; the rule decides what the number means. A fire is a
  low half-height block of 40 light, drawn in a colour for each age (the state is its age, an unbounded number, like 2048's
  numbers). Keys 5 and 6 place wood and fire; a placed fire is looked at from the next tick.
- The rule (`games/voxunami_cave/src/fire.rs`, tested on plain maps like the light): each tick a fire in range ages with
  chance 1 in 4 (so about 48 ticks in all, twelve ages) and goes out leaving air; and for each of its six neighbours that
  burns, sets it alight with chance flammability in 1024 (grass 100, wood 40): the block becomes a new fire of age 0. All of
  it from the world as it stood, so fire moves at most one voxel a tick and two fires lighting one block agree. Tested: the
  mean lifetime, the rates of ignition over 300 seeds, the one-voxel-a-tick rule, freezing out of range and in other levels,
  that every fire in any random scene goes out and stone is never touched, that a saved rule carries on exactly, and
  twelve deliberate breakages of the rule and its integration (all caught).
- In the game (`logic.rs`): fires burn when a player of their level is within `FIRE_RANGE` (64 voxels) of them; the fires and
  the rule's clock are saved (`voxunami_cave:fire`), and a world saved while burning keeps burning when loaded (tested).
  Changes made by fire wake the light rule in the same tick, so flames light the air as they burn.
- Limits, so they are not forgotten: fire does not leap across air or burn upward, there is no rain or water to put it out,
  a burnt block is simply gone (no charring, no ash), flames are drawn as an opaque orange slab, and the burn rates were
  chosen without trying them in play.

Phase 14 (what the two rules share, and a budget for the light):
- New engine crate `lattice`: `ActiveVoxels` and `voxel_random` (see `design-decisions.md`, "Decided (rules for lattice simulation and physics)").
  The light's pending voxels moved onto `ActiveVoxels` (the saved form is unchanged), and `Lighting::step_within` takes a
  budget (Cave uses 20 000 voxels a tick), tested to settle to the same light however small the budget.
- `PoseBook::players` lists the players with their poses and levels, for rules that act near players.

Phase 11 (the light rule, Voxunami Cave; the first lattice rule):
- `games/voxunami_cave/src/lighting.rs`: `Lighting` is the rule and the set of voxels it has yet to look at; `LightWorld`
  is what it needs of a world (the block and state of a voxel, and its light), so it runs the same on the server's world
  and on the plain maps the tests use. A step works out the light of every pending voxel from the world as it stood at the
  start of the step, writes the ones that changed, and makes those and their neighbours pending. So light spreads and
  dies out one voxel a tick (a lamp of 64 takes 64 ticks to reach its full range), in an order that cannot matter, and a
  world with nothing pending is settled and not looked at.
- What a voxel's light is: its block's emission, or one less than the brightest neighbour that lets light out through the
  face between them, if the voxel lets it in through that face; a voxel with no open face (a whole block) holds none, and a
  whole lamp gives light to its neighbours without holding any. Open faces are `BlockTable::connections`, so slabs and
  stairs pass light where their empty part reaches.
- Approximation, so it is not forgotten: the stored light is one number per voxel, so light that comes in by one open face
  is treated as able to leave by any other. A block with a wall inside its cell between two open faces would be wrong; no
  block has one.
- Tested against an independent search from the sources (a priority queue, not a relaxation): any random scene of air,
  stone, glass, slabs, stairs and lamps settles to what the search finds; so do edits made while light is still spreading;
  and a scene moved by any offset (across brick boundaries differently) settles to the same light moved. 3000 cases of each
  passed once by hand. Nine deliberate breakages of the rule are all caught. A reference bug found on the way: the search
  overwrote a brighter source with a dimmer one at the same voxel.
- Pending voxels are saved with the world (`voxunami_cave:light`, tested: a world saved six ticks after a lamp was placed
  settles to byte-identical voxels as one that never stopped).

Phase 12 (light in Voxunami Cave):
- `base_blocks` has the `lamp` block (a whole solid block, emission `LAMP_EMISSION` = 64) and the channel
  `base_blocks:block_light`; the Cave grid has a third channel for it. A server that places or breaks a block tells the rule;
  every tick it steps the rule and the changed light streams to clients as ordinary voxel edits. The client's mesher takes
  the light of the voxel in front of each face (Phase 7), so faces beside a lamp are drawn bright. Key 4 selects the lamp.
- The renderer has `Atmosphere::ambient_light` (and `GameContent::ambient_light`, default 0, so other games and the
  renderer tests are unchanged): the brightness of a face no light reaches, combined with a face's light like a screen blend
  (`1 - (1 - ambient)(1 - light)`). Cave used 160 of 255 as a placeholder for day light until there is sky light, so the
  whole of Cave was dimmer than it had been (the lamp's light is only 64 of 255 at its brightest, so its glow was subtle).
  Since Phase 20 it is 0: see there.
- Worlds saved before the light channel existed load with every voxel unlit (tested).

Phase 10 (chain pushing, Voxunami 2048):
- `Motion::pushes` (stone only) and `PUSH_LIMIT` (12). `Movers::advance` works on the list of movers by index, so that a
  push can move other movers along with their blocks and a cancellation can end the motion of the block it removes.
- Found on the way, a bug of Phase 9: a block cancelled by a block that had already acted this tick left its mover
  behind, pointing at an empty voxel until its next step. A property test found it (the saved case is in
  `games/voxunami_2048/proptest-regressions/`); a cancellation now ends the struck block's motion at once.
- Property tests run over scenes that now contain pushing stone: totals are kept (double, add), never grow (subtract), no
  mover outlives its block, positions are unique, and all motion ends; 50 000 cases of each passed once, by hand.

Phase 9 (tools choose how blocks combine, Voxunami 2048):
- `tool::Tool` (`Double`, `Add`, `Subtract`) and `Tool::combine(moving, struck)`, which says whether two numbers combine
  and into what (`Combination::Becomes(number)` or `Vanish`). The merge rule left `NumberedBlock`. A `Mover` has a tool;
  `Movers::push_block` takes one; a push request carries it (one more byte); a saved mover carries it (saved section
  version 4; older versions read as the double tool).
- The client has the chosen tool in its state and shows it in the title. Property tests: with double or add the total of
  all numbers is kept; with subtract it never grows and no block is ever numbered 0.
- Not built: chain pushing (next), more tools.

Phase 8 (numbers as block states, Voxunami 2048):
- A voxel of 2048 is a material's block (`voxunami_2048:stone`, `ice`, `spring_ice`; air is `voxunami_2048:air`) and, in
  the channel `voxunami_2048:number`, its number as the state. `NumberedBlock` is a material and a `u32` number;
  `from_voxel(block, number)` is `None` for air, for blocks from elsewhere and for a number of 0. Emptying a voxel resets
  its number, so air holds the default in both channels (tested).
- The engine learned colours by state: `BlockDefinition::with_state_colors` and `BlockTable::material_of` give each
  (block, state) a palette entry, and the mesher merges by that. Colours repeat (state `s` has colour `s % count`), so
  every state has one. 2048 uses 61 colours per material: 2 is a primitive root modulo 61, so the first 60 powers of
  two each have a colour of their own and any other number shares the colour of the power with its residue. The
  renderer's palette limit (240) is respected: 4 blocks plus 183 colours.
- The landscape generator is now `voxunami_2048:numbered_hills_v2`: the same numbers and materials at the same voxels as
  version 1, written as a material block plus a number. Version 1 (a block for every number) is retired, not edited: a
  world that names it still loads, keeps its edits and its generator name, and has no terrain where that generator made
  it (tested). Worlds made before this phase are therefore not playable; none were kept.
- The progress message and the saved state carry the largest merge as a number; the saved section is version 3, and
  versions 1 and 2 (an exponent) are still read. The score saturates instead of overflowing.
- Merging is still the doubling rule of `NumberedBlock`; moving it into tools is the next phase.

Phase 7 (block states and shapes, and light in the quad):
- `block_models` (new engine crate): a `Shape` is a union of boxes in eighths of a voxel, from which it works out what the
  mesher and later lighting need: which part of each face of the cell it covers (`face_cover`, 64 bits per face), the
  rectangles of it that show (`exposed`), and which faces of the cell are connected through it (`FaceConnections`, a 6 x 6
  matrix: the open faces a flood fill may cross). `Shape::full/empty/half/stair/from_boxes`. A `BlockDefinition` has a
  colour, whether it is see-through, a light emission and the shape of each of its states; `BlockTable` turns the
  definitions of a game's block registry into what every consumer asks of a (block, state): shape, solidity, opacity, what
  hides what, colour. An unknown block (from a plugin that is not loaded) is an opaque magenta cube; a state the block does
  not have is state 0.
- `voxel_meshing`: `mesh_brick` takes `BrickChannels` (blocks, optional states, optional light) and a `BlockTable`. Whole
  opaque cubes are merged greedily with the key (block, light) as before; blocks that do not fill their cell, and
  see-through ones, give `DetailedQuad`s at eighths of a voxel (16 bytes packed), culled where a neighbour hides them. The
  8-byte quad carries light in the top 8 bits of its material word (block ids are 24 bits).
- `renderer`: a second pipeline draws detailed quads; both shade by the light; `RenderStatistics::detailed_quads_drawn`.
  `upload_brick` takes a `PackedBrickMesh`.
- `world_view`: `ViewChannels { blocks, states, light }` and an `Arc<BlockTable>` are what a mesher is built from;
  queries `state_at`, `is_solid_at` (something to stand on and aim at) and `is_opaque_at` (hides what is behind it).
- `game_shell`: `GameContent::view_channels` and `block_table` replace the material channel, palette and opacity table.
- `base_blocks`: the channel `base_blocks:block_state`, and seven shapes for stone, dirt and grass (whole, bottom slab,
  top slab, stairs rising east, west, north, south); the terrain generator is unchanged (its pinned digests still pass).
- Voxunami Cave: the server accepts `SetBlock` with a state and refuses a state the block does not have (air has only 0),
  writing both channels; R cycles the shape of the block you place; the status line names it. A world file written
  before states exists loads with every voxel in state 0 (tested). Voxunami 2048 uses plain cube definitions; its
  unbounded numbers through the state channel come with the 2048 tools step.
- Limits, so they are not forgotten: see-through blocks are drawn in their opaque colour (no blending), and hide only faces
  of the same block; a sealed hollow between shaped blocks (two stacked slabs) still draws the faces inside it; aiming uses
  whole voxels, not the shape; no game has a light channel yet, so light was only exercised with test and spike fields.

Phase 6 (levels and commands; protocol version 2):
- `WorldData` is a list of levels (`LevelData`: terrain assignments and edited bricks), the first being the main level.
  `LevelIndex` (in `voxel_space`) numbers them. The file has an `engine:levels` section; files written before levels
  load as one level named by the game (`LoadContext::legacy_level`) and are saved in the new form. `add_level` refuses a
  level with other channels or generator versions, or a name already used.
- The protocol names a level in `SetInterest`, `InterestChanged`, `BrickContents` and `VoxelEdits`, and adds `Command`
  and `CommandReply`. A client watches one region of one level at a time; moving to another level sends the whole new
  region. The server tracks edits, generated bricks and audiences per level (`Audience::Watching(level, brick)`).
  Recordings of protocol 1 cannot be played.
- `GameLogic::command_received` and `ServerWorld::reply`; `server::CommandLine` takes a command apart. Replies reach
  only the sender, after the tick's game messages. Games use `PoseBook::handle_command` for `/level`.
- The client keeps every level's data and forgets other levels' bricks when the watched level changes; the session
  re-meshes from scratch for the new level (`WorldMesher::reset`) and draws only players in its level. In a window, `/`
  opens a command line shown in the title; Enter sends it, Esc drops it; answers show in the title and print.
- Cave and 2048 act in the level the player is in. 2048's moving blocks belong to a level and are saved by level name.
  Neither game makes a second level yet; tests do.

Phase 5 (what the earlier phases left):
- Other players are drawn: `renderer::Avatar` (a head over a body in one of 16 colours, taken from the last 16 palette
  values, so games have 240), `Session::other_players` (everyone but this player) and `view::show_other_players`.
  Avatars go through the same pipeline as bricks and are placed exactly like them, so they are as precise far from the
  origin. They do not turn to face anywhere, and they are one voxel wide.
- The server keeps the last 128 generated bricks (`generated_cache`), so game logic reading voxels of terrain nobody has
  edited no longer regenerates a brick per read. A generated brick never changes once its region has a generator.
- `engine:generators` is version 2: one number per leaf, with positions implied by the tiling
  (`region_octree::tile_from_levels`). A world with no edits went from about 40 KB to 3.3 KB at view radius 8. Version 1
  files still load and are saved as version 2.

Phase 4 (infinite 2048, replacing the 4 x 4 x 4 board of Phase 2):
- `games/voxunami_2048` is now: `material` (stone, ice, spring ice and what each does when pushed), `numbered_block` (a
  material and a power of two, 60 block ids), `motion` (the rules of moving and merging blocks, on a `BlockField` so they
  run on plain maps in tests), `landscape` (the frozen generator `voxunami_2048:numbered_hills_v1`, pinned), `content`,
  `messages` (push a block; progress), `logic` (the server: reach check, pushes, one step per tick, score, saved state)
  and `game` (the tools; the title says which block you look at). The 4 x 4 x 4 board, its rules and its push plates
  are gone, and so are the old messages and saved state, so worlds from that stand-in are not loadable.
- `PlayerPose::distance_to_voxel` (in `player_presence`) is Cave's reach check promoted, since a second game needed it.
- Measured on this machine at view radius 12: 9171 bricks held, 183 k quads drawn, 1.3 ms a frame (745 fps) on the
  integrated AMD GPU at 1280x720 and 2.9 s to load on the RTX 3090.

Phase 3 (the network):
- `transport::tcp`: `TcpConnection` and `TcpServerListener` (length-prefixed frames, never blocking, a bounded queue per
  peer, graceful close), and `ListenerSet` so one server accepts the local player and network players together.
- `Server::save` saves a running server's world with its game's state; `game_shell` gained `ServerThread` (the old
  integrated server, now with an optional network listener, autosave and join announcements), `--host`, `--join`,
  `--autosave`, and `voxunami serve GAME` for a dedicated server. Verified with real processes on this machine and with
  tests over real TCP; not yet across two machines.

Phase 2 (second game, and what it showed was shared):
- `game_shell` (engine): everything Cave had that is not Cave: the window and fly camera, the singleplayer server on a
  thread, `Session` (client, mesher, camera, aiming, sending), worlds and recordings on disk, headless screenshots and
  benchmarks, replay viewing, command-line options and help. A game implements `GameDefinition` (its name, content,
  server rules, what clicks and keys do, what its status line says) and `GameContent` (its blocks and worlds).
- `player_presence` (engine): the pose messages and the server's `PoseBook`, used by both games.
- `GameLogic::save_sections`: a game saves state the voxels do not show (a score) as plugin sections of the world file.
- `games/voxunami_2048`: a 4 x 4 x 4 board of tiles made of voxels with push plates; the first version, replaced in
  Phase 4. It showed that a game's state belongs in the voxels, with only what they do not show saved as a section.
- `voxunami_cave` shrank to its blocks, its one message, its rules and its clicks.

Phase 1f (first game):
- `world_view`: `WorldMesher` keeps meshes of the bricks around a viewer up to date on worker threads, reading from a
  client's world (generating unedited bricks from the generator the world assigns, taking edited ones as they are,
  re-meshing a changed brick and its neighbours, discarding stale results); `cast_ray` finds the voxel under the
  crosshair. It replaces the old viewer's streaming code.
- `plugins/base_blocks`: air, stone, dirt, grass, and the frozen generator `base_blocks:rolling_hills_v1` (pinned).
- `games/voxunami_cave`: `CaveContent` (plugins, new and loaded worlds), the server's `CaveLogic` (block changes within
  reach of the player's last pose; everyone told where everyone is), `IntegratedServer` (the server on its own thread at
  20 ticks per second: what singleplayer is), `Session` (a player's side without a window), a window with a fly camera,
  mouse capture, crosshair, break and place, and headless screenshots and benchmarks. `--world FILE` saves and loads the
  world, `--record FILE` records the session; replays play in real time from the recorded player's point of view, or
  render one tick to an image.
- `launcher` (`voxunami`): lists games, starts one by name, and opens a recording with the game its welcome names.
- `renderer` gained an opt-in crosshair. `replay::read_welcome` reads which game a recording belongs to.
- `tools/terrain_viewer` is gone: the game does everything it did (`--screenshot`, `--benchmark`, `--adapter`), through
  the same client path a networked player uses.

Phase 1e (the stream; protocol version 1, documented in `docs/protocol-and-replay.md`):
- `binary_codec` (the byte primitives, moved out of `world_storage`), `voxel_space::BrickBox` (inclusive box of bricks,
  with `without` for the newly watched part), `region_octree::BrickCube::covering`, `voxel_grid::Grid::content_digest`.
- `world_storage`: `WorldData::describe` (the world's shape as a small world file), `SourceTranslator` (translates a
  source's bricks, values and generator ids into the loaded world's numbering; loading uses it, and so does a client),
  and a `test-support` module holding the pretend game with switchable plugins that the tests of every crate below share.
- `protocol`, `transport` (connection interface and in-process loopback), `server` (the tick loop, sessions, watched
  regions, edit replication, `GameLogic` for games to implement), `client` (handshake, a world view built only from
  server messages, applied one whole tick at a time), `replay` (file format, recording connection, playback connection).
- `end_to_end_tests`: a real server, clients and recorders together. Includes replay-equals-live and the check that
  every player sees what the server has in the region it watches.

Phase 24 (property bags on block definitions; specified in `property-bags-spec.md`):
- `block_models`: `PropertyKey<T>` (a const key with id, default and doc), `PropertySchema` (the declared keys, with the engine's
  `engine:density` and `engine:placeable_on_moving_grids` always present), `BlockDefinition::with_property` (every state) and
  `with_state_property` (one state), `BlockTable::try_from_definitions` (one column per key, indexed by model), `handle`, `get`,
  `get_of`. A value resolves as state override, else the block's value, else the key's default; an unknown block has the default.
- `base_blocks` declares `base_blocks:emission`, `base_blocks:flammability` and `base_blocks:passable`, and offers
  `BlockPropertiesExt` (`with_emission` and so on) so its definitions read as before. `PackageSpec.properties` carries a game's
  declarations. The `emission`, `flammability` and `passable` fields and accessors are gone from the engine.
- Cave's lighting, fire, multiscale light, movement and light plugin hold `PropertyHandle`s and read by model.
- Checked: all three golden sessions pass unmodified on Windows and Linux; 8 mutations of the property machinery are each caught;
  the multiscale benchmark did not regress (fill ms for 32/16/8/4 levels: 770-830, 327-336, 251-256, 336-343 before; about 650,
  294, 228, 290-304 after the rules read through handles).

Phase 25 (declared messages and a command registry; specified in `messages-and-commands-spec.md`):
- `protocol::Message` and `check_message`; the six client-to-server and the server-to-client kinds of the presence plugin, Cave, 2048 and Eels
  are now types that implement it (the wire bytes are unchanged, pinned by `tests/wire_format.rs` in each crate before the conversion).
  `PluginHost::check_messages` runs when any game's server is built; `malformed_messages` counts payloads that did not decode.
- `plugin_host`: `CommandSpec::new(..).argument(..)`, `Arguments`, the host's `/help`; `/fly`, `/tp`, `/light` and `/level` converted.
- Golden sessions: 2048 and Eels pass unmodified. Cave's stream hash changed once, only in the reply to the unknown command `/nonsense` (it now
  lists `/help` and each command's arguments, 49 bytes longer); proved by restoring the old text, which gave the old hash. The saved world
  is unchanged. Bad `/tp` numbers now answer `usage: /tp X Y Z; Z must be a number` instead of `..., with numbers`.

Phase 26 (the client interface and the light hook; specified in `client-interface-spec.md`):
- 26a: `world_view::BrickLight` and `BulkLight`; `ScaledLight`, `LevelFormat`, `blend_brightness` and `combined_light` with their tests moved to Cave's
  `scaled_light` module unchanged. Cost of the hook: the per-brick time of a four-scale light was 2.5 to 2.8 ms before and 2.3 to 2.8 ms after (noise
  is about 15%). New mesher tests cover a hook lighting a face that looks into another brick and a hook being the same or another one; five
  mutations of the hook are caught.
- 26b: `game_shell::client_plugin` (`ClientPlugin`, `ClientRegistrar`, `ClientContext`, `ActionSpec`, `MouseButton`), `Session::{key_pressed,
  mouse_pressed, client_command, hud_text, client_plugin}`; `plugin_host::CommandSpec` takes a prefix and parses for the client too. Eleven
  mutations of the client host are caught.
- 26c: 2048 (`Client2048`), Eels (`ClientEels`) and Cave (`ClientLight`, `ClientBlockEditing`, `ClientMovement`) are client plugins; the golden
  sessions pass unmodified (the server stream is unchanged). The tests read a plugin's state through `voxunami_2048::game::progress`,
  `voxunami_eels::game::standings` and `voxunami_cave::game::light_scheme`.

Phase 27a to 27c (entities; specified in `entities-spec.md`):
- 27a: the `entities` crate (`EntityStore` with ids from a counter and a spatial index by brick, `ComponentBag`, `EntityKindSpec`, `ItemSpec`,
  `ItemStack`, `Inventory`); `Orientation` and `EntityPose` in `voxel_space`; `EntityId` and the pose wire format in `protocol`. 16
  mutations are caught.
- 27b: the plugin API (`entity_kind`, `item`, `Context::{spawn, despawn, entity, component, set_component, set_pose, entities_near}`, the events
  `EntitySpawned` and `EntityDespawned`, declared component access and its start-up ambiguity check); `WorldData` holds its entities, and
  `ServerWorld` has the calls. 12 mutations are caught.
- 27c: protocol version 3 (messages 9 to 11), server replication with cached views, the client store, replay of entities, version policy.
  Cost with 1000 entities and 4 clients: 0.5, 0.7 and 1.4 ms a tick with none, a tenth and all moving. 13 mutations are caught.

Phase 27e (a keypair per install):
- The `identity` crate: Ed25519 (`ed25519-dalek`), `Identity` (generate, load or create a key file, prove, hello, answer a challenge), `verify`
  (strict), `ChallengeSource` (operating system randomness, or a counter for tests). The key file is 32 raw bytes, mode 0600 on Unix, and is
  never overwritten if it exists with another length. Default path: the game data folder; `--identity FILE` overrides it.
- Protocol version 4: `Hello` carries the public key, `Prove` (client kind 6) and `Challenge` (server kind 12). The server refuses a wrong
  proof (another key, another game, another challenge), a key that is already playing (also while only challenged), and a client that does not
  prove in time. `ServerWorld::player_key(client)` tells the game who each client is. A join takes two ticks.
- Tests: the server's behaviour tests join by `log_in` (two ticks); `server::test_support::{log_in, test_identity}` is shared by the other
  crates' rigs; each rig gives its players different identities. New tests: identity once at a time (and again after leaving), two claims of
  one key, three kinds of wrong proof and the right one, no proof in time, a replay session sends only the hello and the proof.
- Goldens repinned once: stream +38 bytes (the challenge frame), welcome version 4, the game one tick older when the player arrives (Cave's saved
  world changes with it; 2048 and Eels do not). Verified in all three games by running the old code with an idle tick before each join: its
  stream and saved-world hashes equal the new ones's with the challenge taken out and the version set back to 3. The pinned recording of the
  engine's end-to-end test (84,749 bytes to 84,806) was repinned for the same causes, not checked that way. `following_the_server`'s level test
  had relied on the game's own tick-timed block not landing in the player's region; it now says what it means (the main level holds the
  player's brick and nothing of the second level).

Phase 27d (entities in the world file):
- `engine:entities` (see `world-file-format.md`): the counter, the entities by id, a reserved byte for the fallback pose of Phase 32. A world
  with none saves as before (the golden sessions' saved-world hashes did not move). Dormant entities need no special case: an entity is a kind
  identifier and bytes, so one whose plugin is missing is loaded, kept and saved as it was, and is not shown to clients. 9 mutations are caught.
- Server: a world saved in play and reloaded tells joining clients of its entities, and the counter keeps ids unique.

## Measured
Storage (Ryzen 7600X, release, one thread, best of 7, counting allocator; the single-voxel `get` figures include
loop and bounds overhead, so bulk decode will be faster):
- `get` 1.2-2.6 ns per voxel for every representation; `Grid::get` at random coordinates 9 ns; `Grid::set` 27 ns.
- Filling a brick with `set`: 4-8 ns per voxel for up to 10 distinct values, 19 ns at 100 (linear palette search).
  `from_values` (3 distinct) takes 1.9 ns per voxel, so generators should build whole bricks.
- Memory: a 67 M voxel terrain-like region (2048 bricks, solid layers plus a surface layer) takes 2.5 MiB.
  Uniform bricks cost almost nothing; a 3-value surface brick channel is 8 KB; a dense channel is 4 bytes per voxel.
- Cloning a 2048-brick grid takes 14.5 us; the first write to a snapshotted surface brick takes 2.3 us.

Light in the quad (Windows 10 machine, release, one thread; `docs/light-spike.md`): meshing a 416 x 416 x 256 voxel patch
of rolling hills with a light channel takes 2 to 8 percent longer than without one; quads grow 0 to 25 percent for
Minecraft-style light and 62 percent for 256 smooth levels around very dense lamps.

Rendering and loading (Ryzen 7600X; 1280x720; default rolling hills, 1 m voxels; offscreen, GPU time included):
- Integrated GPU (AMD Radeon, Vulkan): 6.9 ms per frame (146 fps) at view radius 12 (1.4 M quads drawn of 5.5 M
  resident) and 2.4 ms (416 fps) at radius 8. The target was 30 fps. The windowed viewer ran at 359 fps on it.
- RTX 3090 (Vulkan): 1.7 ms (600 fps) at radius 12; 0.6 ms at radius 8. An earlier run of the same benchmark
  averaged 4.5 ms with a 26 ms 95th percentile and did not reproduce in three repeats (probably the GPU clock ramping).
- Loading radius 12 (9171 bricks generated, 3366 meshed) took 4.2 s on 11 worker threads, so loading, not drawing, is
  the cost to attack next. Quad counts are high: 5.5 M quads (44 MB) at radius 12, from 1-voxel detail and caves.
- Memory per quad is 8 bytes; indexed triangles would be about 56.
- Through the game, which gets its world over the message stream (1f): the same scene gives 1.7 ms per frame on the
  RTX 3090 and 6.8 ms (148 fps) on the integrated GPU at radius 12, and loads in 4.0 s, so the stream added nothing
  measurable. The window ran at 60 fps (vsync) while recording.
- Only the 7600X's integrated GPU was measured; a weaker minimum-spec GPU is untested.

Cross-platform determinism: all tests, including the pinned terrain digests and the pinned recording of a standard
multiplayer session, pass on Linux (Ubuntu in WSL2, rustc 1.99.0) as well as Windows. Same CPU architecture and compiler version, so this does not cover other architectures.

Across machines (Phase 3): a Linux server (Ubuntu in WSL2) and a Windows client in separate processes, over TCP, drew a
picture byte-identical to a Windows-only run of the same seed: the client regenerates terrain from the seed and the
server's generator assignments, and both operating systems produce the same voxels. Only terrain was exercised (no
edits), on one physical machine; a real two-computer network test is still to do.

Terrain generation (default rolling hills, release, one thread): a surface brick takes 5.4 ms and a brick the bounds
classify as sky 0.26 ms. Sheet caves run through almost everything, so only 1 of about 1300 sampled underground bricks was
uniform stone: the bounds shortcut helps in the sky but not underground with this cave design. A column-constant
heightfield is re-sampled for each of 36 voxels of height, a known inefficiency left for when it matters. Extrapolation,
not measurement: at Game 4's voxel size, 50 m around the player is about 24 000 surface bricks, tens of CPU-seconds.

Simulation throughput (plain f32 7-point heat stencil, one channel, no branching, 6 cores / 12 threads):
- DRAM-bound 256^3 grid: about 1.9 G voxel-updates/s on 1 thread, about 4 G/s on 6 or 12 threads.
- Cache-resident 32^3 bricks: about 3.2 G/s on 1 thread, about 16 G/s on 12 threads (baseline SSE2 build).
- `target-cpu=native` adds only about 10-20%.

Caveat: that is the cheapest possible kernel on a strong desktop CPU. Real kernels (material lookups, state machines,
fluids) will be several times slower, and the minimum-spec CPU slower still.
Implication: 300 M voxels updated at 20 Hz needs 6 G updates/s for one trivial kernel, so most voxels must sleep.
Implication: a continuous-valued channel stored densely costs 4 bytes per voxel (1.2 GB per 300 M voxels), so
continuous channels need quantization or sparse activity.
