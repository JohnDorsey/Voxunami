# Plan

What is built, what is next, how the plugin and mod system is meant to grow, and what is still open. Decisions that hold
everywhere are in `design-decisions.md`; wishes that are not scheduled are in `future-goals.md`; what each finished phase
built is in `build-log.md`. Status key as in `design-decisions.md`.

## Build plan
Phase 1 is the walking skeleton. Each step ends with passing tests, clippy, rustfmt and a suggested commit.
- 1a Foundations: identifiers, coordinates, grid storage, registry. **Done.**
- 1b Terrain: density fields with bounds and footprint, deterministic noise, generator-version octree, brick
  generation with pinned golden digests. **Done.**
- 1c Seeing it: CPU meshing, wgpu renderer, window with fly camera, offscreen render tests, integrated-GPU check. **Done.**
- 1d Saving: world container with sections, edits over generated terrain, unknown-data preservation tests. **Done.**
- 1e Server and stream: message protocol, loopback transport, recorder, replay-equals-live test. **Done.**
- 2 Second game (Voxunami 2048) and the shared shell. **Done.**
- 3 The network: TCP, hosting, joining, dedicated servers. **Done.**
- 4 Infinite 2048: numbered landscape, pushing, coasting, merging. **Done.**
- 5 Other players drawn, server brick cache, compact generator tree. **Done.**
- 6 Levels (protocol 2, file format, server, client, session) and a text command channel with `/level`. **Done.**
- 7 Block states and shapes in block definitions, detailed quads, light in the quad (with its spike). **Done.**
- 8 Numbers as block states in 2048 (colours by state in the engine, generator v2). **Done.**
- 9 Tools in 2048: the push names a tool, and the tool decides how blocks combine. **Done.**
- 10 Chain pushing in 2048: stone pushes a row of up to 12 blocks. **Done.**
- 11 The light rule (first lattice rule), tested against a search from the sources. **Done.**
- 12 Block light in Cave: lamp block, light channel, streaming, ambient light in the renderer. **Done.**
- 13 Fire in Cave (the second lattice rule). **Done.**
- 14 The `lattice` crate (active voxels, voxel chance) and a budget for the light. **Done.**
- 15 The `physics` crate: a walking body on Rapier, pinned across Windows and Linux. **Done.**
- 16 Cave gravity and collision: the server moves the players (`/fly`, `/tp`). **Done.**
- 17 Voxunami Eels (3D Tron) and the shell hooks it needed. **Done.**
- 18 Mod stage 1, first step: a game's content as one declared package (`ContentPackage`). **Done.**
- 37 (far future) Learning NPCs: brains as data (a network in a fixed format plus a manifest of sensors and actions, in the content
  package), learning in the background with results applied at a tick boundary, weights saved with the world (`future-goals.md`, goal 3).
- 36 (far future) Tentacle locomotion: an octopus character and NPC on articulated sub-grids (goal 1).
- 35 (far future) Airship features: command chair and third-person view, clicking parts for menus, drills, turrets, headlights (goal 2).
- 34 (far future) Dynamic lights: lights carried by entities and grids and drawn by the renderer (headlights, a lit ship passing over ground).
- 33 (planned, after 32) The native and WASM loaders (mod stages 3 to 8): dogfood the API, spike and measure both loaders, generator
  mods, hostile-mod tests, the mod list in the handshake, server-side hooks with budgets, assets and an SDK. The ABI is frozen only
  after Phase 32, because it encodes frames. Phases 34 to 37 are not ordered against it.
- 32 (planned) Moving grids (structures), with the owner's decisions of 2026-10-18 (`design-decisions.md`): grids in the stream with
  transforms; a matrix per grid in the renderer; Rapier bodies from blocks (mass from `engine:density`), full rotation, world gravity
  aboard; connectivity and splitting; docking connectors with a configurable attraction force; players and entities simulated in their
  frame; levels of detail per grid for drawing, for collision (coarse colliders, aggregated mass) and for streaming; a grid saved in
  flight as a file of its own. Specified in `moving-grids-spec.md` (steps 32a to 32k, with a pause after 32h), answered 2026-10-20; 32a (frames), 32b (grids on the server), 32c (protocol and replay), 32d (renderer and picking) and 32e (bodies) and 32f (the gravity field) built, 32g (aboard) next.
- 31 (built) A wider material palette: the renderer's palette in a storage buffer, 1,048,576 game materials instead of 240 (renderer-only; saves and protocol unchanged). Phase 32 (moving grids) is next.
- 30 (built; `ui-spec.md`) Server-driven UI: a UI the server describes in the value vocabulary and the stream drives; the HUD, the command line, questions and Cave's hotbar are on the screen, not in the title. Phase 31 is next.
- 29 (built; `data-packs-spec.md`, `pack-format.md`) Data-driven block packs (JSON), the generated JSON Schema, `check-pack`, and patches: tags by
  union, patch operations, dependency order, loud failures (revision 5; mod stage 2). Phase 30 is next.
