# Plugin and engine architecture review

Written 2026-10-17 by the assistant, at the owner's request: compare Voxunami's engine, plugin and game structure, and the planned
plugin API, with games that have excellent modding support (Hytale especially); say where it is weak, what to do, and what is not
worth doing. The owner has no players and no saved worlds, so every change, however disruptive, is on the table before the first
official release. The decisions that came out of it are in `docs/design-decisions.md` ("Plugin shape revisions", "Open", Build plan
phases 23 to 27). This file holds the reasoning behind them and is not itself a decision.

## 0. How to read this

Every claim carries one of three labels.

- **[code]** I read it in this repository on 2026-10-17.
- **[web]** It comes from a search result or a fetched page listed under Sources. Most are documentation, wiki pages, forum threads
  and mod-portal discussions. I found almost no real surveys of modder opinion, so "how well it works" below means what the docs
  and public threads say, not measured satisfaction. Where I found nothing, I say so.
- **[memory]** I recall how a game does it and did not check today. Treat as a lead, not a fact.

Hytale details are secondhand (hosting guides, community docs and the official API package index), and the game is in early
access with changing APIs.

## 1. A correction to my earlier review

In the first review I said the 256-material palette might need a change to the packed quad. That was wrong. [code] The quad keeps
the material in 24 bits (`engine/voxel_meshing/src/quad.rs`: bits 0 to 23 are the block id or state-colour index, 24 to 31 the
light). The 256 limit is the renderer's `palette: array<vec4<f32>, 256>` in `engine/renderer/src/voxel.wgsl` and `PALETTE_SIZE` in
`voxel_renderer.rs`, with 16 entries kept for avatars, so 240 materials. Saves hold block ids and the protocol carries ids, not
palette indices, so widening the palette later changes neither the format nor the protocol. That moves weakness 5 from "decide
now" to "small, local, can wait".

## 2. The five revisions

| # | Revision | Phase | Where in `docs/design-decisions.md` |
|---|---|---|---|
| 1 | Registrar and a few events | 23 | Proposed, "Plugin shape revisions" item 1; Build plan 23 |
| 2 | Property bags on block definitions | 24 | same, item 2; Build plan 24 |
| 3 | Content package at the handshake | 28 | same, item 3; Build plan 26 |
| 4 | Declared messages and a command registry | 25 | same, item 4; Build plan 25 |
| 5 | Generated schema for the data format | 29 | same, item 5; Build plan 27 |

The order is by dependency: the registrar is what the others register into; property bags are what the package and the data format
carry; declared messages and commands need the registrar; the schema is generated from what is registered. Open questions the five
do not settle are listed under "Open" in the same file.

### 2.1 Property bags: what they are and how they are usually built

A *property bag* is a per-definition map from a name to a value, used instead of fixed fields. A block type is the definition here:
today `BlockDefinition` has `emission` and `flammability` as fields [code], so adding "conductivity" means editing the engine
crate. With a bag, the plugin that cares about conductivity adds a key and reads it.

**Where you meet them.** From loosest to most structured:

