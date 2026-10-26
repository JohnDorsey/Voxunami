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

Phase 32 (moving grids; `moving-grids-spec.md`):
- 32a frames (`voxel_space::{GridPose, voxel_in_other, lattice_alignment, LatticeRotation, LatticeTransform}`): a pose's conversions between a grid and the world
  (positions in `f64` from a brick and an offset, directions), the 24 lattice turns and whole-voxel transforms as exact integers, `voxel_in_other` (the one
  cross-grid conversion; a centre on a boundary is in the floor voxel), and `lattice_alignment` (whether two grids are on one lattice, and the transform).
  16 unit tests and 5 properties (round trips, distances kept, composition, two grids on a lattice agree on every voxel in both directions); golden bit
  patterns checked against a separate Python calculation and equal on Windows and Linux. 21 mutations: 19 caught; 1 equivalent in practice (an exact tie between
  lattice turns); 1 redundant check (a range test that `checked_offset` already makes), which was deleted.

Phase 32h, after the owner's first play (2026-10-21): a crash and lag spikes, both found in the ship commands and the bodies plugin:
- **The crash** ("the brick has no edits and was just assigned a generator", in the server thread, on `/ship create` of a large built ship): `ServerWorld::generated_voxel`,
  which I wrote for `/ship create`, asked the world's cache of generated bricks, and on a miss for a brick that had been edited the code behind it hit an
  `unreachable!` (it assumed the brick had no edits, as every other caller guarantees by looking first). My test asked straight after the edit, while the brick was still
  in the cache of 128; a large ship is built over many bricks and the cache had forgotten them. Fixed by asking the terrain generator itself whatever has been
  edited. Tests: the server unit test that reads 2000 other bricks between the edit and the question (it panicked with the same message before the fix), and a Cave
  test that does the same through `/ship create` (checked against the old code: the same panic).