- 28 (built; `content-package-spec.md`) The content package at the handshake, carried by recordings (revision 3). Phase 29 is next.
- 27 (built, 27a to 27i; Phase 28 is next) Entities and items: server-owned entities with typed components, inventories, the protocol messages and the
  `engine:entities` section reserved before 1.0; players become entities; the prompt facility. Specified in `entities-spec.md` (steps 27a to 27h,
  with a pause after 27d). Control targets, the `Brain` slot and block data stay planned for the phases that first need them.
- 26 The client interface (client plugins: message handlers, view hook, HUD line, key and mouse actions, `.`-commands) and the bulk light
  hook that took the scaled-light concepts out of `world_view`. **Done.** Specified in `client-interface-spec.md` (steps 26a to 26d).
- 25 Declared messages (a `Message` trait and a generated check, no codegen) and a command registry with `/help` (revision 4). **Done.** Specified in
  `messages-and-commands-spec.md` (steps 25a to 25d).
- 24 Property bags on block definitions, per state; `emission`, `flammability` and `passable` left the engine (revision 2). **Done.** Specified in
  `property-bags-spec.md` (steps 24a to 24d); the multiscale benchmark did not regress. A tag is a bool property; no separate tag type.
- 23 The registrar and events; Cave's light, fire and movement become plugins, and 2048 and Eels too (revision 1). **Done.** Specified and
  recorded in `plugin-api-spec.md` (steps 23a to 23e); the new crate is `plugin_host`. Golden sessions lock the recorded stream and the saved
  world of each game bit for bit, on Windows and Linux, in any order of the plugin list.
- 22 Multiscale light cells of any width (1 to 32), a hook for the client's view, four odd-width schemes benchmarked. **Done (a spike).**
- 21 Multiscale light schemes (32/16/8/4 levels, cells to 4/8/16/32) and per-scale weights, benchmarked. **Done (a spike).**
- 20 Cave's sky is dark until there is sky light, and the window's own commands start with a period. **Done.**
- 19 Multiscale light spike in Cave: four scales of 16-level flood fill (cells of 1, 2, 4, 8), switched with `/light`. **Done (a spike).**
- 1f Launcher and first game crate: `world_view`, the `base_blocks` plugin, Voxunami Cave, the launcher; the old viewer
  retired. **Done.**
The physics spike is done (`docs/physics-spike.md`); the adapter for rigid structures belongs with the structures work.

## Proposed
- Order of features, to let real uses shape the engine and the plugin API before either is frozen (proposed 2026-10-16;
  the guide is: do first what is expensive to change later, what is most uncertain, and what gives a second user to a
  shared piece). 1. Levels: a level id through the protocol, save format and server, with one level existing; then a text
  command channel (done: Phase 6). 2. Block shapes in block definitions (open faces, collision shape, translucency, light), with a spike
  on how light reaches the 8-byte quad (done: Phase 7). 3. 2048 tools: chain pushing and tool-chosen merging (done:
  Phases 8 to 10, with numbers as block states). 4. Lighting with
  directional flood fill: the first lattice rule, under the phased-tick rule and its partition-invariance test (block
  light, the rule and its tests: done, Phases 11 and 12; sky light with directions: not).
  5. Random ticks and fire in Cave: the second lattice rule; only then extract the common lattice-rule shape
  (neighbourhood snapshot in, edits out, deterministic random numbers, active region, budget; done: Phases 13 and
  14, see `design-decisions.md`, "Decided (rules for lattice simulation and physics)"). 6. Cave gravity and
  collision on Rapier (done for players: Phases 15 and 16). 7. Eels (done: Phase 17). 8. Mod stages 1 to 5 (see below),
  started once step 5 shows what a block definition and a rule hook carry. 9. Sub-grids with hinges and pistons, and Sculpt.
  10. Mod hooks: lattice rules first, then physics (engine-provided motor and piston parts that mods configure rather than
  implement). 11. Portals, user shaders, assets and an SDK. Level awareness must be in the mod API from its first version.