1. *Untyped map.* Luanti nodes have `groups = {cracky = 3}`, a table of name to integer, used so rules can match nodes across mods
   [web: example node definitions from Luanti's test game, and a commit adding `group:name` matching to LBMs, which ABMs already used]. Minecraft's NBT is an
   untyped tree [memory].
2. *Registered, typed keys.* Minecraft Bedrock blocks have `components`, a map from a namespaced id such as
   `minecraft:light_emission` to a value, each with a known shape [web]. Minecraft Java since 1.20.5 has data components: typed,
   namespaced keys with codecs for disk and network, replacing free-form NBT on item stacks, with a `custom_data` escape for
   anything not converted [web]. This is the pattern I recommend.
3. *ECS components.* Typed, registered, stored per entity in sparse sets or archetype tables. Hytale's server API has an ECS
   package [web]. It solves a different problem (many entities, per-entity changes), not needed for block definitions.
4. *Column storage.* The runtime form I would use: one dense array per key, indexed by block id. It suits hot loops, and
   `BlockTable` is already flat arrays [code].

**The usual three layers**, and what each gives us.

- *File layer:* a JSON object, key to value. Easy to write and merge.
- *Registry layer:* keys are registered by plugins with a kind, default, range and a doc line. This is what turns a typo or a wrong
  type into an error naming the file and path, and what a schema is generated from. Without it a bag is the stringly-typed mess
  people complain about in NBT.
- *Runtime layer:* compile once into columns, hand out typed handles (`PropertyKey<u8>`), so a read in the light rule is an array
  index and a wrong-type read does not compile. Rare keys can use a sparse map instead of a column.

**Sketch for Voxunami:**

```json
{ "id": "base_blocks:lamp", "color": [1.0, 0.95, 0.6],
  "properties": { "base_blocks:emission": 64 } }
```

```rust
// base_blocks, when it registers
let emission = registrar.property::<u8>("base_blocks:emission", Property::new().default(0).doc("light given off, 0 to 255"));
// the light rule, per block read
let given = table.get(emission, block);   // one array index
```

**Two ideas to keep apart.** Minecraft's `BlockState` properties are per voxel (facing, powered). Ours are not: per-voxel data is a
*channel* [code], already a namespaced, translated-by-id bag, and the block's state. A property is the same for every voxel of the
block type. This also means properties are never saved in worlds; they are definitions, rebuilt from content (or from the handshake
package, revision 3).

**Pitfalls the research shows.**

- *Meaning drift.* Bedrock's `light_emission` was `block_light_emission` with a 0.0 to 1.0 float in older formats and is an integer
  0 to 15 now, with a `format_version` to tell which, and a community guide warns that an old format version limits options [web].
  Lessons: integer kinds with explicit ranges, versioned definitions.
- *Merge rules.* In Bedrock a later permutation entry overrides an earlier one for the same component [web]. Decide merge
  semantics up front (override, union for tags, add for numbers).
- *Migration cost.* Moving Minecraft from NBT to components broke commands that used the old tag syntax [web]. Choose the shape
  once.
- *Determinism.* A float property read by a rule would break our bit-identical rule. Our kinds are bool, bounded integer, registry
  id and string; floats only for properties flagged client-only.
- *Sprawl.* Everything becoming a property. The engine keeps typed fields for what it reads itself (colour, shapes, see-through,
  state colours); a property needs a plugin that owns it.

Of the engine's fields, `emission` and `flammability` are read only by Cave's rules [code: `lighting.rs`, `multiscale_light.rs`,
`fire.rs`, `logic.rs`]; `passable` is read by Cave's `movement.rs` [code]; `see_through` and shapes are read by the mesher [code].
Moving the first three out is a relocation, not a behaviour change.

## 3. The eight weaknesses

Each report: what it is in our code, whether the five revisions address it, the options with pros and cons, who does it that way and
how it has gone, and my recommendation with a confidence level. "Two people" lines summarise section 6.

### W1. A game is one lump, not a composition of plugins

**Today [code].** `GameDefinition` is a trait of static functions on a unit struct; each game gives the server one
`Box<dyn GameLogic>`. `CaveLogic` owns classic and multiscale light, fire, movement and the pose book and calls them directly:
`set_block` calls `light_changed`, `movement.brick_changed` and `fire.ignite`; `tick` runs them in a written order;
`command_received` is a `match` over `fly`, `light`, `tp` (and `level` is answered by the pose book); `save_sections`
concatenates their sections by hand. Adding a rule today means editing `CaveLogic` in about five places (the struct, `new`, `tick`,
the coupling in `set_block`/`light_changed`, `save_sections`). There are no events.

**Addressed by the five?** Yes: revision 1 is this.

**Options**

| Option | Pros | Cons |
|---|---|---|
| A. Keep one logic per game | Nothing to build; readable | Every new rule edits `CaveLogic`; no third party can exist |
| B. Registrar, sequential ordered events and systems | Small API; deterministic; replay-safe; plugins independent | Ordering and "who vetoes" conflicts between plugins; events cost a call each, so must be batched for a WASM boundary |
| C. Full ECS with parallel scheduling | Scales; strong tooling for conflicts | Heavy; most of our state is voxels, not entities; parallel order is a determinism hazard |
| D. Plugins patch engine code (Mixin, Harmony) | Maximum reach | Conflicts between mods; not possible across a sandbox; ties to the host language |
| E. Plugins only exchange messages | Decoupled | Latency in a tick; hard to reason about order |

**Who does it, and how it has gone**

- Event buses with priorities and cancellation: Forge has five priority levels, cancelable events, cancelled events hidden from
  later listeners unless they ask, and listeners run in registration order within a priority; a thread shows modders asking how two
  mods that cancel and transform the same event are ordered [web, the thread is from 2013]. Fabric's documentation tells modders to
  prefer events over mixins because events are more compatible and faster, and to use mixins only where no event exists
  [web]. A mixin conflict that stopped another mod's networks ticking is documented in a mod's issue tracker, and a mod exists
  to blacklist or reprioritise mixins, with a warning that it can break things [web].
- Hytale's API index lists an ECS package, and event families: synchronous events, four async events, and ECS events some of
  which are cancellable (block break and place among them) [web: official package index and a community event list; the official
  page does not explain dispatch or ordering].
- Bevy detects "ambiguities", pairs of systems with conflicting access and no defined order, because they are a bug class; detection
  is opt-in per schedule, and a user hit a crash on 0.14 from a conflict inside a third-party crate [web].
- RimWorld has both XML patches for data and Harmony for code; its wiki says there is no technical excuse for conflicts now, but
  also that a C# override can win over a later XML patch because of load order [web].

**Recommendation (confidence: high that composition is needed, medium on details).** Option B, with these rules, all in the design
doc: one sequential order from explicit `after`/`before`, ties by plugin id; events batched (`BlocksChanged` is a slice); only two
"ask first" events can veto; rules keep the lattice shape (snapshot in, edits out, budget) and writers of the world directly are
marked exclusive, so the partition-invariance guarantee survives; declared channel reads and writes are *checked* in tests (the
Bevy idea) and not used to schedule. Extract a `LatticeRule` kind only if converting light (classic and multiscale) and fire shows
a common shape; the earlier decision, that two rules did not, was right at the time.
**Two people:** worth it. It is the only item whose value grows with each addition.

### W2. The client builds its own content instead of receiving it

**Today [code].** `GameContent` is compiled into the client and the server ("Both the server and every client have one"). `Welcome`
carries the world's *description*: seed, registry tables (identifiers only), channels, levels and generator setup, not block
definitions. Clients regenerate unedited terrain from the seed with the same generator code. So a player needs the same plugins
installed, which is the opposite of Hytale's stated model (all mods server-side; players install nothing; the server sends what is
needed on connect) [web].

