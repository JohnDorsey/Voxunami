# Architecture and direction review

Written 2026-10-29 by the assistant, at the owner's request: step back from graphics and ask whether the engine and Voxunami Cave
are well designed, whether they will be easy to mod and to maintain, whether complexity is boxed in the right places, how the
project compares with the most moddable games (Hytale first), what to change about how we work, and what we may be too close to
see. This document is a proposal. Nothing in it is decided until the owner says so (status key as in `design-decisions.md`).

## 0. How to read this

Facts carry a label. Opinions say how sure I am.

- **[measured]** I counted it or ran it on 2026-10-29. Appendix A lists the scripts and commands.
- **[read]** I read the code or the document myself; the file is named where it matters.
- **[reported]** An investigator I delegated to said so. *(checked)* means I then verified it myself. Without that mark, it is theirs.
- **[belief]** My judgement, not tested.

What I did: surveyed the workspace and its history; read the design record, the plan and the earlier plugin review; timed the build
and check loop; wrote a real data pack as a modder would and tried to break it; read the plugin and game interfaces; and had four
investigators work separately, none shown my conclusions: the modding surface, layering and cohesion, what a player can do today, and
outside research on Hytale and other moddable games.

What I did not do: play the game in a window (I cannot), run anything on Linux today, or test multiplayer (Appendix A lists what is
missing). Every number is for the commit `4312b9c` and one Windows 10 machine, with a warm build directory unless it says cold.

**Length.** This document is long because it carries its evidence, and F9 below is about exactly that problem. If you read four things,
read section 1 (the short version), section 8 (what we may be too close to see), section 10 (a sequence) and section 11 (the decisions).
The rest is reference. When you have decided, the decisions move into `design-decisions.md` and a short `ARCHITECTURE.md`, and this
document becomes history: a later session should not load it whole.

## 1. The short version

1. **The code is healthy. The direction is not yet set by a game.** The layering is clean by construction, the tests are dense, a
   change in any of the four crates I tried rebuilds in 3 to 6 seconds, and a type change across 26 crates was done in a day. The risk to the project is
   not how well it is built. It is what it is being built toward.
2. **The skeleton is right.** A server that owns the truth, a client that sees only a stream, bit-identical simulation, names for
   everything (`plugin:name`), data of missing plugins kept byte for byte, rules that see one grid at a time, a registrar of about
   twelve verbs with a fixed order, and data packs with a generated schema and loud, complete error messages. These are the hard
   decisions and they are made well. Protect them.
3. **Three organs are missing or half grown.** (a) The client is thin only for blocks: the sun, how far each level is drawn, the
   camera, gravity, the client's rules and the terrain generators are compiled into both ends. (b) Between "recolour a block" and
   "write Rust" there is nothing: no items, recipes, drops, interactions, or properties a pack can declare. A modder can set 13
   properties on a block. (c) The plugin API is an in-process Rust object with no boundary (`Context::world()` hands out the whole
   server world), so what the loaders of Phase 33 will have to fence in is not yet fenced.
4. **Hytale.** We follow its principles (server owns content, mods are data first, code only on the server, nothing for the
   player to install) and none of its execution (breadth of data, assets, in-game tools, a shipped game that is itself the example
   mod). That is the right amount to copy. We should not read its server source: the licence bars use for a product that competes with
   the game (checked on 2026-10-29; not legal advice). See section 5.
5. **There is not yet a game on top.** Cave is a sandbox with planets of a kind few games have (voxel ground from orbit to the soil,
   bodies on exact orbits) and no loop. Piloting cancels gravity,
   caps speed at 24 metres a second and cannot turn the ship. There are no engines, no fuel, no sound, no menu, no goal, and no way
   to fail. Six steps would make a first flight loop (7.1), and I expect the first four together to be about the size of one of the
   recent sky commits [belief].
6. **What we may be too close to see** (section 8): depth has been rewarded over breadth because "continue" lets me choose and I
   choose depth; nobody but you has played it; speculative generality (128-bit coordinates, a 116-level octree, exact orbits at
   the size of the universe) has become the centre of gravity of a project whose first-hour experience is silent and goalless; tests
   prove I did what I meant, not that it is good; and the record has grown into a place to put things down, not a map.
7. **What I would change** (sections 9 to 11): direct by player outcome, one milestone at a time, with a stop list; sort decisions
   into one-way doors (you decide, on a one-page memo) and two-way doors (I decide and log); scale rigour to the kind of change; you
   play for twenty minutes a week; a README, a one-page architecture and a state document; continuous integration on both systems; and
   three words (*pack*, *module*, *game*) used the same way everywhere.

## 2. What I measured

### 2.1 Size and shape