- **The lag** (half a second or more shortly after making smaller ships): measured with natural terrain and the real command: single ticks of 140 to 435 ms as a falling
  ship reached new ground, 200 ms when it was made. Each was the bodies plugin making 16 to 64 terrain bricks at once (10 to 25 ms each on the thread of the tick, most
  of it generating the neighbouring bricks that the colliders' seams ask about), because a ship had colliders for every brick within a brick of the bricks it occupies,
  and a ship of 16 blocks occupies a whole brick. A ship now has them for the terrain within 8 voxels of the bounds of its colliders (from Rapier, new
  `BodyWorld::bricks_near`) and five ticks of its speed, and lets them go 16 voxels farther out. Over 120 ticks of a small ship falling to natural ground the world
  generated 50 bricks, 24 in the most (375 and 90 before); the worst tick of the 4 by 4 raft is 17 ms (435 before), a 16 by 16 raft 25 ms, 30 by 30 82 ms and 40 by 40 73 ms.
  Also `touches_open` in `build_brick` looks at the faces of a box and not at all its voxels (the same boxes, pinned by a new test taken before the change: 79 to 38 ms
  for one brick of ground), `ServerWorld::bricks_generated` counts the generator's runs (the deterministic way to test this), and two ship physics pins changed because
  fewer far colliders change the order of Rapier's solver (two ships to the bit, the third, which slides and spins, within 0.005 voxel; checked by printing the poses).
- Not fixed, found while measuring (the walker's hitch, 109 and 136 ms in 400 sprinting ticks, was fixed next: see below); the 30 by 30 and 40 by 40 rafts still have single ticks of 70 to 80 ms; a ship has no continuous collision against terrain, so one falling at 250 voxels a
  second goes through a floor 3 thick (it does not at 138). Three mutants of the new lookahead survive (no distance ahead; no speed term; no hysteresis): they are safety margins
  that no scene can show, since the one that would, a faster fall, goes through the floor whatever the colliders.

Phase 32i, part 1 (ship files and the consistent save; 2026-10-21): ships are saved. The step was built in three parts, one after the other, and checked and committed as one, since the
parts changed the same files: files and the save (this), then the lifecycle (simulation distance, freezing, parking, loading on demand), then the questions to the player (a ship that
cannot be found, a ship that went away with its pilot and comes back blocked).
- `world_storage`: `ShipFileName`, `ShipFiles` (`DirectoryShipFiles`, `MemoryShipFiles`), the file of a ship (`ShipFormat`, `ParsedShip`: written from a world's
  numbering of content and read into another's), `ParkedGrid`, `WorldSave`, `engine:grids`, and a place to fall back to for entities in the frame of a ship;
  `MovingGrid` gained `on_pilot_logout` and `sections`. `WorldData::{save_with_ships, load_ship, park_ship, ship_format}`. The rest is in `moving-grids-spec.md` 2.8.
- `server`: `ServerWorld::{load_ship, park_ship}`, the rules told of a ship that came in (`take_changes`), `Server::{save -> WorldSave, with_ship_files,
  load_parked_ships, ship_files_mut}`. `game_shell`: the world file's folder of ships, for the autosave and for the save on exit.
- **A correction to the spec found while building:** the file is named for the number of the write, not the tick. Ticks begin again when a server starts, so
  a new run could write the name that the world file on the disk still names, and a crash between the two writes would leave a ship whose file does not match its
  checksum. The counter is saved in `engine:grids`. And the folder is `<whole name>.ships`, not the name with its extension replaced (two world files differing in
  the extension would have shared one, and the cleaning after a save would have taken the other's files).
- Tests: `ship_files.rs` (the file: round trip, the same bytes every time, a world that numbers content otherwise, every cut and every changed byte found),
  `ship_save.rs` (the set: names and numbers, loading one by one, tombstones with turns, other levels, a disk that fails at each call in turn, damaged and
  missing and swapped files), the entities aboard (`entities_save.rs`), `ships_in_files.rs` (a server and the next one from its save; the rules told), and Cave's
  `saved_ships.rs` (a ship in flight is where it was and moving as it was after a restart; a held ship stays held; a passenger who logged out aboard logs in aboard,
  at the same place in the ship, turned and moved as it is). Two pins, a ship file and a world file, were taken on Windows and are checked on Linux. (The three
  parts were checked and committed together, since they changed the same files; the totals of the whole suite are with part 3.)
- Mutations: 32, 30 caught at once. Two survived and the tests were wrong, not the code: one passed for the wrong reason (the retired id I changed was also
  the id of its own successor, which another check refused), and one never made an ordinary edit in the same tick, so the order of the two kinds of change was
  not tested. Both fixed and caught.
- The questions and the pilot's logout are part 3, below.

Phase 32i, part 2 (the lifecycle; 2026-10-21): ships are kept by how near the players are.
- New crate `engine/grid_lifecycle` (`GridLifecyclePlugin`, `squared_distance_to_bricks`) and `server::ShipSettings` (see `moving-grids-spec.md` 2.7 and 2.8 for what it
  does): frozen beyond the simulation distance, in a file beyond the unload distance, back when a player comes near or a player who was aboard joins. `MovingGrid::frozen`,
  `ServerWorld::{set_grid_frozen, grid_is_simulated, ship_settings}`, the bodies hold a frozen ship, `ActiveVoxels::take_simulated` and the two light rules wait for a grid that
  waits. `serve` has the three flags. Presence places a player in the frame of a ship that is in a file by the ship's saved pose.
- A design point the tests made me look at: with no player there is nothing near any ship, so the first ticks of a server (before its first player has a place) freeze and
  put away every ship, and a player who then joins has the ones near them brought back in the tick they arrive. That is the behaviour the owner described, not a bug; the
  test rigs of ships keep them going with `ShipSettings::everywhere`.
- Tests: `grid_lifecycle/tests/lifecycle.rs` (10: the tenth of hysteresis at both ends, between the two distances, the distance to a ship in a file from either side, other
  levels, a ship that waits for those aboard, a player joining aboard a ship in a file, a disk that fails and is not asked again at once, `everywhere`, each ship its own way), the distances, Cave's `ship_distances.rs` (a
  falling ship stops where it is, is put in a file, and falls on when the player comes back; a frozen ship hangs; what is in a ship is the same after a file; the light of a
  frozen ship waits), the two light rules and `ActiveVoxels::take_simulated`, the flags of `serve`, and the player in the frame of a ship in a file. Mutations: 26.

Phase 32i, part 3 (the pilot's logout and the questions to the player; 2026-10-21): 32i is built.
- `grid_lifecycle` gained the arrivals (`arrival.rs`: the hull, the look at a landing, the search for the nearest free pose, breaking terrain and blocks), the two questions (a ship that
  cannot be found; a ship that would land in something) and `LifecycleReport::waiting`. A game gives it the world's spawn and, for the look, what its blocks are to a body
  (`ArrivalConfig`). `ServerWorld::{set_parked_ship_waits, set_grid_on_pilot_logout}`. Cave: `ShipCommandsPlugin` puts a pilot's ship away with them (`/ship vanish on|off` says whether it
  should), and Cave gives the lifecycle its default camera as the spawn and its blocks for the look. See `moving-grids-spec.md` 2.8 for what happens.
- Decisions made while building, for the owner to overrule: (a) "someone else is aboard" counts only players who are connected: a passenger who is away keeps their place in the ship's
  frame, and the first of them to join brings the ship in; (b) the look at a landing is of the cells with blocks, not of the empty space of the bricks (a first version of the tests could not
  tell, a mutation said so, and there is a test for it now); (c) a ship that cannot be put in a file when its pilot leaves stays and goes on as it was, instead of stopping; (d) the questions
  are not saved; (e) a lost ship's "appear in the air" puts the player where they were in the world, at the height they had; (f) stopping a server does not run the logout of the players
  who are connected, so a pilot's ship does not vanish when a singleplayer window is closed (found while writing how to try this, from reading the code and not run: the window keeps its connection until the world is saved).
  Left alone, since making a stop mean "everyone leaves" changes what a recording ends with; it is in the plan's open list.
- Tests: `grid_lifecycle/tests/arrivals.rs` (24 tests) over a harness moved to `tests/common/mod.rs`, which `lifecycle.rs` now uses with its tests unchanged; Cave's `ship_vanishing.rs`
  (16 tests); the shared Cave harness gained `say`, `put_stone`, `block_at` and a disk that fails. Windows 1646 and Linux 1647 tests pass; fmt and clippy are clean.
- Mutations: 45 in this part, 42 caught as the tests were. Three survived. Two tests were too weak and are fixed (and the mutants caught): the look at a landing could not tell the
  blocks of a ship from the empty space of its bricks (a ship whose empty cells are in the ground now comes back without a question), and the check that a ship that goes is stopped looked at it
  after friction had stopped it anyway (it reads what is in the file now). The third, a guard that left the leaving pilot out of "someone else is aboard", could not be caught because the order of
  the handlers already did it; the order is now stated (`after`), which the host checks when it is built, and the guard is gone. All 58 mutants of parts 1 and 2 that still applied were run again on
  the final code and all were caught (one mutant of part 1 was split in two, for `load_ship` and `admit_ship`, and one of part 2 was replaced by those of part 3).

Phase 32j (fast ships stop at thin ground; 2026-10-21): `BodyWorld::step` cuts a tick into more solver steps while any point of a dynamic body (its speed plus its turn rate times
the reach of its colliders' boxes) would move more than 0.8 voxel in one: the settings' 3 under about 48 voxels a second, 24 at most. A tick in which nothing is fast is cut as before, bit for bit.
- Tests first: a table in `physics` (cubes of 1 and 4, floors of 1 and 3, 100, 250 and 400 a second) that fails with the cap at 3 (a cube of 4 at 400 ended 780 below); the step counts (3, 7, 24);
  Cave's `ship_physics.rs`: a ship from 1000 above the flat floor stops on it (with the cap at 3 it ended at height 103, on the hills 98 below the floor). Cost, measured with a throwaway
  test: a ship of 125 or 1000 blocks at 400 a second costs about 0.05 to 0.07 ms a tick in free flight; only fast ones pay, and only while fast.
- Pins changed, for the reason in the design: the spinning ships of two scenes cross 48 for a few ticks (2 ticks of 5 steps in `physics`'s busy scene). `bodies.rs` `13_668_004_898_244_483_961` to
  `8_592_658_610_343_128_269`; `ship_physics.rs` poses `4_233_839_071_203_554_194` to `15_101_816_868_498_766_766` and ships `8_675_827_597_625_099_639` to `8_817_648_817_650_209_632`. Windows and Linux agree
  (1649 and 1650 pass; fmt and clippy clean). Mutation: the cap at 3 is caught by both new tests.
- The spec's "4 above" puzzle: a cube that tumbled rests on another face, so its origin corner is a cube's width up. Not a bug; the tests say "on top of the floor", not "origin on it".

Phase 32k (terrain colliders on a budget; 2026-10-22): `grid_bodies` splits the terrain a ship wants into the *needed* bricks (its bounds, two voxels and a tenth of a second of its speed around them: always made in the
tick) and the *wanted* rest of the lookahead (8 voxels and a quarter second of speed), which is made nearest first (by rings of 8 voxels, ties by coordinate), **one brick a tick**. A count, not a time, so every machine
simulates the same. `GridBodiesPlugin::report` (bricks made last tick, most in a tick, in all) is what the tests count; Cave has `cave_plugins_reporting`.
- Measured on this machine (the worst tick's terrain work, 150 ticks of a raft falling to natural ground, a throwaway timer in the plugin): 30 by 30: 148 ms before, 74 now; 40 by 40: 191 before, 60 now;
  ticks over 20 ms in the terrain work: 7 and 6 before, 6 and 4 now (a budget of 2 had 16 and 14, which is why it is 1). Bricks generated in the worst tick: 24 and 31 before, 12 now.
- **What it did not fix, and the spec's goal of "no tick over 30 ms" is not met by it:** what is left in the worst ticks is not the colliders of terrain but the first tick (the raft is *made* inside the surface bricks, which are
  needed: about 60 ms, paid once) and Rapier's own step for a big ship in contact with the ground, 25 to 145 ms a tick for 900 to 1600 blocks (a voxel collider against voxel colliders). That is a separate cost and
  a separate step if the owner wants it; the generating of natural terrain is the other (plan.md, Open).
- Tests: the counts of a raft held in the sky (5 bricks in the first tick, the 4 needed and 1 wanted, then 1 a tick up to 9; caught with the budget at 0 and at 100), the generation counts of 30 by 30 and 40 by 40 rafts
  (at most 14 in a tick), and the existing falls (300 and 1000 above) still stop on the ground, so a ship does reach only bricks that have colliders. Not done: the property test of the spec (a ship's path with the budget
  is the path without it when the lookahead is long) -- the falls and the pins say it for the scenes we have. No pin changed. Windows 1651 and Linux 1652 pass; fmt and clippy clean.

Phase 32l (splitting; 2026-10-22): a ship that is cut in two becomes two ships. See `moving-grids-spec.md` 2.7 for what happens.
- `grid_bodies::connectivity` (the flood from the seeds, a count of voxels at a time, pure and tested alone: a row cut in two costs about the smaller part, a hole in a plate of 1600 costs
  under 16, a block that hung by one face costs nothing, and the answer is the same for every budget), `ServerWorld::split_grid` and `SplitPart`/`SplitError` in `server`, the check and the
  centres of mass in the plugin (`voxel_moments` was taken out of `moments_of`, so one voxel's mass is worked out in one place), and the pilot of a split ship in Cave.
- Tests first: the connectivity tests; four of `split_grid` (voxels, speeds with the turn, flags, a tombstone; what is aboard goes to the part it stands on, else the nearest; the rules told; every
  error changes nothing); in Cave a plank cut across falls as two ships that rest on the floor (40 and 36 blocks), a hole and an end and a corner leave one ship, a ship of 15,000 blocks is whole for
  3 to 6 ticks and then two, and a pilot has the controls of the part they are on. Mutation (by hand, not the whole harness): the budget unlimited, the pilot not followed, the rest of the ship dropped,
  the turn left out of the speed of a part: all four caught. Windows 1666 and Linux 1667 pass; fmt and clippy clean.
- One test of the whole suite failed once in the full Windows run: `screen.rs` `a_replay_draws_the_form_exactly_as_it_was_drawn_live...`, one pixel on one GPU adapter. It passed three times alone and has
  nothing to do with ships; I did not find the cause, and say so rather than call it nothing.
- Known: parts are in the order the pieces were found and then the rest, so the part with the most blocks is usually last; nothing in the game depends on it. A ship edited every few ticks is not split until the
  editing pauses. Merging and docking (32m) is next.

Rules aboard (2026-10-22; the owner set fire to a ship they were flying and it did not burn): found, measured, fixed, and checked.
- *Measured first:* the same plank of wood (144 blocks, one fire on it) with a player beside it: as terrain it burned to 3 blocks in 200 ticks; as a ship, nothing burned in 600 ticks and the fire
  did not even age. *Cause, confirmed by changing only that:* `plugins/fire.rs` decided whether a fire was near a player with `Some(player_level) == grid.root_level() && pose.distance_to_voxel(at)`. A
  ship has no root level, so no fire on a ship was ever near anyone, and the rule left it waiting. With only that test made ship-aware the plank burned (and burned apart: the ship was split into
  pieces by 32l, which was a pleasant check that the two compose).
- *My part:* in 32i I wrote "fire never ran on a ship, so it has nothing to keep" and listed it as a known limit. That was a consequence of the same defect, noticed while thinking about freezing and
  not tested.
- *Fixed once, for every rule:* `world_storage::GridFrame` and `WorldData::{grid_frame, voxel_in_world}` (where the voxels of any grid are in the world, including a ship in a file),
  `player_presence::{PlayerPose::distance_to_voxel_of, PlayerReach}` (how far players are from a voxel of any grid; `PlayerReach` owns its snapshot so a rule can hold it while it writes). Fire uses
  `PlayerReach`; block editing's twenty hand-written lines for ships are now the same call. Terrain distances are the same numbers as before (the golden sessions did not move).
- *Checked, not trusted:* `tests/ship_parity.rs` with `tests/common/parity.rs` (a `Site` in the terrain or in a ship, the same blocks at the same offsets): fire burns a plank away in both places; fire
  waits while no player is within its range and burns when one comes, in both; a lamp lights the same values and its removal fades the same values in both (to the value, at 0, 30 and 400 ticks, on a
  site that is whole bricks from the origin: the light works in cells of bricks, so the same offsets must fall in the same cells); a block can be changed within reach and not beyond it, in both. The
  two fire scenarios failed on a ship before the fix; the light and editing ones passed (they were already right). The harness reads a ship through the parts it was cut into.
- *The audit* (the open list in `plan.md`): the design is sound for what a rule does to voxels (everything is keyed by grid, which is why light, bodies, freezing and splitting work on ships), and leaks at
  the edges: questions about the world (now one call), the level-keyed voxel calls, grids not touching, no use of the ship's own down. Windows 1745 and Linux 1746 tests pass; fmt and clippy clean.
- A flaky test of my own, found by the full Linux run and reproduced under artificial load: one fire sometimes burns out without catching anything (about one in six), and which one depends on the tick it
  is first looked at, which a loaded machine can move; so the scenario with a single fire failed now and then on either place. It now has nine fires, and passed eight runs in a row on Linux with every core busy.
- Observed, not investigated: the light of Cave fades slowly after a lamp is taken away (63 to 43 to 21 over 400 ticks, in the terrain as well as on a ship). It looks like how that light scheme works.

The level-keyed voxel calls are removed (2026-10-22; the owner: pre-alpha, no modders yet, be bold). `ServerWorld::{voxel, set_voxel, uniform_value, generated_voxel}` are now crate-private
(`root_voxel` and so on: the root-grid half of `GridView`), and `GridView` gained `uniform_value` and `generated`, which a ship answers too (a brick nobody made is the default throughout; nothing generates
a ship). `ServerWorld::grid` of a level that is not there says `NoSuchLevel`, as the old calls did. `detach_blocks` takes a `GridId`: out of terrain it begins at rest as before, out of a ship the new ship is
where the blocks were, turned as the ship is, going as the ship was at their middle (so docking can undock later). The 2048 board (`motion.rs`, `logic.rs`) is keyed by `GridId`; Eels' arena and Cave's
callers (ship creation, the bodies' terrain, the arrival look, the light-model switch) use `world.grid(..)`.
- *Found on the way:* switching Cave's light model cleared and re-seeded light only in the terrain's bricks, never in a ship's (the same family as the fire). It reaches ships in memory now.
- Tests: the existing ones, rewritten by a tool for the calls (`regrid.py`, a scratch file) and by hand where a test expected an error for a level that is not there; two new for `detach_blocks` out of a
  ship and for `uniform_value` and `generated` on a ship. No pin moved. Windows 1745 and Linux 1746 tests pass; fmt and clippy clean.

Phase 40a (planets on rails and from data, step 1: the world clock; 2026-10-26). See `rails-and-recipes-spec.md`.
- *Asked (owner):* "work on planets on rails and data-driven planets". I wrote the design first (`rails-and-recipes-spec.md`: the clock, generator recipes, bodies in data, rails, kinematic frames, six steps, five questions with my defaults, the risks); this is its first step.
- *Built:* `WorldData::{clock, set_clock, advance_clock}` (whole ticks, saturating), the section `engine:clock` (one varint; written only when the clock is not 0, after the entities; refused when damaged or newer), `Server::step` advances it once a tick, `ServerWorld::world_tick()` reads it. The server's own `tick` is still the count since the start; nothing in the stream, the messages or a recording changed.
- *The one visible change:* every golden session pins the hash of the saved world, and a world that has ticked now has a clock section, so the third number of the 2048, Cave and Eels goldens moved (the first two, the stream's hash and length, did not: that is the check that nothing else did).
- *Tests:* `world_storage/tests/clock.rs` 5 (starts at 0 and advances by whole ticks with saturation; no section at 0 and a resave is the same bytes; not in a description; one number and a damaged one is refused; a newer version is refused), Cave `clock.rs` 1 (a world ticks once a tick and a saved and loaded world goes on from there). **Mutation experiments:** 8 mutants (two ticks a step, none, written at zero, written in a description, wrapping, not loaded, not parsed, trailing bytes accepted), all caught.
- *My mistake from the commit before:* `level_prefetch.rs` was committed with an import it did not use (clippy `-D warnings` fails on that test target); found when the gates ran here, removed.
- Windows 2133 tests pass and Linux 2134 (the one more is the test of a file's permissions); fmt and clippy clean.

The terrain of levels made ahead by the worker (after 39g; 2026-10-26). See `planet-spec.md`, "Bricks made ahead by a worker".
- *Asked:* "continue" (the next thing I had said: the worker could serve the terrain of levels).
- *Built:* `ServerWorld::prefetch_level_bricks` (takes `&mut`, because it gives an unassigned region its generator, as the first read would; the worker's bricks are keyed by the level's root grid like those of reads) and `generated_brick` takes in finished bricks before it runs the generator; Cave's `step_in_a_level` asks for the 5 x 3 x 5 bricks round the player.
- *Measured before building (release):* a walk over the hills for 400 ticks cost no tick over 1 ms but the first: the walker moved too little (54 voxels) to say anything. Hops of thirty voxels, twelve of them: the worst ticks 23, 18 and 14 ms without the request and 4 ms with. So the gain is real and small: the overworld's bricks are cheap (a planet's are 5 ms each and a hop was 100 ms).
- *Tests:* `server` 1 (once, remembered not asked again, a read finds them made), Cave `level_prefetch` 1. **Mutation experiments:** 4 mutants, all caught or not buildable (one named a key the type does not allow); the test of the read took the worker's bricks in itself at first, which hid that a read does: it polls the generator's counter now.
- Windows 2127 tests pass and Linux 2128 (the one more is the test of a file's permissions); fmt and clippy clean.

First contact with a planet made ahead (after 39g; 2026-10-26). See `planet-spec.md`, "Bricks made ahead by a worker".
- *Asked:* "continue", on the wait the first time anyone meets a planet (about 0.5 s, from 39d).
- *Built:* `landing::warm_up_the_poles` (the first run of the walkers' system asks for the 5 x 5 x 5 bricks round the pole of each planet) and, in `step_in_a_level`, the 5 x 5 x 5 bricks round a player in a level in the planet's coordinates for each planet in their reach; `ServerWorld::bricks_prefetched` takes in what the worker has finished before it answers (it counted 0 until the next request otherwise: a test found it).
- *Measured (release):* the worst server tick when a player joins at the pole after the world has had two seconds: 300 ms without, 105 ms with 5 x 3 x 5 bricks, 51 ms with 5 x 5 x 5; a player falling toward the planet from 100 and 400 voxels: 187 ms to 91.
- *My mistakes:* both first tests of the new requests passed on the mutant, because the count they looked at was made up of other requests (the poles' bricks were still being made when the fall began); the tests now wait for the poles and count the difference. The first warm-up sat in the join path and counted its own synchronous bricks (the pole's height is found by reading the column) as the worker's; I looked at wall time of the server tick and not the counts.
- *Tests:* `cosmos` 1 (the poles are made before anyone arrives), `planet` 1 (a faller's bricks are made by the worker). **Mutation experiments:** 5 mutants: one equivalent (every ship asked, which ignores a ship with no terrain).
- Windows 2125 tests pass and Linux 2126 (the one more is the test of a file's permissions); fmt and clippy clean.

Undocking from a planet, and a look for what else cannot be done (after 39g; 2026-10-26). See `planet-spec.md`, "Undocking from a planet" and "Looking for other things that cannot be done".
- *Asked (owner):* make it possible to undock a ship from a planet; look for other inconsistencies and weird things we cannot do.
- *Built:* `Docking::undock_from_planet` (built blocks, sides, which side stands on the ground, `detach_blocks`, separation, released pair), `levels_ports` includes the planets' ports (or the released pair was never forgotten and the ship could never dock again: the test puts it back and it docks), `UndockError::{BothSidesOnTheGround, TooManyBlocks, Detach}`. Cave: the commands that move or steer a ship refuse a planet; a pilot is not followed into a planet; items fall along gravity (`down_for`), are removed by the rule of their grid (`fell_out_of_the_world`), and are picked up in every grid of the level.
- *Found by trying* (all but the last had a test that failed first, or a scratch run): `/ship hold off|push|pilot|vanish on` on a planet were accepted; a pilot who docked with a planet kept "piloting" it and could not walk (the test walked 0 voxels); survival did not work on planets or ships at all (an item lay in the grid of the block and nothing picked it up; I had marked it "from reading" and then ran it: the item lay at the voxel the player stood in for 120 ticks); `/tp` deep into a planet leaves the player stuck in the rock (seen, not changed).
- *My mistakes:* the first floating-sides test had stones side by side, which are welded (a good refusal, a bad test); the first test of the ship's survival pickup broke a block seven voxels away (out of reach, correctly not picked up) and I took the failure for the code's; I wrote a garbled assertion in a hurry (`...then_some(()).map_or_else(...)`) and replaced it; the first mutants of the undock named text that rustfmt had changed.
- *Tests:* `docking` 5 (undock and re-dock, both on the ground refused, floating sides, a pilot who docks walks, the player sees it dock and leave), `planet` 2 (commands refused, second planet 200 km away), `cosmos` 4 (survival pickup on the planet, items on the side of the planet, an item in space and one far to the side, the bottom of a level and of a ship), `flight` 1 (survival pickup from a raft). **Mutation experiments:** 27 mutants of the undock, the command guards, the pilot, the pickup and fall of items: the first run missed 11, of which 5 were text that had changed and 3 were given tests (no gravity, the void under a planet and under a level, a ship's void); **not pinned:** the speed and direction of the separation, a side that stands in ground and not beside it, the 100,000 limit, and pickup in another level.
- Windows 2123 tests pass and Linux 2124 (the one more is the test of a file's permissions); fmt and clippy clean.

Docking with a planet, and `/ship create` on any grid (after 39g; 2026-10-25). See `planet-spec.md`, "A ship docks with a planet".
- *Asked:* "continue" (the docking the owner's stance of the same day names: a ship's blocks are written into the planet's grid and its id stays), and, in the same round if cheap, that `/ship create` works at blocks of ships (the owner found it says "you are not looking at a block" in `mini_cosmos`; it only looked in the level's terrain). It was cheap.
- *Built:* `ServerWorld::dock_into` and `GridStore::absorb` (a tombstone naming a grid that stays); `merge.rs`'s carrying of a voxel's values is one function (`carried_values`) for a merge and a docking; `Planet::ports` and `Bodies::scan_planets_for_ports`; `port_pairs` over ships and planets, in order of id; `Bodies::dock_into_planet`; `SplitError::HasTerrain`. Cave's `/ship create` casts in the level and in every ship of it, takes the nearest hit, and detaches from that grid.
- *Found on the way:* (1) `/ship undock` on two docked ports built on a planet would have cut the planet in parts and destroyed it (the guard in `split_grid` is new). (2) My first test built the port before the ship, so the ports of a brick that changed after the planet had a body were never needed, and a mutant that never rescanned them was missed; the test builds it after. (3) A refusal that restores the ship's pose is not observable the way I wrote the test (the ship is at rest in contact either way), so "a refused planet docking is tried every tick" is not pinned: the gate is the same code as for ships, which has a test.
- *Tests:* `server` 4 (`dock_into`: happy, turned a quarter with a walker, refusals; `split` of a planet), `world_storage` 1 (`absorb`), Cave `docking` 3 (a ship docks, and the world is saved and loaded; a flame makes it refuse; a held ship is left alone), `flight` 1 and `planet` 1 (`/ship create` on a ship and on a planet). **Mutation experiments:** 20 mutants (plugin 8, merge 8, store 3, split 1; the missed ones rerun); two missed ones were given tests (ports of a changed brick; a held ship near a port), one is equivalent (a planet counted as a mover), one is not pinned (above), and one would need a second level in the merge test (`other levels`; not done).
- Windows 2111 tests pass and Linux 2112 (the one more is the test of a file's permissions); fmt and clippy clean.

Bricks made ahead by a worker (after 39g; 2026-10-25). See `planet-spec.md`, "Bricks made ahead by a worker".
- *Asked:* "continue", on the second of the open issues I raised: brick generation on the thread of the tick.
- *Built:* `server::prefetch` (a worker thread started at the first request), `ServerWorld::{prefetch_ship_bricks, take_prefetched, bricks_prefetched}`; the cache of generated bricks is 256. `grid_bodies` asks for the planet's wanted bricks left for later and the bricks beside them; Cave's `step_aboard` asks for the 75 bricks round a walker on a planet.
- *Measured, and the first guess was wrong:* I said the stall was brick generation. A walker's worst tick on new ground was 37 to 43 ms, all in `update_terrain`; generating a brick is 5 ms and reading it 2 ms, and what costs is that `build_brick` looks at the voxels beside its brick, so it makes the six neighbours. Asking for the 27 bricks round the walker gave 26 to 28 ms; asking for 75 (5 x 3 x 5) gives 8.5 ms. A ship skimming the ground at 60 m/s: 15 ms to 11. (The first request I wrote also put a lock-and-poll of the worker in the path of every voxel read; it showed in the numbers and is now only on a miss.)
- *Tests:* `server` 2 (the worker makes what it is asked once, past the extent nothing, remembered bricks are not asked again, and a read finds them made, with the same values as a world that made them where it read them; the prefetcher alone: once while asked, again once taken), Cave `planet` 2 (a ship falling toward a planet and a walker on it are served by the worker; these give the ticks the milliseconds a server's have, because the worker is a thread). **Mutation experiments:** 14 mutants of the worker, the world's side, `grid_bodies` and the walker; the first run missed 8, of which 5 were the wrong test filter (the name of the test did not hold the word I filtered by), one an equivalent no-op I wrote, and two real (a remembered brick asked again; a brick that was taken never asked again), which have tests now. Not pinned by any test: the ring sizes, and that the ahead list adds to the bricks beside (the falling ship's test passes on either).
- Windows 2102 tests pass and Linux 2103 (the one more is the test of a file's permissions); fmt and clippy clean.

Frames, and `/ship distance` (after 39g; 2026-10-25). See `planet-spec.md`, "Frames".
- *Asked (owner):* Cave is to become a space-exploration game (KSP, KSA), so ships will someday need to go on rails when far from the players or in a time warp (added to `future-goals.md`, unscheduled); until then simulation just stops beyond a distance of the nearest player, a server setting with a command to change it.
- *Found:* the setting already existed (`ShipSettings::simulation_distance`, 256 voxels by default, `--ship-simulation-distance`) and the lifecycle already freezes ships beyond it; what was missing was the command, and that **the bodies of a level were in one frame around the first ship of the level**, so ships near a player 16 km or more from that ship were not simulated at all (the issue (h) of 39g, verified then).
- *Built:* `grid_bodies` keeps a `Frame` for each place of a level (`FrameKey` = level and a number): a ship is put in the first frame of its level whose origin it is within 500 bricks of, or makes one centred on it; a ship that goes 1,000 bricks from the origin of its frame is taken out and put where it is; planets get a body and the level's terrain colliders are kept in each frame; docking looks at the ships of a frame.
  `BodiesReport::frames`. `ServerWorld::set_ship_settings` is public; the Cave command `/ship distance [SIMULATE [UNLOAD]|everywhere]` shows the setting or changes it (the policy for a pilot's logout stays).
- *Found on the way:* the first version of the command took `/ship distance 5 far` as `/ship distance 5` (an unreadable second word was dropped); a test of the usage found it. Two mutants were equivalent (the arm that takes three words only to refuse them, which I took out; a ship that is gone being tested twice).
- *My mistakes:* a duplicated block when I cut the file for the tests (I used the index of the first "Space" comment, which came before my new tests, and the tail of the file was written twice; the compiler said so at once and I cut it back); and two mutants that the harness could not read because they ended in an empty replacement (it needs a line).
- *Tests:* `ship_physics` 3 (four ships 0, 10, 40 and 400 km apart all fall, in three frames; a ship put 60 km away has a frame of its own and goes on; the same place in two levels is two frames; the old test that a far ship stays where it is is gone), `flight` 2 (the command shows, changes, refuses, keeps the logout policy; a ship beyond the distance stops and goes on when it grows). **Mutation experiments:** 20 mutants (9 of the frames, 11 of the command): all caught but one that was equivalent, a second check for a ship that is gone.
- Windows 2098 tests pass (4 ignored) and Linux 2099 (the one more is the test of a file's permissions); fmt and clippy clean.

Phase 39g (ships meet a planet, and the same weakness elsewhere; 2026-10-25). See `planet-spec.md`, "Ships meet a planet".
- *Asked (owner, 2026-10-25):* planets are a kind of ship with slightly different rules, sharing the code of ships; all planets will be on rails, so they are never docked to each other or to a moon; a ship that docks with a planet is written into the planet's grid and the planet's identifier stays. Then: make ships collide with generated terrain that has not been modified, and look for similar weaknesses, fixing the straightforward ones
  and bringing the rest up.
- *Built:* `BodyWorld::bricks_near_in` (the bricks of another body's own coordinates that a body is near) and `BodyWorld::remove_brick`; in `grid_bodies` a planet has a fixed body (`Planet`, made once the level has a simulated ship, never keeping a level alive), and `Bodies::bricks_to_change` is the one piece of code that chooses which bricks of the level's terrain *and* of each planet get colliders (`Ground::{Level, Planet}`); a planet's bricks are made with
  `set_brick` from a `GeneratedHull` and are not limited to the generator's extent; `PLANET_REACH_BRICKS` (4,096) for the `f32`. `Obstacles` (where a returning ship may land) takes the ground of a planet as terrain, `break_terrain` takes the grid of each voxel. `GridLifecyclePlugin::new` takes the spawn as a rule of the world and the level (`ArrivalConfig` has `air`); Cave's is `landing::spawn_in`. `merge_grids` refuses a ship with terrain.
- *Found on the way:* (1) **the world's spawn put a pilot inside the planet** (`Go to the world spawn`, from a lost or a blocked ship; the pole is where players arrive, and the lifecycle had its own constant): the test put them at (12, 94, 38) in the rock before the change. (2) A returning ship could land in a planet's unmodified ground and nobody was asked. (3) The first version of the planet's colliders cut the bricks by the generator's extent, and a slab built past it was fallen
  through (a test found it; the extent is gone from the choice). (4) **Free ships are simulated only within 16 km of the first ship of their level** (verified; not fixed, see `plan.md`, Open). (5) Dropped items fall along the grid's -y and go at -1000, which is wrong on a planet (from reading; not run). (6) A cube flush against a one-voxel step on a planet, pulled into it, jitters at about 0.01 m/s and never sleeps (seen; the physics crate alone and a small floor did not show it; the
  tests keep cubes half a voxel off the step). (7) **My mistakes:** I took a worthless mutation run for a result. I started the harness with `nohup ... &` from a subshell, looked with `ps aux` (which cannot see Windows processes), saw nothing and started it again, so two harnesses rewrote one file and one output file; I stopped the second, waited for the first, checked the file against the mutants' text, and ran it again by itself (recorded in the memory notes). The first test of an
  arriving ship also asserted a ship stayed in its file that the rig's settings bring in, which I took out. And my first guess for why the cube did not sleep was the `f32` at 63 km, which a physics test with the planet's distance did not support: I should have said "not understood" earlier than I did.
- *Seen:* a cube let go 10 voxels over the planet's unmodified ground comes to rest on it within 0.05 of the ground's height, to the bit after 40 more ticks, at 63 km from the frame's origin, and on a planet turned a quarter and moved; the bricks made for it are 16 in 60 ticks (8 of them the level's), 8 at most in one tick.
- Windows 2094 tests pass (2076 before; 4 ignored) and Linux 2095 (the one more is the test of a file's permissions); fmt and clippy clean.

Phase 39f (planets as ships, step 7: the level `mini_cosmos`, a planet in it, and the default; 2026-10-24). See `planet-spec.md`.
- *Built:* `terrain::empty` (`EmptyTerrain`: air everywhere, nothing may show) registered as `base_blocks:empty_v1` after the planet; `ContentPackage::new_world_of` (a world of several levels, each with its own default generator); `CaveContent::new_world` is `mini_cosmos` first and the overworld second with the planet made with it (a held ship at the origin, `small_planet_v1`, 2,100 bricks each way, the mass that pulls 10
  at its surface), and `new_overworld_world` is the world Cave began with (the tests that start in the overworld, the golden session, the dedicated servers of the network tests, which now start from an overworld world file). `voxunami_cave::landing::on_a_planet` (the ground at the pole of the planet, edits counted, in the planet's axes); a new player whose client began at the default start stands there, one who asked for another place (`--camera`) is
  put there; `player_presence::Presence::set_level_arrival` (a rule from the level they are in to the one they are sent to), which Cave gives (the pole of a planet; where players start when they come from a level with a planet; the same place otherwise); the bodies of Cave follow the record when the level changes (they kept their own place); a flags byte for a ship in the world file's list (bit 1: it has terrain) and the lifecycle
  brings a planet back from its file whatever the distance. `GameDefinition::grid_detail` was done in 39e; Cave's planet is drawn in detail.
- *Found on the way:* (1) a world that was saved comes back with every ship in a file, and the lifecycle brought a ship back only when a player was within the simulation distance of it, so a planet would have been missing for a player farther than its extent (67 km) until one came near: the flag in the file and the bring-in. (2) The body of a player ignored the record's place when the level changed, so no rule for where players arrive could
  work (`/tp` goes round it): it follows the record now. (3) With no rule, `/level go mini_cosmos` from the overworld would put a player inside the rock of the planet, and `/level go overworld` from the pole 64 km up in the overworld's sky; and a new player's client began at (16, 110, 16), which is inside the planet: the server pushed them out sideways by tens of voxels (the way a body is moved out of ground, which is meant for a few voxels). (4) My mistakes:
  the test of a planet brought back from its file parked it in an action that runs before the lifecycle in the same tick, so it was back before the test looked, and I had cut the output that said so (the run of the whole suite stopped at it, because I had not asked for `--no-fail-fast`: always do); a `git checkout` of `content.rs` to undo a temporary change of the ambient light threw away the uncommitted 39f edits to that file, which I wrote back from the
  script that made them (recorded in the memory notes); and the first pictures from space were empty because a player's pitch is relative to the local horizon, whose down is the planet.
- *Seen (headless screenshots, ambient light raised for them: Cave's is 0 and the terrain is black):* the pole of the planet (rolling green hills, the cave sheets breaking the ground, the far hills as nodes out to the horizon), the horizon from 2.8 km, the planet as a round disc from 230 km and 400 km (angular size as it should be), a view from 76 km up. The first picture from the ground took 8.7 s in the dev profile.
- Tests: Cave `tests/cosmos.rs` 11 (the new world's levels, generators, ship and mass; the overworld-only world; the planet after a restart with what was dug; where a player lands: ground, edits, a whole brick of stone built over it, a planet that is moved with an ordinary ship before it; the first player arrives standing on the planet wherever their client said it began; a player who asked for a place is put there; `/level go` both
  ways; breaking and placing a block on the planet through the client), `terrain` 3, `base_blocks` 1, `game_shell` 1 (a world of several levels), `world_storage` 2 (the flag in the list and the byte), `grid_lifecycle` 1, `player_presence` 1. The golden session of Cave moved once: +21 bytes, the identifier of the generator in the table, 7,190,161 to 7,190,182 (Windows and Linux agree). **Mutation experiments:** 44 mutants of the
  landing, the content, the spawn, the level change, the arrival rule, the presence, the lifecycle, the sections of the world file, the world and the package; the first tests missed 8 (the skip of a brick of air, which a whole brick of stone built over the pole shows; the planet's own axes; a ship that is not a planet found first; the arrival rule given the level they come from, and the one for levels with no planet at either end; the record at spawn). All got a test but the last, which was
  removed (the spec default of the first level was not found by the first run of that mutant, which had the text of the code before it was formatted, and is caught): the body overwrites the record in the same tick, so setting it from the placement shows nowhere.
- Windows 2076 and Linux 2077 tests pass (the one more on Linux is the test of a file's permissions); fmt and clippy clean.

Phase 39e (planets as ships, step 6: levels of detail for a planet; 2026-10-24). See `planet-spec.md`, `lod-spec.md`.
- *Built:* `world_view::CoarseTerrainSource` (version, `may_show`, `coarse_generator`, optional `solid_core`; `LevelData` and `ShipTerrainSource` implement it) and `LodView::{update, settle}(grid, source, viewer, settings)` in place of `(level_index, level, ...)` (the 32 call sites of the existing tests changed by a pattern and nothing else; they pass
  unmodified in what they assert); `ShipTerrainMesher::with_detail` / `set_detail` / `update(source, viewer: WorldPosition)` (a detailed mesher wraps a `LodView`); `MeshUpdate::{UploadGridCoarse, RemoveGridCoarse}` (applied by `game_shell::view` with the renderer's existing calls); `Session::enable_grid_detail`, `GameDefinition::grid_detail` (Cave: 10^7 voxels, light 0, cells of 3
  pixels), the fog reaching as far as the farthest terrain drawn; the server tells every client of a level of every planet in it, and the lifecycle never puts a planet in a file; `TerrainGenerator::solid_core` / `terrain::SolidBall` (the planet's core: its radius less the square root of three times the greatest height of a face, less a voxel) and
  `world_view::horizon::is_hidden_by` (a cube's bounding sphere against the cone of the lines that touch the core, a margin of a brick's diagonal for the viewer's moving within its brick), used by the selection of the view.
- *Measured (release, 4 threads, Cave's planet, 1080 pixels):* from 2 m over the ground 12,879 nodes and 1,334 bricks, 4.8 million quads, settled in 6.0 s (without the horizon 18,764 nodes, 9.2 s); from 1 km 9,929 nodes, 3.2 s (14,962, 5.4 s); from 30 km 1,995 nodes, 0.6 s (3,131, 0.85 s); from 500 km 36 nodes. `cell_pixels` 4, 6, 8, 12 at the surface: 7,947, 3,997, 2,596 and 1,322 nodes.
  The horizon of the core takes a third of the nodes, not the 97% to 99% of the spike: the core is far lower than the ground, so its horizon is 8.4 km from the surface and the real one is half a kilometre. A tighter cull needs the heights of the ground per direction from the generator.
- *Found on the way (my mistakes):* the first test of the session expected the bricks of a view in detail to be those of the plain mesher; the fine region of a view in detail is not the same sphere (a few more bricks), so the test says the plain ones are drawn the same inside it. A finer picture gives no more nodes at this distance (the finest nodes are as fine as nodes are), so the test of the picture's size is of a coarser one. The extent test first
  looked only at cubes with nothing wholly beyond the extent; it did not pin the ends of a cube against the box, which the version that compares with the ground without an extent, at sixteen extents, does (cubes that reach in by their first brick only and by their last brick only both occur). And a `powi` and a `sin` in a test, which clippy refuses (the workspace bans them even in tests).
- Tests: `terrain` 2 (the core is ground at 20,000 places in it and not much deeper than it must be; no core for a planet too small), `world_view` 9 in `ship_lod.rs` (a planet is drawn exactly as a level with the same ground; the extent at sixteen widths; no generator, bricks alone, detail changed, a stored brick, a reset; a round planet's far side is not made and the near side is
  as it was; the horizon goes with the viewer), 1 for a view of another grid, 1 for the terrain version, 8 in `horizon` (the cull against the geometry: every corner and sampled place of a hidden cube is behind the core, for viewers anywhere within the margin, in the cubes near the edge of the shadow and in a dense scan where it begins), `server` 1 (a planet is
  told of from anywhere), `grid_lifecycle` 1 (never put away), `game_shell` 7 in `planet_view_tests` (nodes beyond the bricks, from afar, a recording, the distance and light of the nodes, a coarser picture, detail asked for later, the fog), Cave 1 (what the game declares). **Mutation experiments:** about 70 mutants of the mesher, the replication, the session, the view, the horizon and the core. The first tests missed about twenty
  (the ends of the cubes against the extent; a detailed mesher that is always settled; the fog; the viewport; detail asked for late; the distance and light of nodes; the version of the terrain; a view of another grid; the margins and the touching distance of the horizon; a sphere wider than the cone; the cube inside the core; a core with no size), all got a test but these. **Equivalent, for what a picture shows:** the second check of the extent in the
  mesher (`may_show` and `coarse_generator` both ask; either is enough for the picture and `may_show` saves the work), a planet without a generator saying `may_show` or not (it draws nothing either way), the grid a detailed view is given while it has no corrections to match, the guards at the head of `is_hidden_by` that the comparisons after them make unnecessary (a viewer in the core: the cone's sine is over one and
  the cosines are not numbers), `||` for `&&` in the selection (a cube that may not show is chosen and drawn empty), and the smallest face for the greatest in the core (the faces have the same hills). **Not pinned:** that the margin is exact (the sphere's growth by it, its shrinking, the nearest point of the cube to the touching places): three mutants of the boundary of the cull, within a viewer's margin of the edge of the shadow, where the core
  is hundreds of voxels under the ground; and the square root of three in the core, which the tests' places do not reach and the proof does.
- Windows 2056 and Linux 2057 tests pass (the one more on Linux is the test of a file's permissions); fmt and clippy clean.

Phase 39d (planets as ships, step 5: walkers on a planet, the gravity of a level, a planet's mass; 2026-10-24). See `planet-spec.md`.
- *The server:* `ServerWorld::set_grid_terrain` (terrain, the declared mass, the centre of mass at the origin, held), `set_level_gravity` and a level's pull in `gravity_at`; `moving_voxel`, `moving_uniform_value` public and `&self` (the generated-brick cache and its counter behind a `RefCell` and a `Cell`, so a body can meet several ships while the world is
  borrowed shared); `ServerConfig.level_gravity`, applied by `Server::new` (a level the world lacks is ignored). *The game shell:* `GameDefinition::{gravity, level_gravity}` (defaults: the old gravity, none), wired in `host::server_config`. *Bodies:* `grid_bodies` leaves a ship with terrain out of its bodies, weighing and splitting; `collision::{GeneratedHull, ShipSource, SolidAt}`;
  `collision_of_voxel` and `hint_of_brick` shared by `WorldTerrain` and `GeneratedHull` (the hint stopped asking about orientations: a brick of one block in one state and several orientations is known at once, which only saves a scan). *Cave:* `Blocks::source`, the aboard rules take `impl SolidAt`, `Cave::{gravity, level_gravity}`
  (`PULLING_MASS = 10^19`), `mini_cosmos_id()`.
- *Content:* `base_blocks:small_planet_v1` (the planet of `terrain::planet`, one hundredth of the Earth's radius) is registered after the rolling hills, so the hills keep number 0 in every world made before it; a test pins its three bricks through this plugin's blocks and the order. **Cave's golden session moved once:** the welcome and the saved world list the generators, so the stream grew by 28 bytes (7,190,133 to 7,190,161), which
  is the identifier and nothing else (measured by the length, not frame by frame), and the hashes of the stream and of the saved world changed; Windows and Linux agree on the new pins.
- *Found on the way:* (1) the first run of the walker test: the player never fell. The plugin that gives ships their bodies weighs every ship each tick from the blocks it stores and wrote 0 over the planet's declared mass (found by capturing the ship's mass inside a tick: 0.0 although `set_grid_terrain` had set it). Planets are now left out of that plugin. (2) The same plugin would have taken the planet away when the last block of its
  stored bricks was air (a block put in the sky and taken away again) and cut it into ships when a trench divided a stored brick; the first dig test reached neither (stone is left around a hole), a mutant that took the guard out survived, and two tests that do reach them were added. (3) My mistakes in tests: `yaw` is in degrees (I passed pi and walked 3 degrees off -z); the first line the walker took met a step of one voxel and
  a sinkhole (caves reach the surface 10 voxels deep at three places in 60 voxels), so the test now asserts that the slope it walks is a gentle one; the trench was first cut in the brick above the ground (the column at the origin tops out on the first voxel of the next brick); a test of a hole as deep as a brick, to land on a brick that is one block, cannot be written (all 300 bricks probed, from the surface down, have a cave in them). (4) A comment in
  `game.rs` said a ship of a million tonnes pulls a thousandth of a metre a second a second at a metre; it is 0.02 (a thousand tonnes pull 0.00002, which the comment now says). The constant stands.
- *Measured (release, this machine):* joining and settling next to the planet took 480 ms (five runs, 479 to 486) against 20 ms with no planet (three runs, 19 to 20), about 50 bricks generated at 9 ms; then a tick is 0.05 ms at the median and 0.1 ms at p99, and crossing into a new row of bricks is one tick of 37 to 44 ms (once in 400 ticks of walking). Not spread over ticks. A ship let go over the planet falls through it (4 x 4 x 4 of stone, 40 voxels over the ground, came to
  rest on the overworld's hills 320 m lower): rigid bodies and planets do not meet yet.
- Tests: Cave `tests/planet.rs` 8 (a walker dropped over a planet lands on it, is aboard it in its coordinates, and its mass, centre of mass and hold are still what they were; the pull toward the centre, 10 to within a tenth; walking on the generated ground; a dug hole is fallen into and stored; a slab put on it upside down is a ceiling; a trench across a stored brick and a block put in the sky and taken away
  do not take the planet apart or away; what the game declares), `grid_bodies` 2 (the voxel and the brick helpers, with a stone of four shapes, a mist and an air), `server` 1 (the config's pulls, in two levels, a level the world lacks, the main level's, and the other way round) and the planet's fields asserted in the gravity test, `game_shell` 1 (the host configures the server with the game's gravity).
  **Mutation experiments:** about 50 mutants of `plugin.rs`, `collision.rs`, `movement.rs`, `game.rs`, `server_world.rs`, `server.rs` and `host.rs`. The first tests missed: the guard in the splitting of ships (see 2), the hints of a brick (7 of 8 in a copy of the logic that the planet tests never reach; the logic became one pair of functions with unit tests instead), the terrain and the centre of mass that `set_grid_terrain` sets (asserted only from Cave's
  tests, now in the server's), and the level a configured pull is given to (the test had one level). One stays: using the stored-voxel source for every ship (it gives the same answers for a ship without terrain; it is a speed-up), and the orientation of a brick hint was removed as unobservable.
- Windows 2025 and Linux 2026 tests pass (the one more on Linux is the test of a file's permissions); fmt and clippy clean (the lints caught two `powi` and two assertions on constants in the new tests).

Phase 39c (planets as ships, step 4: the wire and the client; 2026-10-24). See `planet-spec.md`, `protocol-and-replay.md`.
- *The wire:* protocol 8. `GridState.terrain: Option<GridTerrainState>`; the server's `ship_views` tells of a planet with its generator and extent and sends only its stored bricks. *The client:* `ClientGrid.terrain` (`ClientGridTerrain`), the generator translated like an assignment's,
  `GridSummary.terrain` (generator name and extent) so that a replay is compared on it. *The view:* `world_view::TerrainSource` (implemented by `LevelData` and `ShipTerrainSource`) which `WorldMesher::update` and `settle` read from, and `ShipTerrainMesher`; in `game_shell::Session` one per planet,
  the camera taken into the planet's coordinates, the changes of its stored bricks routed to it, the crosshair and the block under it read through it, `settle` waiting for it.
- *Golden sessions:* the three games' and the end-to-end recording moved once, for the version (stream hash only; the length and the saved world did not); each was checked against the code of the commit before with only the version raised to 8 (same hash), so none of them has a ship in it.
- *Found on the way:* the first test of the session sees a stored brick of a planet meshed alone (a face against the ground beside it that is not there) for one frame, because the event of its arrival comes before the mesher of the planet exists and the mesher of ships took it; the mesher of ships now ignores the ships with terrain, and the test looks at every picture ever put on the screen,
  not only the last. A test of the expectation "29 bricks on top" had forgotten that a stored brick with no ground in it uncovers the top of the stone under it.
- Tests: protocol 1 (and the sample messages of the round trip now have terrain: a bad flag, a generator beyond 32 bits and corners the wrong way round are refused), client 2 unit tests and 1 stream test and a case (the generator in this client's numbering, one the description lacks, the summary), server 1 rewritten (a planet is told of with a generator whose number is 1, and sent only its stored brick), `world_view` 6 (a planet meshed exactly as the terrain of a level with the same ground, a stored brick wins and a change redraws,
  nothing beyond the extent but its sides, no generator draws nothing, a light hook changes the pictures, reset), `game_shell` 5 (the bricks around the camera, the crosshair and the block it aims at, the server's edit, removal, a recording), end-to-end 1 (a replay of a session with a planet rebuilds every view, and the live and replayed views are the server's truth, edits and removal). **Mutation experiments:** 26 mutants; the first tests missed 7 (a flag that is neither 0 nor 1 followed by valid terrain bytes, which read as an error for the wrong reason; the filter that keeps the mesher of ships from meshing a planet's stored
  bricks alone, which needs a brick stored from the start; the mesher of a planet kept when the planet goes; the block under the crosshair; a generator number that was 0 by accident; the light of a planet's mesher; and the planets' meshers being told when the view's channels change), six got a test, and the last stays: no test changes the view's channels in a session.
- Windows 2010 and Linux 2011 tests pass (the one more on Linux is the test of a file's permissions); the repinned golden sessions agree on both; fmt and clippy clean.

Phase 39b (planets as ships, step 3: the server reads a ship through its generator; 2026-10-24). See `planet-spec.md`.
- `ServerWorld::{moving_voxel (now `&mut self`), moving_generated_voxel, moving_uniform_value, set_moving_voxel}` and `GridView` for a ship read the stored brick, else the generator's brick in the extent, else the default; edits store the generated brick with the voxel
  changed. `GeneratedBrickCache` is keyed by `GridId` (a level's root grid or a ship). A planet is not in `ship_views` until the wire can carry its terrain (39c).
- Tests: 7 in `server_world` (a planet reads the level's ground inside its extent and the default outside; each brick made once, none outside the extent, a level and a planet not sharing what they remember; an edit stored over the generated brick, wins, and leaves the rest as generated;
  uniform values from the stored or the generated brick; a missing generator named in the error and nothing stored; a ship nothing generates unchanged; a planet not replicated), 1 for the cache keyed by grid (it replaces the one keyed by level). **Mutation experiments:** 16 mutants, all caught the first time.
- One GPU screen test of Cave (`a_replay_draws_the_form_exactly_as_it_was_drawn_live...`, adapter 3) differed by one pixel once in a full run and passed on its own straight away, and in the next full run. Nothing in 39a or 39b draws.
- Windows 1994 and Linux 1995 tests pass (the one more on Linux is a test of a file's permissions, which only Unix has); the planet's file hash agrees on both; fmt and clippy clean.

Phase 39a (planets as ships, step 2: a ship with terrain, and its file; 2026-10-24). See `planet-spec.md`.
- `world_storage::GridTerrain { generator, extent, mass }` and `MovingGrid::terrain: Option<GridTerrain>`; `brick_extent` and `world_brick_bounds` take in the extent. The ship file's optional section `engine:grid_terrain` (see `world-file-format.md`); `ShipFormat` takes
  the world's table of generators (`WorldData::ship_format` supplies it), writes the generator by name and reads it back through the reading world's numbers (`resolve` is asked for `engine:terrain_generator`, as for content); `ParsedShip::referenced_content` lists the generator;
  `ShipFileError::UnknownGenerator` for a number the table lacks. Every place that built a `MovingGrid` has `terrain: None` (8 sites, found by the compiler and a script). A ship that nothing generates writes the file it wrote before: the existing pinned ship file and world file did not move.
- Tests: 9 in `tests/ship_terrain.rs` (round trip and the file the same every time, pinned by hash, 17,126 bytes; no section for a ship that nothing generates; the generator by name through another world's numbers; refusal naming the generator; a number the table lacks; seven kinds of
  damage to the section; parked and brought back; saved with a world and loaded; a generator of a missing plugin kept through two saves) and 1 for the extent. **Mutation experiments:** 18 mutants of the codec, the format, the loader and the extent, all caught the first time.

Phase 39-1 (planets as ships, step 1: the planet's ground, bricks and coarse nodes; 2026-10-24). See `planet-spec.md`.
- `terrain::{PlanetSettings, PlanetGround, PlanetGenerator, planet}` (`planet.rs`, `planet_node.rs`): a cube-sphere of six heightmaps blended with weights whose squares add to one; bounds (Cauchy-Schwarz over the faces that can count); bricks with the layers read radially and the
  overworld's sheet caves; `may_show` from the ground; coarse nodes made by walking columns along the axis the node is nearest to (asking about every cell for nodes that reach the plane through the centre). `coarse.rs` gained `coarse_node_from_fills` (the layering of a node, with the step
  "up" and the source of the fills as parameters; its 23 tests and its pinned nodes did not change), `FractalNoise::largest_magnitude`, and test helpers (`node_digest`, `fills_of_field_cell_by_cell`).
- *Found on the way:* the first walk of a column was a fixed point along the axis; a search from outside found the surface 6 voxels from where it ended in a column at 40 degrees to a steep surface. The walk asks about each cell and does not need the ground to be a graph over the axis.
  A truth lattice with its samples at the low corner of each cell instead of the middle made the nodes look 10% wrong in two measurements; the middle gives 3% to 6%.
- *Measured (release, one thread):* a surface node of the real planet 0.5 to 1.0 ms (levels 1 to 10), of which the fills are 0.4 to 0.5 ms (the lattice of `coarse.rs` takes 20 to 29 ms for the fills); a surface brick 6 ms. Cells with a surface in them drawn wrongly against the voxels under them: 17% at level 1,
  3% to 6% from level 2. The walk differs from asking about every cell of every column in 0.0% to 0.3% of the cells with a surface in them on the real planet, 1.6% on a small planet whose hills are twice as steep per octave.
- *Tests:* 35 more (1,942 to 1,977 on Windows): 23 in `planet`, 11 in `planet_node`, one for `largest_magnitude` (symmetry of a ball under seven turns and mirrors of the cube, to the cell, and the eight nodes at its centre; the layers at every pole; the walk against every cell; the cells against the voxels; the field, a brick and several nodes pinned by bits and digests, Windows and Linux).
  **Mutation experiments:** 44 mutants of the two files; the first tests missed 10: the combination of faces' bounds in a blend zone checked only through boxes big enough to hide it (tiny boxes at the seams and the corners of boxes as probes now), the bound through finer detail
  checked only with slack, bricks whose far corner is just under the surface, the part of the heightmap a box is read from (now tested directly), a sealed node that forgot the reach of its layers, the margin of a column, and the first guess of a walk. Three are equivalent and stay: the one voxel of padding of a heightmap part (it
  guards the rounding of a polynomial), the margin in the test for reaching the plane through the centre (it changes only cells beyond the limb of the nodes at the centre), and a `.max(reach)` in the bounds that was removed because nothing could see it.
- Windows 1977 and Linux 1977 tests pass (the pinned bits and digests agree on both); fmt and clippy clean.

Phase 38 (levels of detail: experiments; 2026-10-24). See `lod-spike.md` and `spikes/lod/`.
- *Columns:* a node of hills made from its height columns (a batch of the same noise, bit for bit; the mesh made straight from the heights) takes 25 microseconds against 14 to 38 milliseconds, and a whole view out to 100 million voxels is made from nothing in 0.1 second on 12 threads.
- *Occlusion:* from a valley or an ordinary place 97 to 99% of the nodes of the rolling hills are hidden behind nearer ground, and a horizon swept over the heights of the columns as the nodes are made (13 to 30 ms beyond the heights) finds them with none wrong
  (rays through the columns as drawn); a horizon made of the generator's bounds is sound but gets two thirds from a valley and nothing from an ordinary place. The sweep's first version had a flaw (occluders in bins they only partly cover) that a stricter oracle found.
- *Drawing:* one indirect call over one arena is 8 to 23 times less CPU than a draw for each node on the RTX 3090 (0.5 ms against 11.6 for 25,500 nodes); the integrated GPU is limited by quads (5 ns each).
- *Not built:* a ray-marched far field, a cubemap of the far field, column meshes read from a height buffer (reasons in the report).
- Nothing in the engine changed; the experiments are a standalone project like the other spikes.

Phase 38 (levels of detail: tight bounds and pruning; 2026-10-24). See `lod-spec.md`, 9b.
- *Tight bounds* (`terrain::noise`): the bound of an octave over a region is the narrower of the range of the lattice values and the value at the middle plus and minus the steepest slope of the blend times the reach; identical output, a node of hills 9 times
  faster to make (9a). `bounds_through_finer` on `FractalNoise` and `DensityField` (all the finer levels at once), `Interval::hull`.
- *Pruning:* `TerrainGenerator::may_show` / `WorldTerrain::may_show` (air all through, or ground all through with ground one cube's edge all round), `WorldTerrain::version`, `RegionOctree::all_in_cube` (which went from 1.1 microseconds a question to 0.25 when it
  stopped looking at all eight children), `coarse_cells::EditedCubes`, `select_around_brick(.., may_hold)`. The client keeps the cubes with or beside an override and the cubes that have a brick of the fine region in them, remembers what the terrain said for two workings
  out (`MayShowMemo`), and works the cubes out again, at most every 10 updates, when an edit comes in a new place. The server follows the selection only to the cubes with an override (it found that the terrain's say changed nothing it sent), and no longer needs a throttle.
- *Found by a test:* the first version left out the air cubes that hold bricks of the fine region, so a block placed three bricks up was not drawn (`a_block_placed_high_in_the_air_among_the_bricks_is_drawn...`).
- *Tests:* 35 more than before (1,907 to 1,942 on Windows), among them a dense scan of every small region across lattice borders and a sweep of the soundness of `may_show` against real bricks. **Mutation experiments:** about 110 mutants; the first tests missed 30, among them the slope constant a little too small, a bound made from the first cell only, caves bounds that forgot the octave a footprint drops, the room round a cube on its
  low sides, a cube judged at level 0, the bookkeeping of edits and the rate limit of the client; each got a test, and the server's code that was left with nothing to do was removed. (A mutant of `cube_may_show` that cannot be told from the code except in a case of
  exact zero makes a test run for ever; it is equivalent.)
- *Measured:* a sixth to a quarter of the cubes to a kilometre without caves, half with them (table in 9b); no cube that was left out had anything in it (flat ground, hills, caves; 0 of about 150,000 checked); the pruned walk to 1,500 voxels is 0.4 ms (flat), 4 ms (hills),
  10 ms (caves) and to 100 million voxels 2.7, 13 and 41 ms (release, one thread), before the memo.
- Windows 1942 and Linux 1943 tests pass; fmt and clippy clean.

Phase 38i (levels of detail, step 9: coarse nodes in the stream, and bricks and nodes drawn together; 2026-10-23). See `lod-spec.md`, 3.5, 3.6a and section 9.
- *Messages and the server* (`coarse_streaming`): `SetDetail` and `CoarseNodes` as plugin messages (no protocol bump), `CoarseServerPlugin` keeping the overrides of every edited cube (levels 1 to 12) with the exact cells of edited bricks
  patched into the generator's node, remade finest first a few a tick, sent coarsest first within a rate in bytes. Pure pieces tested on their own: the order and the removals (`pending_updates`), the credit (`refilled`), the messages in groups of 256, the marking of a brick and its neighbours.
- *Selection:* `FineRegion` (a ball of bricks and the cubes of two that touch it), `ViewSelection`/`select_around_brick` (one function for both ends, the viewer at the middle of its brick), a floor of detail level 1 outside the region.
  `plan_swaps` (the groups in which what is drawn is exchanged for what is wanted). `CoarseNode::patched_with_child`, `without_exposure`, `WorldTerrain::coarse_generator`.
- *The client:* `WorldMesher::with_held_back_display` (`is_ready`, `show`, `hide`), `LodView` (nodes made on threads, replaced where they are when the server says so), `MeshUpdate::UploadCoarse`/`RemoveCoarse`, the session's `enable_coarse_detail`,
  the request it sends from the camera and the picture's height, fog for the distance drawn. Two shortcuts that came from measuring (a uniform node takes 3 ms to make and mesh; see `lod-spec.md` section 9).
- *Tests:* server plugin 15 integration and 13 unit with the messages' 9; the region, the selection and the swap planner (with a property test on tilings with different ready sets); the held-back mesher 10; `LodView` 9 (tiling, walking 8 bricks never leaves a hole or draws twice, an override replaces a picture in place and taking it back
  restores it, a stale picture is never put up); in the shell, an edit far away appears for a watcher and for a player who arrives later, and a recording shows what the live watcher saw.
  **Mutation experiments** on each piece: the server plugin 32 mutants, 29 caught at first (the three left were an equivalent filter, a first-tick burst the budget test did not see and a fine radius not tested at a distance where it matters: tests added, all caught);
  the region, the selection and the swap planner 17 (one equivalent); the held-back mesher 14 (one equivalent, one hangs rather than fails: a keep radius too small makes it reload what it unloads); the sealed-ground shortcut 5 and the uniform mesh 5, all caught
  after a slab of ground in the air and a node beside a bubble were added; `LodView` 17, of which 13 caught, 1 equivalent (a hint), and 3 not tested (the order nodes are made in, a ready node made again, the accounting after a reset).
- *Found:* a mixed node of Cave's terrain takes the generators 375 ms, so Cave does not use any of this yet (section 9); node counts are tens of thousands for a view of a kilometre; the exposure of a cell changes where nothing else does, so a node's version counts only what is sent.
- Windows 1907 and Linux 1908 tests pass; fmt and clippy clean.

Phase 38h (levels of detail, step 8: drawing coarse nodes; 2026-10-23). See `lod-spec.md`, 3.6.
- *The renderer:* a `Placement` has a scale (the fourth float of its offset, which was padding), and the shader multiplies the mesh by it before the turn, so a coarse node's mesh (in cells) is drawn at `2^level` voxels a cell.
  `upload_coarse_node(cube, mesh)` / `remove_coarse_node` / `has_coarse_node` / `coarse_node_count` for the terrain, and `upload_grid_coarse_node` / `remove_grid_coarse_node` / `grid_coarse_node_count` for a ship (in its own coordinates, placed
  and turned by its pose, dropped with it by `remove_grid`). Each node is culled by its own sphere (`node_sphere`: that of a brick times the scale), and counted in `RenderStatistics` (`coarse_nodes_drawn`; `bricks_drawn` and `bricks_culled` include nodes).
  The renderer holds and draws what it is given and does not choose: a node and the bricks it stands for, both held, are both drawn; whoever selects the nodes (38i) removes what a node replaces. The mesh key of a node is its cube (and its ship), which has the level in it.
- *Tests (GPU, on all four adapters):* a coarse node draws **the same picture as the bricks it stands for**: exactly at levels 1 and 2, and at level 3 to within one pixel of 4,096 on the silhouette (the outline of one big quad and of many brick quads share a straight
  edge, but where bricks meet their vertices are snapped to the rasteriser's grid on their own, so a pixel whose middle is on the edge can fall the other way; a wrong scale or place would differ in hundreds). A turned ship with bricks near its origin and a coarse
  node beyond them is drawn as all bricks. A cell of 262 kilometres 1,000 kilometres off is drawn to its angular size (about 12 pixels of 64) and the far corner of a node 8 million voxels across is not culled. The bookkeeping (held, counted, culled, replaced,
  removed with the ship; a cube of level 0 is refused). **Six mutants** (scale not applied in the shader; the sphere's radius or its middle not scaled; a ship's node not turned; its scale 1; removing a ship keeping its nodes) were all caught, the sphere's middle by a
  unit test of `node_sphere` after the picture tests could not see it (culling is conservative, and the sphere of a huge node is so big that a misplaced middle still meets the view).
- Windows 1829 and Linux 1830 tests pass; fmt and clippy clean.

Phase 38g (levels of detail, step 7: meshing a coarse node; 2026-10-23). See `lod-spec.md`.
- `voxel_meshing::mesh_coarse_node(node, neighbours) -> BrickMesh`: a cube for each cell with fill of at least half; a face wherever the cell beside it (in the node, or in the node of the same detail level on that side if it is given; air if not) is not a cube; the greedy
  merge of the mesher (`merge_slice`, now shared) over equal block and light. The quad has the block as its material (the palette of the renderer begins with the block colours) and **the cell's own light on all of its sides**, so a cell with a lamp in it
  is as bright from every side, as the owner described. Quads are in cell units; the renderer scales them (38h). A seam against a node of another detail level is a crack (accepted); detailed (sub-voxel) quads are never made.
- Tests (10 + 1): air and less than half full have no mesh; the exact threshold between cells of 127 and 128; a solid node has six faces, five with a solid neighbour, none with six, six against air; flat ground is one rectangle on top; a hill topped with snow has
  snow on every top and far fewer quads than faces; a lamp cell is as bright on every side and unequal light does not merge (equal does); a property test that the area of the quads is exactly the number of uncovered faces counted the slow way (with and without a neighbour);
  and, from the real generator, a coarse node of the rolling hills meshes to 942 quads whose top area, 1,024, is all grass. **Seven mutants** (neighbour ignored, a missing neighbour taken as solid, light or block out of the merge key, drawn at any fill, faces made when covered,
  the all-solid shortcut) were all caught; one first survived because the threshold was only tested through the shortcut, and a test was added.
- Windows 1822 and Linux 1823 tests pass; fmt and clippy clean.

Phase 38f (levels of detail, step 6: which nodes to draw; 2026-10-23). See `lod-spec.md`, 3.3 and section 8.
- `coarse_cells::select_nodes(extent, &Selection)`: the octree walk, finer near and coarser far, for terrain and every ship alike (the viewer in the grid's own axes), with the zoom cone, the cap, a distance limit, sub-pixel culling and a
  `may_hold` predicate. Pure, IEEE arithmetic only, deterministic (a digest pinned). Tests (14): small grid near and far, tiling with no gap or overlap, every cube fine enough and its parent not (a property of the rule), a planet's counts and
  its finest cube under the viewer, the cap, the sub-pixel cut, the distance limit, `may_hold`, the zoom (only refines; unchanged outside the cone; finer on the axis; a coarser or equal zoom changes nothing), the exact boundary of "fine enough",
  determinism, a turned and moved ship, the viewer inside a cube. Ten mutants, all caught (two needed new tests); one condition (the cap) was found dead by a mutant and removed.
- *Measured, and my estimate was wrong:* the node count is the same at every level, tens of thousands in all, not a few thousand (section 8): 25,957 for a sphere of 10^8 voxels, 23,806 for a world with a surface, 57,427 with a 10 times zoom.
- Windows 1814 and Linux 1815 tests pass; fmt and clippy clean.

Phase 38c to 38e (levels of detail, steps 3 to 5: coarse cells, summaries of bricks, coarse terrain; 2026-10-23). See `lod-spec.md`, sections 3.2, 3.4 and 7.
- *38c, `coarse_cells`:* `CoarseCell` (block, fill, light, exposure), `combine` (eight cells make one), `CoarseNode` (32 x 32 x 32, kept as one cell when they are all the same). Tests (21): a layer of snow is snow, a cave wall does not outvote the ground when
  it has less surface, blocks are counted together across children, ties by octant whatever the block numbers, a speck is not lost, light is the brightest up to 32 voxels and an average above (and falls back to fill), exposure rounds and saturates,
  the cells of a node are combined from the right child and the right octant; two property tests. **Mutation experiments:** eight mutants (volume instead of surface, children not counted together, light always the brightest, the average from level 0,
  fill rounded down, ties to the highest octant, exposure truncated, exposure not divided) were all caught. *I refined the rule of the spec:* the block is the one whose children together have the most surface, not the child with the most.
- *38d, `voxel_meshing::coarse_node_of_brick`:* the node of a brick from the same inputs the mesher takes (the brick and the six beside it). Fill from the shape (a slab 128), exposure from the faces the mesher would draw (partial faces and the faces inside a
  slab counted), light of the air a filled voxel looks out at; the two commonest bricks (sky; buried ground) without looking at a voxel. Eight tests, including a property test that the cells made of a brick keep its surface and fill to within rounding.
  **Seven mutants** (a missing neighbour hides everything, the inside of a voxel not counted, light always the voxel's own, fill rounded down, double exposure, nothing hidden, the neighbour of a brick is itself) were all caught.
- *38e, `terrain::TerrainGenerator::coarse_node` and `FieldTerrainGenerator`:* from the density field at the footprint of the cells: the bounds say air or ground for a block of cells at a time, a lattice of 4 x 4 x 4 samples says how full the rest are, the
  surface and the layers (grass, dirt, stone) come from the fills of the cells around. The default is `None` (see 3.4). Tests: fourteen, including the shape of a slope and a cliff, a pocket of air in the ground, a steep slope, the coarsest level (40), and a pinned
  digest of two nodes of the rolling hills (to be the same on Windows and Linux). **Ten mutants** were run; two survived the first tests (a cell with air in it having a surface of its own, and the top of a slanting surface being grass), and
  two tests were written for them, which catch both. *Two estimators were tried and dropped:* the column of two samples (a slanting surface was called solid) and the middle sample with the range of the bounds (the bounds on noise are too wide:
  twice as many cells, fill off by 39%); the lattice of samples is what stayed.
- *Measured:* section 7 of the spec (agreement with the bricks; the cave finding, which needs the owner; the `ShellCaves` bug, fixed, with a regression test that fails without the fix).
- Windows 1800 and Linux 1801 tests pass; fmt and clippy clean.

Phase 38b (levels of detail, step 2: reverse-Z with an infinite far plane; 2026-10-23). See `lod-spec.md`.
- *What:* `Camera::view_projection` is `perspective_infinite_reverse` (depth is `near / distance`: 1 at the near plane, toward 0 for ever), the depth buffer is cleared to 0 and the test is "greater". `Camera::far_plane` is gone (nothing clips for being far;
  it also held `game_shell::camera_at`'s view radius, which nothing else used, so that parameter is gone and its fifty-odd callers lost an argument). `Frustum` has five planes (there is no far side), and the extraction was written for reversed depth.
- *Tests first:* the depth of a point ahead is `near / d` for distances from 1 to 10^12 and is never clipped for being far; behind and too near are out; the frustum keeps a sphere 10^12 away and drops one behind or beside (CPU); and a GPU test of two walls (red
  nearer, green behind) a long way off. It failed before the change (the far plane cut the walls off), and with the depth test set to "always" it shows green: it does test the order.
- *Measured* (one adapter, an RTX 3090; the test passes on all four): walls 2 metres apart are in order at 100 kilometres and at 10,000 kilometres, in both orders of drawing; at 100,000 kilometres, 2 and 4 metres were wrong in one order and 8 was right. (My first run of this
  at 100,000 kilometres looked right for 2 metres, and was wrong to trust: both walls had the same depth there, because a position in `f32` has steps of 8 metres, and the wall drawn first won the tie. It was seen only by looking from the other side, so the test runs both. My calculation had said
  6 metres at 100,000 kilometres; the measurement agrees with it.)
- Windows 1756 and Linux 1757 tests pass; fmt and clippy clean.

Phase 38a (levels of detail, step 1: orientations in `f64`; 2026-10-23). See `lod-spec.md`.
- *Why first:* `Orientation` was `[f32; 4]` and is used in 62 files; a planet turned by an `f32` quaternion has points metres out (measured: over 199 turns of a point 5 x 10^7 voxels from the origin, the worst miss of an `f32` turn is 3.78
  metres, 0.05 for one turn). It is the riskiest step of Phase 38, so it was done alone, and its pins checked before anything else was built on it.
- *The type:* `Orientation([f64; 4])`; `components()` is `f64`; `from_single_components` and `components_single` are the edges to the physics engine and the graphics card (which stay `f32`; rounding is the only loss);
  `from_yaw_pitch` takes `f32` angles and works in `f64` with `libm`; `compose` normalises in `f64` and keeps the `f64`; `LatticeRotation::orientation` and `nearest` are `f64` (exact for the lattice turns); `AlignmentTolerance`'s closeness is `f64`.
- *The wire and the files:* protocol 7 (a pose has four `f64`); the sections that hold poses (`engine:entities`, `engine:grids`, a ship file's `engine:grid_flight`) are written as version 2 and version 1 (four `f32`) is still read, as exactly the same
  turn (`PoseFormat`, `engine_sections::pose_format_of`, `reads_version`); a version it does not know is refused. Tests: `older_poses.rs` (entities, ships, a world file whose ships section is rewritten as version 1, a ship file whose flight is
  rewritten as version 1, the versions read and refused) and the wire test of version 1 bytes (read as the same turn; not mistaken for the new layout).
- *Held ships keep an exact turn:* the bodies plugin does not write a held ship's pose back from the solver, so a planet that is held keeps its `f64` turn to the bit (test `a_held_ship_keeps_a_turn_that_is_not_an_f32_...`; mutation: writing
  held ships back from the solver makes it fail). A ship the solver moves has the solver's `f32` turn (the test says so); that is a limit of the physics engine, not of the type, and it matters only if a planet is ever moved by the solver.
- *Pins moved, and why (checked by a control, not assumed):* the physics pins (bodies, foreign), `scripted_run` aboard, the scripted flight and both ship-physics hashes all moved, because the starting turns are now worked out in `f64` and rounded once
  to `f32` for the solver, which is not always the `f32` the old arithmetic gave, to the last bit. **Control:** with `from_yaw_pitch` put back to `f32` arithmetic and nothing else changed, all of them came back to their old values, so that is the only thing that moved them
  (storing and composing in `f64` moved nothing). The pose of a grid at an angle (`conversions_are_the_same_bit_for_bit...`) moved for the same reason; the new bits were reproduced by a separate calculation in Python (the matrix of the `f64` turn, then the
  rounding that `displaced` does), and the old value was ten `f32` steps from the exact answer. The golden sessions of the three games moved by whole poses: **measured** by recording each script before and after and comparing frames: the same number of
  frames, and every frame that differs is longer by a multiple of 16 bytes (2048: one pose; eels: 531 poses and 3 bytes of frame lengths; Cave: 169 poses); the saved worlds changed (poses are longer, the sections are version 2). The recording of the end-to-end
  test moved only for the protocol version number in the welcome (control: version 6 gave the old checksum). The ship file and the world file with ships moved by 16 bytes a pose (the world file's length at the old commit was measured: 34,105 to 34,137).
- *Found on the way, not fixed:* `GridPose::local_to_world` rounds an offset to `f32` before it carries into the next brick, so a position can be a few `f32` steps (4 x 10^-6 voxel at most) from the exact one. Harmless at this size; the carry can be done in `f64`
  if it ever matters.
- Windows 1753 and Linux 1754 tests pass; fmt and clippy clean.

Phase 32m part 4 (the owner's answer about orientation; 2026-10-22): one orientation channel for every block, pistons and logs placed as in Minecraft, and an empty ship vanishes.
- *The owner's instructions:* an empty ship vanishes; a block's orientation when placed should depend on the player's; copy Minecraft's pistons for blocks with one front face and logs for blocks with a line of radial
  symmetry; orientation saved the same way for all blocks regardless of how many visually distinct orientations they have, so that they can be rotated by simple algorithms and blocks of unloaded plugins can be rotated too.
- *I replaced my own part 3 answer.* Part 3 closed a block's states under the turns (19 states for a stone). That could not do what is asked here (a log's state would be "which way it stands" and a block of an unloaded plugin would
  not turn), so it is gone: `in_every_orientation` and `describe_shape` are removed and the standard blocks have their seven states again.
- *The channel:* `base_blocks:block_orientation` holds the index in `LatticeRotation::all()` (new `COUNT`, `index`, `from_index`; the order is pinned by a test as a file format). `BlockTable` is indexed by block, state and
  orientation; shapes are de-duplicated (tests: a cube is one shape in 24 orientations, a slab six, a stair twelve). The mesher, world view, picking, hull, terrain colliders, weigher, arrival and docking take the channel;
  a merge turns each voxel's orientation (tests: whatever the block, the turn is the voxel's own followed by the transform; a ship not on the lattice keeps them; a number that is no turn is left).
- *Placing:* `placement.rs` (tests first: the four ground directions, a tilted look, up and down at the thresholds, far away, the axis for all six faces, a block with neither), `SetBlock.against` (round trip for all faces and none, a
  face number past the sixth is an error; the two pinned messages grew by one byte each, `00`/`06`), the client sends the clicked face (`place_block_request`), the server works out the eyes in the grid's own axes
  (`PlacerView::in_frame`) and writes the channel; breaking writes 0. Wood is a log (`engine:mark_both_ends`, a wood-end colour); the dev docking port has one state and a front, so it faces the player when placed
  (checked through the real server: placed from three voxels over, it faces up).
- *An empty ship vanishes* (`split_ships`): a ship that blocks were taken out of and that has none left is removed (`remove_grid`). Mutation: with the removal disabled the test
  `a_ship_with_every_block_taken_out_is_gone_and_a_ship_with_one_left_is_not` fails (caught). A ship never given any block is not removed (only a change removes it).
- *Pins:* 2048 and eels moved by 467 bytes (the welcome: 466 for the three declarations, measured alone, and 1 byte for "no orientation channel", read in the code), saved worlds the same. Cave moved by 619 bytes, measured by recording
  the script before and after and comparing the 6,934 frames one by one: the welcome +74, 109 whole-brick frames +5 each (the channel), 5,747 brick-change frames the same length with another digest (the digest covers the channels);
  74 + 545 = 619. Its saved world changed (a channel more). The wire pins changed only by the byte at the end.
- Windows 1745 and Linux 1746 tests pass; fmt and clippy clean.

Phase 32m part 3 (the owner's answers to the docking report; 2026-10-22): the lever back, every block in every orientation, metric gravity, and the invariance tests.
- *The owner's instructions:* (1) put the lever back; (2) chance keyed by grid is fine; (4) every block must be able to exist in every orientation; gravity should be metric, try 5 metres a second a second.
- *The lever:* the pull is at the port again, as a force on the centre of mass and the torque `arm x force` worked out from the plugin's own centre of mass (`torque_of_force`, tested), not Rapier's (the cause of the first spin). The
  fire test of docking had walls whose ports were at a corner; it now has ports in the middle of the walls, which is how a ship is built that is meant to dock (it passed every run I made of it, three in a row, and in the full suites below).
- *Every block in every orientation:* `BlockDefinition::in_every_orientation` (tests: the number of states of a whole cube, a slab and a stair are 1, 6 and 12; the first seven states of the standard blocks are the seven they were; closed under every
  turn; what a state sets goes with its turns and states that differ in it are not merged; the faces of a port turn; a block with numbered states is refused), applied to stone, dirt, grass, wood, the lamp and the dev docking port. A stair has 12
  orientations and not 24 (checked by the test: its L profile has a half turn symmetry about its diagonal). `base_blocks::describe_shape` names the states. The docking roll test now says, for every roll including the eighths, that the
  slab in the whole is the slab of the second ship turned by the transform (it used to say so for two rolls and shrug at the rest). Mutation experiments on the closure: ignoring the other properties, not turning the shape, not turning the
  faces, not carrying the faces: all four caught. Cave's golden session moved by the package, 2,443 to 2,971 bytes, exactly the 528 the stream grew by.
- *Gravity 5:* the three defaults (`GravitySettings`, `BodySettings`, `CharacterSettings`) and the jump (3.6, the same 1.3 metres). The tests: most of the physics and ship tests were about falling fast and stopping on thin ground, and
  say so now by using an explicit planet of 32 (`heavy_planet`), and the two pinned hashes of the ships are *the same numbers they were* on it, which is the check that nothing else moved; the others were rewritten for slower falls
  (more ticks, a wider trench, the walker's deck kept still until they land) or re-pinned (walking, flights, the aboard run), with the reason in the comment beside the number. New: `ships_fall_at_the_pull_of_cave_five_metres_a_second_a_second`, and
  `a_jump_is_one_impulse_...` (the jump at 5: the speed goes down one step of gravity at a time all the way up, and the top is 1.3 metres). *Found by the tests on the way:* the turned-room tests compared a turned and an upright run at a moment in
  the middle of a jump; they now compare after it, with one jump and a walk, and a grounded character's vertical speed is 'nothing or one step of gravity' (a step of gravity at 5 is 0.08 and was folded away at 32). A tolerance in the character
  step (`1e-5` for 'stopped short') I loosened and put back: it was not the cause. A double jump from the ground-snap margin is refused (a character that is going up cannot jump).
- *Invariance:* `tests/ship_invariance.rs`: two ships merged have the light of one built whole (and differ before the merge), a ship cut in two has the light of two built apart. They hold, with the light (no chance); the merge test
  fails if the merged ship is not announced to the rules (`woken`). The first version of the split test put the lamp where it was joined to nothing, so the check gave it to the other half: a lamp must be on something.
- Windows 1745 and Linux 1746 tests pass; fmt and clippy clean.

Phase 32m part 2 (docking; 2026-10-22): the dev docking port, ships drawn together, turned into line, snapped and made one, and `/ship undock`. See `moving-grids-spec.md` 2.7 ("As built in 32m part 2").
- *The owner's instructions:* a creative-only dev docking port, gunmetal gray with one yellow face that docks with another's; snap to exact alignment in rotation on all axes within 10 degrees, any roll in
  quarter turns; snapping distance 0.3 voxels; the attraction also twists; forces do not scale with the masses of the ships (each port gets the same, so lighter ships accelerate more); a to-do for the
  conversation about ship mass from materials (`future-goals.md`, goal 9); a metric ton per cubic metre for everything that is not air for now; all measurements metric. 1 metric ton per cubic metre is what
  `engine:density` already defaulted to (1,000 kilograms a voxel, a voxel being a metre): nothing changed there, and the docs now say so.
- *Engine:* `Shape::turned` and `LatticeRotation::apply_face`; three engine block properties (`engine:marked_face`, `engine:mark_color`, `engine:docking_face`) and, in `BlockTable`, `material_of_face`,
  `marked_face_material`, `docking_face` and `turned_state`; the mesher draws each face with its own material (the greedy quads merge only faces of one material); `physics::BodyMotion::join`
  (momentum, angular momentum and inertia of bodies joined, in `f64`) and `BodyWorld::apply_torque`, `ServerWorld::apply_grid_torque`; `grid_bodies::docking` (the geometry, `Docking`, `undock`) and
  docking in `GridBodiesPlugin` (ports are kept for each brick of a ship as it is remade, with the mass); `MergePlan::turn` can now add a value to a voxel that had the default.
- *Cave:* the block (`dev_docking_port()` in `content.rs`, placeable by default and `voxunami_cave:creative_only`), `/ship undock`, the shape key goes through the states of the block chosen (a lamp
  has one: its HUD said "bottom slab" before, and asked the server for a state it does not have, which the server refused; it says "whole" now, and `the_shape_key_cycles_through_every_shape_...` was changed
  for that, on purpose), the pilot command follows a ship that was merged as `fly` does.
- *Wire:* the welcome's package carries three more property declarations: 432 bytes, measured by encoding the three alone, in every game's stream; no message changed, so the protocol version stays 6 and the
  three games' golden sessions were pinned again (the saved worlds did not move). Cave's again for the block itself.
- *Found by the tests, fixed, and each pinned by a test:* (a) a force put at a place on a body in the tick the body is made makes a turn that is not the force's: Rapier works out the lever from a centre of mass
  it updates when it steps, and a ship made this tick has the one it started with, the origin (a head-on docking of two ships in line came out spinning at 0.2 radians a second; the test is
  `ships_that_meet_head_on_dock_...`). The same is true of `apply_grid_force` with a place, which nothing but a test uses. I first mended that by adding the lever's torque from the centre of mass this plugin keeps;
  then the fire test, with walls of wood that put a ship's centre 2.5 m from its port, showed the lever itself is the trouble (25 kN m of swing against 10 kN m of twist: the ship drifted and did not dock), so
  **the pull is now put on the centre of mass** and the lever is gone; (b) the record of ships that were undocked and have not come apart forgot a pair in the tick after the split, because the two ships were not yet known to the
  bodies (they docked again at once); (c) `/ship pilot` named the old ship for a tick after a merge (the command runs before the flight step that follows the pilot); (d) my first tests set a ship going after the pull had begun,
  which threw away what the pull had given it (momentum seemed to be lost, by one tick of the force each time) -- the tests were wrong, not the code, and they say why.
- *Measured:* the equal and opposite pull gives 1,000 newton seconds to each ship in a tick (a twentieth of a second), whatever the mass (4 and 3 metric tons: speeds 0.25 and 0.333 m/s); ships 4 m apart meet at about
  9.6 m/s relatively with 20 kN, which is why the default is 10 kN (about 7 m/s, still a crash and a merge, and still within the 0.6 m window of a tick); the momentum of the merged ship equals the sum of
  the two to better than one in a hundred in the straight and in the off-centre, twisted approach.
- *The fire test* (fire crossing what was a boundary) was flaky until it was made a test of something: the fire is chance, keyed by the numbers of the ships and the tick a player's first message reaches the server, which a
  loaded machine moves (the parity tests say so), so 3 blocks and 6 fires were luck; it is a wall of 24 and nine fires lit after the ships have docked (lit before, they burn the first wall down before the second ship arrives), and
  it passed ten runs in a row. *Found on the way, not mended:* a ship that burns away in pieces leaves ships with no blocks in them (a part that was only flames is a ship; when the flames go out it is an empty ship that stays), so
  a burnt-out ship is not removed: noted in `plan.md`.
- *Mutation experiments:* by hand, in groups. The turn of states and the marks in the table (9 mutants), the mesher (3), `BodyMotion::join` (6) and the docking geometry (11) were all caught, but not all at first: in the table, ignoring the docking face or the marked face when turning a state, and preferring another state to the right one, survived until a block whose marked and docking faces differ and a block with twin states were added to the tests; in the geometry, each of the three facing conditions for pulling survived because another failed with it in every case I had, until a test made each the only one that fails. In the plugin, 10 of 11 were caught (a held ship moved, the whole going at no speed, the twist turned the wrong way, undocking across the ports, an undocked pair forgotten, a weld missed, the parts coming together, a released pair not skipped, a creative-only block leaving an item, and a new ship forgotten as released at once) and the backoff of refused tries was caught after its test was rewritten (its first version could not tell). **One is not caught:** that a ship that could not be docked is put back where it was: the bodies move it in the same tick, so its place does not show it, and I know it from reading the code and not from a test.
- Windows 1745 and Linux 1746 tests pass; fmt and clippy clean.
- *Known:* a slab or stair on a ship that docks rolled a quarter turn about a horizontal axis is not turned (the standard blocks have no state for it); a pair of ships can only dock by ports on the
  same level; both held ships never dock; a ship that could not be joined for overlap is retried only when one of the two has moved.

Phase 32m part 1 (merging; 2026-10-22): `ServerWorld::merge_grids`, the operation under docking. See `moving-grids-spec.md` 2.7 ("As built in 32m part 1").
- New file `engine/server/src/merge.rs` (`MergePlan`, `TurnValues`, `MergeError`), `voxel_space::LatticeTransform::apply_point` (a point, not a voxel, carried by a transform; a property test says it
  agrees with `apply` on voxel centres). The server grew no knowledge of docking, masses or block shapes: the plan carries the block channel, the tolerance, the velocity and the function that
  turns a block's values.
- Tests first (six): every block of both ships is where it was in the world (checked by world position of voxel centres, with the smaller turned a quarter and 6 voxels off), each old id a tombstone;
  what is aboard either ship keeps its place in the world and its turn; held if either was, the plan's velocity, and the rules told; the order of the two names does not matter; values are turned
  with a turned ship and not otherwise; every error says why and changes nothing. Mutation (by hand): the overlap check dropped, the turn function not called, held from the base only, the other's
  passengers not carried, their turn not composed, the voxels carried one voxel off, the plan's velocity ignored, the smaller kept as the base: all eight caught.
- Windows {WINDOWS} and Linux {LINUX} tests pass; fmt and clippy clean. (Clippy asked for the `turn` type to be named, hence `TurnValues`, a trait every matching closure implements.)

Planning note, 2026-10-21 (the owner asked for the two open ship items to be put in the plan; they became steps 32j and 32k, before splitting): the claim in the earlier
entries that ships "have no continuous collision detection against terrain" was a guess about the cause, and measuring it showed the cause is another. In the `physics` crate
alone (a throwaway test, removed), cubes of 1 and 4 and rafts of 10 by 10 were dropped at set speeds on a slab of terrain: with the 3 solver steps a tick has, a floor of 1
voxel is gone through from 96 voxels a second (12 to 72 are fine), floors of 2 and 3 from 200 (138 is fine), whatever the ship's size; with 6 steps 200 and 400; with 28 steps
nothing goes through at 400. `ccd_enabled(true)` and `soft_ccd_prediction(12.0)` on the ship bodies gave bit-identical results to doing nothing, and `is_ccd_active()` stayed
false at 13 voxels a tick: Rapier 0.36 does CCD of every fast dynamic body against fixed colliders by itself, but `shape_never_ccd_swept` leaves out `Voxels` shapes and a
body made of them keeps an infinite CCD thickness, and a ship's whole cubes are one `Voxels` collider. Also seen and not understood: in Cave a cube of 4 let go from 2000
above the flat floor (357 voxels a second at the floor) ends at rest 4 voxels above it, a cube of 1 one above it, and cubes of 2, 8 and 12 on it (Rapier pushes a deep overlap
out at 3 voxels a second at most, which makes it slow, not wrong); 32j's tests must explain it or remove it. The fix chosen is more solver steps for fast bodies (32j), and the
work budget of 32k also serves 32l's connectivity check.

Phase 32h, the same fix for walkers (2026-10-21; the owner asked): a walker hitched when it walked into new ground because `CharacterBody` made colliders for the 27 bricks
around the one it is in, 10 to 25 ms a brick on the tick's thread. It now has them for the terrain within 8 voxels of its centre and a quarter of a second of its speed, and
lets them go 16 voxels farther out (the same three numbers as a ship's, in `character.rs`). Measured in Cave with natural terrain, 1500 ticks of a player sprinting from the
start (release build, worst ticks of `ServerWorld::step`): 124 and 99 ms before, 0.1 ms after, and the walker ends at the same place to the last printed digit (it walks into a
wall of the terrain 24 voxels on, so two brick edges were crossed, not many). In the physics tests `Scene` counts the voxels it is asked about: standing in the middle of a brick
asks about fewer than 3 bricks' worth (974,145 voxels before), and sprinting across the edges of 3 bricks never asks for more than 3 bricks' worth in one step (324,676 before in
one). No pinned hash changed (scripted run, oriented and foreign runs, golden session, Cave aboard and flight), because the tests stand on bricks the new rule loads too. Six
mutants of the new code are caught (no distance ahead; a whole brick ahead; no speed term; let go at once; the frame origin forgotten; rounding toward zero), two of them only
after tests were added for them (a character falling at speed has the floor's colliders before it is close; walking away and back makes none twice). Windows 1525 and Linux 1526 tests pass; fmt and clippy are clean.

Phase 32h (Cave's ship commands and the first flight; the pause for the owner is after it):
- `/ship create [all] | push X Y Z | hold on|off | pilot` (`games/voxunami_cave/src/ships.rs` for the rules, `plugins/ships.rs` for the command and the
  system that flies). A ship is made of the blocks that someone built, joined through faces to the one the player looks at (up to 4096), by the engine's
  `detach_blocks`; `all` takes every solid block, which is nearly always too many (anything built on the ground is joined to the ground). Commands are
  about the ship the player is aboard, else the nearest within 48 voxels. A pilot's keys fly the ship by force each tick (held up against gravity,
  24 a second a second toward the keys, doubled by control, drag 1.0 a second), aimed by the pilot's request of this tick; the pilot stands.
- Engine: `ServerWorld::generated_voxel` (what the generator made, whatever was built), `Presence::view_in_the_world`; `Extent::distance_to`; Cave's movement
  has pilots (their keys are not for walking) and the plugin shares the players' bodies with the ship plugin.
- Found by building the test: the pilot's thrust was a tick behind the way they looked (the first W after looking level went up at the earlier
  pitch), because it read the record of their view, which the movement system writes after it: fixed by reading the request, with a test.
- The golden session's stream is 29 bytes longer (the reply to an unknown command lists `/ship`); the saved world is the same. Checked by a control run: with the
  plugin left out of the list the old pin came back exactly.
- Tests: 10 in `flight.rs` (a ship made of what was built and not the ground, which refuses what nobody built and what is too big; the nearer of two ships and one
  out of reach; hold and push, the push's gain of 6 a second to 5 percent; a pilot who hovers, flies north level and up when looking up, stops, goes up and down
  with the same eyes in the ship all the time; only one pilot; a pilot who goes away; a scripted flight pinned to the bit), 5 for the rules, 1 each for the two
  engine questions, and in `pictures.rs` the end-to-end test of the phase: a ship is made, held, boarded, flown, turned by a push off its middle and left, recorded, and the
  replay draws the same picture as the live view to the pixel on every adapter. 29 mutations of the new code: all caught (the first versions of the
  flood-fill test and of the nearest-ship test were not enough, and were made to start from every block and to have two ships, before the run).
- Limits: nothing turns a ship but a force off its middle; the thrust numbers are a guess; `/ship create all` is refused wherever the ground is joined.

Phase 32h, a separate change (a walker slides along terrain walls; it found what the golden session of 32h had sat on):
- Cause, found by tracing Cave's golden session (a walker stood still for 104 steps against a one-voxel hillside wall): the shape cast gives the wall a
  normal with a hair of y (1.2e-5), the slide along it has a hair of downward speed (-3.3e-7), the controller then meets the floor at once (toi 0) and,
  because the floor is a slope it must not slide down and a flat floor has no direction along it to keep, drops the whole slide. Not a
  matter of the wall's height or turn: a plain one-block wall on a flat floor does it (31 of 144 walks at different slants and places stood still).
- Fix: the retry made in 32g for ships (the slide made again, level and along the wall, when the first move got under a tenth of it) now applies to
  every wall. Alone it left 3 of 144, because the slide made again touched the wall again (it is within the controller's gap) and met the same hair;
  it now leans 5% of its length away from the wall. A sweep of 1440 walks (three frame heights, 60 slants, 8 places) has none that stands still.
- Tests: `walking.rs` has the walk that stuck in the golden session's way (stood still 87 steps before) and the sweep (31 of 144 before). Mutations: 7,
  6 caught (no lean; lean into the wall; the retry never made; never taken; walls of ships only, which is the old behaviour; every surface a wall);
  1 survives, "the retry is always taken" (the only difference is that a retry that got less than half of the slide replaces a first move of under a
  tenth of it, which I believe, from reading and not by proof, is never seen). `PINNED_SCRIPTED_RUN` and the foreign and oriented runs did not change.
- The golden session changed, as it had to: the stream hash and its length (7,183,468 to 7,184,946 bytes); the saved world did not.

Phase 32g (aboard: players in a ship's frame; boarding and leaving):
- Entities in any frame: `EntityPose.grid` may be a ship (`voxel_space`: `Orientation::{compose, inverse}`, `GridPose::{place_in_world, place_in_grid}`;
  `entities`: the store takes any grid and has `in_grid`; `server`: `spawn_entity` and `set_entity_pose` refuse a ship that is not there
  (`EntityError::NoSuchGrid`), `remove_grid` puts what was aboard in the level at its world pose in the same call, and replication tells a client of
  the entities aboard each ship it knows, with the ship, and takes them with it when the ship leaves the picture). A world is saved with the passengers
  in the level where their ship has them (ships are saved in 32i); a file with an entity in a ship's frame is refused. `MovingGrid::{brick_extent,
  velocity_at}`; a ship that sleeps has no speed on record.
- Presence: `pose_of`, `players`, `level_of` answer in the world, `frame_of`, `local_pose_of`, `set_pose_in` serve what moves players;
  `PlayerPose::{view_orientation, frame_rotation_of}`; one new presence message, `player_presence:placed` (a returning player is told where they are;
  this also fixed a latent fault for players who logged out far from the start).
- Physics (`CharacterBody`): a frame with an up (against a gravity vector: `step_with_gravity`), a facing that is carried along as up turns, and a box that
  stands in it; `reframe`; ships near a character are colliders that stand where the ships are (`update_foreign`, `foreign_brick_changed`); a slide
  along a ship's wall that comes out as nothing is made again; `keep_clear`. With up `+y` every operation is what it always was: the pinned scripted
  runs and the golden sessions did not change.
- `grid_bodies`: `BlockCollisions` (cached, shared with Cave), `ShipHull::solid_at`. Cave: `aboard` (the two rules), `movement` (a body in a level or aboard
  a ship, made to agree with the player's record each tick), the system after the ships move; `/tp` writes the record at once. Renderer and shell:
  `Camera.frame_rotation`; the client follows a passenger's pose in the world and the turn of their room.
- **Found by measuring, and fixed (all with regression tests):** (1) a ship that fell on a standing walker pressed them into the floor and they fell down
  through the terrain and the world (eyes at 202.6 to 94.6): the controller slides a character out of an overlap along one collider's surface with no
  regard to what is behind it, and the colliders of terrain leave out boxes buried in solid terrain; a ship at 60 a second left a walker half in the
  floor, and a block built over a walker's head made the controller shake them 0.45 into the floor and out again every step. Each tick, after the
  ships' colliders are in place, `keep_clear` moves a character that something has moved into out the shortest way (at most 8 voxels; if there is no room
  it stays). (2) The walker so lifted onto the falling box was thrown ten voxels up at the speed the box had: boarding now drops the speed relative to the
  ship that points away from it. (3) (earlier) the controller lost the whole of a slide along a turned ship's wall: made again, for ships only.
  Two of my statements in the spec were wrong and are corrected there: that nothing pushes a player out of a ship that moves into them (a slow ship did,
  a fast one or one falling did the harm above), and that the controller leaves a body it cannot push out.
- Tests (100 new, and 2 groups of properties; the whole workspace passes on Windows, 1492 tests, and on Linux, 1493, one of which is Linux only): frames and the turn (non-commutative `compose`, round trips); the store and server (entities in a ship's frame, a
  ship removed under passengers, replication, the save and its refusal); presence and the wire (pinned bytes); the oriented controller (a room turned a
  quarter or over: the same room turned; the facing carried, gradually and in one step; a body put into a block of a turned frame leaves by the first way in
  its own order); foreign grids (standing on, walking into and along a turned wall, edits, a ship that moves, a ship far away looked at by hint only, a
  pinned run, ships that fall on, sail into and jump over a character: a pinned run); Cave `aboard.rs` (29 scenarios: landing on a ship that is still, moving,
  fast, turned and moving; the moving ship carrying the walker exactly; a jump; walking off the edge, over a hole; leaving with the ship's speed in the
  world's axes; a cabin turned on its side and over; gravity of the world's strength; logout and return aboard; `/tp`; two players; the view not turned by
  boarding, leaving or the ship being taken away; a scripted run pinned), the picture of a replay of a walker on a turning ship equal to the live one to the
  pixel, and an end-to-end replay with a rider wisp. The pinned values (the aboard run, the foreign runs, the ship physics hashes) are equal on Windows and Linux.
- Mutations: 73 experiments over the new code (frames, store, server, replication, save, presence, the controller's frame, foreign colliders, keeping clear,
  the rules, boarding, leaving, the client): 63 caught at first; of the 10 survivors 8 were gaps in my tests (the sign of a turn crossed with an arm in two
  of three components; the facing carried the wrong way, hidden by the projection that follows when the steps are small and found only in one big step; the
  box for other grids taken at full size, or not turned with the character; the order of the ways out in a turned frame; boarding a turned ship that moves;
  keeping clear aboard, which a block over the head shows and a block at the feet does not, since the controller gets out of that by itself; and a ship taken
  from under a walker, which shows nothing unless the ship has turned since they boarded) and each got a test that now catches it (checked by running the
  mutants again). The other 2 survive every suite I ran them against (all of physics, and the aboard, play, golden, ship and picture tests of Cave); I
  believe, from reading the code and not by proof, that they change nothing anyone can see: taking away the speed toward the ship at boarding as well (the
  controller stops that speed in the next step anyway, and the two differ by less than the controller's gap of a hundredth of a voxel for one step), and
  counting any collider as another grid's (terrain colliders and the voxels agree wherever the colliders are up to date, which the edits of the same tick
  see to).
- Cost (median of 1000 server ticks, Windows): 21 microseconds with a ship and no player; 50 to 60 with a walker near the ship, under it or aboard it; 75 on
  hills far from it; 250 with eight aboard. The check for being inside something changed none of these by more than the run to run difference (about 5).
- Limits (see `moving-grids-spec.md` 2.6 and `plan.md`, Open): a body aboard does not see terrain or other ships; a wall that jumps over a standing walker
  in a tick passes them without touching; nothing is hurt by being pressed; boarding and leaving are looked at once a tick (the first text said three).

Phase 32f (gravity as a field):
- `voxel_space::gravity_at(settings, pulls, position)`: the level's constant plus, for each body of at least the threshold mass, `G m r / (r^2 + s^2)^(3/2)` toward its centre of mass (softened, so finite and zero at the centre).
  `ServerConfig::gravity` and `ServerWorld::set_gravity_settings`; `MovingGrid` has `mass` and `centre_of_mass` (the bodies write them); `ServerWorld::gravity_at` and `gravity_in`; `Context::gravity_at`.
- The bodies take their gravity from the field: Rapier's own gravity is zero and each tick a moving ship gets `mass * gravity` at its centre of mass as a force that does not wake a sleeper (new
  `BodyWorld::apply_steady_force`), except that a change in gravity of more than a thousandth wakes it. The Cave ship tests and their pinned hash did not change.
- Tests: the field (inverse square, direction, the level added, finite and zero at a centre, the threshold including exactly it, sums in order, exact a billion bricks out, bits pinned and checked against Python);
  the world (the level's down in the axes of a ship turned over and a quarter turn; a ship's pull is toward its centre of mass, a ship that is placed or turned elsewhere pulls from there, small ones pull nothing);
  the context (a rule asks for terrain and for a ship turned over, and for a ship that is not there); Cave: a ship of 4.1 million kilograms held in empty space pulls a ship of 6 voxels that is 36 voxels
  away with the acceleration the law gives, to 3 percent, toward its centre of mass and not its corner; with the threshold above its mass, or none, nothing is pulled; a free big ship pulls and is pulled by a held one and is
  pulled by none alone. 14 mutations of the field and its uses, all caught (after one was found to be equivalent: a ship's pull on itself is zero at its centre, so there is nothing to leave out, and the argument for it was removed).

Phase 32e, part 2 (ships move in the server: the `grid_bodies` crate):
- `grid_bodies::GridBodiesPlugin` (registered by Cave beside its other plugins; configured with the block table, channels, the game's passable property and `engine:density`): a system of
  the Physics phase keeps one `BodyWorld` for each level that has ships in step with the server's world. A ship that appears gets a body and a collider made from all its bricks; blocks that change
  (the `ChannelsChanged` event) make the colliders and the mass of their brick again; the terrain within a brick of a ship's bounds (two before it is let go) has colliders made from the world's blocks; forces
  asked with `ServerWorld::apply_grid_force` are applied for the tick; the bodies are stepped; and ships that moved are given the pose and speeds the bodies came to (a sleeping ship is left alone, so it is not told
  of again). A ship put somewhere by anything else (`set_grid_pose`) is simulated from there; a ship that is `held` is a fixed body; a ship that is gone loses its body.
- `MovingGrid` has `linear_velocity`, `angular_velocity` and `held`; `ServerWorld` has `apply_grid_force`, `take_grid_forces`, `set_grid_held`, `set_grid_velocity`. Mass is `engine:density` (1000 kilograms a voxel
  by default) times the share of the cell a block fills, summed by brick.
- Cave's movement now uses `grid_bodies::collision` (the collision of a block, the terrain of a level as a source of collision) instead of its own copies; the walking tests and the golden sessions did not change.
- Limits: a level has one frame, whose origin is the first ship's brick; a ship more than 500 bricks from it is not simulated (it stays where it is) until there are no ships left in the level, when the next sets the frame
  (the simulation distance of 32i, and a frame for each place, will replace this); the velocities are not sent to clients (they see poses); the terrain colliders are made on the thread of the tick (a ship that has just fallen into new
  ground makes up to 27 bricks' worth, which is the cost measured for the character too); Cave's terrain is bumpy, and a cube that lands on a slope rolls, which is the physics and not a fault (the tests land on a floor they make).
- Tests, in Cave: a ship falls (the player is told each tick of exactly where it is), rests on the floor to 0.05 of a voxel, sleeps; the same push moves an 8,000 kg ship faster than a 27,000 kg one, in the proportion of the masses, and
  the mass follows an edit; removing the blocks under a ship or the floor under it sinks it by that much; a held ship stays in the air until let go and a ship put somewhere carries on from there; a ship lands on a held one, and ships
  can be removed; a ship far from the first is not simulated, and the next in an emptied level sets the frame; flames are not in the way; the bits of a busy scene (a turned ship, a pushed one, an edit) are the same twice and pinned. 22 mutations
  of the plugin and its server calls: all caught but two memory and timing details (the terrain margin, and the terrain not being let go), which nothing observes.

Phase 32e, part 1 (bodies, in the physics crate; the server does not use them yet):
- `physics::BodyWorld`: Rapier bodies made of voxels in one frame (a brick of the level is the origin, `recentre` moves it keeping everything where it is): `add_body`,
  `set_brick` (a brick of a body from a `CollisionSource` in the body coordinates: whole cubes are the voxels of one `Voxels` collider edited in place, partial blocks are boxes),
  `set_mass`, `set_state`, `set_kind` (dynamic or fixed), `apply_force` (for one step, at a place of the body or its centre), terrain bricks made with the same `build_brick`
  the character uses, `step` (a tick in substeps, 3 by default, gravity 32 as the character has), `state` and `state_hash`. `MassMoments` sums mass, first and second moments of
  voxels per brick in `f64` and gives the tensor of inertia about the centre of mass (Rapier diagonalises it).
- Tests (19, with the 7 of the mass): a ship falls at g, rests on the ground to 0.02 of a voxel, sleeps; a force is for one step and its acceleration is force over mass; an
  off-centre force turns it the right way; removing the blocks under a ship sinks it by them; a half block holds a ship up half a voxel down, also far along the ship; a ship lands on a
  fixed one; terrain colliders hold or not as they exist, and a brick made again replaces the old; a turned ship falls without turning and rests on its lowest corner; far from the frame's
  origin and after `recentre` it is where it was; errors change nothing. The pinned hash of a busy scene (three ships, forces, an edit) is the same on Windows and Linux.
- 25 mutations: all real ones caught after three tests were added (a half block far along the ship, a removed body leaves nothing, a terrain brick made again replaces the old); three
  mutants are equivalent (a speed Rapier already zeroes, a hint that only saves time, a far-corner check that makes the near one redundant), and two lines were removed as redundant.

Phase 32d (ships drawn and aimed at):
- Renderer: a placement is now an offset and the turn of the grid (64 bytes, in a stride that was already larger); the shader turns a quad's place and its
  normal (the sun stays in the world). Bricks of ships are kept by (grid, brick) beside the terrain's, with `set_grid_pose`, `upload_grid_brick`,
  `remove_grid_brick`, `remove_grid`. A ship brick is placed in `f64` (the ship's origin relative to the camera, bricks differenced exactly, plus the turned place of
  the brick) so a ship, or a brick far along one, is as exact as terrain; the cull is a sphere around the turned centre of the brick.
- `world_view::GridMesher`: meshes ships' bricks like the terrain's (same `mesh_brick`, same light hook) on the calling thread, a budget of bricks a call
  (16 in a session), a brick and its six neighbours when one changes; an absent brick is empty. `MeshUpdate` has four new kinds.
- Session: `ship_updates` tells the renderer of poses that changed and ships that went; `target()` casts into the terrain and into each ship's own coordinates (the camera
  and ray turned back with `GridPose::world_to_local` and `direction_to_local`) and takes the nearest; `RayHit` has the `grid` and the `distance` along the ray.
- Cave: `SetBlock` names a grid; the server accepts the terrain of the player's level or a ship in it, measures reach to the centre of the voxel as turned and placed (`PlayerPose::
  distance_to_position`), and drops anything else (a ship that is not there, another level's grid, too far).
- Tests: GPU, on every adapter: a ship turned half way, tumbled about a diagonal, upside down, with blocks that do not fill their cells, and a brick far along a ship is
  drawn pixel for pixel as the terrain it amounts to (the quaternions chosen are exact); far from the origin is the same as at it; a 45 degree turn is as wide as the diagonal;
  a ship is not drawn without a pose, is culled behind the camera, is drawn when the camera is inside its brick, and goes when removed. In Cave: a player aims at, breaks and builds on a ship not turned, turned half
  way round and a million voxels away; the server refuses out of reach, missing and foreign grids; a ship between the player and the ground is aimed at first; a ship appears,
  moves, is edited and goes in the pictures, the last to the pixel; a lamp on a ship lights the ship. 24 + 5 mutations; the survivors found weak tests (a brick far along a turned ship, a
  detailed block turned, the camera inside the brick) which were then written. One is left that is not observable now: a request for the root grid of another level is
  refused before it could be told apart from a missing level, because Cave has one level.
- Limits: ships are drawn at full detail with no level of detail (plan: terrain's first); a ship has no sky light and its light is its own (see 32b); the mesher for a ship runs on the calling
  thread, so a very large ship arriving takes several frames of 16 bricks.

Phase 32c (ships in the stream; protocol version 6):
- `BrickContents` and `VoxelEdits` name a `GridId` (a level's number is the same bytes as before), and `GridsSpawned`, `GridsChanged` and `GridsDespawned` tell
  a client of the ships in its region: server side `grid_replication` (a per-client list of what it knows, as for entities; a view of each ship made once a
  tick from `MovingGrid::world_brick_bounds`; a budget of bricks of ships per client, `ServerConfig::maximum_ship_bricks`), client side `ClientGrids` and
  `ViewSummary::grids`. Ships are not drawn yet (32d): the mesher still takes only the bricks of terrain.
- Old golden sessions: with the version set back to 5 the old pins passed, so the stream of the terrain is the same bytes; with 6 only the hash of each stream and
  the checksum of the standard recording changed (lengths and saved-world hashes did not), and were re-pinned.
- Tests: wire layouts pinned (a ship's number is a five byte varint), a root grid is refused as a ship; server: a ship in the region is sent with every brick
  in order, none outside it or in another level, ships after the region and before game messages, a move once, an edit of a known brick and a new brick,
  leaving and returning, the budget, removal in the tick of an edit; client: build, move, digest-checked edits, forgetting, six kinds of mistake;
  end to end: a ship that turns, moves, grows and goes, in a recording replayed to the same view tick by tick, and the property test that every player sees
  what the server has now includes the ship flying through regions that move. 17 mutations, all caught.
- Not covered by a test, and so not claimed: that the mesher ignores a ship's `BrickChanged` (it would only waste work until 32d).

Phase 32b (grids on the server):
- `world_storage::{GridStore, MovingGrid}`: the ships of a world (a level, a `GridPose`, and a sparse `voxel_grid::Grid` of their own), ids from a counter that
  only goes up from `GridId::FIRST_ALLOCATED`, `retire` (a split: every part a new id, none the original's) and `merge` (the result a new id), tombstones, and
  `resolve` (follow a retired id through splits and merges to the live grids with the composed transform). `WorldData::grids()`.
- `ServerWorld::grid` answers for ships; `ServerGrid` (the rule's `GridView`) reads and writes a ship's voxels in its own coordinates, records the edits under
  the ship's id (`edits` is keyed by `GridId` now) and says which grid a watched channel change was in. `create_grid`, and `detach_blocks`: takes the blocks at
  some voxels of the terrain out of it (every channel to its default) into a new ship that is where they were, its origin the lowest corner of their box.
- `ActiveVoxels` leaves a ship's pending work out of the sections of the levels (it used to panic): it will be saved with the ship (32i).
- The frame-invariance test (`tests/frames.rs`, plan requirement 6): Cave's light settles to the same light in the terrain of a level and in a ship at five
  poses (turned in five of the 24 ways, bricks apart and a million bricks away). It runs through the real plugins, so it caught nothing wrong but pins that the
  light plugin works on any grid by `GridId`.
- Mutations: 16, all caught after two tests were strengthened (the order of a chain of transforms needs a turn that moves the x axis; a second channel must
  travel with a detached block).
- Limits recorded, each owned by a later step: edits of ships are not sent to clients (32c); a ship is not saved (32i; a world saved now drops its ships, and no
  command can make one before 32h); entities cannot have a ship as their frame (the entity store still refuses, 32g); Cave's fire does not burn on a ship (its
  range test is "a player is in this level", and the ship's simulation distance replaces it in 32i).

Phase 31 (a wider material palette):
- The palette moved from a 256-entry uniform array to a storage buffer of linear RGB (16 bytes a material, 16 MiB at the most), rebuilt by `set_palette` with a new
  bind group. The 16 avatar colours no longer take palette places: they are the last 16 values a quad can carry (0xFFFFF0 and up) and live in the frame uniform,
  so every value below `MAXIMUM_GAME_MATERIALS` (1 << 20, a choice, not a format limit: the quad has room for 16 million) is a game's. A value past the palette
  draws magenta (the shader checks `arrayLength`). Entity boxes and avatars are meshed once from a small block and given their material afterwards
  (`with_material`), since a block table with a block for each value would be a million entries.
- Tests: wide materials (255, 256, 65,535, 65,536, 70,000, 299,999) draw exactly as the same colour low in a small palette; values past the palette are magenta;
  avatars and boxes are right with palettes of 500,000 and 400,000; the largest palette draws and one more panics; the shader's `AVATAR_BASE` agrees with the Rust one.
- 11 mutations: 10 caught (after adding a unit test of the linear conversion); 1 equivalent (clearing 24 or 16 bits of a mesh's old material: it is always 1).

Phase 30 (the user interface):
- 30a: the `ui` crate: the tree and its limits, a canonical byte form with a decoder that refuses before it builds (a description nested 5,000 deep is refused
  without recursing), layout in whole pixels at nine anchors with stacking, hit testing, keyboard focus. 21 tests, 3 properties; 17 mutations caught.
- 30b: the renderer's overlay (`set_overlay`, `OverlayRect`; one instanced quad pipeline, alpha blended, after the crosshair) and `ui::paint` (text from
  `font8x8` as rectangles). GPU tests on every adapter: exact pixels, blending, clipping, non-square pictures. A first design (one oversized triangle and a
  discard) lost ten times as many pixels on the AMD GPU as plain quads; quads kept.
- 30c: forms: the three messages, `Context::show_ui/update_ui/close_ui`, `UiResponded`, the session's open forms, replay. 8 integration tests, 9 unit tests,
  2 Cave tests; 15 mutations, 14 caught (the survivor is memory hygiene, as for prompts).
- 30d: the shell: `game_shell::screen` (build the list, map clicks and keys to actions, pure and tested), `view::show_screen`, the window draws the status,
  the command line and its reply, forms and questions, and a modal takes the mouse and keys; `Session::screen/apply_screen_action/hud_lines`. 11 unit tests,
  a GPU test of the pictures, a replay that draws exactly what was drawn live; 12 mutations caught. A look at a picture of everything: legible, stacked, dimmed.
- 30e: the `overlay` client verb and `ClientContext::block_definition`; Cave's hotbar (swatches in the server's colours, the chosen one outlined, counts in
  survival, a pack's placeable block among them) and an inventory panel on `E`. 11 mutations of the verb and the hotbar: 9 caught; the 2 that survive cannot be reached (a block with no definition is not on the hotbar, and a stack count never passes a u32).
- The flake of the replay picture test is the AMD GPU leaving pixels out (found while pinning pictures); pictures are taken once two frames agree.

Phase 29 (data packs):
- 29a: the `data_packs` crate reads `pack.json`, block files and patch files into the model, collecting every problem with its file and JSON pointer,
  with "did you mean" for near misses (20 tests of exact messages; 17 reader mutations caught after two assertions were added for the survivors).
  `block_models` gained named property setters (`set_property_value`, `property_value`, `remove_property_value`, `add_state`, `set_state_colors`).
- 29b: the loader: dependency order (ties by id, cycles and missing needs refused), blocks in name order, patches in name order, `test`, `set`,
  `remove`, `add`, `optional`, namespace rules, atomic patches; directory reading. 23 tests including a shuffle property; 19 mutations caught (four
  survivors each got a test: patch file order, the default read by `test`, a failed patch leaving a trace for the next, bools given numbers).
- 29c: the generated schemas and a differential test against `jsonschema` (a dev-dependency): a corpus of 75 files, and three properties that damage
  a good file in one place; 11 mutations of schema and loader caught.
- 29d: `GameContent::apply_packs`, `ContentPackage::apply_packs` (the game's blocks keep their numbers, placeholders keep their places, nothing changes on
  an error), `--packs` for play, `--host` and `serve`, `voxunami check-pack GAME DIR` and `pack-schema GAME DIR`; the client refuses a package with more
  colours than the renderer has. Cave tests: a client with no pack draws the pack's blocks and sees them placed, a recording carries them, a world saved
  with them loads without the pack and saves back byte for byte, a dedicated server with `--packs` and one that will not start. 8 wiring mutations caught.
- 29e: `voxunami_cave:placeable`: Cave's items, drops and nine number keys come from it; `ClientContext::blocks_with` reads the server's package. Tests:
  a pack's placeable block has a key, an item and is used up; one that is not leaves no item. Cave's golden repinned once (the package grew by the
  property: 1,345 to 1,647 bytes, +302 in the stream, saved world unchanged).
- The first mutation run of the wiring hung for twenty minutes: a "does not start" test would have started a real server forever if the server had started.
  It is told to stop already, so it fails instead.

Phase 28 (the content package at the handshake):
- 28a: canonical byte form for `BlockDefinition` and `PropertySchema` (`block_models::codec`) and for `PackageData` (`game_shell`): round trips,
  refusals with their reasons, cut-short and trailing-byte checks, and three properties (any definition round-trips and has one spelling; arbitrary
  bytes are an error or have one spelling). 11 mutations of the codec are all caught.
- 28b: protocol version 5, `Welcome.package`, `ServerConfig::with_package`; `GameContent::package_data()` (the three games and the shell's example
  game); the server sends it. Support for streams of other versions was deleted (the `OLDEST_READABLE_PROTOCOL_VERSION` constant, the `Players`
  message, the session's fallback for it, the tests of both).
- 28c: the session decodes the package, builds its block table in its own numbering by identifier, finds the view channels by name in its layout
  and takes the ambient light from it; `configure_renderer` takes a table and an ambient level, not a game. Tests with servers whose package is not
  the client's own: the server's colours and ambient light win, blocks match by identifier whatever order they are listed in, a block with no
  definition is unknown, and a package with an unknown channel, registry or property, or damaged bytes, is a `SessionError` naming why. 8 mutations
  of the session and package code are caught.
- Goldens repinned once, cause in each file: the stream grows by the package and its two byte length (2048: 2,611 + 2, 55,415 to 58,028; Cave:
  1,345 + 2, 7,181,819 to 7,183,166; Eels: 648 + 2, 70,732 to 71,382); saved-world hashes did not change; the end-to-end recording (empty package)
  grew by 1 byte.
- The plugin test rig of Cave duplicated `build_server_with_plugins` and sent no package; it uses `Rig::with_plugins` now.

Phase 27i (documentation and gates):
- `plugin-api-spec.md` gained section 13 (entity kinds, items, the `Context` calls, the prompt, the three new events); the other documents were
  updated by the steps that changed them. Gates for all of Phase 27 on both systems: `cargo fmt --check`, clippy with `-D warnings`, and the whole
  workspace suite pass on Windows (with a GPU) and on Linux.

Phase 27h (the prompt facility):
- `plugin_host`: `Context::prompt` / `cancel_prompt`, the event `PromptAnswered`, the engine messages `engine:prompt`, `engine:prompt_answer` and
  `engine:prompt_closed`, limits (9 options, 200 bytes), numbering from 1 never reused, questions dropped when the player leaves, an invalid or
  unreadable answer ignored (unreadable counted). `game_shell`: the session keeps the open questions from the stream, the shell's HUD line shows
  the oldest first (with `[k more]`), the built-in `.choose N`, `ClientContext::prompt()` / `prompts_waiting()`.
- Tests: 7 unit tests (limits, codec, state), 7 server integration tests with a fake asking plugin, client unit tests (HUD and `.choose`), and 2 Cave
  session tests (the question on the HUD until `.choose 2` answers it and the plugin hears option 1; a replay shows it while asked and not after).
- 19 mutations: 16 caught; one (the host dropping a leaver's questions) survives because it is unobservable (client ids are never reused, so
  it only frees memory); one (a guard against a repeated question id the server never sends) was removed; one pattern did not match.
- No golden moved: no golden session asks a question.

Phase 27g (items, inventories, drops, game modes, boxes):
- Server: the player entity has a nine slot `Inventory` (replicated); Cave declares an item for each of its six placeable blocks, the kind
  `voxunami_cave:dropped_item`, `/gamemode [creative|survival]` (saved per world, not saved while creative), and an items plugin: in survival a broken
  block leaves its item, items fall a voxel every fourth tick and join and are picked up within 3 voxels, placing needs the item and uses it
  up. Block editing now announces `BlockBroken` and `BlockPlaced`. `docs/entities-spec.md` 2.6b has the decisions.
- Client: `ClientContext::entities()`; the verb `entity_appearance`; `EntityBox` in the renderer (22 renderer GPU tests pass on both adapters, 2 new);
  Cave's client shows the survival count on the screen and draws items as boxes.
- Tests: 12 item tests through a server and a session (creative changes nothing, drops, falling and joining independent of the tick phase,
  pickup range, a full and a partly full inventory, placing, saving, the screen, the box), 6 unit tests of the pure parts, 2 of the verb, 2 of
  the renderer, and the Cave golden grew an items script. 22 mutations of the new code: all caught but one that changes nothing (air announced
  as broken: air has no item).
- Goldens repinned once, with every size difference accounted for: the inventory is 28 bytes in each spawn of a player (+1 for 2048, whose frame
  grew past 127 bytes), the world description names the new kind (27 bytes) and the newcomer is told the game mode (30 bytes): Cave +85, Eels +56,
  2048 +29, before the Cave script gained the items segment.
- A test of mine failed once on Linux: it guarded the idle ticks of a joining test with an assertion that settling took no more than a fixed
  number of ticks. The number was not enough on a cold build. Tests that wait whole intervals and assert only settled states do not have this.

Fix of a flaky test (after Phase 27f): a body put inside terrain is moved out of it.
- Cause found by instrumenting and sweeping, not guessed: three explanations were tested and ruled out first (a body embedded after the level
  switch and then lifted, a sliver overlap with a taller column, a stale collision cache). The chain and the evidence are in `docs/plan.md`.
- `engine/physics`: `CharacterBody` clears a body that was made or teleported (`move_out_of_terrain`), see `design-decisions.md`. 33 tests in
  `walking.rs` (16 new: 6 of them fail without the fix, the other 10 guard the policy and fail under wrong versions of it) and 2 properties (random scenes and random hills, put anywhere, any depth) that fail
  without it (the body falls through the floor to y = -64, or is left inside the hills). The pinned scripted run did not change.
- Two independent reviews (one of the physics code, one of the root cause and the tests) found, and I fixed: "straight up" launching players
  through ceilings and walls (now the shortest way), a panic at the top of the world (now solid), a footprint formula in the test that had a
  failure window of its own (now two-part: not inside what it overlaps by more than 0.02, standing on what it overlaps or is within the
  controller's skin of), a scenario test that did not fail without the fix (replaced), and load-dependent tick counts in the original test (fixing the tick
  was tried and removed: settling is unbounded under load, and the Linux cold build exceeded the fixed tick; the walk threshold is 2 now). 15 mutations of the new code: all caught, by the physics tests, the Cave tests, or both; the properties alone leave two
  mutants (the margin, only the middle column) to the Cave test.
- A side-room rule (keep the controller's 0.01 skin from walls when choosing where to put the body) was tried and made things worse, by
  rejecting the good place straight up beside a taller wall; reverted. A property that asserted how a body *rests* failed on rare corners of
  the controller; it now asserts what clearing can promise (not inside, above the floor, not carried off).
- Measured: before, 39 of 256 probe teleports left the body inside terrain; after, 0. Sweep of 160 tick offsets: 11 failures before, 0 after.
  Whole-file runs of the Cave play binary on Windows with every core busy: 10 of 10 clean (38 tests each, 53 to 81 s a run). Linux was not
  run (WSL could not start, `HCS ERROR_NO_SYSTEM_RESOURCES`).

Phase 27f (players become entities):
- `engine:hidden` (entities crate) and its filter in replication. The `player_presence` crate: the kind `player_presence:player` and its
  components (key, connection, look), `Presence` replacing `PoseBook` (calls take the world; reads take `WorldData`), the plugin taking over a
  saved entity at `PlayerJoined` and hiding it at `PlayerLeft`, `LevelChange` at a rejoin in another level. `Presence::players` is a Vec.
- Clients: the session builds its players map from the player entities it was told of (a recording with the old `Players` message still uses it);
  the three games' server code and Cave's movement (a returning player resumes where they were) changed to the new calls.
- Behaviour that changed on purpose: a client knows only the players in the region it watches (Cave's level test said two, now one); a test that
  steps a session without asking for its view never learns its own place (a window asks every frame), so the walking test asks once.
- Goldens repinned once, with the cause in each file: the `Players` messages are gone, entity messages are in, the saved worlds hold their players,
  and 2048's and Eels's scripts now ask for their view each tick, as Cave's did. Checked by what the streams hold (kinds and counts of entity
  messages), since no old-code control gives the same stream.
- Tests: 12 for presence and levels (entities, key kept home, hidden when away, rejoin resumes, another key does not, level before the first pose,
  rejoin tells the level), the hidden flag, a Cave rejoin test, old-recording and wrong-kind tests. 20 mutations of this step are caught (three
  survivors each needed a test: the look on later poses, the old-stream guard, the kind filter).

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