**Addressed by the five?** Mostly: revision 3 sends definitions (data) in the handshake. Code the client must run, the terrain
generators, is not covered until WASM generators exist (mod stages 4 and 5).

**Options**

| Option | Pros | Cons |
|---|---|---|
| A. Same content on both sides (today) | Nothing to send; simple trust | Players install mods; version mismatches; a recording needs the mods to replay |
| B. Server sends a content package (definitions, assets, generator code) at the handshake, cached by hash | No install; mismatches impossible by construction; recordings are self-contained | Definitions must serialize canonically; first join costs bytes; code in the package must be sandboxed; package schema needs versioning |
| C. Data and assets from the server; code mods installed (marked trusted) | Safe core; code stays native | Two classes of mod; the second still needs installs |
| D. Server streams all terrain (no client generation) | Generators need not be deterministic | Loses "terrain nobody edited is never sent"; large bandwidth |

**Who does it, and how it has gone**

- Minecraft Java servers can offer a resource pack: optional by default (the player is asked on first join), `require-resource-pack`
  makes declining a kick, a SHA-1 lets clients cache, and reusing a URL with an old hash leaves players on the stale pack [web].
- Factorio can sync mods on join when the server is authenticated (a "sync mods and restart" step); the client version must match
  exactly [web: hosting guides, not official docs].
- Luanti's server distributes media, with an optional separate HTTP media server for big libraries or slow links; client-side mod
  transfer from the server is documented as planned and not implemented [web]. That the server sends node and item definitions to
  clients is my recollection, which the search did not confirm [memory].
- Veloren's plugin guide says the API is new with no stability guarantees, WASM has no well-defined host ABI so it is
  event-driven, and server-managed distribution to clients is planned; a blog post reported a Wasmtime integration problem with
  thread safety, which may be outdated [web]. It is a project that tried the sandboxed route and has not finished it.

**Recommendation (confidence: medium-high).** Option B for data now (definitions are already plain data), WASM generators later,
generators stay compiled in until then. Keep the "server-streamed generator" kind as the escape hatch.
**Two people:** skip. Client and server are the same binary, so this buys nothing until a server for other people exists. The one
side benefit, self-contained recordings, does not justify it alone.

### W3. The engine knows game concepts

**Today [code].** `flammability` is a field of `BlockDefinition` in the `block_models` engine crate, and its comment says the game's
fire rule decides what it means; `emission` is there too and only Cave reads either. And, a shortcut of mine from the multiscale
spike (phases 19 to 22): `world_view` holds `ScaledLight`, `LevelFormat` and `MAXIMUM_LIGHT_SCALES`, Cave's multiscale-light
concepts, in an engine crate, against the dependency rule in `CLAUDE.md`.

**Addressed by the five?** Partly. Revision 2 removes the block fields. The scaled-light leak is not addressed.

**Options for the fields**