| | |
|---|---|
| Crates | 36: 31 engine, 1 content crate (`plugins/base_blocks`), 3 games, the launcher [measured] |
| Code lines (not blank, not comment) | 50,254 source, 87,879 tests, 463 shader. Tests are 1.75 times the source [measured; about 1.4 percent of the source is counted as tests because five files keep a `#[cfg(test)]` module before their end, and the layering investigator's more careful count is 50.9K and 87.2K] |
| Cave | 5,697 source, 19,410 tests (3.4 times) [measured] |
| Tests | 2,554 passed, 0 failed, 4 ignored in today's full run (2.3) |
| History | 161 commits in 14 days (2026-10-15 to 10-28); 201,246 lines added, 19,222 removed (9.5 percent) [measured] |
| Documents | 983 KB in 28 files; `build-log.md` alone is 294 KB, `design-decisions.md` 94 KB [measured] |
| `target/` | 61 GB [measured] |
| Upward or sideways dependency edges | None, in any of the 36 crates [measured] |

Several of the biggest files are mostly inline tests, so line counts mislead: `server_world.rs` is 4,545 lines of which 1,700 are
implementation; `voxel_renderer.rs` is 2,683 with 2,361 implementation; `grid_bodies/src/plugin.rs` is 2,154 with 1,889
implementation [measured]. The real hubs are the renderer file, the ship plugin and the world type; none is extreme.

### 2.2 Where the effort went

Lines added over the 14 days, by what the code is for (code and tests together, 179,441 lines) [measured]:

| | Share |
|---|---|
| World model, server, network | 24.4 % |
| Cave | 19.6 % (of it, 56 percent tests) |
| Terrain, levels of detail, meshing | 17.6 % |
| Rendering, shell, UI | 13.8 % |
| Mod and content machinery | 10.5 % |
| Simulation and physics | 7.4 % |
| 2048 and Eels | 6.6 % |

Graphics is not where most effort went. The world model and terrain together are 42 percent. The game itself, Cave, is a fifth, and
the larger part of that is tests of ships and planets.

### 2.3 The loop (Windows, development profile, warm build directory) [measured]

| Step | Time |
|---|---|
| Edit a content crate (`base_blocks`), rebuild the launcher | 3 s (3 crates recompiled) |
| Edit Cave, rebuild | 3 s (2 crates) |
| Edit the renderer, rebuild | 4 s (6 crates) |
| Edit the foundation crate 27 others depend on (`voxel_space`), rebuild | 6 s (30 crates) |
| Cold build of the launcher from an empty `target/` (dependencies already in the local cache) | **118 s**: 186 crates, 35 ours and 151 external, 2.7 GB |
| `cargo fmt --check` | 2 s |
| `cargo clippy --workspace --all-targets -- -D warnings` | 9 s |
| `cargo test --workspace --no-fail-fast`, `VOX_REQUIRE_GPU=1` | **916 s (15 min 16 s)**: 2,554 passed, 0 failed, 4 ignored |

Where the 916 seconds go: the GPU rendering file 228 s (97 tests, a quarter of the whole), the multiscale-light tests 73 s, the golden
sessions 69 s, ship parity 60 s, Cave's play tests 57 s, coarse agreement 42 s, docking 31 s. Tests run one binary after another.
The Linux run in WSL was not repeated today; it is a second run of about the same length that you or I start by hand before every
commit, since nothing in the repository runs it for us (F10).

### 2.4 How far a feature reaches

Files and crates touched by the commit that added each feature (`git show --numstat`) [measured]:

| Feature | Files | Crates |
|---|---|---|
| The sun is a star (sky, stars, night light) | 15 | 3 (renderer 533 lines, shell 346, Cave 2) |
| Shadow maps | 23 | 3 |
| The sun as a light of the clock | 41 | 12 |
| Docking | 41 | 12 |
| Planets from data | 39 | 8 (including 14 lines each in 2048 and Eels) |
| Generator recipes | 27 | 8 |
| `/tp` to a planet | 16 | 1 (Cave only, 609 lines) |
| 128-bit coordinates | 157 | 26 |

`/tp` is what a boxed feature looks like. A gameplay feature that needs the protocol, the server, the client and the shell touches
eight to twelve crates, and a Cave feature that adds a method to the shared game trait touches the other two games too.

### 2.5 The modder's first hour [measured, unless marked]

I wrote a pack of two blocks and one patch (4 files, 17 lines) and ran `voxunami check-pack cave packs`.

- Accepted in 0.7 seconds: `1 pack, 2 blocks added, 0 bodies added, 1 patch file, no problems`.
- Four mistakes in one block (a misspelt key, a `texture` key, a missing colour, a value out of range) gave four messages at once,
  each with the file and the path, a "did you mean" and the list of valid keys. This is better than most mod systems.
- A `recipes/` folder holding nonsense was ignored: `no problems`, exit 0. The loader reads three folders and nothing says so.
- The patch example in `pack-format.md` fails against `base_blocks:lamp` ("flammability is not set on this block, so there is
  nothing to remove"). The document's own rule forbids what its example does.
- `pack.json` rejects `format` and `version` as unknown keys: a pack cannot name its own format or version.
- Everything a pack may set on a block is `color`, `see_through`, `states`, `state_colors`, `properties`, `state_properties`, and the
  properties are the 13 the game declares: `base_blocks:emission`, `flammability`, `passable`; `engine:density`, `axis_face`,
  `front_face`, `docking_face`, `marked_face`, `mark_color`, `mark_both_ends`, `placeable_on_moving_grids`;
  `voxunami_cave:placeable`, `creative_only`. A pack cannot declare a property of its own.
- [reported] A pack with more than two blocks cannot place the rest: the number keys 1 to 9 are taken by Cave's seven.

### 2.6 What a player meets [read, measured where marked]

- No sound: none of the audio crates (`rodio`, `cpal`, `kira`, `oddio`, `symphonia`, `alsa`...) is in `Cargo.lock` [measured].
- Piloting a ship: `pilot_force` is `mass * (wish - gravity - drag * velocity)` [read, `games/voxunami_cave/src/ships.rs:85`].
  Gravity is cancelled, acceleration does not depend on the ship, and speed tops out at thrust over drag, 24 metres a second (48
  with Ctrl). The pilot passes no point to `apply_grid_force` and nothing calls `apply_grid_torque` [measured], so a ship cannot
  turn. A piloted ship cannot orbit.
- [reported, *(checked)* in part] No crafting, tools, health, damage, hunger, mobs, goals, menu, settings, chat or permissions.
  Coordinates are shown in hexadecimal by default. The hotbar shows the seven placeable blocks.
- Sculpt, the fourth game in `CLAUDE.md`, does not exist: there is no crate, no directory and no launcher entry [measured].
- No README, no licence, no tags or releases, no continuous integration [measured].

## 3. What is working

- **The layering rule holds mechanically.** I walked `cargo metadata` for every crate: no engine crate depends on a plugin or a game,
  no game on another game, no plugin on another plugin [measured]. This is the rule most projects state and break.
- **The change net is real.** The 128-bit migration touched 157 files in 26 crates and was finished and green the same day. That is
  the best evidence available that the structure tolerates bold change [measured, from the commit].
- **The loop is fast.** Touching a plugin or Cave and running again costs 3 seconds; touching the foundation crate that 27 others
  depend on costs 6 [measured]. Format, lint and check take 11 seconds warm.
- **Formats are treated as contracts.** Data of missing plugins survives a load and a save byte for byte, with property tests
  [reported *(checked)*: `world_storage/tests/round_trip.rs`]. A released generator is never edited. A section of another version is
  refused, never misread.
- **The pack tooling is good.** A generated schema, every problem at once with its path, order-independent loading, a `test`
  operation in patches, `optional` patches that warn (2.5 and `pack-format.md`).
- **The plugin registrar is the right size.** `event`, `entity_kind`, `item`, `system`, `mind_system`, `on`, `on_channels`,
  `on_ask`, `message`, `declares_message`, `command`, `save_section`: twelve verbs, one fixed order, events in bulk
  [read, `registrar.rs`]. It was shaped for a sandbox boundary, though not built for one (F4).
- **The determinism net is mechanical.** `clippy.toml` bans the platform-dependent float functions; golden sessions pin the
  recorded stream and the saved world on two systems [read]. Nothing I found in non-test code violates it (F11 has two small
  caveats).
- **The record is honest about its confidence.** Decided, proposed, measured, open. The earlier plugin review labels every claim
  as code, web or memory. Most project documents do not do this.
- **The levels of detail work.** A river a billion light years wide is drawn from 10^25 voxels away, and nothing in the renderer
  needed to change for it [reported by the design record, `future-goals.md`; your own picture agrees].

## 4. Findings, most important first

Each finding says what it is, the evidence, why it matters, and what I would do with a rough size in phases. Confidence is mine.

### F1. There is no game loop yet, and flight is a hover cheat (confidence: high)

Cave has seven placeable blocks and nothing to do with them: no crafting, tools, health, damage, hunger, mobs, goals, menu or settings,
and no sound (2.6). The owner's direction is clear and recent ("Cave is to become a space-exploration game, like Kerbal Space Program
and KSA", `future-goals.md`, 2026-10-25), and the same document says of orbits and manoeuvres: "None of these is asked for yet."
Piloting is `mass * (wish - gravity - drag * velocity)`: a ship hovers, speeds up the same whatever it weighs, tops out at 24 metres a
second and cannot be turned. There is no engine block, no fuel and no way to fail. About 5,900 of Cave's 19,400 test lines are about
planets, places, the sky and light, and 7,100 about ships, docking and aboard; none tests a loop, a goal, damage or fuel, because none
exists [reported].

This matters more than any structural finding below, because modding, scale and tooling are all in service of a game that a friend
cannot yet want. Section 7 lists the shortest honest path to a first flight loop.

### F2. The client is thin for blocks only (confidence: high)

`PackageData`, which the server sends in `Welcome`, holds the property schema, the block registry and definitions, the channels and the
ambient light [read, `package_data.rs:50`]. Everything else a client needs to show a game comes from a copy of the game compiled into
the client: the sun (`G::sun`, `session.rs:1031`), how far each level is drawn (`G::level_detail`, `session.rs:400`), the reach and
the crosshair, the starting camera, the client's rules and keys, and the terrain generators [read; the generators *(checked)*:
`session.rs:203`]. `Welcome` carries no list of packs, no versions and no hashes; the server refuses a join only for a different
protocol version or game id [reported]. A client regenerates terrain nobody has edited with its own generator, and compares digests
only when an edit arrives (`verifies_edit_digests`, `client_world.rs:412`) [read]: a client with a different build of the same
generator draws different ground until someone digs near it.

The review of 2026-10-17 called this weakness W2 and fixed it for blocks. It has since come back one feature at a time: each new
feature (the sun, how far a level is drawn) was added as another function on `GameDefinition` and not as data in the package. So
"players install nothing" is true of a block and false of a sun.

*What I would do* (one phase; the wire is free to change until the first release): a second package record of rules per level (sun,
distance drawn, gravity, reach, camera) that `GameDefinition` fills and the client reads; UI layouts and key bindings as declared data
(the server-driven UI of Phase 30 is most of the way there); a content fingerprint in the handshake (for each generator in the world,
the digest of one probe brick, which the client regenerates and compares, so a mismatch refuses the join at once and says why); and a
rule that no mod code ever runs on a client.

### F3. The mod surface is three things wide, and the middle is missing (confidence: high)

Today a player with a launcher and a text editor can mod blocks (a flat colour, boxes in eighths of a voxel, the 13 properties),
patches to those blocks, and planets and moons on rails (2.5). Everything else means forking the workspace: rules, items, recipes,
drops, terrain, biomes, ores, mobs, the sun, the UI, sound. There is no crafting code at all [reported]. A pack cannot declare a
property, so it cannot invent a vocabulary that a rule could read; `pack-format.md` says so: "new kinds of behaviour need code,
which is later". There is no `BlockUsed` event and a right click is wired to "place" [reported]. Packs are read once at start and
have no reload [measured: no reload code]; a body in a pack reaches only a new world [reported]. The seven first-party blocks are
written as Rust builders in `base_blocks/src/lib.rs`, not in the pack format [read], so the format modders use is not the one the game
uses; that is how a documentation example can be wrong for months without a test noticing.

The earlier plan put "no scripted data stage" and "behaviour needs code, later" side by side with a Phase 33 for loaders. What that
leaves is a gap where most mods live. Look at what people actually make for Factorio, Minecraft, Vintage Story, RimWorld: recipes,
loot, spawn rules, interactions, biomes, structures, UI. They are data, or they are a few lines of script on top of data.

*What I would do* (three phases, one memo first): let a pack declare properties (namespaced, typed, ranged, documented) and let a
pack's blocks carry them, so vocabulary no longer waits for Rust; add content kinds one at a time, each with a schema, an example and a
test: items, recipes, drops, hardness, names and descriptions; define a *closed* set of interactions (an event, conditions on item,
block, level or chance, effects such as set a block, give or take an item, spawn, push, send, run a command) that the engine runs
deterministically and validates at load; make the first-party blocks a pack read by the same loader; add `/reload`; and run the
doc examples as tests.

### F4. The plugin API has no boundary (confidence: high)

`Context::world()` returns `&mut ServerWorld`, the whole world [read, `context.rs:65`]. Plugins call it 30 times in the games and 23
times in engine plugins, 53 call sites with tests left out [measured]. There is no sandbox, no version on a plugin, no capability limit and no panic containment
(`catch_unwind` appears only in tests); the declared-channel check runs only in debug builds, only for systems [reported]. Events are
Rust types matched by `TypeId` and handlers are closures [read, `event.rs`, `registrar.rs`], and `Registrar` has no `content`, `channel`
or `property` verb, which the plan lists [read] and `plugin-api-spec.md` §3 lists [reported]: a plugin cannot bring its own block. The registrar *is*
the right shape for a boundary (bulk events, names, one order), but nothing yet proves a boundary can be drawn through it.

This is the cost that grows with every plugin written before Phase 33: each call to `world()` is code to rewrite against a narrower
API later, and no one yet knows how narrow it must be.

*What I would do*: a throwaway spike of two or three days, now, before the next plugin: compile one tiny rule to WASM, call it
through a host API of a dozen functions, and see what the Rust-shaped parts of `Registrar` and `Context` cost to cross. Then split
`Context` into the narrow *host API* a module sees and a trusted escape hatch, count the uses of the hatch, and make that count a
tripwire that may only fall.

### F5. The engine is Cave's engine (confidence: high on the facts, medium on what to do)

No upward dependency exists (3), and the engine is clean of game and plugin names: a scan of non-test engine code found no `eels`,
`sculpt` or `voxunami:` id and no block named sand, wood, ore or lava, and `cave` and `2048` only in doc examples and fixtures
[reported]. Two things are different.

*Promotion is inverted.* `CLAUDE.md` says "new code starts in the game that needs it; promote it to shared code when a second game
does". Four engine crates have Cave as their only consumer (`physics`, `grid_bodies`, `grid_lifecycle`, `lattice`: 5,664 lines) and 19
more engine files are named for ships, planets, rails, the sun, shadows or rivers (4,270 lines): about 23 percent of the engine and
plugin code serves one game [reported; the four-crate sum matches my survey]. It happened through decisions recorded as the owner's
("planets are ships", "all planets will be on rails", "a star is a body like a planet"), so it is deliberate. It is a rule that no
longer describes what we do. 2048 and Eels do not link the four crates, but they carry the sun, level-of-detail, planet and ship-save
code through the 25 crates `game_shell` pulls in.

*The terrain crate owns Cave's world.* `terrain` is the engine's only leak of content: 46 lines name stone, dirt, grass or caves
(`TerrainMaterials { air, stone, dirt, grass }`, the same layering restated in four files, `cave_wavelength_log2` and
`cave_half_thickness` as fields of the generic settings), and at least 1,249 of its 2,484 lines are the planet and river generators
that `base_blocks` merely registers [reported]. Because "a released generator is a file format", engine code is now bound for ever to
formats named for a plugin.

None of this is wrong for the engine of a space game, and all of it is tested. The cost is that a mod can change a number where a
struct exposes one (`ShipSettings`, `GameDefinition::sun`) and cannot change a rule; that a second game cannot reuse ships or
planets without taking all of Cave's choices with them; and that each rule is one more thing the engine must keep for ever.

*A correction to my own first count.* I first counted the words planet, sun, moon, orbit and star in the engine as 1,634 mentions in
62 files, 332 of them in `server_world.rs`. That included inline tests. In implementation code it is 715 lines of 51,187 (1.4 percent),
in 14 of the 31 engine crates, concentrated in `grid_bodies/src/plugin.rs` (126), `voxel_renderer.rs` (58), the shell's sun module
(52), `server_world.rs` (52) and `world_storage/src/rail.rs` (49) [measured]. The vocabulary is concentrated, not everywhere; the
facts above are the real argument.

*What I would do*: say plainly in `CLAUDE.md` that this is the engine of a space game, so "the engine never knows content" means "never
a *game's* content", and restate the promotion rule to match ("foundations and the cosmos live in the engine; what a second game could
not use stays in its game"); give the terrain crate generators that take their layers as data (block, thickness; the superflat wish needs
the same thing), so that `stone`, `dirt` and `grass` leave it; put every policy constant behind a settings value a pack can fill, as the
sun and ship settings are (`INFLUENCE_PER_REACH` and `GREATEST_INFLUENCE` in `server_world.rs:140` are the next two); extract a cosmos module
only if a second cosmology appears. Confidence is medium on the last, because a minimal engine with a cosmos plugin is cleaner and I
cannot show it is worth the move now.