- Mods for players (proposed 2026-10-16; today plugins are compile-time crates, see `design-decisions.md`, "Decided"). One host API, two loaders:
  the contract is a versioned table of C-callable functions the host hands to a mod, plus a few functions the mod exports
  (register content, generate a brick, later: tick and message hooks). A native-library loader (C ABI, for mods the owner
  trusts: first-party add-ons, server-admin installs) and a WASM loader (sandboxed, one file for every platform, for mods
  players download) sit behind that one contract, so a mod written in C can be built either way. Native code cannot be
  sandboxed and cannot be forced to give the same floating-point results on every machine, so only the WASM loader is
  for untrusted mods and for anything clients must reproduce, such as terrain generators. Bulk calls only (whole bricks),
  never per-voxel calls across the boundary. Staged, each stage ending with something that runs and is tested:
  1. Settle the in-tree plugin API with the games we have (more of 2048's materials and Cave's rules into plugins), so the
     contract is shaped by real uses. 2. Data-driven mods (a folder or zip with a manifest: blocks, colours, materials):
     no code runs, and it forces the manifest, mods folder, ids, versions and missing-mod behaviour. 3. Dogfood the API:
     run `base_blocks`' generator through it; the pinned digests must match the native output exactly. 4. Spike both
     loaders as throwaway prototypes and measure boundary cost, generator speed against native, and identical results on
     Windows and Linux; choose from the numbers. 5. One narrow code mod type, terrain generators, with tests that
     hostile mods (endless loop, huge memory, wild writes, crash) are stopped. 6. Multiplayer: the handshake lists the
     mods a world needs by id and hash, and mismatches are refused clearly; downloading comes later. 7. Server-side
     logic hooks with CPU and memory budgets, on top of the phased-tick rule. 8. Assets, an SDK, templates and an example
     mod; only then promise compatibility. The API is labelled experimental until others have written real mods. A mod
     cannot add GPU code, change the engine's internals (save container, network protocol, renderer, physics), touch the OS,
     or depend on wall-clock time or unseeded randomness.