| Option | Pros | Cons |
|---|---|---|
| A. Leave them | No work; `emission` is near-universal | Each new concept edits an engine crate; breaks the stated rule |
| B. Property bag (revision 2) | One mechanism; validated; one place in a data pack; the schema comes free | Needs the registry, typed handles, columns: the most new code of the small items |
| C. Plugin-owned side tables, keyed by block id | Tiny; no engine change; typed | Each plugin invents its own loading; nothing shared for validation or a data pack |
| D. Block subclasses with overridable methods | Expressive | Not data; cannot travel in a package |

Precedents: Bedrock's components are option B [web]; Fabric API's flammable-block registry, a table separate from the block, is
option C [memory].

**Options for the scaled light**

1. *Leave it in `world_view`, generalised* (N weighted channels with a declared decode): it is already close to that. Cons: still a
   very specific format in the engine.
2. *A client-side hook, in bulk:* the plugin turns a brick's channels into one brightness channel (a pure function of the brick),
   run in the mesher's workers. Pros: the engine knows nothing of light formats; the call is per brick, so it also fits the
   planned WASM "bulk calls only" rule; pure, so replay is unaffected. Cons: plugin code on the meshing path; must be `Send + Sync`.
3. *The server sends a combined brightness channel.* Pros: client trivial. Cons: more channel traffic on every light change;
   server cost; loses the per-scale channels' own streaming.

**Recommendation (confidence: high on B, medium on 2).** B for fields; option 2 for light when the spike stops being a spike. Until
then the leak is recorded under "Open" as spike debt.
**Two people:** do the small version of B (typed handles and columns with load-time validation), because it also removes the
engine leak; skip tag merging, canonical hashing and schema generation.

### W4. Block behaviour is not data

**Today [code].** Cave names blocks through Rust fields (`content.blocks.lamp`, `fire_block`); the placeable blocks are a
six-entry array; about two dozen references to concrete blocks are in Cave's source. A JSON block could set colour, emission and
shape but could not take part in any rule that is not already written against those fields.

**Addressed by the five?** Partly. Revision 2 lets a rule read any property of any block; revision 5 adds checking. A genuinely new
behaviour still needs a rule plugin written in code.

**Options**

| Option | Pros | Cons |
|---|---|---|
| A. Rust only (today) | Simplest | Not moddable |
| B. Data selects behaviours: properties and tags read by rule plugins | Safe (no code runs); schema-checkable; fast | New kinds of behaviour need a plugin |
| C. Scripts attached to blocks (callbacks such as on-place) | Expressive | Needs a script runtime and a sandbox; per-event boundary cost |
| D. Graphs or visual scripting | Friendly to designers | Large to build; Hytale itself calls its version unpolished |

Precedents: Vintage Story has three tiers: theme packs, JSON content mods that create or change blocks, items, entities, recipes and
world generation, and C# code mods for complex behaviours and AI [web]. Hytale: JSON data assets in an editor, Java plugins, node
visual scripting (announced, unpolished, with acknowledged gaps for NPCs and world generation) [web]. Luanti: Lua callbacks and
groups [web/memory]. Cataclysm: JSON with `copy-from`, `extend`, `delete`, and relative or proportional numeric edits [web].
Luanti's own docs warn that functions not documented may change between versions [web], a reminder to keep our contract
small and labelled.

**Recommendation (confidence: high).** B for the first release: data selects, rule plugins read. Tags (bool properties) are the
cheap and conflict-free way for a rule to match sets, which Luanti's groups show. Revisit C only if a third party asks.
**Two people:** B falls out of revision 2 anyway. Do not build scripting.

### W5. Content capacity is capped

**Today [code, corrected in section 1].** 240 game materials (256-entry palette less 16 avatar colours) are shared by every block
and every state colour across all plugins (`material_of(block, state)` indexes blocks and then all state colours). Blocks are flat
colours; shapes are sub-voxel boxes on an 8 by 8 by 8 grid; there are no textures, audio or models.

**Addressed by the five?** No.

**Options**

| Option | Pros | Cons |
|---|---|---|
| A. Leave 240 | Zero work | A content mod hits it; a game with many states per block hits it |
| B. Palette in a storage buffer (millions of entries) | Small and local to the renderer; keeps the flat-colour style; no format change | Still flat colours |
| C. Texture array indexed by material and face, UVs from position in the shader | Real textures; matches the usual voxel pipeline | Large; affects greedy merging, assets, the package |
| D. Per-voxel colour | No palette | A different art style; bigger data |

Precedent: Teardown has an 8-bit palette of about 255 entries per voxel volume, which its author says made a spray-can shading
feature use eight empty slots per colour and left about 28 colours [web]; MagicaVoxel files have the same 256 limit [web]. The
limit is workable, and it shapes what features are possible.