### F6. Games are lumps behind a static trait (confidence: medium-high)

`GameDefinition` is a trait of 24 static functions on a unit struct, with ten more on its companion `GameContent`; each feature adds one (`sun`, `level_detail`,
`grid_detail`, `level_gravity`...) [read, `definition.rs`], and each addition touches the other two games (14 lines each in the last).
The launcher lists the games at compile time [reported]. Cave's source is about 64 percent rules, 16 percent content and
configuration, 20 percent glue; every rule is a plugin object but all sit in one static list; the only data file is a 17-line planet
[reported]. `plugins/base_blocks` has one consumer, is not a plugin by the host's definition, and holds the river generator and the
planet family [measured, read].

So a game cannot be a pack, which means Cave cannot be the first mod, which means nothing forces the public surface to be enough for a
game. That is the strongest argument for the manifest in section 6 (A1, A8).

### F7. The hubs only grow, and the renderer is an option bag (confidence: high on the measurements, medium on where to cut)

Three types and one file are where most changes land. `ServerWorld` went from 177 to 4,545 lines in 13 days and was touched by 37 of
the 161 commits [measured]; it has 60 public methods, 38 of them about ships, grids, planets, rails and gravity, and 2,845 of its lines
are inline tests [reported]. `Session`, the client's, went from 259 to 1,374 lines in 36 commits [measured], with 47 public methods
and 28 fields [reported]. `VoxelRenderer` has 39 fields and about nine jobs: pipelines, a mesh store, avatars, boxes, the palette, the
overlay, the frame uniform, culling, planning shadow maps (about 450 lines), the 307-line `render`, and readback [reported].
`game_shell` has 22 direct dependencies, the most in the workspace, and its name fits about 39 percent of it: the rest is a client
runtime, content packaging and the sun [reported].

The renderer's options are the clearest case, and they are mine. `Atmosphere` had 4 fields on 2026-10-15 and has 13 [measured]; eight
arrived on 10-28 in four commits (`824bdd3`, `dd063c6`, `c456a08`, `4312b9c`). A parameter of the sun or the sky is copied by hand
through `Sun`, `SunView`, `lit_atmosphere`, `Atmosphere`, `FrameUniform`, `write_frame_uniform` and the shader's `Frame`: eight places in
two crates, and a ninth in the game's `sun()` [reported]. Forty-four of the renderer's 63 exports are used by no other crate, 41 of them
shadow, sky or sun items [reported]. Rust and the shader agree because text tests tie twelve constants together; nothing ties the layout of
the uniform to the shader except the GPU's own validation when it runs [reported]. This is what a feature costs when nothing gives it a
shape.

*What I would do* (one clean-up phase; mechanical, and covered by the tests): split `voxel_renderer.rs` along the seams its own comments
mark (passes, shadows, sky); hand the renderer the sun and sky as one record that it packs in one place; split `ServerWorld` into
modules by concern (entities, voxel access and the change log, ship lifecycle, gravity, rails and carrying, force queues) and move its
2,845 lines of inline tests into `tests/`; do the same for `Session`; and add a test that compares the shader's `Frame` to
`FrameUniform`, so that a new parameter costs fewer places. Twenty-five functions are longer than 120 lines and six longer than 200
(`load_with_translator` 347, `render` 307, `new` 268, `build_brick` 240, `finish` 225, `decode` 202) [reported]; their phases are mostly
labelled by comments, so splitting them is low risk.