- Plugin shape revisions (proposed 2026-10-17; the owner wants the first release shaped as well as it can be while there are
  no players and no saved worlds to protect, especially the parts modders touch). They come from comparing this design with
  Hytale, Minecraft (Java and Bedrock), Factorio, Luanti, Vintage Story, RimWorld, Starbound and Veloren. The eight weaknesses
  they answer, the options weighed for each, what they leave open, and the sources are in
  `docs/plugin-architecture-review.md`. Five changes, in the order they depend on each other, each small in API terms:
  1. **A registrar and a few events (Phase 23).** A plugin is one function handed a `Registrar` (on the server and on every
     client) and registers what it adds: blocks, channels, property keys, commands, message kinds, tick systems, event handlers,
     save sections, and client hooks (a HUD line, key actions, client commands). That list is the API's size budget: an eighth
     kind of thing needs a reason written here first. Events are few, named and batched: `BlocksChanged` carries a slice of
     changes, never one call per voxel, so the later WASM boundary stays bulk-only; `PlayerJoined`, `PlayerLeft`, `LevelChanged`,
     `WorldLoaded` and `WorldSaving`; and two "ask first" events, `BlockPlaceRequested` and `BlockBreakRequested`, whose handlers
     may refuse. Order is part of the replay contract: every handler and system runs in one sequential order given by explicit
     `after`/`before` and, for ties, plugin id (never hash order, never in parallel), so one stream gives one world on every
     machine. Declared channel reads and writes are checked, not trusted and not scheduled: in tests and debug builds a system
     that writes a channel it did not declare fails, and so do two systems that write one channel with no order between them (the
     check Bevy calls ambiguity detection). Rules keep the shape of the lattice decisions above: snapshot in, edits out, budget;
     a system that writes the world directly is marked exclusive, and the partition-invariance test stays. Whether the common
     shape of the three rules (classic light, multiscale light, fire) is extracted as a `LatticeRule` kind is decided by
     converting them, not in advance (the earlier decision was that two rules did not show it). Cave's light, fire and movement
     become three plugins that talk through events (`set_block` stops calling them); 2048's and Eels' rules follow; and
     `GameDefinition` shrinks to what only a game says (name, controls, camera, tick rate). Done the way Phase 18 was: tests
     that pin the saved bytes, the generator digests and every existing Cave test come first and pass unmodified afterwards.
  2. **Property bags on block definitions (Phase 24).** A block definition carries namespaced, typed properties that plugins
     register, instead of fixed fields. `emission` and `flammability` (read only by Cave's rules) move to `base_blocks`' own
     keys; `passable` goes with movement; the engine keeps only what it reads itself: colour, shapes, see-through, state colours.
     A key has an identifier, a value kind (bool, integer with a range, the id of a registry entry, string; nothing a rule reads is
     a float, which keeps simulation bit-identical), a default, a doc line, and whether only clients read it. A bool property is a
     tag (set membership; packs merge tags by union). A table is built from definitions once into one column per key, indexed by
     block id, read through a typed handle (a plain array index in the hot loops); an unknown key, a wrong type or a value out of
     range is an error naming the file and the path. The same value-kind vocabulary serves message fields, command arguments and
     configuration, so there is one small schema language, not three. Per-voxel data stays in channels (already a namespaced bag
     per voxel); properties are the same for every voxel of a block.
  3. **The server sends the content package at the handshake (Phase 28).** `Welcome` carries, or names by hash, the package: block
     definitions with their properties and shapes now, assets and generator WASM later. A client needs no plugin installed and
     builds its block table from the package; a recording carries it, so a replay plays with no plugins present; clients cache by
     content hash (Minecraft's server resource packs do the same). Until WASM generators exist, terrain generators stay compiled in
     (clients still regenerate terrain from the seed) and the package carries data only. For a generator that cannot be made
     bit-identical, a *server-streamed* generator kind (its bricks are sent, nothing is regenerated by the client) is the escape
     hatch, at the cost of bandwidth. Client code finds blocks by identifier on connecting, never by compiled-in ids.
  4. **Messages and commands that are declared (Phase 25).** A plugin's message kinds are declared with a field list in the shared
     value vocabulary, so both ends come from one definition, the round-trip test is generated, and unknown kinds stay ignorable;
     the engine's own hot-path messages (brick edits) keep their hand-written compiled codecs. A command registry takes a name,
     typed arguments, a usage line, a handler and a permission level, and `/help` and the unknown-command reply are generated from
     it; the window's period-prefixed commands are registered the same way by client plugins. Not Brigadier-sized.
  5. **A generated schema for the data format (Phase 29, with data-driven mods = mod stage 2).** A JSON Schema is generated from the
     registered keys and kinds (never written by hand, so it cannot drift), so editors complete and check files, and
     `voxunami check-pack` reports errors with file and path. How one mod changes another's blocks (patch and overlay order) is
     designed in the same phase; see "Open".
  How they map onto the mod stages above: stage 1 (settle the in-tree API) is revisions 1, 2 and 4; stage 2 (data-driven mods)
  is 5 and the patch design; the handshake of stage 6 is 3, which needs only serializable definitions and so does not have to wait
  for JSON loading. None of this adds a loader: the native and WASM loaders (stages 3 to 8) are the last phase, still decided from
  measurements.
- More improvements added to the plan (owner, 2026-10-17: do them all, put the palette widening and the WASM and native loaders
  last; the review's "worth it" verdicts for a team of two are advice about order and size, not a reason to leave any out). Besides
  the five revisions above:
  - **Client interface (Phase 26):** plugins register a HUD line, key actions and period-prefixed client commands (pure functions of
    the stream plus local state, so replay is unaffected); and the scaled-light leak is closed: a bulk client hook turns a brick's
    channels into brightness, `ScaledLight`, `LevelFormat` and `MAXIMUM_LIGHT_SCALES` leave `world_view` for Cave, and the multiscale
    light stops being a spike.
  - **Entities and items (Phase 27):** server-owned entities (id, kind from a registry, pose, typed components from the property
    machinery; large or articulated things are grids with a transform, as decided), items as registry content with stacks that carry
    components, an inventory, and the protocol messages and the `engine:entities` save section, reserved before 1.0 so old recordings
    never need a second decoder. No ECS.
  - **Patches (inside Phase 29):** tags merge by union; other changes to a definition are patch operations by namespaced id (`set`,
    `add`, `remove` on properties, and a JSON-Patch-style `test`); mods apply in dependency order, ties by id; a patch that does not
    apply is a loud error naming file and path. No scripted data stage.
  - **Data-driven JSON blocks and packs (Phase 29)** with `check-pack`, and **server-driven UI (Phase 30)**: UI described by the server
    in the value vocabulary and driven by the stream, so replay holds.
  - **Wider palette (Phase 31):** the renderer's material palette moves to a storage buffer; a renderer-only change (see "Open").
  - **Native and WASM loaders (Phase 33):** mod stages 3 to 8 as listed above: dogfood the API, spike both loaders and measure,
    generator mods so generators can travel in the content package, hostile-mod tests, the mod list in the handshake, server-side
    hooks with budgets, assets and an SDK. The last phase, because it matters only with code from outside.
- Requirements pulled forward from the far-future goals (proposed 2026-10-17; the goals and their difficulty are in `future-goals.md`: an
  octopus that walks by touching walls with tentacles, airships whose blocks and mods work aboard a moving grid, and NPCs with learning
  neural networks). None of them is scheduled before the phases above, but each needs something to be true of the plugin API now,
  because it would break mods later; all are cheap while no world, mod or recording exists.
  1. **Rules see a grid, not the world (Phase 23).** What a rule or handler is given is a `GridView`: one grid and local voxel
     coordinates. A grid is addressed by a `GridId` (a stable unique id; a level's terrain is its root grid), which replaces
     `(LevelIndex, VoxelCoordinate)` as the address in `ActiveVoxels`, in `ServerWorld` voxel access, in events (`BlocksChanged` says
     which grid) and in the saved pending-work sections. Only root grids exist until structures are built, so this is a type change
     now. A plugin has no way to read a neighbour in world coordinates, which is what lets a modded wire, piston or door work on a
     moving grid with no work by its author. Facts about the world come as services on the context in the grid's own terms
     (`gravity()` is the world's gravity as a direction in the grid's local axes, so it changes as a ship turns; `world_position(local)`;
     `sky_exposure(local)`), never as raw coordinates.
  2. **Named tick phases (Phase 23):** messages, decide (brains), pre-physics, physics, post-physics, rules, send; systems and handlers
     are ordered within a phase. Physics is an engine phase, single-threaded as decided, so a plugin's controller applies forces
     before it and reads contacts after it.
  3. **A determinism class on each system (Phase 23):** `Deterministic` (the default; covered by the pinned digests and the
     partition-invariance test) or `Mind` (may differ between machines, so it may only emit *intents* through the validated path a
     player's input takes, and never writes the world). One enum now; it is what lets a learning NPC exist without breaking the rule
     that simulation is bit-identical.
  4. **A `BlockUsed` event (Phase 23):** a player used a block (grid, local position, player, hit), beside the two ask-first events.
  5. **Engine-reserved properties (Phase 24):** the engine registers some keys in the same bag: `engine:density` (a block's mass, for
     bodies) and `engine:placeable_on_moving_grids` (bool, default true: the one thing a mod author sets for airships).
  6. **A frame-invariance test (Phase 23, then required of every plugin rule):** the same rule on the same scene in a moving grid, turned
     by multiples of 90 degrees, and in a static grid whose local gravity is the same vector gives identical local results (the
     partition-invariance idea applied to frames; gravity is the one input that depends on the turn, so the static grid is given
     it). It makes "works aboard an airship" a tested property.
  7. **Client (Phase 26):** view modes (a server-set camera, a third-person orbit about an entity or grid) per control target, and
     picking that returns a grid and a local position.
  8. **Entities (Phase 27):** an entity's pose is a frame and a local pose, players included, the frame being the world or a grid (so
     a minecart on a ship's rail, or a player aboard, is simple to represent); a *control target* (a player's inputs go to an entity
     whose kind supplies the controller and the input mapping: walking, flying, a ship through a command chair, an octopus); *block
     data* (sparse per-block component bags for furnaces, chests and the like, saved and replicated with the grid in its own
     coordinates); a `Brain` slot that emits intents; and inventories.
  9. **The API's size budget counts verbs, not kinds of content.** The verbs: content (into any registry: blocks, items, entity kinds,
     brain models), channel, property, command, message, system or handler, save section, client hook. A new kind of content is not a
     new verb.
  10. **A `GridId` is a stable unique id within a world, not an index (Phase 23):** ships are made, split, merged, saved and loaded
     independently of any level. Ids come from a counter saved with the world (so a replay and a reload agree). A ship file carries an
     origin id of its own; loading it into another world, or twice into one, allocates a fresh `GridId`.
  11. **Nothing in the API assumes a grid is small (Phase 23):** no call returns a whole grid or scans one densely; access is by region,
     by active set, or by neighbourhood; so a grid the size of a moon, and levels of detail for drawing, collision and streaming, can
     come later without changing a rule.
  12. **Grid files (reserved in Phase 27, built in Phase 32):** a grid's saved form is a standalone file in the container format with its
     own registry and channel tables and its block data, so it is portable to another world (decided 2026-10-18); sections that are
     optional hold its flight state (pose, velocities, pending rule work), and a load into another world drops them. A world file refers
     to a ship by `GridId`; the world and its ships are saved at one tick so that they agree. Entities aboard belong to the world, not
     the ship file.
  13. **Merge and split are grid operations (reserved in Phase 23, built in Phase 32):** docking merges two grids into one (decided
     2026-10-18). `GridMerged { from, into, transform }` and `GridSplit { from, parts }` are events and stream messages; the transform is
     lattice-aligned (a turn by a multiple of 90 degrees and a whole-voxel offset); ids of parts come from the saved counter; a
     merged-away id leaves a tombstone naming its successor and the transform, so a saved reference to it still resolves; entities in
     the frame are re-framed. Anything that blocks the merge or the split is the engine's to refuse, not a plugin's.
  14. **Plugin state must survive a merge and a split (Phase 23):** persistent per-grid state is keyed by voxels (so it can be moved by
     the transform) or is a cache the plugin can rebuild from the voxels; the registrar's save-section hook says which. The tests
     of requirement 6 grow two: a scene built as two grids and merged settles to what it would have been built as one (merge
     invariance), and the converse for a split.
  15. **A prompt facility (Phase 27):** a choice sent to one client (a title and options) and its answer, with the entity held until
     answered. First use: the missing-ship case (spawn at the world spawn, spawn in midair, quit to title). The UI of Phase 30 can
     restyle it.
  16. **A world-space fallback pose is saved with every entity whose frame is a grid (Phase 27's entity section),** for the midair choice
     and for any grid that cannot be loaded.
  17. **Gravity is a field (Phase 32, owner 2026-10-20):** `gravity_at(local position)`, summed from sources, where a source is the level's own
     and any ship above a configurable mass; replaces `gravity()` in requirement 1. Huge ships pull (the earlier "undecided" is now yes).
  18. **One cross-grid conversion and one door (Phase 32 builds the conversion; the door waits for the first fluid):** `voxel_in_other(from, voxel, to)`
     is a pure function; a mailbox of outflows and inflows (conserving, ordered, rate-limited) is the only way a rule affects another grid, for
     fluids and for current on a channel (`future-goals.md`, "Ship goals" 3). Rules still never read another grid.
  19. **Grids can be loaded and unloaded while the server runs (Phase 32):** parking for the simulation distance and for a ship that leaves with its
     pilot; a grid's header says what to do when its pilot logs out (`stay` or `vanish`); a server setting can override it.
  20. **Ship simulation distance and unload distance are server settings (Phase 32);** terrain should get the same shape later.
  21. **Mesh keys are `(grid, brick)` and can grow a level of detail** (Phase 32 draws level 0): terrain's levels of detail, when built, serve
     grids too (`future-goals.md`, "Ship goals" 1).
  Not changed by the goals: the content package, the schema, patches, the palette. One ordering consequence, accepted by the owner
  2026-10-18: the loader ABI (Phase 33) encodes the coordinate model, so it is frozen only after grids, frames and entities exist
  (Phases 27 and 32).

## Open
- Plugin architecture (2026-10-17; `docs/plugin-architecture-review.md` has the options and sources; each item is now in the
  Build plan, and what stays open is the detail): (a) *Entities and items* (Phase 27): only players exist, and the protocol, the
  replay stream and the save format have no entity messages; the design says creatures are finite grids, which is not built. What
  an entity replicates, how an inventory is shaped, and what the first mob is are open. (b) *Patches* (Phase 29): the operations
  and failure rules are written above; the policy for a patch to a property whose plugin is missing (ignore with a warning, or
  refuse) is open. (c) *Materials* (Phase 31): built in Phase 31: the renderer draws up to 1,048,576 game materials (it was 240: a 256-entry palette less 16 for
  avatars); the packed quad has 24 bits for the material and saves hold block ids, so widening it was a renderer-only change; whether
  the first release has textures is open, and a texture path (an array indexed by material and face, UVs from position) is a larger
  decision than the palette. (d) *Scaled light in the engine*: closed in Phase 26 (the `BrickLight` hook; Cave's concepts live in Cave). (e) *Client interface*
  (Phases 26 and 30): plugins now register a HUD line, key and mouse actions and `.`-commands (Phase 26); the HUD is still the window
  title and there is no UI toolkit (Phase 30). Rebinding actions with client commands is a later phase (the owner's decision: any
  number of actions may share a key or button; they are keyed by action identifier so that rebinding can name them).
- **Explained and fixed (2026-10-19):** `a_walker_standing_in_flames_falls_when_the_fire_burns_the_ground_away` failed about 1 full Linux run in 4 and
  2 of 30 runs of Cave's play tests. The failure printed that the walker had dropped 0.0533 voxel (one tick of free fall) in the two ticks after
  fire was placed, and that the grass under them was gone: the fire rule burns grass by chance from the tick number, the tick the test starts
  its script on depends on how many rounds the loading before it took (a busy machine takes more), and a grass that burned inside the two-tick
  window started the walker falling. Engine and physics are deterministic (both failures were bit-identical); the test raced the chance. The
  assertion now says what is true in both cases: with the grass there they have not moved; with it burnt they have only fallen. 0 failures in 20
  runs after.
- A second flaky test, unexplained (2026-10-18): `walkers_pass_through_flames_and_the_cellar_has_the_same_ground` failed once in a full
  parallel Linux run, at its last check, which compares the player's height with a ground height read from the client's mesher. Run alone
  (`--test-threads=1`) it failed 0 times in 30 on the commit before Phase 23 and 0 in 30 after Phase 23b, so nothing says Phase 23 caused
  it. My belief, not tested: it depends on how far the client's worker threads have got, which a loaded machine changes; the first flaky
  test above also reads client state. A fix would make such tests wait on a condition, as the `world_view` meshing test now does.
- **`an_edit_changes_the_picture_and_a_replay_draws_exactly_what_the_player_saw`: EXPLAINED AND FIXED (2026-10-21, Phase 30).** It compares a live picture with
  a replayed one byte for byte, and on the integrated AMD GPU the first frames of a fresh renderer sometimes leave pixels out (measured: 15 pixels of a world
  picture on DX12, in 1 run of about 40; 1 to 7 of 2000 one-pixel rectangles in the first four frames; 2 pixels of a line of text in the sixth), never
  on the NVIDIA card. The belief below that it depended on client worker threads was wrong for this test. Pictures compared to the pixel are now
  taken once a frame after them repeats them (`settled`); 30 runs afterwards were clean. The two walker tests are a different matter (explained below).
- More occurrences of the flaky tests (2026-10-18, during Phases 24 and 25): `an_edit_changes_the_picture_and_a_replay_draws_exactly_what_the_player_saw`
  (`games/voxunami_cave/tests/pictures.rs`, the replayed picture differed from the live one) failed in 2 of about 6 full parallel Windows runs and in
  0 of 3 runs of `cargo test -p voxunami_cave` alone; `walkers_pass_through_flames_and_the_cellar_has_the_same_ground` failed once more in a full Linux
  run and passed on the rerun. Both only under the load of a whole-workspace run, so the same belief applies (the result depends on how far the
  client's worker threads have got); not tested. `a_walker_standing_in_flames_falls_when_the_fire_burns_the_ground_away` failed once more in
  a full parallel Linux run (end of Phase 26) and 0 of 15 times alone (explained above). Also: `cargo test --workspace` stops at the first failing crate, so use `--no-fail-fast`.
- `walkers_pass_through_flames_and_the_cellar_has_the_same_ground`: EXPLAINED AND FIXED (2026-10-19). It failed in about 1 of 8 whole-file runs
  under load, and about 1.7 percent of the time by a sweep of the tick the fire starts at. The chain, each link checked: (1) how many ticks
  `rig.settle` runs depends on how busy the client's threads are, so the tick at which the test's fire is placed differs between runs; (2) fire
  burns grass by chance from the tick, and when two or more grass columns burn in the walker's path they are a trench it cannot climb out of, so
  it stops there; (3) `/level go cellar` teleports the body to the same eyes in a level whose ground is whole, so the body is one voxel inside
  solid grass; (4) the physics did not resolve that: Rapier pushed the body some way sideways and left it still inside blocks (39 of 256 probe
  teleports into hills), and a body put several voxels deep fell through the ground, because the colliders leave out boxes buried inside solid
  terrain. The fix is in the physics: a body that is made or teleported is moved the shortest way out of terrain (see `design-decisions.md`).
  Regression tests: `a_player_in_a_trench_sent_to_a_level_whose_ground_is_whole_ends_up_on_it_not_in_it` reproduces the original failure exactly
  (eyes at [15.78, 103.63, 12.31] inside voxels (15, 102, 12) and (16, 102, 12)) without the fix and passes with it; the teleport probe in
  Cave; and in `engine/physics/tests/walking.rs`, 16 new tests and 2 properties, of which 6 tests and both properties fail without the fix (the rest guard the policy). The original test no longer asserts the walker
  got more than 3 voxels into the flames (2 now: a burnt trench can stop it short, which is not the flames stopping it). A fix that
  fixed the tick the fire starts at was tried and removed: how long settling takes is unbounded under load (it exceeded 1000 ticks during a
  cold Linux build), so any fixed tick is a new way to fail. Measured on Windows, 10 whole-file runs with every core busy, all clean, and on Linux, 8 whole-file runs with every
  core busy, all clean (the condition that failed about 1 in 8 before).
  Left open, small: (a) a body put within a hair (under the controller's 0.01 skin) of two walls at once can be left standing a little above the
  ground it falls beside (the controller counts the walls as support): it is not inside anything, and the properties do not assert on how a body
  rests; (b) editing a block into a body that is already standing there is not cleared (only placing a body is).
  (The `/tp` reply used to name the place asked for even when the body was moved out of terrain; it now clears the body during the
  command and says where the eyes are: `teleported to X Y Z (moved from X Y Z, which is inside the terrain)`.)
- Light (Phase 12): cost is one lamp's 64 ticks of spreading averaged 15 ms a tick in a release build (with two clients
  watching; measured once, not repeated), and a tick is 50 ms, so a few lamps placed at once would overrun it. The budget
  of 20 000 voxels a tick (Phase 14) is a guess, not tuned, and its streaming cost to clients is not measured. The sky light
  with its six directions, how block and sky light combine, and a daylight cycle (replacing the placeholder ambient light)
  are not built either, and the glow of a lamp could not be judged by eye here.
- Keyboard controls in a window (reported by the owner, 2026-10-16): in both games the keys type into the title bar instead
  of moving the player. Not reproduced: with real key events sent to the real window (Windows 10, RTX 3090, built from
  this code), pressing W and A left the title as it was, and holding W for two seconds changed the picture (the camera
  moved). The title shows typed text only while the command line is open, and it was opened by Enter or `/`. The owner
  gets control back by pressing Enter, so an Enter (or a `/`) reached the window first. One way was reproduced, an Enter
  still down when the window is brought to the front, but not shown to be the owner's. Changes: only `/` opens the
  command line now (Enter only sends it), a repeated or made-up press does not, a change of focus drops a half-typed
  command, and the console says when it opens. Whether that cures the owner's case is not known.
- 2048 shows numbers by colour only (the renderer draws no text; the title names the block you look at), and only
  stone, ice and spring ice exist. Materials that pass momentum on, other tools (a line push, an undo), and players
  being in the way of blocks are not built. The play controls were never tried with a real mouse and keyboard.
- Levels: a world cannot gain a level while a server runs, no game ships a second level yet, and `add_level` cannot check
  that a level's generators were built from the world's seed (a mismatch shows up as clients going out of sync). Typing
  commands in the window was written but never tried with a real keyboard.
- Client-side prediction (owner, 2026-10-16: walking and falling feel jerky and stuttery; deferred). My view: the stutter is
  as likely to come from the 20 Hz updates and the dead reckoning in `PoseFollower` as from latency, so smoothing the
  followed pose comes first; real prediction (the client runs the same `CharacterBody` and corrects toward the server) changes no
  API: `movement_message` and the server's `Movement` stay as they are. Eels already glides by sending a pose every tick.
- Movement (Phases 15 and 16): Cave players walk with gravity and collision, on the server. Not built: prediction on the
  client (a player's own keys show after the round trip to the server, plus a tick; on a distant server that will be felt),
  swimming, ladders, crouching and edge-holding, damage from falls, and pushing other players. The camera bobs by up to
  about 1.4 cm where bricks meet (the body is momentarily held by two colliders), too little to see but not zero. How it feels
  was not judged by eye: the tests measure heights and distances, and the real window was tried only with synthetic keys.
  Other players are drawn as a plain pillar that does not face where they look, and there is no inventory.
- Network scale: a spatial index over watched regions (an edit currently scans all clients); passwords or at least a
  shared secret, and encryption, before play over the internet; resending single bricks to repair a client that fell out of sync; replay keyframes for seeking; interest with a
  level of detail. See "Known limits" in `docs/protocol-and-replay.md`.
- The server's cache of generated bricks is 128 bricks, chosen without measuring; it matters when logic reads many
  different bricks every tick (large moving structures, simulation across a wide area).
- Which replicated entity state (players, structures) is common to all games; it travels as game messages until a second
  game shows what to promote.
- Active-voxel budget per tick for Game 4; set empirically once simulation exists.
- Generator boundaries: hard cuts, or may a new generator blend toward its neighbour? Who can trigger regeneration?
- How joints are authored, how merging aligns rotations (snap to 90 degrees?), and what happens on overlap.
- How "sky light" is computed from open space.
- Terrain performance: skip sampling known-uniform columns, reuse planar noise across a column, and design caves so
  bounds can classify more bricks.
- Determinism is verified only for x86-64 and rustc 1.99.0 (Windows and Linux); other architectures and compiler
  versions are untested.

- Rendering at scale: the 5.5 M-quad view at radius 12 needs LOD (coarser bricks far away) and GPU-driven culling to
  scale further; draw calls are currently one per visible brick.
- GPU tests that are *failing* while running in parallel have twice crashed or hung the test process (an access violation,
  and a ten-minute hang); neither reproduced in later parallel runs and passing runs have been stable. When debugging a
  GPU test failure, run `--test-threads=1` under a timeout.
- Interactive controls (keyboard, mouse capture, resizing) were written but not exercised by the assistant, who cannot
  see or touch the window; the window opens, renders and closes cleanly on both GPUs.