**Recommendation (confidence: medium).** B when needed (it is small and reversible); decide C only when the art direction wants
textures. **Two people:** skip until content exceeds about 200 materials.

### W6. Modders write plumbing by hand

**Today [code].** Each game's `messages.rs` hand-writes encode and decode functions (332, 492 and 266 lines, tests included).
Commands are matched by name inside `command_received`, with a hard-coded "unknown command; try ..." line. Plugins cannot add
client commands or key actions; `status_text` puts text in the window title, which is the whole HUD; there is no UI.

**Addressed by the five?** Mostly: revision 4 (declared messages, command registry) and the client hooks in revision 1. No UI toolkit.

**Options: messages**

| Option | Pros | Cons |
|---|---|---|
| A. Hand-written codecs (today) | Compact; full control | Two functions to keep in sync per message; every plugin repeats it |
| B. A `Codec` trait with helpers and a generated round-trip test | Small; keeps our binary format | Still code per message |
| C. Declared field lists in the shared value vocabulary, encoded generically | One definition, both ends; non-Rust (WASM) plugins can use it; schema comes free | Slower and larger than compiled codecs; fine for plugin messages, not for brick edits |
| D. Schema files and code generation (Protobuf and similar) | Cross-language | New toolchain; heavy for us |

Minecraft's data components are written to disk and sent over the network with codecs [web: Fabric docs on data components]; Hytale's API
index has a `protocol.packets` package [web: package index; how it is made is not stated].

**Options: commands**

| Option | Pros | Cons |
|---|---|---|
| A. `match` on names (today) | Trivial | No help, no usage, no permissions |
| B. Registry: name, typed arguments, usage, handler, permission level | Generated help and errors; small | A little code |
| C. A Brigadier-style tree with literals, typed arguments and suggestions | Rich; tab completion | Much bigger |

Brigadier is a tree of literals and arguments with a `requires` predicate for permissions, registered through a callback in
Fabric [web]. Luanti's chat commands are a name, a params string and a function [memory].

**Options: client interface**