### F8. Leftovers, duplicates and rules that drifted (confidence: medium)

- **Two lighting systems.** Cave has `Classic` and `MultiScale` light, switched with `/light` and saved in the world [read,
  `plugins/light.rs`]: about 1,430 lines of implementation (3,430 with their inline tests) and 73 of the 916 test seconds. The big file
  calls itself a spike. No entry in `design-decisions.md` says which survives. I do not know what role the owner intends for it, so this
  is a question, not a proposal to delete.
- **Words.** *Plugin* means a Rust object that registers rules, and also the namespace in a pack's `depends_on` (a content crate that
  implements no plugin). Pack, content package, game content and game overlap [reported].
- **Stale text.** `plan.md`'s Open list says "only players exist" (Phase 27 built entities), "the HUD is still the window title"
  (Phase 30) and "a daylight cycle is not built" (built 10-28) [read]. Cave's `lib.rs` still says "fly through caverns" [reported].
  `CLAUDE.md` lists Sculpt, which does not exist (2.6). The build plan lists phases out of order (39, 38, 37... 1f) and leaves out Phase 40
  and the sun.
- **Constants that must agree, with nothing to check them** [reported]: `FULL_LIGHT` (255) in `coarse_cells` and `voxel_meshing`;
  `LIGHT_YEAR` in `terrain` and in Cave; the terrain look-ahead distances in `physics` and `grid_bodies`; each client plugin's id
  written again as a string beside its server plugin's (four Cave pairs, one each in 2048 and Eels); 87 identifiers named in the
  documents, six of which are in no code. The shader constants, by contrast, are tied by text tests: where drift was thought of, it is
  caught.
- **Rules and practice.** `CLAUDE.md` forbids a test-only crate that depends on the crate under test, and `engine/end_to_end_tests`
  is one (613 lines) [reported]. "No technical debt" holds in that no TODO or FIXME comment exists; 41 softer markers ("not yet", "is
  open", "for now") and the Open list in `plan.md` carry the debt in prose [reported]. I would write "no *unrecorded* debt" and keep one
  short list.
- **A hole in the determinism lint.** `clippy.toml` bans the platform-dependent float functions but not `sin_cos`, `exp_m1`, `ln_1p`,
  `asinh`, `acosh` and `atanh`. Nothing calls them today [measured].
- **Strays.** `testworld` and `testworld.ships/` sit untracked in the repository root [measured]; two other checkouts live in
  `.claude/worktrees` [reported].

### F9. The record has outgrown its use as a map (confidence: high)

983 KB of documents. `CLAUDE.md` sends every architectural task to `design-decisions.md` (94 KB, about 24,000 tokens), which holds
standing decisions and phase diaries in one list; a single bullet on docking carries over a hundred words of constants.
`plan.md` (56 KB) holds the build plan, proposals, open questions, instructions for the owner to try a build, and the post-mortems
of flaky tests. Orientation for me is `CLAUDE.md` plus the design record, plan and goals: about 200 KB, or 50,000 tokens, before any
code [measured sizes]. For a person that is roughly two to three hours of reading [belief]. There is no README, no one-page
architecture and no statement of where the project stands.

The record is my only memory across sessions. When it is too large to load whole I work from summaries, and summaries drift. It is
also the owner's only instrument for steering what they cannot read in code. A map the owner can hold in their head is worth more
than another specification.

### F10. The gates are manual and the front door is shut (confidence: high; easy)

There is no continuous integration. Before every commit someone runs the Windows suite (916 seconds today) and a copy in WSL by hand.
"Bit-identical on Windows and Linux" is therefore true whenever someone ran both, not on every push. There is no README, licence,
release or installer, so a friend needs a Rust toolchain and a recent stable compiler; the cold build itself is cheap, 118 seconds, so the
barrier is the toolchain and not the wait. `target/` is 61 GB.

One more reason the gate matters: the GPU tests, which hold the Rust and shader maths together, pass *vacuously* when no graphics
adapter exists unless `VOX_REQUIRE_GPU` is set (`gpu_rendering.rs:50`) [reported]. A machine with no adapter, and any naive CI, reports
green for tests that never ran.

*What I would do* (under a day): a workflow that runs format, lints and tests on Windows and Linux for every push, with
`VOX_REQUIRE_GPU=1` and a software adapter (WARP on Windows, a software Vulkan driver on Linux) so that the GPU tests run for real
where their pictures do not depend on the adapter, plus an ARM Linux job once, because determinism has been verified on x86-64 only; a release workflow that builds a zip; a README with a 60-second start; and a note in `CLAUDE.md` on how to prune `target/`.

### F11. The server has no robustness net for code it did not write (confidence: medium-high; read, not run)

- `serve` loops on a stop flag and never looks at the server thread [read, `serve.rs:46`]. If a rule panics, nothing notices; on Ctrl-C
  `ServerThread::stop` unwraps the join (`host.rs:333`) and panics before `save_world` (`serve.rs:51`), so what is on disk is the last
  autosave. I did not execute this, but I read both lines.
- An infinite loop in a rule hangs the tick thread; the only limit is 4,096 events a tick [reported].
- A client discards the report of what it could not load (`_report`, `session.rs:207`) [reported].
- The hard rule says "no wall-clock time in simulation or client cosmetics". `PoseFollower` uses `Instant` to smooth the view
  [read, `following.rs`]. The practice is right (presentation smoothing, never recorded or compared), and the rule's wording is
  wrong. A rule that is broken in letter by correct code gets ignored in spirit; say what it means, and then it can be checked.

### F12. Speculative generality has a standing cost (confidence: medium; a judgement)

The design record is candid about the price. "Any new per-voxel loop must do the same: check once, then work in 64 bits." Squares of
distances "overflow `i128` across the range and are compared exactly after a check per axis, or saturate." Levels 41 to 116 of the octree
"are checked with synthetic fields and a planet scaled up in the tests, not seen in play." A coordinate on the wire is a
length-prefixed integer so that 256- and 512-bit coordinates "need only a reader that accepts a longer length", for a future that
depends on shrinking ships. Orbits are exact to 2^-33 voxel at a radius of 2^120. A river is a billion light years wide so that the
levels of detail have something to draw at every scale.

These work, they are tested, and the river is a fair proof of the levels of detail (it is drawn from 10^25 voxels away). Removing
them would cost more than keeping them, so I do not recommend removing anything. The cost is that each is a rule every future author
of a loop, a distance, a wire field or a test must remember, and that they have become the project's centre of gravity while a new
player's first hour has no sound and no goal. I would *freeze* them: no new work whose only argument is the size of the universe unless
a scene in the first-flight milestone needs it, and a sentence in `CLAUDE.md` saying which range the game targets (a solar system) and
which the engine tolerates.

## 5. Moddability, honestly

### 5.1 Where we stand

Section 2.5 is the short answer: three things are moddable by someone with a text editor, and the pack tooling that exists is better
than most. The distance to "mod in anything" is not in the foundation. It is in breadth (F3), in who may run code and where (F2, F4),
and in everything around the data: an editor, a reload, assets, a way to share.

### 5.2 Hytale, as it has shipped (2026-10-29)

The investigator's sources are public pages. I read the two that carry the most weight myself (marked *checked*); the rest is reported
and its quotations come through a summariser, so treat wording as approximate.

- **Status.** Early Access since 13 January 2026; Update 7 on 12 October [reported].
- **What it says it is doing** *(checked: Hypixel's post of 20 November 2025)*. All modding is "based on the host of the server or host
  of the game", and single-player runs "a local server that is just for yourself". "We don't intend to support any client mods."
  Blocks, items, NPCs, world generation and drops are JSON data assets. Server plugins are Java `.jar` files and "come with safety
  concerns". "We do not have text-based scripting and we do not intend to add it"; visual scripting is "a core pillar" with no date.
  "We cannot guarantee data integrity." The Asset Editor has "notable limitations around NPCs, world generation, and interactions."
- **What has shipped** [reported]. A JSON asset-pack format and Java 25 plugins on an entity-component server; an in-game Asset Editor
  that validates and reloads on save; node editors for world generation and NPCs; a Blockbench plugin; assets that stream to a client
  on joining (one asset capped at 256 MiB); an in-game mod browser through CurseForge since Update 6, with no paid mods; more than 5,000
  mods and 20 million downloads by early March 2026 (confounded by an audience in the millions). Visual scripting: nothing found.
- **What has not** [reported]. No sandbox for plugins ("full Java access" in community summaries of developer Q and A); no published
  API stability policy, a protocol bump in Update 6 that forced plugins to be rebuilt, and an Update 8 pre-release that stops loading
  the old world-generation JSON, so data packs break too; a client image-decoder flaw that a hostile server could use, patched in July.
- **The source** *(checked: the Shared Source Licence, version 2.0, effective 2026-07-21)*. Access is by private repository for account
  holders. Reading is allowed "to develop plugins, tools, and documentation". Section 3.3 excludes use "to develop, market, or operate
  any product or service that directly or indirectly competes with the Game" and "to perform competitive analysis". **We should not read
  Hytale's server source.** We are a voxel game; a reading that lands in our design would be hard to defend. This is not legal advice.
  The public posts, the community documents and the modding pages are enough for what this review needs.

### 5.3 Are we following Hytale's footsteps?

In architecture, yes. In execution, a long way behind. That is the right place to be.

| Hytale | Voxunami today | |
|---|---|---|
| The server owns content; single-player is a local server | The same: an integrated server, a client that sees only the stream | Match |
| JSON data for blocks, items, NPCs, world generation, drops | Blocks, patches, planets | The same kind, about a fifth of the breadth |
| No client mods | Client plugins are compiled in; the server cannot send code; the rule is not written down | Match in effect |
| Assets stream to a client on joining | A package of block definitions; no assets | Partial |
| An editor that validates and reloads on save | A generated schema and `check-pack` (every error, a "did you mean"); no editor, no reload | Our validator is ahead; the rest is absent |
| Java plugins with full access | Rust plugins compiled in, with full access | The same risk. WASM is where we can differ |
| Visual scripting as a pillar | Nothing | We should not promise it |
| A mod browser | Nothing | After a stranger has made a mod |
| A stability policy | None | Neither has one; ours can come first |
| Source behind a licence | Ours | Decision 10 |

What I would copy: the *shipped* parts, which are the data layer, the editor-with-validation and reload, and streaming content to the
client. Hytale's own words suggest that is what produced its mods ("assets trivially easy, plugins harder", from two modders in the
investigator's sources; and Hypixel's admission in November 2025 of weak editor coverage and documentation that caused "serious
friction"). What I would not copy: unsandboxed code, a roadmap pillar nobody has shipped, no stability policy, and a team of a hundred.
Where we can be better than Hytale: a sandbox that falls out of the determinism we already have, a saved world that survives its mods
being taken away (Hytale says it "cannot guarantee data integrity"; ours is tested), a replay of everything that happened, and a
validator that is already good.

### 5.4 The others, one line each

[reported, from the research investigator; I did not check these pages]

| Game | Layers | The decision that mattered most | A failure worth knowing |
|---|---|---|---|
| Factorio | A data stage builds tables, then an event-driven Lua stage; lockstep, inputs only; mod checksums must match | The sandbox *falls out of determinism*: no `io` or `os`, custom trigonometry, a fixed order for `pairs` | 2.0 forced wholesale ports; the biggest overhaul mod's 2.0 line is still alpha |
| Vintage Story | JSON content, C# code mods, theme packs | A written compatibility policy with freeze stages; JSON mods "likely safe across several major updates" | No incident found; its policy page lags a version |
| Minecraft | JSON data packs on the server; Fabric and NeoForge Java mods | Obfuscated code for a decade; removal announced October 2025 | Fractureiser (2023); a 2026 modpack's overrides folder carried an infostealer |
| Space Engineers | XML data; C# mod and Programmable Block scripts compiled under API whitelists | The whitelist makes player-written code safe enough to distribute | SE2 so far has asset-only modding; scripting "not included yet" |
| Starbound | JSON assets, `.patch` files, Lua behaviours | Patch layering: many mods edit one file | "Could not apply patch" when two mods target the same place |
| Luanti | Server-side Lua; definitions and media transferred to clients by hash | The nearest relative of our design and Hytale's | A critical sandbox escape (CVE-2026-40959) in 2026 |
| Kerbal Space Program | `.cfg` parts plus unsandboxed C# DLLs; ModuleManager patches | Community tools filled the missing official API | KSP2's studio closed in 2024 |
| RimWorld | XML defs, XPath patches, C# with Harmony | Open managed code: breadth, at the price of conflicts | Multiplayer is a volunteer patch set: determinism retrofitted is costly |
| Roblox, Teardown | A Lua sandbox behind server authority | A sandbox you can share code through | Free-model backdoors through `require(assetId)` |

### 5.5 What they teach

1. **A data layer with an editor and a fast reload is what grows a scene** (Hytale's volume; Minecraft's slower loop is the contrast).
   Our validator is half of that. Reload and an editor-quality loop are the other half (A6).
2. **Churn is the best-documented killer.** Fabric's largest change, Hytale's protocol and world-generation breaks, toolchain churn.
   A written stability policy or a declared end state (Vintage Story's stages, Factorio's "last major update") earns authors' trust. We
   should announce stability *tiers* at release-0: pack format and wire protocol versioned with migrations (a pack ships small upgraders
   keyed by its format, as Factorio's migrations and Minecraft's format ranges do); the host API a WASM module sees versioned;
   the Rust plugin API internal and free to change.
3. **Safety has three shapes.** No client code (Hytale), a sandbox (Factorio, Luau, Space Engineers' whitelist, Luanti), or nothing
   (Minecraft, KSP, RimWorld), and "nothing" fails loudest. Sandboxes fail too: Luanti's escape, Luau "not formally proven",
   Roblox's backdoors. So: no client code, a WASM sandbox with a narrow host API, and assets that are hash-addressed, size-capped and
   decoded by fuzzed decoders (Hytale patched a client image flaw that a hostile server could exploit).
4. **Determinism is what makes a sandbox cheap.** Factorio's restrictions are a by-product of lockstep. We already ban the
   platform-dependent maths, order everything, and pin the result on two systems; a mod-list checksum in the handshake is the missing piece.
5. **Retrofitting multiplayer or determinism onto a mod ecosystem is the expensive path** (RimWorld, Teardown). We started with it. That
   is an advantage worth protecting.
6. **Scope is a plausible cause of the projects that failed** (KSP2, Dual Universe, Starbase) [the investigator's inference]. Own one
   differentiator before breadth.

## 6. Other ways to do it, and what I would pick

You asked me to imagine other designs. These are the axes where another design is plausible. For each: what we do now, the
alternatives, my pick, and what would change my mind. *One-way* means hard to reverse once anyone depends on it (9.1).

### A1. One unit of content: the pack (one-way; confidence: high)

Now there are five words for overlapping things (*plugin*, *content crate*, *pack*, *content package*, *game*), a Rust path for the
first-party game and a second-class data path for everyone else. Pick three words, used everywhere:

- a **pack** is a folder of data and assets with a manifest (`id`, `format`, `version`, `requires`, `depends_on`);
- a **module** is code that registers rules: in-tree Rust now, WASM later, shipped in or beside a pack;
- a **game** is a manifest (data) naming the packs and modules a world starts from, and its levels and their rules.

First-party Cave is packs and modules; so are 2048 and Eels; `base_blocks` becomes a pack plus the Rust generators it truly needs;
"plugin" retires. That makes Cave the first mod, so the public surface has to be enough for a game, and it removes the second-class
data path. The alternatives are to keep the Rust trait and add a loader beside it (two ways to do everything, for ever) or to keep a
game a Rust crate that links packs (today). What would change my mind: a manifest too weak to set a game up. A game may then keep a
small optional Rust shim.

### A2. A thin client (one-way; high)

Package v2 as in F2. No mod code ever runs on a client; client behaviour is a closed set of widgets (hotbar, bars, text, lists,
buttons) configured by data, and a new widget is an engine change. Alternatives: client-side mods (Forge and Fabric style: rich,
fragile, unsafe, and they break replay); streaming everything (loses "terrain nobody edited is never sent"); the game compiled into
both ends (today). What would change my mind: a need no widget set can express and that cannot wait for a release, such as a modder's
own mini-game HUD. Then a sandboxed *interface* script, not game code, could be considered.

### A3. How a mod expresses behaviour (one-way for the vocabulary; high on the order, medium on the runtime)

In this order:

1. **Properties a pack declares** and a **closed interaction vocabulary** (F3). Most mods end here.
2. **Rule tables for neighbour-driven change** (spread, decay, growth, flow). Fire, the lamp's light and the planned water are each
   a rule with one shape: snapshot in, edits out, chance from `voxel_random`, a budget. The `lattice` crate has now been through three
   rules; a table format is the extraction the 2026-10-17 plan was waiting for.
3. **WASM modules** for what data cannot say: server only, fuel-metered so that a budget is a count of instructions and never a
   time (the rule the design record already states), deterministic by the language's definition (apart from the bit pattern of a NaN,
   which a host can canonicalise), one file on every platform. Start with
   an interpreter (the `wasmi` class: pure Rust, no JIT, deterministic fuel). For rules, correctness and safety beat speed. Use a JIT
   only for a generator, and only if a measurement asks.

I would **cut the native C-ABI loader** from Phase 33. It cannot be sandboxed and cannot promise the same floating-point results on
every machine (the plan says so itself), and the trusted tier already exists as in-tree crates. That halves Phase 33.

The serious alternative is **Lua**, which is what Factorio, Luanti, Starbound, Garry's Mod and Roblox use. Its authoring is the
easiest and its precedent the strongest. But stock Lua's maths goes through the host's library, and the order of a table's keys is
not fixed unless the VM is patched (Factorio patches it), so unpatched it breaks "bit-identical" and replay. I would pick WASM at
about 60/40. What would change that: in-game ship computers, where a player types a script into a block (Space Engineers'
programmable block, kOS), want a language you can edit in the window, and a deterministic Lua as the single guest language would
beat WASM. It would be one or the other, not both.

### A4. Assets and art direction (one-way; the owner's taste; medium)

Now: flat colours from a palette of 1,048,576, boxes in eighths of a voxel, no textures, no models, no sound. The first design said
"textured blocks near the player". I would pick **sub-voxel models with a palette**: a block, an item or a ship part is a tiny grid;
no textures; sounds are named events with sample files; everything is addressed by hash in the package; `.vox` files import. Ships,
planets and parts are voxel grids already, the levels of detail already coarsen a grid at any distance, and a modder using MagicaVoxel
needs no UV tool. Assets sent by a server are an attack surface (Hytale patched an image-decoder flaw that a hostile server could use),
so cap each asset's size, decode with fuzzed decoders, and never let an asset name a path. A texture pipeline (atlas, mips, per-face
UVs, LOD for textures) is a large separate system. Textures are what Minecraft, Vintage Story and Hytale's hybrid use, and they are
familiar to modders and players. This is yours because it decides how the game looks, the package format, the renderer and the tools.

### A5. When mods meet (two-way; high)

The current rule is good: dependency order, ties by id, a pack may patch only what it depends on, `test` in a patch, `optional`, all
errors at once. Add a warning naming both packs when two set the same property of the same block unless one declares `after` the other;
a manifest with `format`, `version` and `requires`; the pack list with hashes in the handshake (clients need no packs, but
recordings and diagnostics do); and, from release-0, migrations: a pack ships small upgraders keyed by its format (5.5).

### A6. Iteration speed for modders (two-way; high)

`/reload` (run pack loading again, resend the package; blocks keep their numbers by name); `voxunami new-pack`; one tested example
per content kind; doc examples run as tests; `check-pack` that rejects a folder it does not read; a development mode with `/give` and
`/setblock`; and a hotbar that pages. Factorio's and Minecraft's ecosystems grew on the edit-and-run loop. Ours is 3 seconds for a
developer and a server restart for a modder.

### A7. Adding a feature: a rubric (two-way; high)

Ask in this order. Is it content? A pack. Is it a rule? A module on the registrar. Is it a mechanism no module can express? The
engine, with a one-page memo. Is it presentation? A renderer parameter carried in the package.

`/tp` was one crate and 609 lines: boxed. The sun was a renderer mechanism (the sky pass), a policy (the model of a day) and data
(Cave's sun). It touched three crates because the policy sits in the shell and the data in a Rust function; as a rule record in the
package it would be a mechanism in one crate and a data file. A thruster would be a pack (a block with `thrust`, `fuel_burn` and its
facing as properties), one module (apply a force at the block along its orientation), and a HUD from the server-driven UI, with no
engine edit. Track crates touched per feature: the median of the vertical ones is about eight; the aim is three.

### A8. How a game is defined (one-way; medium-high)

A data manifest with an optional Rust shim, as in A1. It is also what lets one world run Cave's rules, the superflat level on the
wish list, or a mod's own game, with no new Rust.

### Rejected, with reasons

- **An entity-component system.** Voxels, not entities, are most of our state; a second data model beside grids buys little.
  (The review of 2026-10-17 said the same.)
- **Mods in a separate process.** The best isolation, at a high price in latency and in deterministic order. WASM in process gets
  most of it.
- **An event-sourced server.** Replays already are the stream; making the server's state a fold over it costs memory and
  complicates saves.
- **Client-side mods, in any form** (A2). **A scripting language of our own.** **A mod marketplace**, until a stranger has made one mod.

## 7. The game: from here to the best blocky spaceflight simulator

### 7.1 The shortest path to a first flight loop

Adapted from the product investigator's list, with changes. Sizes are beliefs.

1. **A destination.** Load the moon and the star by default; the data and tests exist (`examples/packs/solar_system`). Then check
   frames and cost at those distances; nobody has.
2. **Engines as blocks.** Thrust, fuel burn and facing as properties; one module applies a force at the block, along its
   orientation, every tick. No gravity cancel and no speed cap. `apply_grid_force` already takes a point. This replaces `pilot_force`.
3. **Attitude.** Keys to torque (`apply_grid_torque` exists and nothing calls it), and reaction wheels as blocks with a torque property.
4. **A seat and a flight HUD.** The seat hands a pilot's controls to a ship; the HUD (speed, altitude, fuel, an apoapsis estimate) is
   server-driven UI (`Node::Bar` exists and no game uses it); a chase view and a first-person view.
5. **Finite fuel.** A per-ship store (a component) and tanks as blocks with a capacity property; creative refill first, a source
   later. The design record lists block data as unbuilt, so a per-ship component is the smaller step.
6. **Stakes.** Blocks break on a hard impact; a respawn; the first goals as a checklist a pack declares ("reach orbit", "land on the
   moon").

Not yet: ships on rails, time warp, an economy, mobs, a tutorial. A lap of the default planet is about 8.4 minutes at about
800 metres a second [belief, from a radius of 63,710 voxels and 10 m/s²], so the first loop needs no time warp. The example moon, 1.5
million metres out, would want one soon after.

First, before building on it, an **orbit-stability test**. No test covers a free ship in orbit [reported]. A ship at orbital speed for a
hundred laps should neither decay nor gain energy at 20 ticks a second with the solver we have. If it does, that is the first thing to
learn, and the cheapest.

I expect steps 2 to 4 together to be of the order of one recent sky commit, not several [belief]. The product investigator's verdict
was the same in different words: the engine is ahead of the game, so the risk is product and design, not physics.

### 7.2 What would make it the best

The pitch, as a test for every feature: **one continuous world in which you walk the corridors of your own ship, launch it from the
ground of a planet whose mountains you can dig, dock it into a base you built on a moon, and fly on to a star, with nothing
loading, on a server others can join with nothing installed.** We have the foundation for each clause; the sentence is the spine of M1.

The field, from the research investigator [reported; none of it checked by me, and several sources are weak]: Space Engineers has
walkable ships and voxel planets, gravity wells rather than orbits, and co-op for its sequel slipped to early 2027; Kerbal Space Program
has orbits and no walking inside and no multiplayer, and its sequel's studio closed; Dual Universe's servers closed in August 2025;
Starbase is thinly populated. Two newer entries claim adjacent ground: PlanetSmith promises voxel worlds "without a break from orbit
down to the ground" for a hundred players, and funded a small Kickstarter in hours (claims unverified), and Cubic Odyssey shipped
co-op in May 2025. So the niche is not empty, and it is not crowded either. The investigator's guess at the gap is **cooperative,
moddable spaceflight with real orbits and a time warp that everyone shares** (moderate confidence that the gap exists, low that
demand supports a team). That fits us unusually well: a body's place is a pure function of the world's clock, so a shared warp is
a clock everyone agrees to run faster, and a coasting ship can be on rails during it. It is not built: ships on rails are the
engine's real unknown (7.3). It is the candidate for the one differentiator to own (decision 11), and it is a reason to build the
first loop on a *server* others can join, not only on a single-player window.

What the leaders have that we do not, in the order a player notices: sound; clear flight feedback and instruments; a good
building experience (symmetry, copy and paste, blueprints); a way to plan a transfer and to time-warp; functional blocks and
automation; a workshop where a mod is one click; a reason to go anywhere.

What not to copy is the cost trouble big ships cause elsewhere. We start in a good place: the project's own measurements are 0.011 ms
a tick awake for a ship of 10,164 blocks and 0.024 ms for 101,614 [reported from `moving-grids-spec.md`; their measurement, not mine].
Keep a ship one collider.

### 7.3 Risks specific to flight

- **Feel.** Input goes to a server that ticks at 20 Hz, moves the ship with a force and sends the result back, and the view
  extrapolates it (`PoseFollower`). On 2026-10-16 the owner reported walking and falling as jerky; prediction was deferred and is
  still listed as open [read, `plan.md:385`]. A pilot turning a ship will feel a tick of delay and then the follower's smoothing. I
  cannot judge feel; you can. Cheap experiments: a higher tick rate for Cave (it is set per game); the same flight with the follower off.
  The real fix is prediction for the local player and the piloted ship.
- **Orbit accuracy** at 20 ticks a second (above).
- **The size of a ship** against the 4,096-block limit of `/ship create` and the collider budget.
- **Ships far from players.** A free ship freezes 256 metres from every player [reported], and a ship in orbit is far from everyone.
  Ships on rails are the real engine unknown for interplanetary play. After the first loop.

## 8. What we may be too close to see

I would rather say these plainly once than have you find them later.

1. **I choose depth, and "continue" lets me.** Where tests give crisp feedback (determinism, scale, formats, rendering) I can go
   deeper for ever, and I do, with pleasure and rigour: 45 mutants and four GPUs for a sky. Where feedback is vague (does flying feel
   good, is the first minute welcoming), work sinks in the queue. Left to choose, I keep choosing the first kind. The last five days
   were the universe, a river, places, the sun, shadows, stars. Each was good engineering. None moved a player closer to a flight.
2. **No one else has touched it.** One person reads the design and one model writes the code. "Easy to mod" and "easy to maintain" are
   unfalsified until a second person tries. A friend playing for ten minutes, a modder writing one pack, a Rust developer reading one
   crate: each teaches more than another phase.
3. **Tests prove I did what I meant.** 88,000 lines check my intent against my code. Nothing checks whether it is good. The test
   that matters most, you playing, is the one nobody schedules. The feel questions I put to you on 10-21 and 10-22 (thrust and drag,
   the docking defaults) have no recorded answers [read, `plan.md`; the product investigator found the same].
4. **Speculative generality (F12)** is the centre of gravity of the engine and a magnet for new wishes. A project that can draw a
   river a billion light years wide can mistake that for the product.
5. **The record is a place to put things down.** A megabyte nobody can hold; you cannot steer by it and I re-read it at a cost
   every session.
6. **"Mine, the owner's to overrule" is a moral hazard.** About 18 entries in `design-decisions.md` carry it, beside about 24
   owner-dated decisions [measured; a rough count]. You cannot overrule what you have not read, and the entries are long. So a
   number of consequential choices are mine by default and yours in name.
7. **The first minute.** A flat green hill and a hotbar; no sound; no instruction; hexadecimal coordinates; a first click that only
   captures the mouse (2.6; I looked at the screenshots in `target/shots/`). A friend would leave before reaching anything the engine
   does well. Hexadecimal is your choice and a fine one for us, and a barrier for a newcomer; a development mode and a friendly
   default would serve both.
8. **Bus factor.** The builder is a model and the reader is one person. If either is unavailable for a month the other cannot carry
   on alone. The cure is cheap: a one-page architecture and a state document, so that a new model or a new person starts in an hour.
9. **Which game is the product.** Cave is the space game; 2048 and Eels are finished toys (6.6 percent of the lines, unchanged in
   design since 10-17); Sculpt is an idea. Saying which is the product frees the others to be fixtures.

## 9. How we work

### 9.1 Decisions: two kinds of door

A **one-way door** is expensive to reverse once anyone depends on it: a file format or wire layout, the coordinate model, the mod
surface and its manifest, the asset pipeline, the licence and distribution, the flagship game. A **two-way door** is everything else:
a constant, a layout, a shader, an internal API, a name.

- One-way: I write a one-page memo (the question, two or three real alternatives, my pick, the cost of being wrong, what would
  change my mind), you decide in that session, and the decision goes into a short entry (title, door, status, date, who). I do not
  build past a one-way door you have not decided.
- Two-way: I decide, build, and log it in a line. You may overrule at any time, cheaply.
- "Continue" means "carry on with the plan as written". When the plan has no next item, I ask. Better: "continue with X; Y is out of
  scope."

Today the record mixes both kinds in long bullets. A table of a dozen one-way doors is easier to hold than 94 KB.

### 9.2 Priorities: one outcome at a time

Direct the work by what a person can do at the end, not by the technique. "A player can fly from the planet to the moon and back" is an
outcome; "add thrusters" is a step toward it. One milestone is open at a time. Each has a **gate** (something you do yourself, in the
real window, for twenty minutes) and a **stop list** (what we are not doing). Your pause on graphics was the right move; it cost
nothing and bought this review. Make *stop* a regular tool, and write the stopped thing in a parking lot with the trigger that would
bring it back, so that stopping is not forgetting.

### 9.3 Rigour in proportion

The rigour here is a real strength and I do not want to lose it. It is not free: the full gate is 15 minutes on one system, and the sky
alone took 14 GPU tests and 45 mutants.

| Tier | What | Rigour |
|---|---|---|
| 1 | Formats, wire, saves, determinism, the mod boundary: whatever a world or a mod depends on | Tests first, property tests, mutation experiments, both systems, a memo for a one-way door |
| 2 | Rules, physics, the world model, the server | Tests first, the pinned sessions, both systems |
| 3 | Presentation, tuning, cosmetics, tools | A test that it exists and does not break the picture, a screenshot, you looking at it |

Mutation experiments, extra adapters and golden pins belong in tier 1. The sky was tier 3 and was done at tier 1.

### 9.4 You as manager: eight habits

1. Say the outcome first: "By Friday a friend can fly to the moon." I find the shortest honest path and say what it leaves out.
2. Play for twenty minutes a week in the real window and write five lines: what felt good, what annoyed you, what you did not
   understand. They go to the top of the backlog. The open feel questions are older than this review.
3. Replace "continue" with "continue with X". If you want me to choose, say "choose".
4. Ask me for a cost before I build a wish: lines, crates, permanent rules, what it blocks. I build quickly and I under-weigh what I
   leave behind.
5. Decide one-way doors on the memo, quickly, and leave the rest to me.
6. Ask for subtraction. Every third phase, one that only deletes, merges or splits. Churn is 9.5 percent and no phase has been a
   clean-up.
7. Get a second person in: a friend to play, later one modder to write a pack. Ask them what confused them, and do not explain.
8. Keep the ability to say no. Each wish is delightful. The discipline that makes room for the next one is the unglamorous one.

### 9.5 Me as coworker: what I will do

- Every proposal states the outcome, the cost, what it blocks, and what would make us remove it.
- I flag one-way doors, stop, and wait.
- I keep a one-page state document (not written: the map in `docs/README.md` is the nearest thing) (where we are, what is next, what waits on you) and rewrite it at the end of each phase, so a
  new session starts in minutes.
- I scale rigour by tier and say which tier I am in.
- I tell you once, briefly, when I think a request is a detour, then do what you decide.
- I separate verified from believed in every report, as I have tried to do here.

### 9.6 The record

README (one screen: what it is, a picture, how to run it, how to make a first pack); ARCHITECTURE (two pages: the layers, the three
words, the hard rules, where each concern lives, a diagram); MODDING (the first pack in ten minutes, every example run as a test);
STATE (this week); decisions as short entries with the one-way doors in a table; phase diaries moved to `build-log.md`; `plan.md`
holding the milestones and the parking lot and nothing else; and a sweep of stale statements now (F8).

### 9.7 What to watch: a small dashboard

| Measure | Now | Aim |
|---|---|---|
| Crates touched by a vertical feature (median) | about 8 | 3 |
| Edit to running, in a plugin | 3 s | stays |
| Full gate, one system | 916 s | under 600 (give the GPU file its own job) |
| Things a pack can define | 3 | 10 |
| A new person's first pack | about 10 min, with one wrong example | 10 min, none wrong |
| Calls to `Context::world()` in modules | 53 | falls, never rises |
| Orientation cost for a new session | about 50,000 tokens | under 10,000 |
| Minutes the owner plays each week | not recorded | 20 |

## 10. A sequence I would propose

Milestones are outcomes. Each has a gate and a stop list. Sizes are beliefs, in phases.

**M0. Housekeeping (a few days; all two-way doors; I would start on your word).** CI on two systems; README, ARCHITECTURE, STATE; sweep
the stale statements; fix the pack example; make `check-pack` reject a folder it does not read; `format` and `version` in `pack.json`;
supervise the server thread and save on a panic; an architecture test that reads `cargo metadata` and fails on an upward edge, one
that fails on `Instant` or `SystemTime` in simulation crates (after the rule is reworded), and the doc-example test; move the inline
tests out of `server_world.rs`; add the missing float functions to `clippy.toml`; ignore `testworld*`; drop Sculpt from `CLAUDE.md` or
mark it an idea. *Gate:* a person who has never seen it builds and runs it from the README.

**M1. First Flight (3 to 4 phases).** Section 7. Parts as properties, one module, no engine edit if I can help it: that makes M1 the
first real test of the mod surface. *Gate:* you fly from the pole of the planet to the moon and back in one sitting and can say what
you would change. *Stop list:* sky, shadows, rivers, universe-scale work, 2048 and Eels features, the multiscale light.

**M2. A mod in ten minutes (3 to 4 phases; two memos first).** Package v2 and the handshake fingerprint (F2); pack-declared properties,
items, recipes, drops and interactions (F3); `/reload`; `new-pack`; tested examples; the base game as a pack; the WASM spike (F4),
with a decision. *Gate:* someone who is not you writes a pack with a new block, a recipe and a new engine part, from the documents
alone, and a second player joins your server and sees it with nothing installed.

**M3. A friend can play (2 to 3 phases).** Sound (an audio crate, sound events in the package), a pause and settings menu, key
rebinding, a first-minute tutorial, a coordinate display a stranger can read, a release build and a README. *Gate:* a friend, no help,
ten minutes, and what they say written down.

**M4. Code mods (3 or more phases).** The WASM host if the spike says go: a narrow host API, fuel budgets, panic containment, a mod
list in the handshake, a first module that is not ours. *Gate:* the hostile-mod tests pass and a third party's module runs.

**Parking lot, with triggers.** Ships on rails and a shared time warp (after M3: it is the candidate differentiator of 7.2, and the
engine's real unknown); the sky's gradient and a moon (when a scene needs one); branching rivers (only if a scene needs them);
learning NPCs and the octopus (after M4); Sculpt (an idea); a mod marketplace (after a stranger has passed the M2 gate).

## 11. Decisions I need from you

| # | Decision | My pick | Confidence | If decided late |
|---|---|---|---|---|
| 1 | Which game is the product | Cave. 2048 and Eels become conformance fixtures (kept compiling and tested, no new features). Sculpt struck from `CLAUDE.md` | High | Feature work spent on toys |
| 2 | A thin client: package v2, and "no mod code on a client" | Yes (A2) | High | Each new feature adds another compiled-in function |
| 3 | The words and units: pack, module, game manifest | Yes (A1, A8) | High | Cave cannot become a mod |
| 4 | The code-mod runtime | WASM, server only, after a spike; cut the native loader | Medium-high on the order and on cutting native; medium on WASM over Lua | In-tree plugins keep calling `world()` |
| 5 | Art direction and assets | Sub-voxel models with a palette, no textures, sounds as events | Medium; your taste | The package format sets before assets exist |
| 6 | The pack manifest: `format`, `version`, `requires` | Yes, now | High | Packs in the wild with no format |
| 7 | What the engine is | The engine of a space game, policy behind settings a pack can fill (F5) | Medium | The cosmos grows into the engine |
| 8 | The second light system | Your call; I do not know its intended role | Low | About 1,430 lines (3,430 with tests) and 73 test seconds carried |
| 9 | When formats freeze (release-0) | At the M3 gate | Medium | Boldness has an end; choose the date |
| 10 | Licence, distribution, and where mods are shared | Decide before M3. Meanwhile: do not read Hytale's gated server source | n/a | Shapes the README and the manifest |
| 11 | The one differentiator to own | Cooperative orbital flight on a moddable server, with a time warp everyone shares (7.2) | Medium | Scope spreads across everything the genre has tried |

## Appendix A. Evidence and method

All figures are for commit `4312b9c`, Windows 10, a warm build directory, 2026-10-29. Scripts live in the session's scratchpad and
can be rerun; the ones that matter are described here.

- **Size, layering, top files.** A Python script walks `cargo metadata --no-deps --offline`, counts non-blank non-comment lines by
  crate (source, tests, shaders; a source file's `#[cfg(test)]` tail counts as tests), and checks every normal and dev dependency edge
  against the order engine, plugins, games, launcher.
- **History.** A script over `git log --numstat` sums lines added and removed by crate and by kind, by day, and by the domain a
  crate is for.
- **Feature reach.** `git show --numstat` for eight representative commits.
- **The loop.** A script touched a source file in each of four crates and ran `cargo build -p launcher` after each, then
  `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace --no-fail-fast` with
  `VOX_REQUIRE_GPU=1`, with a timestamp around each. Touching a file changes its time and nothing else.
- **The modder's trial.** A pack of four files, run through a copy of the already-built `voxunami` executable, so the build lock was
  free for other work: acceptance, then five failure cases.
- **Searches.** `Cargo.lock` for audio, loader and scripting crates (none of the common ones present); `apply_grid_torque` and
  `apply_grid_force` calls in `games/`; `context.world()` calls in `games/` and `engine/`; `G::` calls in `game_shell`; reload code.
- **Investigators.** Four, separate, none shown my conclusions: the modding surface (it ran `check-pack` and `pack-schema` on scratch
  packs), layering and cohesion, what a player can do, outside research. I spot-checked the sharpest claims; section labels say which.
- **Cold build.** `cargo build -p launcher --offline` into an empty `CARGO_TARGET_DIR`, timed; the directory was deleted afterwards.
  The time excludes downloading dependencies.
- **Not done.** The Linux run; multiplayer; any use of the game in a window; GPU tests on adapters other than the default; reading all
  of the 983 KB of documents (I read `design-decisions.md`, `plan.md`, the earlier review, `pack-format.md` and parts of
  `future-goals.md`).

## Appendix B. Where I may be wrong

- **I cannot play.** Everything I say about feel, about the first minute, and about what a friend would do comes from code,
  screenshots and an investigator's reading. Treat it as a hypothesis for your twenty minutes.
- **Some findings rest on investigators**, who read and in places ran things for me. I checked the ones I leaned on hardest and
  marked them; the rest are theirs.
- **I looked for problems.** A review biases toward faults. Section 3 is as long as I could make it honestly.
- **Intent outside the documents.** Your plans for the game may be bigger or different from what is written. If so, parts of
  section 7 are wrong and I would like to be told.
- **My recommendations carry my habits.** I like data formats, schemas and tests; a different lead might weigh a playable
  prototype ahead of all of them, and could be right.
- **Windows only, one machine.** Nothing here was measured on Linux, on another GPU, or under multiplayer load.

## Appendix C. Wording I would propose for `CLAUDE.md` (not applied; it is your file)

- *Direction.* Each phase begins with a one-sentence outcome a player or modder could see, and ends by rewriting the state document.
- *Decisions.* One-way doors (formats, wire, coordinates, the mod surface, assets, licence, the flagship game) need a one-page memo
  and the owner's yes before they are built; two-way doors are decided and logged in a line.
- *Rigour.* Tier 1 (whatever a world or a mod depends on) gets tests first, property tests, mutation experiments and both systems;
  tier 2 (rules, physics, the server) gets tests first and the pinned sessions; tier 3 (presentation, tuning) gets a test that it exists
  and a screenshot.
- *The engine.* This is the engine of a space game: it knows grids, frames, rails and generators, and no game's content. Policy lives
  behind settings a pack can fill.
- *Words.* A pack is data; a module is code that registers rules; a game is a manifest.
- *Time.* No wall-clock time in simulation or in anything recorded, saved, sent or compared; presentation smoothing may use it.
- *Other games' code.* Use the public documents of other games; do not read source that is licensed on terms that bar use in a
  competing product (Hytale's Shared Source).
- *Stability.* Pack formats and the wire protocol are versioned and migrate; the host API a WASM module sees is versioned; the Rust
  plugin API is internal and may change.