| Option | Pros | Cons |
|---|---|---|
| A. Title-bar text (today) | Zero | Not a HUD |
| B. Plugin-registered HUD lines, key actions and `.`-commands | Small; covers most needs | No windows or menus |
| C. Server-driven UI descriptions (Luanti-style forms) | Works without client mods; replay-safe if driven by the stream | A UI model to design |
| D. Shipped UI files (Hytale: custom `.ui` files in the plugin's assets [web, community guide]) | Rich | A UI toolkit to build |

**Recommendation (confidence: medium-high).** Messages: C for plugin messages and A for the engine's own; commands: B; client: B
now, C later if needed. **Two people:** the command registry is worth it (tiny); for messages do B only (a trait and a generated test);
skip C and D.

### W7. There is no entity or item model

**Today [code and design doc].** Only players exist, as poses. The design says creatures and structures are finite grids and the
"Open" list says which replicated entity state is common to all games is undecided. The protocol, replay stream and save format have
no entity messages and no `engine:entities` section. Players have no inventory; the placeable blocks are a hard-coded array.

**Addressed by the five?** No (the registrar could register entity kinds later; nothing is decided).

**Options**

| Option | Pros | Cons |
|---|---|---|
| A. None; mobs as voxel patterns (what Eels does with worms) | No work | Not enough for mobs, items or inventories |
| B. Server-owned entities: id, kind from a registry, pose, replicated typed components (the property machinery); items as registry content with stacks carrying components | Small; replay-friendly; reuses revision 2 | Must be designed carefully once; new protocol messages |
| C. A full ECS (Hytale's server, Veloren) | Scales to many entity kinds and behaviours | Heavy; most of our state is voxels; a second data model beside grids |
| D. Entities are grids with a transform (the design doc's principle) | Consistent with structures and creatures | Per-entity cost; needs sub-grids in the stream (planned for structures anyway) |

Precedents: Minecraft's item stacks carry typed, namespaced data components [web]. Vintage Story entities are JSON assets with
behaviours and an AI-task system that picks the highest-priority runnable task per slot [web]. Hytale's server is built on an ECS
[web].

**Recommendation (confidence: medium).** B with D's principle for large or articulated things; no ECS. The one decision worth
making before 1.0 is whether to reserve the protocol messages and the save section, because after release adding them needs a
protocol version and decoders for old recordings. **Two people:** worth doing once, minimally, when the first mob or item arrives.

### W8. Changing other mods' content is not planned

**Today.** Nothing: the mod plan has data-driven mods (stage 2) but no rule for two mods that touch the same block.

**Addressed by the five?** Partly: revision 5 reserves the slot and the property bag makes the unit of change ("set this property
of this block") obvious. The design is not made.

**Options**

| Option | Pros | Cons |
|---|---|---|
| A. Whole-definition override, last wins | Trivial | N mods touching one block need compatibility packs |
| B. Additive merge for sets and flags (tags `replace`-free, `extend`, `delete`) | Conflict-free where it applies | Does not cover numbers |
| C. Patch operations on definitions (RFC 6902-style: add, remove, replace, test, move, copy) | Precise; many mods can coexist | Order matters; failures can be silent or loud |
| D. A data stage with scripts over the merged table (Factorio) | Most powerful | Order conflicts; needs a script runtime |
| E. Code patching | Reaches anything | Fragile; not possible in a sandbox |

**Who does it, and how it has gone**

- Starbound `.patch` files use `op`, `path` and `value` with add, remove, replace, test, move and copy; modders argued in public that
  patches avoid multiplying compatibility patches, a failing patch raises an exception, and `sbtool` generates patches by diffing
  unpacked assets [web].
- Vintage Story's patching is based on RFC 6902, operates before parsing, and a bundled tool is recommended for writing patches;
  code patching exists for changes repeated across many objects and picks up newly added assets automatically [web].
- RimWorld: before alpha 17 modders could only overwrite whole defs, which broke when two mods touched one; XPath patch
  operations followed. Advice in threads: wrapping patches so they always succeed hides errors that help debugging [web].
- Factorio: mods change each other's prototypes in `data`, `data-updates` and `data-final-fixes`. Authors' threads ask others not
  to create items in the late stage and to use the earlier ones so every mod can see them, describe two mods clashing in the final
  stage, and optional dependencies being added only to force load order; a mod exists only to hold cross-mod compatibility fixes
  [web]. The model is what makes mods interoperate, and its cost is order.
- Cataclysm: `copy-from` with `extend`, `delete`, relative and proportional edits, for variants and tweaks [web].
- Minecraft data packs: whole files override, tags merge additively unless they say replace [memory].

**Recommendation (confidence: medium).** B plus C: tags merge by union; other changes are patch operations on definitions by
namespaced id, `set`, `add` and `remove` on properties plus JSON-Patch-style `test` for safety; mods apply in dependency order with
ties by id; a patch that does not apply is a loud error naming file and path, never silent. No scripted data stage in version 1.
**Two people:** skip until mods exist; if designed on properties it is about a page of code.

## 4. Which weaknesses the five revisions address

| | Weakness | 1 Registrar | 2 Properties | 3 Package | 4 Messages, commands | 5 Schema | Net |
|---|---|---|---|---|---|---|---|
| W1 | Lump, no composition | **yes** | | | | | Addressed |
| W2 | Client builds content | | | **data yes**, code later | | | Mostly |
| W3 | Engine knows game concepts | | **fields yes** | | | | Partly (scaled light not) |
| W4 | Behaviour is not data | | **data yes** | | | checks | Partly (new behaviour needs code) |
| W5 | Content capacity | | | | | | **No** |
| W6 | Plumbing by hand | client hooks | | | **messages, commands** | | Mostly (no UI toolkit) |
| W7 | No entities or items | | | | | | **No** |
| W8 | Modifying others' content | | unit of change | | | slot reserved | Partly (not designed) |

## 5. Incompatibilities and tensions

None of the five conflict outright. These are the places where two choices pull against each other, and how I would settle each.

1. **Per-voxel events versus "bulk calls only" across a WASM boundary.** A `BlockChanged` call per voxel would cost a boundary
   crossing each. Settled by making the event `BlocksChanged(&[Change])`, delivered once per tick per level.
2. **Handler order versus replay.** An order that depends on hash iteration, thread timing or registration timing breaks
   "the same stream gives the same world". Settled by one sequential order from `after`/`before` and plugin id. This rules out a
   Bevy-style parallel scheduler unless it is added later as an optimisation that must give identical results.
3. **Declared read and write channels versus free-form handlers.** Handlers can write anywhere, so declarations only mean something
   for systems; presenting them as a guarantee would be false comfort. Settled by making them a *test-time check* for both.
4. **Free-form tick systems versus the phased-tick rule** (snapshot in, edits out, partition-invariance test). A plugin that
   writes the world directly during a tick bypasses it. Settled by the lattice shape for rules and an "exclusive" mark for the rest.
5. **Content package versus client-side terrain generation.** Clients need generator code, which a data package cannot carry. So
   revision 3 is complete only after WASM generators (stages 4 and 5); until then generators stay compiled in. The alternative,
   server-streamed generators, gives up "terrain nobody has edited is never sent". The package also forces client code to find
   blocks by identifier on connecting: today Cave's client refers to concrete ids (the placeable array, `content.blocks.*`).
6. **Property bags versus determinism and hashing.** The package needs a canonical byte form (sorted keys, no float
   ambiguity), and rules must be bit-identical. Settled by integer-only kinds for anything a rule reads, floats only for
   client-only properties.
7. **Two schema vocabularies.** If messages use compiled codecs and properties use dynamic values, we maintain two type systems.
   Settled by one value-kind vocabulary for properties, plugin messages, command arguments and config, and compiled codecs only
   for the engine's own hot-path messages.
8. **Entities as ECS versus "creatures are grids" and the simple registrar.** Adopting an ECS later fights both. Settled by
   choosing B/D in W7: entities carry the same typed components as properties; no archetype storage.
9. **Client-side hooks versus "the client's view comes only from the server stream".** A hook that mutated client world state
   would break replay. Settled by hooks being pure functions of the stream plus local UI state (HUD lines, key actions, the light
   hook of W3).
10. **Patches versus preserving data of missing plugins.** Patches apply to definitions, which are never saved, so there is no
    clash with the saved-data rule; the policy for a patch to a property whose plugin is missing (ignore with a warning, or refuse)
    still needs choosing.

## 6. What is not worth the complexity for two people and fewer than 100 additions

*Decision, 2026-10-17: the owner wants every item below in the plan, including the skips, with the palette widening and the WASM and
native loaders last (Build plan phases 23 to 32). The verdicts stay as written: they are advice about size and order, for a team of
two, not a reason to leave something out.*

Assume only the owner and the assistant add features after 1.0, and fewer than 100 additions (blocks, rules, commands, message
kinds). Then the audience is us, the same binary runs both ends, and the cost of each mechanism is paid back only by how often we
use it.

| Improvement | Verdict | Why |
|---|---|---|
| 1. Registrar and events | **Worth it**, trimmed (about eight events, checks in tests, no scheduler) | The only one whose value grows with every addition: each new Cave rule today edits `CaveLogic` in about five places [code] |
| 2. Property bags | **Small version** (typed handles, columns, load-time validation) | Fixes the engine leak and the two-dozen hard-coded references; skip tags merging, canonical hashing, schema |
| 3. Content package at the handshake | **Skip** | Same binary on both ends; revisit when a server for others exists. Keep a version field in `Welcome` |
| 4a. Command registry | **Worth it** | Tiny; generated help; removes the hard-coded unknown-command line |
| 4b. Declared messages | **Skip** codegen and schema; do a `Codec` trait with a generated round-trip test | Perhaps 10 to 20 message kinds in total; hand codecs with a test are fine |
| 5. Generated schema | **Skip** unless we adopt JSON blocks; then it is cheap | Nothing to check without a data format |
| W7. Entities and items | **Worth it once**, minimal, when the first mob or item arrives; **skip** an ECS | Cave's growth needs them; a second data model does not pay |
| W5. Palette widening | **Skip** until about 200 materials | Cheap when needed, no format impact |
| W8. Patches | **Skip** | Needs mods |
| W3. Scaled-light hook | **Do** when the multiscale spike graduates, or remove the spike's leak | A stated rule broken; small |
| Data-driven JSON blocks | **Maybe** | Hot reload is nice; a Rust builder serves 100 blocks |
| WASM and native loaders (stages 4 to 8) | **Skip**: the largest item that might never pay off | Only matters with untrusted third parties |
| UI toolkit, server-driven UI | **Skip** | HUD lines and key actions cover most |

If modders do arrive, the order changes: 3, then 5 and the patch design, then WASM.

## 7. Suggested sequence and gates

1. **Phase 23** (registrar and events), tests first. **Gate:** the pinned digests, saved bytes and every existing Cave test pass
   unmodified; the checks of W1 are on.
2. **Phase 24** (properties). **Gate:** the same, plus a test that moving `emission` and `flammability` changed no behaviour.
3. **Phase 25** (messages and commands). **Gate:** the round-trip test is generated for every declared message.
4. **Phase 26** (client interface, light hook), then **Phase 27** (entities and items, with the protocol messages and the save
   section reserved before 1.0). Whether the first release ships textures stays open (see "Open").
5. **Phases 28 and 29** (content package; JSON packs, schema, patches), then **Phase 30** (server-driven UI). They need only
   definitions that serialize, so they do not have to wait for WASM.
6. Last: **Phase 31** (palette in a storage buffer) and **Phase 32** (the loaders).

## Sources

- Hytale: [How Does Hytale Modding Work?](https://wabbanode.com/blog/hytale/how-does-hytale-modding-work); [Hytale Server API 0.7.2 package index](https://docs.hytale.com/api/); [Events (community docs)](https://hytalemodding.dev/en/docs/server/events); [GUI System (community docs)](https://hytale-docs.pages.dev/gui/)
- Minecraft Bedrock: [Block components (Bedrock Wiki)](https://wiki.bedrock.dev/blocks/block-components); [minecraft:light_emission (Microsoft Learn)](https://learn.microsoft.com/zh-cn/minecraft/creator/reference/content/blockreference/examples/blockcomponents/minecraftblock_light_emission); [Blocks documentation 1.16.100.4](https://bedrock.dev/docs/1.16.0.0/1.16.100.4/Blocks)
- Minecraft Java: [Custom data components (Fabric docs)](https://docs.fabricmc.net/develop/items/custom-data-components); [NeoForged 1.20.5 primer](https://docs.neoforged.net/primer/docs/1.20.5); [NBT component migrator](https://www.minecraftmaps.com/tools/nbt-component-migrator); [Server resource packs](https://support.aternos.org/hc/en-us/articles/5013255309085-Adding-a-custom-resource-pack-to-your-server-Minecraft-Java-Edition); [Fabric commands](https://docs.fabricmc.net/develop/commands/basics); [Fabric events](https://docs.fabricmc.net/develop/events); [Fabric events tutorial](https://wiki.fabricmc.net/tutorial:events); [Forge events](https://docs.minecraftforge.net/en/1.13.x/events/intro/); [Forge forum thread on event order](https://forums.minecraftforge.net/topic/13505-solved-question-about-forge-events/); [a mixin conflict report](https://gitlab.com/antarcticgardens/create-new-age/-/issues/101)
- Luanti: [Timers and ABMs](https://rubenwardy.com/minetest_modding_book/en/map/timers.html); [Remote media](https://docs.luanti.org/server/remote-media); [Client Lua API](https://github.com/luanti-org/luanti/blob/master/doc/client_lua_api.md); [Content development](https://docs.luanti.org/content-dev)
- Factorio: [Cerys author on stages](https://mods.factorio.com/mod/Cerys-Moon-of-Fulgora/discussion/6791fdd0c91816df8d731b75); [Nullius author on stages](https://mods.factorio.com/mod/nullius/discussion/63b04fe49ffe09c607366ff5); [Krastorio 2 discussion](https://mods.factorio.com/mod/Krastorio2/discussion/5e9efab1047ff6000e512892); [Patchwork compatibility fixes](https://mods.factorio.com/mod/patchwork-compat-fixes); [Joining and mod sync (hosting guide)](https://supercraft.host/wiki/factorio/howtojoin)
- Vintage Story: [JSON patching](https://wiki.vintagestory.at/index.php/Modding:JSON_Patching/en); [Mod types](https://wiki.vintagestory.at/index.php/Modding:Mod_Types); [Asset patching in code](https://wiki.vintagestory.at/index.php/Modding:Code_Patching); [Entity behaviors](https://wiki.vintagestory.at/Modding:Entity_Behaviors); [Task AI](https://wiki.vintagestory.at/index.php/Modding:Entity_Behavior_taskai)
- RimWorld: [Patch operations](https://rimworldwiki.com/wiki/Modding_Tutorials/PatchOperations); [Compatibility](https://rimworldwiki.com/wiki/Modding_Tutorials/Compatibility); [XPath forum thread](https://ludeon.com/forums/index.php?msg=401378)
- Starbound: [JSON merging thread](https://community.playstarbound.com/threads/june-25-changing-json-merging.81014/page-5); [Advanced modding](https://www.starbounder.org/Modding:Advanced_Modding); [sbtool](https://www.npmjs.com/package/sbtool)
- Cataclysm: DDA: [JSON inheritance](https://docs.cataclysmdda.org/JSON/JSON_INHERITANCE.html)
- Veloren: [Writing a plugin](https://book.veloren.net/contributors/modders/writing-a-plugin.html); [This Week in Veloren 70](https://veloren.net/devblog-70)
- Bevy: [ambiguity detection issue](https://github.com/bevyengine/bevy/issues/14524)
- Teardown and MagicaVoxel: [Spray can (Teardown developer blog)](https://blog.voxagon.se/2020/12/03/spraycan.html); [.vox format](https://paulbourke.net/dataformats/vox/)
